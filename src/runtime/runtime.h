/* Lust C runtime. Every compilation unit includes this text.
   - With LUST_IMPL defined (one unit per program): full function definitions.
   - Without it (all other units): declarations only, so those units stay small and parse fast.
   Rules: wrapping math is done in uint64_t, so C integer promotion never makes signed overflow;
   every check that Rust does in debug builds is done here and panics the same way. */
#include <stdint.h>
#include <stddef.h>
#ifdef LUST_IMPL
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
/* pthread key and once types (thread.c, panic.c, sync.c). The runtime
   hand-declares the pthread calls for tcc, whose <pthread.h> prototypes
   clash with them, and the hand types (unsigned int key, int once with
   0 as its init) match glibc and musl on Linux. On Darwin they are
   wrong (the key is an unsigned long, the once is a 16-byte struct
   whose valid init is not zero), so a Darwin build, which never uses
   tcc, takes the real <pthread.h> types instead. */
#ifdef _WIN32
/* A Fls index and an INIT_ONCE (a pointer word, zero-init). */
typedef unsigned int lust_pthread_key_t;
typedef int64_t lust_pthread_once_t;
#define LUST_PTHREAD_ONCE_INIT 0
#elif defined(__APPLE__)
#include <pthread.h>
#include <time.h>
typedef pthread_key_t lust_pthread_key_t;
typedef pthread_once_t lust_pthread_once_t;
#define LUST_PTHREAD_ONCE_INIT PTHREAD_ONCE_INIT
#else
typedef unsigned int lust_pthread_key_t;
typedef int lust_pthread_once_t;
#define LUST_PTHREAD_ONCE_INIT 0
#endif
#define LUST_BODY(...) __VA_ARGS__
#else
void free(void *);
void abort(void);
void *memset(void *, int, size_t);
void *memcpy(void *, const void *, size_t);
#define LUST_BODY(...) ;
#endif

/* std::hint::black_box: the value's slot escapes into an empty asm that may read and write
   memory, so an optimizer can assume nothing about the value or what it points at (rustc's
   form). tcc does not optimize: nothing to hide. */
#ifdef __TINYC__
#define lust_black_box(p) ((void)(p))
#else
#define lust_black_box(p) __asm__ __volatile__("" : : "r"(p) : "memory")
#endif

/* A byte copy whose length is known only at run time but is often one small value: an
   element of the runtime's type-erased Vec, hash table and sort, a short string, a Drain
   tail. Up to 32 bytes it is a few loads and stores with no call, every load before any
   store, so the ranges may overlap (a memmove); a longer copy calls memmove. tcc units
   call lust_memmove, as before. */
#ifdef __TINYC__
#define lust_copy_small(d, s, n) lust_memmove((d), (s), (n))
#else
static inline __attribute__((always_inline)) void lust_copy_small(void *d, const void *s, uint64_t n) {
    unsigned char *dp = (unsigned char *)d;
    const unsigned char *sp = (const unsigned char *)s;
    if (__builtin_expect(n > 32, 0)) {
        __builtin_memmove(d, s, (size_t)n);
    } else if (n > 16) {
        uint64_t a, b, c, e;
        __builtin_memcpy(&a, sp, 8);
        __builtin_memcpy(&b, sp + 8, 8);
        __builtin_memcpy(&c, sp + n - 16, 8);
        __builtin_memcpy(&e, sp + n - 8, 8);
        __builtin_memcpy(dp, &a, 8);
        __builtin_memcpy(dp + 8, &b, 8);
        __builtin_memcpy(dp + n - 16, &c, 8);
        __builtin_memcpy(dp + n - 8, &e, 8);
    } else if (n >= 8) {
        uint64_t a, e;
        __builtin_memcpy(&a, sp, 8);
        __builtin_memcpy(&e, sp + n - 8, 8);
        __builtin_memcpy(dp, &a, 8);
        __builtin_memcpy(dp + n - 8, &e, 8);
    } else if (n >= 4) {
        uint32_t a, e;
        __builtin_memcpy(&a, sp, 4);
        __builtin_memcpy(&e, sp + n - 4, 4);
        __builtin_memcpy(dp, &a, 4);
        __builtin_memcpy(dp + n - 4, &e, 4);
    } else if (n) {
        unsigned char a = sp[0], b = sp[n >> 1], c = sp[n - 1];
        dp[0] = a;
        dp[n >> 1] = b;
        dp[n - 1] = c;
    }
}
#endif

