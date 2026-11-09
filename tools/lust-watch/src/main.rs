//! `lust watch check`. Runs in its own process so `notify` is not linked into
//! `lust` or `lust-core`. Each check is a `lust check` of the same arguments,
//! in this process's working directory, so the warm daemon session, target,
//! features, profile, and sandbox are the ones the client already implements.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use lust_watch::{is_real_dir, is_relevant, is_skipped_dir, parse_watch, survey, usage, watch_plan, Plan, Scheduler};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

static STOP: AtomicBool = AtomicBool::new(false);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || (args.len() == 1 && args.iter().any(|a| a == "--help" || a == "-h")) {
        eprintln!("{}", usage());
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }
    let cmd = match parse_watch(&args) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let client = client_path();
    if !client.is_file() {
        eprintln!("error: cannot find the lust client at {}", client.display());
        std::process::exit(1);
    }
    let debounce_ms = std::env::var("LUST_WATCH_DEBOUNCE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
    let debounce = Duration::from_millis(debounce_ms);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut plan = watch_plan(&cmd.file, cmd.manifest.as_deref(), &cwd);
    plan.output = cmd.output.clone();
    install_signals();
    let (tx, rx) = mpsc::channel();
    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("error: cannot start the file watcher: {e}");
            std::process::exit(1);
        }
    };
    let mut watched = std::collections::HashSet::new();
    for root in &plan.roots {
        add_tree(&mut watcher, &mut watched, root, &plan);
    }
    if watched.is_empty() {
        eprintln!("error: nothing to watch for {}", cmd.file.display());
        std::process::exit(1);
    }
    let before = daemon_ids();
    eprintln!("lust watch: {} (debounce {debounce_ms} ms)", cmd.file.display());
    let mut sched = Scheduler::new(debounce);
    sched.begin();
    // Fingerprint of the last check whose bytes matched the tree. A later
    // kernel event for those same bytes is not another check.
    let mut settled: Option<u64> = None;
    let mut inflight = match start_check(&client, &cmd.args, &plan) {
        Ok(run) => Some(run),
        Err(e) => {
            eprintln!("error: cannot run {}: {e}", client.display());
            std::process::exit(1);
        }
    };
    loop {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        while let Ok(event) = rx.try_recv() {
            on_event(&event, &mut watcher, &mut watched, &plan, &mut sched, &inflight, settled);
        }
        let now = Instant::now();
        // The quiet time is up. Re-read mtimes first: saves the kernel has
        // not delivered yet still move this one deadline instead of starting
        // a check that would need a second one.
        if sched.time_until(now).is_some_and(|d| d.is_zero()) {
            let saved = survey(&plan.roots, &plan.exclude, plan.output.as_deref());
            if settled.is_some_and(|fp| fp == saved.fingerprint) {
                sched.on_unchanged();
            } else if sched.armed() != Some(saved.fingerprint) {
                // The bytes moved during the quiet window (the mtime did
                // not have to). This same check waits out a new window.
                sched.on_save(now, saved.fingerprint);
            } else if let Some(trigger) = sched.poll(now) {
                match start_check(&client, &cmd.args, &plan) {
                    Ok(run) => inflight = Some(Check { trigger: Some(trigger), ..run }),
                    Err(e) => {
                        eprintln!("error: cannot run {}: {e}", client.display());
                        sched.on_finished();
                    }
                }
                continue;
            }
        }
        let done = inflight.as_mut().and_then(|run| run.child.try_wait().ok().flatten());
        if let Some(status) = done {
            let run = inflight.take().expect("the check that just finished");
            finish_check(run, status, &plan, &mut sched, &mut settled);
            continue;
        }
        let slice = sched.time_until(Instant::now()).unwrap_or(Duration::from_millis(50)).min(Duration::from_millis(50));
        match rx.recv_timeout(slice) {
            Ok(event) => on_event(&event, &mut watcher, &mut watched, &plan, &mut sched, &inflight, settled),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if let Some(mut run) = inflight {
        terminate_child(&mut run.child);
    }
    drop(watcher);
    drop(watched);
    stop_daemons_we_started(&before);
    let _ = std::env::set_current_dir(std::env::temp_dir());
    eprintln!("lust watch: stopped");
}

struct Check {
    child: Child,
    fingerprint: u64,
    /// The edit this check answers. None for the check started with the command.
    trigger: Option<Instant>,
}

fn start_check(client: &Path, args: &[String], plan: &Plan) -> std::io::Result<Check> {
    let fingerprint = survey(&plan.roots, &plan.exclude, plan.output.as_deref()).fingerprint;
    let mut cmd = Command::new(client);
    cmd.arg("check").args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    // A new process group so this command's Ctrl-C is handled here, and the
    // check is killed by us instead of racing the watcher.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0200);
    }
    let child = cmd.spawn()?;
    Ok(Check { child, fingerprint, trigger: None })
}

