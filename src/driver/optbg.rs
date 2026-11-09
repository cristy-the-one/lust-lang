// `--opt=bg` (optional): the fast tcc build is the one this request links, and an
// optimised build of the same source is started in the background the first time.
// A later request whose sources, profile, target and lust build still match
// installs that cached binary over the tcc output before the client runs it.
// The optimised build is a separate session (`ProfileFlags::opt`): direct calls,
// the host C compiler, strict floating-point flags (src/cc.rs). The sandbox
// flag and the dev profile ride along, so the cached binary stops on the same
// OS calls and panics on the same overflows.
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::cache::{self, hash128};
use crate::diag::{FileId, SourceMap};
use crate::protocol::{self, Request};

/// How long a per-binary lock is trusted while that build has not published.
const LOCK_FRESH: u64 = 60 * 60;

/// Default bound on one background compile (`LUST_OPT_TIMEOUT` seconds).
const DEFAULT_TIMEOUT: u64 = 600;

/// Installs a cached optimised binary over `req.out`, or starts a background
/// `--opt` build that will publish one. The tcc binary is already at `req.out`.
pub fn swap_or_spawn(req: &Request, map: &SourceMap, stderr: &mut String, daemon: bool) {
    let Some(bin) = cache_bin(req, map) else {
        let _ = writeln!(stderr, "opt: no optimising C compiler on PATH (install gcc, clang or zig, or set LUST_CC); keeping the fast build");
        return;
    };
    let marker = marker_path(&bin);
    let lock = lock_path(&bin);
    if marker.is_file() && bin.is_file() {
        match install(&bin, &req.out) {
            Ok(()) => {
                let _ = writeln!(stderr, "opt: cached optimised build");
            }
            Err(e) => {
                let _ = writeln!(stderr, "opt: cached build could not be installed ({e}); keeping the fast build");
            }
        }
        return;
    }
    if background_busy() || lock_fresh(&lock) {
        let _ = writeln!(stderr, "opt: optimised build still running");
        return;
    }
    let _ = std::fs::remove_file(&lock);
    if let Err(e) = write_lock(&lock) {
        let _ = writeln!(stderr, "opt: could not start an optimised build ({e}); keeping the fast build");
        return;
    }
    match spawn(req, &bin, daemon) {
        Ok(()) => {
            let _ = writeln!(stderr, "opt: optimised build started in the background");
        }
        Err(e) => {
            let _ = std::fs::remove_file(&lock);
            let _ = writeln!(stderr, "opt: could not start an optimised build ({e}); keeping the fast build");
        }
    }
}

/// An `--opt` link whose output is the cache file. The daemon writes the
/// marker here: the background client only asks for the build, and the env
/// of the process that spawned it is not this process's.
fn cache_output(req: &Request) -> bool {
    req.session.profile.opt && req.out.starts_with(opt_cache_root())
}

/// The optimised binary landed in the cache. The next `--opt=bg` installs it.
pub fn published(req: &Request) {
    if !cache_output(req) {
        return;
    }
    let _ = std::fs::write(marker_path(&req.out), b"ok\n");
    let _ = std::fs::remove_file(lock_path(&req.out));
}

/// The cached optimised build failed. Drop a partial binary so the next
/// `--opt=bg` can try again.
pub fn abandoned(req: &Request) {
    if !cache_output(req) {
        return;
    }
    let _ = std::fs::remove_file(&req.out);
    let _ = std::fs::remove_file(marker_path(&req.out));
    let _ = std::fs::remove_file(lock_path(&req.out));
}

fn marker_path(bin: &Path) -> PathBuf {
    PathBuf::from(format!("{}.ok", bin.display()))
}

fn lock_path(bin: &Path) -> PathBuf {
    PathBuf::from(format!("{}.lock", bin.display()))
}

/// The cached optimised binary for this request, or None when this host has
/// no C compiler for the opt backend (the fast build still stands).
fn cache_bin(req: &Request, map: &SourceMap) -> Option<PathBuf> {
    let triple = if req.session.target_triple.is_empty() { protocol::host_triple().to_string() } else { req.session.target_triple.clone() };
    if triple.is_empty() {
        return None;
    }
    let stamp = crate::cc::compiler_stamp(&triple);
    if stamp.is_empty() || stamp.starts_with("no C compiler") {
        return None;
    }
    let mut files: Vec<String> = (0..map.file_count())
        .map(|i| {
            let id = FileId(i as u32);
            format!("{}\t{:016x}", map.path(id), map.text_hash(id))
        })
        .collect();
    files.sort();
    let prof = format!(
        "panic={} overflow={} debug={} sandbox={}",
        req.session.profile.panic_abort as u8,
        req.session.profile.overflow_checks as u8,
        req.session.profile.debug_assertions as u8,
        req.session.profile.sandbox as u8
    );
    let body = files.join("\n");
    let named = req.file.display().to_string();
    let (a, b) = hash128(
        &[b"opt-fp-v2", env!("LUST_BUILD_HASH").as_bytes(), triple.as_bytes(), named.as_bytes(), prof.as_bytes(), stamp.as_bytes(), body.as_bytes()],
        19,
    );
    let mut name = format!("opt-{a:016x}{b:016x}");
    if triple.contains("windows") || cfg!(windows) {
        name.push_str(".exe");
    }
    Some(opt_cache_root().join(name))
}

