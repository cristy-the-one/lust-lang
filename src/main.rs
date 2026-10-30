// lust-core: the lust compiler (see DESIGN.md). Usually started by the `lust` client.
//
//   lust-core check|build|run <file.rs> ...   compile cold, in this process
//   lust-core daemon                          serve warm, incremental compiles for this directory
//   lust-core test <file.rs> ...              run #[test] functions, one child process each
//   lust-core explain <L####>                 long-form help for one diagnostic code
//   lust-core expand <file.rs>                print the file's expanded token stream
//   lust-core capabilities [--json]           what the supported subset contains
/// AST v2 keeps shapes for syntax whose parsers land later (docs/ARCH.md S3b),
/// so many nodes are defined before anything reads them.
#[allow(dead_code)]
mod ast;
mod borrowck;
mod capabilities;
mod cc;
mod cgen;
mod check;
mod colwidth;
mod daemon;
mod def;
mod diag;
mod driver;
mod expand;
mod explain;
mod image;
mod intern;
mod ir;
mod lexer;
mod lower;
#[allow(dead_code)] // Instance discovery is called from collect and the driver as W-TRAITS-CORE lands in increments.
mod mono;
mod parser;
mod pool;
mod protocol;
#[allow(dead_code)] // Wired into the driver by the worklist increment.
mod resolve;
mod scan;
mod stdload;
mod stdloc;
mod stdmods;
mod tcc;
mod testrun;
#[allow(dead_code)] // See `mod mono`.
mod traits;
mod ty;

use protocol::Invocation;

#[global_allocator]
static GLOBAL: image::Alloc = image::Alloc;

