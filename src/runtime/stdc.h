/* Std runtime hooks of package W-STD-C: the C behind the time and fmt
   intrinsics of src/ty/lang.rs that std source (library/core and
   library/std time modules) calls.

   Declarations here are in every unit; bodies (stdc.c) only in the
   runtime unit (LUST_IMPL), like deps.h. */
#ifndef LUST_RT_STDC_H
#define LUST_RT_STDC_H

/* Clock readings for `Instant::now` (CLOCK_MONOTONIC) and
   `SystemTime::now` (CLOCK_REALTIME): `lo` is tv_sec as i64 bits, `hi`
   is tv_nsec. A clock_gettime failure aborts, as std's unwrap panics. */
lust_u128 lust_clock_now_raw(void);
lust_u128 lust_system_time_raw(void);
/* QueryPerformanceFrequency, or 0 off Windows. Instant's epsilon is one tick. */
uint64_t lust_qpc_freq(void);

/* `Formatter`'s alignment as `rt::Alignment` encodes it: 0 left,
   1 right, 2 center, 3 unknown (none written). */
uint32_t lust_fmt_align(lust_Formatter *f);

#ifdef LUST_IMPL
#include "stdc.c"
#endif

#endif
