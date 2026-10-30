/* ---------- Atomics, threads, and sync (W-THREADS, ARCH 7.3) ----------

   Every atomic access in a lust program goes through these functions, so
   generated code holds no atomic syntax: tcc and gcc compile the same
   plain calls. The bodies use the __atomic builtins both compilers have
   (tcc lowers them to calls into libtcc1's lock-prefixed helpers).

   Orderings arrive as Rust's `atomic::Ordering` discriminant: 0 Relaxed,
   1 Release, 2 Acquire, 3 AcqRel, 4 SeqCst. The functions panic with
   core's messages for the orderings Rust's methods reject at run time
   (rustc rejects the same orderings at compile time when they are
   written as the variant itself; check/sync.rs gives L0542 there). */

/* Rust's Ordering to the C11 memory order. */
#define LUST_MO(o) ((o) == 0 ? 0 : (o) == 1 ? 3 : (o) == 2 ? 2 : (o) == 3 ? 4 : 5)
/* The order a compare-exchange uses on success: never weaker than its
   failure order (C11 asks for that; a stronger order is always correct). */
#define LUST_MO_CAS(s, f) (((f) == 4) ? 5 : ((f) == 2 && ((s) == 0 || (s) == 1)) ? ((s) == 0 ? 2 : 4) : LUST_MO(s))
/* The failure order of a compare-exchange loop that implements an RMW with
   order `o`: the load half of `o`. */
#define LUST_MO_LOADPART(o) ((o) == 4 ? 5 : ((o) == 2 || (o) == 3) ? 2 : 0)

#ifdef LUST_IMPL
#ifdef __TINYC__
/* tcc has no fence builtins: libtcc1 provides these two (stdatomic.h). */
extern void atomic_thread_fence(int);
extern void atomic_signal_fence(int);
#define LUST_THREAD_FENCE(o) atomic_thread_fence(o)
#define LUST_SIGNAL_FENCE(o) atomic_signal_fence(o)
#else
#define LUST_THREAD_FENCE(o) __atomic_thread_fence(o)
#define LUST_SIGNAL_FENCE(o) __atomic_signal_fence(o)
#endif
#endif

/* Ordering panics come from core's atomic.rs (no track_caller), as rustc
   1.99 reports them; `weak` picks compare_exchange_weak's lines. The
   load and store `loc` params stay for the call shape. */
#define LUST_ATOMIC_RS "core/src/sync/atomic.rs:"

#define LUST_ATOMIC_WIDTH(N, U, S)                                                                                     \
    U lust_at_load_##N(U *p, uint32_t o, lust_loc loc) LUST_BODY({                                                    \
        if (o == 1) lust_panic_std_lit("there is no such thing as a release load", LUST_ATOMIC_RS "3997:24");                                       \
        if (o == 3) lust_panic_std_lit("there is no such thing as an acquire-release load", LUST_ATOMIC_RS "3998:23");                        \
        U r;                                                                                                           \
        __atomic_load(p, &r, LUST_MO(o));                                                                              \
        return r;                                                                                                      \
    })                                                                                                                 \
    void lust_at_store_##N(U *p, U v, uint32_t o, lust_loc loc) LUST_BODY({                                           \
        if (o == 2) { lust_panic_std_lit("there is no such thing as an acquire store", LUST_ATOMIC_RS "3981:24"); return; }                       \
        if (o == 3) { lust_panic_std_lit("there is no such thing as an acquire-release store", LUST_ATOMIC_RS "3982:23"); return; }           \
        __atomic_store(p, &v, LUST_MO(o));                                                                             \
    })                                                                                                                 \
    U lust_at_swap_##N(U *p, U v, uint32_t o) LUST_BODY({                                                             \
        U r;                                                                                                           \
        __atomic_exchange(p, &v, &r, LUST_MO(o));                                                                      \
        return r;                                                                                                      \
    })                                                                                                                 \
    int lust_at_cas_##N(U *p, U cur, U nv, uint32_t s, uint32_t f, U *old, int weak) LUST_BODY({                      \
        if (f == 3 || f == 1) {                                                                                        \
            if (f == 3) lust_panic_std_lit("there is no such thing as an acquire-release failure ordering", weak ? LUST_ATOMIC_RS "4184:28" : LUST_ATOMIC_RS "4118:28"); \
            else lust_panic_std_lit("there is no such thing as a release failure ordering", weak ? LUST_ATOMIC_RS "4185:29" : LUST_ATOMIC_RS "4119:29"); \
            *old = cur;                                                                                                \
            return 0;                                                                                                  \
        }                                                                                                              \
        U e = cur;                                                                                                     \
        int ok = __atomic_compare_exchange(p, &e, &nv, 0, LUST_MO_CAS(s, f), LUST_MO(f));                              \
        *old = e;                                                                                                      \
        return ok;                                                                                                     \
    })                                                                                                                 \
    U lust_at_fetch_add_##N(U *p, U v, uint32_t o) LUST_BODY({ return __atomic_fetch_add(p, v, LUST_MO(o)); })        \
    U lust_at_fetch_sub_##N(U *p, U v, uint32_t o) LUST_BODY({ return __atomic_fetch_sub(p, v, LUST_MO(o)); })        \
    U lust_at_fetch_and_##N(U *p, U v, uint32_t o) LUST_BODY({ return __atomic_fetch_and(p, v, LUST_MO(o)); })        \
    U lust_at_fetch_or_##N(U *p, U v, uint32_t o) LUST_BODY({ return __atomic_fetch_or(p, v, LUST_MO(o)); })          \
    U lust_at_fetch_xor_##N(U *p, U v, uint32_t o) LUST_BODY({ return __atomic_fetch_xor(p, v, LUST_MO(o)); })        \
    U lust_at_fetch_nand_##N(U *p, U v, uint32_t o) LUST_BODY({                                                       \
        U cur;                                                                                                         \
        __atomic_load(p, &cur, 0);                                                                                     \
        for (;;) {                                                                                                     \
            U nv = (U) ~(cur & v);                                                                                     \
            if (__atomic_compare_exchange(p, &cur, &nv, 1, LUST_MO(o), LUST_MO_LOADPART(o))) return cur;               \
        }                                                                                                              \
    })                                                                                                                 \
    U lust_at_fetch_umax_##N(U *p, U v, uint32_t o) LUST_BODY({                                                       \
        U cur;                                                                                                         \
        __atomic_load(p, &cur, 0);                                                                                     \
        for (;;) {                                                                                                     \
            U nv = cur > v ? cur : v;                                                                                  \
            if (__atomic_compare_exchange(p, &cur, &nv, 1, LUST_MO(o), LUST_MO_LOADPART(o))) return cur;               \
        }                                                                                                              \
    })                                                                                                                 \
    U lust_at_fetch_umin_##N(U *p, U v, uint32_t o) LUST_BODY({                                                       \
        U cur;                                                                                                         \
        __atomic_load(p, &cur, 0);                                                                                     \
        for (;;) {                                                                                                     \
            U nv = cur < v ? cur : v;                                                                                  \
            if (__atomic_compare_exchange(p, &cur, &nv, 1, LUST_MO(o), LUST_MO_LOADPART(o))) return cur;               \
        }                                                                                                              \
    })                                                                                                                 \
    U lust_at_fetch_smax_##N(U *p, U v, uint32_t o) LUST_BODY({                                                       \
        U cur;                                                                                                         \
        __atomic_load(p, &cur, 0);                                                                                     \
        for (;;) {                                                                                                     \
            U nv = (S)cur > (S)v ? cur : v;                                                                            \
            if (__atomic_compare_exchange(p, &cur, &nv, 1, LUST_MO(o), LUST_MO_LOADPART(o))) return cur;               \
        }                                                                                                              \
    })                                                                                                                 \
    U lust_at_fetch_smin_##N(U *p, U v, uint32_t o) LUST_BODY({                                                       \
        U cur;                                                                                                         \
        __atomic_load(p, &cur, 0);                                                                                     \
        for (;;) {                                                                                                     \
            U nv = (S)cur < (S)v ? cur : v;                                                                            \
            if (__atomic_compare_exchange(p, &cur, &nv, 1, LUST_MO(o), LUST_MO_LOADPART(o))) return cur;               \
        }                                                                                                              \
    })

LUST_ATOMIC_WIDTH(1, uint8_t, int8_t)
LUST_ATOMIC_WIDTH(2, uint16_t, int16_t)
LUST_ATOMIC_WIDTH(4, uint32_t, int32_t)
LUST_ATOMIC_WIDTH(8, uint64_t, int64_t)

/* `AtomicBool::fetch_nand`, as core writes it: `fetch_xor(true)` when
   `v` is true (a nand with true is a not), else `swap(true)`. */
uint8_t lust_at_fetch_bnand_1(uint8_t *p, uint8_t v, uint32_t o) LUST_BODY({
    return v ? lust_at_fetch_xor_1(p, 1, o) : lust_at_swap_1(p, 1, o);
})

void lust_at_fence(uint32_t o, lust_loc loc) LUST_BODY({
    (void)loc;
    if (o == 0) lust_panic_std_lit("there is no such thing as a relaxed fence", LUST_ATOMIC_RS "4497:24");
    LUST_THREAD_FENCE(LUST_MO(o));
})

void lust_at_compiler_fence(uint32_t o, lust_loc loc) LUST_BODY({
    (void)loc;
    if (o == 0) lust_panic_std_lit("there is no such thing as a relaxed fence", LUST_ATOMIC_RS "4582:24");
    LUST_SIGNAL_FENCE(LUST_MO(o));
})

/* ---------- Arc and sync::Weak ----------

   An `Arc<T>` points at the same counted header as an `Rc<T>` (`RcI_T`:
   `strong`, `weak`, then the value), but every count changes atomically,
   as alloc::sync does. `weak` counts the Weaks plus one implicit weak
   held by all the strong refs together. `get_mut`'s uniqueness test
   locks the weak count (UINT64_MAX) for a moment; `downgrade` waits it
   out. These functions take the header as `uint64_t *h`: h[0] is
   `strong`, h[1] is `weak`. */

/* Adds one to a count. Rust aborts when a count passes isize::MAX. */
void lust_arc_inc(uint64_t *c) LUST_BODY({
    if (__atomic_fetch_add(c, 1, 0) > (uint64_t)INT64_MAX) abort();
})

/* Takes one from a count; 1 when that was the last (with the acquire
   fence Rust's drop takes before it frees). */
int lust_arc_dec(uint64_t *c) LUST_BODY({
    if (__atomic_fetch_sub(c, 1, 3) != 1) return 0;
    LUST_THREAD_FENCE(2);
    return 1;
})

