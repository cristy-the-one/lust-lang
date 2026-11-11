// expect: L9001
// Valid Rust. With bounds that are not literals rustc may panic with a
// message lust does not reproduce yet, so lust rejects the call.
fn main() {
    let lo = 0.0f64;
    let hi = 1.0f64;
    println!("{:?}", 3.5f64.clamp(lo, hi));
}
