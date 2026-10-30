/* Slice sorting for the lust runtime: a port of Rust 1.99's driftsort
   (`sort`, `sort_by`, `sort_by_key`) and ipnsort (`sort_unstable*`)
   from library/core/src/slice/sort, comparison for comparison
   (W-STD-A phase 3; ARCH 3.3 "Sorting").

   Exactness scope: the sequence of `is_less` calls, the resulting
   order (including equal elements under the unstable sort and
   inconsistent comparators), and the order-violation panic all match
   Rust for the same element size, Copy-ness, and freeze-ness (no
   Cell/RefCell inside). Those three properties choose the small-sort
   variant in Rust, so the caller passes them. Scratch length follows
   Rust (a 4096-byte stack buffer when it fits, else a heap buffer of
   the computed length). Every scratch buffer is aligned for the
   element (`align`), because the comparator sees pointers into it:
   an element aligned over 16 never uses a stack buffer, since tcc
   does not realign stack frames (fx-overalign).

   Elements are moved as bytes (`size` per element), as Rust's ptr
   copies are. `less(ctx, a, b)` is the comparator adapter codegen
   emits per call site (sort_by: `compare(a, b) == Less`; sort_by_key:
   `f(a).lt(&f(b))`; sort: `a.lt(b)`). */

typedef uint8_t (*lust_less_fn)(void *ctx, const void *a, const void *b);

typedef struct {
    uint64_t size, align;
    uint8_t freeze, copy;
    void *ctx;
    lust_less_fn less;
} lust_sort_cx;

#define LS_AT(p, i) ((uint8_t *)(p) + (uint64_t)(i) * s->size)
#define LS_LESS(a, b) (s->less(s->ctx, (a), (b)) != 0)

uint32_t lust_sort_ilog2(uint64_t n) LUST_BODY({ return 63 - (uint32_t)__builtin_clzll(n); })

void lust_sort_swap(lust_sort_cx *s, uint8_t *a, uint8_t *b) LUST_BODY({
    uint8_t tmp[64];
    uint64_t left = s->size;
    while (left) {
        uint64_t n = left < 64 ? left : 64;
        lust_copy_small(tmp, a, n);
        lust_copy_small(a, b, n);
        lust_copy_small(b, tmp, n);
        a += n;
        b += n;
        left -= n;
    }
})

void lust_sort_reverse(lust_sort_cx *s, uint8_t *v, uint64_t len) LUST_BODY({
    for (uint64_t i = 0; i < len / 2; i++) lust_sort_swap(s, LS_AT(v, i), LS_AT(v, len - 1 - i));
})

/* A temporary element buffer: on the stack for small elements. */
typedef struct { uint8_t *p; uint8_t local[64] __attribute__((aligned(16))); } lust_sort_tmp;
uint8_t *lust_sort_tmp_get(lust_sort_cx *s, lust_sort_tmp *t) LUST_BODY({
    t->p = s->size <= 64 && s->align <= 16 ? t->local : (uint8_t *)lust_alloc(s->size, s->align);
    return t->p;
})
void lust_sort_tmp_free(lust_sort_cx *s, lust_sort_tmp *t) LUST_BODY({
    /* Heap temps come from lust_alloc. On Windows an element aligned past
       16 is an _aligned_malloc block; free() of it fail-fasts. */
    if (t->p != t->local) lust_dealloc(t->p, s->size, s->align);
})

/* shared/mod.rs find_existing_run */
uint64_t lust_sort_find_run(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *was_reversed) LUST_BODY({
    *was_reversed = 0;
    if (len < 2) return len;
    uint64_t run_len = 2;
    uint8_t desc = LS_LESS(LS_AT(v, 1), LS_AT(v, 0));
    if (desc) {
        while (run_len < len && LS_LESS(LS_AT(v, run_len), LS_AT(v, run_len - 1))) run_len++;
    } else {
        while (run_len < len && !LS_LESS(LS_AT(v, run_len), LS_AT(v, run_len - 1))) run_len++;
    }
    *was_reversed = desc;
    return run_len;
})

