# Adapter: libm 0.2.16

Lust adapter crate (docs/ARCH.md section 3.6). Dev builds compile it in
place of the registry package libm 0.2.16; release builds use the real
crate through Cargo. Gate: `scripts/test-adapters.sh libm`.

The implementation is lust source, the same shape as the other adapters.
The trig, exp, log, and pow bodies are the real crate's musl kernels.
`sqrt` and `sqrtf` are IEEE square root, which is what libm's default
`arch` feature uses on x86_64. Rounding, `fmod`, `scalbn`, `copysign`,
and `fmin`/`fmax` follow the generic bit recipes. Returned bits match
the real crate, including signed zero, subnormals, infinities, and NaNs.
`tests/adapters/crates/libm/random.rs` checks a fixed seed of 4096
random f64 patterns and 4096 random f32 patterns on every function,
plus values near multiples of pi/2.

## Implemented

Free functions and the same names on `Libm<f64>` / `Libm<f32>`:

- f64: `sqrt`, `sin`, `cos`, `tan`, `sincos`, `asin`, `acos`, `atan`,
  `atan2`, `floor`, `ceil`, `trunc`, `round`, `fabs`, `copysign`, `exp`,
  `log`, `log2`, `log10`, `pow`, `hypot`, `fmod`, `scalbn`, `fmin`, `fmax`
- f32: the `f` forms of those (`sinf`, `sqrtf`, `powf`, ...)

Features: `arch`, `default`.

## Rejected (lust error, never a different answer)

- Bessel, gamma, error function, hyperbolic, `fma`, `frexp`, `ldexp`,
  `nextafter`, `rint`, and the rest of the crate (L0101).
- `unstable`, `force-soft-floats`, and the other non-default features (L0508).
- `size_of::<Libm<f64>>()` (L0652). This type is not in `exact-layout`.
