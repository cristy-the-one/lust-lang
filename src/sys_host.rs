// Process and memory queries the daemon and the base-library sweep share.
// On Linux a live pid is a `/proc/<pid>` directory, which is what the sweep
// already used. On Windows it is an open process whose exit code is still
// `STILL_ACTIVE`.
use std::path::Path;

/// True when `pid` is a running process. A reused pid reads as alive, which
/// only keeps a stale directory or base file longer.
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        return Path::new(&format!("/proc/{pid}")).exists();
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        return unsafe { kill(pid as i32, 0) } == 0;
    }
    #[cfg(windows)]
    {
        win_process_alive(pid)
    }
}

#[cfg(windows)]
fn win_process_alive(pid: u32) -> bool {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn GetExitCodeProcess(process: *mut std::ffi::c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    unsafe {
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

/// Working-set kilobytes and a thread count, for the daemon stats line.
/// Linux reads `/proc/self/status` (`VmRSS`, `Threads`).
pub fn mem_line() -> (String, String) {
    #[cfg(not(windows))]
    {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let field = |name: &str| {
            status.lines().find(|l| l.starts_with(name)).and_then(|l| l.split_whitespace().nth(1)).unwrap_or("?").to_string()
        };
        (field("VmRSS:"), field("Threads:"))
    }
    #[cfg(windows)]
    {
        (win_rss_kb(), win_thread_count())
    }
}

#[cfg(windows)]
fn win_rss_kb() -> String {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    // SAFETY: `c` is a valid out-buffer of the size passed.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    if ok == 0 { "?".into() } else { (c.working_set_size / 1024).to_string() }
}

#[cfg(windows)]
fn win_thread_count() -> String {
    #[repr(C)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        thread_id: u32,
        owner: u32,
        base_pri: i32,
        delta_pri: i32,
        flags: u32,
    }
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut std::ffi::c_void;
        fn Thread32First(snap: *mut std::ffi::c_void, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snap: *mut std::ffi::c_void, entry: *mut ThreadEntry) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    const SNAPTHREAD: u32 = 4;
    unsafe {
        let snap = CreateToolhelp32Snapshot(SNAPTHREAD, 0);
        if snap as isize == -1 {
            return "?".into();
        }
        let me = std::process::id();
        let mut entry = ThreadEntry { size: std::mem::size_of::<ThreadEntry>() as u32, usage: 0, thread_id: 0, owner: 0, base_pri: 0, delta_pri: 0, flags: 0 };
        let mut n = 0u64;
        if Thread32First(snap, &mut entry) != 0 {
            loop {
                if entry.owner == me {
                    n += 1;
                }
                if Thread32Next(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        n.to_string()
    }
}
