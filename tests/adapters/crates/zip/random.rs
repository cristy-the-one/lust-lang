// Seeded archives. Each one is a few stored or Deflated entries, sometimes
// a directory, written the same way on both sides. The hex of the archive
// and the inflated bodies must match zip 9.2.0.
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, System, ZipArchive, ZipWriter};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn opts() -> SimpleFileOptions {
    SimpleFileOptions::default().system(System::Unix)
}

fn hex(bytes: &[u8]) -> String {
    let digits = b"0123456789abcdef";
    let mut s = String::new();
    for &b in bytes {
        s.push(digits[(b >> 4) as usize] as char);
        s.push(digits[(b & 0xf) as usize] as char);
    }
    s
}

fn name_of(rng: &mut Rng, dir: bool) -> String {
    let alphabet = b"abcxyz01";
    let n = 1 + rng.below(6) as usize;
    let mut s = String::new();
    if rng.below(5) == 0 {
        s.push_str("n");
        s.push('é');
    }
    let mut i = 0;
    while i < n {
        s.push(alphabet[rng.below(alphabet.len() as u64) as usize] as char);
        i += 1;
    }
    if dir {
        s.push('/');
    }
    s
}

fn payload(rng: &mut Rng) -> Vec<u8> {
    let n = rng.below(24) as usize;
    let mut v = Vec::new();
    let mut i = 0;
    while i < n {
        v.push(rng.below(256) as u8);
        i += 1;
    }
    v
}

fn one(rng: &mut Rng, files: usize) {
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    let mut i = 0;
    while i < files {
        let dir = rng.below(6) == 0;
        let name = name_of(rng, dir);
        if dir {
            let _ = z.add_directory(&name, opts());
        } else {
            let stored = rng.below(2) == 0;
            let mut o = opts();
            if stored {
                o = o.compression_method(CompressionMethod::Stored);
            } else {
                let level = 1 + rng.below(9) as i64;
                o = o.compression_level(Some(level));
            }
            if z.start_file(&name, o).is_err() {
                i += 1;
                continue;
            }
            let body = payload(rng);
            let _ = z.write_all(&body);
        }
        i += 1;
    }
    let cursor = z.finish().unwrap();
    let bytes = cursor.get_ref().clone();
    print!("{} ", hex(&bytes));
    let mut archive = ZipArchive::new(cursor).unwrap();
    let mut j = 0;
    while j < archive.len() {
        let mut f = archive.by_index(j).unwrap();
        let mut body = Vec::new();
        f.read_to_end(&mut body).unwrap();
        print!("{}:{} ", f.name().unwrap(), hex(&body));
        j += 1;
    }
    println!();
}

fn main() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut n = 0;
    while n < 2000 {
        let files = 1 + rng.below(4) as usize;
        one(&mut rng, files);
        n += 1;
    }
}
