// An integer whose type is still open casts to a float, as in rustc: the
// variable defaults to i32 (or takes a later use's type), then converts.
fn main() {
    let mut xs = Vec::new();
    for i in 0..4 {
        xs.push(-6.0 + 4.0 * i as f64);
    }
    let n = 7;
    let half = n as f32 / 2.0;
    let big = 3_000_000_000;
    let wide: u64 = big;
    println!("{:?} {} {} {}", xs, half, 300 as f64, wide as f64);
    let k = 255;
    let byte: u8 = k;
    println!("{} {}", byte as f64, -5 as f64);
}
