// Randomized bit compare against libm 0.2.16. The seed is fixed, so
// cargo and lust see the same inputs. A few thousand uniform bit
// patterns per function, plus signed zero, NaNs, infinities,
// subnormals, and values at and beside multiples of pi/2.
fn xd(name: &str, x: f64, y: f64) {
    println!("d {name} {:016x} {:016x}", x.to_bits(), y.to_bits());
}
fn xs(name: &str, x: f32, y: f32) {
    println!("s {name} {:08x} {:08x}", x.to_bits(), y.to_bits());
}
fn yd(name: &str, a: f64, b: f64, y: f64) {
    println!("d {name} {:016x} {:016x} {:016x}", a.to_bits(), b.to_bits(), y.to_bits());
}
fn ys(name: &str, a: f32, b: f32, y: f32) {
    println!("s {name} {:08x} {:08x} {:08x}", a.to_bits(), b.to_bits(), y.to_bits());
}

fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x = x ^ (x << 13);
    x = x ^ (x >> 7);
    x = x ^ (x << 17);
    *state = x;
    x
}

fn unary_f64(x: f64) {
    xd("sqrt", x, libm::sqrt(x));
    xd("sin", x, libm::sin(x));
    xd("cos", x, libm::cos(x));
    xd("tan", x, libm::tan(x));
    let (sn, cs) = libm::sincos(x);
    xd("sincos_s", x, sn);
    xd("sincos_c", x, cs);
    xd("asin", x, libm::asin(x));
    xd("acos", x, libm::acos(x));
    xd("atan", x, libm::atan(x));
    xd("floor", x, libm::floor(x));
    xd("ceil", x, libm::ceil(x));
    xd("trunc", x, libm::trunc(x));
    xd("round", x, libm::round(x));
    xd("fabs", x, libm::fabs(x));
    xd("exp", x, libm::exp(x));
    xd("log", x, libm::log(x));
    xd("log2", x, libm::log2(x));
    xd("log10", x, libm::log10(x));
    xd("libm_sin", x, libm::Libm::<f64>::sin(x));
    let n = ((x.to_bits() % 4001) as i32) - 2000;
    xd("scalbn", x, libm::scalbn(x, n));
}

fn unary_f32(x: f32) {
    xs("sqrtf", x, libm::sqrtf(x));
    xs("sinf", x, libm::sinf(x));
    xs("cosf", x, libm::cosf(x));
    xs("tanf", x, libm::tanf(x));
    let (sn, cs) = libm::sincosf(x);
    xs("sincosf_s", x, sn);
    xs("sincosf_c", x, cs);
    xs("asinf", x, libm::asinf(x));
    xs("acosf", x, libm::acosf(x));
    xs("atanf", x, libm::atanf(x));
    xs("floorf", x, libm::floorf(x));
    xs("ceilf", x, libm::ceilf(x));
    xs("truncf", x, libm::truncf(x));
    xs("roundf", x, libm::roundf(x));
    xs("fabsf", x, libm::fabsf(x));
    xs("expf", x, libm::expf(x));
    xs("logf", x, libm::logf(x));
    xs("log2f", x, libm::log2f(x));
    xs("log10f", x, libm::log10f(x));
    xs("libm_cos", x, libm::Libm::<f32>::cos(x));
    let n = ((x.to_bits() % 401) as i32) - 200;
    xs("scalbnf", x, libm::scalbnf(x, n));
}

fn pair_f64(y: f64, x: f64) {
    yd("atan2", y, x, libm::atan2(y, x));
    yd("hypot", y, x, libm::hypot(y, x));
    yd("fmod", y, x, libm::fmod(y, x));
    yd("pow", y, x, libm::pow(y, x));
    yd("copysign", y, x, libm::copysign(y, x));
    yd("fmin", y, x, libm::fmin(y, x));
    yd("fmax", y, x, libm::fmax(y, x));
}