/* RawVec's size rule: `n` elements of `elem` bytes (elem != 0) fit when n * elem is at most
   isize::MAX. One multiply with an overflow test: `n <= isize::MAX / elem` divides, a 64-bit
   div of 10 or more cycles at every Vec allocation. An elem the compiler knows (a call site's
   sizeof) folds to one compare. tcc units keep the division. */
#ifdef __TINYC__
#define lust_vec_fits(n, elem) ((uint64_t)(n) <= 9223372036854775807ull / (uint64_t)(elem))
#else
static inline __attribute__((always_inline)) int lust_vec_fits(uint64_t n, uint64_t elem) {
    uint64_t bytes;
    if (__builtin_constant_p(elem)) return n <= 9223372036854775807ull / elem;
    return !__builtin_mul_overflow(n, elem, &bytes) && bytes <= 9223372036854775807ull;
}
#endif


#if defined(_WIN32) && defined(LUST_IMPL)
/* The Win32 thread layer (rel-win): the runtime names pthread calls
   with hand declarations (no <pthread.h>, as under tcc); on Windows
   those names are these small wrappers over CreateThread, FlsAlloc
   and InitOnceExecuteOnce. A thread handle rides in a uintptr_t (the
   JoinHdr word is uintptr_t for this: a HANDLE does not fit the
   32-bit Windows long). Fls destructors run at thread exit, as
   pthread key destructors do; the main thread runs its TLS
   destructors from lust_tls_exit, as on Linux. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
void _lock_file(FILE *);
void _unlock_file(FILE *);
#define flockfile _lock_file
#define funlockfile _unlock_file
static int lust_win_key_create(unsigned int *key, void (*dtor)(void *)) {
    DWORD i = FlsAlloc((PFLS_CALLBACK_FUNCTION)dtor);
    if (i == 0xFFFFFFFFu) return 1;
    *key = (unsigned int)i;
    return 0;
}
static void *lust_win_getspecific(unsigned int key) { return FlsGetValue((DWORD)key); }
static int lust_win_setspecific(unsigned int key, const void *v) { return FlsSetValue((DWORD)key, (void *)v) ? 0 : 1; }
typedef struct { void (*init)(void); } lust_win_once_arg;
static BOOL CALLBACK lust_win_once_cb(PINIT_ONCE o, void *p, void **c) {
    (void)o; (void)c;
    ((lust_win_once_arg *)p)->init();
    return TRUE;
}
static int lust_win_once(lust_pthread_once_t *once, void (*init)(void)) {
    lust_win_once_arg a = {init};
    return InitOnceExecuteOnce((PINIT_ONCE)once, lust_win_once_cb, &a, 0) ? 0 : 1;
}
typedef struct { void *(*start)(void *); void *arg; } lust_win_start;
static DWORD WINAPI lust_win_thread_cb(void *p) {
    lust_win_start s = *(lust_win_start *)p;
    free(p);
    s.start(s.arg);
    return 0;
}
static int lust_win_thread_create(uintptr_t *t, const void *attr, void *(*start)(void *), void *arg) {
    (void)attr;
    lust_win_start *s = (lust_win_start *)malloc(sizeof *s);
    if (!s) return 1;
    s->start = start;
    s->arg = arg;
    HANDLE h = CreateThread(0, 0, lust_win_thread_cb, s, 0, 0);
    if (!h) { free(s); return 1; }
    *t = (uintptr_t)h;
    return 0;
}
static int lust_win_thread_join(uintptr_t t, void **ret) {
    (void)ret;
    WaitForSingleObject((HANDLE)t, INFINITE);
    CloseHandle((HANDLE)t);
    return 0;
}
static int lust_win_thread_detach(uintptr_t t) { CloseHandle((HANDLE)t); return 0; }
static int lust_win_yield(void) { SwitchToThread(); return 0; }
#define pthread_key_create lust_win_key_create
#define pthread_getspecific lust_win_getspecific
#define pthread_setspecific lust_win_setspecific
#define pthread_once lust_win_once
#define pthread_create lust_win_thread_create
#define pthread_join lust_win_thread_join
#define pthread_detach lust_win_thread_detach
#define sched_yield lust_win_yield
#endif

/* 128-bit integers. tcc has no __int128 (spike K3-13), so codegen uses a
   two-word struct; _Alignas on the first member gives the type the
   16-byte alignment Rust u128/i128 have (spike K3-21), which a type-level
   aligned attribute cannot do in tcc. `lo` is the low word. Size 16. */
