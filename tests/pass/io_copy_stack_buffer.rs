// std::io::copy on std's stack_buffer_copy path: 8 KiB reads (the buffer zeroed once, then holding the earlier bytes), Interrupted retried, write_all per chunk, errors returned.
use std::io::{self, Read, Write};
struct Src { left: usize, calls: u32 }
impl Read for Src {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        println!("read {} first={} calls={}", buf.len(), buf[0], self.calls);
        if self.calls == 2 { return Err(io::Error::new(io::ErrorKind::Interrupted, "again")); }
        let n = self.left.min(3000);
        for b in &mut buf[..n] { *b = b'a' + (self.calls % 26) as u8; }
        self.left -= n;
        Ok(n)
    }
}
struct Sink2 { got: Vec<usize> }
impl Write for Sink2 {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> { let n = buf.len().min(1000); self.got.push(n); Ok(n) }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
struct Fail;
impl Write for Fail {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> { Err(io::Error::other("nope")) }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
fn main() {
    let mut s = Src { left: 7000, calls: 0 };
    let mut w = Sink2 { got: Vec::new() };
    let n = io::copy(&mut s, &mut w).unwrap();
    println!("{n} {:?}", w.got);
    let mut s = Src { left: 10, calls: 0 };
    println!("{:?}", io::copy(&mut s, &mut Fail).map_err(|e| e.to_string()));
    let data = b"hello world".to_vec();
    let mut r: &[u8] = &data;
    let mut out: Vec<u8> = Vec::new();
    println!("{:?} {:?}", io::copy(&mut &mut r, &mut &mut out).unwrap(), out.len());
    let mut v = Vec::new();
    println!("{:?} {:?}", io::copy(&mut Src { left: 5, calls: 0 }, &mut &mut v).unwrap(), v);
    let (a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
    b.write_all(b"xyz").unwrap(); drop(b);
    let mut sink = io::sink();
    println!("{:?}", io::copy(&mut &a, &mut sink).unwrap());
}
