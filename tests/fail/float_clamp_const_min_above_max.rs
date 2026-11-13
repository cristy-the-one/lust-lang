// expect: L0630
// rustc E0080: const evaluation of clamp panics with
// "min > max, or either was NaN" and does not format the bounds.
const LO: f64 = 2.0;
const HI: f64 = 1.0;
const A: f64 = 0.5f64.clamp(LO, HI);
fn main() {
    println!("{A}");
}
