// Bit-exact libm 0.2.16. Hex is the raw f64/f32 bits, so -0 and NaN
// payloads compare as well as ordinary values.
fn d(name: &str, x: f64) {
    println!("d {name} {:016x}", x.to_bits());
}
fn s(name: &str, x: f32) {
    println!("s {name} {:08x}", x.to_bits());
}

fn main() {
    let pi = 3.14159265358979323846_f64;
    let mut xs: Vec<f64> = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        2.0,
        -2.0,
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
        1.0e-300,
        -1.0e-300,
        1.0e16,
        -1.0e16,
        1.0e300,
        -1.0e300,
        8.0e18,
    ];
    for k in [0i32, 1, 2, 3, 4, 5, 7, 20, 100, 1000, 1_000_000] {
        let base = (k as f64) * pi / 2.0;
        xs.push(base);
        xs.push(-base);
        xs.push(base + f64::from_bits(0x3cb0_0000_0000_0000));
        xs.push(base - f64::from_bits(0x3cb0_0000_0000_0000));
    }
    for &x in &xs {
        d("sqrt", libm::sqrt(x));
        d("sin", libm::sin(x));
        d("cos", libm::cos(x));
        d("tan", libm::tan(x));
        let (sn, cs) = libm::sincos(x);
        d("sincos_s", sn);
        d("sincos_c", cs);
        d("asin", libm::asin(x));
        d("acos", libm::acos(x));
        d("atan", libm::atan(x));
        d("floor", libm::floor(x));
        d("ceil", libm::ceil(x));
        d("trunc", libm::trunc(x));
        d("round", libm::round(x));
        d("fabs", libm::fabs(x));
        d("exp", libm::exp(x));
        d("log", libm::log(x));
        d("log2", libm::log2(x));
        d("log10", libm::log10(x));
        d("libm_sin", libm::Libm::<f64>::sin(x));
        for n in [-2000, -53, -1, 0, 1, 52, 1023, 2000] {
            d(&format!("scalbn_{n}"), libm::scalbn(x, n));
        }
    }
    let pairs = [
        0.0, -0.0, 1.0, -1.0, 0.5, 2.5, f64::from_bits(1), f64::INFINITY, f64::NEG_INFINITY, f64::NAN,
        -f64::NAN, 1.0e20, 3.0, pi / 2.0,
    ];
    for &y in &pairs {
        for &x in &pairs {
            d("atan2", libm::atan2(y, x));
            d("hypot", libm::hypot(y, x));
            d("fmod", libm::fmod(y, x));
            d("pow", libm::pow(y, x));
            d("copysign", libm::copysign(y, x));
            d("fmin", libm::fmin(y, x));
            d("fmax", libm::fmax(y, x));
        }
    }

    let mut ys: Vec<f32> = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        f32::from_bits(1),
        f32::from_bits(1 << 31 | 1),
        f32::MIN_POSITIVE,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        -f32::NAN,
        1.0e-20,
        1.0e20,
    ];
    let pif = 3.14159274_f32;
    for k in [0i32, 1, 2, 3, 4, 10, 100, 10_000] {
        let base = (k as f32) * pif / 2.0;
        ys.push(base);
        ys.push(-base);
    }
    for &x in &ys {
        s("sqrtf", libm::sqrtf(x));
        s("sinf", libm::sinf(x));
        s("cosf", libm::cosf(x));
        s("tanf", libm::tanf(x));
        let (sn, cs) = libm::sincosf(x);
        s("sincosf_s", sn);
        s("sincosf_c", cs);
        s("asinf", libm::asinf(x));
        s("acosf", libm::acosf(x));
        s("atanf", libm::atanf(x));
        s("floorf", libm::floorf(x));
        s("ceilf", libm::ceilf(x));
        s("truncf", libm::truncf(x));
        s("roundf", libm::roundf(x));
        s("fabsf", libm::fabsf(x));
        s("expf", libm::expf(x));
        s("logf", libm::logf(x));
        s("log2f", libm::log2f(x));
        s("log10f", libm::log10f(x));
        s("libm_cos", libm::Libm::<f32>::cos(x));
        for n in [-200, -24, -1, 0, 1, 23, 127, 400] {
            s(&format!("scalbnf_{n}"), libm::scalbnf(x, n));
        }
    }
    let pairsf = [0.0_f32, -0.0, 1.0, -1.0, 0.5, f32::INFINITY, f32::NEG_INFINITY, f32::NAN, -f32::NAN, 1.0e16];
    for &y in &pairsf {
        for &x in &pairsf {
            s("atan2f", libm::atan2f(y, x));
            s("hypotf", libm::hypotf(y, x));
            s("fmodf", libm::fmodf(y, x));
            s("powf", libm::powf(y, x));
            s("copysignf", libm::copysignf(y, x));
            s("fminf", libm::fminf(y, x));
            s("fmaxf", libm::fmaxf(y, x));
        }
    }
}