/* The one shared allocation of every empty `Arc<str>` and `Arc<[T]>`
   that `Default` makes (std's static ArcInner, w-unsized): its strong
   count starts at 1, so it is never freed. Takes one more strong ref and
   returns the data pointer, right after the two counts. */
void *lust_arc_empty(void) LUST_BODY({
    static _Alignas(16) uint64_t h[2] = { 1, 1 };
    lust_arc_inc(&h[0]);
    return (void *)(h + 2);
})

/* An acquire load of a count. */
uint64_t lust_arc_load(uint64_t *c) LUST_BODY({
    uint64_t v;
    __atomic_load(c, &v, 2);
    return v;
})

/* `Arc::downgrade`: one more weak ref. */
void lust_arc_downgrade(uint64_t *h) LUST_BODY({
    uint64_t cur;
    __atomic_load(&h[1], &cur, 0);
    for (;;) {
        if (cur == UINT64_MAX) {
            __atomic_load(&h[1], &cur, 0);
            continue;
        }
        if (cur > (uint64_t)INT64_MAX) abort();
        uint64_t nv = cur + 1;
        if (__atomic_compare_exchange(&h[1], &cur, &nv, 1, 2, 0)) return;
    }
})

/* `Weak::upgrade` on a live pointer: 1 when a strong ref was taken
   (the strong count was not zero). */
int lust_arc_upgrade(uint64_t *h) LUST_BODY({
    uint64_t n;
    __atomic_load(&h[0], &n, 0);
    for (;;) {
        if (n == 0) return 0;
        if (n > (uint64_t)INT64_MAX) abort();
        uint64_t nv = n + 1;
        if (__atomic_compare_exchange(&h[0], &n, &nv, 1, 2, 0)) return 1;
    }
})

/* `Arc::weak_count`: the Weaks, without the implicit one. */
uint64_t lust_arc_weak_count(uint64_t *h) LUST_BODY({
    uint64_t w = lust_arc_load(&h[1]);
    return w == UINT64_MAX ? 0 : w - 1;
})

/* `Weak::weak_count` on a live pointer: 0 once the value is gone. */
uint64_t lust_arc_weak_weak_count(uint64_t *h) LUST_BODY({
    uint64_t w = lust_arc_load(&h[1]);
    uint64_t s = lust_arc_load(&h[0]);
    if (s == 0) return 0;
    return w == UINT64_MAX ? 0 : w - 1;
})

/* `Arc::get_mut`'s test: no other strong ref and no Weak. */
int lust_arc_is_unique(uint64_t *h) LUST_BODY({
    uint64_t one = 1, locked = UINT64_MAX;
    if (!__atomic_compare_exchange(&h[1], &one, &locked, 0, 2, 0)) return 0;
    int unique = lust_arc_load(&h[0]) == 1;
    uint64_t back = 1;
    __atomic_store(&h[1], &back, 3);
    return unique;
})

/* `Arc::make_mut`'s three cases: 0 another strong ref exists (clone the
   value), 1 this was the only strong ref but Weaks exist (strong is now
   0: move the value out), 2 unique (nothing to do). */
int lust_arc_make_mut_state(uint64_t *h) LUST_BODY({
    uint64_t one = 1, zero = 0;
    if (!__atomic_compare_exchange(&h[0], &one, &zero, 0, 2, 0)) return 0;
    uint64_t w;
    __atomic_load(&h[1], &w, 0);
    if (w != 1) return 1;
    __atomic_store(&h[0], &one, 3);
    return 2;
})

/* `Arc::try_unwrap`'s test: 1 when this was the only strong ref (strong
   is now 0 and the caller owns the value). */
int lust_arc_take_unique(uint64_t *h) LUST_BODY({
    uint64_t one = 1, zero = 0;
    if (!__atomic_compare_exchange(&h[0], &one, &zero, 0, 0, 0)) return 0;
    LUST_THREAD_FENCE(2);
    return 1;
})

/* ---------- Locks: Mutex and RwLock over a futex word ----------

   A `Mutex<T>` is { uint32_t st; uint8_t poison; T v; } (Windows: st is
   a uint8_t, rustc's SmallFutex) and an `RwLock<T>` is
   { uint32_t st; uint32_t wn; uint8_t poison; T v; } (cgen/sync.rs). All
   zero is unlocked, so `Mutex::new` in a static needs no constructor. The
   poison flag stays 0: a panic aborts the process. Locking a lock this
   thread already holds deadlocks, as std's futex locks do on Linux. */
#ifdef LUST_IMPL
#if defined(__linux__)
long syscall(long number, ...);
#if defined(__x86_64__)
#define LUST_SYS_FUTEX 202
#define LUST_SYS_SCHED_GETAFFINITY 204
#elif defined(__aarch64__)
#define LUST_SYS_FUTEX 98
#define LUST_SYS_SCHED_GETAFFINITY 123
#endif
#elif defined(__APPLE__)
/* Darwin's futex: the ulock calls in libSystem (no public header;
   UL_COMPARE_AND_WAIT sleeps while the u32 at the address equals the
   value, with a timeout in microseconds, 0 for no limit; wake takes
   ULF_WAKE_ALL to wake every waiter). */
int __ulock_wait(uint32_t op, void *addr, uint64_t value, uint32_t timeout_us);
int __ulock_wake(uint32_t op, void *addr, uint64_t wake_value);
#define LUST_ULOCK_COMPARE_AND_WAIT 1u
#define LUST_ULOCK_WAKE_ALL 0x100u
#endif
int sched_yield(void);
/* Sleep while *p == v (spurious wakeups are fine: callers loop). */
#ifdef _WIN32
/* WaitOnAddress and its wakes live behind an api-set with no zig
   import library: resolve them from kernel32 on first use. */
typedef int (*lust_woa_fn)(volatile void *, void *, size_t, uint32_t);
typedef void (*lust_wake_fn)(void *);
static lust_woa_fn lust_woa;
static lust_wake_fn lust_wake1, lust_wakeall;
static void lust_woa_init(void) {
    HMODULE k = GetModuleHandleW(L"kernel32.dll");
    lust_woa = (lust_woa_fn)GetProcAddress(k, "WaitOnAddress");
    lust_wake1 = (lust_wake_fn)GetProcAddress(k, "WakeByAddressSingle");
    lust_wakeall = (lust_wake_fn)GetProcAddress(k, "WakeByAddressAll");
}
static lust_pthread_once_t lust_woa_once = LUST_PTHREAD_ONCE_INIT;
#endif
static void lust_futex_wait(uint32_t *p, uint32_t v) {
#ifdef _WIN32
    pthread_once(&lust_woa_once, lust_woa_init);
    if (lust_woa) lust_woa(p, &v, sizeof v, 0xFFFFFFFFu);
    else SwitchToThread();
#elif defined(LUST_SYS_FUTEX)
    syscall(LUST_SYS_FUTEX, p, 128 /* FUTEX_WAIT_PRIVATE */, v, (void *)0, (void *)0, 0);
#elif defined(__APPLE__)
    __ulock_wait(LUST_ULOCK_COMPARE_AND_WAIT, p, v, 0);
#else
    (void)p; (void)v;
    sched_yield();
#endif
}
static void lust_futex_wake(uint32_t *p, int n) {
#ifdef _WIN32
    pthread_once(&lust_woa_once, lust_woa_init);
    if (n == 1) { if (lust_wake1) lust_wake1(p); } else if (lust_wakeall) lust_wakeall(p);
#elif defined(LUST_SYS_FUTEX)
    syscall(LUST_SYS_FUTEX, p, 129 /* FUTEX_WAKE_PRIVATE */, n, (void *)0, (void *)0, 0);
#elif defined(__APPLE__)
    __ulock_wake(LUST_ULOCK_COMPARE_AND_WAIT | (n == 1 ? 0 : LUST_ULOCK_WAKE_ALL), p, 0);
#else
    (void)p; (void)n;
#endif
}
/* The timed form of lust_futex_wait for the Condvar and channel
   timeouts below: sleep while *p == v, at most rs seconds and rn
   nanoseconds (callers recompute what is left and loop, so a capped
   or early return is fine). */
static void lust_futex_wait_for(uint32_t *p, uint32_t v, long rs, long rn) {
#ifdef _WIN32
    pthread_once(&lust_woa_once, lust_woa_init);
    uint64_t ms = (uint64_t)rs * 1000ull + (uint64_t)rn / 1000000ull;
    if (ms == 0) ms = 1;
    if (ms > 0xffffffffull) ms = 0xffffffffull;
    if (lust_woa) lust_woa(p, &v, sizeof v, (uint32_t)ms);
    else SwitchToThread();
#elif defined(LUST_SYS_FUTEX)
    long rel[2];
    rel[0] = rs;
    rel[1] = rn;
    syscall(LUST_SYS_FUTEX, p, 128 /* FUTEX_WAIT_PRIVATE */, v, rel, (void *)0, 0);
#elif defined(__APPLE__)
    uint64_t us = (uint64_t)rs * 1000000u + (uint64_t)rn / 1000u;
    if (us == 0) us = 1;
    if (us > 0xffffffffu) us = 0xffffffffu;
    __ulock_wait(LUST_ULOCK_COMPARE_AND_WAIT, p, v, (uint32_t)us);
#else
    (void)p; (void)v; (void)rs; (void)rn;
    sched_yield();
#endif
}
#ifdef _WIN32
/* One-byte WaitOnAddress for rustc's SmallFutex mutex word. */
static void lust_mutex_wait8(uint8_t *p, uint8_t v) {
    pthread_once(&lust_woa_once, lust_woa_init);
    if (lust_woa) lust_woa(p, &v, 1, 0xffffffffu);
    else SwitchToThread();
}
static void lust_mutex_wake8(uint8_t *p) {
    pthread_once(&lust_woa_once, lust_woa_init);
    if (lust_wake1) lust_wake1(p);
}
#endif
#endif

/* Poisoning (panic=unwind): the `poison` byte of a Mutex or RwLock. Bit 0
   is std's poison flag; bit 1 says the write guard that holds the lock was
   taken while its thread was panicking. A write guard dropped while
   panicking poisons the lock only when it was not taken that way (std's
   poison::Guard). */
void lust_poison_enter(uint8_t *f) LUST_BODY({
    if (lust_unw_panicking()) *f |= 2;
    else *f &= (uint8_t)~2;
})
void lust_poison_leave(uint8_t *f) LUST_BODY({
    if (!(*f & 2) && lust_unw_panicking()) *f |= 1;
})

