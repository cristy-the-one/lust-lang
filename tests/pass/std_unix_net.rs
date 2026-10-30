// std::os::unix::net: pair, bind/connect/accept by pathname and
// abstract name, addresses, shutdown, and io errors.
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};

fn main() {
    let (mut a, mut b) = UnixStream::pair().unwrap();
    a.write_all(b"hi").unwrap();
    let mut buf = [0u8; 4];
    let n = b.read(&mut buf).unwrap();
    println!("{} {}", n, String::from_utf8_lossy(&buf[..n]));
    println!("{} {}", a.local_addr().unwrap().is_unnamed(), a.peer_addr().unwrap().is_unnamed());
    let named = SocketAddr::from_pathname("rel.sock").unwrap();
    println!("{:?}", named);
    let abs = SocketAddr::from_abstract_name(b"lust-test").unwrap();
    println!("{:?} {:?}", abs, abs.as_abstract_name().map(|x| String::from_utf8_lossy(x).into_owned()));
    println!("{:?}", SocketAddr::from_pathname("x".repeat(200)).map_err(|e| e.kind()));
    let dir = std::env::temp_dir().join(format!("lust-unix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.sock");
    let l = UnixListener::bind(&path).unwrap();
    let local = l.local_addr().unwrap();
    println!("{} {}", local.as_pathname().map_or(false, |p| p == path.as_path()), local.is_unnamed());
    println!("{:?}", UnixListener::bind(&path).map_err(|e| e.kind()));
    let mut c = UnixStream::connect(&path).unwrap();
    let (mut s, peer) = l.accept().unwrap();
    println!("{}", peer.is_unnamed());
    c.write_all(b"ping").unwrap();
    let n = s.read(&mut buf).unwrap();
    println!("{} {}", n, String::from_utf8_lossy(&buf[..n]));
    c.shutdown(Shutdown::Write).unwrap();
    println!("{}", s.read(&mut buf).unwrap());
    println!("{:?}", UnixStream::connect(dir.join("missing")).map_err(|e| e.kind()));
    let al = UnixListener::bind_addr(&abs).unwrap();
    println!("{}", al.local_addr().unwrap().as_abstract_name().map_or(false, |x| String::from_utf8_lossy(x) == "lust-test"));
    let mut ac = UnixStream::connect_addr(&abs).unwrap();
    let (mut asrv, _) = al.accept().unwrap();
    ac.write_all(b"z").unwrap();
    println!("{}", asrv.read(&mut buf).unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}
