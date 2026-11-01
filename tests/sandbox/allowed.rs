// A `--sandbox` program that keeps every allowed service: stdin, stdout,
// stderr, the clock, sleeping, path text, and exit.
use std::io::{BufRead, Write};
use std::path::Path;

fn main() {
    let mut words = 0usize;
    let mut lines = 0usize;
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        lines += 1;
        words += line.split_whitespace().count();
    }
    std::thread::sleep(std::time::Duration::from_millis(1));
    let t = std::time::Instant::now();
    let p = Path::new("/a/b.rs");
    let mut out = std::io::stdout().lock();
    writeln!(out, "{lines} {words} {:?} {} {}", option_env!("LUST_SANDBOX_PROBE"), p.is_absolute(), p.extension().unwrap().to_str().unwrap()).unwrap();
    eprintln!("elapsed is small: {}", t.elapsed().as_secs() < 5);
    drop(out);
    if words > 3 {
        std::process::exit(3);
    }
}
