// Float maths bit for bit: each method std lowers to the platform's C maths
// library (sin, cos, ..., exp, ln, powf, hypot), on f64 and f32, over 3000
// fixed inputs each (edge cases, multiples of pi/4, the asin/acos domain and
// near 1, small, large, and random bit patterns). It prints one hash of the
// result bits per function, so a last-bit difference from rustc's output on
// any input fails the case and names the function. On Windows rustc's std
// imports these from the UCRT; msvcrt's functions of the same names differ
// from it on a few inputs in a hundred (lust before the fix that added this).
const N: usize = 3000;

struct Rng {
    s: u64,
}

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.s = x;
        x
    }
    fn uni(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next() >> 11) as f64 * (1.0 / 9007199254740992.0);
        lo + (hi - lo) * u
    }
}

fn edges() -> Vec<f64> {
    let pi = std::f64::consts::PI;
    let mut v = vec![
        0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 2.0, 1e-300, -1e-300, 5e-324, -5e-324, 1e-8, -1e-8, 1e300, -1e300,
        f64::MAX, f64::MIN, f64::MIN_POSITIVE, 710.0, -745.0, 709.78, 1e6, 1e15, 1e22, f64::INFINITY,
        f64::NEG_INFINITY, f64::NAN, 0.9999999999999999, 1.0000000000000002, -0.9999999999999999, f64::EPSILON,
        1.0 - f64::EPSILON,
    ];
    let mut k = -40;
    while k <= 40 {
        v.push(k as f64 * pi / 2.0);
        v.push(k as f64 * pi / 4.0);
        k += 1;
    }
    v
}

fn input(f: usize, i: usize, rng: &mut Rng, e: &Vec<f64>) -> f64 {
    if i < e.len() {
        return e[i];
    }
    let r = i % 6;
    if r == 0 {
        let x = f64::from_bits(rng.next());
        if x.is_finite() { x } else { 0.25 }
    } else if f == 3 || f == 4 || r == 1 {
        if r == 2 { 1.0 - rng.uni(0.0, 1e-6) } else { rng.uni(-1.0, 1.0) }
    } else if r == 2 {
        rng.uni(-10.0, 10.0)
    } else if r == 3 {
        rng.uni(-1000.0, 1000.0)
    } else if r == 4 {
        rng.uni(0.0, 2.0)
    } else {
        rng.uni(-750.0, 750.0)
    }
}

fn f64_op(f: usize, x: f64, y: f64) -> f64 {
    match f {
        0 => x.sin(),
        1 => x.cos(),
        2 => x.tan(),
        3 => x.asin(),
        4 => x.acos(),
        5 => x.atan(),
        6 => x.atan2(y),
        7 => x.sinh(),
        8 => x.cosh(),
        9 => x.tanh(),
        10 => x.hypot(y),
        11 => x.exp2(),
        12 => x.exp_m1(),
        13 => x.ln_1p(),
        14 => x.to_radians(),
        15 => x.to_degrees(),
        16 => x.exp(),
        17 => x.ln(),
        18 => x.log2(),
        19 => x.log10(),
        20 => x.powf(y),
        _ => x.sqrt(),
    }
}

fn f32_op(f: usize, x: f32, y: f32) -> f32 {
    match f {
        0 => x.sin(),
        1 => x.cos(),
        2 => x.tan(),
        3 => x.asin(),
        4 => x.acos(),
        5 => x.atan(),
        6 => x.atan2(y),
        7 => x.sinh(),
        8 => x.cosh(),
        9 => x.tanh(),
        10 => x.hypot(y),
        11 => x.exp2(),
        12 => x.exp_m1(),
        13 => x.ln_1p(),
        14 => x.to_radians(),
        15 => x.to_degrees(),
        16 => x.exp(),
        17 => x.ln(),
        18 => x.log2(),
        19 => x.log10(),
        20 => x.powf(y),
        _ => x.sqrt(),
    }
}

const NAMES: [&str; 22] = [
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "hypot", "exp2", "exp_m1",
    "ln_1p", "to_radians", "to_degrees", "exp", "ln", "log2", "log10", "powf", "sqrt",
];

fn mix(h: u64, b: u64) -> u64 {
    (h ^ b).wrapping_mul(0x100000001b3)
}

fn main() {
    let e = edges();
    let mut f = 0;
    while f < NAMES.len() {
        let mut rng = Rng { s: 0x9E3779B97F4A7C15 ^ (f as u64 + 1) };
        let (mut h64, mut h32) = (0xcbf29ce484222325u64, 0xcbf29ce484222325u64);
        let mut i = 0;
        while i < N {
            let x = input(f, i, &mut rng, &e);
            let y = if f == 20 { rng.uni(-4.0, 4.0) } else { rng.uni(-100.0, 100.0) };
            h64 = mix(h64, f64_op(f, x, y).to_bits());
            h32 = mix(h32, f32_op(f, x as f32, y as f32).to_bits() as u64);
            i += 1;
        }
        println!("{} f64 {:016x} f32 {:016x}", NAMES[f], h64, h32);
        f += 1;
    }
    // A gear recipe's involute point, printed in full.
    let (rb, t) = (15.0f64, 0.3f64);
    let (px, py) = (rb * (t.cos() + t * t.sin()), rb * (t.sin() - t * t.cos()));
    println!("{:?} {:?} {:?} {:?}", px, py, py.atan2(px).to_degrees(), (px * px + py * py).sqrt().ln().exp());
}