fn finish_check(run: Check, status: ExitStatus, plan: &Plan, sched: &mut Scheduler, settled: &mut Option<u64>) {
    // `try_wait` already reaped the child. Stale means the bytes on disk
    // are not the bytes this check snapshotted. A notification that does
    // not change those bytes (the FSEvents scan, or a second batch of a
    // save we already checked) leaves the result current and arms nothing.
    let saved = survey(&plan.roots, &plan.exclude, plan.output.as_deref());
    let stale = saved.fingerprint != run.fingerprint;
    let code = status.code().unwrap_or(1);
    let latency = run.trigger.map(|t| t.elapsed().as_millis().to_string()).unwrap_or_else(|| "-".to_string());
    let fresh = if stale { "stale" } else { "current" };
    eprintln!("watch-status: {fresh} code={code} latency-ms={latency}");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    if !stale {
        *settled = Some(run.fingerprint);
    }
    sched.on_finished();
    if stale {
        // Exactly one follow-up, quieted from these bytes. A later event
        // for the same bytes does not add another window.
        arm_if_changed(plan, sched, *settled);
    }
}

fn on_event(
    event: &Event,
    watcher: &mut RecommendedWatcher,
    watched: &mut std::collections::HashSet<PathBuf>,
    plan: &Plan,
    sched: &mut Scheduler,
    inflight: &Option<Check>,
    settled: Option<u64>,
) {
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    let mut look = false;
    for path in &event.paths {
        if is_skipped_dir(path, &plan.exclude) {
            continue;
        }
        if is_real_dir(path) {
            add_tree(watcher, watched, path, plan);
            look = true;
            continue;
        }
        if is_relevant(path, &plan.exclude, plan.output.as_deref()) {
            look = true;
        }
    }
    if !look {
        return;
    }
    let saved = survey(&plan.roots, &plan.exclude, plan.output.as_deref());
    if sched.running() {
        // Same bytes as the check that is already running: the event is
        // the scan or a duplicate, not an edit.
        if inflight.as_ref().is_some_and(|run| saved.fingerprint != run.fingerprint) {
            sched.on_save(Instant::now(), saved.fingerprint);
        }
        return;
    }
    if settled.is_some_and(|fp| fp == saved.fingerprint) {
        sched.on_unchanged();
        return;
    }
    sched.on_save(Instant::now(), saved.fingerprint);
}

fn arm_if_changed(plan: &Plan, sched: &mut Scheduler, settled: Option<u64>) {
    let saved = survey(&plan.roots, &plan.exclude, plan.output.as_deref());
    if settled.is_some_and(|fp| fp == saved.fingerprint) {
        sched.on_unchanged();
        return;
    }
    sched.on_save(Instant::now(), saved.fingerprint);
}

fn add_tree(watcher: &mut RecommendedWatcher, watched: &mut std::collections::HashSet<PathBuf>, dir: &Path, plan: &Plan) {
    if is_skipped_dir(dir, &plan.exclude) || !is_real_dir(dir) {
        return;
    }
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !watched.insert(dir.clone()) {
        return;
    }
    if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
        return;
    }
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    for ent in rd.flatten() {
        let path = ent.path();
        if is_real_dir(&path) {
            add_tree(watcher, watched, &path, plan);
        }
    }
}

fn client_path() -> PathBuf {
    if let Some(p) = std::env::var_os("LUST") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    match std::env::current_exe() {
        Ok(exe) => sibling(&exe, "lust"),
        Err(_) => PathBuf::from(if cfg!(windows) { "lust.exe" } else { "lust" }),
    }
}

