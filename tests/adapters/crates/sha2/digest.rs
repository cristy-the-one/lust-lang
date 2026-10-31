// sha2 0.10.9: SHA-256 one-shot and streaming digests, including empty
// input, non-ASCII bytes, block edges, and large inputs. Hex text is
// the bytes printed with {:02x}; Debug and indexing are the byte view.
use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
    let mut s = String::new();
    for b in bytes {
        s.push_str(&format!("{:02x}", *b));
    }
    s
}

fn hex_upper(bytes: &[u8]) -> String {
    let mut s = String::new();
    for b in bytes {
        s.push_str(&format!("{:02X}", *b));
    }
    s
}

macro_rules! show {
    ($label:expr, $out:expr) => {{
        let out = $out;
        println!("{} {}", $label, hex(out.as_slice()));
        println!("{} U {}", $label, hex_upper(out.as_slice()));
        println!("{} D {:?}", $label, out);
        println!("{} S {:?}", $label, out.as_slice());
        println!("{} i {} {}", $label, out[0], out[out.len() - 1]);
        println!("{} r {:?}", $label, &out[..4]);
    }};
}

fn fill(n: usize) -> Vec<u8> {
    let mut v = Vec::new();
    let mut x = 0x1234_5678u32;
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push((x >> 24) as u8);
    }
    v
}

fn main() {
    show!("empty", Sha256::digest(b""));
    show!("abc", Sha256::digest(b"abc"));
    show!("hello", Sha256::digest(b"hello world"));
    let text = "héllo — 日本語";
    show!("utf8", Sha256::digest(text));
    let mut raw = Vec::new();
    for b in 0..=255u8 {
        raw.push(b);
    }
    show!("bytes", Sha256::digest(&raw));
    for n in [0usize, 1, 2, 3, 55, 56, 63, 64, 65, 119, 120, 127, 128, 129, 1000, 4096, 20000] {
        let data = fill(n);
        let one = Sha256::digest(&data);
        let mut h = Sha256::new();
        let mut i = 0;
        while i < data.len() {
            let step = 1 + (i % 70);
            let end = (i + step).min(data.len());
            h.update(&data[i..end]);
            h.update(b"");
            i = end;
        }
        let streamed = h.finalize();
        println!("len {n} {} {}", hex(one.as_slice()), hex(streamed.as_slice()));
    }
    let mut h = Sha256::new();
    h.update("hello");
    h.update([b' ', b'w']);
    h.update(vec![b'o', b'r', b'l', b'd']);
    show!("parts", h.finalize());
    let mut h = Sha256::new();
    h.update(b"abc");
    let copy = h.clone();
    h.update(b"def");
    show!("clone", copy.finalize());
    show!("cloned-more", h.finalize());
    let mut h = Sha256::new();
    h.update(b"zzz");
    h.reset();
    h.update(b"abc");
    show!("reset", h.finalize());
    let d: Sha256 = Default::default();
    show!("default", d.finalize());
}
