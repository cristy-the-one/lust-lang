/* Erased hash table core: see hash.h for the contract. This file is a
   scalar port of hashbrown 0.17.1 raw.rs (Rust 1.99 vendored std
   sources, library/vendor/hashbrown-0.17.1). Function and variable
   comments name the hashbrown counterpart where the match is not
   obvious. Internal helpers are static so this fragment can be
   amalgamated into the runtime unit without symbol clashes. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define LH_WIDTH ((uint64_t)LUST_HASH_GROUP_WIDTH)

static void lust_hash_capacity_overflow(void) {
    static const char msg[] = "Hash table capacity overflow";
    lust_panic_at((const uint8_t *)msg, sizeof msg - 1, "/rust/deps/hashbrown-0.17.1/src/raw.rs", 36, 40);
}

/* Tag::is_full / special_is_empty for one control byte. */
static int lh_is_full(uint8_t c) { return (c & 0x80u) == 0; }

/* Group::static_empty for the empty singleton; 16 bytes cover both
   group widths this core supports. */
const uint8_t lust_hash_static_empty[16] = {
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
};

static uint64_t lh_align_up(uint64_t x, uint64_t a) { return (x + a - 1) & ~(a - 1); }

static uint64_t lh_ctrl_align(const lust_hash_table *t) {
    uint64_t a = t->elem_align < LH_WIDTH ? LH_WIDTH : t->elem_align;
    return a ? a : LH_WIDTH;
}

/* RawTableInner::bucket_ptr with forward storage. */
static void *lh_slot(const lust_hash_table *t, uint64_t index) {
    return (void *)(t->data + index * t->elem_size);
}

/* bucket_mask_to_capacity. */
static uint64_t lh_bucket_mask_to_capacity(uint64_t bucket_mask) {
    if (bucket_mask < 8) return bucket_mask;
    return ((bucket_mask + 1) / 8) * 7;
}

/* capacity_to_buckets. Returns 0 on overflow (None in Rust). */
static int lh_capacity_to_buckets(uint64_t cap, uint64_t elem_size, uint64_t *out) {
    if (cap < 15) {
        uint64_t min_cap;
        if (LH_WIDTH == 16 && elem_size <= 1) min_cap = 14;
        else if (LH_WIDTH == 16 && elem_size >= 2 && elem_size <= 3) min_cap = 7;
        else if (LH_WIDTH == 8 && elem_size <= 1) min_cap = 7;
        else min_cap = 3;
        if (cap < min_cap) cap = min_cap;
        *out = cap < 4 ? 4 : cap < 8 ? 8 : 16;
        return 1;
    }
    if (cap > UINT64_MAX / 8) return 0;
    uint64_t adjusted = cap * 8 / 7;
    uint64_t buckets = 1;
    while (buckets < adjusted) {
        if (buckets > (UINT64_MAX >> 1)) return 0;
        buckets <<= 1;
    }
    *out = buckets;
    return 1;
}

/* TableLayout::calculate_layout_for plus allocation. One malloc holds the
   buckets then the control bytes, aligned to ctrl_align. Control bytes
   are left uninitialized (RawTableInner::new_uninitialized); callers fill
   them. The oversized-allocation reuse in hashbrown cannot trigger with
   the Rust Global allocator, which returns exactly the layout size, so
   it is not reproduced. */
static int lh_alloc_buckets(lust_hash_table *t, uint64_t buckets) {
    uint64_t ctrl_align = lh_ctrl_align(t);
    /* A capacity overflow panics; under panic=unwind the panic returns
       here and the table stays as it was. */
    if (t->elem_size && buckets > UINT64_MAX / t->elem_size) return lust_hash_capacity_overflow(), 0;
    uint64_t data_bytes = t->elem_size * buckets;
    if (data_bytes > UINT64_MAX - (ctrl_align - 1)) return lust_hash_capacity_overflow(), 0;
    uint64_t ctrl_offset = lh_align_up(data_bytes, ctrl_align);
    if (ctrl_offset > UINT64_MAX - buckets - LH_WIDTH) return lust_hash_capacity_overflow(), 0;
    uint64_t total = ctrl_offset + buckets + LH_WIDTH;
    if (total > (uint64_t)INT64_MAX - (ctrl_align - 1)) return lust_hash_capacity_overflow(), 0;
    LUST_MEM_ASK(total);
    void *raw = malloc((size_t)(total + ctrl_align));
    if (!raw) lust_oom_size(total); /* hashbrown's table layout */
    LUST_MEM_NOTE(total);
    uintptr_t base = lh_align_up((uint64_t)(uintptr_t)raw, ctrl_align);
    t->alloc_base = raw;
    t->data = (uint8_t *)base;
    t->ctrl = (uint8_t *)base + ctrl_offset;
    t->bucket_mask = buckets - 1;
    t->items = 0;
    t->growth_left = lh_bucket_mask_to_capacity(buckets - 1);
    return 1;
}

static void lh_free_table(lust_hash_table *t) {
    free(t->alloc_base);
    t->alloc_base = NULL;
    t->data = NULL;
    t->ctrl = (uint8_t *)lust_hash_static_empty;
    t->bucket_mask = 0;
    t->items = 0;
    t->growth_left = 0;
}

static int lh_is_singleton(const lust_hash_table *t) { return t->bucket_mask == 0; }
static uint64_t lh_num_buckets(const lust_hash_table *t) { return t->bucket_mask + 1; }

/* RawTableInner::with_capacity into an already-initialized layout. 0
   after a capacity overflow panic (panic=unwind), with `t` empty. */
static int lh_with_capacity(lust_hash_table *t, uint64_t capacity) {
    uint64_t buckets = 0;
    if (capacity && !lh_capacity_to_buckets(capacity, t->elem_size, &buckets)) lust_hash_capacity_overflow();
    if (capacity == 0 || !buckets || !lh_alloc_buckets(t, buckets)) {
        t->ctrl = (uint8_t *)lust_hash_static_empty;
        t->data = NULL;
        t->alloc_base = NULL;
        t->bucket_mask = 0;
        t->items = 0;
        t->growth_left = 0;
        return capacity == 0;
    }
    memset(t->ctrl, LUST_HASH_EMPTY, (size_t)(buckets + LH_WIDTH));
    return 1;
}

void lust_hash_init(lust_hash_table *t, uint64_t elem_size, uint64_t elem_align) {
    t->elem_size = elem_size;
    t->elem_align = elem_align ? elem_align : 1;
    t->ctrl = (uint8_t *)lust_hash_static_empty;
    t->data = NULL;
    t->alloc_base = NULL;
    t->bucket_mask = 0;
    t->items = 0;
    t->growth_left = 0;
}

void lust_hash_init_with_capacity(lust_hash_table *t, uint64_t elem_size, uint64_t elem_align, uint64_t capacity) {
    t->elem_size = elem_size;
    t->elem_align = elem_align ? elem_align : 1;
    lh_with_capacity(t, capacity);
}

uint64_t lust_hash_len(const lust_hash_table *t) { return t->items; }
uint64_t lust_hash_capacity(const lust_hash_table *t) { return t->items + t->growth_left; }
uint64_t lust_hash_buckets(const lust_hash_table *t) { return lh_num_buckets(t); }
int lust_hash_is_empty(const lust_hash_table *t) { return t->items == 0; }

void *lust_hash_bucket(const lust_hash_table *t, uint64_t index) {
    if (index >= lh_num_buckets(t) || !lh_is_full(t->ctrl[index])) return NULL;
    return lh_slot(t, index);
}
uint8_t lust_hash_ctrl(const lust_hash_table *t, uint64_t index) { return t->ctrl[index]; }

/* RawTableInner::set_ctrl: also writes the mirrored tail byte. */
static void lh_set_ctrl(lust_hash_table *t, uint64_t index, uint8_t ctrl) {
    uint64_t index2 = ((index - LH_WIDTH) & t->bucket_mask) + LH_WIDTH;
    t->ctrl[index] = ctrl;
    t->ctrl[index2] = ctrl;
}
static void lh_set_ctrl_hash(lust_hash_table *t, uint64_t index, uint64_t hash) {
    lh_set_ctrl(t, index, lust_hash_tag(hash));
}

/* A group load plus BitMask queries. Bit i is byte i. The scan is SWAR
   over 8-byte chunks (the generic hashbrown group does the same); only
   LH_WIDTH is observable, not this choice. Little-endian loads: both
   tier 1 targets are little-endian, and hashbrown's NEON group is
   little-endian only too. */
