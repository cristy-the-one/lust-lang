/* ---------- Panics (abort mode) ----------
   Matches rustc 1.99 with -C panic=abort: the default hook prints
       \nthread '<name>' (<os tid>) panicked at <file>:<line>:<col>:\n<msg>\nnote: ...\n
   to stderr, then the process aborts (SIGABRT, shell status 134). Stdout
   is Rust's LineWriter (below): an abort loses the bytes it still holds. */

#ifdef LUST_IMPL
#ifndef _WIN32
#include <unistd.h>
#endif
#include <errno.h>
#include <signal.h>
#ifdef __APPLE__
#include <pthread.h>
#elif !defined(_WIN32)
#include <sys/syscall.h>
#endif

#ifdef _WIN32
/* rustc 1.99 abort_internal: __fastfail(FAST_FAIL_FATAL_APP_EXIT).
   msvcrt abort() is exit code 3. Git bash reports this fail-fast
   (STATUS_STACK_BUFFER_OVERRUN) as 127, which is rustc's status for a
   nested panic, a thread-local drop panic, and an allocation failure.
   An uncaught unwind panic still returns 101 from main; it does not
   come through here. */
__attribute__((noreturn)) void lust_fast_abort(void) {
    __asm__ __volatile__("movl $7, %ecx\n\tint $0x29");
    for (;;) {}
}
#endif

/* The OS thread id Rust's default hook prints (`thread::current_os_id`). */
static unsigned long long lust_os_tid(void) {
#ifdef _WIN32
    return (unsigned long long)GetCurrentThreadId();
#elif defined(__APPLE__)
    uint64_t t = 0;
    pthread_threadid_np(NULL, &t);
    return (unsigned long long)t;
#else
    return (unsigned long long)syscall(SYS_gettid);
#endif
}

/* Std source locations of panics that Rust raises inside std (no
   track_caller), as rustc 1.99 prints them. */
#define LUST_STD_SRC "/rustc/b940084d7eb6a299eb4bfeb8e34901bc051e7ac4/library/"
#endif

/* ---------- Unwinding (panic=unwind, ARCH 7.2) ----------
   Explicit propagation: a panic runs the hook, then marks an exception
   pending on its thread and returns. Generated code checks after each
   call and each statement that can panic: when its thread has a pending
   exception, the frame takes it (`lust_unw_take`) into its own `ex`,
   runs its cleanup blocks (the drops of its live locals, as normal
   code), and gives it back (`lust_unw_resume`) as it returns. So while
   a frame cleans up, nothing is pending and the destructors it calls
   run normally. A destructor that ends with a new pending exception
   during cleanup aborts (`lust_unw_cleanup_abort`).
   `lust_unw_pending` counts threads with a pending exception: 0 in
   almost every run, so the fast path of every check is one load. No C
   TLS (tcc breaks it in shared objects): thread state is under a
   pthread key. A panic while its thread already has a pending exception
   comes from code that runs on after the first panic in the same C
   statement (it uses only safe placeholder values); it does nothing.
   `lust_unwind_mode` is 0 (panic=abort: every panic aborts, as before)
   unless main sets it. The runtime object is the same in both modes.
   Mode 2 (unwind tables, a clang or gcc build with -fexceptions; below)
   has no pending state: a panic throws a C++ exception (unwind.cc) that
   runs each frame's cleanup variable, and catch points catch it. */
typedef struct {
    uint32_t kind; /* LUST_EXC_STR, LUST_EXC_STRING, or LUST_EXC_BOX */
    lust_str s;    /* the message bytes; LUST_EXC_STRING owns them */
    lust_dyn box;  /* LUST_EXC_BOX: a Box<dyn Any + Send> */
} lust_exc;
#define LUST_EXC_STR 0u
#define LUST_EXC_STRING 1u
#define LUST_EXC_BOX 2u
extern int32_t lust_unwind_mode;
extern int32_t lust_unw_pending;
/* The checks codegen emits (cgen/unwind.rs): take a new exception and
   run the cleanup from block b, or abort inside cleanup. */
#define LUST_UNW(b) if (lust_unw_pending && lust_unw_take(&ex)) goto bb##b
#define LUST_UNW_ABORT() if (lust_unw_pending && lust_unw_mine()) lust_unw_cleanup_abort()
#ifdef LUST_IMPL
int32_t lust_unwind_mode;
int32_t lust_unw_pending;
/* Hand pthread declarations for tcc on Linux; on Darwin the real
   <pthread.h> is included above (runtime.h has the type spellings),
   and on Windows runtime.h maps these names to its Win32 wrappers. */
#if !defined(__APPLE__) && !defined(_WIN32)
int pthread_key_create(unsigned int *key, void (*dtor)(void *));
void *pthread_getspecific(unsigned int key);
int pthread_setspecific(unsigned int key, const void *v);
int pthread_once(int *once, void (*init)(void));
#endif

/* One thread's panic state. `count` is Rust's local panic count: it
   rises when a panic starts and falls when a catch takes it, so
   `thread::panicking()` is `count > 0`. */
typedef struct {
    int32_t count;
    int32_t in_hook;
    int32_t pending;
    lust_exc exc;
    /* A panic message with arguments is being formatted (std formats
       it lazily inside the panic, before the hook prints): a panic
       now is a panic in the hook, which prints the inner message
       (blank when `msg_dyn`: it has arguments) and aborts in both
       modes. */
    int32_t in_fmt;
    int32_t msg_dyn;
    /* While a custom hook runs: its panic's payload and location. */
    lust_dyn hook_payload;
    void *hook_loc;
    /* Mode 2: the innermost catch point, the innermost unwind in flight,
       and the unwind the last catch took. */
    struct lust_catch *top;
    struct lust_uexc *unwinding;
    struct lust_uexc *caught;
} lust_unw_state;
#ifndef __TINYC__
static void lust_unw_eh_raise(lust_unw_state *s, lust_exc *ex) __attribute__((noreturn));
#endif
static lust_pthread_key_t lust_unw_key;
static lust_pthread_once_t lust_unw_once = LUST_PTHREAD_ONCE_INIT;
/* Rust's FIRST_PANIC: the backtrace note prints for the first panic only. */
static int32_t lust_first_panic = 1;

/* Set once the key exists (as thread.c's lust_tls_ready). */
static int lust_unw_ready;
static void lust_unw_make_key(void) {
    if (pthread_key_create(&lust_unw_key, free) != 0) abort();
    int one = 1;
    __atomic_store(&lust_unw_ready, &one, 3);
}
static lust_unw_state *lust_unw_get(void) {
    pthread_once(&lust_unw_once, lust_unw_make_key);
    lust_unw_state *s = (lust_unw_state *)pthread_getspecific(lust_unw_key);
    if (!s) {
        s = (lust_unw_state *)calloc(1, sizeof *s);
        if (!s) abort();
        pthread_setspecific(lust_unw_key, s);
    }
    return s;
}
/* The calling thread's state, or NULL when it never panicked. */
static lust_unw_state *lust_unw_peek(void) {
    int ready;
    __atomic_load(&lust_unw_ready, &ready, 2);
    if (!ready) return 0;
    return (lust_unw_state *)pthread_getspecific(lust_unw_key);
}
/* A panic while a panic message formats (std formats it lazily, inside
   the panic): Rust's nested-panic abort, printing the inner message
   (blank when it has arguments: `msg_dyn`). Else clears `msg_dyn`. */
