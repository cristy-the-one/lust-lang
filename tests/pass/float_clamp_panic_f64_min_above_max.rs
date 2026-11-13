// Variable f64 bounds with min > max. The message names both bounds
// with Debug (`1e20`, not a fixed-point spelling).
// panic: min > max, or either was NaN. min = 1e20, max = 1.0
fn main() {
    let lo = 1.0e20f64;
    let hi = 1.0f64;
    println!("ok {:?}", 0.5f64.clamp(0.0, hi));
    println!("{:?}", 0.0f64.clamp(lo, hi));
}