/* A spin-wait hint for the lock loops below (as kconst.c's). */
static void lust_lock_pause(void) {
#if !defined(__TINYC__) && (defined(__x86_64__) || defined(__i386__))
    __builtin_ia32_pause();
#elif !defined(__TINYC__) && defined(__aarch64__)
    __asm__ __volatile__("yield");
#endif
}

/* The lock word after up to 100 relaxed loads while it reads `busy` (std's
   futex locks `spin`): a lock held for a short time is then taken without
   a futex wait and wake, two syscalls and two context switches. */
static uint32_t lust_lock_spin(uint32_t *st, uint32_t busy) {
    uint32_t s;
    for (int i = 0; i < 100; i++) {
        __atomic_load(st, &s, 0);
        if (s != busy) return s;
        lust_lock_pause();
    }
    __atomic_load(st, &s, 0);
    return s;
}

/* Mutex states: 0 unlocked, 1 locked, 2 locked with waiters (Drepper's
   futex mutex, as std's: spin first, then mark waiters and sleep).
   Windows waits on one byte (SmallFutex / WaitOnAddress size 1). A
   channel's `uint32_t` lock uses the same entry; only its low byte
   moves, which is the whole state. */
#ifdef _WIN32
static uint8_t lust_lock_spin8(uint8_t *st, uint8_t busy) {
    uint8_t s;
    for (int i = 0; i < 100; i++) {
        __atomic_load(st, &s, 0);
        if (s != busy) return s;
        lust_lock_pause();
    }
    __atomic_load(st, &s, 0);
    return s;
}
void lust_mutex_lock(void *stp) LUST_BODY({
    uint8_t *st = (uint8_t *)stp;
    uint8_t zero = 0, one = 1, two = 2, old;
    if (__atomic_compare_exchange(st, &zero, &one, 0, 2, 0)) return;
    uint8_t s = lust_lock_spin8(st, 1);
    if (s == 0) {
        if (__atomic_compare_exchange(st, &s, &one, 0, 2, 0)) return;
    }
    for (;;) {
        if (s != 2) {
            __atomic_exchange(st, &two, &old, 2);
            if (old == 0) return;
        }
        lust_mutex_wait8(st, 2);
        s = lust_lock_spin8(st, 1);
    }
})
int lust_mutex_trylock(void *stp) LUST_BODY({
    uint8_t *st = (uint8_t *)stp;
    uint8_t zero = 0, one = 1;
    return __atomic_compare_exchange(st, &zero, &one, 0, 2, 0);
})
void lust_mutex_unlock(void *stp) LUST_BODY({
    uint8_t *st = (uint8_t *)stp;
    uint8_t zero = 0, old;
    __atomic_exchange(st, &zero, &old, 3);
    if (old == 2) lust_mutex_wake8(st);
})
#else
void lust_mutex_lock(uint32_t *st) LUST_BODY({
    uint32_t zero = 0, one = 1, two = 2, old;
    if (__atomic_compare_exchange(st, &zero, &one, 0, 2, 0)) return;
    uint32_t s = lust_lock_spin(st, 1);
    if (s == 0) {
        if (__atomic_compare_exchange(st, &s, &one, 0, 2, 0)) return;
    }
    for (;;) {
        if (s != 2) {
            __atomic_exchange(st, &two, &old, 2);
            if (old == 0) return;
        }
        lust_futex_wait(st, 2);
        s = lust_lock_spin(st, 1);
    }
})

/* 1 when the lock was taken. */
int lust_mutex_trylock(uint32_t *st) LUST_BODY({
    uint32_t zero = 0, one = 1;
    return __atomic_compare_exchange(st, &zero, &one, 0, 2, 0);
})

void lust_mutex_unlock(uint32_t *st) LUST_BODY({
    uint32_t zero = 0, old;
    __atomic_exchange(st, &zero, &old, 3);
    if (old == 2) lust_futex_wake(st, 1);
})
#endif

/* RwLock: st is the reader count, or LUST_RW_WRITER while written; st[1]
   (wn) counts sleepers. Every step is SeqCst, so an unlock that reads
   wn == 0 happens before a sleeper's increment, and that sleeper's futex
   wait then sees the changed state and returns at once. */
#define LUST_RW_WRITER 0xffffffffu
void lust_rw_read(uint32_t *st) LUST_BODY({
    for (int spun = 0;;) {
        uint32_t s;
        __atomic_load(st, &s, 5);
        if (s < LUST_RW_WRITER - 1) {
            uint32_t n = s + 1;
            if (__atomic_compare_exchange(st, &s, &n, 0, 5, 5)) return;
            continue;
        }
        /* Written: spin once (as std's spin_read) before sleeping. */
        if (!spun) {
            spun = 1;
            if (lust_lock_spin(st, LUST_RW_WRITER) != LUST_RW_WRITER) continue;
        }
        __atomic_fetch_add(&st[1], 1, 5);
        lust_futex_wait(st, s);
        __atomic_fetch_sub(&st[1], 1, 5);
        spun = 0;
    }
})

int lust_rw_tryread(uint32_t *st) LUST_BODY({
    uint32_t s;
    __atomic_load(st, &s, 5);
    while (s < LUST_RW_WRITER - 1) {
        uint32_t n = s + 1;
        if (__atomic_compare_exchange(st, &s, &n, 0, 5, 5)) return 1;
    }
    return 0;
})

void lust_rw_read_unlock(uint32_t *st) LUST_BODY({
    uint32_t w;
    if (__atomic_fetch_sub(st, 1, 5) == 1) {
        __atomic_load(&st[1], &w, 5);
        if (w) lust_futex_wake(st, 0x7fffffff);
    }
})

void lust_rw_write(uint32_t *st) LUST_BODY({
    for (int spun = 0;;) {
        uint32_t s = 0, wr = LUST_RW_WRITER;
        if (__atomic_compare_exchange(st, &s, &wr, 0, 5, 5)) return;
        /* Held: spin once (as std's spin_write) until it is free, then try again. */
        if (!spun) {
            spun = 1;
            uint32_t t;
            for (int i = 0; i < 100; i++) {
                __atomic_load(st, &t, 0);
                if (t == 0) break;
                lust_lock_pause();
            }
            continue;
        }
        __atomic_fetch_add(&st[1], 1, 5);
        lust_futex_wait(st, s);
        __atomic_fetch_sub(&st[1], 1, 5);
        spun = 0;
    }
})

int lust_rw_trywrite(uint32_t *st) LUST_BODY({
    uint32_t s = 0, wr = LUST_RW_WRITER;
    return __atomic_compare_exchange(st, &s, &wr, 0, 5, 5);
})

void lust_rw_write_unlock(uint32_t *st) LUST_BODY({
    uint32_t zero = 0, w;
    __atomic_store(st, &zero, 5);
    __atomic_load(&st[1], &w, 5);
    if (w) lust_futex_wake(st, 0x7fffffff);
})

/* ---------- Threads: spawn, join, current (pthreads) ----------

   A `JoinHandle<T>` points at a packet the generated code allocates:
   { lust_JoinHdr h; T ret; Env env; } (cgen/sync.rs). The spawned thread
   runs h.run (a generated trampoline that calls the closure and stores
   its result), then gives up its reference. The handle and the thread
   each hold one reference (h.rc); whichever lets go last drops the result
   (h.drop_ret) and frees the packet. `join` waits, takes the result, and
   frees the packet. A `Thread` is a counted lust_ThreadInfo; the current
   thread's lives in a pthread key (no C _Thread_local), whose destructor
   gives up the thread's reference at thread exit. */
typedef struct lust_ThreadInfo {
    uint64_t rc;
    uint64_t id;
    const uint8_t *name;
    uint64_t name_len;
    /* A Builder name's owned copy (0 for a static name). */
    uint8_t *name_buf;
    /* A spawned thread's stack overflow window and alternate signal
       stack (panic.c `lust_thread_guard_init`); 0 for the main thread. */
    uintptr_t guard_lo, guard_hi;
    void *alt;
} lust_ThreadInfo;

/* A `thread::Scope`: the count of its live packets. The scope's end
   waits until it is 0; a packet counts down when it is freed, after its
   result dropped, so no scoped thread touches borrowed data after. */
typedef struct lust_Scope {
    uint32_t live;
    uint32_t panicked; /* a thread of the scope panicked and nobody joined it */
} lust_Scope;

typedef struct lust_JoinHdr {
    uintptr_t t; /* pthread_t on POSIX, a HANDLE as a word on Windows */
    lust_ThreadInfo *th;
    uint32_t rc;
    uint32_t done;
    void (*run)(struct lust_JoinHdr *);
    void (*drop_ret)(struct lust_JoinHdr *);
    lust_Scope *scope; /* 0 for thread::spawn */
    uint32_t panicked; /* the thread ended by a panic; `payload` is its Box<dyn Any + Send> */
    lust_dyn payload;
} lust_JoinHdr;

/* A JoinHdr's thread id as the pthread calls take it. The field is an
   unsigned long on every target (pthread_t is a word on glibc and a
   pointer on Darwin, both 8 bytes, so the packet layout cgen lays out
   is the same); on Darwin the calls cast it to the real pthread_t. */
#ifdef __APPLE__
#define LUST_PTHREAD_ID_P(p) ((pthread_t *)(void *)(p))
#define LUST_PTHREAD_ID(v) ((pthread_t)(uintptr_t)(v))
#else
#define LUST_PTHREAD_ID_P(p) (p)
#define LUST_PTHREAD_ID(v) (v)
#endif

#ifdef LUST_IMPL
/* Hand pthread declarations for tcc on Linux; on Darwin runtime.h
   included the real <pthread.h> and these would clash with it, and
   on Windows runtime.h maps these names to its Win32 wrappers.
   The thread word is uintptr_t: a HANDLE on Windows, where the
   32-bit long cannot hold one (same size as unsigned long here). */
#if !defined(__APPLE__) && !defined(_WIN32)
int pthread_create(uintptr_t *t, const void *attr, void *(*start)(void *), void *arg);
int pthread_join(uintptr_t t, void **ret);
int pthread_detach(uintptr_t t);
int pthread_key_create(unsigned int *key, void (*dtor)(void *));
void *pthread_getspecific(unsigned int key);
int pthread_setspecific(unsigned int key, const void *v);
int pthread_once(int *once, void (*init)(void));
#endif
static lust_pthread_key_t lust_cur_key;
static lust_pthread_once_t lust_cur_once = LUST_PTHREAD_ONCE_INIT;
/* ThreadId(1) is the main thread; spawned threads count from 2 in spawn
   order, as std assigns them. */