static void lust_fmt_nested_check(const uint8_t *msg, uint64_t len, const char *file, uint32_t line, uint32_t col) {
    lust_unw_state *st = lust_unw_peek();
    if (st && st->in_fmt) {
        fprintf(stderr, "panicked at %s:%u:%u:\n%.*s\nthread panicked while processing panic. aborting.\n", file, (unsigned)line, (unsigned)col,
                st->msg_dyn ? 0 : (int)len, (const char *)msg);
        fflush(stderr);
        abort();
    }
    if (st) st->msg_dyn = 0;
}

static void lust_unw_set_pending(lust_unw_state *s, lust_exc *ex) {
    s->exc = *ex;
    s->pending = 1;
    __atomic_add_fetch(&lust_unw_pending, 1, 5);
}

/* Rust's FIRST_PANIC swap: 1 for the process's first panic only. */
static int32_t lust_first_panic_take(void) {
    int32_t zero = 0, old;
    __atomic_exchange(&lust_first_panic, &zero, &old, 0);
    return old;
}

/* The calling thread's name (runtime/sync.c). */
static void lust_cur_thread_name(const uint8_t **p, uint64_t *n);

/* The default hook's text (std's default_hook) on the current thread. */
static void lust_default_hook(const uint8_t *msg, uint64_t len, const char *file, uint64_t flen, uint32_t line, uint32_t col, int32_t count) {
    const uint8_t *tn;
    uint64_t tl;
    lust_cur_thread_name(&tn, &tl);
    fprintf(stderr, "\nthread '%.*s' (%llu) panicked at %.*s:%u:%u:\n%.*s\n", (int)tl, (const char *)tn, lust_os_tid(), (int)flen, file, (unsigned)line, (unsigned)col, (int)len,
            (const char *)msg);
    /* A panic while panicking prints a full backtrace in Rust; FIRST_PANIC
       is only taken when the style is Off. */
    int32_t bt = count >= 2 ? 3 : lust_bt_style();
    lust_bt_tail(bt, bt == 1 && lust_first_panic_take());
    fflush(stderr);
}

/* The type ids of `&'static str` and `String` (check/any.rs) and the
   Box<dyn Any + Send> vtables of the two message payloads (drop, size,
   align, the two fmt shims (0 here: `Any` has no fmt supertrait), then
   the type id words at VT_ANY_ID: check/object.rs). Main sets them
   when the program unwinds or installs a hook (`lust_unw_init`). */
static uint64_t lust_any_ids[4];
static const void *lust_vt_str[7], *lust_vt_string[7];

/* `panic::set_hook` (std source, library/std/src/panic.rs): while a hook
   is installed, a panic calls the std runner with the payload and
   location, which calls the hook. Main registers the runner. */
typedef struct { lust_str file; uint32_t line, col; } lust_location;
static int32_t lust_hook_on;
static lust_Unit (*lust_hook_runner)(uint8_t can_unwind);

/* Runs the hook for a panic starting with payload `ex` (message text
   `msg`), as std's rust_panic_with_hook: a panic inside the hook aborts
   at once; else the custom hook, or the default text. */
static void lust_run_hook(lust_exc *ex, const uint8_t *msg, uint64_t len, const char *file, uint32_t line, uint32_t col, int32_t count, int32_t *in_hook) {
    if (*in_hook) {
        fprintf(stderr, "panicked at %s:%u:%u:\n%.*s\nthread panicked while processing panic. aborting.\n", file, (unsigned)line, (unsigned)col, (int)len, (const char *)msg);
        fflush(stderr);
        abort();
    }
    *in_hook = 1;
    if (lust_hook_on && lust_hook_runner) {
        lust_location loc;
        loc.file.ptr = (uint8_t *)file;
        loc.file.len = strlen(file);
        loc.line = line;
        loc.col = col;
        lust_str sv = ex->s;
        lust_String so;
        so.ptr = ex->s.ptr;
        so.len = ex->s.len;
        so.cap = ex->s.len;
        lust_dyn p;
        if (ex->kind == LUST_EXC_BOX) p = ex->box;
        else if (ex->kind == LUST_EXC_STR) { p.data = &sv; p.vt = lust_vt_str; }
        else { p.data = &so; p.vt = lust_vt_string; }
        lust_unw_state *st = lust_unw_get();
        st->hook_payload = p;
        st->hook_loc = &loc;
        lust_hook_runner(lust_unwind_mode ? 1 : 0);
        st->hook_loc = 0;
    } else {
        lust_default_hook(msg, len, file, strlen(file), line, col, count);
    }
    *in_hook = 0;
}

/* A panic under panic=abort with a hook installed: the hook, then abort
   (no hook: callers print the default text themselves). */
static void lust_abort_hooked(lust_exc *ex, const uint8_t *msg, uint64_t len, const char *file, uint32_t line, uint32_t col) {
    lust_unw_state *s = lust_unw_get();
    if (!s->in_hook) s->count++;
    lust_run_hook(ex, msg, len, file, line, col, s->count, &s->in_hook);
    fflush(stderr);
    abort();
}

/* Starts a panic in unwind mode: the hook (with `msg`, the payload's
   text), then `ex` pending. */
static void lust_unw_raise_exc(lust_exc *ex, const uint8_t *msg, uint64_t len, const char *file, uint32_t line, uint32_t col) {
    lust_unw_state *s = lust_unw_get();
    if (s->pending) return;
    if (!s->in_hook) s->count++;
    lust_run_hook(ex, msg, len, file, line, col, s->count, &s->in_hook);
#ifndef __TINYC__
    if (lust_unwind_mode == 2) lust_unw_eh_raise(s, ex);
#endif
    lust_unw_set_pending(s, ex);
}

/* A message panic: the payload is a `&'static str` (LUST_EXC_STR, when
   Rust's message is a literal) or a `String` with the message. A STR
   payload keeps its copy for the rest of the run, as a static does. */
/* The copies behind STR payloads: they live to the end of the run, on
   this list so they stay reachable. */
typedef struct lust_static_msg { struct lust_static_msg *next; uint8_t bytes[]; } lust_static_msg;
static lust_static_msg *lust_static_msgs;
static uint8_t *lust_static_copy(const uint8_t *msg, uint64_t len) {
    lust_static_msg *m = (lust_static_msg *)malloc(sizeof(lust_static_msg) + len + 1);
    if (!m) abort();
    if (len) memcpy(m->bytes, msg, len);
    lust_static_msg *head;
    do { __atomic_load(&lust_static_msgs, &head, 2); m->next = head; } while (!__atomic_compare_exchange(&lust_static_msgs, &head, &m, 0, 3, 2));
    return m->bytes;
}

static void lust_unw_raise(uint32_t kind, const uint8_t *msg, uint64_t len, const char *file, uint32_t line, uint32_t col) {
    lust_exc ex;
    ex.kind = kind;
    ex.box.data = 0;
    ex.box.vt = 0;
    if (!lust_unwind_mode) {
        ex.s.ptr = (uint8_t *)msg;
        ex.s.len = len;
        lust_abort_hooked(&ex, msg, len, file, line, col);
    }
    if (kind == LUST_EXC_STR) {
        ex.s.ptr = lust_static_copy(msg, len);
    } else {
        ex.s.ptr = (uint8_t *)lust_alloc(len, 1);
        if (len) memcpy(ex.s.ptr, msg, len);
    }
    ex.s.len = len;
    ex.box.data = 0;
    ex.box.vt = 0;
    lust_unw_raise_exc(&ex, msg, len, file, line, col);
}