fn pair_f32(y: f32, x: f32) {
    ys("atan2f", y, x, libm::atan2f(y, x));
    ys("hypotf", y, x, libm::hypotf(y, x));
    ys("fmodf", y, x, libm::fmodf(y, x));
    ys("powf", y, x, libm::powf(y, x));
    ys("copysignf", y, x, libm::copysignf(y, x));
    ys("fminf", y, x, libm::fminf(y, x));
    ys("fmaxf", y, x, libm::fmaxf(y, x));
}

fn main() {
    let pi = 3.14159265358979323846_f64;
    let mut edges: Vec<f64> = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        f64::from_bits(1),
        f64::from_bits(1 << 63 | 1),
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::MIN,
        f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -f64::NAN,
        f64::from_bits(0x7ff0_0000_0000_0001),
        f64::from_bits(0xfff0_0000_0000_0001),
        1.0e-300,
        -1.0e-300,
        1.0e16,
        1.0e300,
    ];
    let mut k: i32 = 0;
    while k < 32 {
        let base = (k as f64) * pi / 2.0;
        edges.push(base);
        edges.push(-base);
        edges.push(f64::from_bits(base.to_bits().wrapping_add(1)));
        edges.push(f64::from_bits(base.to_bits().wrapping_sub(1)));
        edges.push(f64::from_bits(base.to_bits().wrapping_add(100)));
        edges.push(f64::from_bits(base.to_bits().wrapping_sub(100)));
        k = k + 1;
    }
    for step in [100i32, 1000, 10_000, 1_000_000] {
        let base = (step as f64) * pi / 2.0;
        edges.push(base);
        edges.push(-base);
        edges.push(f64::from_bits(base.to_bits().wrapping_add(1)));
        edges.push(f64::from_bits(base.to_bits().wrapping_sub(1)));
    }
    for &x in &edges {
        unary_f64(x);
    }
    let pair_edges = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        2.5,
        f64::from_bits(1),
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -f64::NAN,
        pi / 2.0,
        -pi / 2.0,
        1.0e20,
    ];
    for &y in &pair_edges {
        for &x in &pair_edges {
            pair_f64(y, x);
        }
    }

    let mut state: u64 = 0x1234_5678_9abc_def1;
    let mut i: i32 = 0;
    while i < 4096 {
        let bits = xorshift(&mut state);
        unary_f64(f64::from_bits(bits));
        let bits2 = xorshift(&mut state);
        pair_f64(f64::from_bits(bits), f64::from_bits(bits2));
        i = i + 1;
    }

    let pif = 3.14159274_f32;
    let mut edgesf: Vec<f32> = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        f32::from_bits(1),
        f32::from_bits(1 << 31 | 1),
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        f32::MIN,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        -f32::NAN,
        f32::from_bits(0x7f80_0001),
        1.0e-20,
        1.0e20,
    ];
    k = 0;
    while k < 16 {
        let base = (k as f32) * pif / 2.0;
        edgesf.push(base);
        edgesf.push(-base);
        edgesf.push(f32::from_bits(base.to_bits().wrapping_add(1)));
        edgesf.push(f32::from_bits(base.to_bits().wrapping_sub(1)));
        k = k + 1;
    }
    for &x in &edgesf {
        unary_f32(x);
    }
    let pair_edgesf = [
        0.0_f32,
        -0.0,
        1.0,
        -1.0,
        0.5,
        f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        -f32::NAN,
        pif / 2.0,
        1.0e16,
    ];
    for &y in &pair_edgesf {
        for &x in &pair_edgesf {
            pair_f32(y, x);
        }
    }
    let mut i: i32 = 0;
    while i < 4096 {
        let bits = xorshift(&mut state);
        let x = f32::from_bits(bits as u32);
        unary_f32(x);
        let bits2 = xorshift(&mut state);
        pair_f32(x, f32::from_bits(bits2 as u32));
        i = i + 1;
    }
}
