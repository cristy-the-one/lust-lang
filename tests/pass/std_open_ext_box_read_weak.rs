// OpenOptionsExt::mode and custom_flags (O_PATH, O_DIRECTORY) with std's
// OpenOptions Debug; Read and BufRead through Box<dyn BufRead + Send> and
// Box<dyn Read>; sync::Weak::upgrade as a fn value into an Arc<dyn Tr>.
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::sync::{Arc, OnceLock, Weak};

trait Tr {
    fn n(&self) -> u8;
}
struct I;
impl Tr for I {
    fn n(&self) -> u8 {
        4
    }
}
struct Holder {
    me: OnceLock<Weak<dyn Tr + Send + Sync>>,
}
impl Holder {
    fn get(&self) -> Arc<dyn Tr + Send + Sync> {
        self.me.get().and_then(Weak::upgrade).expect("alive")
    }
}

fn read_full<R: Read + ?Sized>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn main() {
    let dir = std::env::temp_dir().join(format!("lust-open-ext-{}", std::process::id()));
    let _ = std::fs::create_dir(&dir);
    let p = dir.join("f");
    let mut o = OpenOptions::new();
    o.write(true).create(true).mode(0o600);
    println!("{:?}", o);
    let f = o.open(&p).unwrap();
    println!("{:o}", f.metadata().unwrap().permissions().mode() & 0o777);
    drop(f);
    println!("{:?}", OpenOptions::new().read(true).custom_flags(0o10000000).open(&p).map(|_| ()));
    println!("{:?}", OpenOptions::new().read(true).custom_flags(0o200000).open(&p).map(|_| ()));
    println!("{:?}", OpenOptions::new().read(true).custom_flags(3).mode(0o100755));
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_dir(&dir);

    let mut r: Box<dyn BufRead + Send> = Box::new(BufReader::new(&b"hello\nworld"[..]));
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let mut buf = [0u8; 3];
    println!("{:?} {:?} {:?}", line, read_full(&mut r, &mut buf), buf);
    let mut b: Box<dyn Read> = Box::new(&b"xyz"[..]);
    let mut v = Vec::new();
    println!("{:?} {:?}", b.read_to_end(&mut v), v);

    weak();
}

fn weak() {
    let a: Arc<dyn Tr + Send + Sync> = Arc::new(I);
    let h = Holder { me: OnceLock::new() };
    let _ = h.me.set(Arc::downgrade(&a));
    println!("{}", h.get().n());
}