/* The text of a payload, as std's default hook finds it: a `&str` or a
   `String`, else "Box<dyn Any>". */
static lust_str lust_exc_text(lust_exc *ex) {
    lust_str r = ex->s;
    if (ex->kind != LUST_EXC_BOX) return r;
    const void *const *vt = ex->box.vt;
    uint64_t lo = (uint64_t)(uintptr_t)vt[5], hi = (uint64_t)(uintptr_t)vt[6];
    if (lo == lust_any_ids[0] && hi == lust_any_ids[1]) return *(lust_str *)ex->box.data;
    if (lo == lust_any_ids[2] && hi == lust_any_ids[3]) return lust_string_as_str((lust_String *)ex->box.data);
    r.ptr = (uint8_t *)"Box<dyn Any>";
    r.len = 12;
    return r;
}

/* The payload as the Box<dyn Any + Send> a catch returns. */
static lust_dyn lust_exc_box(lust_exc *ex) {
    lust_dyn d;
    if (ex->kind == LUST_EXC_BOX) return ex->box;
    if (ex->kind == LUST_EXC_STR) {
        lust_str *p = (lust_str *)lust_alloc(sizeof(lust_str), 8);
        *p = ex->s;
        d.data = p;
        d.vt = lust_vt_str;
        return d;
    }
    lust_String *p = (lust_String *)lust_alloc(sizeof(lust_String), 8);
    p->ptr = ex->s.ptr;
    p->len = ex->s.len;
    p->cap = ex->s.len;
    d.data = p;
    d.vt = lust_vt_string;
    return d;
}

/* Frees an exception's payload (an uncaught one at a thread root). */
static void lust_exc_drop(lust_exc *ex) {
    if (ex->kind == LUST_EXC_STRING) lust_dealloc(ex->s.ptr, ex->s.len, 1);
    if (ex->kind == LUST_EXC_BOX) {
        if (ex->box.vt[0]) ((void (*)(void *))ex->box.vt[0])(ex->box.data);
        lust_dealloc(ex->box.data, (uint64_t)(uintptr_t)ex->box.vt[1], (uint64_t)(uintptr_t)ex->box.vt[2]);
    }
}
#endif

/* Takes the calling thread's pending exception into `ex` (a frame starts
   its cleanup): 1, or 0 when this thread has none. */
int32_t lust_unw_take(lust_exc *ex) LUST_BODY({
    lust_unw_state *s = lust_unw_peek();
    if (!s || !s->pending) return 0;
    *ex = s->exc;
    s->pending = 0;
    __atomic_sub_fetch(&lust_unw_pending, 1, 5);
    return 1;
})

/* Makes `ex` pending again: the frame's cleanup is done and it returns. */
void lust_unw_resume(lust_exc *ex) LUST_BODY({ lust_unw_set_pending(lust_unw_get(), ex); })

/* `thread::panicking()`: a panic started on this thread and no catch
   has taken it yet. */
uint8_t lust_unw_panicking(void) LUST_BODY({
    lust_unw_state *s = lust_unw_peek();
    return s && s->count > 0;
})

/* 1 when the calling thread has a pending exception. */
int32_t lust_unw_mine(void) LUST_BODY({
    lust_unw_state *s = lust_unw_peek();
    return s && s->pending;
})

/* A destructor that cleanup called ended with a new exception: Rust's
   nounwind "panic in a destructor during cleanup", then abort. */
void lust_unw_cleanup_abort(void) LUST_BODY({
    const uint8_t *tn;
    uint64_t tl;
    lust_cur_thread_name(&tn, &tl);
    fprintf(stderr, "\nthread '%.*s' (%llu) panicked at " LUST_STD_SRC "core/src/panicking.rs:233:5:\npanic in a destructor during cleanup\nthread caused non-unwinding panic. aborting.\n",
            (int)tl, (const char *)tn, lust_os_tid());
    fflush(stderr);
    abort();
})

/* One step of drop glue that keeps dropping after a panic (Rust drops the
   other fields and elements as cleanup): the first exception moves into
   `ex` and sets `*u`; one more aborts. The glue resumes `ex` at its end. */
void lust_unw_dstep(lust_exc *ex, int32_t *u) LUST_BODY({
    if (!lust_unw_mine()) return;
    if (*u) lust_unw_cleanup_abort();
    lust_unw_take(ex);
    *u = 1;
})

#ifdef LUST_IMPL
#ifndef __TINYC__
/* ---------- Unwind tables (lust_unwind_mode 2) ----------
   A clang or gcc build compiles every unit with -fexceptions. A frame
   with drops has one cleanup variable (cgen: the frame struct, with the
   locals its cleanup ladder reads and the ladder entry stored before
   each call that can unwind); builtins that own a value add small
   guards. A panic runs the hook, then throws a C++ exception of a
   private type (runtime/unwind.cc): the platform's C++ personality
   unwinds, running those cleanups frame by frame, to the innermost
   catch point (`lust_unw_try`: catch_unwind, a thread root, main), the
   only place that catches it. Nothing runs after a call to check for a
   panic, so a panic never returns. */
void lust_unw_cxx_throw(void *x) __attribute__((noreturn));
int32_t lust_unw_cxx_try(void (*f)(void *), void *ctx, void **caught);
/* Weak fallbacks for a build without the C++ unit (a release build with
   panic=abort or LUST_UNWIND_TABLES=0, gcc's sanitized builds): mode 2
   never runs there. runtime/unwind.cc's definitions replace them. */
__attribute__((weak, noreturn)) void lust_unw_cxx_throw(void *x) {
    (void)x;
    abort();
}
__attribute__((weak)) int32_t lust_unw_cxx_try(void (*f)(void *), void *ctx, void **caught) {
    (void)caught;
    f(ctx);
    return 0;
}
struct lust_catch { struct lust_catch *prev; int32_t in_glue; };
struct lust_uexc {
    lust_exc exc;
    struct lust_catch *target; /* the catch point this unwind goes to */
    struct lust_uexc *outer;   /* the unwind in flight when this one began */
};
/* The innermost catch point that is not drop glue's (`lust_unw_dtry`): a panic
   in a drop that cleanup runs aborts even when glue would catch it. */
static struct lust_catch *lust_unw_outer_catch(struct lust_catch *c) {
    while (c && c->in_glue) c = c->prev;
    return c;
}
/* Throws `x`. A panic that would leave the cleanup of an unwind in flight (no
   catch point began since that unwind did) is Rust's "panic in a destructor
   during cleanup": it aborts before the throw, so no exception ever leaves a
   cleanup variable's fn. */
static void lust_unw_eh_go(lust_unw_state *s, struct lust_uexc *x) __attribute__((noreturn));
static void lust_unw_eh_go(lust_unw_state *s, struct lust_uexc *x) {
    if (!s->top) abort();
    if (s->unwinding && lust_unw_outer_catch(s->top) == s->unwinding->target) lust_unw_cleanup_abort();
    x->target = s->top;
    x->outer = s->unwinding;
    s->unwinding = x;
    lust_unw_cxx_throw(x);
}
/* Raises `ex` (its hook already ran). */
static void lust_unw_eh_raise(lust_unw_state *s, lust_exc *ex) {
    struct lust_uexc *x = (struct lust_uexc *)calloc(1, sizeof *x);
    if (!x) abort();
    x->exc = *ex;
    lust_unw_eh_go(s, x);
}
/* Runs f(ctx) under a catch point (`in_glue` for drop glue's). A caught
   unwind is no longer in flight. */
