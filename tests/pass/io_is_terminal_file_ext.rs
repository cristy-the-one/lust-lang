// io::IsTerminal, os::unix::fs::FileExt (pread/pwrite, read_exact_at EOF,
// EINVAL offsets), and SocketAddrExt::from_abstract_name over AsRef<[u8]>
// (tf-p6 R23); std const_error Debug forms of the SUN_LEN errors.
use std::io::IsTerminal;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::FileExt;
use std::os::unix::net::{SocketAddr, UnixStream};
fn main() {
    println!("{} {} {}", std::io::stdout().is_terminal(), std::io::stdin().is_terminal(), std::io::stderr().lock().is_terminal());
    let e = SocketAddr::from_abstract_name([b'a'; 200]).unwrap_err();
    println!("{:?} | {}", e, e);
    let a = SocketAddr::from_abstract_name("name").unwrap();
    println!("{:?} {:?}", a, a.as_abstract_name());
    let a2 = SocketAddr::from_abstract_name(String::from("n2")).unwrap();
    println!("{:?}", a2);
    let e2 = UnixStream::connect("/nonexistent/x/y/z").unwrap_err();
    println!("{:?}", e2.kind());
    let long = "/".repeat(120);
    let e3 = UnixStream::connect(&long).unwrap_err();
    println!("{:?} | {}", e3, e3);
    let p = std::env::temp_dir().join(format!("fe_{}", std::process::id()));
    std::fs::write(&p, b"hello world").unwrap();
    let f = std::fs::OpenOptions::new().read(true).write(true).open(&p).unwrap();
    let mut b = [0u8; 5];
    let n = f.read_at(&mut b, 6).unwrap();
    println!("{n} {:?}", std::str::from_utf8(&b[..n]));
    println!("{:?}", f.read_exact_at(&mut b, 8).map_err(|e| format!("{e:?} {e}")));
    f.write_all_at(b"WORLD", 6).unwrap();
    println!("{:?} {:?}", f.write_at(b"!", 11), std::fs::read_to_string(&p));
    let mut big = [0u8; 3];
    println!("{:?}", f.read_at(&mut big, u64::MAX).map_err(|e| e.kind()));
    println!("{:?}", f.read_at(&mut big, 100));
    println!("{}", f.is_terminal());
    let o: std::os::fd::OwnedFd = f.into();
    println!("{}", o.is_terminal());
    std::fs::remove_file(&p).unwrap();
}