/// Where optimised binaries are cached. `LUST_CACHE_DIR` when the host sets
/// it (Lime Slice passes its own folder). Otherwise a user cache directory,
/// never the recipe's directory and never a `.lust-cache` inside the project.
fn opt_cache_root() -> PathBuf {
    if let Some(d) = std::env::var_os("LUST_CACHE_DIR")
        && !d.is_empty()
    {
        return PathBuf::from(d).join("opt");
    }
    user_cache_dir().join("lust").join("opt")
}

fn user_cache_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA")
        && !local.is_empty()
    {
        return PathBuf::from(local);
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home).join("Library").join("Caches");
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg);
    }
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home).join(".cache");
    }
    std::env::temp_dir()
}

fn install(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = PathBuf::from(format!("{}.opt-tmp", to.display()));
    std::fs::copy(from, &tmp).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    if std::fs::rename(&tmp, to).is_err() {
        let _ = std::fs::remove_file(to);
        std::fs::rename(&tmp, to).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn lock_fresh(lock: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(lock) else { return false };
    let Ok(modified) = meta.modified() else { return false };
    modified.elapsed().map(|d| d.as_secs() < LOCK_FRESH).unwrap_or(false)
}

fn write_lock(lock: &Path) -> Result<(), String> {
    if let Some(parent) = lock.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    std::fs::write(lock, format!("pid {} {now}\n", std::process::id())).map_err(|e| e.to_string())
}

fn spawn(req: &Request, bin: &Path, daemon: bool) -> Result<(), String> {
    if let Some(parent) = bin.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let root = opt_cache_root();
    let log_path = root.join("opt-bg.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let client = protocol::sibling_tool(&exe, "lust");
    let prog = if client.is_file() { client } else { exe };
    let args = background_args(req, bin);
    let _ = daemon;
    // Unix: a shell stays the group leader so a timeout or SIGTERM kills the
    // compiler children too. It runs only the lust build. Windows: lust.exe
    // itself, in a job object that dies with this process.
    #[cfg(unix)]
    let mut cmd = {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(r#"trap 'kill -KILL 0' TERM; "$0" "$@"; s=$?; exit $s"#).arg(&prog).args(&args);
        cmd
    };
    #[cfg(windows)]
    let mut cmd = {
        let mut cmd = Command::new(&prog);
        cmd.args(&args);
        cmd
    };
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::from(log)).stderr(std::process::Stdio::from(err));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                {
                    // PR_SET_PDEATHSIG = 1, SIGTERM = 15. Preserved across exec.
                    if prctl(1, 15) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if getppid() == 1 {
                        return Err(std::io::Error::other("parent exited"));
                    }
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW. Not DETACHED_PROCESS: the job object kills this
        // tree when the daemon exits.
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot run {}: {e}", prog.display()))?;
    let pid = child.id();
    let _ = std::fs::write(global_lock(), format!("pid {pid}\n"));
    #[cfg(windows)]
    let job = winjob::assign(&child);
    let limit = timeout_secs();
    let global = global_lock();
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let limit = std::time::Duration::from_secs(limit);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if start.elapsed() >= limit => {
                    #[cfg(unix)]
                    unsafe {
                        kill(-(pid as i32), 9);
                    }
                    #[cfg(windows)]
                    {
                        let _ = child.kill();
                        #[cfg(windows)]
                        if let Some(job) = job {
                            winjob::terminate(job);
                        }
                    }
                    let _ = child.wait();
                    break;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
                Err(_) => break,
            }
        }
        if std::fs::read_to_string(&global).ok().is_some_and(|t| t.contains(&format!("pid {pid}"))) {
            let _ = std::fs::remove_file(&global);
        }
    });
    Ok(())
}

fn timeout_secs() -> u64 {
    std::env::var("LUST_OPT_TIMEOUT").ok().and_then(|v| v.parse().ok()).filter(|n| *n > 0).unwrap_or(DEFAULT_TIMEOUT)
}

fn global_lock() -> PathBuf {
    opt_cache_root().join("running.lock")
}

/// True when some background optimised build is still in this process's
/// cache and its pid is alive. One build at a time.
fn background_busy() -> bool {
    let text = std::fs::read_to_string(global_lock()).unwrap_or_default();
    let Some(pid) = text.split_whitespace().nth(1).and_then(|s| s.parse::<u32>().ok()) else {
        return false;
    };
    if crate::sys_host::process_alive(pid) {
        return true;
    }
    let _ = std::fs::remove_file(global_lock());
    false
}

/// Stops background optimised builds. The daemon calls this on shutdown;
/// on Linux the parent-death signal is the one that fires if this is skipped.
pub fn kill_background() {
    let text = std::fs::read_to_string(global_lock()).unwrap_or_default();
    if let Some(pid) = text.split_whitespace().nth(1).and_then(|s| s.parse::<u32>().ok()) {
        #[cfg(unix)]
        unsafe {
            kill(-(pid as i32), 15);
            kill(-(pid as i32), 9);
        }
        #[cfg(windows)]
        {
            let _ = pid;
            winjob::terminate_all();
        }
    }
    let _ = std::fs::remove_file(global_lock());
}

#[cfg(unix)]
unsafe extern "C" {
    fn setpgid(pid: i32, pgid: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn prctl(option: i32, arg2: u64, ...) -> i32;
    fn getppid() -> i32;
}

#[cfg(windows)]
mod winjob {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::Mutex;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attrs: *mut u8, name: *const u16) -> *mut u8;
        fn SetInformationJobObject(job: *mut u8, class: u32, info: *const u8, len: u32) -> i32;
        fn AssignProcessToJobObject(job: *mut u8, process: *mut u8) -> i32;
        fn TerminateJobObject(job: *mut u8, code: u32) -> i32;
        fn CloseHandle(handle: *mut u8) -> i32;
    }

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    const JobObjectBasicLimitInformation: u32 = 2;

    #[repr(C)]
    struct BasicLimit {
        per_process: i64,
        per_job: i64,
        flags: u32,
        _pad: u32,
        min_ws: usize,
        max_ws: usize,
        active: u32,
        _pad2: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }

    static JOBS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

    pub fn assign(child: &Child) -> Option<usize> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let info = BasicLimit {
                per_process: 0,
                per_job: 0,
                flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                _pad: 0,
                min_ws: 0,
                max_ws: 0,
                active: 0,
                _pad2: 0,
                affinity: 0,
                priority: 0,
                scheduling: 0,
            };
            SetInformationJobObject(job, JobObjectBasicLimitInformation, &info as *const BasicLimit as *const u8, std::mem::size_of::<BasicLimit>() as u32);
            let handle = child.as_raw_handle() as *mut u8;
            if AssignProcessToJobObject(job, handle) == 0 {
                CloseHandle(job);
                return None;
            }
            let bits = job as usize;
            if let Ok(mut g) = JOBS.lock() {
                g.push(bits);
            }
            Some(bits)
        }
    }

    pub fn terminate(job: usize) {
        unsafe {
            TerminateJobObject(job as *mut u8, 1);
            CloseHandle(job as *mut u8);
        }
        if let Ok(mut g) = JOBS.lock() {
            g.retain(|j| *j != job);
        }
    }

    pub fn terminate_all() {
        let jobs: Vec<usize> = JOBS.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
        for job in jobs {
            unsafe {
                TerminateJobObject(job as *mut u8, 1);
                CloseHandle(job as *mut u8);
            }
        }
    }
}

