// capability: rejected: `clamp` on `f64` or `f32` unless both bounds are float literals with min <= max
// expect: L9001

fn main() {
    let lo = 0.0f64;
    let hi = 1.0f64;
    println!("{:?}", 3.5f64.clamp(lo, hi));
}