static int32_t lust_unw_catch_at(void (*f)(void *), void *ctx, int32_t in_glue, struct lust_uexc **out) {
    lust_unw_state *s = lust_unw_get();
    struct lust_catch c;
    c.prev = s->top;
    c.in_glue = in_glue;
    s->top = &c;
    void *x = 0;
    int32_t r = lust_unw_cxx_try(f, ctx, &x);
    s->top = c.prev;
    if (r) {
        struct lust_uexc *u = (struct lust_uexc *)x;
        s->unwinding = u->outer;
        *out = u;
    }
    return r;
}
int32_t lust_unw_try(void (*f)(void *), void *ctx) {
    struct lust_uexc *u = 0;
    if (!lust_unw_catch_at(f, ctx, 0, &u)) return 0;
    lust_unw_get()->caught = u;
    return 1;
}
/* A drop in drop glue that can panic (a user Drop, a dyn drop): Rust drops
   the other fields as cleanup and passes the panic on at the glue's end
   (`lust_unw_reraise`). The first panic's record is kept in `*ux`; a second
   one in the same glue aborts. */
void lust_unw_dtry(void (*f)(void *), void *arg, void **ux) {
    struct lust_uexc *u = 0;
    if (!lust_unw_catch_at(f, arg, 1, &u)) return;
    if (*ux) lust_unw_cleanup_abort();
    *ux = u;
}
/* The end of drop glue that caught a panic: it unwinds on (no hook; the panic
   count stays). */
void lust_unw_reraise(void *ux) { lust_unw_eh_go(lust_unw_get(), (struct lust_uexc *)ux); }
static void lust_unw_eh_take(lust_exc *ex) {
    lust_unw_state *s = lust_unw_get();
    struct lust_uexc *x = s->caught;
    s->caught = 0;
    *ex = x->exc;
    free(x);
}
#else
int32_t lust_unw_try(void (*f)(void *), void *ctx) {
    f(ctx);
    return 0;
}
void lust_unw_dtry(void (*f)(void *), void *arg, void **ux) {
    (void)ux;
    f(arg);
}
void lust_unw_reraise(void *ux) {
    (void)ux;
    abort();
}
static void lust_unw_eh_take(lust_exc *ex) { (void)ex; abort(); }
#endif
/* A raise in either unwind mode: mode 2 never returns, mode 1 leaves the
   exception pending. */
static void lust_unw_raise_or_pend(lust_unw_state *s, lust_exc *ex) {
#ifndef __TINYC__
    if (lust_unwind_mode == 2) lust_unw_eh_raise(s, ex);
#endif
    lust_unw_set_pending(s, ex);
}
/* The exception a catch took: mode 2's caught one, else the pending one. */
static void lust_unw_take_any(lust_exc *ex) {
    if (lust_unwind_mode == 2) lust_unw_eh_take(ex);
    else lust_unw_take(ex);
}
#endif

/* Setup from main: the payload type ids (see lust_any_ids), and whether
   panics unwind. */
void lust_unw_init(int32_t unwind, uint64_t str_lo, uint64_t str_hi, uint64_t string_lo, uint64_t string_hi) LUST_BODY({
    lust_unwind_mode = unwind;
    lust_any_ids[0] = str_lo;
    lust_any_ids[1] = str_hi;
    lust_any_ids[2] = string_lo;
    lust_any_ids[3] = string_hi;
    lust_vt_str[0] = 0;
    lust_vt_str[1] = (const void *)(uintptr_t)sizeof(lust_str);
    lust_vt_str[2] = (const void *)(uintptr_t)8;
    lust_vt_str[3] = 0;
    lust_vt_str[4] = 0;
    lust_vt_str[5] = (const void *)(uintptr_t)str_lo;
    lust_vt_str[6] = (const void *)(uintptr_t)str_hi;
    lust_vt_string[0] = (const void *)lust_string_drop;
    lust_vt_string[1] = (const void *)(uintptr_t)sizeof(lust_String);
    lust_vt_string[2] = (const void *)(uintptr_t)8;
    lust_vt_string[3] = 0;
    lust_vt_string[4] = 0;
    lust_vt_string[5] = (const void *)(uintptr_t)string_lo;
    lust_vt_string[6] = (const void *)(uintptr_t)string_hi;
})

#ifdef LUST_IMPL
/* std's hook (`Option<Box<dyn Fn(&PanicHookInfo) + Sync + Send>>`, std
   source), stored here as bytes: all zero is None. */
static uint64_t lust_hook_bytes[4];
static int32_t lust_hook_lock;
#endif

/* `core::intrinsics::panic_hook_swap`: swaps `n` bytes at `v` with the
   hook slot. */
void lust_hook_swap(void *v, uint64_t n) LUST_BODY({
    int32_t one = 1, zero = 0, old;
    do { __atomic_exchange(&lust_hook_lock, &one, &old, 2); } while (old);
    uint8_t tmp[sizeof lust_hook_bytes];
    if (n > sizeof tmp) abort();
    memcpy(tmp, lust_hook_bytes, n);
    memcpy(lust_hook_bytes, v, n);
    memcpy(v, tmp, n);
    __atomic_store(&lust_hook_lock, &zero, 3);
})

/* `core::intrinsics::panic_hook_ref`: the hook slot. */
void *lust_hook_slot(void) LUST_BODY({ return lust_hook_bytes; })

/* Main registers set_hook's runner (`__lust_run_hook`'s slot). */
void lust_hook_set_runner(lust_fnptr f) LUST_BODY({ lust_hook_runner = (lust_Unit (*)(uint8_t))f; })

/* `core::intrinsics::panic_hook_payload` and `panic_hook_location`: the
   panic the calling thread's hook runs for. */
lust_dyn lust_hook_payload(void) LUST_BODY({ return lust_unw_get()->hook_payload; })
void *lust_hook_location(void) LUST_BODY({ return lust_unw_get()->hook_loc; })

/* `core::intrinsics::panic_hook_custom`: set_hook installed a hook (1),
   or take_hook removed it (0). */
void lust_hook_custom(uint8_t on) LUST_BODY({
    int32_t v = on;
    __atomic_store(&lust_hook_on, &v, 3);
})

/* `core::intrinsics::panic_default_hook`: std's default hook text, for a
   hook that `take_hook` returned (on the calling thread's panic). */
void lust_hook_default(lust_str file, uint32_t line, uint32_t col, lust_str msg) LUST_BODY({
    lust_unw_state *s = lust_unw_peek();
    lust_default_hook(msg.ptr, msg.len, (const char *)file.ptr, file.len, line, col, s ? s->count : 1);
})

/* `catch_unwind`'s Err: takes the calling thread's pending exception
   (the caller saw `lust_unw_mine`; in mode 2 the one its catch point
   caught), ends its panic (the count falls), and returns the payload as
   a Box<dyn Any + Send>. */
lust_dyn lust_unw_catch(void) LUST_BODY({
    lust_exc ex;
    lust_unw_take_any(&ex);
    lust_unw_get()->count--;
    return lust_exc_box(&ex);
})

/* Unwind tables: a builtin's cleanup variable for a value it owns (cgen
   `zc_guard`): the value drops when a panic unwinds before the builtin
   disarms it. */
