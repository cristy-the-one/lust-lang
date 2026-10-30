/* ---------- Constant storage ----------
   rustc gives equal constants one address: promoted temps (`&1`),
   literals (`"ab"`, `b"ab"`), and the pointees of const and static
   values. A static itself is never shared. In one codegen unit, two
   constants share storage when their LLVM constants are equal: the same
   bytes, the same uninit runs, and pointers at the same places. The
   linker also merges 4, 8, 16 and 32 byte constants that hold no
   pointer by their bytes (an uninit byte is 0 in the object file), in
   any unit (LLVM's .rodata.cstN). Windows link.exe does not merge those
   sections, so an uninit byte stays in the key there and does not share
   with a fully init constant of the same bytes. A fully init constant
   whose only zero byte is its last one is a C string to LLVM
   (.rodata.str1.A), so it is not in that set; equal C strings still
   share one address, including on Windows (`/GF`). lust gives every constant of the program that address:
   each site interns its bytes here at its first run and keeps the
   canonical address in a slot of its own (cgen/kconst.rs), and the glue
   unit registers the pointees of statics at startup (lust_kreg, or
   lust_kreg_h with a hash codegen computed, which reads no byte of the
   pointee). Zero-sized constants never come here: their address is
   their alignment (cgen `const_zst_addr`).

   A site's layout descriptor (cgen `kdesc`) says which bytes of its
   value are init. Word 0 is the word count that follows; then ops:
     1 off n            n init bytes
     2 off              a pointer: a reloc when it is the address of a
                        constant here, else plain bytes (a dangling
                        zero-sized address, a static, a fn)
     3 off count stride sublen sub[sublen]       an array
     4 off size nvar disc[nvar] {sublen sub}*nvar
                        an enum with a tag of `size` bytes at off (a
                        16-byte tag takes two words per disc, low first)
     5 off noff nsize start first span dataful nvar {sublen sub}*nvar
                        (a 16-byte niche takes two words for start)
                        an enum whose tag is the niche scalar at off+noff:
                        value start+d is variant first+d for d < span,
                        any other value the dataful variant (layout.rs
                        `niche_tag_value`)
   Offsets in a sub-descriptor count from the element or enum start.
   A null descriptor means every byte is init. */

const void *lust_kintern(const void **cache, const void *p, uint64_t n, uint64_t align, const uint64_t *desc);
void lust_kreg(const void *p, uint64_t n, uint64_t align, const uint64_t *desc);
void lust_kreg_h(const void *p, uint64_t n, uint64_t align, uint64_t kind, uint64_t hash);

#ifdef LUST_IMPL
/* One interned constant: `kind` 1 is a mergeable 4/8/16/32 byte constant
   with no pointer that is not a C string (the key is its bytes), 0 any
   other (the key is its
   bytes, then its byte classes: 0 uninit, 1 init, 2 pointer). `align`
   is the alignment its storage guarantees. `at_addr`: the entry holds no
   copy of its key, which is the n bytes at `addr`, every one init (a
   static's pointee registered by lust_kreg_h). */
typedef struct lust_kent {
    uint64_t hash, n, align;
    const void *addr;
    uint8_t kind, at_addr;
    uint8_t key[];
} lust_kent;

static lust_kent **lust_ktab;
static uint64_t lust_ktab_cap, lust_ktab_len;
/* The addresses of the entries, to tell a reloc from plain bytes. */
static uintptr_t *lust_kaddr;
static uint64_t lust_kaddr_cap;
static int32_t lust_klock;

/* The table's hash of a key: 8 bytes per step, read little-endian, mixed so the low bits
   (the table index) see every byte. cgen/kconst.rs `khash` computes the same for the
   static pointees it registers by hash (lust_kreg_h): the two must agree. */
static uint64_t lust_khash(const uint8_t *b, uint64_t n, uint64_t h) {
    uint64_t i = 0;
    for (; i + 8 <= n; i += 8) {
        uint64_t w;
        memcpy(&w, b + i, 8);
#if defined(__BYTE_ORDER__) && __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
        w = __builtin_bswap64(w);
#endif
        h = (h ^ w) * 0x9e3779b97f4a7c15ull;
        h ^= h >> 32;
    }
    for (; i < n; i++) h = (h ^ b[i]) * 0x100000001b3ull;
    h ^= h >> 29;
    h *= 0xbf58476d1ce4e5b9ull;
    return h ^ (h >> 32);
}

static uint64_t lust_kaddr_slot(uintptr_t a) { return (uint64_t)((a >> 3) * 0x9e3779b97f4a7c15ull); }

static int lust_kis_entry(uintptr_t a) {
    if (!lust_kaddr_cap || !a) return 0;
    for (uint64_t i = lust_kaddr_slot(a) & (lust_kaddr_cap - 1);; i = (i + 1) & (lust_kaddr_cap - 1)) {
        if (lust_kaddr[i] == a) return 1;
        if (!lust_kaddr[i]) return 0;
    }
}