static uint64_t lust_next_tid = 2;
static lust_ThreadInfo lust_main_info = {1, 1, (const uint8_t *)"main", 4, 0};

static void lust_thread_info_release(void *p) {
    lust_ThreadInfo *t = (lust_ThreadInfo *)p;
    if (t && t != &lust_main_info && __atomic_sub_fetch(&t->rc, 1, 4) == 0) {
        free(t->name_buf);
        free(t);
    }
}
static int lust_cur_ready;
static void lust_cur_key_init(void) {
    pthread_key_create(&lust_cur_key, lust_thread_info_release);
    int one = 1;
    __atomic_store(&lust_cur_ready, &one, 3);
}

/* The calling thread's name for the panic hook: "main", the Builder's
   name, or "<unnamed>" (panic.c). */
static void lust_cur_thread_name(const uint8_t **p, uint64_t *n) {
    int ready;
    __atomic_load(&lust_cur_ready, &ready, 2);
    lust_ThreadInfo *t = ready ? (lust_ThreadInfo *)pthread_getspecific(lust_cur_key) : 0;
    if (!t) t = &lust_main_info;
    if (t->name) { *p = t->name; *n = t->name_len; }
    else { *p = (const uint8_t *)"<unnamed>"; *n = 9; }
}

#if defined(__linux__) || defined(__APPLE__)
/* The stack overflow handler's view of the calling thread (panic.c): a
   spawned thread's guard window and its name, "<unknown>" when it has
   none, as std's handler prints it. The main thread keeps the defaults. */
static void lust_cur_thread_guard(uintptr_t *lo, uintptr_t *hi, const uint8_t **name, uint64_t *n) {
    int ready;
    __atomic_load(&lust_cur_ready, &ready, 2);
    lust_ThreadInfo *t = ready ? (lust_ThreadInfo *)pthread_getspecific(lust_cur_key) : 0;
    if (!t || t == &lust_main_info || !t->guard_hi) return;
    *lo = t->guard_lo;
    *hi = t->guard_hi;
    if (t->name) { *name = t->name; *n = t->name_len; }
    else { *name = (const uint8_t *)"<unknown>"; *n = 9; }
}
#endif

/* Drops a packet's result: the value, or the payload of a panic. */
static void lust_packet_drop_result(lust_JoinHdr *h) {
    if (!h->panicked) { h->drop_ret(h); return; }
    if (h->payload.vt[0]) ((void (*)(void *))h->payload.vt[0])(h->payload.data);
    lust_dealloc(h->payload.data, (uint64_t)(uintptr_t)h->payload.vt[1], (uint64_t)(uintptr_t)h->payload.vt[2]);
}

/* Frees a packet whose result was moved out or dropped; a scoped one
   then counts down its scope. */
static void lust_packet_free(lust_JoinHdr *h) {
    lust_Scope *sc = h->scope;
    uint32_t one = 1;
    if (sc && h->panicked) __atomic_store(&sc->panicked, &one, 5);
    lust_thread_info_release(h->th);
    free(h);
    if (sc && __atomic_sub_fetch(&sc->live, 1, 5) == 0) lust_futex_wake(&sc->live, 0x7fffffff);
}

static void *lust_thread_entry(void *p) {
    lust_JoinHdr *h = (lust_JoinHdr *)p;
    lust_ThreadInfo *self = h->th;
    lust_thread_guard_init(&self->guard_lo, &self->guard_hi, &self->alt);
    pthread_setspecific(lust_cur_key, h->th);
#ifdef __APPLE__
    /* Darwin's pthread_setname_np names only the calling thread, so a
       Builder name is set here, as std sets it in the spawned thread
       on macOS (the parent-side call below is Linux's two-arg form). */
    if (self->name) {
        char nm[64];
        uint64_t n = self->name_len < 63 ? self->name_len : 63;
        uint64_t i = 0;
        for (; i < n && self->name[i] != 0; i++) nm[i] = (char)self->name[i];
        nm[i] = 0;
        pthread_setname_np(nm);
    }
#endif
    /* A panic that reached the thread's root ends the thread; its payload
       is the join's Err (std's spawn wrapper catches it). In mode 2 the
       root is a catch point. */
    lust_exc ex;
    if (lust_unwind_mode == 2) {
        if (lust_unw_try((void (*)(void *))h->run, h)) {
            lust_unw_take_any(&ex);
            h->payload = lust_exc_box(&ex);
            h->panicked = 1;
        }
    } else {
        h->run(h);
        if (lust_unwind_mode && lust_unw_pending && lust_unw_take(&ex)) {
            h->payload = lust_exc_box(&ex);
            h->panicked = 1;
        }
    }
    void *alt = self->alt;
    uint32_t one = 1;
    __atomic_store(&h->done, &one, 3);
    if (__atomic_sub_fetch(&h->rc, 1, 4) == 0) {
        lust_packet_drop_result(h);
        lust_packet_free(h);
    }
    lust_thread_guard_fini(alt);
    return 0;
}
#endif

/* Starts the thread for a filled packet (run, drop_ret, scope, ret, env). */
void lust_thread_spawn(lust_JoinHdr *h, lust_loc loc) LUST_BODY({
    if (h->scope) __atomic_add_fetch(&h->scope->live, 1, 5);
    pthread_once(&lust_cur_once, lust_cur_key_init);
    lust_ThreadInfo *t = (lust_ThreadInfo *)calloc(1, sizeof(lust_ThreadInfo));
    if (!t) lust_oom();
    t->rc = 2; /* the handle's (h->th) and the running thread's (its key) */
    t->id = __atomic_fetch_add(&lust_next_tid, 1, 0);
    t->name = 0;
    t->name_len = 0;
    t->name_buf = 0;
    h->th = t;
    h->rc = 2;
    h->done = 0;
    h->panicked = 0;
    if (pthread_create(LUST_PTHREAD_ID_P(&h->t), 0, lust_thread_entry, h) != 0) lust_panic_cstr_owned("failed to spawn thread", loc);
})

/* A `thread::Builder` value (cgen/sync.rs lays it out): the
   name as a String, the stack size in bytes (0 = default), and
   whether a name was set (an empty name is Some("")). */
typedef struct lust_ThreadBuilder {
    lust_String name;
    uint64_t stack;
    uint8_t has_name;
    uint8_t has_stack;
} lust_ThreadBuilder;

#if !defined(__APPLE__) && !defined(_WIN32)
int pthread_attr_init(void *attr);
int pthread_attr_destroy(void *attr);
int pthread_attr_setstacksize(void *attr, uint64_t size);
int pthread_setname_np(uintptr_t t, const char *name);
#endif

/* `Builder::spawn`: lust_thread_spawn with the builder's name
   and stack size. 0, or the errno of the failed call (the caller
   builds std's io::Error from it). The name bytes are copied:
   the ThreadInfo owns them. An interior NUL truncates the OS
   thread name; the Thread::name value keeps the full string, as
   std's does (its CString would fail there: see the check for
   the panic on a NUL in the caller). */
int lust_thread_spawn_builder(lust_JoinHdr *h, const lust_ThreadBuilder *b) LUST_BODY({
    if (h->scope) __atomic_add_fetch(&h->scope->live, 1, 5);
    pthread_once(&lust_cur_once, lust_cur_key_init);
    lust_ThreadInfo *t = (lust_ThreadInfo *)calloc(1, sizeof(lust_ThreadInfo));
    if (!t) lust_oom();
    t->rc = 2;
    t->id = __atomic_fetch_add(&lust_next_tid, 1, 0);
    t->name = 0;
    t->name_len = 0;
    t->name_buf = 0;
    if (b->has_name) {
        uint8_t *copy = (uint8_t *)malloc(b->name.len ? b->name.len : 1);
        if (!copy) lust_oom();
        for (uint64_t i = 0; i < b->name.len; i++) copy[i] = b->name.ptr[i];
        t->name = copy;
        t->name_len = b->name.len;
        t->name_buf = copy;
    }
    h->th = t;
    h->rc = 2;
    h->done = 0;
    h->panicked = 0;
    /* pthread_attr_t is opaque and platform-sized; a zeroed
       64-byte block covers glibc's (56 bytes) on the targets
       lust runs on. On Darwin the real pthread_attr_t stands here:
       a zeroed block is not a valid Darwin attr (it holds a
       signature word pthread_attr_init writes). */
#ifdef __APPLE__
    pthread_attr_t attr;
    void *attrp = 0;
    if (b->has_stack) {
        pthread_attr_init(&attr);
        uint64_t sz = b->stack < 16384 ? 16384 : b->stack;
        int e2 = pthread_attr_setstacksize(&attr, (size_t)sz);
        if (e2 != 0) {
            pthread_attr_destroy(&attr);
            lust_thread_info_release(t);
            lust_thread_info_release(t);
            if (h->scope) __atomic_sub_fetch(&h->scope->live, 1, 5);
            return e2;
        }
        attrp = &attr;
    }
#else
    uint64_t attr[8] = {0, 0, 0, 0, 0, 0, 0, 0};
    void *attrp = 0;
#ifndef _WIN32
    if (b->has_stack) {
        pthread_attr_init(attr);
        uint64_t sz = b->stack < 16384 ? 16384 : b->stack;
        int e2 = pthread_attr_setstacksize(attr, sz);
        if (e2 != 0) {
            pthread_attr_destroy(attr);
            lust_thread_info_release(t);
            lust_thread_info_release(t);
            if (h->scope) __atomic_sub_fetch(&h->scope->live, 1, 5);
            return e2;
        }
        attrp = attr;
    }
#endif
#endif
    int r = pthread_create(LUST_PTHREAD_ID_P(&h->t), attrp, lust_thread_entry, h);
#ifndef _WIN32
    if (attrp) pthread_attr_destroy(attrp);
#endif
    if (r != 0) {
        lust_thread_info_release(t);
        lust_thread_info_release(t);
        if (h->scope) __atomic_sub_fetch(&h->scope->live, 1, 5);
        return r;
    }
#ifndef __APPLE__
    if (b->has_name) {
        char nm[16];
        uint64_t n = b->name.len < 15 ? b->name.len : 15;
        uint64_t i = 0;
        for (; i < n && b->name.ptr[i] != 0; i++) nm[i] = (char)b->name.ptr[i];
        nm[i] = 0;
        #ifndef _WIN32

        pthread_setname_np(h->t, nm);

        #endif
    }
#endif
    return 0;
})

