// expect: L9001
// Valid Rust. rustc compiles this program (it panics only when run,
// when min > max), so lust rejects the call.
fn main() {
    println!("{:?}", 3.5f64.clamp(2.0, 1.0));
}