/* smallsort.rs insert_tail: `tail` sifts left until `begin`. */
void lust_sort_insert_tail(lust_sort_cx *s, uint8_t *begin, uint8_t *tail) LUST_BODY({
    uint8_t *sift = tail - s->size;
    if (!LS_LESS(tail, sift)) return;
    lust_sort_tmp t;
    uint8_t *tmp = lust_sort_tmp_get(s, &t);
    lust_copy_small(tmp, tail, s->size);
    uint8_t *gap = tail;
    for (;;) {
        lust_copy_small(gap, sift, s->size);
        gap = sift;
        if (sift == begin) break;
        sift -= s->size;
        if (!LS_LESS(tmp, sift)) break;
    }
    lust_copy_small(gap, tmp, s->size);
    lust_sort_tmp_free(s, &t);
})

void lust_sort_insertion_shift_left(lust_sort_cx *s, uint8_t *v, uint64_t len, uint64_t offset) LUST_BODY({
    if (offset == 0 || offset > len) abort();
    for (uint64_t i = offset; i < len; i++) lust_sort_insert_tail(s, v, LS_AT(v, i));
})

/* smallsort.rs sort4_stable: 4 elements of v into dst, stably. */
void lust_sort_sort4_stable(lust_sort_cx *s, uint8_t *v, uint8_t *dst) LUST_BODY({
    uint8_t c1 = LS_LESS(LS_AT(v, 1), LS_AT(v, 0));
    uint8_t c2 = LS_LESS(LS_AT(v, 3), LS_AT(v, 2));
    uint8_t *a = LS_AT(v, c1);
    uint8_t *b = LS_AT(v, !c1);
    uint8_t *c = LS_AT(v, 2 + c2);
    uint8_t *d = LS_AT(v, 2 + !c2);
    uint8_t c3 = LS_LESS(c, a);
    uint8_t c4 = LS_LESS(d, b);
    uint8_t *min = c3 ? c : a;
    uint8_t *max = c4 ? b : d;
    uint8_t *unknown_left = c3 ? a : (c4 ? c : b);
    uint8_t *unknown_right = c4 ? d : (c3 ? b : c);
    uint8_t c5 = LS_LESS(unknown_right, unknown_left);
    uint8_t *lo = c5 ? unknown_right : unknown_left;
    uint8_t *hi = c5 ? unknown_left : unknown_right;
    lust_copy_small(LS_AT(dst, 0), min, s->size);
    lust_copy_small(LS_AT(dst, 1), lo, s->size);
    lust_copy_small(LS_AT(dst, 2), hi, s->size);
    lust_copy_small(LS_AT(dst, 3), max, s->size);
})

void lust_sort_ord_violation(void) LUST_BODY({
    lust_panic_cstr("user-provided comparison function does not correctly implement a total order", 0);
})

/* smallsort.rs bidirectional_merge: the two sorted halves of src
   (len / 2 and the rest) into dst, from both ends at once. */
void lust_sort_bidir_merge(lust_sort_cx *s, uint8_t *src, uint64_t len, uint8_t *dst) LUST_BODY({
    uint64_t half = len / 2;
    uint64_t sz = s->size;
    uint8_t *left = src, *right = LS_AT(src, half), *out = dst;
    uint8_t *left_rev = LS_AT(src, half - 1), *right_rev = LS_AT(src, len - 1), *out_rev = LS_AT(dst, len - 1);
    for (uint64_t i = 0; i < half; i++) {
        /* merge_up */
        uint8_t is_l = !LS_LESS(right, left);
        lust_copy_small(out, is_l ? left : right, sz);
        right += is_l ? 0 : sz;
        left += is_l ? sz : 0;
        out += sz;
        /* merge_down */
        is_l = !LS_LESS(right_rev, left_rev);
        lust_copy_small(out_rev, is_l ? right_rev : left_rev, sz);
        right_rev -= is_l ? sz : 0;
        left_rev -= is_l ? 0 : sz;
        out_rev -= sz;
    }
    uint8_t *left_end = left_rev + sz, *right_end = right_rev + sz;
    if (len % 2 != 0) {
        uint8_t left_nonempty = left < left_end;
        lust_copy_small(out, left_nonempty ? left : right, sz);
        left += left_nonempty ? sz : 0;
        right += left_nonempty ? 0 : sz;
    }
    if (left != left_end || right != right_end) lust_sort_ord_violation();
})

void lust_sort_sort8_stable(lust_sort_cx *s, uint8_t *v, uint8_t *dst, uint8_t *scratch) LUST_BODY({
    lust_sort_sort4_stable(s, v, scratch);
    lust_sort_sort4_stable(s, LS_AT(v, 4), LS_AT(scratch, 4));
    lust_sort_bidir_merge(s, scratch, 8, dst);
})

