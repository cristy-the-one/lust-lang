/* Float text conversion for the lust runtime: byte-exact with Rust 1.99 std.
   Included at the end of runtime.c. The compiler inlines this header (and, for the
   runtime unit, float.c) into the generated C, because units are compiled from
   strings and cannot resolve file includes; see cgen::runtime_source.
   Correctness is gated by tests/float/fuzz.sh against rustc and ryu-js 1.0.2. */
#ifndef LUST_RT_FLOAT_H
#define LUST_RT_FLOAT_H

/* Modes for lust_float_spec.mode: Display, Debug, LowerExp, UpperExp. */
#define LUST_FLOAT_DISPLAY 0
#define LUST_FLOAT_DEBUG 1
#define LUST_FLOAT_LOWER_EXP 2
#define LUST_FLOAT_UPPER_EXP 3

/* Precision value that means "no precision was given". */
#define LUST_FLOAT_NO_PRECISION 0xFFFFFFFFu

/* One parsed format spec for a float, as Rust's format string grammar gives it.
   Zero every field for plain Display: mode 0, width 0, fill 0 (' '), align 0
   (right, like Rust numbers), precision LUST_FLOAT_NO_PRECISION.
   Notes from rustc 1.99 measurements, kept here so codegen can rely on them:
   - zero_pad (the '0' flag) beats fill and align, and pads after the sign.
     It applies to inf and NaN too ("00000inf", "00000NaN", "-0000inf").
   - alt ('#') has no effect on float output. It is accepted and ignored.
   - NaN never gets a sign, not even with plus or a negative NaN input. */
typedef struct {
    uint32_t precision;
    uint32_t width;
    uint32_t fill; /* Unicode scalar value; 0 means ' ' */
    uint8_t align; /* 0 = default, else '<', '>', or '^' */
    uint8_t plus;  /* '+' flag */
    uint8_t zero_pad;
    uint8_t alt;
    uint8_t mode;
} lust_float_spec;

/* Appends `v` formatted per `spec` (Rust Display, Debug, LowerExp, UpperExp). */
void lust_fmt_f64(lust_String *out, double v, lust_float_spec spec);
void lust_fmt_f32(lust_String *out, float v, lust_float_spec spec);

/* Plain Display ("{}") and Debug ("{:?}") with no other spec fields. */
void lust_fmt_f64_display(lust_String *out, double v);
void lust_fmt_f64_debug(lust_String *out, double v);
void lust_fmt_f32_display(lust_String *out, float v);
void lust_fmt_f32_debug(lust_String *out, float v);

/* ECMAScript Number::toString as ryu-js 1.0.2 Buffer::format does it:
   NaN, "Infinity", "-Infinity", and -0.0 prints as "0". */
void lust_fmt_js_f64(lust_String *out, double v);
void lust_fmt_js_f32(lust_String *out, float v);
/* Same text into a caller buffer of at least 32 bytes. No NUL is written.
   Returns the byte length. */
uint64_t lust_fmt_js_f64_buf(char *buf, double v);
uint64_t lust_fmt_js_f32_buf(char *buf, float v);

/* Rust str::parse::<f64> / ::<f32>. Returns 0 on success (*out is set),
   1 for an empty string, 2 for an invalid string. These are the two
   ParseFloatError kinds; overflow and underflow are success (inf or 0). */
int lust_parse_f64(lust_str s, double *out);
int lust_parse_f32(lust_str s, float *out);

/* Saturating float-to-int casts, Rust `as` semantics: NaN gives 0,
   values out of range clamp, fractions truncate toward zero. */