/// The `--opt` command that rebuilds `req` into `bin`. Same sandbox, profile
/// and crate selection; the opt flag is what selects the optimising backend.
fn background_args(req: &Request, bin: &Path) -> Vec<String> {
    let mut a = vec!["build".to_string(), req.file.display().to_string(), "--opt".to_string(), "-o".to_string(), bin.display().to_string()];
    if req.session.profile.sandbox {
        a.push("--sandbox".into());
    }
    if req.debug {
        a.push("-g".into());
    }
    let p = &req.session.profile;
    a.push("-C".into());
    a.push(format!("panic={}", if p.panic_abort { "abort" } else { "unwind" }));
    a.push("-C".into());
    a.push(format!("overflow-checks={}", if p.overflow_checks { "on" } else { "off" }));
    a.push("-C".into());
    a.push(format!("debug-assertions={}", if p.debug_assertions { "on" } else { "off" }));
    if !req.session.target_triple.is_empty() {
        a.push("--target".into());
        a.push(req.session.target_triple.clone());
    }
    if let Some(m) = &req.session.krate.manifest_path {
        a.push("--manifest-path".into());
        a.push(m.display().to_string());
        if let Some(pkg) = req.session.krate.package_id.split_whitespace().next() {
            if !pkg.is_empty() && !pkg.starts_with("file:") {
                a.push("--package".into());
                a.push(pkg.to_string());
            }
        }
        match req.session.krate.target_kind.as_str() {
            "lib" => a.push("--lib".into()),
            "bin" => {
                a.push("--bin".into());
                a.push(req.session.krate.target_name.clone());
            }
            _ => {}
        }
    }
    if !req.session.features.is_empty() {
        a.push("--features".into());
        a.push(req.session.features.join(","));
    }
    if !req.session.default_features {
        a.push("--no-default-features".into());
    }
    if req.session.all_features {
        a.push("--all-features".into());
    }
    if req.session.cargo_net.offline {
        a.push("--offline".into());
    }
    if req.session.cargo_net.locked {
        a.push("--locked".into());
    }
    a
}
