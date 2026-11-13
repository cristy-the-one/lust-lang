// A NaN lower bound panics the same way min > max does.
// panic: min > max, or either was NaN. min = NaN, max = 1.0
fn main() {
    let lo = f64::NAN;
    let hi = 1.0f64;
    println!("{:?}", 0.5f64.clamp(lo, hi));
}
