// f64::clamp and f32::clamp with bounds that are not float literals:
// in range, below, above, NaN input, equal bounds, signed zero, a
// const item, a const fn, and the path form.
const LO: f64 = 0.0;
const HI: f64 = 1.0;
const MID: f64 = 0.25f64.clamp(LO, HI);
const BELOW: f64 = (-4.0f64).clamp(LO, HI);
const ABOVE: f64 = 6.0f64.clamp(LO, HI);

const fn clamp64(x: f64, lo: f64, hi: f64) -> f64 {
    x.clamp(lo, hi)
}
const fn clamp32(x: f32, lo: f32, hi: f32) -> f32 {
    x.clamp(lo, hi)
}

fn bounds() -> (f64, f64) {
    (-1.5, -0.5)
}

fn main() {
    println!("const {:?} {:?} {:?}", MID, BELOW, ABOVE);
    let lo = 0.0f64;
    let hi = 1.0f64;
    let xs = [-5.0f64, -0.0, 0.0, 0.25, 1.0, 7.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    for &x in xs.iter() {
        println!("f64 {:?} -> {:?} {:?}", x, x.clamp(lo, hi), f64::clamp(x, lo, hi));
    }
    let lo32 = 0.0f32;
    let hi32 = 1.0f32;
    let fs = [-5.0f32, -0.0, 0.5, 9.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
    for &x in fs.iter() {
        println!("f32 {:?} -> {:?} {:?}", x, x.clamp(lo32, hi32), f32::clamp(x, lo32, hi32));
    }
    let b = 0.2f64;
    println!("expr {:?}", 3.5f64.clamp(b + 0.1, b * 10.0));
    let same = 2.5f64;
    println!("eq {:?} {:?}", 1.0f64.clamp(same, same), f64::NAN.clamp(same, same));
    let same32 = 2.5f32;
    println!("eq32 {:?} {:?}", 9.0f32.clamp(same32, same32), f32::NAN.clamp(same32, same32));
    let n0 = -0.0f64;
    let p0 = 0.0f64;
    println!("z {:?} {:?} {:?} {:?}", 0.0f64.clamp(n0, p0), (-0.0f64).clamp(n0, p0), 1.0f64.clamp(n0, p0), (-1.0f64).clamp(n0, p0));
    println!("call {:?} {:?}", clamp64(-1.0, lo, hi), clamp32(4.0, lo32, hi32));
    let f = f64::clamp;
    let g = f32::clamp;
    println!("val {:?} {:?} {:?}", f(3.5, lo, hi), f(-1.0, lo, hi), g(9.0, lo32, hi32));
    let (lo2, hi2) = bounds();
    println!("fn {:?} {:?} {:?}", (-3.0f64).clamp(lo2, hi2), (-1.0f64).clamp(lo2, hi2), 0.0f64.clamp(lo2, hi2));
}