static uint64_t lh_load64(const uint8_t *p) {
    uint64_t w;
    memcpy(&w, p, 8);
    return w;
}
static uint32_t lh_spread_bits(uint64_t spread) {
    return (uint32_t)((((spread >> 7) & 0x0101010101010101ull) * 0x0102040810204080ull) >> 56);
}
static uint64_t lh_eq_spread(uint64_t w, uint8_t tag) {
    uint64_t x = w ^ (tag * 0x0101010101010101ull);
    return (x - 0x0101010101010101ull) & ~x & 0x8080808080808080ull;
}
#if defined(__SSE2__) && !defined(__TINYC__) && LUST_HASH_GROUP_WIDTH == 16
#include <emmintrin.h>
/* hashbrown's x86_64 group: one 16-byte load and byte compares. Its masks are
   exact, where the SWAR tag test below may also flag the byte above a match
   (a candidate that eq rejects): the same lookups, inserts and order. */
static uint32_t lh_movemask_eq(__m128i g, uint8_t b) {
    return (uint32_t)_mm_movemask_epi8(_mm_cmpeq_epi8(g, _mm_set1_epi8((char)b)));
}
/* mode: 0 match_tag, 1 match_empty, 2 match_empty_or_deleted, 3 match_full */
static uint32_t lh_group_bits(const lust_hash_table *t, uint64_t pos, int mode, uint8_t tag) {
    __m128i g = _mm_loadu_si128((const __m128i *)(t->ctrl + pos));
    if (mode == 0) return lh_movemask_eq(g, tag);
    if (mode == 1) return lh_movemask_eq(g, LUST_HASH_EMPTY);
    uint32_t hi = (uint32_t)_mm_movemask_epi8(g);
    return mode == 2 ? hi : hi ^ 0xFFFFu;
}
/* All three masks a probe needs, from one load. */
static void lh_group_masks(const lust_hash_table *t, uint64_t pos, uint8_t tag, uint32_t *tag_bits,
                           uint32_t *empty_bits, uint32_t *eod_bits) {
    __m128i g = _mm_loadu_si128((const __m128i *)(t->ctrl + pos));
    *tag_bits = lh_movemask_eq(g, tag);
    *empty_bits = lh_movemask_eq(g, LUST_HASH_EMPTY);
    *eod_bits = (uint32_t)_mm_movemask_epi8(g);
}
#else
/* mode: 0 match_tag, 1 match_empty, 2 match_empty_or_deleted, 3 match_full */
static uint32_t lh_group_bits(const lust_hash_table *t, uint64_t pos, int mode, uint8_t tag) {
    uint32_t bits = 0;
    for (uint64_t off = 0; off < LH_WIDTH; off += 8) {
        uint64_t w = lh_load64(t->ctrl + pos + off);
        uint64_t spread;
        if (mode == 0) spread = lh_eq_spread(w, tag);
        else if (mode == 1) spread = lh_eq_spread(w, LUST_HASH_EMPTY);
        else if (mode == 2) spread = w & 0x8080808080808080ull;
        else spread = ~w & 0x8080808080808080ull;
        bits |= lh_spread_bits(spread) << off;
    }
    return bits;
}
/* All three masks a probe needs, from one load per chunk. */
static void lh_group_masks(const lust_hash_table *t, uint64_t pos, uint8_t tag, uint32_t *tag_bits,
                           uint32_t *empty_bits, uint32_t *eod_bits) {
    uint32_t tb = 0, eb = 0, db = 0;
    for (uint64_t off = 0; off < LH_WIDTH; off += 8) {
        uint64_t w = lh_load64(t->ctrl + pos + off);
        tb |= lh_spread_bits(lh_eq_spread(w, tag)) << off;
        eb |= lh_spread_bits(lh_eq_spread(w, LUST_HASH_EMPTY)) << off;
        db |= lh_spread_bits(w & 0x8080808080808080ull) << off;
    }
    *tag_bits = tb;
    *empty_bits = eb;
    *eod_bits = db;
}
#endif
static int lh_lowest_bit(uint32_t bits, uint64_t *out) {
    if (!bits) return 0;
#ifndef __TINYC__
    *out = (uint64_t)__builtin_ctz(bits);
#else
    uint64_t i = 0;
    while (!(bits & 1u)) { bits >>= 1; i++; }
    *out = i;
#endif
    return 1;
}

/* ProbeSeq: triangular probe by groups. */
typedef struct {
    uint64_t pos;
    uint64_t stride;
} lh_probe;
static lh_probe lh_probe_seq(const lust_hash_table *t, uint64_t hash) {
    lh_probe p;
    p.pos = hash & t->bucket_mask; /* h1 */
    p.stride = 0;
    return p;
}
static void lh_probe_next(lh_probe *p, uint64_t bucket_mask) {
    p->stride += LH_WIDTH;
    p->pos = (p->pos + p->stride) & bucket_mask;
}

/* RawTableInner::fix_insert_index. */
static uint64_t lh_fix_insert_index(const lust_hash_table *t, uint64_t index) {
    if (lh_is_full(t->ctrl[index])) {
        uint64_t bit = 0;
        lh_lowest_bit(lh_group_bits(t, 0, 2, 0), &bit);
        index = bit;
    }
    return index;
}

/* RawTableInner::find_insert_index_in_group; -1 style via return 0. */
static int lh_find_insert_index_in_group(const lust_hash_table *t, uint32_t empty_or_deleted, const lh_probe *p,
                                         uint64_t *out) {
    uint64_t bit;
    if (!lh_lowest_bit(empty_or_deleted, &bit)) return 0;
    *out = (p->pos + bit) & t->bucket_mask;
    return 1;
}

/* RawTableInner::find_insert_index. */
static uint64_t lh_find_insert_index(const lust_hash_table *t, uint64_t hash) {
    lh_probe p = lh_probe_seq(t, hash);
    for (;;) {
        uint32_t eod = lh_group_bits(t, p.pos, 2, 0);
        uint64_t index;
        if (lh_find_insert_index_in_group(t, eod, &p, &index)) return lh_fix_insert_index(t, index);
        lh_probe_next(&p, t->bucket_mask);
    }
}

/* RawTableInner::find_inner with `eq` over elements. */
static int lh_find_inner(const lust_hash_table *t, uint64_t hash, lust_hash_eq_fn eq, void *eq_ctx, uint64_t *out) {
    uint8_t tag = lust_hash_tag(hash);
    lh_probe p = lh_probe_seq(t, hash);
    for (;;) {
        uint32_t m, empty_bits, eod_bits;
        lh_group_masks(t, p.pos, tag, &m, &empty_bits, &eod_bits);
        (void)eod_bits;
        while (m) {
            uint64_t bit;
            lh_lowest_bit(m, &bit);
            m &= m - 1;
            uint64_t index = (p.pos + bit) & t->bucket_mask;
            if (eq(eq_ctx, lh_slot(t, index))) {
                *out = index;
                return 1;
            }
        }
        if (empty_bits) return 0;
        lh_probe_next(&p, t->bucket_mask);
    }
}

/* RawTableInner::find_or_find_insert_index_inner.
   Returns 1 with a found index, or 0 with an insert index. */
static int lh_find_or_insert_inner(const lust_hash_table *t, uint64_t hash, lust_hash_eq_fn eq, void *eq_ctx,
                                   uint64_t *out) {
    int have_insert = 0;
    uint64_t insert_index = 0;
    uint8_t tag = lust_hash_tag(hash);
    lh_probe p = lh_probe_seq(t, hash);
    for (;;) {
        uint32_t m, empty_bits, eod_bits;
        lh_group_masks(t, p.pos, tag, &m, &empty_bits, &eod_bits);
        while (m) {
            uint64_t bit;
            lh_lowest_bit(m, &bit);
            m &= m - 1;
            uint64_t index = (p.pos + bit) & t->bucket_mask;
            if (eq(eq_ctx, lh_slot(t, index))) {
                *out = index;
                return 1;
            }
        }
        if (!have_insert) {
            have_insert = lh_find_insert_index_in_group(t, eod_bits, &p, &insert_index);
        }
        if (have_insert && empty_bits) {
            *out = lh_fix_insert_index(t, insert_index);
            return 0;
        }
        lh_probe_next(&p, t->bucket_mask);
    }
}

/* RawTableInner::prepare_insert_index: find, set the tag, report the old. */
static uint64_t lh_prepare_insert_index(lust_hash_table *t, uint64_t hash, uint8_t *out_old) {
    uint64_t index = lh_find_insert_index(t, hash);
    *out_old = t->ctrl[index];
    lh_set_ctrl_hash(t, index, hash);
    return index;
}

