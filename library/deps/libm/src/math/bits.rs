//! Bit operations and IEEE helpers for the functions whose real-crate
//! bodies are generic over `Float`. On x86_64 the real crate's default
//! `arch` feature lowers `sqrt`/`sqrtf` to SSE, which is the same
//! correctly rounded operation as `f64::sqrt` / `f32::sqrt`.

pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

pub fn sqrtf(x: f32) -> f32 {
    x.sqrt()
}

pub fn fabs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0x7fff_ffff_ffff_ffff)
}

pub fn fabsf(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7fff_ffff)
}

pub fn copysign(x: f64, y: f64) -> f64 {
    f64::from_bits((x.to_bits() & 0x7fff_ffff_ffff_ffff) | (y.to_bits() & 0x8000_0000_0000_0000))
}

pub fn copysignf(x: f32, y: f32) -> f32 {
    f32::from_bits((x.to_bits() & 0x7fff_ffff) | (y.to_bits() & 0x8000_0000))
}

fn ilog2(x: u64) -> u32 {
    63 - x.leading_zeros()
}

/// `(x << e) % y` for `y != 0`, with `x` already reduced below a small multiple of `y`.
fn rem_shl(mut x: u64, mut e: u32, y: u64) -> u64 {
    if y & (y - 1) == 0 {
        if e >= 64 {
            return 0;
        }
        return (x << e) & (y - 1);
    }
    x %= y;
    while e > 0 {
        let zeros = x.leading_zeros();
        let room = if zeros <= 1 { 1 } else { (zeros - 1).min(32) };
        let k = if e < room { e } else { room };
        x = (((x as u128) << k) % (y as u128)) as u64;
        e -= k;
    }
    x
}

fn fmod64(x: f64, y: f64, sig: u32, sign: u64, exp: u64, implicit: u64) -> f64 {
    let sx = x.to_bits() & sign;
    let ux = x.to_bits() & !sign;
    let uy = y.to_bits() & !sign;
    let x_nan_or_inf = ux & exp == exp;
    let y_nan_or_zero = uy.wrapping_sub(1) & exp == exp;
    if x_nan_or_inf || y_nan_or_zero {
        return (x * y) / (x * y);
    }
    if ux < uy {
        return x;
    }
    let (num, ex) = sig_exp(ux, sign, exp, implicit);
    let (div, ey) = sig_exp(uy, sign, exp, implicit);
    let rem = rem_shl(num, ex - ey, div);
    if rem == 0 {
        return f64::from_bits(sx);
    }
    let shift = ey.min(sig - ilog2(rem));
    let bits = (rem << shift) + (((ey - shift) as u64) << sig);
    f64::from_bits(sx + bits)
}

fn sig_exp(mut bits: u64, sign: u64, exp: u64, implicit: u64) -> (u64, u32) {
    bits &= !sign;
    let sat = if bits >= implicit { bits - implicit } else { 0 };
    (bits - (sat & exp), (sat >> sig_of(implicit)) as u32)
}

fn sig_of(implicit: u64) -> u32 {
    63 - implicit.leading_zeros()
}

pub fn fmod(x: f64, y: f64) -> f64 {
    fmod64(x, y, 52, 1 << 63, 0x7ff << 52, 1 << 52)
}

pub fn fmodf(x: f32, y: f32) -> f32 {
    fmod32(x, y)
}

fn fmod32(x: f32, y: f32) -> f32 {
    let sign: u32 = 1 << 31;
    let exp: u32 = 0xff << 23;
    let implicit: u32 = 1 << 23;
    let sx = x.to_bits() & sign;
    let ux = x.to_bits() & !sign;
    let uy = y.to_bits() & !sign;
    let x_nan_or_inf = ux & exp == exp;
    let y_nan_or_zero = uy.wrapping_sub(1) & exp == exp;
    if x_nan_or_inf || y_nan_or_zero {
        return (x * y) / (x * y);
    }
    if ux < uy {
        return x;
    }
    let (num, ex) = {
        let bits = ux;
        let sat = if bits >= implicit { bits - implicit } else { 0 };
        (bits - (sat & exp), sat >> 23)
    };
    let (div, ey) = {
        let bits = uy;
        let sat = if bits >= implicit { bits - implicit } else { 0 };
        (bits - (sat & exp), sat >> 23)
    };
    let rem = rem_shl(num as u64, ex - ey, div as u64) as u32;
    if rem == 0 {
        return f32::from_bits(sx);
    }
    let lg = 31 - rem.leading_zeros();
    let shift = ey.min(23 - lg);
    let bits = (rem << shift) + ((ey - shift) << 23);
    f32::from_bits(sx + bits)
}

pub fn floor(x: f64) -> f64 {
    floor64(x)
}

pub fn floorf(x: f32) -> f32 {
    floor32(x)
}

pub fn ceil(x: f64) -> f64 {
    ceil64(x)
}

pub fn ceilf(x: f32) -> f32 {
    ceil32(x)
}

pub fn trunc(x: f64) -> f64 {
    trunc64(x)
}

pub fn truncf(x: f32) -> f32 {
    trunc32(x)
}

fn floor64(x: f64) -> f64 {
    let mut ix = x.to_bits();
    let e = ((ix >> 52) & 0x7ff) as i32 - 1023;
    if e >= 52 {
        return x;
    }
    if e >= 0 {
        let m = (1u64 << (52 - e)) - 1;
        if ix & m == 0 {
            return x;
        }
        if ix >> 63 != 0 {
            ix = ix.wrapping_add(m);
        }
        return f64::from_bits(ix & !m);
    }
    if ix >> 63 == 0 {
        return 0.0;
    }
    if ix << 1 != 0 {
        return -1.0;
    }
    x
}

