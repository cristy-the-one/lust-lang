//! libm 0.2.16, the float maths lime-slice-core calls.
//!
//! The trig, exp, log, and pow kernels are the real crate's musl ports
//! with the `unsafe` index and volatile side-effect macros removed. Those
//! macros do not change returned bits. `sqrt` follows the crate's x86_64
//! `arch` path, which is IEEE square root. Rounding and remainder are the
//! same bit recipes as the generic bodies.
//!
//! `Libm<T>` exists so `Libm::<f64>::sin` resolves. The methods are
//! inherent. rustc still calls the real crate's methods.

mod math;

pub use math::{
    acos, acosf, asin, asinf, atan, atan2, atan2f, atanf, ceil, ceilf, copysign, copysignf, cos, cosf, exp, expf, fabs,
    fabsf, floor, floorf, fmax, fmaxf, fmin, fminf, fmod, fmodf, hypot, hypotf, log, log10, log10f, log2, log2f, logf,
    pow, powf, round, roundf, scalbn, scalbnf, sin, sincos, sincosf, sinf, sqrt, sqrtf, tan, tanf, trunc, truncf,
};

pub struct Libm<T> {
    _p: std::marker::PhantomData<T>,
}

impl Libm<f64> {
    pub fn acos(x: f64) -> f64 { acos(x) }
    pub fn asin(x: f64) -> f64 { asin(x) }
    pub fn atan(x: f64) -> f64 { atan(x) }
    pub fn atan2(y: f64, x: f64) -> f64 { atan2(y, x) }
    pub fn ceil(x: f64) -> f64 { ceil(x) }
    pub fn copysign(x: f64, y: f64) -> f64 { copysign(x, y) }
    pub fn cos(x: f64) -> f64 { cos(x) }
    pub fn exp(x: f64) -> f64 { exp(x) }
    pub fn fabs(x: f64) -> f64 { fabs(x) }
    pub fn floor(x: f64) -> f64 { floor(x) }
    pub fn fmax(x: f64, y: f64) -> f64 { fmax(x, y) }
    pub fn fmin(x: f64, y: f64) -> f64 { fmin(x, y) }
    pub fn fmod(x: f64, y: f64) -> f64 { fmod(x, y) }
    pub fn hypot(x: f64, y: f64) -> f64 { hypot(x, y) }
    pub fn log(x: f64) -> f64 { log(x) }
    pub fn log2(x: f64) -> f64 { log2(x) }
    pub fn log10(x: f64) -> f64 { log10(x) }
    pub fn pow(x: f64, y: f64) -> f64 { pow(x, y) }
    pub fn round(x: f64) -> f64 { round(x) }
    pub fn scalbn(x: f64, n: i32) -> f64 { scalbn(x, n) }
    pub fn sin(x: f64) -> f64 { sin(x) }
    pub fn sincos(x: f64) -> (f64, f64) { sincos(x) }
    pub fn sqrt(x: f64) -> f64 { sqrt(x) }
    pub fn tan(x: f64) -> f64 { tan(x) }
    pub fn trunc(x: f64) -> f64 { trunc(x) }
}

impl Libm<f32> {
    pub fn acos(x: f32) -> f32 { acosf(x) }
    pub fn asin(x: f32) -> f32 { asinf(x) }
    pub fn atan(x: f32) -> f32 { atanf(x) }
    pub fn atan2(y: f32, x: f32) -> f32 { atan2f(y, x) }
    pub fn ceil(x: f32) -> f32 { ceilf(x) }
    pub fn copysign(x: f32, y: f32) -> f32 { copysignf(x, y) }
    pub fn cos(x: f32) -> f32 { cosf(x) }
    pub fn exp(x: f32) -> f32 { expf(x) }
    pub fn fabs(x: f32) -> f32 { fabsf(x) }
    pub fn floor(x: f32) -> f32 { floorf(x) }
    pub fn fmax(x: f32, y: f32) -> f32 { fmaxf(x, y) }
    pub fn fmin(x: f32, y: f32) -> f32 { fminf(x, y) }
    pub fn fmod(x: f32, y: f32) -> f32 { fmodf(x, y) }
    pub fn hypot(x: f32, y: f32) -> f32 { hypotf(x, y) }
    pub fn log(x: f32) -> f32 { logf(x) }
    pub fn log2(x: f32) -> f32 { log2f(x) }
    pub fn log10(x: f32) -> f32 { log10f(x) }
    pub fn pow(x: f32, y: f32) -> f32 { powf(x, y) }
    pub fn round(x: f32) -> f32 { roundf(x) }
    pub fn scalbn(x: f32, n: i32) -> f32 { scalbnf(x, n) }
    pub fn sin(x: f32) -> f32 { sinf(x) }
    pub fn sincos(x: f32) -> (f32, f32) { sincosf(x) }
    pub fn sqrt(x: f32) -> f32 { sqrtf(x) }
    pub fn tan(x: f32) -> f32 { tanf(x) }
    pub fn trunc(x: f32) -> f32 { truncf(x) }
}
