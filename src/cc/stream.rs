// A release build's cc step, started while its units assemble (`cc::Stream`): the assembly
// hands over each plain C unit as soon as its text exists (`add`). The stream keys it as
// `cc::build` does (`unit_key`, the same inputs), takes a cache hit at once, and starts a
// miss's compile as a child process, the exact command `build` runs (`unit_command`), largest
// unit first. `cc::build_with` then takes each unit the stream has (`finish`): its key and file
// name, and its object once its child exited; it never compiles or scans that unit again, and
// it alone stores objects in the cache, after their compile succeeded.
//
// Two phases. While the units assemble, at most `early` children run (the assembly keeps the
// cores busy), and every `add` reaps finished children and starts waiting ones: the stream has
// no thread then, because an image save forks right after the assembly
// (`image::save_background`) and must find no other thread, and clang children are processes
// of their own. From `go` (the cc step, after that fork) a thread of its own keeps `jobs`
// children running until every unit started and exited. A forked process drops the stream as
// memory only: only the process that started the children waits for or kills them.
//
// With a profile to use (LUST_PGO_USE) a unit is named and keyed as `build` does it
// (`pgo::stable_name`, its own records from the warm profile, `pgo::Warm`), and compiles in its
// own dir (`pgo::STATIC_FILE`); a profile not yet warm in the work dir, an instrumented build
// (LUST_PGO_GEN) and the LLVM backend take `build` alone. LUST_CC_STREAM=0 turns it off;
// LUST_CC_STREAM_JOBS and LUST_CC_STREAM_EARLY set `jobs` and `early`.
use super::CcSpec;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

pub struct Stream(Arc<Inner>);

struct Inner {
    spec: CcSpec,
    debug: bool,
    flags: Vec<String>,
    cache: crate::driver::cache::Cache,
    work: PathBuf,
    /// The profile to use, with its index (LUST_PGO_USE).
    warm: Option<super::pgo::Warm>,
    jobs: usize,
    early: usize,
    /// Set by `go`: `jobs` children may run, and the stream's thread runs them.
    going: AtomicBool,
    owner: u32,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    units: HashMap<String, Unit>,
    /// Units with a running child.
    running: Vec<String>,
    /// Units not started yet: (text length, name).
    waiting: Vec<(usize, String)>,
}

struct Unit {
    key: String,
    /// The file name it compiles as: the unit's name, or with a profile its text's
    /// (`pgo::stable_name`). Its object is `work/<file>.o`.
    file: String,
    /// With a profile, the unit's own flags (`pgo::Warm::unit_flags`).
    pgo: Option<Vec<String>>,
    run: Run,
}

enum Run {
    /// The cache had its object; it is linked at `work/<name>.o`.
    Hit,
    Waiting,
    Running(Child),
    Exited(ExitStatus),
    Failed(String),
    /// `finish` took it.
    Taken,
}

impl Stream {
    /// A stream for a release C build for `triple`, or None: off (LUST_CC_STREAM=0), a profile
    /// build, no cache, or no compiler for the target.
    pub fn start(triple: &str, debug: bool, cache: Option<crate::driver::cache::Cache>, work: PathBuf) -> Option<Stream> {
        if std::env::var_os("LUST_CC_STREAM").is_some_and(|v| v == "0") || std::env::var_os("LUST_PGO_GEN").is_some() {
            return None;
        }
        let spec = super::spec_for(triple).ok()?;
        let cache = cache?;
        std::fs::create_dir_all(&work).ok()?;
        // With a profile, units compile from inside `work`, resolved as `build` resolves it.
        let (work, warm) = match std::env::var_os("LUST_PGO_USE") {
            Some(_) => {
                let work = std::fs::canonicalize(&work).ok()?;
                let warm = super::pgo::Warm::load(&spec, &work).ok().flatten()?;
                (work, Some(warm))
            }
            None => (work, None),
        };
        let num = |v: &str, d: usize| std::env::var(v).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let jobs = num("LUST_CC_STREAM_JOBS", crate::pool::threads().max(1));
        let early = num("LUST_CC_STREAM_EARLY", (jobs / 4).max(1));
        let mut flags: Vec<String> = super::cflags(debug).split_whitespace().map(str::to_string).collect();
        super::drop_zig_darwin_lto(&spec, &mut flags);
        let owner = std::process::id();
        Some(Stream(Arc::new(Inner {
            spec,
            debug,
            flags,
            cache,
            work,
            warm,
            jobs,
            early,
            going: AtomicBool::new(false),
            owner,
            state: Mutex::default(),
        })))
    }