typedef struct { _Alignas(16) uint64_t lo; uint64_t hi; } lust_u128;
typedef struct { _Alignas(16) uint64_t lo; uint64_t hi; } lust_i128;

/* Rust `()`: a zero-sized C form, so aggregates with a direct unit
   field have the exact Rust size. Size 0 (GNU empty struct, in tcc
   as in gcc), align 1. */
typedef struct { } lust_Unit;
_Static_assert(sizeof(lust_Unit) == 0 && _Alignof(lust_Unit) == 1, "unit layout");

typedef struct { uint8_t *ptr; uint64_t len; } lust_str;
typedef struct { uint8_t *ptr; uint64_t len; uint64_t cap; } lust_String;
typedef struct { void *ptr; uint64_t len; uint64_t cap; } lust_Vec;
/* `vec::IntoIter<T>`: the moved-in Vec buffer; elements `front..len` are still owned. */
typedef struct { void *ptr; uint64_t len; uint64_t cap; uint64_t front; } lust_VecIter;
/* `vec::Drain<T>` and `string::Drain`: the drained container (a lust_Vec
   or a lust_String; both start with ptr, len, cap), the drain start, the
   elements still to yield (`front..back`, indexes into the container's
   buffer), and the tail after the drained range. A Vec's length is set
   to `start` while its Drain lives; the drop glue (cgen.rs) drops what
   was not yielded and moves the tail down to `start`. */
typedef struct { void *vec; uint64_t start; uint64_t front; uint64_t back; uint64_t tail_start; uint64_t tail_len; } lust_VecDrain;
/* `&dyn Trait` and `&mut dyn Trait` (ARCH 2.8): the data pointer and the
   vtable, an array of words: drop glue, size, align, one super vtable per
   supertrait, then one entry per method (see src/check/object.rs). */
typedef struct { void *data; const void *const *vt; } lust_dyn;

/* Program globals, defined in the runtime unit and set by main. */
extern int lust_argc;
extern char **lust_argv;
extern const char *lust_file;

/* Every function is called through this table (see cgen.rs). */
typedef void (*lust_fnptr)(void);
extern lust_fnptr lust_fns[];
/* One slot trampoline per slot (ARCH 4.5): stable addresses that
   forward through lust_fns, stored in fn pointer values. */
extern lust_fnptr lust_tramps[];
/* Entry for a `main` that returns `Result<(), E>`: set by the unit
   that holds main when it registers; NULL for a `()` main. */
extern int (*lust_main_entry_p)(void);
void lust_base_init(void);
/* catch_unwind under unwind tables (panic.c, lust_unwind_mode 2): runs f(ctx) under a
   catch point; 1 when a panic unwound to it. */
int32_t lust_unw_try(void (*f)(void *), void *ctx);
int32_t lust_unw_main_caught(void);
void lust_unw_dtry(void (*f)(void *), void *arg, void **ux);
void lust_unw_reraise(void *ux);

/* A source location: (function id << 40) | (line within the function << 20) | column, with
   20 bits each for the relative line and the column. Lines are relative to each function's
   first line (lust_fn_line), so the code for an unchanged function stays valid when code
   above it moves. A function id at or above 0xFFFF00 instead names a std source line
   (panic.c LUST_STD_SLOT), with absolute line and column. */
typedef uint64_t lust_loc;
/* A fn's location slot (`lust_so_<id>`, release C: the slot already shifted to its bits,
   slot << 40) read at a panic site: a volatile read, so the C compiler keeps the load in
   the cold panic path instead of hoisting it into the fn's hot path. A #[track_caller]
   call's location argument reads the slot plainly (cgen/names.rs `loc`, `hot_loc`). */
#define LUST_SO(x) (*(const volatile uint64_t *)&(x))
/* The slot of a location word with no fn slot (a numbered or std word): a #[track_caller]
   callee gets &lust_so_none and the whole word (cgen/names.rs `hot_loc`). */
extern const uint64_t lust_so_none;
extern const uint32_t *lust_fn_line;
/* thread_local! slots below this have their cell pointer in thread.c's native
   lust_tls_fast array, for the inline access (inl.h `lust_tls_cell`). */
#define LUST_TLS_FAST 512
#ifdef LUST_IMPL
const uint64_t lust_so_none = 0;
int lust_argc;
char **lust_argv;
const char *lust_file;
const uint32_t *lust_fn_line;
#endif