struct lust_dguard { void (*drop)(void *); void *p; int32_t armed; };
void lust_dguard_end(struct lust_dguard *g) LUST_BODY({
    if (g->armed) g->drop(g->p);
})
/* `retain` under unwind tables: when the predicate or a rejected element's
   drop panics, the unprocessed rest moves down behind the kept elements
   (std's `BackshiftOnDrop`), so nothing is lost or dropped twice. */
struct lust_retain_guard { lust_Vec *v; uint64_t *r; uint64_t *w; uint64_t es; int32_t armed; };
void lust_retain_guard_end(struct lust_retain_guard *g) LUST_BODY({
    if (!g->armed) return;
    uint64_t r = *g->r, w = *g->w, n = g->v->len - r;
    if (n && w != r) memmove((char *)g->v->ptr + w * g->es, (char *)g->v->ptr + r * g->es, n * g->es);
    g->v->len = w + n;
})

/* Mode 2: after main's catch point caught a panic, std's lang_start drops
   the payload and the process exits with 101. */
int32_t lust_unw_main_caught(void) LUST_BODY({
    lust_exc ex;
    lust_unw_take_any(&ex);
    lust_exc_drop(&ex);
    return 101;
})

/* `resume_unwind(payload)`: the payload unwinds again with no hook. */
void lust_unw_resume_box(lust_dyn b) LUST_BODY({
    if (!lust_unwind_mode) abort();
    lust_unw_state *s = lust_unw_get();
    if (s->pending) return;
    s->count++;
    lust_exc ex;
    ex.kind = LUST_EXC_BOX;
    ex.box = b;
    ex.s.ptr = 0;
    ex.s.len = 0;
    lust_unw_raise_or_pend(s, &ex);
})

/* After `main` returns: 101 when it ended by a panic (std's lang_start
   catches it and drops the payload), else 0. */
int32_t lust_unw_main_exit(void) LUST_BODY({
    lust_exc ex;
    if (!lust_unw_pending || !lust_unw_take(&ex)) return 0;
    lust_exc_drop(&ex);
    return 101;
})

/* Prints the default hook text for `msg` at `file:line:col` and aborts;
   in unwind mode it raises the panic and returns. */
void lust_panic_at(const uint8_t *msg, uint64_t len, const char *file, uint32_t line, uint32_t col) LUST_BODY({
    lust_fmt_nested_check(msg, len, file, line, col);
    if (lust_unwind_mode || lust_hook_on) {
        lust_unw_raise(LUST_EXC_STRING, msg, len, file, line, col);
        return;
    }
    const uint8_t *tn;
    uint64_t tl;
    lust_cur_thread_name(&tn, &tl);
    fprintf(stderr, "\nthread '%.*s' (%llu) panicked at %s:%u:%u:\n%.*s\n", (int)tl, (const char *)tn, lust_os_tid(), file, (unsigned)line, (unsigned)col, (int)len,
            (const char *)msg);
    int32_t bt = lust_bt_style();
    lust_bt_tail(bt, bt == 1 && lust_first_panic_take());
    fflush(stderr);
    abort();
})

/* A std source location in a lust_loc (cgen's `std_loc`): the slot
   field (the top 24 bits) is LUST_STD_SLOT plus an index into
   lust_std_files, the line and col fields are absolute. Index bit
   LUST_STD_POW marks `pow`, whose overflow says "exponentiate" where
   its multiply says "multiply". */
#define LUST_STD_SLOT 0xFFFF00u
#define LUST_STD_POW 0x40u
#ifdef LUST_IMPL
static const char *const lust_std_files[] = {
    LUST_STD_SRC "core/src/num/mod.rs",
    LUST_STD_SRC "core/src/char/methods.rs",
    LUST_STD_SRC "core/src/char/convert.rs",
    LUST_STD_SRC "alloc/src/vec/mod.rs",
    LUST_STD_SRC "core/src/cmp.rs",
    LUST_STD_SRC "core/src/iter/adapters/step_by.rs",
    LUST_STD_SRC "alloc/src/vec/spec_from_iter_nested.rs",
    LUST_STD_SRC "core/src/iter/traits/accum.rs",
    LUST_STD_SRC "core/src/iter/range.rs",
    LUST_STD_SRC "core/src/cell.rs",
    LUST_STD_SRC "std/src/thread/local.rs",
    LUST_STD_SRC "core/src/ops/arith.rs",
    LUST_STD_SRC "core/src/slice/index.rs",
    LUST_STD_SRC "core/src/iter/traits/exact_size.rs",
    LUST_STD_SRC "core/src/slice/mod.rs",
    LUST_STD_SRC "core/src/ops/function.rs",
    LUST_STD_SRC "std/src/env.rs",
    /* The files of std source markers (src/stdloc.rs STD_FILES). */
    LUST_STD_SRC "core/src/time.rs",
    LUST_STD_SRC "std/src/time.rs",
    LUST_STD_SRC "alloc/src/collections/btree/search.rs",
    LUST_STD_SRC "alloc/src/collections/vec_deque/mod.rs",
    LUST_STD_SRC "std/src/path.rs",
    LUST_STD_SRC "std/src/panicking.rs",
    LUST_STD_SRC "core/src/cell/once.rs",
    LUST_STD_SRC "core/src/cell/lazy.rs",
    LUST_STD_SRC "std/src/sync/lazy_lock.rs",
    LUST_STD_SRC "core/src/io/write.rs",
    LUST_STD_SRC "alloc/src/io/read.rs",
    LUST_STD_SRC "alloc/src/io/impls.rs",
    LUST_STD_SRC "core/src/io/borrowed_buf.rs",
    LUST_STD_SRC "alloc/src/rc.rs",
    LUST_STD_SRC "alloc/src/sync.rs",
    LUST_STD_SRC "core/src/str/mod.rs",
    LUST_STD_SRC "std/src/os/unix/fs.rs",
    LUST_STD_SRC "alloc/src/str.rs",
};
#endif

/* Per-slot source files, set by the main unit when the crate has more
   than one file: slot s's fn lives in lust_files[lust_slot_file[s]].
   NULL for a one-file crate, which uses lust_file. */
extern const char *const *lust_files;
extern const uint32_t *lust_slot_file;
#ifdef LUST_IMPL
const char *const *lust_files;
const uint32_t *lust_slot_file;
#endif

#ifdef LUST_IMPL
/* A message panic at a packed location; `kind` is the unwind payload's
   type (LUST_EXC_STR or LUST_EXC_STRING), as Rust picks it per site.
   Under panic=unwind every lust_panic_* call returns with the exception
   pending, and the caller's frame unwinds at its next check. A runtime
   helper that panics must return at once, with no more writes: Rust
   leaves memory and output as they were at the panic. */
static void lust_panic_kind(uint32_t kind, const uint8_t *p, uint64_t n, lust_loc loc) {
    uint32_t slot = (uint32_t)(loc >> 40);
    const char *file;
    uint32_t line;
    if (slot >= LUST_STD_SLOT) {
        file = lust_std_files[(slot - LUST_STD_SLOT) & ~LUST_STD_POW];
        line = (loc >> 20) & 0xFFFFF;
    } else {
        file = lust_slot_file ? lust_files[lust_slot_file[slot]] : lust_file;
        line = lust_fn_line[slot] + ((loc >> 20) & 0xFFFFF);
    }
    if (lust_unwind_mode || lust_hook_on) {
        lust_fmt_nested_check(p, n, file, line, loc & 0xFFFFF);
        lust_unw_raise(kind, p, n, file, line, loc & 0xFFFFF);
        return;
    }
    lust_panic_at(p, n, file, line, loc & 0xFFFFF);
}
#endif

