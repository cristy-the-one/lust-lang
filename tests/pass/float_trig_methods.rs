// Trigonometry and kin on f64 and f32: std calls the platform's C math
// library for these (cmath), so lust's output matches rustc's bit for bit
// on the same platform. to_radians and to_degrees are std's constants.
fn main() {
    let xs = [0.0f64, -0.0, 0.5, -1.25, 1.0, 3.0, 1e-8, 100.0, f64::NAN, f64::INFINITY];
    for &x in xs.iter() {
        println!("{:?} sin {:?} cos {:?} tan {:?} atan {:?}", x, x.sin(), x.cos(), x.tan(), x.atan());
        println!("{:?} asin {:?} acos {:?} sinh {:?} cosh {:?} tanh {:?}", x, x.asin(), x.acos(), x.sinh(), x.cosh(), x.tanh());
        println!("{:?} exp2 {:?} exp_m1 {:?} ln_1p {:?}", x, x.exp2(), x.exp_m1(), x.ln_1p());
        println!("{:?} rad {:?} deg {:?} atan2 {:?} hypot {:?}", x, x.to_radians(), x.to_degrees(), x.atan2(-2.0), x.hypot(4.0));
    }
    let fs = [0.0f32, 0.5, -1.25, 3.0, 100.0];
    for &x in fs.iter() {
        println!("f32 {:?} {:?} {:?} {:?} {:?}", x, x.sin(), x.cos(), x.atan2(1.5), x.hypot(2.0));
        println!("f32 {:?} {:?} {:?}", x, x.to_radians(), x.to_degrees());
    }
    // An involute point, as a gear recipe computes it.
    let rb = 15.0f64;
    let t = 0.3f64;
    let (px, py) = (rb * (t.cos() + t * t.sin()), rb * (t.sin() - t * t.cos()));
    println!("{:?} {:?} {:?}", px, py, py.atan2(px).to_degrees());
    println!("{:?}", std::f64::consts::PI.sin());
}
