// String interning. Identifiers become `Sym` (a u32) during lexing, so later stages compare integers.
// The interner is sharded by hash, so many threads can lex and parse at once.
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::sync::{Mutex, OnceLock};

/// FxHash (from rustc). Much faster than SipHash for short keys; no DoS resistance needed here.
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            self.add(u64::from_le_bytes(*w));
        }
        for &b in rest {
            self.add(b as u64);
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

impl FxHasher {
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

pub type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub type FxSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sym(pub u32);

/// The source name of a def name that may be a resolver flat name
/// (`a$b$Name`, or `k<hash>$a$Name` for a source dependency; see
/// `resolve::worklist::flat_name`): the last `$` segment, which is the
/// name as written. Plain names pass through. This is the name rustc
/// prints in derived `Debug` output. Display only; never a lookup key.
pub fn source_name(name: &str) -> &str {
    name.rsplit('$').next().unwrap_or(name)
}

/// The source path of a def name that may be a resolver flat name, for
/// diagnostics: `a$b$Name` reads `a::b::Name`. A leading
/// dependency-crate hash segment (`k` plus 16 hex digits) is dropped,
/// because the flat name does not carry the crate name;
/// `ResolvedTables::display_names` holds the full path (with the crate
/// name) when the resolver ran. Display only; never a lookup key.
pub fn source_path(name: &str) -> String {
    let mut segs: Vec<&str> = name.split('$').collect();
    if segs.len() > 1 && is_dep_prefix(segs[0]) {
        segs.remove(0);
    }
    segs.join("::")
}

/// True for the stable-hash prefix segment of a source dependency's
/// flat names (`k` plus 16 hex digits).
fn is_dep_prefix(seg: &str) -> bool {
    seg.len() == 17 && seg.starts_with('k') && seg[1..].chars().all(|c| c.is_ascii_hexdigit())
}

const SHARD_BITS: u32 = 6;
const SHARDS: usize = 1 << SHARD_BITS;
/// Entries in a shard's first bucket; bucket `b` holds `FIRST << b` entries.
const FIRST: usize = 64;
const BUCKETS: usize = 32;

/// One shard. `map` (behind the lock) finds a string's index on intern.
/// `buckets` hold the strings by index for `get`, which takes no lock:
/// a bucket and each entry in it are written once, under the lock and
/// before their index is handed out, and never move or change after.
/// Checkers on many threads call `get` constantly, so a lock there
/// serialized them.
#[derive(Default)]
struct Shard {
    map: Mutex<FxMap<Box<str>, u32>>,
    buckets: [OnceLock<Box<[OnceLock<Box<str>>]>>; BUCKETS],
}

/// The bucket and offset of entry `i` in a shard: bucket `b` starts at
/// `FIRST * (2^b - 1)`.
fn slot(i: usize) -> (usize, usize) {
    let b = (usize::BITS - 1 - (i / FIRST + 1).leading_zeros()) as usize;
    (b, i - FIRST * ((1 << b) - 1))
}

/// Strings the parser and the resolver intern by literal on hot paths
/// (`_` patterns, path keywords): interned first, and then answered from
/// `Interner::fixed` without a lock. Before, every thread of a parallel
/// parse or rewrite took the same few shard locks for them.
const FIXED: [&str; 8] = ["_", "::", "crate", "self", "super", "Self", "doc", "macro_rules"];

/// A `Sym` packs `(index in shard << SHARD_BITS) | shard`.
pub struct Interner {
    shards: Vec<Shard>,
    fixed: [Sym; FIXED.len()],
}

impl Default for Interner {
    fn default() -> Interner {
        let mut i = Interner { shards: (0..SHARDS).map(|_| Shard::default()).collect(), fixed: [Sym(0); FIXED.len()] };
        for (k, s) in FIXED.iter().enumerate() {
            i.fixed[k] = i.intern_locked(s);
        }
        i
    }
}

impl Interner {
    pub fn intern(&self, s: &str) -> Sym {
        if s.len() <= 11
            && let Some(k) = FIXED.iter().position(|f| *f == s)
        {
            return self.fixed[k];
        }
        self.intern_locked(s)
    }

    /// The shard that holds `s`.
    fn shard_of(s: &str) -> usize {
        let mut h = FxHasher::default();
        s.hash(&mut h);
        (h.finish() >> 58) as usize & (SHARDS - 1)
    }

    /// `intern` through the shard lock.
    fn intern_locked(&self, s: &str) -> Sym {
        let shard = Self::shard_of(s);
        let sh = &self.shards[shard];
        let mut map = sh.map.lock().unwrap();
        if let Some(&i) = map.get(s) {
            return Sym((i << SHARD_BITS) | shard as u32);
        }
        let i = map.len();
        let (b, off) = slot(i);
        let bucket = sh.buckets[b].get_or_init(|| (0..FIRST << b).map(|_| OnceLock::new()).collect());
        let _ = bucket[off].set(s.into());
        map.insert(s.into(), i as u32);
        Sym(((i as u32) << SHARD_BITS) | shard as u32)
    }

    /// The `Sym` of `s` when something interned it already, without
    /// interning it. A scan for names can skip the ones no source uses.
    pub fn find(&self, s: &str) -> Option<Sym> {
        if let Some(k) = FIXED.iter().position(|f| *f == s) {
            return Some(self.fixed[k]);
        }
        let shard = Self::shard_of(s);
        let i = *self.shards[shard].map.lock().unwrap().get(s)?;
        Some(Sym((i << SHARD_BITS) | shard as u32))
    }

    pub fn get(&self, sym: Sym) -> &str {
        let (b, off) = slot((sym.0 >> SHARD_BITS) as usize);
        self.shards[(sym.0 as usize) & (SHARDS - 1)].buckets[b]
            .get()
            .and_then(|bucket| bucket[off].get())
            .expect("a Sym from this interner")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every interned string reads back through `get` across bucket edges,
    /// and interning it again returns the same symbol.
    #[test]
    fn get_reads_back_across_buckets() {
        assert_eq!(slot(0), (0, 0));
        assert_eq!(slot(63), (0, 63));
        assert_eq!(slot(64), (1, 0));
        assert_eq!(slot(191), (1, 127));
        assert_eq!(slot(192), (2, 0));
        let i = Interner::default();
        let syms: Vec<Sym> = (0..20_000).map(|n| i.intern(&format!("name{n}"))).collect();
        for (n, &s) in syms.iter().enumerate() {
            assert_eq!(i.get(s), format!("name{n}"));
            assert_eq!(i.intern(&format!("name{n}")), s);
        }
        // Fixed strings answer without the lock, and agree with a locked lookup.
        for f in FIXED {
            assert_eq!(i.intern(f), i.intern_locked(f));
            assert_eq!(i.get(i.intern(f)), f);
        }
    }

    /// `find` returns the interned symbol, and None (without interning)
    /// for a string nobody interned.
    #[test]
    fn find_does_not_intern() {
        let i = Interner::default();
        let a = i.intern("NonZero");
        assert_eq!(i.find("NonZero"), Some(a));
        assert_eq!(i.find("self"), Some(i.intern("self")));
        assert_eq!(i.find("MaybeUninit"), None);
        assert_eq!(i.find("MaybeUninit"), None);
    }
}

macro_rules! known_syms {
    ($($name:ident),* $(,)?) => {
        /// Symbols the compiler looks up by name. Interned before lexing, so they always exist.
        #[allow(non_snake_case)]
        pub struct Known {
            $(pub $name: Sym,)*
            /// `self` and `Self` are keywords, so they cannot be macro field names.
            pub self_value: Sym,
            pub self_type: Sym,
        }
        impl Known {
            pub fn new(i: &Interner) -> Known {
                Known {
                    $($name: i.intern(stringify!($name)),)*
                    self_value: i.intern("self"),
                    self_type: i.intern("Self"),
                }
            }
        }
    };
}

known_syms!(
    std,
    core,
    intrinsics,
    env,
    args,
    String,
    Vec,
    Box,
    Rc,
    str,
    main,
    new,
    from,
    with_capacity,
    count,
    len,
    is_empty,
    push,
    push_str,
    as_str,
    then,
    then_some,
    to_string,
    to_owned,
    clone,
    wrapping_add,
    wrapping_sub,
    wrapping_mul,
    wrapping_div,
    wrapping_rem,
    wrapping_neg,
    wrapping_pow,
    wrapping_shl,
    wrapping_shr,
    wrapping_abs,
    checked_add,
    checked_sub,
    checked_mul,
    checked_div,
    checked_rem,
    checked_neg,
    checked_abs,
    checked_pow,
    checked_shl,
    checked_shr,
    saturating_add,
    saturating_sub,
    saturating_mul,
    saturating_div,
    saturating_neg,
    saturating_abs,
    saturating_pow,
    overflowing_add,
    overflowing_sub,
    overflowing_mul,
    overflowing_div,
    overflowing_rem,
    overflowing_neg,
    overflowing_abs,
    overflowing_pow,
    overflowing_shl,
    overflowing_shr,
    count_ones,
    count_zeros,
    leading_zeros,
    trailing_zeros,
    leading_ones,
    trailing_ones,
    rotate_left,
    rotate_right,
    swap_bytes,
    reverse_bits,
    div_ceil,
    div_euclid,
    rem_euclid,
    signum,
    is_positive,
    is_negative,
    is_power_of_two,
    is_multiple_of,
    next_multiple_of,
    checked_next_multiple_of,
    unsigned_abs,
    make_ascii_lowercase,
    make_ascii_uppercase,
    unbounded_shl,
    unbounded_shr,
    cast_signed,
    cast_unsigned,
    isqrt,
    checked_isqrt,
    bit_width,
    highest_one,
    lowest_one,
    isolate_highest_one,
    isolate_lowest_one,
    midpoint,
    abs_diff,
    strict_add,
    strict_sub,
    strict_mul,
    strict_div,
    strict_rem,
    strict_div_euclid,
    strict_rem_euclid,
    strict_neg,
    strict_abs,
    strict_pow,
    strict_shl,
    strict_shr,
    strict_add_unsigned,
    strict_sub_unsigned,
    strict_add_signed,
    strict_sub_signed,
    checked_add_unsigned,
    checked_sub_unsigned,
    checked_add_signed,
    checked_sub_signed,
    wrapping_add_unsigned,
    wrapping_sub_unsigned,
    wrapping_add_signed,
    wrapping_sub_signed,
    saturating_add_unsigned,
    saturating_sub_unsigned,
    saturating_add_signed,
    saturating_sub_signed,
    overflowing_add_unsigned,
    overflowing_sub_unsigned,
    overflowing_add_signed,
    overflowing_sub_signed,
    checked_signed_diff,
    carrying_add,
    borrowing_sub,
    carrying_mul,
    carrying_mul_add,
    sqrt,
    powi,
    powf,
    exp,
    ln,
    log,
    log2,
    log10,
    mul_add,
    floor,
    ceil,
    round,
    trunc,
    fract,
    recip,
    is_nan,
    is_infinite,
    is_finite,
    is_sign_positive,
    is_sign_negative,
    to_bits,
    from_bits,
    copysign,
    sin,
    cos,
    tan,
    asin,
    acos,
    atan,
    sinh,
    cosh,
    tanh,
    exp2,
    exp_m1,
    ln_1p,
    to_radians,
    to_degrees,
    atan2,
    hypot,
    len_utf8,
    len_utf16,
    encode_utf8,
    encode_utf16,
    is_ascii,
    is_ascii_punctuation,
    is_ascii_graphic,
    is_ascii_hexdigit,
    is_ascii_control,
    is_digit,
    abs,
    min,
    max,
    pow,
    bool,
    char,
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    i128,
    u128,
    f32,
    f64,
    println,
    print,
    eprintln,
    eprint,
    format,
    panic,
    vec,
    assert,
    assert_eq,
    assert_ne,
    todo,
    unreachable,
    format_args,
    derive,
    test,
    allow,
    warn,
    deny,
    Option,
    Some,
    None,
    is_some,
    is_none,
    unwrap,
    expect,
    unwrap_or,
    pop,
    get,
    first,
    last,
    contains,
    find,
    rfind,
    swap,
    take,
    clear,
    insert,
    remove,
    truncate,
    starts_with,
    ends_with,
    trim,
    to_uppercase,
    to_lowercase,
    chars,
    next,
    is_ascii_digit,
    is_ascii_alphabetic,
    is_ascii_alphanumeric,
    is_ascii_whitespace,
    is_ascii_uppercase,
    is_ascii_lowercase,
    is_alphabetic,
    is_numeric,
    is_alphanumeric,
    is_lowercase,
    is_uppercase,
    is_whitespace,
    is_control,
    to_ascii_uppercase,
    to_ascii_lowercase,
    to_digit,
    from_u32,
    from_digit,
    Result,
    Ok,
    Err,
    is_ok,
    is_err,
    capacity,
    reserve,
    reserve_exact,
    shrink_to_fit,
    shrink_to,
    swap_remove,
    append,
    extend_from_slice,
    split_off,
    resize,
    dedup,
    retain,
    retain_mut,
    reverse,
    first_mut,
    last_mut,
    get_mut,
    as_slice,
    as_mut_slice,
    to_vec,
    drop,
    insert_str,
    as_bytes,
    into_bytes,
    strip_prefix,
    strip_suffix,
    replace,
    repeat,
    trim_start,
    trim_end,
    is_char_boundary,
    from_utf8,
    leak,
    strong_count,
    weak_count,
    make_mut,
    try_unwrap,
    default,
    unwrap_or_default,
);

impl Known {
    /// The integer type a name refers to, if any.
    pub fn int_ty(&self, s: Sym) -> Option<crate::ty::IntTy> {
        use crate::ty::IntTy::*;
        let table = [
            (self.i8, I8),
            (self.i16, I16),
            (self.i32, I32),
            (self.i64, I64),
            (self.isize, Isize),
            (self.u8, U8),
            (self.u16, U16),
            (self.u32, U32),
            (self.u64, U64),
            (self.usize, Usize),
            (self.i128, I128),
            (self.u128, U128),
        ];
        table.iter().find(|(k, _)| *k == s).map(|&(_, t)| t)
    }
}