static void lust_kaddr_add(uintptr_t a) {
    if ((lust_ktab_len + 1) * 2 > lust_kaddr_cap) {
        uint64_t cap = lust_kaddr_cap ? lust_kaddr_cap * 2 : 64;
        uintptr_t *old = lust_kaddr;
        uint64_t old_cap = lust_kaddr_cap;
        lust_kaddr = (uintptr_t *)calloc(cap, sizeof(uintptr_t));
        if (!lust_kaddr) lust_oom();
        lust_kaddr_cap = cap;
        for (uint64_t i = 0; i < old_cap; i++)
            if (old[i]) lust_kaddr_add(old[i]);
        free(old);
    }
    uint64_t i = lust_kaddr_slot(a) & (lust_kaddr_cap - 1);
    while (lust_kaddr[i] && lust_kaddr[i] != a) i = (i + 1) & (lust_kaddr_cap - 1);
    lust_kaddr[i] = a;
}

static uint64_t lust_kread(const uint8_t *p, uint64_t size) {
    uint64_t v = 0;
    for (uint64_t i = 0; i < size && i < 8; i++) v |= (uint64_t)p[i] << (8 * i);
    return v;
}

/* Marks the byte classes of the value at `v + base` by descriptor ops
   `d[0..len)` into `cls`. */
static void lust_kwalk(const uint8_t *v, uint8_t *cls, const uint64_t *d, uint64_t len, uint64_t base) {
    uint64_t i = 0;
    while (i < len) {
        uint64_t op = d[i], off = base + d[i + 1];
        if (op == 1) {
            memset(cls + off, 1, d[i + 2]);
            i += 3;
        } else if (op == 2) {
            uintptr_t a;
            memcpy(&a, v + off, sizeof a);
            memset(cls + off, lust_kis_entry(a) ? 2 : 1, sizeof a);
            i += 2;
        } else if (op == 3) {
            uint64_t count = d[i + 2], stride = d[i + 3], sl = d[i + 4];
            for (uint64_t k = 0; k < count; k++) lust_kwalk(v, cls, d + i + 5, sl, off + k * stride);
            i += 5 + sl;
        } else {
            /* An enum: find the variant, mark its tag bytes, walk its fields. */
            uint64_t nvar, j, var = 0;
            if (op == 4) {
                /* A 16-byte tag compares two words per discriminant. */
                uint64_t size = d[i + 2], w = size == 16 ? 2 : 1;
                uint64_t lo = lust_kread(v + off, size >= 8 ? 8 : size), hi = w == 2 ? lust_kread(v + off + 8, 8) : 0;
                uint64_t mask = size >= 8 ? ~0ull : (1ull << (8 * size)) - 1;
                nvar = d[i + 3];
                for (uint64_t k = 0; k < nvar; k++)
                    if ((d[i + 4 + w * k] & mask) == lo && (w == 1 || d[i + 5 + w * k] == hi)) var = k;
                memset(cls + off, 1, size);
                j = i + 4 + w * nvar;
            } else {
                /* A 16-byte niche: `start` is two words, the distance is
                   taken over 128 bits. */
                uint64_t noff = d[i + 2], nsize = d[i + 3], w = nsize == 16 ? 2 : 1;
                uint64_t first = d[i + 4 + w], span = d[i + 5 + w], dataful = d[i + 6 + w];
                uint64_t mask = nsize >= 8 ? ~0ull : (1ull << (8 * nsize)) - 1;
                uint64_t lo = lust_kread(v + off + noff, nsize >= 8 ? 8 : nsize), rel = (lo - d[i + 4]) & mask, rhi = 0;
                if (w == 2) rhi = lust_kread(v + off + noff + 8, 8) - d[i + 5] - (lo < d[i + 4]);
                nvar = d[i + 7 + w];
                var = rhi == 0 && rel < span ? first + rel : dataful;
                if (var != dataful) memset(cls + off + noff, 1, nsize);
                j = i + 8 + w;
            }
            for (uint64_t k = 0; k < nvar; k++) {
                uint64_t sl = d[j];
                if (k == var) lust_kwalk(v, cls, d + j + 1, sl, off);
                j += 1 + sl;
            }
            i = j;
        }
    }
}

/* The key of the `n` bytes at `p` into `key` (2n bytes); returns the kind.
   `key[0..n)` are the bytes with uninit ones 0; for kind 0, `key[n..2n)`
   are the classes. A C string (`cstr`: all init, one zero byte, at the
   end) is kind 0, as LLVM puts it in a string section (getKindForGlobal).
   As rustc (const_alloc_to_llvm), a run of bytes
   between pointers with more than 16 init and uninit chunks is all init. */