/* smallsort.rs small_sort_general_with_scratch (scratch >= len + 16). */
void lust_sort_small_general(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *scratch) LUST_BODY({
    if (len < 2) return;
    uint64_t half = len / 2;
    uint64_t presorted;
    if (s->size <= 16 && len >= 16) {
        lust_sort_sort8_stable(s, v, scratch, LS_AT(scratch, len));
        lust_sort_sort8_stable(s, LS_AT(v, half), LS_AT(scratch, half), LS_AT(scratch, len + 8));
        presorted = 8;
    } else if (len >= 8) {
        lust_sort_sort4_stable(s, v, scratch);
        lust_sort_sort4_stable(s, LS_AT(v, half), LS_AT(scratch, half));
        presorted = 4;
    } else {
        lust_copy_small(scratch, v, s->size);
        lust_copy_small(LS_AT(scratch, half), LS_AT(v, half), s->size);
        presorted = 1;
    }
    for (int k = 0; k < 2; k++) {
        uint64_t offset = k == 0 ? 0 : half;
        uint8_t *src = LS_AT(v, offset), *dst = LS_AT(scratch, offset);
        uint64_t desired = k == 0 ? half : len - half;
        for (uint64_t i = presorted; i < desired; i++) {
            lust_copy_small(LS_AT(dst, i), LS_AT(src, i), s->size);
            lust_sort_insert_tail(s, dst, LS_AT(dst, i));
        }
    }
    lust_sort_bidir_merge(s, scratch, len, v);
})

/* smallsort.rs swap_if_less: a stable compare-exchange. */
void lust_sort_swap_if_less(lust_sort_cx *s, uint8_t *v, uint64_t a_pos, uint64_t b_pos) LUST_BODY({
    uint8_t *va = LS_AT(v, a_pos), *vb = LS_AT(v, b_pos);
    if (LS_LESS(vb, va)) lust_sort_swap(s, va, vb);
})

void lust_sort_net(lust_sort_cx *s, uint8_t *v, const uint8_t *pairs, int n) LUST_BODY({
    for (int i = 0; i < n; i++) lust_sort_swap_if_less(s, v, pairs[2 * i], pairs[2 * i + 1]);
})

/* smallsort.rs small_sort_network (len <= 32, Copy + Freeze, size <= 8). */
void lust_sort_small_network(lust_sort_cx *s, uint8_t *v, uint64_t len) LUST_BODY({
    if (len < 2) return;
    if (len > 32) abort();
    uint64_t half = len / 2;
    uint8_t no_merge = len < 18;
    uint8_t *region = v;
    uint64_t region_len = no_merge ? len : half;
    for (;;) {
        uint64_t presorted;
        static const uint8_t lust_sort_net9[] = {0, 3, 1, 7, 2, 5, 4, 8, 0, 7, 2, 4, 3, 8, 5, 6, 0, 2, 1, 3, 4, 5, 7, 8, 1, 4, 3, 6, 5, 7,
                                                 0, 1, 2, 4, 3, 5, 6, 8, 2, 3, 4, 5, 6, 7, 1, 2, 3, 4, 5, 6};
        static const uint8_t lust_sort_net13[] = {0, 12, 1, 10, 2, 9, 3, 7, 5, 11, 6, 8, 1, 6, 2, 3, 4, 11, 7, 9, 8, 10, 0, 4, 1, 2, 3, 6,
                                                  7, 8, 9, 10, 11, 12, 4, 6, 5, 9, 8, 11, 10, 12, 0, 5, 3, 8, 4, 7, 6, 11, 9, 10, 0, 1, 2, 5,
                                                  6, 9, 7, 8, 10, 11, 1, 3, 2, 4, 5, 6, 9, 10, 1, 2, 3, 4, 5, 7, 6, 8, 2, 3, 4, 5, 6, 7, 8, 9,
                                                  3, 4, 5, 6};
        if (region_len >= 13) {
            lust_sort_net(s, region, lust_sort_net13, 45);
            presorted = 13;
        } else if (region_len >= 9) {
            lust_sort_net(s, region, lust_sort_net9, 25);
            presorted = 9;
        } else {
            presorted = 1;
        }
        lust_sort_insertion_shift_left(s, region, region_len, presorted);
        if (no_merge) return;
        if (region != v) break;
        region = LS_AT(v, half);
        region_len = len - half;
    }
    uint8_t stack_buf[32 * 8] __attribute__((aligned(16)));
    uint8_t *stack = s->align <= 16 ? stack_buf : (uint8_t *)lust_alloc(sizeof stack_buf, s->align);
    lust_sort_bidir_merge(s, v, len, stack);
    lust_copy_small(v, stack, len * s->size);
    if (stack != stack_buf) lust_dealloc(stack, sizeof stack_buf, s->align);
})

