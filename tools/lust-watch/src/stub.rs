//! Stand-in for the lust client. Scheduling tests point `LUST` here so a check
//! has a known delay and records the exact arguments and working directory.
//! The real client is used for diagnostic, sandbox, feature, and daemon tests.

use std::io::Write;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    if let Some(path) = std::env::var_os("WATCH_STUB_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "start\tpid={}\tcwd={cwd}\targs={}", std::process::id(), args.join("\t"));
            let _ = f.flush();
        }
    }
    let sleep_ms = std::env::var("WATCH_STUB_SLEEP_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let body = lust_watch::parse_watch(&args).ok().and_then(|cmd| std::fs::read_to_string(cmd.file).ok());
    // Read before the sleep so a save during the check is not what this
    // process reports. The watcher then has to call the result stale.
    if sleep_ms > 0 {
        std::thread::sleep(Duration::from_millis(sleep_ms));
    }
    let mut code = 0;
    if let Some(text) = body {
        eprintln!("stub-body:{}", text.trim_end());
        if text.contains("STUB_FAIL") {
            eprintln!("stub error STUB_FAIL");
            code = 1;
        }
    }
    if let Some(path) = std::env::var_os("WATCH_STUB_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "end\tpid={}", std::process::id());
            let _ = f.flush();
        }
    }
    std::process::exit(code);
}