fn sibling(exe: &Path, name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let with_exe = exe.with_file_name(format!("{name}.exe"));
        let bare = exe.with_file_name(name);
        if with_exe.is_file() {
            return with_exe;
        }
        if bare.is_file() {
            return bare;
        }
        return with_exe;
    }
    #[cfg(not(windows))]
    {
        exe.with_file_name(name)
    }
}

struct DaemonId {
    lock: PathBuf,
    pid: u32,
}

fn daemon_ids() -> Vec<DaemonId> {
    let mut out = Vec::new();
    let mut consider = |lock: PathBuf| {
        if let Some(pid) = read_pid(&lock) {
            out.push(DaemonId { lock, pid });
        }
    };
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rd) = std::fs::read_dir(&cwd) {
            for ent in rd.flatten() {
                let name = ent.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".lust-daemon-") && name.ends_with(".sock.lock") {
                    consider(ent.path());
                }
            }
        }
    }
    if let Some(sock) = std::env::var_os("LUST_DAEMON_SOCKET") {
        if !sock.is_empty() {
            let mut lock = std::ffi::OsString::from(&sock);
            lock.push(".lock");
            consider(PathBuf::from(lock));
        }
    }
    out
}

fn read_pid(lock: &Path) -> Option<u32> {
    std::fs::read_to_string(lock).ok().and_then(|t| t.trim().parse().ok())
}

fn stop_daemons_we_started(before: &[DaemonId]) {
    let ours: Vec<u32> = daemon_ids()
        .into_iter()
        .filter(|now| now.pid > 1 && now.pid != std::process::id() && !before.iter().any(|b| b.pid == now.pid))
        .map(|now| now.pid)
        .collect();
    if ours.is_empty() {
        return;
    }
    // Removing the address file is the daemon's graceful stop. Drop every
    // socket that was not already there: the lock file and the socket share
    // a name when the client and lust-core are the same build.
    let kept: std::collections::HashSet<PathBuf> = before.iter().map(|b| b.lock.with_extension("")).collect();
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rd) = std::fs::read_dir(cwd) {
            for ent in rd.flatten() {
                let name = ent.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".lust-daemon-") && name.ends_with(".sock") && !kept.contains(&ent.path()) {
                    let _ = std::fs::remove_file(ent.path());
                }
            }
        }
    }
    for pid in ours {
        if !wait_dead(pid, Duration::from_secs(2)) {
            force_kill(pid);
            let _ = wait_dead(pid, Duration::from_secs(2));
        }
    }
}

fn wait_dead(pid: u32, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    !pid_alive(pid)
}

fn terminate_child(child: &mut Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        unsafe { kill(-(pid as i32), SIGTERM) };
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(800) {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        unsafe { kill(-(pid as i32), SIGKILL) };
        let _ = child.kill();
        let _ = child.wait();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/T", "/PID", &pid.to_string()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(800) {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &pid.to_string()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn force_kill(pid: u32) {
    #[cfg(unix)]
    unsafe {
        kill(pid as i32, SIGTERM);
        std::thread::sleep(Duration::from_millis(200));
        if pid_alive(pid) {
            kill(pid as i32, SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &pid.to_string()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

fn pid_alive(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    #[cfg(unix)]
    {
        unsafe { kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn GetExitCodeProcess(handle: *mut std::ffi::c_void, code: *mut u32) -> i32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
        unsafe { CloseHandle(handle) };
        ok != 0 && code == STILL_ACTIVE
    }
}

#[cfg(unix)]
const SIGTERM: i32 = 15;
#[cfg(unix)]
const SIGKILL: i32 = 9;

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn signal(sig: i32, handler: *const ()) -> usize;
}

fn install_signals() {
    #[cfg(unix)]
    unsafe {
        extern "C" fn on_stop(_: i32) {
            STOP.store(true, Ordering::SeqCst);
        }
        signal(2, on_stop as *const ());
        signal(15, on_stop as *const ());
    }
    #[cfg(windows)]
    unsafe {
        extern "system" fn on_ctrl(ctrl: u32) -> i32 {
            if ctrl <= 2 {
                STOP.store(true, Ordering::SeqCst);
                1
            } else {
                0
            }
        }
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
        }
        SetConsoleCtrlHandler(Some(on_ctrl), 1);
    }
}