/* shared/pivot.rs */
uint8_t *lust_sort_median3(lust_sort_cx *s, uint8_t *a, uint8_t *b, uint8_t *c) LUST_BODY({
    uint8_t x = LS_LESS(a, b);
    uint8_t y = LS_LESS(a, c);
    if (x == y) {
        uint8_t z = LS_LESS(b, c);
        return (z ^ x) ? c : b;
    }
    return a;
})
uint8_t *lust_sort_median3_rec(lust_sort_cx *s, uint8_t *a, uint8_t *b, uint8_t *c, uint64_t n) LUST_BODY({
    if (n * 8 >= 64) {
        uint64_t n8 = n / 8;
        a = lust_sort_median3_rec(s, a, LS_AT(a, n8 * 4), LS_AT(a, n8 * 7), n8);
        b = lust_sort_median3_rec(s, b, LS_AT(b, n8 * 4), LS_AT(b, n8 * 7), n8);
        c = lust_sort_median3_rec(s, c, LS_AT(c, n8 * 4), LS_AT(c, n8 * 7), n8);
    }
    return lust_sort_median3(s, a, b, c);
})
uint64_t lust_sort_choose_pivot(lust_sort_cx *s, uint8_t *v, uint64_t len) LUST_BODY({
    if (len < 8) abort();
    uint64_t d8 = len / 8;
    uint8_t *a = v, *b = LS_AT(v, d8 * 4), *c = LS_AT(v, d8 * 7);
    uint8_t *m = len < 64 ? lust_sort_median3(s, a, b, c) : lust_sort_median3_rec(s, a, b, c, d8);
    return (uint64_t)(m - v) / s->size;
})

/* ---------- stable: driftsort ---------- */

uint64_t lust_sort_stable_threshold(lust_sort_cx *s) LUST_BODY({ return s->freeze ? 32 : 16; })

void lust_sort_stable_small(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *scratch) LUST_BODY({
    if (s->freeze) {
        lust_sort_small_general(s, v, len, scratch);
    } else if (len >= 2) {
        lust_sort_insertion_shift_left(s, v, len, 1);
    }
})

/* stable/merge.rs merge: v[..mid] and v[mid..] are sorted. */
void lust_sort_merge(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *scratch, uint64_t scratch_len, uint64_t mid) LUST_BODY({
    uint64_t sz = s->size;
    if (mid == 0 || mid >= len || scratch_len < (mid < len - mid ? mid : len - mid)) return;
    uint8_t *buf = scratch, *v_mid = LS_AT(v, mid), *v_end = LS_AT(v, len);
    uint64_t left_len = mid, right_len = len - mid;
    uint8_t left_shorter = left_len <= right_len;
    uint8_t *save_base = left_shorter ? v : v_mid;
    uint64_t save_len = left_shorter ? left_len : right_len;
    lust_copy_small(buf, save_base, save_len * sz);
    uint8_t *start = buf, *end = buf + save_len * sz, *dst = save_base;
    if (left_shorter) {
        uint8_t *right = v_mid;
        while (start != end && right != v_end) {
            uint8_t consume_left = !LS_LESS(right, start);
            lust_copy_small(dst, consume_left ? start : right, sz);
            start += consume_left ? sz : 0;
            right += consume_left ? 0 : sz;
            dst += sz;
        }
    } else {
        uint8_t *out = v_end;
        for (;;) {
            uint8_t *left = dst - sz, *right = end - sz;
            out -= sz;
            uint8_t consume_left = LS_LESS(right, left);
            lust_copy_small(out, consume_left ? left : right, sz);
            dst = consume_left ? left : left + sz;
            end = consume_left ? right + sz : right;
            if (dst == v || end == buf) break;
        }
    }
    lust_copy_small(dst, start, (uint64_t)(end - start));
})

