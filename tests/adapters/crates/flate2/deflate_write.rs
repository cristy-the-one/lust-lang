// flate2 1.1.10 write::DeflateEncoder: raw deflate bytes (no zlib header)
// at the levels zip asks for, including an empty payload and short writes
// into the inner Vec. The zip adapter finishes one of these per Deflated entry.
use std::io::Write;

fn fnv(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn once(level: u32, data: &[u8]) -> (u64, usize, u64, u64) {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
    e.write_all(data).unwrap();
    let n_in = e.total_in();
    let n_out = e.total_out();
    let z = e.finish().unwrap();
    (fnv(&z), z.len(), n_in, n_out)
}

fn main() {
    let samples: [&[u8]; 4] = [b"", b"hello 3mf\n", b"a", b"0123456789abcdef0123456789abcdef"];
    for level in [1u32, 6, 9] {
        for data in samples {
            let (h, n, inn, out) = once(level, data);
            println!("L{level} {} {h:016x} {n} {inn} {out}", data.len());
        }
    }
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    let text = b"chunked-deflate-payload";
    let mut i = 0;
    while i < text.len() {
        let n = e.write(&text[i..i + 1]).unwrap();
        i += n;
    }
    let z = e.finish().unwrap();
    println!("chunks {:016x} {}", fnv(&z), z.len());
}
