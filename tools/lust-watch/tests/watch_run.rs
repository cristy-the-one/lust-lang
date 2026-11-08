//! Behavioral tests for `lust watch check`. The scheduler's exact coalescing
//! is also covered by unit tests; these drive the binary.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn bin(name: &str) -> PathBuf {
    PathBuf::from(env(name))
}

fn env(key: &str) -> String {
    match key {
        "watch" => env!("CARGO_BIN_EXE_lust-watch").to_string(),
        "stub" => env!("CARGO_BIN_EXE_watch-stub").to_string(),
        _ => unreachable!(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("lust-watch-{name}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

struct Session {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
    dir: PathBuf,
}

impl Session {
    fn spawn(dir: &Path, args: &[&str], extra: &[(&str, String)]) -> Session {
        let mut cmd = Command::new(bin("watch"));
        cmd.args(args).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        cmd.env_remove("LUST_NO_DAEMON");
        cmd.env_remove("LUST_NO_CACHE");
        cmd.env_remove("LUST_DAEMON_SOCKET");
        for (k, v) in extra {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0000_0200);
        }
        let mut child = cmd.spawn().unwrap_or_else(|e| panic!("spawn lust-watch: {e}"));
        let stderr = child.stderr.take().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let slot = Arc::clone(&lines);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => slot.lock().unwrap().push(buf.trim_end().to_string()),
                }
            }
        });
        Session { child, lines, dir: dir.to_path_buf() }
    }

    fn line_count(&self, needle: &str) -> usize {
        self.lines.lock().unwrap().iter().filter(|l| l.contains(needle)).count()
    }

    fn wait_until(&mut self, needle: &str, n: usize, limit: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < limit {
            if self.line_count(needle) >= n {
                return true;
            }
            if self.child.try_wait().ok().flatten().is_some() {
                return self.line_count(needle) >= n;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn statuses(&self) -> Vec<String> {
        self.lines.lock().unwrap().iter().filter(|l| l.starts_with("watch-status:")).cloned().collect()
    }

    fn output(&self) -> String {
        self.lines.lock().unwrap().join("\n")
    }

    fn signal_stop(&self) {
        let pid = self.child.id();
        #[cfg(unix)]
        unsafe {
            unsafe extern "C" {
                fn kill(pid: i32, sig: i32) -> i32;
            }
            kill(pid as i32, 2);
        }
        #[cfg(windows)]
        unsafe {
            unsafe extern "system" {
                fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
            }
            // CTRL_BREAK_EVENT. The child was started in its own process group.
            if GenerateConsoleCtrlEvent(1, pid) == 0 {
                let _ = Command::new("taskkill").args(["/PID", &pid.to_string()]).status();
            }
        }
    }

    fn wait_exit(&mut self, limit: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < limit {
            if self.child.try_wait().ok().flatten().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.signal_stop();
            if !self.wait_exit(Duration::from_secs(3)) {
                let _ = self.child.kill();
                let killed = Instant::now();
                while killed.elapsed() < Duration::from_secs(2) {
                    if self.child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stub_env(dir: &Path, sleep_ms: u64, debounce_ms: u64) -> Vec<(&str, String)> {
    vec![
        ("LUST", bin("stub").display().to_string()),
        ("WATCH_STUB_LOG", dir.join("stub.log").display().to_string()),
        ("WATCH_STUB_SLEEP_MS", sleep_ms.to_string()),
        ("LUST_WATCH_DEBOUNCE_MS", debounce_ms.to_string()),
    ]
}

fn stub_starts(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("stub.log")).unwrap_or_default().lines().filter(|l| l.starts_with("start\t")).count()
}

fn compiler() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LUST")?);
    p.is_file().then_some(p)
}

fn require_compiler() -> Option<PathBuf> {
    let found = compiler();
    if found.is_none() && std::env::var_os("LUST_WATCH_REQUIRE_COMPILER").is_some() {
        panic!("LUST must point at the lust client");
    }
    found
}

#[test]
fn usage_rejects_run_and_test() {
    for args in [vec!["test", "a.rs"], vec!["run", "a.rs"], vec!["build", "a.rs"]] {
        let out = Command::new(bin("watch")).args(&args).output().unwrap();
        assert_ne!(out.status.code(), Some(0), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("only runs check"), "{err}");
    }
}

#[test]
fn rapid_edits_coalesce_to_one_check() {
    let dir = scratch("rapid");
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    let env = stub_env(&dir, 0, 120);
    let mut session = Session::spawn(&dir, &["check", "main.rs", "--sandbox"], &env);
    assert!(session.wait_until("watch-status:", 1, Duration::from_secs(5)), "{}", session.output());
    std::thread::sleep(Duration::from_millis(50));
    for i in 0..8 {
        std::fs::write(dir.join("main.rs"), format!("fn main() {{ let _ = {i}; }}\n")).unwrap();
        std::thread::sleep(Duration::from_millis(15));
    }
    assert!(session.wait_until("watch-status:", 2, Duration::from_secs(5)), "{}", session.output());
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(session.line_count("watch-status:"), 2, "one follow-up after the burst\n{}", session.output());
    assert_eq!(stub_starts(&dir), 2, "{}", std::fs::read_to_string(dir.join("stub.log")).unwrap_or_default());
    let log = std::fs::read_to_string(dir.join("stub.log")).unwrap();
    assert!(log.contains("--sandbox"), "{log}");
    let name = dir.file_name().unwrap().to_str().unwrap();
    assert!(log.contains(name), "check keeps the working directory\n{log}");
    session.signal_stop();
    assert!(session.wait_exit(Duration::from_secs(3)), "clean stop\n{}", session.output());
}

#[test]
fn same_bytes_are_not_another_check() {
    // A kernel can report the file again without the bytes changing
    // (FSEvents does this for the tree that was already on disk, and again
    // in a later batch). Those reports must not start a check.
    let dir = scratch("same");
    let src = "fn main() {}\n";
    std::fs::write(dir.join("main.rs"), src).unwrap();
    let env = stub_env(&dir, 0, 80);
    let mut session = Session::spawn(&dir, &["check", "main.rs"], &env);
    assert!(session.wait_until("watch-status:", 1, Duration::from_secs(5)), "{}", session.output());
    std::thread::sleep(Duration::from_millis(40));
    for _ in 0..6 {
        std::fs::write(dir.join("main.rs"), src).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(session.line_count("watch-status:"), 1, "same bytes are not a new check\n{}", session.output());
    assert_eq!(stub_starts(&dir), 1, "{}", std::fs::read_to_string(dir.join("stub.log")).unwrap_or_default());
}

#[test]
fn edits_during_a_check_schedule_one_follow_up_and_mark_stale() {
    let dir = scratch("during");
    std::fs::write(dir.join("main.rs"), "one\n").unwrap();
    // The check holds the file for a second. The saves are a few milliseconds
    // apart, inside that hold: a 20ms pause on a busy runner stretches past
    // the hold, the follow-up starts on an intermediate save, and that
    // result is stale. The assertions are the final bytes and one follow-up.
    let env = stub_env(&dir, 1_000, 40);
    let mut session = Session::spawn(&dir, &["check", "main.rs", "--features", "a,b", "--target", "x86_64-unknown-linux-gnu", "-C", "panic=abort"], &env);
    let log = dir.join("stub.log");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) && std::fs::read_to_string(&log).unwrap_or_default().lines().filter(|l| l.starts_with("start\t")).count() < 1 {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(std::fs::read_to_string(&log).unwrap_or_default().contains("start\t"), "check started\n{}", session.output());
    for i in 0..5 {
        std::fs::write(dir.join("main.rs"), format!("two-{i}\n")).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.wait_until("watch-status:", 2, Duration::from_secs(5)), "{}", session.output());
    std::thread::sleep(Duration::from_millis(250));
    let statuses = session.statuses();
    assert_eq!(statuses.len(), 2, "{}\n{}", statuses.join(" | "), session.output());
    assert!(statuses[0].contains("stale"), "in-flight edit is stale: {}", statuses[0]);
    assert!(statuses[1].contains("current"), "follow-up matches the saved source: {}", statuses[1]);
    let text = session.output();
    assert!(text.contains("stub-body:one"), "first check read the pre-edit source\n{text}");
    assert!(text.contains("stub-body:two-4"), "follow-up read the last save\n{text}");
    assert_eq!(stub_starts(&dir), 2);
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(args.contains("--features"), "{args}");
    assert!(args.contains("a,b"), "{args}");
    assert!(args.contains("--target"), "{args}");
    assert!(args.contains("x86_64-unknown-linux-gnu"), "{args}");
    assert!(args.contains("-C"), "{args}");
    assert!(args.contains("panic=abort"), "{args}");
    session.signal_stop();
    assert!(session.wait_exit(Duration::from_secs(3)));
}

#[test]
fn excluded_output_dirs_do_not_check() {
    let dir = scratch("exclude");
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    let env = stub_env(&dir, 0, 60);
    let mut session = Session::spawn(&dir, &["check", "main.rs", "-o", "out.bin"], &env);
    assert!(session.wait_until("watch-status:", 1, Duration::from_secs(5)), "{}", session.output());
    std::thread::sleep(Duration::from_millis(40));
    std::fs::create_dir_all(dir.join("target/debug")).unwrap();
    std::fs::create_dir_all(dir.join(".lust-cache")).unwrap();
    std::fs::write(dir.join("target/debug/gen.rs"), "fn generated() {}\n").unwrap();
    std::fs::write(dir.join(".lust-cache/cache.rs"), "fn cached() {}\n").unwrap();
    std::fs::write(dir.join("out.bin"), "binary").unwrap();
    std::fs::write(dir.join("README.md"), "notes").unwrap();
    std::thread::sleep(Duration::from_millis(350));
    assert_eq!(stub_starts(&dir), 1, "generated files are outside the watch set\n{}", session.output());
    std::fs::write(dir.join("main.rs"), "fn main() { let _ = 1; }\n").unwrap();
    assert!(session.wait_until("watch-status:", 2, Duration::from_secs(5)), "{}", session.output());
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(stub_starts(&dir), 2, "{}", session.output());
}

#[test]
fn shutdown_kills_the_check_and_releases_the_directory() {
    let dir = scratch("stop");
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    let env = stub_env(&dir, 8_000, 40);
    let mut session = Session::spawn(&dir, &["check", "main.rs"], &env);
    let log = dir.join("stub.log");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) && !std::fs::read_to_string(&log).unwrap_or_default().contains("start\t") {
        std::thread::sleep(Duration::from_millis(10));
    }
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let pid: u32 = text.split("pid=").nth(1).and_then(|s| s.split('\t').next()).and_then(|s| s.parse().ok()).unwrap_or_else(|| panic!("no stub pid\n{text}"));
    assert!(alive(pid), "stub running");
    session.signal_stop();
    assert!(session.wait_exit(Duration::from_secs(4)), "watcher exits on Ctrl-C\n{}", session.output());
    let deadline = Instant::now() + Duration::from_secs(2);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "check process is gone");
    assert!(session.output().contains("lust watch: stopped"), "{}", session.output());
    let probe = dir.join("released");
    std::fs::write(&probe, "ok").unwrap();
    assert!(probe.is_file());
}

#[test]
fn compile_error_then_fix_and_latency() {
    let Some(lust) = require_compiler() else { return };
    let dir = scratch("fix");
    std::fs::write(dir.join("main.rs"), "fn main() { let x: i32 = \"no\"; }\n").unwrap();
    let route = dir.join("route.log");
    let mut session = Session::spawn(
        &dir,
        &["check", "main.rs", "-C", "panic=abort", "-C", "overflow-checks=on", "-C", "debug-assertions=off"],
        &[
            ("LUST", lust.display().to_string()),
            ("LUST_WATCH_DEBOUNCE_MS", "30".into()),
            ("LUST_ROUTE_LOG", route.display().to_string()),
            ("LUST_DAEMON_IDLE", "120".into()),
        ],
    );
    assert!(session.wait_until("watch-status:", 1, Duration::from_secs(30)), "{}", session.output());
    let first = session.output();
    assert!(first.contains("L0110"), "type error\n{first}");
    assert!(first.contains("watch-status: current code=1"), "{first}");
    let edited = Instant::now();
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    assert!(session.wait_until("watch-status:", 2, Duration::from_secs(30)), "{}", session.output());
    let wall_ms = edited.elapsed().as_millis();
    let statuses = session.statuses();
    assert!(statuses.last().unwrap().contains("current"), "{statuses:?}");
    assert!(statuses.last().unwrap().contains("code=0"), "{statuses:?}\n{}", session.output());
    let tool_ms: u128 = statuses.last().unwrap().split("latency-ms=").nth(1).unwrap().parse().unwrap();
    println!("WATCH_LATENCY_MS tool={tool_ms} wall={wall_ms} debounce=30");
    assert!(tool_ms < 20_000, "tool latency {tool_ms}");
    assert!(wall_ms < 20_000, "wall latency {wall_ms}");
    let route_txt = std::fs::read_to_string(&route).unwrap_or_default();
    assert!(route_txt.contains("cold"), "first check is the client cold path\n{route_txt}");
    // A second edit should hit the warm daemon the first check started.
    let again = Instant::now();
    std::fs::write(dir.join("main.rs"), "fn main() { let _ = 1; }\n").unwrap();
    assert!(session.wait_until("watch-status:", 3, Duration::from_secs(30)), "{}\n{route_txt}", session.output());
    let warm_ms = again.elapsed().as_millis();
    println!("WATCH_LATENCY_MS warm_wall={warm_ms}");
    let route_txt = std::fs::read_to_string(&route).unwrap_or_default();
    assert!(route_txt.contains("daemon"), "later check reuses the warm session\n{route_txt}");
    session.signal_stop();
    assert!(session.wait_exit(Duration::from_secs(5)), "{}", session.output());
    assert_no_daemon(&dir);
}

#[test]
fn sandbox_and_features_reach_the_compiler() {
    let Some(lust) = require_compiler() else { return };
    let dir = scratch("flags");
    std::fs::write(dir.join("sand.rs"), "fn main() { let _ = std::fs::write(\"x\", \"y\"); }\n").unwrap();
    let feat_dir = scratch("feat");
    std::fs::create_dir_all(feat_dir.join("src")).unwrap();
    std::fs::write(
        feat_dir.join("Cargo.toml"),
        "[package]\nname = \"w\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[features]\nextra = []\n",
    )
    .unwrap();
    std::fs::write(feat_dir.join("src/main.rs"), "fn main() {\n    #[cfg(feature = \"extra\")]\n    let x: i32 = \"no\";\n}\n").unwrap();
    let host = host_triple();
    let mut sand_args = vec!["check", "sand.rs", "--sandbox"];
    let target_arg;
    if !host.is_empty() {
        target_arg = format!("--target={host}");
        sand_args.push(target_arg.as_str());
    }
    let mut sand = Session::spawn(&dir, &sand_args, &[("LUST", lust.display().to_string()), ("LUST_WATCH_DEBOUNCE_MS", "50".into()), ("LUST_DAEMON_IDLE", "60".into())]);
    assert!(sand.wait_until("watch-status:", 1, Duration::from_secs(30)), "{}", sand.output());
    let text = sand.output();
    assert!(text.contains("L0532"), "sandbox flag reached lust\n{text}");
    assert!(text.contains("code=1"), "{text}");
    let sand_pid = daemon_pid(&dir);
    drop(sand);
    if let Some(pid) = sand_pid {
        wait_dead(pid);
        assert!(!alive(pid), "sandbox watch left a daemon");
    }

    let mut feat = Session::spawn(
        &feat_dir,
        &["check", "src/main.rs", "--manifest-path", "Cargo.toml", "--features", "extra", "-C", "panic=abort"],
        &[("LUST", lust.display().to_string()), ("LUST_WATCH_DEBOUNCE_MS", "50".into()), ("LUST_DAEMON_IDLE", "60".into())],
    );
    assert!(feat.wait_until("watch-status:", 1, Duration::from_secs(60)), "{}", feat.output());
    let text = feat.output();
    assert!(text.contains("L0110"), "feature extra was selected\n{text}");
    let feat_pid = daemon_pid(&feat_dir);
    drop(feat);
    if let Some(pid) = feat_pid {
        wait_dead(pid);
        assert!(!alive(pid), "feature watch left a daemon");
    }
}

#[test]
fn shutdown_stops_the_daemon_and_the_directory_can_be_removed() {
    let Some(lust) = require_compiler() else { return };
    let dir = scratch("daemon");
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    let mut session = Session::spawn(
        &dir,
        &["check", "main.rs"],
        &[("LUST", lust.display().to_string()), ("LUST_WATCH_DEBOUNCE_MS", "40".into()), ("LUST_DAEMON_IDLE", "180".into())],
    );
    assert!(session.wait_until("watch-status:", 1, Duration::from_secs(30)), "{}", session.output());
    let pid = daemon_pid(&dir).unwrap_or_else(|| panic!("daemon did not start\n{}", session.output()));
    assert!(alive(pid));
    session.signal_stop();
    assert!(session.wait_exit(Duration::from_secs(5)), "{}", session.output());
    let deadline = Instant::now() + Duration::from_secs(3);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "daemon started by watch is gone");
    // The watcher has left this directory. Removing it must succeed on
    // Windows too, where a live cwd or a directory watch holds the delete.
    std::thread::sleep(Duration::from_millis(50));
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| panic!("directory still held: {e}"));
}

fn daemon_pid(dir: &Path) -> Option<u32> {
    let rd = std::fs::read_dir(dir).ok()?;
    for ent in rd.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".lust-daemon-") && name.ends_with(".sock.lock") {
            return std::fs::read_to_string(ent.path()).ok().and_then(|t| t.trim().parse().ok());
        }
    }
    None
}

fn wait_dead(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn assert_no_daemon(dir: &Path) {
    if let Some(pid) = daemon_pid(dir) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(pid), "daemon {pid} still running in {}", dir.display());
    }
}

fn host_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        _ => "",
    }
}

fn alive(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    #[cfg(unix)]
    unsafe {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        kill(pid as i32, 0) == 0
    }
    #[cfg(windows)]
    unsafe {
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn GetExitCodeProcess(handle: *mut std::ffi::c_void, code: *mut u32) -> i32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE
    }
}