void lust_sort_drift(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *scratch, uint64_t scratch_len, uint8_t eager_sort);

/* stable/quicksort.rs stable_partition; `inv` sorts with
   `|a, b| !is_less(b, a)` (the equal-partition comparator). */
uint64_t lust_sort_stable_partition(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *scratch, uint64_t scratch_len,
                                           uint64_t pivot_pos, uint8_t pivot_goes_left, uint8_t inv) LUST_BODY({
    uint64_t sz = s->size;
    if (scratch_len < len || pivot_pos >= len) abort();
    uint8_t *pivot = LS_AT(v, pivot_pos);
    uint8_t *scan = v, *scratch_rev = LS_AT(scratch, len), *pivot_in_scratch = 0;
    uint64_t num_left = 0, loop_end_pos = pivot_pos;
    for (;;) {
        uint8_t *loop_end = LS_AT(v, loop_end_pos);
        while (scan < loop_end) {
            uint8_t towards_left = inv ? !LS_LESS(pivot, scan) : LS_LESS(scan, pivot);
            scratch_rev -= sz;
            uint8_t *dst = (towards_left ? scratch : scratch_rev) + num_left * sz;
            lust_copy_small(dst, scan, sz);
            num_left += towards_left;
            scan += sz;
        }
        if (loop_end_pos == len) break;
        scratch_rev -= sz;
        pivot_in_scratch = (pivot_goes_left ? scratch : scratch_rev) + num_left * sz;
        lust_copy_small(pivot_in_scratch, scan, sz);
        num_left += pivot_goes_left;
        scan += sz;
        loop_end_pos = len;
    }
    if (!s->freeze) lust_copy_small(pivot_in_scratch, pivot, sz);
    lust_copy_small(v, scratch, num_left * sz);
    for (uint64_t i = 0; i < len - num_left; i++) lust_copy_small(LS_AT(v, num_left + i), LS_AT(scratch, len - 1 - i), sz);
    return num_left;
})

/* stable/quicksort.rs quicksort */
void lust_sort_stable_quick(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *scratch, uint64_t scratch_len, uint32_t limit,
                                   uint8_t *left_ancestor_pivot) LUST_BODY({
    for (;;) {
        if (len <= lust_sort_stable_threshold(s)) {
            lust_sort_stable_small(s, v, len, scratch);
            return;
        }
        if (limit == 0) {
            lust_sort_drift(s, v, len, scratch, scratch_len, 1);
            return;
        }
        limit--;
        uint64_t pivot_pos = lust_sort_choose_pivot(s, v, len);
        lust_sort_tmp t;
        uint8_t *pivot_copy = lust_sort_tmp_get(s, &t);
        lust_copy_small(pivot_copy, LS_AT(v, pivot_pos), s->size);
        uint8_t *pivot_ref = s->freeze ? pivot_copy : 0;
        uint8_t perform_equal = 0;
        if (left_ancestor_pivot) perform_equal = !LS_LESS(left_ancestor_pivot, LS_AT(v, pivot_pos));
        uint64_t left_len = 0;
        if (!perform_equal) {
            left_len = lust_sort_stable_partition(s, v, len, scratch, scratch_len, pivot_pos, 0, 0);
            perform_equal = left_len == 0;
        }
        if (perform_equal) {
            uint64_t mid_eq = lust_sort_stable_partition(s, v, len, scratch, scratch_len, pivot_pos, 1, 1);
            v = LS_AT(v, mid_eq);
            len -= mid_eq;
            left_ancestor_pivot = 0;
            lust_sort_tmp_free(s, &t);
            continue;
        }
        lust_sort_stable_quick(s, LS_AT(v, left_len), len - left_len, scratch, scratch_len, limit, pivot_ref);
        lust_sort_tmp_free(s, &t);
        len = left_len;
    }
})

void lust_sort_stable_quicksort(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *scratch, uint64_t scratch_len) LUST_BODY({
    uint32_t limit = 2 * lust_sort_ilog2(len | 1);
    lust_sort_stable_quick(s, v, len, scratch, scratch_len, limit, 0);
})