/* RawTableInner::erase (no element drop). The EMPTY vs DELETED choice:
   DELETED when the WIDTH buckets ending just after this one contain no
   EMPTY, measured as leading zeros of the group before plus trailing
   zeros of the group at this bucket in hashbrown's BitMask terms. */
static void lh_erase(lust_hash_table *t, uint64_t index) {
    uint64_t index_before = (index - LH_WIDTH) & t->bucket_mask;
    uint32_t eb = lh_group_bits(t, index_before, 1, 0);
    uint32_t ea = lh_group_bits(t, index, 1, 0);
    uint64_t leading = LH_WIDTH, trailing = LH_WIDTH;
    if (eb) {
        uint64_t hb = 0;
        for (uint64_t i = 0; i < LH_WIDTH; i++)
            if (eb & (1u << i)) hb = i;
        leading = LH_WIDTH - 1 - hb;
    }
    if (ea) lh_lowest_bit(ea, &trailing);
    uint8_t ctrl;
    if (leading + trailing >= LH_WIDTH) {
        ctrl = LUST_HASH_DELETED;
    } else {
        t->growth_left += 1;
        ctrl = LUST_HASH_EMPTY;
    }
    lh_set_ctrl(t, index, ctrl);
    t->items -= 1;
}

/* RawTableInner::clear_no_drop. */
static void lh_clear_no_drop(lust_hash_table *t) {
    if (!lh_is_singleton(t)) memset(t->ctrl, LUST_HASH_EMPTY, (size_t)(lh_num_buckets(t) + LH_WIDTH));
    t->items = 0;
    t->growth_left = lh_bucket_mask_to_capacity(t->bucket_mask);
}

/* Drops every element in bucket order, when drop != NULL. */
static void lh_drop_elements(lust_hash_table *t, lust_hash_drop_fn drop, void *ctx) {
    if (!drop || t->items == 0) return;
    uint64_t left = t->items;
    for (uint64_t i = 0; i < lh_num_buckets(t) && left; i++) {
        if (lh_is_full(t->ctrl[i])) {
            drop(ctx, lh_slot(t, i));
            left--;
        }
    }
}

/* RawTableInner::prepare_rehash_in_place: full -> DELETED, special ->
   EMPTY, in the first buckets bytes only, then fix the mirrored tail. */
static void lh_prepare_rehash_in_place(lust_hash_table *t) {
    uint64_t buckets = lh_num_buckets(t);
    for (uint64_t i = 0; i < buckets; i++)
        t->ctrl[i] = lh_is_full(t->ctrl[i]) ? LUST_HASH_DELETED : LUST_HASH_EMPTY;
    if (buckets < LH_WIDTH) memcpy(t->ctrl + LH_WIDTH, t->ctrl, (size_t)buckets);
    else memcpy(t->ctrl + buckets, t->ctrl, (size_t)LH_WIDTH);
}

/* RawTableInner::is_in_same_group. */
static int lh_is_in_same_group(const lust_hash_table *t, uint64_t i, uint64_t new_i, uint64_t hash) {
    uint64_t pos0 = hash & t->bucket_mask;
    uint64_t a = ((i - pos0) & t->bucket_mask) / LH_WIDTH;
    uint64_t b = ((new_i - pos0) & t->bucket_mask) / LH_WIDTH;
    return a == b;
}

/* RawTableInner::rehash_in_place. The scope guard for a panicking hasher
   has no C counterpart: hash_of must not fail. */
static void lh_rehash_in_place(lust_hash_table *t, lust_hash_of_fn hash_of, void *hash_ctx) {
    lh_prepare_rehash_in_place(t);
    uint64_t buckets = lh_num_buckets(t);
    for (uint64_t i = 0; i < buckets; i++) {
        if (t->ctrl[i] != LUST_HASH_DELETED) continue;
        for (;;) {
            uint64_t hash = hash_of(hash_ctx, lh_slot(t, i));
            uint64_t new_i = lh_find_insert_index(t, hash);
            if (lh_is_in_same_group(t, i, new_i, hash)) {
                lh_set_ctrl_hash(t, i, hash);
                break;
            }
            uint8_t prev = t->ctrl[new_i];
            lh_set_ctrl_hash(t, new_i, hash);
            if (prev == LUST_HASH_EMPTY) {
                lh_set_ctrl(t, i, LUST_HASH_EMPTY);
                lust_copy_small(lh_slot(t, new_i), lh_slot(t, i), t->elem_size);
                break;
            }
            /* prev is DELETED: swap the two elements and retry bucket i. */
            uint8_t *a = (uint8_t *)lh_slot(t, i);
            uint8_t *b = (uint8_t *)lh_slot(t, new_i);
            for (uint64_t k = 0; k < t->elem_size; k++) {
                uint8_t tmp = a[k];
                a[k] = b[k];
                b[k] = tmp;
            }
        }
    }
    t->growth_left = lh_bucket_mask_to_capacity(t->bucket_mask) - t->items;
}

/* RawTableInner::resize_inner: build a new table, move elements in old
   bucket order, swap, free the old allocation. */
static void lh_resize_inner(lust_hash_table *t, uint64_t capacity, lust_hash_of_fn hash_of, void *hash_ctx) {
    lust_hash_table nt;
    nt.elem_size = t->elem_size;
    nt.elem_align = t->elem_align;
    if (!lh_with_capacity(&nt, capacity)) return;
    if (t->items) {
        uint64_t left = t->items;
        for (uint64_t i = 0; i < lh_num_buckets(t) && left; i++) {
            if (!lh_is_full(t->ctrl[i])) continue;
            left--;
            uint64_t hash = hash_of(hash_ctx, lh_slot(t, i));
            uint8_t old;
            uint64_t ni = lh_prepare_insert_index(&nt, hash, &old);
            (void)old;
            lust_copy_small(lh_slot(&nt, ni), lh_slot(t, i), t->elem_size);
        }
    }
    nt.growth_left -= t->items;
    nt.items = t->items;
    lust_hash_table oldt = *t;
    *t = nt;
    free(oldt.alloc_base);
}

/* RawTableInner::reserve_rehash_inner. */
static void lh_reserve_rehash_inner(lust_hash_table *t, uint64_t additional, lust_hash_of_fn hash_of, void *hash_ctx) {
    if (additional > UINT64_MAX - t->items) {
        lust_hash_capacity_overflow();
        return;
    }
    uint64_t new_items = t->items + additional;
    uint64_t full_capacity = lh_bucket_mask_to_capacity(t->bucket_mask);
    if (new_items <= full_capacity / 2) {
        lh_rehash_in_place(t, hash_of, hash_ctx);
    } else {
        uint64_t cap = new_items > full_capacity + 1 ? new_items : full_capacity + 1;
        lh_resize_inner(t, cap, hash_of, hash_ctx);
    }
}

void lust_hash_reserve(lust_hash_table *t, uint64_t additional, lust_hash_of_fn hash_of, void *hash_ctx) {
    if (additional > t->growth_left) lh_reserve_rehash_inner(t, additional, hash_of, hash_ctx);
}

void lust_hash_shrink_to(lust_hash_table *t, uint64_t min_capacity, lust_hash_of_fn hash_of, void *hash_ctx) {
    uint64_t min_size = t->items > min_capacity ? t->items : min_capacity;
    if (min_size == 0) {
        uint64_t es = t->elem_size, ea = t->elem_align;
        lh_free_table(t);
        t->elem_size = es;
        t->elem_align = ea;
        return;
    }
    uint64_t min_buckets;
    if (!lh_capacity_to_buckets(min_size, t->elem_size, &min_buckets)) return;
    if (min_buckets < lh_num_buckets(t)) {
        if (t->items == 0) {
            lust_hash_table nt;
            nt.elem_size = t->elem_size;
            nt.elem_align = t->elem_align;
            lh_with_capacity(&nt, min_size);
            free(t->alloc_base);
            *t = nt;
        } else {
            lh_resize_inner(t, min_size, hash_of, hash_ctx);
        }
    }
}

void *lust_hash_find(const lust_hash_table *t, uint64_t hash, lust_hash_eq_fn eq, void *eq_ctx, uint64_t *out_index) {
    uint64_t index;
    if (!lh_find_inner(t, hash, eq, eq_ctx, &index)) return NULL;
    if (out_index) *out_index = index;
    return lh_slot(t, index);
}

