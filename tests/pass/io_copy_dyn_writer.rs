// std::io::copy from OS handles into Box<dyn Write> (the ts-rust shape: a socket into a boxed writer).
use std::io::{self, Write};
fn main() {
    let (a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
    b.write_all(&vec![7u8; 20000]).unwrap(); drop(b);
    let mut out: Box<dyn Write + Send> = Box::new(Vec::new());
    println!("{:?}", io::copy(&mut &a, &mut out).unwrap());
    let mut f = std::fs::File::open("/etc/hostname").unwrap();
    let mut o2: Box<dyn Write> = Box::new(io::sink());
    println!("{}", io::copy(&mut f, &mut o2).unwrap() > 0);
}
