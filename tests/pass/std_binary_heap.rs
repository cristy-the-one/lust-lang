// std's BinaryHeap: std's sift rules, so Debug, as_slice, into_vec and
// into_sorted_vec show std's element order; Reverse makes a min-heap; Clone.
use std::cmp::Reverse;
use std::collections::BinaryHeap;

fn main() {
    let mut h: BinaryHeap<u32> = BinaryHeap::new();
    for x in [5u32, 1, 8, 3, 9, 2, 8, 7, 4, 6, 0] {
        h.push(x);
    }
    println!("{:?}", h);
    println!("{:?} {}", h.peek(), h.len());
    let mut out = Vec::new();
    while let Some(x) = h.pop() {
        out.push(x);
        if out.len() == 4 {
            println!("{:?}", h);
        }
    }
    println!("{:?}", out);
    let mut m: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    m.push(Reverse((3, 1)));
    m.push(Reverse((1, 9)));
    m.push(Reverse((2, 5)));
    m.push(Reverse((1, 2)));
    while let Some(Reverse((a, b))) = m.pop() {
        print!("{a}:{b} ");
    }
    println!();
    let v = BinaryHeap::from(vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5]);
    println!("{:?}", v.as_slice());
    println!("{:?}", BinaryHeap::from(v.as_slice().to_vec()).into_sorted_vec());
    println!("{:?}", v.into_vec());
    let mut m2: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    for (a, b) in [(3, 1), (1, 9), (2, 5), (1, 2)] {
        m2.push(Reverse((a, b)));
    }
    let m3 = m2.clone();
    println!("{:?} {} {:?}", m3, m3.len(), m3.peek());
    let s: BinaryHeap<String> = BinaryHeap::from(vec![String::from("b"), String::from("a"), String::from("c")]);
    println!("{:?}", s.clone().into_sorted_vec());
    let mut e: BinaryHeap<i64> = BinaryHeap::with_capacity(4);
    println!("{} {:?} {:?}", e.is_empty(), e.pop(), e.peek());
    e.push(-1);
    e.clear();
    println!("{}", e.len());
}