/* RawTable::insert_at_index / record_item_insert_at. */
void *lust_hash_insert_at(lust_hash_table *t, uint64_t hash, uint64_t index) {
    uint8_t old = t->ctrl[index];
    if (old == LUST_HASH_EMPTY) t->growth_left -= 1;
    lh_set_ctrl(t, index, lust_hash_tag(hash));
    t->items += 1;
    return lh_slot(t, index);
}

int lust_hash_find_or_insert(lust_hash_table *t, uint64_t hash, lust_hash_eq_fn eq, void *eq_ctx,
                             lust_hash_of_fn hash_of, void *hash_ctx, void **out_slot, uint64_t *out_index) {
    lust_hash_reserve(t, 1, hash_of, hash_ctx);
    uint64_t index;
    if (lh_find_or_insert_inner(t, hash, eq, eq_ctx, &index)) {
        if (out_slot) *out_slot = lh_slot(t, index);
        if (out_index) *out_index = index;
        return 1;
    }
    if (out_index) *out_index = index;
    return 0;
}

void *lust_hash_insert_unique_slot(lust_hash_table *t, uint64_t hash, lust_hash_of_fn hash_of, void *hash_ctx,
                                   uint64_t *out_index) {
    uint64_t index = lh_find_insert_index(t, hash);
    uint8_t old = t->ctrl[index];
    if (t->growth_left == 0 && old == LUST_HASH_EMPTY) {
        lust_hash_reserve(t, 1, hash_of, hash_ctx);
        index = lh_find_insert_index(t, hash);
    }
    void *slot = lust_hash_insert_at(t, hash, index);
    if (out_index) *out_index = index;
    return slot;
}

void *lust_hash_insert_no_grow_slot(lust_hash_table *t, uint64_t hash, uint64_t *out_index) {
    uint8_t old;
    uint64_t index = lh_prepare_insert_index(t, hash, &old);
    if (old == LUST_HASH_EMPTY) t->growth_left -= 1;
    t->items += 1;
    if (out_index) *out_index = index;
    return lh_slot(t, index);
}

void lust_hash_erase_at(lust_hash_table *t, uint64_t index, void *out_elem) {
    if (out_elem && t->elem_size) lust_copy_small(out_elem, lh_slot(t, index), t->elem_size);
    lh_erase(t, index);
}

uint64_t lust_hash_index_of(const lust_hash_table *t, const void *slot) {
    if (t->elem_size) return (uint64_t)((const uint8_t *)slot - t->data) / t->elem_size;
    for (uint64_t i = 0; i < lh_num_buckets(t); i++)
        if (lh_is_full(t->ctrl[i])) return i;
    return 0;
}

int lust_hash_remove(lust_hash_table *t, uint64_t hash, lust_hash_eq_fn eq, void *eq_ctx, void *out_elem) {
    uint64_t index;
    if (!lh_find_inner(t, hash, eq, eq_ctx, &index)) return 0;
    lust_hash_erase_at(t, index, out_elem);
    return 1;
}

void lust_hash_clear(lust_hash_table *t, lust_hash_drop_fn drop, void *ctx) {
    if (t->items == 0) return; /* RawTable::clear keeps tombstones when empty. */
    lh_drop_elements(t, drop, ctx);
    lh_clear_no_drop(t);
}

void lust_hash_drop_table(lust_hash_table *t, lust_hash_drop_fn drop, void *ctx) {
    uint64_t es = t->elem_size, ea = t->elem_align;
    lh_drop_elements(t, drop, ctx);
    lh_free_table(t);
    t->elem_size = es;
    t->elem_align = ea;
}

void lust_hash_retain(lust_hash_table *t, lust_hash_keep_fn keep, void *keep_ctx, lust_hash_drop_fn drop, void *drop_ctx) {
    uint64_t left = t->items;
    for (uint64_t i = 0; i < lh_num_buckets(t) && left; i++) {
        if (!lh_is_full(t->ctrl[i])) continue;
        left--;
        void *slot = lh_slot(t, i);
        if (!keep(keep_ctx, slot)) {
            lh_erase(t, i);
            if (drop) drop(drop_ctx, slot);
        }
    }
}

void lust_hash_clone_into(lust_hash_table *dst, const lust_hash_table *src, lust_hash_clone_fn clone_elem, void *ctx) {
    if (lh_is_singleton(src)) return;
    uint64_t buckets = lh_num_buckets(src);
    lh_alloc_buckets(dst, buckets);
    memcpy(dst->ctrl, src->ctrl, (size_t)(buckets + LH_WIDTH));
    dst->items = src->items;
    dst->growth_left = src->growth_left;
    uint64_t left = src->items;
    for (uint64_t i = 0; i < buckets && left; i++) {
        if (!lh_is_full(src->ctrl[i])) continue;
        left--;
        if (clone_elem) clone_elem(ctx, lh_slot(dst, i), lh_slot(src, i));
        else memcpy(lh_slot(dst, i), lh_slot(src, i), (size_t)src->elem_size);
    }
}

void lust_hash_iter_init(lust_hash_iter *it, const lust_hash_table *t) {
    it->t = t;
    it->next_index = 0;
    it->group_base = 0;
    it->group_bits = 0;
    it->left = t->items;
}
/* Group-at-a-time scan like RawIterRange: aligned groups, bits low to
   high, bounded by the item count (so tail bytes of small tables and
   the singleton never produce phantom items). */
void *lust_hash_iter_next(lust_hash_iter *it, uint64_t *out_index) {
    if (!it->left) return NULL;
    while (!it->group_bits) {
        it->group_base = it->next_index;
        it->group_bits = lh_group_bits(it->t, it->group_base, 3, 0);
        it->next_index += LH_WIDTH;
    }
    uint64_t bit;
    lh_lowest_bit(it->group_bits, &bit);
    it->group_bits &= it->group_bits - 1;
    it->left--;
    uint64_t i = it->group_base + bit;
    if (out_index) *out_index = i;
    return lh_slot(it->t, i);
}

void lust_hash_drain_init(lust_hash_drain *d, lust_hash_table *t) {
    d->t = t;
    d->next_index = 0;
    d->group_base = 0;
    d->group_bits = 0;
    d->left = t->items;
}
void *lust_hash_drain_next(lust_hash_drain *d, uint64_t *out_index) {
    if (!d->left) return NULL;
    while (!d->group_bits) {
        d->group_base = d->next_index;
        d->group_bits = lh_group_bits(d->t, d->group_base, 3, 0);
        d->next_index += LH_WIDTH;
    }
    uint64_t bit;
    lh_lowest_bit(d->group_bits, &bit);
    d->group_bits &= d->group_bits - 1;
    d->left--;
    uint64_t i = d->group_base + bit;
    if (out_index) *out_index = i;
    return lh_slot(d->t, i);
}
void lust_hash_drain_finish(lust_hash_drain *d, lust_hash_drop_fn drop, void *ctx) {
    if (drop) {
        void *slot;
        while ((slot = lust_hash_drain_next(d, NULL)) != NULL) drop(ctx, slot);
    }
    lh_clear_no_drop(d->t);
    d->left = 0;
    d->group_bits = 0;
}

/* ---------------- FxHasher (rustc-hash 2.1.2, 64-bit) ---------------- */

#define LUST_FX_K 0xf1357aea2e62a9c5ull
#define LUST_FX_SEED1 0x243f6a8885a308d3ull
#define LUST_FX_SEED2 0x13198a2e03707344ull
#define LUST_FX_PREVENT_ZERO 0xa4093822299f31d0ull

/* ---------------- SipHash-1-3 (core::hash::sip) ---------------- */

static uint64_t lh_rotl(uint64_t x, int b) { return (x << b) | (x >> (64 - b)); }

/* sip.rs compress! */
static void lh_sip_round(lust_sip13 *s) {
    s->v0 += s->v1; s->v1 = lh_rotl(s->v1, 13); s->v1 ^= s->v0; s->v0 = lh_rotl(s->v0, 32);
    s->v2 += s->v3; s->v3 = lh_rotl(s->v3, 16); s->v3 ^= s->v2;
    s->v0 += s->v3; s->v3 = lh_rotl(s->v3, 21); s->v3 ^= s->v0;
    s->v2 += s->v1; s->v1 = lh_rotl(s->v1, 17); s->v1 ^= s->v2; s->v2 = lh_rotl(s->v2, 32);
}

/* u8to64_le: `len` (< 8) bytes at `p` as a little-endian word. */
static uint64_t lh_u8to64_le(const uint8_t *p, uint64_t len) {
    uint64_t out = 0;
    for (uint64_t i = 0; i < len; i++) out |= (uint64_t)p[i] << (8 * i);
    return out;
}