/* ---------- `thread::available_parallelism` ----------
   A port of std 1.99's Linux code (library/std/src/sys/thread/unix.rs:
   `available_parallelism` and `mod cgroups`), step for step, so the
   answer is rustc's on the same box: min(affinity count, cgroup quota),
   the quota rounded down and at least 1, sysconf as the fallback. Paths
   follow Rust's `PathBuf` push/pop/starts_with/strip_prefix on Unix. */
#ifdef LUST_IMPL
int access(const char *path, int mode);
long sysconf(int name);
#if defined(__APPLE__)
#define LUST_SC_NPROCESSORS_ONLN 58
#else
#define LUST_SC_NPROCESSORS_ONLN 84
#endif

/* A growable byte path (`PathBuf`). */
typedef struct { char *p; size_t n, cap; } lust_pb;

static void lust_pb_add(lust_pb *b, const char *s, size_t n) {
    if (b->n + n + 1 > b->cap) {
        b->cap = (b->n + n + 1) * 2;
        b->p = (char *)realloc(b->p, b->cap);
    }
    memcpy(b->p + b->n, s, n);
    b->n += n;
    b->p[b->n] = 0;
}

/* `PathBuf::push`: an absolute path replaces; else a separator first
   when the buffer is not empty and does not end in one. */
static void lust_pb_push(lust_pb *b, const char *s, size_t n) {
    int need_sep = b->n > 0 && b->p[b->n - 1] != '/';
    if (n > 0 && s[0] == '/') b->n = 0;
    else if (need_sep) lust_pb_add(b, "/", 1);
    lust_pb_add(b, s, n);
}

/* Bytes before the body of a path: the root `/`, or a leading `.` that
   is its own component (`Components::len_before_body`). */
static size_t lust_path_front(const char *p, size_t n) {
    if (n > 0 && p[0] == '/') return 1;
    if (n > 0 && p[0] == '.' && (n == 1 || p[1] == '/')) return 1;
    return 0;
}

/* The last body component of p[lb..end) from the back: its start; *sep
   gets the index of the separator before it (or lb when there is none). */
static size_t lust_path_back(const char *p, size_t lb, size_t end, size_t *cut) {
    size_t s = end;
    while (s > lb && p[s - 1] != '/') s--;
    *cut = s > lb ? s - 1 : lb;
    return s;
}

static int lust_path_skip(const char *c, size_t n) { return n == 0 || (n == 1 && c[0] == '.'); }

/* The end of a path slice after `Components::trim_right`. */
static size_t lust_path_trim_right(const char *p, size_t lb, size_t end) {
    while (end > lb) {
        size_t cut, s = lust_path_back(p, lb, end, &cut);
        if (!lust_path_skip(p + s, end - s)) break;
        end = cut;
    }
    return end;
}

/* `PathBuf::pop`: 0 when the last component is the root or there is none. */
static int lust_pb_pop(lust_pb *b) {
    size_t lb = lust_path_front(b->p, b->n), end = b->n;
    for (;;) {
        if (end <= lb) return 0;
        size_t cut, s = lust_path_back(b->p, lb, end, &cut);
        int skip = lust_path_skip(b->p + s, end - s);
        end = cut;
        if (!skip) break;
    }
    b->n = lust_path_trim_right(b->p, lb, end);
    b->p[b->n] = 0;
    return 1;
}

/* One forward component of a path: kind 0 root, 1 cur dir, 2 normal
   (`..` is a normal string here: it compares equal only to `..`). */
typedef struct { const char *p; size_t n, i; int started; } lust_pcomps;

static int lust_pcomp_next(lust_pcomps *c, int *kind, const char **s, size_t *len) {
    if (!c->started) {
        c->started = 1;
        if (c->n > 0 && c->p[0] == '/') { c->i = 1; *kind = 0; *s = c->p; *len = 1; return 1; }
        if (c->n > 0 && c->p[0] == '.' && (c->n == 1 || c->p[1] == '/')) { c->i = 1; *kind = 1; *s = c->p; *len = 1; return 1; }
    }
    while (c->i < c->n) {
        size_t a = c->i;
        while (c->i < c->n && c->p[c->i] != '/') c->i++;
        size_t e = c->i;
        if (c->i < c->n) c->i++;
        if (!lust_path_skip(c->p + a, e - a)) { *kind = 2; *s = c->p + a; *len = e - a; return 1; }
    }
    return 0;
}

/* `Path::strip_prefix` (`starts_with` when `rest` is null): 1 when every
   component of `base` matches the front of `p`; *rest is the remainder
   as `Components::as_path` gives it (both ends trimmed). */
static int lust_path_strip(const char *p, size_t n, const char *base, size_t bn, const char **rest, size_t *rest_n) {
    lust_pcomps a = {p, n, 0, 0}, b = {base, bn, 0, 0};
    int ka, kb;
    const char *sa, *sb;
    size_t la, lbn;
    while (lust_pcomp_next(&b, &kb, &sb, &lbn)) {
        if (!lust_pcomp_next(&a, &ka, &sa, &la) || ka != kb || la != lbn || memcmp(sa, sb, la) != 0) return 0;
    }
    if (rest) {
        size_t i = a.started ? a.i : 0;
        /* trim_left: separators and `.` components before the next one,
           once a component was taken (an untouched iterator keeps all). */
        for (; a.started;) {
            while (i < n && p[i] == '/') i++;
            if (i < n && p[i] == '.' && (i + 1 == n || p[i + 1] == '/')) { i++; continue; }
            break;
        }
        size_t end = lust_path_trim_right(p, i, n);
        *rest = p + i;
        *rest_n = end > i ? end - i : 0;
    }
    return 1;
}

static int lust_path_starts(const char *p, size_t n, const char *base) {
    return lust_path_strip(p, n, base, strlen(base), 0, 0);
}

/* `File::open(path)` then `read_to_string` (or `read_to_end` when
   `text` is 0). 1 on success with the bytes in *out (malloc'd). */
static int lust_read_file(const char *path, int text, char **out, size_t *len) {
    FILE *f = fopen(path, "re");
    if (!f) return 0;
    size_t cap = 256, n = 0;
    char *buf = (char *)malloc(cap);
    for (;;) {
        if (n == cap) buf = (char *)realloc(buf, cap *= 2);
        size_t got = fread(buf + n, 1, cap - n, f);
        n += got;
        if (got == 0) break;
    }
    int bad = ferror(f);
    fclose(f);
    uint64_t up;
    uint8_t el;
    if (bad || (text && !lust_utf8_check((const uint8_t *)buf, n, &up, &el))) {
        free(buf);
        return 0;
    }
    *out = buf;
    *len = n;
    return 1;
}

/* `str::parse::<usize>`: an optional `+`, then one or more ASCII digits,
   with no overflow. */
static int lust_parse_usize(const char *s, size_t n, uint64_t *v) {
    if (n > 0 && s[0] == '+') { s++; n--; }
    if (n == 0) return 0;
    uint64_t r = 0;
    for (size_t i = 0; i < n; i++) {
        if (s[i] < '0' || s[i] > '9') return 0;
        uint64_t d = (uint64_t)(s[i] - '0');
        if (r > (UINT64_MAX - d) / 10) return 0;
        r = r * 10 + d;
    }
    *v = r;
    return 1;
}

static int lust_path_exists(const lust_pb *b) { return access(b->p, 0) == 0; }

/* `quota_v2`: the smallest cpu.max quota of the cgroup and its parents. */
static uint64_t lust_quota_v2(const char *group, size_t gn) {
    uint64_t quota = UINT64_MAX;
    const char *mount = "/sys/fs/cgroup";
    lust_pb path = {0, 0, 0};
    lust_pb_push(&path, mount, strlen(mount));
    lust_pb_push(&path, group, gn);
    lust_pb_push(&path, "cgroup.controllers", 18);
    if (!lust_path_exists(&path)) { free(path.p); return UINT64_MAX; }
    lust_pb_pop(&path);
    while (lust_path_starts(path.p, path.n, mount)) {
        lust_pb_push(&path, "cpu.max", 7);
        char *buf;
        size_t n;
        if (lust_read_file(path.p, 1, &buf, &n)) {
            /* `read_buf.lines().next()?`, then two ' '-split fields; a
               missing one ends the whole walk (the `?` leaves the try block). */
            if (n == 0) { free(buf); break; }
            size_t e = 0;
            while (e < n && buf[e] != '\n') e++;
            if (e < n && e > 0 && buf[e - 1] == '\r') e--; /* `\r` only before `\n` */
            size_t sp = 0;
            while (sp < e && buf[sp] != ' ') sp++;
            if (sp == e) { free(buf); break; }
            size_t pe = sp + 1;
            while (pe < e && buf[pe] != ' ') pe++;
            uint64_t limit, period;
            if (lust_parse_usize(buf, sp, &limit) && lust_parse_usize(buf + sp + 1, pe - sp - 1, &period) && period > 0) {
                uint64_t q = limit / period;
                if (q < quota) quota = q;
            }
            free(buf);
        }
        lust_pb_pop(&path);
        lust_pb_pop(&path);
    }
    free(path.p);
    return quota;
}

/* `parse_file` of quota_v1: <dir>/<name> trimmed, as a usize. */
static int lust_v1_file(lust_pb *path, const char *name, uint64_t *v) {
    lust_pb_push(path, name, strlen(name));
    char *buf;
    size_t n;
    int ok = lust_read_file(path->p, 1, &buf, &n);
    lust_pb_pop(path);
    if (!ok) return 0;
    lust_str t = lust_str_trim((lust_str){(uint8_t *)buf, n});
    ok = lust_parse_usize((const char *)t.ptr, t.len, v);
    free(buf);
    return ok;
}

/* Field `idx` of `s` split on ' '. */
static void lust_field(const char *s, size_t n, size_t idx, const char **f, size_t *fn) {
    size_t a = 0, k = 0;
    for (size_t i = 0; i <= n; i++) {
        if (i == n || s[i] == ' ') {
            if (k++ == idx) { *f = s + a; *fn = i - a; return; }
            a = i + 1;
        }
    }
    *f = s + n;
    *fn = 0;
}

/* `find_mountpoint`: a cgroup v1 mount with the cpu controller whose
   root holds `group`. 1 with *mount (malloc'd) and the group path
   below it; 0 for std's None. */