/* A panic with a formatted message (a `String` payload). Takes `msg`:
   the payload holds a copy, and in unwind mode the caller's temporary is
   freed here, as the code after the panic never runs. */
void lust_panic(lust_String *msg, lust_loc loc) LUST_BODY({
    lust_panic_kind(LUST_EXC_STRING, msg->ptr, msg->len, loc);
    lust_string_drop(msg);
})

/* A panic whose message Rust writes as a literal (a `&'static str`
   payload): `panic!("text")`, overflow, unwrap on None. Takes `msg`. */
void lust_panic_lit(lust_String *msg, lust_loc loc) LUST_BODY({
    lust_panic_kind(LUST_EXC_STR, msg->ptr, msg->len, loc);
    lust_string_drop(msg);
})

/* A panic whose message has arguments (`panic!("{}", x)`, assert_eq!,
   `Result::unwrap`): cgen calls lust_panic_begin before it formats the
   message and lust_panic_fmt with the result. */
void lust_panic_begin(lust_loc loc) LUST_BODY({
    lust_unw_state *st = lust_unw_get();
    if (st->in_fmt) {
        lust_String s = lust_string_new();
        st->msg_dyn = 1;
        lust_panic(&s, loc);
    }
    st->in_fmt = 1;
})

void lust_panic_fmt(lust_String *msg, lust_loc loc) LUST_BODY({
    lust_unw_get()->in_fmt = 0;
    lust_panic(msg, loc);
})

/* A runtime panic whose Rust message has arguments (bounds, slice
   ranges): as lust_panic, but blank when it happens in a panic. */
void lust_panic_dyn(lust_String *msg, lust_loc loc) LUST_BODY({
    lust_unw_get()->msg_dyn = 1;
    lust_panic(msg, loc);
})

void lust_panic_cstr_dyn(const char *msg, lust_loc loc) LUST_BODY({
    lust_String s = lust_string_new();
    lust_string_push_cstr(&s, msg);
    lust_panic_dyn(&s, loc);
})

void lust_panic_str(lust_str msg, lust_loc loc) LUST_BODY({
    lust_String s = lust_string_from(msg);
    lust_panic(&s, loc);
})

/* A literal message (a `&'static str` payload). */
void lust_panic_cstr(const char *msg, lust_loc loc) LUST_BODY({ lust_panic_kind(LUST_EXC_STR, (const uint8_t *)msg, strlen(msg), loc); })

/* A fixed message Rust formats at run time (a `String` payload: RefCell's
   borrow errors, `expect` texts). */
void lust_panic_cstr_owned(const char *msg, lust_loc loc) LUST_BODY({ lust_panic_kind(LUST_EXC_STRING, (const uint8_t *)msg, strlen(msg), loc); })

/* "attempt to <op> with overflow" (checked integer math). */
void lust_ovf(const char *op, lust_loc loc) LUST_BODY({
    uint32_t slot = (uint32_t)(loc >> 40);
    if (slot >= LUST_STD_SLOT && ((slot - LUST_STD_SLOT) & LUST_STD_POW)) op = "exponentiate";
    lust_String s = lust_string_new();
    lust_string_push_cstr(&s, "attempt to ");
    lust_string_push_cstr(&s, op);
    lust_string_push_cstr(&s, " with overflow");
    lust_panic_lit(&s, loc);
})

/* A panic Rust raises at a fixed line of its own std source: `where` is
   "path:line:col" under library/. */
#ifdef LUST_IMPL
static void lust_panic_std_kind(uint32_t kind, const char *msg, const char *where) {
    lust_unw_state *st = lust_unw_peek();
    if (st && st->in_fmt) {
        fprintf(stderr, "panicked at " LUST_STD_SRC "%s:\n%s\nthread panicked while processing panic. aborting.\n", where, msg);
        fflush(stderr);
        abort();
    }
    if (lust_unwind_mode || lust_hook_on) {
        /* `where` is "path:line:col"; split it for the hook. */
        const char *c2 = strrchr(where, ':'), *c1 = c2;
        while (c1 > where && c1[-1] != ':') c1--;
        char file[256];
        int n = snprintf(file, sizeof file, LUST_STD_SRC "%.*s", (int)(c1 - 1 - where), where);
        (void)n;
        lust_unw_raise(kind, (const uint8_t *)msg, strlen(msg), file, (uint32_t)strtoul(c1, 0, 10), (uint32_t)strtoul(c2 + 1, 0, 10));
        return;
    }
    const uint8_t *tn;
    uint64_t tl;
    lust_cur_thread_name(&tn, &tl);
    fprintf(stderr, "\nthread '%.*s' (%llu) panicked at " LUST_STD_SRC "%s:\n%s\n", (int)tl, (const char *)tn, lust_os_tid(), where, msg);
    int32_t bt = lust_bt_style();
    lust_bt_tail(bt, bt == 1 && lust_first_panic_take());
    fflush(stderr);
    abort();
}
#endif

/* A panic Rust raises with a formatted message (a `String` payload) at a
   fixed line of its own std source: `where` is "path:line:col" under
   library/. */
void lust_panic_std(const char *msg, const char *where) LUST_BODY({ lust_panic_std_kind(LUST_EXC_STRING, msg, where); })

/* As lust_panic_std, for a message std writes as a literal (a `&str`
   payload: RawVec's capacity overflow, the atomic ordering checks). */
void lust_panic_std_lit(const char *msg, const char *where) LUST_BODY({ lust_panic_std_kind(LUST_EXC_STR, msg, where); })

/* core 1.99's `BorrowedCursor::advance_checked` assert, as `io::copy`'s
   default `read_buf` hits it when `read` returns more than the buffer. */
void lust_panic_read_buf_overrun(void) LUST_BODY({ lust_panic_std_kind(LUST_EXC_STR, "assertion failed: n <= init_unfilled", "core/src/io/borrowed_buf.rs:287:9"); })

/* `panic_any(payload)`: the hook prints the payload's text, then it
   unwinds (abort mode: aborts). */
void lust_panic_box(lust_dyn b, lust_loc loc) LUST_BODY({
    lust_exc ex;
    ex.kind = LUST_EXC_BOX;
    ex.box = b;
    ex.s.ptr = 0;
    ex.s.len = 0;
    lust_str t = lust_exc_text(&ex);
    uint32_t slot = (uint32_t)(loc >> 40);
    const char *file = lust_slot_file ? lust_files[lust_slot_file[slot]] : lust_file;
    uint32_t line = lust_fn_line[slot] + ((loc >> 20) & 0xFFFFF);
    if (lust_unwind_mode) lust_unw_raise_exc(&ex, t.ptr, t.len, file, line, loc & 0xFFFFF);
    else if (lust_hook_on) lust_abort_hooked(&ex, t.ptr, t.len, file, line, loc & 0xFFFFF);
    else lust_panic_kind(LUST_EXC_STR, t.ptr, t.len, loc);
})

/* The file and line of a packed location, as the panic messages above
   print them: `Location::caller()` builds its value from these (cgen
   `B::LocationCaller`), so a lib fn's location names its own file. */
