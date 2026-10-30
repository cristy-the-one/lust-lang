// std's ByteStr Debug where std keeps bytes as CStrings or ByteStrs: a
// Command's program, argv[0], args (alternate form), and cwd; an abstract
// socket name (tf-p6). `'` is `\'`, controls `\xNN`, NUL `\0`,
// valid non-ASCII text by char::escape_debug, invalid bytes `\xNN`.
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::SocketAddr;
use std::os::unix::process::CommandExt;
use std::process::Command;
fn main() {
    for t in ["it's", "tab\there", "q\"uote", "caf\u{e9}", "\u{301}x", "\x7f\x01"] {
        let mut c = Command::new(t);
        c.arg(t).current_dir(t);
        println!("{:?}", c);
        println!("{:#?}", c);
        let mut d = Command::new("p");
        d.arg0(t).arg(t).env("K", t);
        println!("{:?}", d);
        println!("{:?}", SocketAddr::from_abstract_name(t).unwrap());
    }
    let p = std::ffi::OsStr::from_bytes(b"x\xff\xfey");
    let mut c = Command::new(p);
    c.arg0(p).arg(p);
    println!("{:?}", c);
    println!("{:?}", SocketAddr::from_abstract_name(b"x\xff\xc3y\0").unwrap());
}
