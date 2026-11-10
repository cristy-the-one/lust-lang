// `--max-memory`: each mode holds or churns memory in a different way. The
// mode is a line on stdin (a sandboxed program reads no arguments).
use std::collections::HashMap;
use std::io::Read;

fn main() {
    println!("before");
    let mut mode = String::new();
    std::io::stdin().read_to_string(&mut mode).unwrap();
    match mode.trim() {
        // 200 MiB held in 1 MiB blocks, every byte written.
        "vec" => {
            let mut keep: Vec<Vec<u8>> = Vec::new();
            for i in 0..200usize {
                keep.push(vec![(i % 251) as u8 + 1; 1 << 20]);
            }
            println!("held {} MiB", keep.len());
        }
        // One request of 512 MiB, never written.
        "big" => {
            let v = vec![0u8; 512 << 20];
            println!("len {}", v.len());
        }
        // A hash table that grows past 100 MiB.
        "map" => {
            let mut m = HashMap::new();
            for i in 0..6_000_000u64 {
                m.insert(i, i);
            }
            println!("entries {}", m.len());
        }
        // 2 GiB requested in turn, 1 MiB held at a time.
        "churn" => {
            let mut total = 0usize;
            for i in 0..2048usize {
                let v = vec![(i % 251) as u8 + 1; 1 << 20];
                total += v.len();
            }
            println!("churned {} MiB", total >> 20);
        }
        other => println!("unknown mode {other}"),
    }
}
