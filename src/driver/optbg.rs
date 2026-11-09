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

/// How long a lock is trusted while a background build has not published.
const LOCK_FRESH: u64 = 60 * 60;

/// Installs a cached optimised binary over `req.out`, or starts a background
/// `--opt` build that will publish one. The tcc binary is already at `req.out`.
pub fn swap_or_spawn(req: &Request, map: &SourceMap, stderr: &mut String) {
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
    if lock_fresh(&lock) {
        let _ = writeln!(stderr, "opt: optimised build still running");
        return;
    }
    let _ = std::fs::remove_file(&lock);
    if let Err(e) = write_lock(&lock) {
        let _ = writeln!(stderr, "opt: could not start an optimised build ({e}); keeping the fast build");
        return;
    }
    match spawn(req, &bin) {
        Ok(()) => {
            let _ = writeln!(stderr, "opt: optimised build started in the background");
        }
        Err(e) => {
            let _ = std::fs::remove_file(&lock);
            let _ = writeln!(stderr, "opt: could not start an optimised build ({e}); keeping the fast build");
        }
    }
}

/// The background `--opt` child finished a link. Writes the marker the next
/// `--opt=bg` waits for, and drops the lock. No-op unless this process was
/// started as that child (`LUST_OPT_PUBLISH`).
pub fn published(req: &Request) {
    if !publishing() {
        return;
    }
    let _ = std::fs::write(marker_path(&req.out), b"ok\n");
    let _ = std::fs::remove_file(lock_path(&req.out));
}

/// The background `--opt` child failed. Drops a partial binary, the marker
/// and the lock so the next `--opt=bg` can try again.
pub fn abandoned(req: &Request) {
    if !publishing() {
        return;
    }
    let _ = std::fs::remove_file(&req.out);
    let _ = std::fs::remove_file(marker_path(&req.out));
    let _ = std::fs::remove_file(lock_path(&req.out));
}

fn publishing() -> bool {
    std::env::var("LUST_OPT_PUBLISH").ok().as_deref() == Some("1")
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
        &[b"opt-fp-v1", env!("LUST_BUILD_HASH").as_bytes(), triple.as_bytes(), named.as_bytes(), prof.as_bytes(), stamp.as_bytes(), body.as_bytes()],
        19,
    );
    let mut name = format!("opt-{a:016x}{b:016x}");
    if triple.contains("windows") || cfg!(windows) {
        name.push_str(".exe");
    }
    Some(cache::cache_dir(&req.file).join("opt").join(name))
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

fn spawn(req: &Request, bin: &Path) -> Result<(), String> {
    if let Some(parent) = bin.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let log_path = cache::cache_dir(&req.file).join("opt-bg.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let client = protocol::sibling_tool(&exe, "lust");
    let prog = if client.is_file() { client } else { exe };
    let mut cmd = Command::new(&prog);
    cmd.args(background_args(req, bin))
        .env("LUST_OPT_PUBLISH", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(err));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // A new session: the build keeps running after this process
                // exits, and it is not tied to the client's terminal.
                // setsid is async-signal-safe and takes no arguments.
                if setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        cmd.creation_flags(0x00000008 | 0x00000200 | 0x08000000);
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot run {}: {e}", prog.display()))?;
    // Reap the child so a long-lived daemon does not keep a zombie. The
    // child has its own session; this wait does not block the request.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(unix)]
unsafe extern "C" {
    fn setsid() -> i32;
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