lust_sip13 lust_sip13_new_with_keys(uint64_t k0, uint64_t k1) {
    lust_sip13 h;
    h.k0 = k0;
    h.k1 = k1;
    h.length = 0;
    h.v0 = k0 ^ 0x736f6d6570736575ull;
    h.v1 = k1 ^ 0x646f72616e646f6dull;
    h.v2 = k0 ^ 0x6c7967656e657261ull;
    h.v3 = k1 ^ 0x7465646279746573ull;
    h.tail = 0;
    h.ntail = 0;
    return h;
}

/* sip.rs Hasher::write. */
void lust_sip13_write(lust_sip13 *h, const void *bytes, uint64_t length) {
    const uint8_t *msg = (const uint8_t *)bytes;
    h->length += length;
    uint64_t needed = 0;
    if (h->ntail != 0) {
        needed = 8 - h->ntail;
        h->tail |= lh_u8to64_le(msg, length < needed ? length : needed) << (8 * h->ntail);
        if (length < needed) {
            h->ntail += length;
            return;
        }
        h->v3 ^= h->tail;
        lh_sip_round(h);
        h->v0 ^= h->tail;
        h->ntail = 0;
    }
    uint64_t len = length - needed;
    uint64_t left = len & 7;
    uint64_t i = needed;
    while (i < len - left) {
        uint64_t mi = lh_u8to64_le(msg + i, 8);
        h->v3 ^= mi;
        lh_sip_round(h);
        h->v0 ^= mi;
        i += 8;
    }
    h->tail = lh_u8to64_le(msg + i, left);
    h->ntail = left;
}

/* sip.rs Hasher::finish: 1 compression round per block, 3 to finish. */
uint64_t lust_sip13_finish(const lust_sip13 *h) {
    lust_sip13 s = *h;
    uint64_t b = ((s.length & 0xff) << 56) | s.tail;
    s.v3 ^= b;
    lh_sip_round(&s);
    s.v0 ^= b;
    s.v2 ^= 0xff;
    lh_sip_round(&s);
    lh_sip_round(&s);
    lh_sip_round(&s);
    return s.v0 ^ s.v1 ^ s.v2 ^ s.v3;
}

lust_random_state lust_random_state_new(void) {
    static uint64_t k0 = 0x6c75737473697031ull;
    lust_random_state r;
    r.k0 = k0++;
    r.k1 = 0x6861736862726f77ull;
    return r;
}

lust_fx_hasher lust_fx_new(void) {
    lust_fx_hasher h;
    h.hash = 0;
    return h;
}
lust_fx_hasher lust_fx_with_seed(uint64_t seed) {
    lust_fx_hasher h;
    h.hash = seed;
    return h;
}
static void lust_fx_add(lust_fx_hasher *h, uint64_t i) { h->hash = (h->hash + i) * LUST_FX_K; }

/* multiply_mix: the full 64x64->128 product, low half XOR high half.
   Done with 32-bit parts so tcc (no __int128) gets the same bits. */
static uint64_t lust_fx_multiply_mix(uint64_t x, uint64_t y) {
    uint64_t lo = x * y;
    uint64_t xlo = x & 0xFFFFFFFFull, xhi = x >> 32;
    uint64_t ylo = y & 0xFFFFFFFFull, yhi = y >> 32;
    uint64_t ll = xlo * ylo, lh = xlo * yhi, hl = xhi * ylo, hh = xhi * yhi;
    uint64_t cross = (ll >> 32) + (lh & 0xFFFFFFFFull) + (hl & 0xFFFFFFFFull);
    uint64_t hi = hh + (lh >> 32) + (hl >> 32) + (cross >> 32);
    return lo ^ hi;
}

static uint64_t lust_fx_read_le64(const uint8_t *p) {
    uint64_t v = 0;
    for (int i = 7; i >= 0; i--) v = (v << 8) | p[i];
    return v;
}
static uint64_t lust_fx_read_le32(const uint8_t *p) {
    return (uint64_t)p[0] | ((uint64_t)p[1] << 8) | ((uint64_t)p[2] << 16) | ((uint64_t)p[3] << 24);
}

uint64_t lust_fx_hash_bytes(const void *bytes, uint64_t len) {
    const uint8_t *b = (const uint8_t *)bytes;
    uint64_t s0 = LUST_FX_SEED1, s1 = LUST_FX_SEED2;
    if (len <= 16) {
        if (len >= 8) {
            s0 ^= lust_fx_read_le64(b);
            s1 ^= lust_fx_read_le64(b + len - 8);
        } else if (len >= 4) {
            s0 ^= lust_fx_read_le32(b);
            s1 ^= lust_fx_read_le32(b + len - 4);
        } else if (len > 0) {
            s0 ^= b[0];
            s1 ^= ((uint64_t)b[len - 1] << 8) | b[len / 2];
        }
    } else {
        uint64_t bulk_len = len - 1;
        uint64_t i = 0;
        while (bulk_len - i >= 16) {
            uint64_t x = lust_fx_read_le64(b + i);
            uint64_t y = lust_fx_read_le64(b + i + 8);
            uint64_t t = lust_fx_multiply_mix(s0 ^ x, LUST_FX_PREVENT_ZERO ^ y);
            s0 = s1;
            s1 = t;
            i += 16;
        }
        s0 ^= lust_fx_read_le64(b + len - 16);
        s1 ^= lust_fx_read_le64(b + len - 8);
    }
    return lust_fx_multiply_mix(s0, s1) ^ len;
}

void lust_fx_write(lust_fx_hasher *h, const void *bytes, uint64_t len) {
    lust_fx_write_u64(h, lust_fx_hash_bytes(bytes, len));
}
void lust_fx_write_u8(lust_fx_hasher *h, uint8_t v) { lust_fx_add(h, v); }
void lust_fx_write_u16(lust_fx_hasher *h, uint16_t v) { lust_fx_add(h, v); }
void lust_fx_write_u32(lust_fx_hasher *h, uint32_t v) { lust_fx_add(h, v); }
void lust_fx_write_u64(lust_fx_hasher *h, uint64_t v) { lust_fx_add(h, v); }
void lust_fx_write_u128(lust_fx_hasher *h, uint64_t lo, uint64_t hi) {
    lust_fx_add(h, lo);
    lust_fx_add(h, hi);
}
void lust_fx_write_usize(lust_fx_hasher *h, uint64_t v) { lust_fx_add(h, v); }
void lust_fx_write_i8(lust_fx_hasher *h, int8_t v) { lust_fx_write_u8(h, (uint8_t)v); }
void lust_fx_write_i16(lust_fx_hasher *h, int16_t v) { lust_fx_write_u16(h, (uint16_t)v); }
void lust_fx_write_i32(lust_fx_hasher *h, int32_t v) { lust_fx_write_u32(h, (uint32_t)v); }
void lust_fx_write_i64(lust_fx_hasher *h, int64_t v) { lust_fx_write_u64(h, (uint64_t)v); }
void lust_fx_write_i128(lust_fx_hasher *h, uint64_t lo, uint64_t hi) { lust_fx_write_u128(h, lo, hi); }
void lust_fx_write_isize(lust_fx_hasher *h, int64_t v) { lust_fx_write_usize(h, (uint64_t)v); }
void lust_fx_write_length_prefix(lust_fx_hasher *h, uint64_t len) { lust_fx_write_usize(h, len); }
void lust_fx_write_str(lust_fx_hasher *h, const void *bytes, uint64_t len) {
    lust_fx_write(h, bytes, len);
    lust_fx_write_u8(h, 0xFF);
}
uint64_t lust_fx_finish(const lust_fx_hasher *h) { return (h->hash << 26) | (h->hash >> 38); }

uint64_t lust_fx_hash_u32(uint32_t v) {
    lust_fx_hasher h = lust_fx_new();
    lust_fx_write_u32(&h, v);
    return lust_fx_finish(&h);
}
uint64_t lust_fx_hash_u64(uint64_t v) {
    lust_fx_hasher h = lust_fx_new();
    lust_fx_write_u64(&h, v);
    return lust_fx_finish(&h);
}
uint64_t lust_fx_hash_usize(uint64_t v) {
    lust_fx_hasher h = lust_fx_new();
    lust_fx_write_usize(&h, v);
    return lust_fx_finish(&h);
}
uint64_t lust_fx_hash_str(const void *bytes, uint64_t len) {
    lust_fx_hasher h = lust_fx_new();
    lust_fx_write_str(&h, bytes, len);
    return lust_fx_finish(&h);
}