static int lust_v1_mountpoint(const char *group, size_t gn, char **mount, const char **sub, size_t *sub_n) {
    char *buf;
    size_t n;
    /* read_line fails on bytes that are not UTF-8: read all of it, and
       stop at the first line that is not valid. */
    if (!lust_read_file("/proc/self/mountinfo", 0, &buf, &n)) return 0;
    int found = 0;
    size_t at = 0;
    while (at < n) {
        size_t e = at;
        while (e < n && buf[e] != '\n') e++;
        size_t next = e < n ? e + 1 : e;
        uint64_t up;
        uint8_t el;
        if (!lust_utf8_check((const uint8_t *)buf + at, next - at, &up, &el)) break;
        lust_str line = lust_str_trim((lust_str){(uint8_t *)buf + at, e - at});
        at = next;
        /* Fields split on ' ': sub_path = [3], mount_point = [4],
           mount_opts = last, filesystem_type = third from last. */
        const char *L = (const char *)line.ptr;
        size_t k = 1;
        for (size_t i = 0; i < line.len; i++) k += L[i] == ' ';
        if (k < 8) break;
        const char *sp, *mp, *opts, *fs;
        size_t spn, mpn, optn, fsn;
        lust_field(L, line.len, 3, &sp, &spn);
        lust_field(L, line.len, 4, &mp, &mpn);
        lust_field(L, line.len, k - 1, &opts, &optn);
        lust_field(L, line.len, k - 3, &fs, &fsn);
        int cpu = 0;
        for (size_t i = 0, b = 0; i <= optn; i++) {
            if (i == optn || opts[i] == ',') {
                if (i - b == 3 && memcmp(opts + b, "cpu", 3) == 0) cpu = 1;
                b = i + 1;
            }
        }
        if (fsn != 6 || memcmp(fs, "cgroup", 6) != 0 || !cpu) continue;
        const char *rel;
        size_t reln;
        if (!lust_path_strip(sp, spn, "/", 1, &rel, &reln)) break;
        const char *rest;
        size_t restn;
        if (!lust_path_strip(group, gn, rel, reln, &rest, &restn)) continue;
        *mount = (char *)malloc(mpn + 1);
        memcpy(*mount, mp, mpn);
        (*mount)[mpn] = 0;
        *sub = rest;
        *sub_n = restn;
        found = 1;
        break;
    }
    free(buf);
    return found;
}

/* `quota_v1`: the first cpu-controller mount that holds the cgroup,
   walked up to the mount's root. */
static uint64_t lust_quota_v1(const char *group, size_t gn) {
    uint64_t quota = UINT64_MAX;
    lust_pb path = {0, 0, 0};
    for (int m = 0; m < 3; m++) {
        char *owned = 0;
        const char *mount, *g = group;
        size_t g_n = gn;
        if (m == 0) mount = "/sys/fs/cgroup/cpu";
        else if (m == 1) mount = "/sys/fs/cgroup/cpu,cpuacct";
        else if (lust_v1_mountpoint(group, gn, &owned, &g, &g_n)) mount = owned;
        else continue;
        path.n = 0;
        lust_pb_push(&path, mount, strlen(mount));
        lust_pb_push(&path, g, g_n);
        if (!lust_path_exists(&path)) { free(owned); continue; }
        while (lust_path_starts(path.p, path.n, mount)) {
            uint64_t limit, period;
            int has_l = lust_v1_file(&path, "cpu.cfs_quota_us", &limit);
            int has_p = lust_v1_file(&path, "cpu.cfs_period_us", &period);
            if (has_l && has_p && period > 0 && limit / period < quota) quota = limit / period;
            lust_pb_pop(&path);
        }
        free(owned);
        break;
    }
    free(path.p);
    return quota;
}

/* `cgroups::quota`: the CPU quota in whole cores (rounded down), or
   UINT64_MAX when there is none or it cannot be read. */
static uint64_t lust_cgroup_quota(void) {
    char *buf;
    size_t n;
    if (!lust_read_file("/proc/self/cgroup", 0, &buf, &n)) return UINT64_MAX;
    /* The fold over the lines: (path, version) of the last v1 line with
       the cpu controller, else of the last v2 line; a matching line
       with no third field resets it (the `?` in the closure). */
    int have = 0, v1 = 0;
    size_t ps = 0, pn = 0;
    for (size_t at = 0;;) {
        size_t e = at;
        while (e < n && buf[e] != '\n') e++;
        const char *c1 = memchr(buf + at, ':', e - at);
        if (c1) {
            const char *f1 = c1 + 1;
            const char *c2 = memchr(f1, ':', (size_t)(buf + e - f1));
            size_t f1n = (size_t)((c2 ? c2 : buf + e) - f1);
            int ver = 0; /* 2: v2, 1: v1 */
            if (f1n == 0) ver = 2;
            else {
                uint64_t up;
                uint8_t el;
                if (lust_utf8_check((const uint8_t *)f1, f1n, &up, &el)) {
                    for (size_t i = 0, b = 0; i <= f1n; i++) {
                        if (i == f1n || f1[i] == ',') {
                            if (i - b == 3 && memcmp(f1 + b, "cpu", 3) == 0) ver = 1;
                            b = i + 1;
                        }
                    }
                }
            }
            if (ver && !(have && ver == 2)) {
                if (!c2) have = 0;
                else {
                    size_t f2 = (size_t)(c2 + 1 - buf), f2n = e - f2;
                    if (f2n == 0) {
                        free(buf);
                        lust_panic_std("range start index 1 out of range for slice of length 0", "std/src/sys/thread/unix.rs:843:31");
                        return UINT64_MAX;
                    }
                    have = 1;
                    v1 = ver == 1;
                    ps = f2 + 1;
                    pn = f2n - 1;
                }
            }
        }
        if (e >= n) break;
        at = e + 1;
    }
    uint64_t q = UINT64_MAX;
    if (have) q = v1 ? lust_quota_v1(buf + ps, pn) : lust_quota_v2(buf + ps, pn);
    free(buf);
    return q;
}

#endif

/* `thread::available_parallelism` on Linux: the count (> 0), or 0 with
   *err an errno (`last_os_error`) or -1 (`UNKNOWN_THREAD_COUNT`). */
uint64_t lust_avail_parallelism(int32_t *err) LUST_BODY({
    *err = 0;
    uint64_t quota = UINT64_MAX;
#if defined(__linux__)
    quota = lust_cgroup_quota();
    if (lust_unw_pending && lust_unw_mine()) return 1;
    if (quota < 1) quota = 1;
#ifdef LUST_SYS_SCHED_GETAFFINITY
    /* glibc's cpu_set_t is 128 bytes; the wrapper zeroes what the
       kernel does not write, as the zeroed mask here. */
    uint64_t mask[16] = {0};
    if (syscall(LUST_SYS_SCHED_GETAFFINITY, 0, sizeof mask, mask) >= 0) {
        uint64_t count = 0;
        for (int i = 0; i < 16; i++) count += (uint64_t)__builtin_popcountll(mask[i]);
        if (count > quota) count = quota;
        if (count > 0) return count;
    }
#endif
#endif
#ifdef _WIN32
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    long cpus = (long)si.dwNumberOfProcessors;
#else
    long cpus = sysconf(LUST_SC_NPROCESSORS_ONLN);
#endif
    if (cpus == -1) {
        *err = errno;
        return 0;
    }
    if (cpus == 0) {
        *err = -1;
        return 0;
    }
    return (uint64_t)cpus < quota ? (uint64_t)cpus : quota;
})

/* Waits for the thread; the caller then moves the result out and calls
   lust_join_free. */
void lust_thread_join(lust_JoinHdr *h) LUST_BODY({ pthread_join(LUST_PTHREAD_ID(h->t), 0); })

void lust_join_free(lust_JoinHdr *h) LUST_BODY({ lust_packet_free(h); })

/* A dropped JoinHandle detaches its thread. */
void lust_join_drop(lust_JoinHdr *h) LUST_BODY({
    pthread_detach(LUST_PTHREAD_ID(h->t));
    if (__atomic_sub_fetch(&h->rc, 1, 4) == 0) {
        lust_packet_drop_result(h);
        lust_packet_free(h);
    }
})

int lust_join_is_finished(lust_JoinHdr *h) LUST_BODY({
    uint32_t d;
    __atomic_load(&h->done, &d, 2);
    return d != 0;
})

/* `thread::current()`: a new reference to this thread's info. */
lust_ThreadInfo *lust_thread_current(void) LUST_BODY({
    pthread_once(&lust_cur_once, lust_cur_key_init);
    lust_ThreadInfo *t = (lust_ThreadInfo *)pthread_getspecific(lust_cur_key);
    if (!t) return &lust_main_info;
    __atomic_add_fetch(&t->rc, 1, 0);
    return t;
})

lust_ThreadInfo *lust_thread_clone(lust_ThreadInfo *t) LUST_BODY({
    if (t != &lust_main_info) __atomic_add_fetch(&t->rc, 1, 0);
    return t;
})

void lust_thread_drop(lust_ThreadInfo *t) LUST_BODY({ lust_thread_info_release(t); })

void lust_thread_yield(void) LUST_BODY({ sched_yield(); })

/* The end of `thread::scope`: waits for every packet of the scope. */
void lust_scope_wait(lust_Scope *sc) LUST_BODY({
    for (;;) {
        uint32_t n;
        __atomic_load(&sc->live, &n, 5);
        if (n == 0) return;
        lust_futex_wait(&sc->live, n);
    }
})

/* ---------- mpsc channels ----------

   One lust_Chan per channel, shared by every Sender/SyncSender and the
   Receiver (each of those is a lust_Chan*). A futex mutex guards it; `seq`
   is a condition word (waiters sleep on its value, every change bumps it
   and wakes all, if `sleepers`, the threads that sleep on it, is not 0:
   both change only under the lock). Values move in and out by memcpy of
   `esize` bytes.
   `cap` is -1 for `channel()` and the bound for `sync_channel(n)`; a
   bound of 0 is a rendezvous: a sender places its value only while a
   receiver waits, then waits until that value is taken. A rendezvous
   sender that waits counts itself in `tx_waiting`, so `try_recv` and an
   expired `recv_timeout` still pair with it, as std's zero-capacity
   channel does; such a receiver counts in `rx_claim`, which `try_send`
   does not see (it pairs only with a blocked receiver). The channel is
   freed when the last sender and the receiver are gone; values still
   queued then drop through `drop_elem`. */
typedef struct lust_Chan {
    uint32_t lock;
    uint32_t seq;
    uint32_t sleepers;
    uint64_t senders;
    uint32_t rx_alive;
    uint32_t rx_waiting;
    uint32_t tx_waiting;
    uint32_t rx_claim;
    int64_t cap;
    uint64_t esize;
    uint64_t ealign;
    uint8_t *buf;
    uint64_t head, len, bufcap;
    uint64_t taken;
    void (*drop_elem)(void *);
} lust_Chan;

