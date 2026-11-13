// Literal bounds with min > max are a runtime panic, at the same
// std site as a variable bound.
// panic: min > max, or either was NaN. min = 2.0, max = 1.0
fn main() {
    println!("{:?}", 3.5f64.clamp(2.0, 1.0));
}