void lust_loc_where(lust_loc loc, const char **file, uint32_t *line) LUST_BODY({
    uint32_t slot = (uint32_t)(loc >> 40);
    if (slot >= LUST_STD_SLOT) {
        *file = lust_std_files[(slot - LUST_STD_SLOT) & ~LUST_STD_POW];
        *line = (loc >> 20) & 0xFFFFF;
    } else {
        *file = lust_slot_file ? lust_files[lust_slot_file[slot]] : lust_file;
        *line = lust_fn_line[slot] + ((loc >> 20) & 0xFFFFF);
    }
})

/* ---------- Stdout as Rust's LineWriter ----------
   Rust's stdout is a LineWriter with a 1024-byte buffer over fd 1, tty
   or not: a write that holds a newline sends everything up to its last
   newline at once, and only the tail stays buffered (BufWriter rules:
   a tail that does not fit flushes the buffer, one of 1024 bytes or
   more is written directly). So a panic=abort loses exactly the held
   tail, and stdout and stderr interleave as in Rust. `lust_print_out`
   gets one formatted string per print, where Rust writes piece by
   piece; the held bytes differ only when one partial line passes 1024
   bytes. SIGPIPE is ignored as in Rust, so a write to a closed pipe
   panics with Rust's message. */
#ifdef LUST_IMPL
static uint8_t lust_out_buf[1024];
static uint64_t lust_out_len;
/* io::Write on Stdout (sys.c) returns a write error where print! panics:
   in soft mode the first error lands in lust_out_err (errno, or EIO for
   a zero-byte write) and the held bytes are dropped. */
static int lust_out_soft, lust_out_err;

static void lust_out_raw(const uint8_t *p, uint64_t n) {
    while (n) {
#ifdef _WIN32
        DWORD done = 0;
        int64_t w = WriteFile(GetStdHandle((DWORD)-11), p, (DWORD)(n > 0x7fffffff ? 0x7fffffff : n), &done, 0) ? (int64_t)done : -1;
        if (w < 0) errno = (int)GetLastError();
#else
        ssize_t w = write(1, p, n);
#endif
        if (w < 0 && errno == EINTR) continue;
        if (w <= 0) {
            int e = w < 0 ? errno : 0;
            if (lust_out_soft) {
                if (!lust_out_err) lust_out_err = e ? e : EIO;
                lust_out_len = 0;
                return;
            }
            lust_String s = lust_string_new();
            lust_string_push_cstr(&s, "failed printing to stdout: ");
            if (e == EPIPE) lust_string_push_cstr(&s, "Broken pipe (os error 32)");
            else if (e == EBADF) lust_string_push_cstr(&s, "Bad file descriptor (os error 9)");
            else if (e == ENOSPC) lust_string_push_cstr(&s, "No space left on device (os error 28)");
            else if (e == 0) lust_string_push_cstr(&s, "failed to write whole buffer");
            else { char b[32]; snprintf(b, sizeof b, " (os error %d)", e); lust_string_push_cstr(&s, strerror(e)); lust_string_push_cstr(&s, b); }
            lust_out_len = 0;
            lust_panic_at(s.ptr, s.len, LUST_STD_SRC "std/src/io/stdio.rs", 1168, 9);
            return;
        }
        p += w;
        n -= (uint64_t)w;
    }
}

static void lust_out_flush_buf(void) {
    uint64_t n = lust_out_len;
    lust_out_len = 0;
    if (n) lust_out_raw(lust_out_buf, n);
}
#endif

/* LineWriterShim::write_all: the complete lines (through the last
   newline) go out after any held bytes; the tail then takes the
   BufWriter::write_all path. */
void lust_print_out(const uint8_t *p, uint64_t n) LUST_BODY({
    uint64_t k = n;
    while (k && p[k - 1] != '\n') k--;
    if (k) {
        lust_out_flush_buf();
        lust_out_raw(p, k);
        p += k;
        n -= k;
    } else if (lust_out_len && lust_out_buf[lust_out_len - 1] == '\n') {
        lust_out_flush_buf();
    }
    if (lust_out_len + n > sizeof lust_out_buf) lust_out_flush_buf();
    if (n >= sizeof lust_out_buf) lust_out_raw(p, n);
    else { memcpy(lust_out_buf + lust_out_len, p, n); lust_out_len += n; }
})

/* Flushes stdout: at exit and `process::exit` (Rust's rt::cleanup). */
void lust_print_flush(void) LUST_BODY({ lust_out_flush_buf(); })

/* `io::Write` for Stdout (sys.c): the same LineWriter as print!, so the
   two interleave in program order; 0 or an errno instead of a panic. */
int32_t lust_stdout_write_soft(const uint8_t *p, uint64_t n) LUST_BODY({
    lust_out_soft = 1;
    lust_out_err = 0;
    lust_print_out(p, n);
    lust_out_soft = 0;
    return lust_out_err;
})
int32_t lust_stdout_flush_soft(void) LUST_BODY({
    lust_out_soft = 1;
    lust_out_err = 0;
    lust_out_flush_buf();
    lust_out_soft = 0;
    return lust_out_err;
})

/* ---------- Stack overflow ----------
   Rust puts a SIGSEGV/SIGBUS handler on an alternate stack: a fault in
   the guard region below the thread's stack prints
       \nthread 'main' (<os tid>) has overflowed its stack\nfatal runtime error: stack overflow, aborting\n
   and aborts (134); any other fault gets the default action. The stack
   bounds come from pthread_getattr_np on Linux and from
   pthread_get_stackaddr_np on Darwin. The region counts 16
   MiB below the stack, as a tcc frame can step past one guard page. */
#if defined(LUST_IMPL) && (defined(__linux__) || defined(__APPLE__))
#ifdef __APPLE__
#include <sys/resource.h>
#else
/* No <pthread.h>: sync.c declares pthread_create with its own types.
   pthread_attr_t is opaque here (56 bytes on x86_64 and aarch64 glibc). */
typedef union { char bytes[64]; long align; } lust_pthread_attr;
unsigned long pthread_self(void);
int pthread_getattr_np(unsigned long, void *);
int pthread_attr_getstack(const void *, void **, size_t *);
int pthread_attr_destroy(void *);
#endif
static uintptr_t lust_guard_lo, lust_guard_hi;
static char lust_alt_stack[65536];
/* A spawned thread's guard window and name (sync.c): leaves the main
   thread's values when the caller is the main thread. */
static void lust_cur_thread_guard(uintptr_t *lo, uintptr_t *hi, const uint8_t **name, uint64_t *n);

static void lust_segv(int sig, siginfo_t *info, void *ctx) {
    (void)ctx;
    uintptr_t a = (uintptr_t)info->si_addr;
    uintptr_t lo = lust_guard_lo, hi = lust_guard_hi;
    const uint8_t *name = (const uint8_t *)"main";
    uint64_t name_len = 4;
    lust_cur_thread_guard(&lo, &hi, &name, &name_len);
    if (a >= lo && a < hi) {
        char tid[24];
        int n = 0;
        unsigned long long t = lust_os_tid();
        do { tid[n++] = (char)('0' + t % 10); t /= 10; } while (t);
        char msg[160 + 64];
        int m = 0;
        const char *head = "\nthread '", *mid = "' (", *tail = ") has overflowed its stack\nfatal runtime error: stack overflow, aborting\n";
        while (*head) msg[m++] = *head++;
        for (uint64_t i = 0; i < name_len && i < 64; i++) msg[m++] = (char)name[i];
        while (*mid) msg[m++] = *mid++;
        while (n) msg[m++] = tid[--n];
        while (*tail) msg[m++] = *tail++;
        if (write(2, msg, (size_t)m) < 0) {}
        abort();
    }
    /* Not a stack overflow: the default action on return re-runs the fault. */
    signal(sig, SIG_DFL);
}