/* ---------------- IndexMap core (indexmap 2.14.0) ----------------
   A port of indexmap's inner::Core: a Vec of Bucket{hash, key, value}
   plus a hashbrown HashTable<usize> side index. Strategy choices that
   leave different side-table state (erase_indices, retain rebuild,
   entries capacity jumps) follow inner.rs; choices that only change how
   stored index values are found (increment/decrement_indices bulk vs
   per-entry updates) collapse to one scan here because the result is the
   same set of values in the same buckets. */

static void li_oob(void) {
    fputs("indexmap core: index out of bounds\n", stderr);
    abort();
}

/* Entry field offsets: fields ordered by alignment (largest first), in
   declaration order on ties, like the Rust layout of Bucket. Only the
   entry byte size can ever differ from a future lust layout engine;
   entry counts (what capacity() reports) do not depend on it. */
static void li_layout(lust_index_table *t, uint64_t ks, uint64_t ka, uint64_t vs, uint64_t va) {
    uint64_t sizes[3] = {8, ks, vs};
    uint64_t aligns[3] = {8, ka ? ka : 1, va ? va : 1};
    int order[3] = {0, 1, 2};
    for (int i = 0; i < 3; i++)
        for (int j = i + 1; j < 3; j++)
            if (aligns[order[j]] > aligns[order[i]]) {
                int tmp = order[i];
                order[i] = order[j];
                order[j] = tmp;
            }
    uint64_t offsets[3] = {0, 0, 0};
    uint64_t end = 0, max_align = 1;
    for (int i = 0; i < 3; i++) {
        int f = order[i];
        end = lh_align_up(end, aligns[f]);
        offsets[f] = end;
        end += sizes[f];
        if (aligns[f] > max_align) max_align = aligns[f];
    }
    t->hash_offset = offsets[0];
    t->key_offset = offsets[1];
    t->value_offset = offsets[2];
    t->key_size = ks;
    t->value_size = vs;
    t->entry_align = max_align;
    t->entry_size = lh_align_up(end, max_align);
}

static uint8_t *li_entry(const lust_index_table *t, uint64_t i) { return t->entries + i * t->entry_size; }
static void *li_key_ptr(const lust_index_table *t, uint64_t i) { return li_entry(t, i) + t->key_offset; }
static void *li_value_ptr(const lust_index_table *t, uint64_t i) { return li_entry(t, i) + t->value_offset; }
static uint64_t li_entry_hash_at(const lust_index_table *t, uint64_t i) {
    uint64_t h;
    memcpy(&h, li_entry(t, i) + t->hash_offset, 8);
    return h;
}
static void li_entry_set_hash(lust_index_table *t, uint64_t i, uint64_t h) {
    memcpy(li_entry(t, i) + t->hash_offset, &h, 8);
}
static uint64_t li_max_entries(const lust_index_table *t) { return (uint64_t)INT64_MAX / t->entry_size; }

/* The entries Vec, with Rust RawVec capacity rules for the paths
   indexmap uses: exact reserve, amortized reserve, exact shrink. */
static void li_entries_alloc(lust_index_table *t, uint64_t new_cap) {
    void *raw = NULL;
    uint8_t *base = NULL;
    if (new_cap) {
        if (new_cap > UINT64_MAX / t->entry_size) {
            lust_hash_capacity_overflow();
            return;
        }
        uint64_t bytes = new_cap * t->entry_size;
        LUST_MEM_ASK(bytes);
        raw = malloc((size_t)(bytes + t->entry_align));
        if (!raw) lust_oom_size(bytes); /* indexmap's entries Vec */
        LUST_MEM_NOTE(bytes);
        base = (uint8_t *)lh_align_up((uint64_t)(uintptr_t)raw, t->entry_align);
        if (t->len) memcpy(base, t->entries, (size_t)(t->len * t->entry_size));
    }
    free(t->entries_alloc);
    t->entries_alloc = raw;
    t->entries = base;
    t->entries_cap = new_cap;
}
static int li_entries_try_reserve_exact(lust_index_table *t, uint64_t additional) {
    if (additional <= t->entries_cap - t->len) return 1;
    if (additional > UINT64_MAX - t->len) return 0;
    uint64_t needed = t->len + additional;
    if (needed > li_max_entries(t)) return 0;
    li_entries_alloc(t, needed);
    return 1;
}
static void li_entries_reserve_exact(lust_index_table *t, uint64_t additional) {
    if (!li_entries_try_reserve_exact(t, additional)) lust_hash_capacity_overflow();
}
static void li_entries_shrink_to(lust_index_table *t, uint64_t min_capacity) {
    if (t->entries_cap > min_capacity) {
        uint64_t target = t->len > min_capacity ? t->len : min_capacity;
        if (target < t->entries_cap) li_entries_alloc(t, target);
    }
}

/* Core::reserve_entries: jump the entries capacity to the side table
   capacity when that is more than asked for, else reserve exactly. */
static void li_reserve_entries(lust_index_table *t, uint64_t additional) {
    uint64_t icap = lust_hash_capacity(&t->indices);
    uint64_t try_cap = icap < li_max_entries(t) ? icap : li_max_entries(t);
    uint64_t try_add = try_cap - t->len;
    if (try_add > additional && li_entries_try_reserve_exact(t, try_add)) return;
    li_entries_reserve_exact(t, additional);
}

/* Side-table callbacks. The hash of a stored entry index sits in that
   entry. Modes cover the two rehash windows where stored indices and
   entry positions disagree:
   0: stored i -> entry i
   1: shift_insert: stored i -> entry (i < a ? i : i - 1)
   2: erase bulk rebuild: stored i (new position) -> entry (i < a ? i : i + b) */
typedef struct {
    lust_index_table *t;
    int mode;
    uint64_t a, b;
} li_hash_ctx;
static uint64_t li_hash_of(void *vctx, const void *elem) {
    li_hash_ctx *c = (li_hash_ctx *)vctx;
    uint64_t i;
    memcpy(&i, elem, 8);
    if (c->mode == 1) i = i < c->a ? i : i - 1;
    else if (c->mode == 2) i = i < c->a ? i : i + c->b;
    return li_entry_hash_at(c->t, i);
}
typedef struct {
    lust_index_table *t;
    lust_index_eq_fn eq;
    void *eq_ctx;
} li_eq_ctx;
static int li_eq_by_key(void *vctx, const void *elem) {
    li_eq_ctx *c = (li_eq_ctx *)vctx;
    uint64_t i;
    memcpy(&i, elem, 8);
    return c->eq(c->eq_ctx, li_key_ptr(c->t, i));
}
static int li_eq_by_index(void *vctx, const void *elem) {
    uint64_t want = *(uint64_t *)vctx;
    uint64_t i;
    memcpy(&i, elem, 8);
    return i == want;
}

/* inner::erase_index / update_index over the side table. */
static void li_erase_index(lust_index_table *t, uint64_t hash, uint64_t pos) {
    uint64_t bucket;
    if (lust_hash_find(&t->indices, hash, li_eq_by_index, &pos, &bucket)) lust_hash_erase_at(&t->indices, bucket, NULL);
}
static void li_update_index(lust_index_table *t, uint64_t hash, uint64_t old_pos, uint64_t new_pos) {
    void *slot = lust_hash_find(&t->indices, hash, li_eq_by_index, &old_pos, NULL);
    if (!slot) abort(); /* "index not found" in Rust */
    memcpy(slot, &new_pos, 8);
}

/* Mutates every stored index in [start, end) by delta (+1 or -1). */
static void li_shift_stored(lust_index_table *t, uint64_t start, uint64_t end, int delta) {
    lust_hash_iter it;
    lust_hash_iter_init(&it, &t->indices);
    void *slot;
    while ((slot = lust_hash_iter_next(&it, NULL)) != NULL) {
        uint64_t v;
        memcpy(&v, slot, 8);
        if (v >= start && v < end) {
            v = delta > 0 ? v + 1 : v - 1;
            memcpy(slot, &v, 8);
        }
    }
}

/* inner::insert_bulk_no_grow over one run of entries: inserts entry
   old positions [first_old, first_old+count) with their hashes, storing
   new positions first_new... in order. The caller guarantees room, so
   the hasher (mode-2 adjusted when rebuilding) is never rehashed with,
   exactly the unreachable hasher in indexmap. */
