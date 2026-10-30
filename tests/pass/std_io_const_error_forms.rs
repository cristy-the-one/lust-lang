// std const_error! forms (Debug `Error { kind, message }`) of the zero
// timeout and socket address errors (tf-p6); OpenOptions keeps its Custom form.
use std::net::ToSocketAddrs;
fn main() {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    println!("{:?}", a.set_read_timeout(Some(std::time::Duration::ZERO)));
    println!("{:?}", std::fs::OpenOptions::new().open("/tmp"));
    println!("{:?}", std::fs::OpenOptions::new().read(true).truncate(true).open("/tmp/x"));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let s = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
    println!("{:?}", s.set_read_timeout(Some(std::time::Duration::ZERO)));
    println!("{:?}", "nocolon".to_socket_addrs().map(|_| ()));
    println!("{:?}", "localhost:99999".to_socket_addrs().map(|_| ()));
}