static uint8_t lust_kkey(const void *p, uint64_t n, const uint64_t *desc, uint8_t *key) {
    uint8_t *cls = key + n;
    memcpy(key, p, n);
    if (!desc) {
        /* Every byte init and no pointer (most static pointees): a C string when its one
           zero byte is the last; no chunk rule applies. */
        memset(cls, 1, n);
        int cstr = memchr(key, 0, n) == (const void *)(key + n - 1);
        return !cstr && (n == 4 || n == 8 || n == 16 || n == 32);
    }
    memset(cls, 0, n);
    lust_kwalk((const uint8_t *)p, cls, desc + 1, desc[0], 0);
    int reloc = 0, uninit = 0;
    uint64_t zeros = 0;
    for (uint64_t i = 0; i < n; i++) {
        if (!cls[i]) key[i] = 0, uninit = 1;
        if (cls[i] == 2) reloc = 1;
        zeros += !key[i];
    }
    int cstr = !uninit && !reloc && zeros == 1 && !key[n - 1];
    /* Kind 1 is the linker's byte merge (.rodata.cstN): an uninit byte
       is 0 in the object file. link.exe does not do that merge, so on
       Windows an uninit byte keeps kind 0 and the class bytes. A fully
       init constant is still kind 1, and a C string stays kind 0. */
#ifdef _WIN32
    if (!uninit && !reloc && !cstr && (n == 4 || n == 8 || n == 16 || n == 32)) return 1;
#else
    if (!reloc && !cstr && (n == 4 || n == 8 || n == 16 || n == 32)) return 1;
#endif
    for (uint64_t s = 0; s < n;) {
        if (cls[s] == 2) {
            s++;
            continue;
        }
        uint64_t e = s, chunks = 1;
        while (e + 1 < n && cls[e + 1] != 2) {
            if (cls[e + 1] != cls[e]) chunks++;
            e++;
        }
        if (chunks > 16) memset(cls + s, 1, e + 1 - s);
        s = e + 1;
    }
    return 0;
}

/* True when entry `e` (of size n and kind) holds the key of the n bytes `b` with init classes
   `cls` (kind 0 only; null: every byte init). */
static int lust_keq(const lust_kent *e, const uint8_t *b, const uint8_t *cls) {
    if (memcmp(e->at_addr ? (const uint8_t *)e->addr : e->key, b, e->n)) return 0;
    if (e->kind) return 1;
    const uint8_t *ec = e->at_addr ? 0 : e->key + e->n;
    if (ec && cls) return !memcmp(ec, cls, e->n);
    const uint8_t *c = ec ? ec : cls;
    for (uint64_t i = 0; c && i < e->n; i++)
        if (c[i] != 1) return 0;
    return 1;
}

/* The entry with hash h, size n and kind `kind` that fits `align` and holds the key (b, cls)
   (lust_keq), or null with `*slot` the free slot for a new one. Grows the table first. */
static lust_kent *lust_kprobe(uint64_t h, uint64_t n, uint8_t kind, uint64_t align, const uint8_t *b, const uint8_t *cls, uint64_t *slot) {
    if ((lust_ktab_len + 1) * 2 > lust_ktab_cap) {
        uint64_t cap = lust_ktab_cap ? lust_ktab_cap * 2 : 64;
        lust_kent **t = (lust_kent **)calloc(cap, sizeof *t);
        if (!t) lust_oom();
        for (uint64_t i = 0; i < lust_ktab_cap; i++) {
            lust_kent *e = lust_ktab[i];
            if (!e) continue;
            uint64_t j = e->hash & (cap - 1);
            while (t[j]) j = (j + 1) & (cap - 1);
            t[j] = e;
        }
        free(lust_ktab);
        lust_ktab = t;
        lust_ktab_cap = cap;
    }
    uint64_t i = h & (lust_ktab_cap - 1);
    for (lust_kent *e; (e = lust_ktab[i]); i = (i + 1) & (lust_ktab_cap - 1))
        if (e->hash == h && e->n == n && e->kind == kind && e->align >= align && lust_keq(e, b, cls)) return e;
    *slot = i;
    return 0;
}

/* Puts the new entry `e` in the free slot `i` (lust_kprobe). */
static void lust_kput(lust_kent *e, uint64_t i) {
    lust_ktab[i] = e;
    lust_ktab_len++;
    lust_kaddr_add((uintptr_t)e->addr);
}

/* The entry for the constant at `p`, added when there is none that fits
   `align`: a copy (`copy`) or `p` itself (a static's pointee). */