static void li_insert_bulk(lust_index_table *t, uint64_t first_old, uint64_t count, uint64_t first_new,
                           li_hash_ctx *hc) {
    for (uint64_t j = 0; j < count; j++) {
        uint64_t hash = li_entry_hash_at(t, first_old + j);
        void *slot = lust_hash_insert_unique_slot(&t->indices, hash, li_hash_of, hc, NULL);
        uint64_t v = first_new + j;
        memcpy(slot, &v, 8);
    }
}

void lust_index_rebuild(lust_index_table *t) {
    lust_hash_clear(&t->indices, NULL, NULL);
    li_hash_ctx hc = {t, 0, 0, 0};
    li_insert_bulk(t, 0, t->len, 0, &hc);
}

/* inner::Core::erase_indices: rewrites the side table for removing the
   entry range [start, end) before the entries themselves move. */
typedef struct {
    uint64_t start, end, offset;
} li_erase_retain_ctx;
static int li_erase_retain_keep(void *vctx, void *elem) {
    li_erase_retain_ctx *c = (li_erase_retain_ctx *)vctx;
    uint64_t v;
    memcpy(&v, elem, 8);
    if (v >= c->end) {
        v -= c->offset;
        memcpy(elem, &v, 8);
        return 1;
    }
    return v < c->start;
}
static void li_erase_indices(lust_index_table *t, uint64_t start, uint64_t end) {
    uint64_t erased = end - start;
    uint64_t shifted = t->len - end;
    uint64_t half = lust_hash_capacity(&t->indices) / 2;
    if (erased == 0) return;
    if (start + shifted < half && start < erased) {
        lust_hash_clear(&t->indices, NULL, NULL);
        li_hash_ctx hc = {t, 2, start, erased};
        li_insert_bulk(t, 0, start, 0, &hc);
        li_insert_bulk(t, end, shifted, start, &hc);
    } else if (erased + shifted < half) {
        for (uint64_t i = start; i < end; i++) li_erase_index(t, li_entry_hash_at(t, i), i);
        for (uint64_t j = 0; j < shifted; j++)
            li_update_index(t, li_entry_hash_at(t, end + j), end + j, start + j);
    } else {
        li_erase_retain_ctx c = {start, end, erased};
        lust_hash_retain(&t->indices, li_erase_retain_keep, &c, NULL, NULL);
    }
}

void lust_index_init(lust_index_table *t, uint64_t key_size, uint64_t key_align, uint64_t value_size,
                     uint64_t value_align) {
    li_layout(t, key_size, key_align, value_size, value_align);
    lust_hash_init(&t->indices, 8, 8);
    t->entries = NULL;
    t->entries_alloc = NULL;
    t->len = 0;
    t->entries_cap = 0;
}
void lust_index_init_with_capacity(lust_index_table *t, uint64_t key_size, uint64_t key_align, uint64_t value_size,
                                   uint64_t value_align, uint64_t capacity) {
    li_layout(t, key_size, key_align, value_size, value_align);
    lust_hash_init_with_capacity(&t->indices, 8, 8, capacity);
    t->entries = NULL;
    t->entries_alloc = NULL;
    t->len = 0;
    t->entries_cap = 0;
    if (capacity) li_entries_alloc(t, capacity);
}

static void li_drop_one(lust_index_table *t, uint64_t i, lust_index_drop_fn drop, void *ctx) {
    if (drop) drop(ctx, li_key_ptr(t, i), li_value_ptr(t, i));
}

void lust_index_drop_table(lust_index_table *t, lust_index_drop_fn drop, void *ctx) {
    for (uint64_t i = 0; i < t->len; i++) li_drop_one(t, i, drop, ctx);
    free(t->entries_alloc);
    t->entries_alloc = NULL;
    t->entries = NULL;
    t->len = 0;
    t->entries_cap = 0;
    lust_hash_drop_table(&t->indices, NULL, NULL);
}

uint64_t lust_index_len(const lust_index_table *t) { return t->len; }
uint64_t lust_index_capacity(const lust_index_table *t) {
    uint64_t ic = lust_hash_capacity(&t->indices);
    return ic < t->entries_cap ? ic : t->entries_cap;
}
int lust_index_is_empty(const lust_index_table *t) { return t->len == 0; }

int lust_index_get_index(const lust_index_table *t, uint64_t index, void **out_key, void **out_value) {
    if (index >= t->len) return 0;
    if (out_key) *out_key = li_key_ptr(t, index);
    if (out_value) *out_value = li_value_ptr(t, index);
    return 1;
}
void *lust_index_entry_mut(lust_index_table *t, uint64_t index) {
    if (index >= t->len) return NULL;
    return li_entry(t, index);
}
uint64_t lust_index_entry_hash(const lust_index_table *t, uint64_t index) { return li_entry_hash_at(t, index); }

uint64_t lust_index_get_index_of(const lust_index_table *t, uint64_t hash, lust_index_eq_fn eq, void *eq_ctx) {
    li_eq_ctx c = {(lust_index_table *)t, eq, eq_ctx};
    void *slot = lust_hash_find(&t->indices, hash, li_eq_by_key, &c, NULL);
    if (!slot) return UINT64_MAX;
    uint64_t v;
    memcpy(&v, slot, 8);
    return v;
}
int lust_index_get(const lust_index_table *t, uint64_t hash, lust_index_eq_fn eq, void *eq_ctx, void **out_key,
                   void **out_value, uint64_t *out_index) {
    uint64_t i = lust_index_get_index_of(t, hash, eq, eq_ctx);
    if (i == UINT64_MAX) return 0;
    if (out_key) *out_key = li_key_ptr(t, i);
    if (out_value) *out_value = li_value_ptr(t, i);
    if (out_index) *out_index = i;
    return 1;
}

/* Core::push_entry split in two: make room only when full, then claim
   the entry slot at the current len and set its hash word. */
static void li_push_entry_slot(lust_index_table *t, uint64_t hash, void **out_key, void **out_value) {
    if (t->len == t->entries_cap) li_reserve_entries(t, 1);
    uint64_t i = t->len++;
    li_entry_set_hash(t, i, hash);
    if (out_key) *out_key = li_key_ptr(t, i);
    if (out_value) *out_value = li_value_ptr(t, i);
}

uint64_t lust_index_insert_full(lust_index_table *t, uint64_t hash, lust_index_eq_fn eq, void *eq_ctx, int *out_replaced,
                                void **out_key_slot, void **out_value_slot) {
    li_eq_ctx ec = {t, eq, eq_ctx};
    li_hash_ctx hc = {t, 0, 0, 0};
    void *slot;
    uint64_t bidx;
    if (lust_hash_find_or_insert(&t->indices, hash, li_eq_by_key, &ec, li_hash_of, &hc, &slot, &bidx)) {
        uint64_t pos;
        memcpy(&pos, slot, 8);
        *out_replaced = 1;
        if (out_key_slot) *out_key_slot = li_key_ptr(t, pos);
        if (out_value_slot) *out_value_slot = li_value_ptr(t, pos);
        return pos;
    }
    *out_replaced = 0;
    uint64_t pos = t->len;
    void *islot = lust_hash_insert_at(&t->indices, hash, bidx);
    memcpy(islot, &pos, 8);
    li_push_entry_slot(t, hash, out_key_slot, out_value_slot);
    return pos;
}

uint64_t lust_index_insert_unique(lust_index_table *t, uint64_t hash, void **out_key_slot, void **out_value_slot) {
    uint64_t pos = lust_hash_len(&t->indices);
    li_hash_ctx hc = {t, 0, 0, 0};
    void *islot = lust_hash_insert_unique_slot(&t->indices, hash, li_hash_of, &hc, NULL);
    memcpy(islot, &pos, 8);
    li_push_entry_slot(t, hash, out_key_slot, out_value_slot);
    return pos;
}

void lust_index_shift_insert_unique(lust_index_table *t, uint64_t index, uint64_t hash, void **out_key_slot,
                                    void **out_value_slot) {
    uint64_t end = lust_hash_len(&t->indices);
    if (index > end) li_oob();
    li_shift_stored(t, index, end, +1);
    li_hash_ctx hc = {t, 1, index, 0};
    void *islot = lust_hash_insert_unique_slot(&t->indices, hash, li_hash_of, &hc, NULL);
    memcpy(islot, &index, 8);
    if (t->len == t->entries_cap) li_reserve_entries(t, 1);
    memmove(li_entry(t, index + 1), li_entry(t, index), (size_t)((t->len - index) * t->entry_size));
    t->len++;
    li_entry_set_hash(t, index, hash);
    if (out_key_slot) *out_key_slot = li_key_ptr(t, index);
    if (out_value_slot) *out_value_slot = li_value_ptr(t, index);
}