#ifdef LUST_IMPL
/* Lock held. Without a sleeper a change makes no futex call: each send and
   recv woke all before, with no one to wake (tsgo: thousands per run). */
static void lust_chan_changed(lust_Chan *c) {
    __atomic_add_fetch(&c->seq, 1, 5);
    if (c->sleepers) lust_futex_wake(&c->seq, 0x7fffffff);
}
/* Sleeps until the channel changes; the lock is dropped meanwhile. A change
   between the unlock and the sleep sees this sleeper and bumps `seq`, so the
   futex wait returns at once. */
static void lust_chan_wait(lust_Chan *c) {
    uint32_t s;
    c->sleepers++;
    __atomic_load(&c->seq, &s, 5);
    lust_mutex_unlock(&c->lock);
    lust_futex_wait(&c->seq, s);
    lust_mutex_lock(&c->lock);
    c->sleepers--;
}
static void lust_chan_push(lust_Chan *c, const void *v) {
    if (c->esize == 0) { c->len++; return; }
    if (c->len == c->bufcap) {
        uint64_t nc = c->bufcap ? c->bufcap * 2 : 4;
        uint8_t *nb = (uint8_t *)lust_alloc(nc * c->esize, c->ealign);
        for (uint64_t i = 0; i < c->len; i++) memcpy(nb + i * c->esize, c->buf + ((c->head + i) % c->bufcap) * c->esize, c->esize);
        if (c->bufcap && c->esize) lust_dealloc(c->buf, c->bufcap * c->esize, c->ealign);
        c->buf = nb;
        c->bufcap = nc;
        c->head = 0;
    }
    memcpy(c->buf + ((c->head + c->len) % c->bufcap) * c->esize, v, c->esize);
    c->len++;
}
static void lust_chan_pop(lust_Chan *c, void *out) {
    if (c->esize == 0) { c->len--; c->taken++; return; }
    memcpy(out, c->buf + c->head * c->esize, c->esize);
    c->head = (c->head + 1) % c->bufcap;
    c->len--;
    c->taken++;
}
/* Drops the queued values and frees the channel (lock held, last user). */
static void lust_chan_drain(lust_Chan *c) {
    if (c->drop_elem)
        for (uint64_t i = 0; i < c->len; i++) c->drop_elem(c->esize ? c->buf + ((c->head + i) % c->bufcap) * c->esize : (uint8_t *)(uintptr_t)c->ealign);
    c->len = 0;
}
static void lust_chan_free(lust_Chan *c) {
    lust_chan_drain(c);
    if (c->bufcap && c->esize) lust_dealloc(c->buf, c->bufcap * c->esize, c->ealign);
    free(c);
}
#endif

#ifdef LUST_IMPL
/* A receiver that does not block (`try_recv`, an expired
   `recv_timeout`) on a rendezvous channel with a waiting sender: that
   sender is committed (std pairs with it at once), so wait for its
   value and take it. The lock is held on entry and released here. */
static void lust_chan_claim(lust_Chan *c, void *out) {
    c->rx_claim++;
    lust_chan_changed(c);
    while (c->len == 0) lust_chan_wait(c);
    lust_chan_pop(c, out);
    c->rx_claim--;
    lust_chan_changed(c);
    lust_mutex_unlock(&c->lock);
}
#endif

lust_Chan *lust_chan_new(uint64_t esize, uint64_t ealign, int64_t cap, void (*drop_elem)(void *)) LUST_BODY({
    lust_Chan *c = (lust_Chan *)calloc(1, sizeof(lust_Chan));
    if (!c) lust_oom();
    c->senders = 1;
    c->rx_alive = 1;
    c->cap = cap;
    c->esize = esize;
    c->ealign = ealign;
    c->drop_elem = drop_elem;
    return c;
})

/* 0 sent; 1 the receiver is gone (the value stays with the caller). */
int lust_chan_send(lust_Chan *c, const void *v) LUST_BODY({
    lust_mutex_lock(&c->lock);
    if (c->cap == 0) {
        c->tx_waiting++;
        while (c->rx_alive && (c->rx_waiting + c->rx_claim == 0 || c->len > 0)) lust_chan_wait(c);
        c->tx_waiting--;
        if (!c->rx_alive) { lust_mutex_unlock(&c->lock); return 1; }
        uint64_t ticket = c->taken;
        lust_chan_push(c, v);
        lust_chan_changed(c);
        while (c->taken == ticket && c->rx_alive) lust_chan_wait(c);
        lust_mutex_unlock(&c->lock);
        return 0;
    }
    while (c->rx_alive && c->cap > 0 && c->len >= (uint64_t)c->cap) lust_chan_wait(c);
    if (!c->rx_alive) { lust_mutex_unlock(&c->lock); return 1; }
    lust_chan_push(c, v);
    lust_chan_changed(c);
    lust_mutex_unlock(&c->lock);
    return 0;
})

/* try_send: 0 sent; 1 full (the value stays with the caller);
   2 the receiver is gone (the value stays with the caller). A
   rendezvous channel (cap 0) sends only into a waiting receiver
   with nothing queued, as std's does. */
int lust_chan_try_send(lust_Chan *c, const void *v) LUST_BODY({
    lust_mutex_lock(&c->lock);
    if (!c->rx_alive) { lust_mutex_unlock(&c->lock); return 2; }
    if (c->cap == 0) {
        if (c->rx_waiting == 0 || c->len > 0) { lust_mutex_unlock(&c->lock); return 1; }
    } else if (c->cap > 0 && c->len >= (uint64_t)c->cap) {
        lust_mutex_unlock(&c->lock);
        return 1;
    }
    lust_chan_push(c, v);
    lust_chan_changed(c);
    lust_mutex_unlock(&c->lock);
    return 0;
})

/* 0 received into out; 1 every sender is gone and nothing is queued.
   `block` 0 is try_recv: 1 means empty, 2 disconnected. */
int lust_chan_recv(lust_Chan *c, void *out, int block) LUST_BODY({
    lust_mutex_lock(&c->lock);
    c->rx_waiting++;
    lust_chan_changed(c);
    for (;;) {
        if (c->len > 0) {
            lust_chan_pop(c, out);
            c->rx_waiting--;
            lust_chan_changed(c);
            lust_mutex_unlock(&c->lock);
            return 0;
        }
        if (c->senders == 0) { c->rx_waiting--; lust_mutex_unlock(&c->lock); return block ? 1 : 2; }
        if (!block) {
            c->rx_waiting--;
            if (c->cap == 0 && c->tx_waiting > 0) { lust_chan_claim(c, out); return 0; }
            lust_mutex_unlock(&c->lock);
            return 1;
        }
        lust_chan_wait(c);
    }
})

lust_Chan *lust_chan_clone_sender(lust_Chan *c) LUST_BODY({
    lust_mutex_lock(&c->lock);
    c->senders++;
    lust_mutex_unlock(&c->lock);
    return c;
})

void lust_chan_drop_sender(lust_Chan *c) LUST_BODY({
    lust_mutex_lock(&c->lock);
    if (--c->senders == 0) {
        if (!c->rx_alive) { lust_chan_free(c); return; }
        lust_chan_changed(c);
    }
    lust_mutex_unlock(&c->lock);
})

void lust_chan_drop_receiver(lust_Chan *c) LUST_BODY({
    lust_mutex_lock(&c->lock);
    c->rx_alive = 0;
    if (c->senders == 0) { lust_chan_free(c); return; }
    /* Queued values drop now, as std's receiver drop does. */
    lust_chan_drain(c);
    lust_chan_changed(c);
    lust_mutex_unlock(&c->lock);
})

/* ---------- Condvar, Barrier, OnceLock ----------

   A Condvar is a sequence word: a waiter reads it, unlocks the mutex,
   sleeps while it is unchanged, and locks again; a notify bumps it and
   wakes one or all (spurious wakeups are allowed, as std documents).
   A Barrier is { lock, seq, count, generation, n }: the n-th arrival
   starts a new generation, wakes the others, and is the leader. A
   OnceLock<T> is { state, T }: 0 empty, 1 being set, 2 set, 3 being set
   with a thread waiting (std's QUEUED): only then does the end of the set
   wake (it woke always before: one futex call per init, 23,380 on tsgo's
   effect check). */
void lust_cv_wait(uint32_t *cv, uint32_t *m) LUST_BODY({
    uint32_t s;
    __atomic_load(cv, &s, 5);
    lust_mutex_unlock(m);
    lust_futex_wait(cv, s);
    lust_mutex_lock(m);
})

void lust_cv_notify(uint32_t *cv, int all) LUST_BODY({
    __atomic_add_fetch(cv, 1, 5);
    lust_futex_wake(cv, all ? 0x7fffffff : 1);
})

/* `Condvar::wait_timeout`: like lust_cv_wait, but 1 when the
   duration elapsed with no notify seen (std's timed_out: true
   only when the timeout is known to have elapsed). The deadline
   is monotonic, as std's is on Linux; the futex wait takes the
   remaining time (a relative timeout, CLOCK_MONOTONIC). */
#if defined(__linux__)
#if defined(__x86_64__)
#define LUST_SYS_CLOCK_GETTIME 228
#elif defined(__aarch64__)
#define LUST_SYS_CLOCK_GETTIME 113
#endif
#endif

static void lust_mono_now(long ts[2]) LUST_BODY({
#ifdef _WIN32
    LARGE_INTEGER freq, now;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&now);
    ts[0] = (long)(now.QuadPart / freq.QuadPart);
    ts[1] = (long)(((now.QuadPart % freq.QuadPart) * 1000000000) / freq.QuadPart);
#elif defined(LUST_SYS_CLOCK_GETTIME)
    syscall(LUST_SYS_CLOCK_GETTIME, 1 /* CLOCK_MONOTONIC */, ts);
#elif defined(__APPLE__)
    /* struct timespec is two longs on Darwin, as ts is. std's
       Instant on Apple is CLOCK_UPTIME_RAW (mach_absolute_time),
       the clock its timeouts are measured against. */
    clock_gettime(CLOCK_UPTIME_RAW, (void *)ts);
#else
    ts[0] = 0;
    ts[1] = 0;
#endif
})

/* `Instant::now()` for `Condvar::wait_timeout_while` (cgen/sync.rs). */
void lust_cv_start(long ts[2]) LUST_BODY({ lust_mono_now(ts); })

/* std's `dur.checked_sub(start.elapsed())`: 1 and the time left in
   `*rs`/`*rn`, or 0 when the elapsed time is past `dur`. */
