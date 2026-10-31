//! Musl-derived kernels. `i!` is the upstream unchecked index, written
//! as a normal index so the values stay the same without `unsafe`.

macro_rules! div {
    ($a:expr, $b:expr) => {
        $a / $b
    };
}

macro_rules! i {
    ($array:expr, $index:expr) => {
        $array[$index]
    };
    ($array:expr, $index:expr, =, $rhs:expr) => {
        $array[$index] = $rhs
    };
    ($array:expr, $index:expr, +=, $rhs:expr) => {
        $array[$index] += $rhs
    };
    ($array:expr, $index:expr, -=, $rhs:expr) => {
        $array[$index] -= $rhs
    };
    ($array:expr, $index:expr, &=, $rhs:expr) => {
        $array[$index] &= $rhs
    };
    ($array:expr, $index:expr, ==, $rhs:expr) => {
        $array[$index] == $rhs
    };
}

mod m_acos;
mod m_acosf;
mod m_asin;
mod m_asinf;
mod m_atan;
mod m_atan2;
mod m_atan2f;
mod m_atanf;
mod bits;
mod m_cos;
mod m_cosf;
mod m_exp;
mod m_expf;
mod m_hypot;
mod m_hypotf;
mod m_k_cos;
mod m_k_cosf;
mod m_k_sin;
mod m_k_sinf;
mod m_k_tan;
mod m_k_tanf;
mod m_log;
mod m_log10;
mod m_log10f;
mod m_log2;
mod m_log2f;
mod m_logf;
mod m_pow;
mod m_powf;
mod m_rem_pio2;
mod m_rem_pio2_large;
mod m_rem_pio2f;
mod m_sin;
mod m_sinf;
mod m_sincos;
mod m_sincosf;
mod m_tan;
mod m_tanf;

// Public functions are re-exported under their crate names. The modules
// are named `m_*` so the function and the module do not share a path.
pub use bits::{
    ceil, ceilf, copysign, copysignf, fabs, fabsf, floor, floorf, fmax, fmaxf, fmin, fminf, fmod, fmodf, round, roundf,
    scalbn, scalbnf, sqrt, sqrtf, trunc, truncf,
};

pub use m_acos::acos;
pub use m_acosf::acosf;
pub use m_asin::asin;
pub use m_asinf::asinf;
pub use m_atan::atan;
pub use m_atan2::atan2;
pub use m_atan2f::atan2f;
pub use m_atanf::atanf;
pub use m_cos::cos;
pub use m_cosf::cosf;
pub use m_exp::exp;
pub use m_expf::expf;
pub use m_hypot::hypot;
pub use m_hypotf::hypotf;
pub(crate) use m_k_cos::k_cos;
pub(crate) use m_k_cosf::k_cosf;
pub(crate) use m_k_sin::k_sin;
pub(crate) use m_k_sinf::k_sinf;
pub(crate) use m_k_tan::k_tan;
pub(crate) use m_k_tanf::k_tanf;
pub use m_log::log;
pub use m_log10::log10;
pub use m_log10f::log10f;
pub use m_log2::log2;
pub use m_log2f::log2f;
pub use m_logf::logf;
pub use m_pow::pow;
pub use m_powf::powf;
pub(crate) use m_rem_pio2::rem_pio2;
pub(crate) use m_rem_pio2_large::rem_pio2_large;
pub(crate) use m_rem_pio2f::rem_pio2f;
pub use m_sin::sin;
pub use m_sincos::sincos;
pub use m_sincosf::sincosf;
pub use m_sinf::sinf;
pub use m_tan::tan;
pub use m_tanf::tanf;

#[inline]
pub(crate) fn get_high_word(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

#[inline]
pub(crate) fn get_low_word(x: f64) -> u32 {
    x.to_bits() as u32
}

#[inline]
pub(crate) fn with_set_high_word(f: f64, hi: u32) -> f64 {
    let mut tmp = f.to_bits();
    tmp &= 0x0000_0000_ffff_ffff;
    tmp |= (hi as u64) << 32;
    f64::from_bits(tmp)
}

#[inline]
pub(crate) fn with_set_low_word(f: f64, lo: u32) -> f64 {
    let mut tmp = f.to_bits();
    tmp &= 0xffff_ffff_0000_0000;
    tmp |= lo as u64;
    f64::from_bits(tmp)
}