/* stable/drift.rs; a run is (len << 1) | sorted. */
void lust_sort_drift(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *scratch, uint64_t scratch_len, uint8_t eager_sort) LUST_BODY({
    if (len < 2) return;
    uint64_t scale = ((1ull << 62) + len - 1) / len;
    uint64_t min_good;
    if (len <= 64 * 64) {
        min_good = len - len / 2 < 64 ? len - len / 2 : 64;
    } else {
        uint32_t ilog = lust_sort_ilog2(len | 1);
        uint32_t shift = (ilog + 1) / 2;
        min_good = ((1ull << shift) + (len >> shift)) / 2;
    }
    uint64_t runs[66];
    uint8_t depths[66];
    uint64_t stack_len = 0, scan_idx = 0;
    uint64_t prev_run = 1; /* new_sorted(0) */
    for (;;) {
        uint64_t next_run;
        uint8_t desired;
        if (scan_idx < len) {
            /* create_run */
            uint8_t *rv = LS_AT(v, scan_idx);
            uint64_t rlen = len - scan_idx;
            next_run = 0;
            uint8_t made = 0;
            if (rlen >= min_good) {
                uint8_t rev;
                uint64_t run_len = lust_sort_find_run(s, rv, rlen, &rev);
                if (run_len >= min_good) {
                    if (rev) lust_sort_reverse(s, rv, run_len);
                    next_run = (run_len << 1) | 1;
                    made = 1;
                }
            }
            if (!made) {
                if (eager_sort) {
                    uint64_t thr = lust_sort_stable_threshold(s);
                    uint64_t eager_len = thr < rlen ? thr : rlen;
                    lust_sort_stable_quick(s, rv, eager_len, scratch, scratch_len, 0, 0);
                    next_run = (eager_len << 1) | 1;
                } else {
                    next_run = (min_good < rlen ? min_good : rlen) << 1;
                }
            }
            uint64_t left = scan_idx - (prev_run >> 1), mid = scan_idx, right = scan_idx + (next_run >> 1);
            uint64_t x = left + mid, y = mid + right;
            uint64_t z = (scale * x) ^ (scale * y);
            desired = z == 0 ? 64 : (uint8_t)__builtin_clzll(z);
        } else {
            next_run = 1;
            desired = 0;
        }
        while (stack_len > 1 && depths[stack_len - 1] >= desired) {
            uint64_t left = runs[stack_len - 1];
            uint64_t merged_len = (left >> 1) + (prev_run >> 1);
            uint8_t *mv = LS_AT(v, scan_idx - merged_len);
            /* logical_merge */
            if (merged_len > scratch_len || (left & 1) || (prev_run & 1)) {
                if (!(left & 1)) lust_sort_stable_quicksort(s, mv, left >> 1, scratch, scratch_len);
                if (!(prev_run & 1)) lust_sort_stable_quicksort(s, LS_AT(mv, left >> 1), prev_run >> 1, scratch, scratch_len);
                lust_sort_merge(s, mv, merged_len, scratch, scratch_len, left >> 1);
                prev_run = (merged_len << 1) | 1;
            } else {
                prev_run = merged_len << 1;
            }
            stack_len--;
        }
        runs[stack_len] = prev_run;
        depths[stack_len] = desired;
        stack_len++;
        if (scan_idx >= len) break;
        scan_idx += next_run >> 1;
        prev_run = next_run;
    }
    if (!(prev_run & 1)) lust_sort_stable_quicksort(s, v, len, scratch, scratch_len);
})

/* stable/mod.rs sort and driftsort_main */
void lust_sort_stable(void *v, uint64_t len, uint64_t size, uint64_t align, uint8_t freeze, void *ctx, lust_less_fn less) LUST_BODY({
    if (size == 0 || len < 2) return;
    lust_sort_cx cx = {size, align, freeze, 0, ctx, less};
    lust_sort_cx *s = &cx;
    if (len <= 20) {
        lust_sort_insertion_shift_left(s, (uint8_t *)v, len, 1);
        return;
    }
    uint64_t max_full = 8000000 / size;
    uint64_t a = len - len / 2, b = len < max_full ? len : max_full;
    uint64_t alloc_len = a > b ? a : b;
    if (alloc_len < 48) alloc_len = 48;
    uint64_t stack_len = 4096 / size;
    uint64_t scratch_len = stack_len >= alloc_len ? stack_len : alloc_len;
    uint8_t *scratch = align <= 16 ? (uint8_t *)malloc(scratch_len * size) : (uint8_t *)lust_alloc(scratch_len * size, align);
    if (!scratch) abort();
    uint8_t eager = len <= lust_sort_stable_threshold(s) * 2;
    lust_sort_drift(s, (uint8_t *)v, len, scratch, scratch_len, eager);
    lust_dealloc(scratch, scratch_len * size, align);
})

