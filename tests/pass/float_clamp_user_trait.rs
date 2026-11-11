// A user trait's `clamp` on f64. rustc's probe finds the inherent
// method first, so the trait body is not called.
trait Bound {
    fn clamp(self, lo: Self, hi: Self) -> Self;
}
impl Bound for f64 {
    fn clamp(self, _lo: f64, _hi: f64) -> f64 {
        42.0
    }
}
fn main() {
    println!("{:?}", 0.25f64.clamp(0.0, 1.0));
    println!("{:?}", 3.5f64.clamp(0.0, 1.0));
}