fn floor32(x: f32) -> f32 {
    let mut ix = x.to_bits();
    let e = ((ix >> 23) & 0xff) as i32 - 127;
    if e >= 23 {
        return x;
    }
    if e >= 0 {
        let m = (1u32 << (23 - e)) - 1;
        if ix & m == 0 {
            return x;
        }
        if ix >> 31 != 0 {
            ix = ix.wrapping_add(m);
        }
        return f32::from_bits(ix & !m);
    }
    if ix >> 31 == 0 {
        return 0.0;
    }
    if ix << 1 != 0 {
        return -1.0;
    }
    x
}

fn ceil64(x: f64) -> f64 {
    let mut ix = x.to_bits();
    let e = ((ix >> 52) & 0x7ff) as i32 - 1023;
    if e >= 52 {
        return x;
    }
    if e >= 0 {
        let m = (1u64 << (52 - e)) - 1;
        if ix & m == 0 {
            return x;
        }
        if ix >> 63 == 0 {
            ix = ix.wrapping_add(m);
        }
        return f64::from_bits(ix & !m);
    }
    if ix >> 63 != 0 {
        return -0.0;
    }
    if ix << 1 != 0 {
        return 1.0;
    }
    x
}

fn ceil32(x: f32) -> f32 {
    let mut ix = x.to_bits();
    let e = ((ix >> 23) & 0xff) as i32 - 127;
    if e >= 23 {
        return x;
    }
    if e >= 0 {
        let m = (1u32 << (23 - e)) - 1;
        if ix & m == 0 {
            return x;
        }
        if ix >> 31 == 0 {
            ix = ix.wrapping_add(m);
        }
        return f32::from_bits(ix & !m);
    }
    if ix >> 31 != 0 {
        return -0.0;
    }
    if ix << 1 != 0 {
        return 1.0;
    }
    x
}

fn trunc64(x: f64) -> f64 {
    let mut ix = x.to_bits();
    let e = ((ix >> 52) & 0x7ff) as i32 - 1023;
    if e >= 52 {
        return x;
    }
    let mask = if e < 0 { 1u64 << 63 } else { !((1u64 << (52 - e)) - 1) };
    if ix & !mask == 0 {
        return x;
    }
    ix &= mask;
    f64::from_bits(ix)
}

fn trunc32(x: f32) -> f32 {
    let mut ix = x.to_bits();
    let e = ((ix >> 23) & 0xff) as i32 - 127;
    if e >= 23 {
        return x;
    }
    let mask = if e < 0 { 1u32 << 31 } else { !((1u32 << (23 - e)) - 1) };
    if ix & !mask == 0 {
        return x;
    }
    ix &= mask;
    f32::from_bits(ix)
}

pub fn round(x: f64) -> f64 {
    let half = 0.5 - 0.25 * f64::from_bits(0x3cb0_0000_0000_0000);
    trunc64(x + copysign(half, x))
}

pub fn roundf(x: f32) -> f32 {
    let half = 0.5 - 0.25 * f32::from_bits(0x3400_0000);
    trunc32(x + copysignf(half, x))
}

pub fn scalbn(mut x: f64, mut n: i32) -> f64 {
    let exp_max = 1023;
    let exp_min = -1022;
    let f_exp_max = f64::from_bits(0x7fe0_0000_0000_0000);
    let f_exp_min = f64::from_bits(0x0010_0000_0000_0000);
    let f_pow_subnorm = f64::from_bits((53 + 1023) << 52);
    if n > exp_max {
        x *= f_exp_max;
        n -= exp_max;
        if n > exp_max {
            x *= f_exp_max;
            n -= exp_max;
            if n > exp_max {
                n = exp_max;
            }
        }
    } else if n < exp_min {
        let mul = f_exp_min * f_pow_subnorm;
        let add = -exp_min - 53;
        x *= mul;
        n += add;
        if n < exp_min {
            x *= mul;
            n += add;
            if n < exp_min {
                n = exp_min;
            }
        }
    }
    let scale = f64::from_bits(((1023 + n) as u64) << 52);
    x * scale
}

pub fn scalbnf(mut x: f32, mut n: i32) -> f32 {
    let exp_max = 127;
    let exp_min = -126;
    let f_exp_max = f32::from_bits(254 << 23);
    let f_exp_min = f32::from_bits(1 << 23);
    let f_pow_subnorm = f32::from_bits((24 + 127) << 23);
    if n > exp_max {
        x *= f_exp_max;
        n -= exp_max;
        if n > exp_max {
            x *= f_exp_max;
            n -= exp_max;
            if n > exp_max {
                n = exp_max;
            }
        }
    } else if n < exp_min {
        let mul = f_exp_min * f_pow_subnorm;
        let add = -exp_min - 24;
        x *= mul;
        n += add;
        if n < exp_min {
            x *= mul;
            n += add;
            if n < exp_min {
                n = exp_min;
            }
        }
    }
    let scale = f32::from_bits(((127 + n) as u32) << 23);
    x * scale
}

pub fn fmin(x: f64, y: f64) -> f64 {
    let res = if y.is_nan() || x < y { x } else { y };
    res * 1.0
}

pub fn fminf(x: f32, y: f32) -> f32 {
    let res = if y.is_nan() || x < y { x } else { y };
    res * 1.0
}

pub fn fmax(x: f64, y: f64) -> f64 {
    let res = if x.is_nan() || x < y { y } else { x };
    res * 1.0
}

pub fn fmaxf(x: f32, y: f32) -> f32 {
    let res = if x.is_nan() || x < y { y } else { x };
    res * 1.0
}