static void li_copy_out(lust_index_table *t, uint64_t i, void *out_key, void *out_value) {
    if (out_key && t->key_size) memcpy(out_key, li_key_ptr(t, i), (size_t)t->key_size);
    if (out_value && t->value_size) memcpy(out_value, li_value_ptr(t, i), (size_t)t->value_size);
}

static void li_swap_remove_finish(lust_index_table *t, uint64_t index, void *out_key, void *out_value) {
    li_copy_out(t, index, out_key, out_value);
    uint64_t last = t->len - 1;
    if (index != last) memcpy(li_entry(t, index), li_entry(t, last), (size_t)t->entry_size);
    t->len--;
    if (index < t->len) li_update_index(t, li_entry_hash_at(t, index), t->len, index);
}
static void li_shift_remove_finish(lust_index_table *t, uint64_t index, void *out_key, void *out_value) {
    li_shift_stored(t, index + 1, t->len, -1);
    li_copy_out(t, index, out_key, out_value);
    memmove(li_entry(t, index), li_entry(t, index + 1), (size_t)((t->len - index - 1) * t->entry_size));
    t->len--;
}

/* OccupiedEntry::remove on the side table by key; returns the position. */
static int li_remove_from_indices_by_key(lust_index_table *t, uint64_t hash, lust_index_eq_fn eq, void *eq_ctx,
                                         uint64_t *out_pos) {
    li_eq_ctx ec = {t, eq, eq_ctx};
    uint64_t bucket;
    void *slot = lust_hash_find(&t->indices, hash, li_eq_by_key, &ec, &bucket);
    if (!slot) return 0;
    memcpy(out_pos, slot, 8);
    lust_hash_erase_at(&t->indices, bucket, NULL);
    return 1;
}

int lust_index_swap_remove(lust_index_table *t, uint64_t hash, lust_index_eq_fn eq, void *eq_ctx, void *out_key,
                           void *out_value, uint64_t *out_index) {
    uint64_t pos;
    if (!li_remove_from_indices_by_key(t, hash, eq, eq_ctx, &pos)) return 0;
    li_swap_remove_finish(t, pos, out_key, out_value);
    if (out_index) *out_index = pos;
    return 1;
}
int lust_index_shift_remove(lust_index_table *t, uint64_t hash, lust_index_eq_fn eq, void *eq_ctx, void *out_key,
                            void *out_value, uint64_t *out_index) {
    uint64_t pos;
    if (!li_remove_from_indices_by_key(t, hash, eq, eq_ctx, &pos)) return 0;
    li_shift_remove_finish(t, pos, out_key, out_value);
    if (out_index) *out_index = pos;
    return 1;
}
int lust_index_swap_remove_index(lust_index_table *t, uint64_t index, void *out_key, void *out_value) {
    if (index >= t->len) return 0;
    li_erase_index(t, li_entry_hash_at(t, index), index);
    li_swap_remove_finish(t, index, out_key, out_value);
    return 1;
}
int lust_index_shift_remove_index(lust_index_table *t, uint64_t index, void *out_key, void *out_value) {
    if (index >= t->len) return 0;
    li_erase_index(t, li_entry_hash_at(t, index), index);
    li_shift_remove_finish(t, index, out_key, out_value);
    return 1;
}
int lust_index_pop(lust_index_table *t, void *out_key, void *out_value) {
    if (t->len == 0) return 0;
    uint64_t last = t->len - 1;
    uint64_t hash = li_entry_hash_at(t, last);
    li_copy_out(t, last, out_key, out_value);
    t->len--;
    li_erase_index(t, hash, t->len);
    return 1;
}

void lust_index_reserve(lust_index_table *t, uint64_t additional) {
    li_hash_ctx hc = {t, 0, 0, 0};
    lust_hash_reserve(&t->indices, additional, li_hash_of, &hc);
    if (additional > t->entries_cap - t->len) li_reserve_entries(t, additional);
}
void lust_index_reserve_exact(lust_index_table *t, uint64_t additional) {
    li_hash_ctx hc = {t, 0, 0, 0};
    lust_hash_reserve(&t->indices, additional, li_hash_of, &hc);
    li_entries_reserve_exact(t, additional);
}
void lust_index_shrink_to(lust_index_table *t, uint64_t min_capacity) {
    li_hash_ctx hc = {t, 0, 0, 0};
    lust_hash_shrink_to(&t->indices, min_capacity, li_hash_of, &hc);
    li_entries_shrink_to(t, min_capacity);
}
void lust_index_clear(lust_index_table *t, lust_index_drop_fn drop, void *ctx) {
    lust_hash_clear(&t->indices, NULL, NULL);
    for (uint64_t i = 0; i < t->len; i++) li_drop_one(t, i, drop, ctx);
    t->len = 0;
}
void lust_index_truncate(lust_index_table *t, uint64_t len, lust_index_drop_fn drop, void *ctx) {
    if (len >= t->len) return;
    li_erase_indices(t, len, t->len);
    for (uint64_t i = len; i < t->len; i++) li_drop_one(t, i, drop, ctx);
    t->len = len;
}
void lust_index_drain_range(lust_index_table *t, uint64_t start, uint64_t end, lust_index_drop_fn drop, void *ctx) {
    if (start > end || end > t->len) li_oob();
    li_erase_indices(t, start, end);
    for (uint64_t i = start; i < end; i++) li_drop_one(t, i, drop, ctx);
    memmove(li_entry(t, start), li_entry(t, end), (size_t)((t->len - end) * t->entry_size));
    t->len -= end - start;
}
void lust_index_retain(lust_index_table *t, lust_index_keep_fn keep, void *keep_ctx, lust_index_drop_fn drop,
                       void *drop_ctx) {
    uint64_t w = 0;
    for (uint64_t r = 0; r < t->len; r++) {
        if (keep(keep_ctx, li_key_ptr(t, r), li_value_ptr(t, r))) {
            if (w != r) memcpy(li_entry(t, w), li_entry(t, r), (size_t)t->entry_size);
            w++;
        } else {
            li_drop_one(t, r, drop, drop_ctx);
        }
    }
    int removed = w < t->len;
    t->len = w;
    if (removed && t->len < lust_hash_len(&t->indices)) lust_index_rebuild(t);
}
void lust_index_reverse(lust_index_table *t) {
    for (uint64_t i = 0; i < t->len / 2; i++) {
        uint8_t *a = li_entry(t, i);
        uint8_t *b = li_entry(t, t->len - 1 - i);
        for (uint64_t k = 0; k < t->entry_size; k++) {
            uint8_t tmp = a[k];
            a[k] = b[k];
            b[k] = tmp;
        }
    }
    lust_hash_iter it;
    lust_hash_iter_init(&it, &t->indices);
    void *slot;
    while ((slot = lust_hash_iter_next(&it, NULL)) != NULL) {
        uint64_t v;
        memcpy(&v, slot, 8);
        v = t->len - v - 1;
        memcpy(slot, &v, 8);
    }
}
void lust_index_swap_indices(lust_index_table *t, uint64_t a, uint64_t b) {
    if (a == b && a < t->len) return;
    if (a >= t->len || b >= t->len) li_oob();
    uint64_t ha = li_entry_hash_at(t, a), hb = li_entry_hash_at(t, b);
    li_update_index(t, ha, a, b);
    li_update_index(t, hb, b, a);
    uint8_t *ea = li_entry(t, a);
    uint8_t *eb = li_entry(t, b);
    for (uint64_t k = 0; k < t->entry_size; k++) {
        uint8_t tmp = ea[k];
        ea[k] = eb[k];
        eb[k] = tmp;
    }
}
void lust_index_move_index(lust_index_table *t, uint64_t from, uint64_t to) {
    if (from >= t->len || to >= t->len) li_oob();
    if (from == to) return;
    uint64_t hash = li_entry_hash_at(t, from);
    uint8_t *tmp = (uint8_t *)malloc((size_t)t->entry_size);
    if (!tmp) lust_oom();
    memcpy(tmp, li_entry(t, from), (size_t)t->entry_size);
    if (from < to) {
        li_shift_stored(t, from + 1, to + 1, -1);
        memmove(li_entry(t, from), li_entry(t, from + 1), (size_t)((to - from) * t->entry_size));
    } else {
        li_shift_stored(t, to, from, +1);
        memmove(li_entry(t, to + 1), li_entry(t, to), (size_t)((from - to) * t->entry_size));
    }
    memcpy(li_entry(t, to), tmp, (size_t)t->entry_size);
    free(tmp);
    li_update_index(t, hash, from, to);
}