int8_t lust_f64_to_i8(double v);
int16_t lust_f64_to_i16(double v);
int32_t lust_f64_to_i32(double v);
int64_t lust_f64_to_i64(double v);
uint8_t lust_f64_to_u8(double v);
uint16_t lust_f64_to_u16(double v);
uint32_t lust_f64_to_u32(double v);
uint64_t lust_f64_to_u64(double v);
int8_t lust_f32_to_i8(float v);
int16_t lust_f32_to_i16(float v);
int32_t lust_f32_to_i32(float v);
int64_t lust_f32_to_i64(float v);
uint8_t lust_f32_to_u8(float v);
uint16_t lust_f32_to_u16(float v);
uint32_t lust_f32_to_u32(float v);
uint64_t lust_f32_to_u64(float v);
/* 128-bit results split into words: tcc has no __int128. *hi is the high
   64 bits (sign-extended pattern for the signed form). */
void lust_f64_to_i128(double v, uint64_t *lo, uint64_t *hi);
void lust_f64_to_u128(double v, uint64_t *lo, uint64_t *hi);
void lust_f32_to_i128(float v, uint64_t *lo, uint64_t *hi);
void lust_f32_to_u128(float v, uint64_t *lo, uint64_t *hi);

/* Int-to-float casts with a single round-to-nearest-even, done in integer
   arithmetic so no compiler can double-round them. */
double lust_i64_to_f64(int64_t v);
double lust_u64_to_f64(uint64_t v);
float lust_i64_to_f32(int64_t v);
float lust_u64_to_f32(uint64_t v);
double lust_i128_to_f64(uint64_t lo, uint64_t hi);
double lust_u128_to_f64(uint64_t lo, uint64_t hi);
float lust_i128_to_f32(uint64_t lo, uint64_t hi);
float lust_u128_to_f32(uint64_t lo, uint64_t hi);

/* Remainder (Rust `%` on floats, truncating division); defined in core.c
   next to the bounds helpers so no libm link is needed. */
double lust_fmod_f64(double, double);
float lust_fmod_f32(float, float);

/* Float-to-float casts (Rust `as`): f64 to f32 rounds to nearest even and
   overflows to inf; f32 to f64 is exact. Bit-level code, so the host
   compiler's float behavior cannot change the result. */
float lust_f64_to_f32(double v);
double lust_f32_to_f64(float v);


/* The libm entry points the float methods call: Rust lowers these
   methods to the same libm calls on this platform. The powi, min,
   max, and signum wrappers are lust's (Rust-specific semantics);
   they are defined in float.c. */
double sqrt(double);
float sqrtf(float);
double pow(double, double);
float powf(float, float);
double exp(double);
float expf(float);
double log(double);
float logf(float);
double log2(double);
float log2f(float);
double log10(double);
float log10f(float);
double floor(double);
float floorf(float);
double ceil(double);
float ceilf(float);
double round(double);
float roundf(float);
double trunc(double);
float truncf(float);
double fma(double, double, double);
float fmaf(float, float, float);
double fabs(double);
float fabsf(float);
double copysign(double, double);
float copysignf(float, float);
double sin(double);
float sinf(float);
double cos(double);
float cosf(float);
double tan(double);
float tanf(float);
double asin(double);
float asinf(float);
double acos(double);
float acosf(float);
double atan(double);
float atanf(float);
double sinh(double);
float sinhf(float);
double cosh(double);
float coshf(float);
double tanh(double);
float tanhf(float);
double exp2(double);
float exp2f(float);
double expm1(double);
float expm1f(float);
double log1p(double);
float log1pf(float);
double atan2(double, double);
float atan2f(float, float);
double hypot(double, double);
float hypotf(float, float);

double lust_f64_powi(double a, int32_t e);
float lust_f32_powi(float a, int32_t e);
double lust_f64_min(double a, double b);
double lust_f64_max(double a, double b);
float lust_f32_min(float a, float b);
float lust_f32_max(float a, float b);
double lust_f64_signum(double a);
float lust_f32_signum(float a);
uint64_t lust_f64_to_bits(double v);
uint32_t lust_f32_to_bits(float v);
double lust_f64_from_bits_of(uint64_t b);
float lust_f32_from_bits_of(uint32_t b);

#ifdef LUST_IMPL
#include "float.c"
#endif

#endif