/// Deletes a one-shot session's object directory. A serial walk costs 3 to 6 ms on a cold
/// build (one unlink per object and C file), so many files are unlinked in parallel on the
/// thread pool. The final remove_dir_all handles subdirectories and anything the parallel
/// pass missed.
fn remove_obj_dir(dir: &std::path::Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        let files: Vec<std::path::PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file()).collect();
        if files.len() > 16 {
            pool::par_map(&files, |f| {
                let _ = std::fs::remove_file(f);
            });
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Raises the main thread's stack limit toward `pool::STACK_SIZE` (best
/// effort, capped by the hard limit), so the thread the compiler runs
/// on holds deep expressions the parser accepts. The main thread keeps
/// running the compile because an in-memory `lust run` forks, and a
/// fork is only sound from a lone thread (see src/tcc.rs). On Linux the
/// main stack grows on demand up to the soft limit at fault time, so
/// raising it here, before any deep work, is enough.
#[cfg(unix)]
fn raise_stack_limit() {
    #[repr(C)]
    struct Rlimit {
        cur: u64,
        max: u64,
    }
    unsafe extern "C" {
        fn getrlimit(resource: i32, rlim: *mut Rlimit) -> i32;
        fn setrlimit(resource: i32, rlim: *const Rlimit) -> i32;
    }
    const RLIMIT_STACK: i32 = 3;
    let mut rl = Rlimit { cur: 0, max: 0 };
    // SAFETY: `rl` is a valid out pointer for the call.
    if unsafe { getrlimit(RLIMIT_STACK, &mut rl) } != 0 {
        return;
    }
    let want = (pool::STACK_SIZE as u64).min(rl.max).max(rl.cur);
    if want > rl.cur {
        rl.cur = want;
        // SAFETY: `rl` names a limit no higher than the hard limit read above.
        unsafe { setrlimit(RLIMIT_STACK, &rl) };
    }
}

/// A request this process parsed itself: this process is its client, so its env is the
/// request's, and the request reads it (`env: None`, `expand::builtin::compile_env`) instead of
/// a copy of the whole env, which a front image save would write into the image (it writes
/// every live heap block), secrets and all. A daemon request still carries its client's env.
fn own_env(req: &mut driver::Request) {
    req.client.env = None;
}

/// The root file size from which a one-shot compile keeps transparent
/// huge pages in its serial phases (see `main`). On boat, realish40
/// (940 KB) and plain1600 (1.2 MB) gain from them there, realish20
/// (470 KB) is even, and fmt40 (54 KB) and a trivial program lose.
const BIG_FILE: u64 = 256 << 10;

/// LUST_FRONT_CENSUS=1 (a probe of what a front image holds): after the restore, drops each
/// owner of each session and the process tables in turn and prints the restored bytes each
/// drop freed (`image::restored_freed`), largest first; the caller exits with no build and no save.
fn census(mut sessions: Vec<Box<driver::Session>>) {
    use std::mem::take;
    let mut rows: Vec<(String, usize)> = Vec::new();
    let mut row = |rows: &mut Vec<(String, usize)>, name: &str, f: &mut dyn FnMut()| {
        let a = image::restored_freed();
        f();
        rows.push((name.to_string(), image::restored_freed() - a));
    };
    for s in sessions.iter_mut() {
        let rc = &mut s.front.resolve_cache;
        let (files, same, toks) = rc.same_streams();
        eprintln!("census: resolve_cache: {same} of {files} files expand to their raw tokens ({toks} tokens)");
        for (part, name) in ["raw tokens", "outlines", "expanded tokens", "final chunks", "std asks", "paths, seed, expander"].iter().enumerate() {
            row(&mut rows, &format!("resolve_cache {name}"), &mut || rc.drop_part(part));
        }
        if let Some(pb) = s.front.patch.as_mut() {
            for (part, name) in [(1, "file ranges, extras"), (2, "item scans"), (3, "dropped"), (4, "lists")] {
                row(&mut rows, &format!("patch {name}"), &mut || pb.drop_part(part));
            }
        }
        let c = &mut s.caches;
        for (part, name) in [(8, "discovery reads"), (9, "discovery outputs"), (10, "read keys"), (0, "templates"), (1, "closure templates"), (2, "discovery rest"), (3, "unstored"), (4, "held"), (5, "memo"), (6, "outputs"), (7, "readers index")] {
            row(&mut rows, &format!("gen_reuse {name}"), &mut || c.gen_reuse.drop_part(part));
        }
        row(&mut rows, "caches.out code", &mut || c.out.iter_mut().for_each(|o| drop(take(&mut o.text))));
        row(&mut rows, "caches.out", &mut || drop(take(&mut c.out)));
        row(&mut rows, "caches.slot_reads", &mut || drop(take(&mut c.slot_reads)));
        row(&mut rows, "caches.read_keys", &mut || drop(take(&mut c.read_keys)));
        row(&mut rows, "caches.unit_reads", &mut || drop(take(&mut c.unit_reads)));
        row(&mut rows, "caches.derive_tables", &mut || drop(take(&mut c.derive_tables)));
        row(&mut rows, "caches.slot_ir", &mut || drop(take(&mut c.slot_ir)));
        row(&mut rows, "caches.glue", &mut || drop(take(&mut c.glue)));
        row(&mut rows, "caches.unit_records", &mut || drop(take(&mut c.unit_records)));
        row(&mut rows, "caches.gen_reuse", &mut || drop(take(&mut c.gen_reuse)));
        row(&mut rows, "caches.inst_ids", &mut || drop(take(&mut c.inst_ids)));
        let f = &mut s.front;
        for i in 0..f.kept.len() {
            let Some(kp) = f.kept[i].as_mut() else { continue };
            let p = &mut kp.program;
            row(&mut rows, &format!("kept{i}.fns"), &mut || drop(take(&mut p.fns)));
            row(&mut rows, &format!("kept{i}.adts"), &mut || drop(take(&mut p.adts)));
            row(&mut rows, &format!("kept{i}.const_values"), &mut || drop(take(&mut p.const_values)));
            row(&mut rows, &format!("kept{i}.body_items"), &mut || drop(take(&mut p.body_items)));
            row(&mut rows, &format!("kept{i}.ast"), &mut || drop(take(&mut p.ast)));
        }
        row(&mut rows, "kept.rest", &mut || f.kept = Default::default());
        if let Some(t) = f.patch.as_mut().and_then(|pb| pb.tables_mut()) {
            row(&mut rows, "tables maps", &mut || drop(take(&mut t.maps)));
            row(&mut rows, "tables glob log", &mut || drop(take(&mut t.glob_log)));
            row(&mut rows, "tables def info", &mut || drop(take(&mut t.def_info)));
            row(&mut rows, "tables flat", &mut || drop(take(&mut t.flat)));
            row(&mut rows, "tables pending single", &mut || drop(take(&mut t.pending_single)));
            row(&mut rows, "tables modules", &mut || drop(take(&mut t.modules)));
            row(&mut rows, "tables vis", &mut || {
                drop(take(&mut t.assoc_vis));
                drop(take(&mut t.field_vis));
                drop(take(&mut t.tuple_field_vis));
            });
        }
        row(&mut rows, "front.patch (tables, rest)", &mut || drop(f.patch.take()));
        row(&mut rows, "front.resolve_cache", &mut || drop(take(&mut f.resolve_cache)));
        row(&mut rows, "front.mdr", &mut || drop(take(&mut f.mdr)));
        row(&mut rows, "front.chunks+src", &mut || {
            drop(take(&mut f.chunks));
            drop(take(&mut f.src));
        });
        row(&mut rows, "front.ast", &mut || drop(take(&mut f.ast)));
        row(&mut rows, "front.map", &mut || drop(take(&mut f.map)));
        row(&mut rows, "front.slot tables", &mut || {
            drop(take(&mut f.fn_slot));
            drop(take(&mut f.slot_hash));
            drop(take(&mut f.slot_env));
            drop(take(&mut f.slot_head));
        });
        row(&mut rows, "std_cache", &mut || drop(s.std_cache.take()));
    }
    row(&mut rows, "the rest of the sessions", &mut || drop(take(&mut sessions)));
    row(&mut rows, "diag global tables", &mut || drop(diag::take_global_tables()));
    row(&mut rows, "cargo graphs", &mut || drop(resolve::cargo::take_graph_cache()));
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let total: usize = rows.iter().map(|r| r.1).sum();
    eprintln!("census: {} MB of restored blocks freed", total >> 20);
    for (name, b) in rows {
        eprintln!("census: {name:<28} {:>8.1} MB", b as f64 / 1048576.0);
    }
}

/// Builds `reqs` in order with a front image (`LUST_FRONT_STATE`,
/// src/image.rs). The image holds one session per crate selection, as a
/// daemon keeps them: a request takes its own session from the image, or
/// a new persistent one. A release-driver run (cc requests) stops at the
/// first failed target. Then the save, here or (`LUST_FRONT_SAVE=background`)
/// in a forked child after this process answers. Returns the answers; a compile
/// panic discards the image (an oracle failure answers code 3, as in a
/// daemon; any other panic resumes).
fn image_run(reqs: &[driver::Request], dir: &std::path::Path) -> Vec<driver::Response> {
    let t = std::time::Instant::now();
    // Compact mode (`LUST_FRONT_COMPACT=1`, src/image.rs): a background save found the image
    // too scattered and started this run of the same command (`compact_exec`), or a job of its
    // own runs it. It rebuilds the image cold: it loads nothing, makes no artifact (every cc
    // step is parked and dropped), and writes the new image as a base.
    let compact = image::compact_mode();
    if compact && !reqs.iter().all(|r| r.cc) {
        image::refuse_compaction(dir, "a compaction of a run that is not all release driver builds");
        return Vec::new();
    }
    if compact {
        if !image::compact_lock(dir) {
            image::refuse_compaction(dir, "no state lock");
            return Vec::new();
        }
    } else {
        image::lock(dir);
    }
    image::allow_compaction(!compact && reqs.iter().all(|r| r.cc));
    if compact {
        image::wait_for_build();
    }
    // The crate's source files are read beside the load (`resolve::prefetch`).
    if !compact {
        resolve::prefetch::start(dir);
    }
    let loaded = if compact { Err("compacting the image".to_string()) } else { image::preloaded().unwrap_or_else(|| image::load(dir)) };
    let (mut sessions, loaded) = match loaded {
        Ok(roots) => {
            let image::Roots { mut sessions, tables, stamps, graphs } = *roots;
            diag::put_global_tables(tables);
            resolve::cargo::put_graph_cache(graphs);
            stamps.restore();
            sessions.iter_mut().for_each(|s| s.after_restore());
            (sessions, Ok(()))
        }
        Err(why) => (Vec::new(), Err(why)),
    };
    let load_ms = t.elapsed().as_secs_f64() * 1e3;
    if std::env::var_os("LUST_FRONT_CENSUS").is_some_and(|v| v == "1") && loaded.is_ok() {
        census(std::mem::take(&mut sessions));
        std::process::exit(0);
    }
    // `--timings` or LUST_FRONT_STATE_VERBOSE=1 report each load and the
    // save; a failed save is always reported.
    let verbose = reqs.iter().any(|r| r.timings) || std::env::var_os("LUST_FRONT_STATE_VERBOSE").is_some_and(|v| v != "0");
    let mut resps = Vec::new();
    let background = std::env::var_os(image::SAVE_ENV).is_some_and(|v| v == "background");
    for (k, req) in reqs.iter().enumerate() {
        let found = sessions.iter().position(|s| s.key == req.session);
        let how = match (&loaded, found) {
            (Ok(()), Some(_)) => format!("restored generation {} ({})", image::generation(), image::layers_note()),
            (Ok(()), None) => "cold (the image has no session for this crate selection)".to_string(),
            (Err(why), _) => format!("cold ({why})"),
        };
        let i = found.unwrap_or_else(|| {
            let (a, _) = driver::cache::hash128(&[req.session.describe().as_bytes()], 0);
            sessions.push(Box::new(driver::Session::new(dir.join(format!("obj-{a:016x}")), true, req.session.clone())));
            sessions.len() - 1
        });
        if req.cc {
            let triple = if req.session.target_triple.is_empty() { protocol::host_triple() } else { &req.session.target_triple };
            eprintln!("lust: release build for {triple} -> {}", req.out.display());
        }
        let session = &mut sessions[i];
        session.caches.image_one_shot = true;
        // The last release build parks its cc step (`driver::defer_cc`), so the background
        // save below forks first and writes the image while clang runs.
        driver::defer_cc(if compact { req.cc } else { background && req.cc && k + 1 == reqs.len() });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver::compile(req, session)));
        driver::defer_cc(false);
        if compact {
            driver::drop_parked_cc();
        }
        match result {
            Ok(mut resp) => {
                if verbose {
                    use std::fmt::Write as _;
                    let _ = writeln!(resp.stderr, "lust: front state {how} in {load_ms:.0} ms");
                    if let Some(n) = image::touched_note() {
                        let _ = writeln!(resp.stderr, "lust: front state {n}");
                    }
                }
                let stop = req.cc && resp.code != 0;
                resps.push(resp);
                if stop {
                    break;
                }
            }
            Err(payload) => {
                // The next build must not start from a session this one broke.
                image::discard(dir);
                if let Some(line) = driver::oracle_failure(&*payload) {
                    resps.push(driver::Response { stderr: format!("error: internal: {line}\n"), code: 3, ..Default::default() });
                    return resps;
                }
                std::panic::resume_unwind(payload);
            }
        }
    }
    if compact && resps.iter().any(|r| r.code != 0) {
        image::refuse_compaction(dir, "the cold rebuild failed");
        let _ = std::fs::write(dir.join("save.log"), "lust: front state not compacted: the cold rebuild failed\n");
        return resps;
    }
    sessions.iter_mut().for_each(|s| s.settle());
    // A prefetch no resolve took ends here, so its texts stay out of the image; the list is
    // the next build's.
    drop(resolve::prefetch::take());
    resolve::prefetch::write_list(dir, sessions.iter().flat_map(|s| s.front.resolve_cache.source_paths()));
    // The kept programs ride only when the next build may reuse them
    // (`Front::image_keeps_kept`). A background save drops the others in its
    // forked child (`image::save_background`), off this build's way to its cc step.
    if compact || !background {
        driver::drop_unkept(&mut sessions);
    }
    let roots = Box::new(image::Roots {
        sessions,
        tables: diag::take_global_tables(),
        stamps: driver::Stamps::now(),
        graphs: resolve::cargo::take_graph_cache(),
    });
    if compact {
        let line = match image::save(dir, roots) {
            Ok(line) => format!("{} (compacted: cold rebuild and save in {:.0} ms)\n", line, t.elapsed().as_secs_f64() * 1e3),
            Err(e) => {
                image::refuse_compaction(dir, &e);
                format!("lust: front state not compacted: {e}\n")
            }
        };
        let _ = std::fs::write(dir.join("save.log"), &line);
        if let Some(last) = resps.last_mut() {
            last.stderr.push_str(&line);
        }
        return resps;
    }
    let saved = if background { image::save_background(dir, roots) } else { image::save(dir, roots) };
    let line = match saved {
        Ok(line) if verbose => line + "\n",
        Ok(_) => String::new(),
        Err(e) => format!("lust: front state not saved: {e}\n"),
    };
    if let Some(last) = resps.last_mut() {
        last.stderr.push_str(&line);
    }
    // The parked cc step: it runs now, while the forked child writes the image.
    if let Some((code, out)) = driver::parked_cc_result()
        && let Some(last) = resps.last_mut()
    {
        last.stderr.push_str(&out);
        if code != 0 {
            last.code = code;
        }
    }
    resps
}