/* ---------- unstable: ipnsort ---------- */

uint8_t lust_sort_network_ok(lust_sort_cx *s) LUST_BODY({ return s->copy && s->size <= 8 && s->size * 32 <= 4096; })

uint64_t lust_sort_unstable_threshold(lust_sort_cx *s) LUST_BODY({
    if (!s->freeze) return 16;
    if (lust_sort_network_ok(s)) return 32;
    return s->size * 48 <= 4096 ? 32 : 16;
})

void lust_sort_unstable_small(lust_sort_cx *s, uint8_t *v, uint64_t len) LUST_BODY({
    if (s->freeze && lust_sort_network_ok(s)) {
        lust_sort_small_network(s, v, len);
    } else if (s->freeze && s->size * 48 <= 4096) {
        uint8_t scratch_buf[4096] __attribute__((aligned(16)));
        uint8_t *scratch = s->align <= 16 ? scratch_buf : (uint8_t *)lust_alloc(sizeof scratch_buf, s->align);
        lust_sort_small_general(s, v, len, scratch);
        if (scratch != scratch_buf) lust_dealloc(scratch, sizeof scratch_buf, s->align);
    } else if (len >= 2) {
        lust_sort_insertion_shift_left(s, v, len, 1);
    }
})

/* unstable/heapsort.rs */
void lust_sort_sift_down(lust_sort_cx *s, uint8_t *v, uint64_t len, uint64_t node) LUST_BODY({
    for (;;) {
        uint64_t child = 2 * node + 1;
        if (child >= len) break;
        if (child + 1 < len) child += LS_LESS(LS_AT(v, child), LS_AT(v, child + 1));
        if (!LS_LESS(LS_AT(v, node), LS_AT(v, child))) break;
        lust_sort_swap(s, LS_AT(v, node), LS_AT(v, child));
        node = child;
    }
})
void lust_sort_heapsort(lust_sort_cx *s, uint8_t *v, uint64_t len) LUST_BODY({
    for (uint64_t i = len + len / 2; i-- > 0;) {
        uint64_t sift_idx;
        if (i >= len) {
            sift_idx = i - len;
        } else {
            lust_sort_swap(s, v, LS_AT(v, i));
            sift_idx = 0;
        }
        lust_sort_sift_down(s, v, i < len ? i : len, sift_idx);
    }
})

/* unstable/quicksort.rs partitions; `inv` is `|a, b| !is_less(b, a)`. */
uint8_t lust_sort_cmp(lust_sort_cx *s, uint8_t *a, uint8_t *b, uint8_t inv) LUST_BODY({ return inv ? !LS_LESS(b, a) : LS_LESS(a, b); })

uint64_t lust_sort_lomuto(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *pivot, uint8_t inv) LUST_BODY({
    uint64_t sz = s->size;
    if (len == 0) return 0;
    lust_sort_tmp t;
    uint8_t *gap_value = lust_sort_tmp_get(s, &t);
    lust_copy_small(gap_value, v, sz);
    uint64_t num_lt = 0;
    uint8_t *right = LS_AT(v, 1), *gap_pos = v, *end = LS_AT(v, len);
    for (;;) {
        uint8_t is_done = right == end;
        if (is_done) right = gap_value;
        uint8_t right_is_lt = lust_sort_cmp(s, right, pivot, inv);
        uint8_t *left = LS_AT(v, num_lt);
        lust_copy_small(gap_pos, left, sz);
        lust_copy_small(left, right, sz);
        gap_pos = right;
        num_lt += right_is_lt;
        right += sz;
        if (is_done) break;
    }
    lust_sort_tmp_free(s, &t);
    return num_lt;
})

