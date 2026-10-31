#ifndef _WIN32
#include <time.h>
#endif

#ifdef _WIN32
/* QueryPerformanceFrequency: Windows Instant's epsilon is one tick. */
uint64_t lust_qpc_freq(void) {
    LARGE_INTEGER freq;
    QueryPerformanceFrequency(&freq);
    return freq.QuadPart > 0 ? (uint64_t)freq.QuadPart : 1;
}

/* QueryPerformanceCounter as a (sec, nsec) pair, and the system
   FILETIME (100 ns ticks since 1601) shifted to the Unix epoch. */
lust_u128 lust_clock_now_raw(void) {
    LARGE_INTEGER freq, now;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&now);
    lust_u128 r;
    r.lo = (uint64_t)(now.QuadPart / freq.QuadPart);
    r.hi = (uint64_t)(((now.QuadPart % freq.QuadPart) * 1000000000) / freq.QuadPart);
    return r;
}

lust_u128 lust_system_time_raw(void) {
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    uint64_t ticks = ((uint64_t)ft.dwHighDateTime << 32) | ft.dwLowDateTime;
    ticks -= 116444736000000000ull;
    lust_u128 r;
    r.lo = ticks / 10000000ull;
    r.hi = (ticks % 10000000ull) * 100ull;
    return r;
}

uint32_t lust_fmt_align(lust_Formatter *f) {
    return f->align;
}
#else
uint64_t lust_qpc_freq(void) {
    return 0;
}
/* One clock reading as the u128 the time intrinsics return. */
static lust_u128 lust_clock_read(clockid_t clock) {
    struct timespec ts;
    if (clock_gettime(clock, &ts) != 0) {
        abort();
    }
    lust_u128 r;
    r.lo = (uint64_t)(int64_t)ts.tv_sec;
    r.hi = (uint64_t)ts.tv_nsec;
    return r;
}

lust_u128 lust_clock_now_raw(void) {
#ifdef __APPLE__
    /* std's Instant on Apple is CLOCK_UPTIME_RAW. */
    return lust_clock_read(CLOCK_UPTIME_RAW);
#else
    return lust_clock_read(CLOCK_MONOTONIC);
#endif
}

lust_u128 lust_system_time_raw(void) {
    return lust_clock_read(CLOCK_REALTIME);
}

uint32_t lust_fmt_align(lust_Formatter *f) {
    return f->align;
}
#endif