static const void *lust_kfind(const void *p, uint64_t n, uint64_t align, const uint64_t *desc, int copy) {
    uint8_t small[256];
    uint8_t *key = 2 * n <= sizeof small ? small : (uint8_t *)malloc(2 * n);
    if (!key) lust_oom();
    uint8_t kind = lust_kkey(p, n, desc, key);
    uint64_t klen = kind ? n : 2 * n;
    uint64_t h = lust_khash(key, klen, 0xcbf29ce484222325ull ^ n ^ ((uint64_t)kind << 63));
    uint64_t i;
    lust_kent *e = lust_kprobe(h, n, kind, align, key, key + n, &i);
    if (e) {
        if (key != small) free(key);
        return e->addr;
    }
    e = (lust_kent *)malloc(sizeof *e + klen);
    if (!e) lust_oom();
    e->hash = h;
    e->n = n;
    e->kind = kind;
    e->at_addr = 0;
    memcpy(e->key, key, klen);
    if (copy) {
        /* At least 16-aligned, so a later site of a stricter type (up to
           16) finds this copy, as rustc raises a shared constant's
           alignment to its strictest use. */
        uint64_t al = align < 16 ? 16 : align;
#ifdef _WIN32
        void *c = _aligned_malloc((size_t)((n + al - 1) / al * al), (size_t)al);
#else
        void *c = aligned_alloc(al, (n + al - 1) / al * al);
#endif
        if (!c) lust_oom_size(n);
        memcpy(c, key, n);
        e->addr = c;
        e->align = al;
    } else {
        e->addr = p;
        e->align = align;
    }
    if (key != small) free(key);
    lust_kput(e, i);
    return e->addr;
}

/* A spin-wait hint: lets the core that holds the lock run, and stops the
   wait loop from filling the pipeline with loads. */
static void lust_kpause(void) {
#if !defined(__TINYC__) && (defined(__x86_64__) || defined(__i386__))
    __builtin_ia32_pause();
#elif !defined(__TINYC__) && defined(__aarch64__)
    __asm__ __volatile__("yield");
#endif
}

/* Takes the table lock, or returns 0 without it as soon as `*cache` (when
   given) holds an address, which another thread published for the same
   site. A waiter reads the lock word with a pause between reads (test and
   test-and-set): an exchange in a loop would take the cache line from the
   holder on every try. */
static int lust_klock_take(const void **cache, const void **r) {
    int32_t one = 1, old;
    /* Each load goes into a local: tcc's __atomic_load does not write through a result
       pointer that is not the address of a variable. */
    const void *v;
    for (;;) {
        if (cache) {
            __atomic_load(cache, &v, 2);
            if (v) return *r = v, 0;
        }
        __atomic_exchange(&lust_klock, &one, &old, 2);
        if (!old) return 1;
        do {
            lust_kpause();
            if (cache) {
                __atomic_load(cache, &v, 2);
                if (v) return *r = v, 0;
            }
            __atomic_load(&lust_klock, &old, 0);
        } while (old);
    }
}

static void lust_klock_give(void) {
    int32_t zero = 0;
    __atomic_store(&lust_klock, &zero, 3);
}
#endif

/* The canonical address of the `n` bytes at `p`
   (alignment `align`, descriptor `desc`), stored into `*cache`. Threads
   that run a site for the first time together all come here: one interns,
   and the others return its address once it is in the site's slot, without
   taking the lock. */
const void *lust_kintern(const void **cache, const void *p, uint64_t n, uint64_t align, const uint64_t *desc) LUST_BODY({
    const void *r;
    if (!lust_klock_take(cache, &r)) return r;
    __atomic_load(cache, &r, 0);
    if (!r) {
        r = lust_kfind(p, n, align, desc, 1);
        __atomic_store(cache, &r, 3);
    }
    lust_klock_give();
    return r;
})

/* A static's pointee of `n` bytes at `p`, as the canonical storage of
   its content (the glue unit's lust_kstatics_init, before main). */
void lust_kreg(const void *p, uint64_t n, uint64_t align, const uint64_t *desc) LUST_BODY({
    (void)lust_klock_take(0, 0);
    (void)lust_kfind(p, n, align, desc, 0);
    lust_klock_give();
})

/* As lust_kreg for a pointee whose bytes are all init and hold no pointer, by the kind and
   hash of its key that codegen computed (cgen/kconst.rs `khash`): the entry keeps no copy of
   the key, and its bytes are read only to compare with an entry of an equal hash. */
void lust_kreg_h(const void *p, uint64_t n, uint64_t align, uint64_t kind, uint64_t hash) LUST_BODY({
    (void)lust_klock_take(0, 0);
    uint64_t i;
    if (!lust_kprobe(hash, n, (uint8_t)kind, align, (const uint8_t *)p, 0, &i)) {
        lust_kent *e = (lust_kent *)malloc(sizeof *e);
        if (!e) lust_oom();
        e->hash = hash;
        e->n = n;
        e->align = align;
        e->addr = p;
        e->kind = (uint8_t)kind;
        e->at_addr = 1;
        lust_kput(e, i);
    }
    lust_klock_give();
})
