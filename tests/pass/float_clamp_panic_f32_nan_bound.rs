// A NaN upper bound on f32, including a negative zero lower bound.
// panic: min > max, or either was NaN. min = -0.0, max = NaN
fn main() {
    let lo = -0.0f32;
    let hi = f32::NAN;
    println!("{:?}", 0.5f32.clamp(lo, hi));
}
