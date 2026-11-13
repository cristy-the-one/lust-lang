// Variable f32 bounds with min > max.
// panic: min > max, or either was NaN. min = 1.5, max = -2.25
fn main() {
    let lo = 1.5f32;
    let hi = -2.25f32;
    println!("{:?}", 0.0f32.clamp(lo, hi));
}
