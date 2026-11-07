// The runtime helpers that release C runs inline (src/runtime/inl.h): hash table len,
// capacity, init and drop; bounds and Vec::remove checks; UTF-8 decoding; get(range);
// alloc and free; String new, drop and push; starts_with and ends_with; float min and max;
// u64/i64 to f64 casts; 128-bit add, sub and bit ops. Each at the edges where the inline
// path hands over to the runtime function.
use std::collections::{HashMap, HashSet};
use std::hint::black_box;

fn caught(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
    match std::panic::catch_unwind(f) {
        Ok(()) => "no panic".into(),
        Err(p) => match p.downcast::<String>() {
            Ok(s) => *s,
            Err(_) => "not a String".into(),
        },
    }
}

#[repr(align(32))]
#[derive(Debug, Clone, Copy)]
struct A32(u8);

fn main() {
    // Hash tables: empty (no allocation), grown, cleared, dropped empty and full.
    let mut m: HashMap<u32, String> = HashMap::new();
    println!("{} {} {}", m.len(), m.capacity(), m.is_empty());
    for i in 0..20 {
        m.insert(i, format!("v{i}"));
    }
    println!("{} {} {}", m.len(), m.capacity() >= 20, m.is_empty());
    m.clear();
    println!("{} {}", m.len(), m.is_empty());
    let s: HashSet<&str> = HashSet::with_capacity(0);
    println!("{} {}", s.len(), s.capacity());
    drop(s);
    let maps: Vec<HashMap<u8, String>> = (0..3u8)
        .map(|i| {
            let mut h = HashMap::new();
            if i > 0 {
                h.insert(i, i.to_string());
            }
            h
        })
        .collect();
    println!("{:?}", maps.iter().map(|h| h.len()).collect::<Vec<_>>());
    drop(maps);

    // Bounds: in range, then the panics' messages.
    let v = vec![10, 20, 30];
    println!("{}", v[black_box(2usize)]);
    println!("{}", caught(|| println!("{}", vec![1, 2, 3][black_box(3usize)])));
    println!("{}", caught(|| {
        let mut v = vec![1, 2, 3];
        v.remove(black_box(5usize));
    }));
    let mut w = vec![1, 2, 3];
    println!("{} {:?}", w.remove(black_box(2usize)), w);

    // UTF-8: ASCII and 2, 3 and 4 byte chars.
    let t = "aé€😀z";
    println!("{:?}", t.chars().collect::<Vec<_>>());
    println!("{:?}", t.char_indices().collect::<Vec<_>>());
    println!("{} {} {:?}", t.chars().count(), "".chars().count(), "abc".chars().rev().collect::<String>());

    // get(range): each kind of range, in and out of bounds.
    let a = [1, 2, 3, 4, 5];
    let n = black_box(5usize);
    println!("{:?} {:?} {:?} {:?}", a.get(1..3), a.get(1..=4), a.get(..n), a.get(..=n));
    println!("{:?} {:?} {:?} {:?}", a.get(2..), a.get(..), a.get(n..), a.get(n + 1..));
    println!("{:?} {:?} {:?}", a.get(3..2), a.get(0..=n - 1), a.get(n..n));
    println!("{:?} {:?} {:?}", t.get(1..3), t.get(0..2), t.get(3..));

    // Allocation: sizes, zero sizes, over-aligned elements, boxes.
    let big = vec![A32(1), A32(2)];
    println!("{:?} {} {}", big, big[1].0, big.as_ptr() as usize % 32);
    let zs = vec![(); 5];
    let e: Vec<u64> = Vec::with_capacity(0);
    let b = Box::new(5u8);
    let many: Vec<Vec<u16>> = (0..50).map(|i| vec![i; i as usize]).collect();
    println!("{} {} {} {}", zs.len(), e.capacity(), b, many.iter().map(|v| v.len()).sum::<usize>());

    // Strings: new, pushes that fit and that grow, multibyte pushes, drop.
    let mut s = String::new();
    println!("{:?} {}", s, s.capacity());
    s.push_str("");
    s.push_str("hello");
    s.push(' ');
    s.push('é');
    s.push_str(" world");
    let mut f = String::with_capacity(4);
    for c in ['a', 'b', 'c', 'd', 'e', '€'] {
        f.push(c);
    }
    f.push_str("xy");
    println!("{s} {f} {} {}", s.len(), f.len());
    let c = s.clone();
    drop(s);
    println!("{c}");
    for (h, p) in [("hello", ""), ("hello", "he"), ("hello", "hello"), ("he", "hello"), ("hello", "lo"), ("", ""), ("abc", "abd")] {
        println!("{} {}", h.starts_with(p), h.ends_with(p));
    }

    // Float min and max: NaN, infinities, and which finite value wins.
    // `f64::min`/`max` are `minimumNumber`/`maximumNumber` with the nsz
    // exception: of `+0.0` and `-0.0`, either sign may come back, and
    // arm64 does not pick the one x86_64 picks. Zeros are printed positive.
    let show64 = |v: f64| {
        let v = if v == 0.0 { 0.0 } else { v };
        format!("{v:?}{}", if v.is_sign_negative() { "-" } else { "" })
    };
    let vals = [0.0f64, -0.0, 1.5, -2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    for &x in &vals {
        let row: Vec<String> = vals
            .iter()
            .map(|&y| {
                let (lo, hi) = (black_box(x).min(y), black_box(x).max(y));
                format!("{} {}", show64(lo), show64(hi))
            })
            .collect();
        println!("{}", row.join(" "));
    }
    let show32 = |v: f32| if v == 0.0 { 0.0 } else { v };
    let fs = [0.0f32, -0.0, f32::NAN, 2.5];
    for &x in &fs {
        let row: Vec<String> = fs.iter().map(|&y| format!("{:?} {:?}", show32(black_box(x).min(y)), show32(black_box(x).max(y)))).collect();
        println!("{}", row.join(" "));
    }

    // Integer to f64 casts, round to nearest with ties to even.
    let us = [0u64, 1, 1 << 53, (1 << 53) + 1, (1 << 53) + 3, (1 << 63) + 1, u64::MAX - 1, u64::MAX, 12345678901234567890];
    for &u in &us {
        println!("{:?} {:?}", black_box(u) as f64, black_box(u as i64) as f64);
    }
    println!("{:?} {:?}", black_box(i64::MIN) as f64, black_box(i64::MAX) as f64);

    // 128-bit wrapping add and sub, and bit ops.
    let xs = [0u128, 1, u128::MAX, (1u128 << 64) | 7];
    for &x in &xs {
        for &y in &xs {
            let (x, y) = (black_box(x), black_box(y));
            println!("{} {} {} {} {}", x.wrapping_add(y), x.wrapping_sub(y), x & y, x | y, x ^ y);
        }
    }
    let is = [0i128, -1, i128::MIN, i128::MAX];
    for &x in &is {
        for &y in &is {
            let (x, y) = (black_box(x), black_box(y));
            println!("{} {}", x.wrapping_add(y), x.wrapping_sub(y));
        }
    }
}
