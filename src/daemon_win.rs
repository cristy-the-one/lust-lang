// Windows daemon transport: a TCP listener on 127.0.0.1 and a one-line
// address file at the socket path the client already names
// (`.lust-daemon-<build>.sock`, or `LUST_DAEMON_SOCKET`). Unix keeps a
// filesystem socket at that path; this file is the Windows equivalent the
// client reads. Removing the file is the stop request, as on Linux.
//
// Accept waits on a Winsock event so a connection does not add a poll
// interval to the warm path. A second event wakes the wait for Ctrl+C and
// for deletion of the address file.
use std::net::{TcpListener, TcpStream};
use std::os::windows::io::AsRawSocket;
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::Duration;

unsafe extern "system" {
    fn CreateEventW(attrs: *mut std::ffi::c_void, manual: i32, initial: i32, name: *const u16) -> *mut std::ffi::c_void;
    fn SetEvent(event: *mut std::ffi::c_void) -> i32;
    fn WaitForMultipleObjects(count: u32, handles: *const *mut std::ffi::c_void, wait_all: i32, millis: u32) -> u32;
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
    fn WSACreateEvent() -> *mut std::ffi::c_void;
    fn WSAEventSelect(socket: usize, event: *mut std::ffi::c_void, mask: i32) -> i32;
    fn WSAEnumNetworkEvents(socket: usize, event: *mut std::ffi::c_void, events: *mut NetEvents) -> i32;
    fn WSAGetLastError() -> i32;
    fn select(nfds: i32, read: *mut FdSet, write: *mut FdSet, except: *mut FdSet, timeout: *mut Timeval) -> i32;
    fn recv(socket: usize, buf: *mut u8, len: i32, flags: i32) -> i32;
}

#[repr(C)]
struct NetEvents {
    events: i32,
    error: [i32; 10],
}

#[repr(C)]
struct FdSet {
    count: u32,
    socks: [usize; 64],
}

#[repr(C)]
struct Timeval {
    sec: i32,
    usec: i32,
}

const FD_ACCEPT: i32 = 1 << 3;
const WAIT_TIMEOUT: u32 = 258;
const MSG_PEEK: i32 = 2;
const WSAEWOULDBLOCK: i32 = 10035;

static WAKE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe extern "system" fn on_ctrl(ctrl: u32) -> i32 {
    // CTRL_C_EVENT, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_SHUTDOWN_EVENT.
    if matches!(ctrl, 0 | 1 | 2 | 6) {
        super::request_shutdown();
        let wake = WAKE.load(Ordering::SeqCst);
        if !wake.is_null() {
            // Edition 2024 does not treat an unsafe fn body as an unsafe block.
            unsafe { SetEvent(wake) };
        }
        1
    } else {
        0
    }
}

pub struct Listener {
    tcp: TcpListener,
    socket: usize,
    accept_event: *mut std::ffi::c_void,
    wake_event: *mut std::ffi::c_void,
}

// The events are process-lifetime handles. The listener never moves across threads
// except the serve loop, which is one thread; the watcher thread only calls SetEvent.
unsafe impl Send for Listener {}

impl Listener {
    pub fn bind(sock: &Path) -> std::io::Result<Self> {
        let tcp = TcpListener::bind("127.0.0.1:0")?;
        tcp.set_nonblocking(true)?;
        let socket = tcp.as_raw_socket() as usize;
        let accept_event = unsafe { WSACreateEvent() };
        if accept_event.is_null() || accept_event == (-1isize as *mut std::ffi::c_void) {
            return Err(std::io::Error::other("WSACreateEvent failed"));
        }
        if unsafe { WSAEventSelect(socket, accept_event, FD_ACCEPT) } != 0 {
            return Err(std::io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
        }
        let wake_event = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if wake_event.is_null() {
            return Err(std::io::Error::other("CreateEventW failed"));
        }
        WAKE.store(wake_event, Ordering::SeqCst);
        unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
        let port = tcp.local_addr()?.port();
        if let Some(parent) = sock.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = sock.with_extension("tmp");
        std::fs::write(&tmp, format!("127.0.0.1:{port}\n"))?;
        std::fs::rename(&tmp, sock)?;
        let path = sock.to_path_buf();
        let wake = wake_event as usize;
        // Removal of the address file is the stop request. The thread only
        // watches; the serve loop does the cleanup.
        let _ = std::thread::Builder::new().name("lust-sock-watch".into()).spawn(move || {
            while path.exists() {
                std::thread::sleep(Duration::from_millis(50));
            }
            unsafe { SetEvent(wake as *mut std::ffi::c_void) };
        });
        Ok(Listener { tcp, socket, accept_event, wake_event })
    }

    /// True when a client is waiting. False on timeout or on a shutdown wake
    /// (Ctrl+C or the address file disappearing).
    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        let ms = timeout.map_or(0xFFFF_FFFF, |d| d.as_millis().min(0xFFFF_FFFE) as u32);
        let handles = [self.accept_event, self.wake_event];
        let r = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, ms) };
        if r == 0 {
            let mut ev = NetEvents { events: 0, error: [0; 10] };
            unsafe { WSAEnumNetworkEvents(self.socket, self.accept_event, &mut ev) };
            return true;
        }
        let _ = WAIT_TIMEOUT;
        false
    }

    pub fn accept(&self) -> Option<TcpStream> {
        match self.tcp.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                Some(stream)
            }
            Err(_) => None,
        }
    }
}

pub fn stream_token(stream: &TcpStream) -> u64 {
    stream.as_raw_socket()
}

/// True when the client has closed its side. `select` plus a peek, so the
/// socket's blocking mode stays as the serve loop left it.
pub fn peer_gone(token: u64) -> bool {
    let mut set = FdSet { count: 1, socks: [0; 64] };
    set.socks[0] = token as usize;
    let mut tv = Timeval { sec: 0, usec: 0 };
    let ready = unsafe { select(0, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), &mut tv) };
    if ready <= 0 {
        return false;
    }
    let mut buf = [0u8; 1];
    let n = unsafe { recv(token as usize, buf.as_mut_ptr(), 1, MSG_PEEK) };
    n == 0 || (n < 0 && unsafe { WSAGetLastError() } != WSAEWOULDBLOCK)
}