int lust_cv_left(const long start[2], uint64_t secs, uint32_t nanos, uint64_t *rs, uint32_t *rn) LUST_BODY({
    long now[2];
    lust_mono_now(now);
    uint64_t es = (uint64_t)(now[0] - start[0]);
    long en = now[1] - start[1];
    if (en < 0) {
        es -= 1;
        en += 1000000000L;
    }
    if (es > secs || (es == secs && (uint32_t)en > nanos)) return 0;
    uint64_t s = secs - es;
    long n = (long)nanos - en;
    if (n < 0) {
        s -= 1;
        n += 1000000000L;
    }
    *rs = s;
    *rn = (uint32_t)n;
    return 1;
})

int lust_cv_wait_timeout(uint32_t *cv, uint32_t *m, uint64_t secs, uint32_t nanos) LUST_BODY({
    long dl[2];
    lust_mono_now(dl);
    dl[0] += (long)secs;
    dl[1] += (long)nanos;
    if (dl[1] >= 1000000000L) {
        dl[0] += 1;
        dl[1] -= 1000000000L;
    }
    uint32_t s;
    __atomic_load(cv, &s, 5);
    lust_mutex_unlock(m);
    int timed = 0;
    for (;;) {
        uint32_t cur;
        __atomic_load(cv, &cur, 5);
        if (cur != s) break;
        long now[2];
        lust_mono_now(now);
        long rs = dl[0] - now[0];
        long rn = dl[1] - now[1];
        if (rn < 0) {
            rs -= 1;
            rn += 1000000000L;
        }
        if (rs < 0) {
            timed = 1;
            break;
        }
        lust_futex_wait_for(cv, s, rs, rn);
    }
    lust_mutex_lock(m);
    return timed;
})

/* `Receiver::recv_timeout`: 0 received into out; 1 timed out; 2 every
   sender is gone and nothing is queued. std's order: a queued value
   first, then disconnection, then the deadline (`Instant::now() +
   timeout`, monotonic). A deadline past what `Instant` holds (tv_sec
   overflows i64) is std's plain `recv`. */
int lust_chan_recv_timeout(lust_Chan *c, void *out, uint64_t secs, uint32_t nanos) LUST_BODY({
    long dl[2];
    lust_mono_now(dl);
    if (secs > (uint64_t)(0x7fffffffffffffffL - dl[0])) return lust_chan_recv(c, out, 1) ? 2 : 0;
    dl[0] += (long)secs;
    dl[1] += (long)nanos;
    if (dl[1] >= 1000000000L) {
        if (dl[0] == 0x7fffffffffffffffL) return lust_chan_recv(c, out, 1) ? 2 : 0;
        dl[0] += 1;
        dl[1] -= 1000000000L;
    }
    lust_mutex_lock(&c->lock);
    c->rx_waiting++;
    lust_chan_changed(c);
    for (;;) {
        if (c->len > 0) {
            lust_chan_pop(c, out);
            c->rx_waiting--;
            lust_chan_changed(c);
            lust_mutex_unlock(&c->lock);
            return 0;
        }
        if (c->senders == 0) { c->rx_waiting--; lust_mutex_unlock(&c->lock); return 2; }
        long now[2];
        lust_mono_now(now);
        long rs = dl[0] - now[0];
        long rn = dl[1] - now[1];
        if (rn < 0) {
            rs -= 1;
            rn += 1000000000L;
        }
        if (rs < 0 || (rs == 0 && rn == 0)) {
            c->rx_waiting--;
            if (c->cap == 0 && c->tx_waiting > 0) { lust_chan_claim(c, out); return 0; }
            lust_mutex_unlock(&c->lock);
            return 1;
        }
        uint32_t s;
        c->sleepers++;
        __atomic_load(&c->seq, &s, 5);
        lust_mutex_unlock(&c->lock);
        lust_futex_wait_for(&c->seq, s, rs, rn);
        lust_mutex_lock(&c->lock);
        c->sleepers--;
    }
})

typedef struct lust_Barrier {
    uint32_t lock;
    uint32_t seq;
    uint64_t count;
    uint64_t gen;
    uint64_t n;
} lust_Barrier;

/* 1 for the leader (the arrival that releases the others). */
int lust_barrier_wait(lust_Barrier *b) LUST_BODY({
    lust_mutex_lock(&b->lock);
    uint64_t g = b->gen;
    if (++b->count < b->n) {
        while (b->gen == g) {
            uint32_t s;
            __atomic_load(&b->seq, &s, 5);
            lust_mutex_unlock(&b->lock);
            lust_futex_wait(&b->seq, s);
            lust_mutex_lock(&b->lock);
        }
        lust_mutex_unlock(&b->lock);
        return 0;
    }
    b->count = 0;
    b->gen++;
    lust_cv_notify(&b->seq, 1);
    lust_mutex_unlock(&b->lock);
    return 1;
})

/* 1 when the caller must set the value (state is now 1); 0 when it is
   set (after waiting out another thread's set). */
int lust_once_begin(uint32_t *st) LUST_BODY({
    for (;;) {
        uint32_t zero = 0, one = 1, three = 3, s;
        if (__atomic_compare_exchange(st, &zero, &one, 0, 2, 2)) return 1;
        __atomic_load(st, &s, 2);
        if (s == 2) return 0;
        if (s == 0) continue;
        /* Being set: say a thread waits (1 -> 3), then sleep while it is so. */
        if (s == 1 && !__atomic_compare_exchange(st, &s, &three, 0, 2, 2)) continue;
        lust_futex_wait(st, 3);
    }
})

void lust_once_finish(uint32_t *st) LUST_BODY({
    uint32_t two = 2, old;
    __atomic_exchange(st, &two, &old, 3);
    if (old == 3) lust_futex_wake(st, 0x7fffffff);
})

/* A set whose init closure panicked (panic=unwind): the lock is empty
   again, and one waiting thread becomes the next setter, as in std. */
void lust_once_abort(uint32_t *st) LUST_BODY({
    uint32_t zero = 0, old;
    __atomic_exchange(st, &zero, &old, 3);
    if (old == 3) lust_futex_wake(st, 0x7fffffff);
})

/* `get_or_init` under unwind tables (runtime/panic.c mode 2): the init block's
   cleanup variable. When the init panicked (still armed), it drops a by-ref
   closure's env (std drops `f` inside the call) and empties the lock again. */
struct lust_once_guard { uint32_t *st; void *env; void (*drop)(void *); int32_t armed; };
void lust_once_guard_end(struct lust_once_guard *g) LUST_BODY({
    if (!g->armed) return;
    if (g->drop) g->drop(g->env);
    lust_once_abort(g->st);
})

int lust_once_is_done(uint32_t *st) LUST_BODY({
    uint32_t s;
    __atomic_load(st, &s, 2);
    return s == 2;
})

/* ---------- Duration and thread::sleep ----------

   A Duration is { uint64_t secs; uint32_t nanos; } (nanos < 1e9). */
typedef struct lust_Duration {
    uint64_t secs;
    uint32_t nanos;
} lust_Duration;

/* `Duration::new(secs, nanos)`: carries whole seconds out of nanos;
   panics as std on overflow. */
lust_Duration lust_dur_new(uint64_t secs, uint32_t nanos, lust_loc loc) LUST_BODY({
    lust_Duration d;
    uint64_t extra = nanos / 1000000000u;
    (void)loc;
    if (secs > UINT64_MAX - extra) lust_panic_std("overflow in Duration::new", "core/src/time.rs:201:18");
    d.secs = secs + extra;
    d.nanos = nanos % 1000000000u;
    return d;
})

/* secs * per_sec + nanos / nanos_per as a u128 (as_millis and kin). */
lust_u128 lust_dur_as(lust_Duration d, uint64_t per_sec, uint64_t nanos_per) LUST_BODY({
    uint64_t a = d.secs, b = per_sec;
    uint64_t alo = a & 0xffffffffu, ahi = a >> 32, blo = b & 0xffffffffu, bhi = b >> 32;
    uint64_t ll = alo * blo, lh = alo * bhi, hl = ahi * blo, hh = ahi * bhi;
    uint64_t mid = (ll >> 32) + (lh & 0xffffffffu) + (hl & 0xffffffffu);
    lust_u128 r;
    r.lo = (ll & 0xffffffffu) | (mid << 32);
    r.hi = hh + (lh >> 32) + (hl >> 32) + (mid >> 32);
    lust_u128 add;
    add.lo = d.nanos / nanos_per;
    add.hi = 0;
    return lust_u128_add(r, add);
})

#ifdef LUST_IMPL
#if defined(__linux__) && defined(__x86_64__)
#define LUST_SYS_NANOSLEEP 35
#elif defined(__linux__) && defined(__aarch64__)
#define LUST_SYS_NANOSLEEP 101
#endif
#endif

/* `thread::sleep`: nanosleep through syscall (no <time.h> in the runtime
   text), resumed after a signal. On Darwin, libc nanosleep (runtime.h
   included <time.h> there), the same loop. */
void lust_thread_sleep(lust_Duration d) LUST_BODY({
#ifdef _WIN32
    while (d.secs > 0 || d.nanos > 0) {
        uint64_t ms = d.secs > 4000000 ? 4000000000ull : d.secs * 1000 + d.nanos / 1000000;
        Sleep((DWORD)ms);
        if (d.secs > 4000000) d.secs -= 4000000; else { d.secs = 0; d.nanos = 0; }
    }
#elif defined(LUST_SYS_NANOSLEEP)
    long ts[2], rem[2];
    while (d.secs > 0 || d.nanos > 0) {
        uint64_t s = d.secs > 0x7fffffff ? 0x7fffffff : d.secs;
        ts[0] = (long)s;
        ts[1] = (long)d.nanos;
        d.secs -= s;
        d.nanos = 0;
        while (syscall(LUST_SYS_NANOSLEEP, ts, rem) != 0) { ts[0] = rem[0]; ts[1] = rem[1]; }
    }
#elif defined(__APPLE__)
    long ts[2], rem[2];
    while (d.secs > 0 || d.nanos > 0) {
        uint64_t s = d.secs > 0x7fffffff ? 0x7fffffff : d.secs;
        ts[0] = (long)s;
        ts[1] = (long)d.nanos;
        d.secs -= s;
        d.nanos = 0;
        while (nanosleep((void *)ts, (void *)rem) != 0) { ts[0] = rem[0]; ts[1] = rem[1]; }
    }
#else
    (void)d;
    sched_yield();
#endif
})