uint64_t lust_sort_hoare(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *pivot, uint8_t inv) LUST_BODY({
    uint64_t sz = s->size;
    if (len == 0) return 0;
    lust_sort_tmp t;
    uint8_t *gap_value = lust_sort_tmp_get(s, &t);
    uint8_t *gap_pos = 0;
    uint8_t *left = v, *right = LS_AT(v, len);
    for (;;) {
        while (left < right && lust_sort_cmp(s, left, pivot, inv)) left += sz;
        for (;;) {
            right -= sz;
            if (left >= right || lust_sort_cmp(s, right, pivot, inv)) break;
        }
        if (left >= right) break;
        if (!gap_pos) {
            lust_copy_small(gap_value, left, sz);
        } else {
            lust_copy_small(gap_pos, left, sz);
        }
        gap_pos = right;
        lust_copy_small(left, right, sz);
        left += sz;
    }
    if (gap_pos) lust_copy_small(gap_pos, gap_value, sz);
    lust_sort_tmp_free(s, &t);
    return (uint64_t)(left - v) / sz;
})

uint64_t lust_sort_partition(lust_sort_cx *s, uint8_t *v, uint64_t len, uint64_t pivot, uint8_t inv) LUST_BODY({
    if (len == 0) return 0;
    if (pivot >= len) abort();
    lust_sort_swap(s, v, LS_AT(v, pivot));
    uint64_t num_lt = s->size <= 96 ? lust_sort_lomuto(s, LS_AT(v, 1), len - 1, v, inv) : lust_sort_hoare(s, LS_AT(v, 1), len - 1, v, inv);
    if (num_lt >= len) abort();
    lust_sort_swap(s, v, LS_AT(v, num_lt));
    return num_lt;
})

void lust_sort_unstable_quick(lust_sort_cx *s, uint8_t *v, uint64_t len, uint8_t *ancestor_pivot, uint32_t limit) LUST_BODY({
    for (;;) {
        if (len <= lust_sort_unstable_threshold(s)) {
            lust_sort_unstable_small(s, v, len);
            return;
        }
        if (limit == 0) {
            lust_sort_heapsort(s, v, len);
            return;
        }
        limit--;
        uint64_t pivot_pos = lust_sort_choose_pivot(s, v, len);
        if (ancestor_pivot && !LS_LESS(ancestor_pivot, LS_AT(v, pivot_pos))) {
            uint64_t num_lt = lust_sort_partition(s, v, len, pivot_pos, 1);
            v = LS_AT(v, num_lt + 1);
            len -= num_lt + 1;
            ancestor_pivot = 0;
            continue;
        }
        uint64_t num_lt = lust_sort_partition(s, v, len, pivot_pos, 0);
        lust_sort_unstable_quick(s, v, num_lt, ancestor_pivot, limit);
        ancestor_pivot = LS_AT(v, num_lt);
        v = LS_AT(v, num_lt + 1);
        len -= num_lt + 1;
    }
})

/* unstable/mod.rs sort and ipnsort */
void lust_sort_unstable(void *v, uint64_t len, uint64_t size, uint64_t align, uint8_t freeze, uint8_t copy, void *ctx, lust_less_fn less) LUST_BODY({
    if (size == 0 || len < 2) return;
    lust_sort_cx cx = {size, align, freeze, copy, ctx, less};
    lust_sort_cx *s = &cx;
    uint8_t *b = (uint8_t *)v;
    if (len <= 20) {
        lust_sort_insertion_shift_left(s, b, len, 1);
        return;
    }
    uint8_t rev;
    uint64_t run_len = lust_sort_find_run(s, b, len, &rev);
    if (run_len == len) {
        if (rev) lust_sort_reverse(s, b, len);
        return;
    }
    lust_sort_unstable_quick(s, b, len, 0, 2 * lust_sort_ilog2(len | 1));
})

#undef LS_AT
#undef LS_LESS

/* slice::binary_search_by (Rust 1.99's loop): the Ok index, or
   -(index + 1) for Err(index). `cmp(ctx, p)` answers `p.cmp(target)`
   as negative, zero, or positive (Less, Equal, Greater). */
typedef int32_t (*lust_cmp_fn)(void *ctx, const void *p);
int64_t lust_binary_search(void *v, uint64_t len, uint64_t size, void *ctx, lust_cmp_fn cmp) LUST_BODY({
    if (len == 0) return -1;
    uint64_t base = 0, n = len;
    while (n > 1) {
        uint64_t half = n / 2;
        uint64_t mid = base + half;
        int32_t c = cmp(ctx, (uint8_t *)v + mid * size);
        base = c > 0 ? base : mid;
        n -= half;
    }
    int32_t c = cmp(ctx, (uint8_t *)v + base * size);
    if (c == 0) return (int64_t)base;
    return -(int64_t)(base + (c < 0)) - 1;
})