    /// Hands over unit `name` (`u<N>`) with its C text: a cache hit is linked now, a miss
    /// written to `work/<name>.c` and compiled now or when a job is free. A unit whose key
    /// reads its preprocessed text (`#include <`) stays with `build`.
    pub fn add(&self, name: &str, text: &str) {
        let s = &self.0;
        if text.is_empty() || text.contains("#include <") {
            return;
        }
        // With a profile: the file name, own records and key part `build` gives this text.
        let (file, part, pgo) = match &s.warm {
            Some(w) => {
                let (exts, statics) = super::pgo::definitions_of(text);
                let file = super::pgo::stable_name(&exts).unwrap_or_else(|| name.to_string());
                let own = w.own(&format!("{file}.c"), &exts, &statics);
                let fnless = super::pgo::fnless(&(exts, statics));
                (file, Some(w.key_part(&own, fnless)), Some(w.unit_flags(&own, fnless)))
            }
            None => (name.to_string(), None, None),
        };
        let key = super::unit_key(text, None, &s.spec, s.debug, part.as_deref(), false);
        let obj = s.work.join(format!("{file}.o"));
        let run = if s.cache.unit_object(&key, &obj) {
            Run::Hit
        } else {
            // The object may be a hard link to a cache entry: a compiler that wrote its
            // output in place would change the entry.
            let _ = std::fs::remove_file(&obj);
            let src = s.src(&file, pgo.is_some());
            match src.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&src, text)) {
                Ok(()) => Run::Waiting,
                Err(e) => Run::Failed(format!("cannot write {}: {e}\n", src.display())),
            }
        };
        let mut st = s.lock();
        if matches!(run, Run::Waiting) {
            st.waiting.push((text.len(), name.to_string()));
        }
        st.units.insert(name.to_string(), Unit { key, file, pgo, run });
        s.pump(&mut st);
    }

    /// The cc step began (after any image save's fork): from now on a thread of the stream's
    /// own keeps `jobs` children running until every unit it has started and exited.
    pub(super) fn go(&self) {
        let s = Arc::clone(&self.0);
        s.going.store(true, Ordering::Relaxed);
        std::thread::spawn(move || {
            loop {
                {
                    let mut st = s.lock();
                    s.pump(&mut st);
                    if st.waiting.is_empty() && st.running.is_empty() {
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
    }

    /// The profile the stream loaded (`pgo::Warm`), for `build`'s own load.
    pub(super) fn warm(&self) -> Option<&super::pgo::Warm> {
        self.0.warm.as_ref()
    }

    /// The key of `name` and the file name it compiles as, when the stream has it.
    pub(super) fn key_and_file(&self, name: &str) -> Option<(String, String)> {
        self.0.lock().units.get(name).map(|u| (u.key.clone(), u.file.clone()))
    }

    /// Waits for the object of `name` (after `go`): None when the stream does not have it,
    /// Some(Ok(true)) for a cache hit, Some(Ok(false)) for a compile that succeeded (its object
    /// is at `work/<name>.o`), else the compiler's error.
    pub(super) fn finish(&self, name: &str) -> Option<Result<bool, String>> {
        let s = &self.0;
        let status = loop {
            {
                let mut st = s.lock();
                let u = st.units.get_mut(name)?;
                match &u.run {
                    Run::Waiting | Run::Running(_) => {}
                    _ => match std::mem::replace(&mut u.run, Run::Taken) {
                        Run::Hit => return Some(Ok(true)),
                        Run::Exited(x) => break x,
                        Run::Failed(e) => return Some(Err(e)),
                        _ => return None,
                    },
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        Some(match status.success() {
            true => Ok(false),
            false => Err(format!("--- unit {name} ({status})\n{}", std::fs::read_to_string(s.err_file(name)).unwrap_or_default())),
        })
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn err_file(&self, name: &str) -> PathBuf {
        self.work.join(format!("{name}.err"))
    }

    /// Where a unit compiled as `file` has its text: `work/<file>.c`, or with a profile
    /// `work/<file>.d/lust.c` (`pgo::STATIC_FILE`).
    fn src(&self, file: &str, pgo: bool) -> PathBuf {
        if pgo { self.work.join(format!("{file}.d")).join(super::pgo::STATIC_FILE) } else { self.work.join(format!("{file}.c")) }
    }

    /// Starts the compile of unit `name`: `build`'s command, its stderr to `work/<name>.err`.
    fn launch(&self, name: &str, u: &Unit) -> Result<Child, String> {
        let (obj, src) = (self.work.join(format!("{}.o", u.file)), self.src(&u.file, u.pgo.is_some()));
        let err =
            std::fs::File::create(self.err_file(name)).map_err(|e| format!("cannot write {}: {e}\n", self.err_file(name).display()))?;
        let dir = src.parent().map(PathBuf::from).unwrap_or_default();
        // A streamed unit includes no system header (`add` keeps those for `build`).
        let words = super::unit_words(&self.spec, self.debug, false);
        super::unit_command(&words, &self.flags, &obj, Some(&u.key), u.pgo.clone().map(|f| (f, dir.as_path())), &src)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(err)
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}\n", words.join(" ")))
    }

    /// Reaps finished children and starts waiting units, largest first: up to `early`
    /// running before `go`, `jobs` after.
    fn pump(&self, st: &mut State) {
        let mut done = Vec::new();
        for name in &st.running {
            if let Some(Unit { run: Run::Running(c), .. }) = st.units.get_mut(name)
                && let Ok(Some(x)) = c.try_wait()
            {
                done.push((name.clone(), x));
            }
        }
        for (name, x) in done {
            st.running.retain(|n| *n != name);
            if let Some(u) = st.units.get_mut(&name) {
                u.run = Run::Exited(x);
            }
        }
        let cap = if self.going.load(Ordering::Relaxed) { self.jobs } else { self.early };
        while st.running.len() < cap && !st.waiting.is_empty() {
            let at = (0..st.waiting.len()).max_by_key(|&i| st.waiting[i].0).unwrap_or(0);
            let (_, name) = st.waiting.swap_remove(at);
            let Some(u) = st.units.get(&name) else { continue };
            let run = match self.launch(&name, u) {
                Ok(c) => {
                    st.running.push(name.clone());
                    Run::Running(c)
                }
                Err(e) => Run::Failed(e),
            };
            if let Some(u) = st.units.get_mut(&name) {
                u.run = run;
            }
        }
    }
}

impl Drop for Inner {
    /// The process that started the children stops those `build` did not take (a build that
    /// failed before its cc step); a forked process (an image save) only frees the memory.
    fn drop(&mut self) {
        if std::process::id() != self.owner {
            return;
        }
        let mut st = self.lock();
        for u in st.units.values_mut() {
            if let Run::Running(c) = &mut u.run {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}
