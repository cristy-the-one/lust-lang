// base64 0.22.1: encode and decode on the four preset engines, including
// empty input, padding lengths, non-ASCII bytes, and a large buffer.
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::prelude::*;

fn fill(n: usize) -> Vec<u8> {
    let mut v = Vec::new();
    let mut x = 0x9e37_79b9u32;
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push((x >> 16) as u8);
    }
    v
}

fn round(name: &str, enc: &str, raw: &[u8], back: Result<Vec<u8>, base64::DecodeError>) {
    match back {
        Ok(v) if v == raw => println!("{name} {enc} ok {}", v.len()),
        Ok(v) => println!("{name} {enc} mismatch {v:?}"),
        Err(e) => println!("{name} {enc} err {e} | {e:?}"),
    }
}

fn main() {
    let samples: Vec<Vec<u8>> = vec![
        vec![],
        vec![0],
        b"f".to_vec(),
        b"fo".to_vec(),
        b"foo".to_vec(),
        b"foobar".to_vec(),
        vec![0xff],
        vec![0xff, 0xec, 0x20, 0x55, 0],
        vec![0, 1, 127, 128, 255, 0xc3, 0xa9],
        "héllo".as_bytes().to_vec(),
        fill(31),
        fill(32),
        fill(33),
        fill(1000),
        fill(8192),
    ];
    for raw in &samples {
        let enc = STANDARD.encode(raw);
        round("std", &enc, raw, STANDARD.decode(enc.as_bytes()));
        let enc = STANDARD_NO_PAD.encode(raw);
        round("np", &enc, raw, STANDARD_NO_PAD.decode(enc.as_bytes()));
        let enc = URL_SAFE.encode(raw);
        round("url", &enc, raw, URL_SAFE.decode(enc.as_bytes()));
        let enc = URL_SAFE_NO_PAD.encode(raw);
        round("urlnp", &enc, raw, URL_SAFE_NO_PAD.decode(enc.as_bytes()));
    }
    println!("str {}", STANDARD.encode("hi"));
    println!("vec {}", STANDARD.encode(vec![104u8, 105]));
    println!("arr {}", STANDARD.encode([104u8, 105, 33]));
    println!("pre {}", BASE64_STANDARD.encode(b"hi"));
    println!("pre-url {}", BASE64_URL_SAFE_NO_PAD.encode(b"\xff\xef\x00"));
}
