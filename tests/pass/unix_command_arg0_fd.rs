// CommandExt::arg0 (argv[0], NUL check, both Debug forms) and the OwnedFd
// conversions of UnixStream and Stdio (tf-p6 R11).
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
fn main() {
    let (mut a, b) = UnixStream::pair().unwrap();
    let mut cmd = Command::new("cat");
    cmd.arg0("x").args(&["-"]);
    println!("{:?}", cmd);
    println!("{:#?}", cmd);
    let fd = OwnedFd::from(b);
    cmd.stdin(Stdio::from(fd)).stdout(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    a.write_all(b"hello\n").unwrap();
    drop(a);
    let mut out = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    println!("{:?} {}", out, child.wait().unwrap().success());
    let mut c2 = Command::new("sh");
    c2.arg0("sh").arg("-c").arg("echo $0");
    println!("{:?}", c2);
    println!("{}", String::from_utf8(c2.output().unwrap().stdout).unwrap().trim());
    let mut c3 = Command::new("sh");
    c3.arg0("myname").arg("-c").arg("echo $0");
    println!("{}", String::from_utf8(c3.output().unwrap().stdout).unwrap().trim());
    let mut c4 = Command::new("true");
    c4.arg0("a\0b");
    println!("{:?}", c4.status().map_err(|e| e.to_string()));
    let (s1, s2) = UnixStream::pair().unwrap();
    let back = UnixStream::from(OwnedFd::from(s1));
    drop(s2);
    let mut buf = Vec::new();
    println!("{:?}", (&back).read_to_end(&mut buf).map(|n| n));
}
