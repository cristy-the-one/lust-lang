// f64::clamp and f32::clamp with literal bounds: below, inside, above,
// NaN (stays NaN), infinities, and the sign of zero.
fn main() {
    let xs = [-5.0f64, -0.0, 0.0, 0.25, 1.0, 7.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    for &x in xs.iter() {
        println!("{:?} -> {:?} {:?} {:?}", x, x.clamp(0.0, 1.0), x.clamp(-120.0, 120.0), x.clamp(-1.5, -0.5));
    }
    let fs = [-5.0f32, -0.0, 0.5, 9.0, f32::NAN];
    for &x in fs.iter() {
        println!("f32 {:?} -> {:?} {:?}", x, x.clamp(0.0, 1.0), x.clamp(2.5, 2.5));
    }
    let mut acc = 0.0f64;
    for i in 0..10 {
        acc = (acc + i as f64 * 0.7).clamp(-3.0, 3.0);
    }
    println!("{:?}", acc);
}