/* The calling thread's guard window: one page above the stack's low end
   down to 16 MiB below it (see above). False when the bounds are unknown. */
static int lust_guard_window(uintptr_t *lo, uintptr_t *hi) {
    uintptr_t low;
#ifdef __APPLE__
    pthread_t self = pthread_self();
    void *top = pthread_get_stackaddr_np(self);
    if (!top) return 0;
    size_t size = pthread_get_stacksize_np(self);
    if (pthread_main_np()) {
        /* The main thread's size_np is not its stack size on macOS:
           std reads the main stack size from RLIMIT_STACK there. */
        struct rlimit rl;
        if (getrlimit(RLIMIT_STACK, &rl) == 0 && rl.rlim_cur != RLIM_INFINITY) size = (size_t)rl.rlim_cur;
    }
    if (!size || (uintptr_t)top < (uintptr_t)size) return 0;
    low = (uintptr_t)top - (uintptr_t)size;
#else
    lust_pthread_attr at;
    void *addr;
    size_t size;
    if (pthread_getattr_np(pthread_self(), &at) != 0) return 0;
    int ok = pthread_attr_getstack(&at, &addr, &size) == 0;
    pthread_attr_destroy(&at);
    if (!ok) return 0;
    low = (uintptr_t)addr;
#endif
    long page = sysconf(_SC_PAGESIZE);
    *hi = low + (uintptr_t)(page > 0 ? page : 4096);
    *lo = low > (16u << 20) ? low - (16u << 20) : 0;
    return 1;
}

/* A spawned thread's start (sync.c): its own guard window and alternate
   signal stack, as std's thread start sets them up, so an overflow there
   reports with the thread's name. `*alt` is freed by lust_thread_guard_fini. */
static void lust_thread_guard_init(uintptr_t *lo, uintptr_t *hi, void **alt) {
    if (!lust_guard_window(lo, hi)) return;
    stack_t ss;
    memset(&ss, 0, sizeof ss);
    ss.ss_sp = malloc(65536);
    if (!ss.ss_sp) return;
    ss.ss_size = 65536;
    if (sigaltstack(&ss, NULL) != 0) { free(ss.ss_sp); return; }
    *alt = ss.ss_sp;
}

static void lust_thread_guard_fini(void *alt) {
    if (!alt) return;
    stack_t ss;
    memset(&ss, 0, sizeof ss);
    ss.ss_flags = SS_DISABLE;
    sigaltstack(&ss, NULL);
    free(alt);
}

static void lust_stack_guard_init(void) {
    if (!lust_guard_window(&lust_guard_lo, &lust_guard_hi)) return;
    stack_t ss;
    memset(&ss, 0, sizeof ss);
    ss.ss_sp = lust_alt_stack;
    ss.ss_size = sizeof lust_alt_stack;
    if (sigaltstack(&ss, NULL) != 0) return;
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = lust_segv;
    sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGSEGV, &sa, NULL);
    sigaction(SIGBUS, &sa, NULL);
}
#elif defined(LUST_IMPL)
#ifdef _WIN32
/* Windows has no SIGSEGV guard page we can catch with sigaltstack.
   A vectored handler sees STATUS_STACK_OVERFLOW, prints Rust's line,
   and lets the default action terminate so the process status stays
   the one bash already reports for that exception. */
static LONG CALLBACK lust_win_overflow(EXCEPTION_POINTERS *ep) {
    if (!ep || !ep->ExceptionRecord) return 0; /* EXCEPTION_CONTINUE_SEARCH */
    DWORD code = ep->ExceptionRecord->ExceptionCode;
    if (code == 0xC0000005u) {
        /* An access violation is a lust bug. One line on stderr so the
           difftest failure names the faulting address. */
        char msg[80];
        uintptr_t a = ep->ExceptionRecord->ExceptionInformation && ep->ExceptionRecord->NumberParameters > 1
            ? (uintptr_t)ep->ExceptionRecord->ExceptionInformation[1] : 0;
        int m = 0;
        const char *head = "access violation at 0x";
        while (*head) msg[m++] = *head++;
        for (int s = (int)(sizeof(uintptr_t) * 2 - 1); s >= 0; s--) msg[m++] = "0123456789abcdef"[(a >> (s * 4)) & 0xf];
        msg[m++] = '\n';
        HANDLE err = GetStdHandle((DWORD)-12);
        DWORD wrote = 0;
        WriteFile(err, msg, (DWORD)m, &wrote, 0);
        return 0;
    }
    if (code != 0xC00000FDu) return 0;
    const uint8_t *name = (const uint8_t *)"main";
    uint64_t name_len = 4;
    const uint8_t *got;
    uint64_t got_len;
    lust_cur_thread_name(&got, &got_len);
    if (got && got_len && !(got_len == 9 && got[0] == '<')) {
        name = got;
        name_len = got_len;
    }
    char msg[160];
    int m = 0;
    const char *head = "\nthread '", *mid = "' (", *tail = ") has overflowed its stack\n";
    while (*head && m < (int)sizeof msg - 1) msg[m++] = *head++;
    for (uint64_t i = 0; i < name_len && m < (int)sizeof msg - 1; i++) msg[m++] = (char)name[i];
    while (*mid && m < (int)sizeof msg - 1) msg[m++] = *mid++;
    char tid[24];
    int n = 0;
    unsigned long t = (unsigned long)GetCurrentThreadId();
    do { tid[n++] = (char)('0' + t % 10); t /= 10; } while (t && n < (int)sizeof tid);
    while (n && m < (int)sizeof msg - 1) msg[m++] = tid[--n];
    while (*tail && m < (int)sizeof msg - 1) msg[m++] = *tail++;
    HANDLE err = GetStdHandle((DWORD)-12);
    DWORD wrote = 0;
    WriteFile(err, msg, (DWORD)m, &wrote, 0);
    return 0;
}
static void lust_stack_guard_init(void) {
    typedef PVOID (WINAPI *add_fn)(ULONG, LONG (CALLBACK *)(EXCEPTION_POINTERS *));
    add_fn add = (add_fn)GetProcAddress(GetModuleHandleA("kernel32.dll"), "AddVectoredExceptionHandler");
    if (add) add(1, lust_win_overflow);
}
#else
static void lust_stack_guard_init(void) {}
#endif
static void lust_thread_guard_init(uintptr_t *lo, uintptr_t *hi, void **alt) { (void)lo; (void)hi; (void)alt; }
static void lust_thread_guard_fini(void *alt) { (void)alt; }
#endif

/* Runtime setup before main: Rust ignores SIGPIPE and reports a stack
   overflow (above). */
void lust_rt_init(void) LUST_BODY({
#ifndef _WIN32
    signal(SIGPIPE, SIG_IGN);
#endif
    lust_stack_guard_init();
})

/* A print to stdout (`to_stderr` 0) or stderr whose fmt impl returned
   Err: Rust has already written the pieces before the failing one. */
void lust_print_failed(int to_stderr, lust_String *s) LUST_BODY({
    if (to_stderr) { if (s->len) fwrite(s->ptr, 1, s->len, stderr); fflush(stderr); }
    else lust_print_out(s->ptr, s->len);
    lust_panic_std("a formatting trait implementation returned an error when the underlying stream did not", "core/src/io/write.rs:411:17");
})