fn main() {
    // A re-executed image process (src/image.rs) runs with randomization
    // off and an arena variable: its children get neither.
    image::process_start();
    // Cargo runs lust-core as its RUSTC_WRAPPER during the feature probe.
    if let Some(code) = resolve::probe::wrapper_main() {
        std::process::exit(code);
    }
    #[cfg(unix)]
    raise_stack_limit();
    tcc::use_mimalloc();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Agent-kit commands run in this process; they never go through the daemon.
    match args.first().map(String::as_str) {
        Some("test") => std::process::exit(testrun::run(&args[1..])),
        Some("explain") => std::process::exit(explain::run(&args[1..])),
        Some("capabilities") => std::process::exit(capabilities::run(&args[1..])),
        Some("expand") => std::process::exit(expand::run_expand(&args[1..])),
        _ => {}
    }
    // A CI job may check its sources out while lust restores its front image
    // (`LUST_SOURCES_READY`, resolve/prefetch.rs). The request reads the manifest, so an image
    // build goes into its image process at once, starts the load there, and only then waits
    // for the sources.
    if protocol::waits_for_sources(&args) {
        if let Some(dir) = image::state_dir().filter(|_| !image::compact_mode()) {
            if image::arena_on() {
                image::lock(&dir);
                resolve::prefetch::start(&dir);
                image::preload(&dir);
            } else {
                // Returns only when it cannot; the request's own path says why.
                let _ = image::reexec(&dir);
            }
        }
        resolve::prefetch::wait_ready();
    }
    match protocol::parse_args(&args) {
        Some(Invocation::Daemon) => daemon::serve(&protocol::socket_path()),
        // The `lust` client handles these itself; direct `lust-core` calls work too.
        Some(Invocation::Release { file, out, profile, debug, net }) => {
            protocol::run_release(&file, out.as_deref(), profile.as_deref(), debug, net)
        }
        // The release driver: one cold compile per target, in this process,
        // in order. A failed target stops the run with its code.
        Some(Invocation::ReleaseCc { reqs }) => {
            let mut reqs = reqs.unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(2)
            });
            reqs.iter_mut().for_each(own_env);
            // With a front image, every target's session comes from it
            // (in the re-executed image process).
            if let Some(dir) = image::state_dir() {
                if image::arena_on() {
                    let resps = image_run(&reqs, &dir);
                    resps.iter().for_each(|r| eprint!("{}", r.stderr));
                    image::exit_fast(resps.iter().map(|r| r.code).find(|&c| c != 0).unwrap_or(0));
                }
                let why = image::reexec(&dir);
                // A compacting process (src/image.rs `compact_exec`) builds nothing without the image.
                if image::compact_mode() {
                    image::refuse_compaction(&dir, &why);
                    std::process::exit(0);
                }
                eprintln!("lust: front state off: {why}");
            }
            let mut code = 0;
            for req in &reqs {
                let triple = if req.session.target_triple.is_empty() { protocol::host_triple() } else { &req.session.target_triple };
                eprintln!("lust: release build for {triple} -> {}", req.out.display());
                let dir = std::env::temp_dir().join(format!("lust-{}-cc", std::process::id()));
                let mut session = driver::Session::new(dir, false, req.session.clone());
                session.caches.leak_at_exit = true;
                let resp = driver::compile(req, &mut session);
                remove_obj_dir(session.obj_dir());
                eprint!("{}", resp.stderr);
                if resp.code != 0 {
                    code = resp.code;
                    break;
                }
                std::mem::forget(session);
            }
            std::process::exit(code);
        }
        Some(Invocation::RustcCheck { file }) => protocol::run_rustc_check(&file),
        Some(Invocation::Compile { mut req, run, args }) => {
            own_env(&mut req);
            // Whole-request disk cache (cold runs only; the daemon path keeps its own
            // in-memory state). `--emit-c` writes side files, so it always compiles. A `run`
            // request is also never cached: its response can be the program's own output and
            // exit code (an in-memory run writes no executable), not a build result.
            // A front image build (src/image.rs): re-execute into an image
            // process, or run as one.
            if let Some(dir) = image::state_dir() {
                if image::arena_on() {
                    let resp = image_run(std::slice::from_ref(&req), &dir).pop().expect("one answer per request");
                    protocol::finish(resp, run, &req.out, &args);
                }
                let why = image::reexec(&dir);
                if image::compact_mode() {
                    image::refuse_compaction(&dir, &why);
                    std::process::exit(0);
                }
                eprintln!("lust: front state off: {why}");
            }
            let start = std::time::Instant::now();
            let cache = if req.emit_c.is_none() && req.run_args.is_none() && !req.cc { driver::cache::Cache::open(&req.file) } else { None };
            let src = cache.as_ref().and_then(|_| std::fs::read(&req.file).ok());
            if let (Some(cache), Some(src)) = (&cache, &src)
                && let Some(resp) = cache.lookup(&req, src, start)
            {
                protocol::finish(resp, run, &req.out, &args);
            }
            // Transparent huge pages cost a 2 MB zeroing per first touch. Pool
            // threads touch memory sparsely, so parallel phases always run
            // without them. A big file's serial phases (collect over every
            // item) touch memory densely and gain from them; a small program
            // touches little and loses, so it runs without them throughout,
            // with them on again before `finish` runs the program.
            let small = std::fs::metadata(&req.file).map_or(true, |m| m.len() < BIG_FILE);
            if small {
                tcc::set_huge_pages(false);
            } else {
                pool::small_pages_in_parallel();
            }
            let dir = std::env::temp_dir().join(format!("lust-{}", std::process::id()));
            let mut session = driver::Session::new(dir, false, req.session.clone());
            session.caches.leak_at_exit = true;
            // A panicking compile must not leave its temp dir behind:
            // clean up, then let the panic run its normal course.
            let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver::compile(&req, &mut session)));
            let resp = match compiled {
                Ok(resp) => resp,
                Err(payload) => {
                    remove_obj_dir(session.obj_dir());
                    std::panic::resume_unwind(payload);
                }
            };
            remove_obj_dir(session.obj_dir());
            if small {
                tcc::set_huge_pages(true);
            }
            if let (Some(cache), Some(src)) = (&cache, &src) {
                cache.store(&req, src, &resp);
            }
            // This process exits inside `finish`. Leak the session instead of freeing it:
            // dropping millions of small allocations (many made on worker threads) costs
            // about 10 ms on a 31k-line crate, and the OS reclaims the memory for free.
            // The daemon path never reaches this code, so it still frees normally.
            std::mem::forget(session);
            protocol::finish(resp, run, &req.out, &args);
        }
        None => {
            eprintln!("{}", protocol::USAGE);
            std::process::exit(2);
        }
    }
}
