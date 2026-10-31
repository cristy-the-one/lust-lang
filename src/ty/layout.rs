// The layout engine (docs/ARCH.md section 4.4). One engine computes size, align,
// field offsets, enum tag placement, and the niche for every sized type lust
// accepts. A type outside the supported-shape grammar of 4.4 returns `Err(())`;
// the caller turns that into the layout reject code. The engine never guesses a size.
//
// The core functions work on field `Layout`s, so the standalone gate test can
// build the real nested Type/TypeData field graph directly (4.4). `layout_of_ty`
// is the adapter from semantic `Ty` to those functions; it reads Adt shapes
// through the `AdtShapes` trait so this file does not depend on collect's tables.
#![allow(dead_code)] // Wired into size_of/align_of by check and into codegen by W-CGEN.

use super::hash::HashTy;
use super::iter::IterKind;
use super::sync::SyncTy;
use super::{AdtId, BuiltinTy, FloatTy, IntTy, Ty, TyIntern, TyKind};

/// A niche: a scalar range inside a type whose unused values can hold enum tags
/// of an outer type. `free` counts the usable values; they start at `start`
/// (the first value in the niche scalar's bit pattern that is not a valid value
/// of the scalar, following rustc's valid-range model).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Niche {
    pub offset: u64,
    /// Width of the niche scalar in bytes.
    pub scalar_size: u64,
    pub free: u128,
    pub start: u128,
}

impl Niche {
    /// The bit mask of the niche scalar.
    fn mask(&self) -> u128 {
        if self.scalar_size * 8 >= 128 { u128::MAX } else { (1u128 << (self.scalar_size * 8)) - 1 }
    }

    /// rustc 1.99's `Niche::reserve`: takes `count` free values next to
    /// one end of the valid range and returns the first value taken (the
    /// tag of the first niche variant; the rest count up from it) and
    /// the niche that is left. The valid range runs from `start + free`
    /// up to `start - 1`, wrapping. One value (an `Option`) goes to the
    /// side nearer zero in signed terms, a tie to the low side, except
    /// for the valid range 0..=1 (`bool`), which keeps the older rule
    /// below like a count above one.
    fn reserve(&self, count: u128) -> Option<(u128, Option<Niche>)> {
        if count == 0 || count > self.free {
            return None;
        }
        let mask = self.mask();
        let bits = (self.scalar_size * 8) as u32;
        let (vs, ve) = (self.start.wrapping_add(self.free) & mask, self.start.wrapping_sub(1) & mask);
        let left = |start| if self.free == count { None } else { Some(Niche { start, free: self.free - count, ..*self }) };
        // `move_start`: the values just below the valid range.
        let low = || Some((vs.wrapping_sub(count) & mask, left(self.start)));
        // `move_end`: the values just above it.
        let high = || Some((self.start, left(self.start.wrapping_add(count) & mask)));
        let sext_abs = |v: u128| {
            let sh = 128 - bits;
            (((v << sh) as i128) >> sh).unsigned_abs()
        };
        if count == 1 && (vs, ve) != (0, 1) {
            let next_up = sext_abs(ve.wrapping_add(1) & mask);
            let next_down = sext_abs(vs.wrapping_sub(1) & mask);
            if next_down <= next_up { low() } else { high() }
        } else if vs > ve {
            high()
        } else if vs <= mask - ve {
            if count <= vs { low() } else { high() }
        } else {
            let end = ve.wrapping_add(count) & mask;
            if (1..=ve).contains(&end) { low() } else { high() }
        }
    }
}

/// The niche scalar bit pattern that stands for variant `vi` of a
/// niche-filled enum (rustc's `TagEncoding::Niche`): `start` plus the
/// distance of `vi` from `first`, wrapping at the top of the scalar.
pub fn niche_tag_value(niche: Niche, first: usize, start: u128, vi: usize) -> u128 {
    start.wrapping_add((vi - first) as u128) & niche.mask()
}

/// How an enum stores its discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumTag {
    /// Only the named variant is inhabited: no tag is stored.
    None(usize),
    /// A tag scalar at offset 0 with this width in bytes.
    Explicit { size: u64, signed: bool },
    /// The discriminant lives in the niche of the named variant (`niche`
    /// is that variant's niche before the reserve). Variants `first..=last`
    /// map to niche values from `start` by variant index (`niche_tag_value`),
    /// so a dataful or absent variant inside the span still uses a value.
    NicheFilled { variant: usize, niche: Niche, first: usize, last: usize, start: u128 },
}

/// The layout of one sized type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: u64,
    pub align: u64,
    /// Field offsets for a struct or tuple, in declaration order. For an enum,
    /// the offsets of each variant's fields, in declaration order per variant.
    pub field_offsets: Vec<u64>,
    pub variant_offsets: Vec<Vec<u64>>,
    pub niche: Option<Niche>,
    pub enum_tag: Option<EnumTag>,
    /// Uninhabited in rustc's layout sense: no value exists (a field is
    /// uninhabited, or every enum variant is). Such a type can still have
    /// a size (`(u8, Never)`); an enum drops only absent variants.
    pub uninhabited: bool,
}

impl Layout {
    /// A plain scalar or aggregate layout with no field or tag detail.
    fn simple(size: u64, align: u64, niche: Option<Niche>) -> Layout {
        Layout { size, align, field_offsets: Vec::new(), variant_offsets: Vec::new(), niche, enum_tag: None, uninhabited: false }
    }
}

/// The `repr` of an Adt, in layout terms. The kind is the base
/// layout (Rust, C, or transparent); `pack` is the `repr(packed(N))`
/// bound (`repr(packed)` is N = 1), which caps every field's
/// alignment at N under either base layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Repr {
    pub kind: ReprKind,
    /// A fixed integer tag (`repr(u8)` and kin): width in bits and signedness.
    pub int: Option<(u32, bool)>,
    /// The integer tag is written `isize` or `usize` (not `i64`/`u64`):
    /// `mem::discriminant` hashes it as that type.
    pub word: bool,
    /// `repr(align(n))`.
    pub align: Option<u64>,
    /// `repr(packed)` or `repr(packed(N))`: the alignment bound.
    pub pack: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReprKind {
    #[default]
    Default,
    C,
    Transparent,
}

impl Repr {
    /// True when the repr fixes the enum tag, so the niche-filled candidate is off.
    fn fixes_tag(&self) -> bool {
        self.int.is_some() || self.kind != ReprKind::Default
    }

    /// The integer type that holds an enum's discriminant in rustc
    /// (`DiscriminantKind`): the `repr` integer, else `isize`.
    pub fn disc_holder(&self) -> IntTy {
        match self.int {
            None => IntTy::Isize,
            Some((_, true)) if self.word => IntTy::Isize,
            Some((_, false)) if self.word => IntTy::Usize,
            Some((bits, signed)) => IntTy::ALL
                .iter()
                .copied()
                .find(|i| i.bits() == bits && i.signed() == signed && !matches!(i, IntTy::Isize | IntTy::Usize))
                .unwrap_or(IntTy::Isize),
        }
    }
}

/// The code of a discriminant's integer type in std source's
/// `Discriminant::kind` (library/core/src/mem.rs).
pub fn disc_kind_code(it: IntTy) -> u8 {
    match it {
        IntTy::I8 => 0,
        IntTy::I16 => 1,
        IntTy::I32 => 2,
        IntTy::I64 => 3,
        IntTy::I128 => 4,
        IntTy::Isize => 5,
        IntTy::U8 => 6,
        IntTy::U16 => 7,
        IntTy::U32 => 8,
        IntTy::U64 => 9,
        IntTy::U128 => 10,
        IntTy::Usize => 11,
    }
}

fn align_up(v: u64, align: u64) -> u64 {
    if align <= 1 { v } else { v.div_ceil(align) * align }
}

/// The layout of an integer scalar.
pub fn int_layout(t: IntTy) -> Layout {
    let size = (t.bits() / 8) as u64;
    Layout::simple(size, size, None)
}

/// The layout of a float scalar.
pub fn float_layout(t: FloatTy) -> Layout {
    match t {
        FloatTy::F32 => Layout::simple(4, 4, None),
        FloatTy::F64 => Layout::simple(8, 8, None),
    }
}

/// `bool`: one byte; values 2 to 255 are free.
pub fn bool_layout() -> Layout {
    Layout::simple(1, 1, Some(Niche { offset: 0, scalar_size: 1, free: 254, start: 2 }))
}

/// `char`: four bytes; values above 0x10FFFF are free for niche purposes
/// (rustc models the valid range as 0..=0x10FFFF here).
pub fn char_layout() -> Layout {
    Layout::simple(4, 4, Some(Niche { offset: 0, scalar_size: 4, free: (1u128 << 32) - 0x110000, start: 0x110000 }))
}

/// A non-null thin pointer scalar (`&T`, `Box<T>`, a fn pointer): null is free.
pub fn ptr_layout() -> Layout {
    Layout::simple(8, 8, Some(Niche { offset: 0, scalar_size: 8, free: 1, start: 0 }))
}

/// A raw pointer: may be null, so no niche.
pub fn raw_ptr_layout() -> Layout {
    Layout::simple(8, 8, None)
}

/// A raw pointer to `str` or `[T]`: two words, no niche (tf-fatptr).
pub fn fat_raw_ptr_layout() -> Layout {
    Layout::simple(16, 8, None)
}

/// A raw pointer to `dyn Tr`: the data word may be null, the vtable
/// word may not, so it keeps that niche (rustc's `Option<*const dyn
/// Tr>` is 16 bytes).
pub fn dyn_raw_ptr_layout() -> Layout {
    Layout::simple(16, 8, Some(Niche { offset: 8, scalar_size: 8, free: 1, start: 0 }))
}

/// A fat pointer (slice, str, or dyn reference): the data word keeps the
/// non-null niche.
pub fn fat_ptr_layout() -> Layout {
    Layout::simple(16, 8, Some(Niche { offset: 0, scalar_size: 8, free: 1, start: 0 }))
}

/// A zero-sized type with the given alignment.
pub fn zst_layout(align: u64) -> Layout {
    Layout::simple(0, align.max(1), None)
}

/// The never type: zero-sized and uninhabited.
pub fn never_layout() -> Layout {
    let mut l = zst_layout(1);
    l.uninhabited = true;
    l
}

/// A `Vec`/`String` capacity word: rustc caps a capacity at
/// `isize::MAX`, so every value above it is niche space. This is the
/// niche that lets `Option<Option<Vec<T>>>` stay one word per field.
pub fn cap_layout() -> Layout {
    Layout::simple(8, 8, Some(Niche { offset: 0, scalar_size: 8, free: 1u128 << 63, start: 1u128 << 63 }))
}

/// A `NonZero` integer scalar: zero is free.
pub fn nonzero_layout(t: IntTy) -> Layout {
    let mut l = int_layout(t);
    l.niche = Some(Niche { offset: 0, scalar_size: l.size, free: 1, start: 0 });
    l
}

/// The layout of `[elem; len]`. A non-empty array keeps the element niche
/// at offset 0, as rustc does: the first element sits there.
pub fn array_layout(elem: &Layout, len: u64) -> Result<Layout, ()> {
    let size = elem.size.checked_mul(len).ok_or(())?;
    let niche = if len > 0 { elem.niche } else { None };
    let mut l = Layout::simple(size, elem.align, niche);
    l.uninhabited = len > 0 && elem.uninhabited;
    Ok(l)
}

/// Places fields in the given order with normal padding. Returns size before
/// the final round-up and the offsets in placement order.
fn place_in_order(fields: &[Layout], order: &[usize]) -> (u64, Vec<u64>) {
    let mut offsets = vec![0u64; fields.len()];
    let mut cur = 0u64;
    for &i in order {
        cur = align_up(cur, fields[i].align);
        offsets[i] = cur;
        cur += fields[i].size;
    }
    (cur, offsets)
}

/// The niche a struct-like aggregate reports: the field niche with the most
/// free values, at that field's offset; ties go to the first field in layout
/// (placement) order.
fn best_niche(fields: &[Layout], offsets: &[u64], order: &[usize]) -> Option<Niche> {
    let mut best: Option<Niche> = None;
    for &i in order {
        if let Some(n) = fields[i].niche {
            let cand = Niche { offset: offsets[i] + n.offset, ..n };
            if best.is_none_or(|b| cand.free > b.free) {
                best = Some(cand);
            }
        }
    }
    best
}

/// Which end of a univariant layout its largest niche is biased toward
/// (rustc's two-pass field ordering: enums pack other variants into the
/// larger side of the niche).
#[derive(Clone, Copy, PartialEq, Eq)]
enum NicheBias {
    Start,
    End,
}

/// Whether the last field may be unsized. Tuple types use `MaybeUnsized`:
/// their last element stays last, outside the field reordering.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UnivKind {
    AlwaysSized,
    MaybeUnsized,
}

/// rustc's alignment group key for one field: `log2` of an effective
/// alignment derived from the field's align and size, so a `[u8; 4]`
/// groups with align-4 fields and a `[u8; 6]` with align-2 fields. The
/// niche bias caps or drops the size part for niche-carrying fields.
fn alignment_group_key(f: &Layout, max_field_align: u64, largest_niche_free: u128, bias: NicheBias, packed: bool) -> u64 {
    // Under `repr(packed(N))` the group is the capped alignment
    // alone: the size part does not count, so `packed(1)` keeps
    // declaration order and a large low-align field does not lead.
    if packed {
        return f.align.trailing_zeros() as u64;
    }
    let niche_free = f.niche.map_or(0, |n| n.free);
    let mut key = f.align.max(f.size).trailing_zeros();
    if largest_niche_free > 0 {
        match bias {
            NicheBias::Start => key = key.min(max_field_align.trailing_zeros()),
            NicheBias::End if niche_free == largest_niche_free => key = f.align.trailing_zeros(),
            NicheBias::End => {}
        }
    }
    key as u64
}

/// One biased pass of rustc's univariant (repr(Rust) struct or tuple)
/// layout: stable-sort the optimizable fields by alignment group
/// (largest first), then niche placement within the group; place them
/// in that order with normal padding.
fn univariant_biased(fields: &[Layout], repr: Repr, kind: UnivKind, bias: NicheBias) -> Result<Layout, ()> {
    let mut order: Vec<usize> = (0..fields.len()).collect();
    let end = if kind == UnivKind::MaybeUnsized && !fields.is_empty() { fields.len() - 1 } else { fields.len() };
    if fields.len() > 1 {
        let max_field_align = fields[..end].iter().map(|f| f.align).max().unwrap_or(1);
        let largest_niche_free = fields[..end].iter().filter_map(|f| f.niche).map(|n| n.free).max().unwrap_or(0);
        order[..end].sort_by_key(|&x| {
            let f = &fields[x];
            let niche_free = f.niche.map_or(0, |n| n.free);
            let niche_key = match bias {
                NicheBias::Start => !niche_free,
                NicheBias::End => niche_free,
            };
            let inner_key = match bias {
                NicheBias::Start => f.niche.map_or(0, |n| n.offset),
                NicheBias::End => f.niche.map_or(0, |n| !(f.size - n.scalar_size - n.offset)),
            };
            (std::cmp::Reverse(alignment_group_key(f, max_field_align, largest_niche_free, bias, repr.pack.is_some())), niche_key, inner_key)
        });
    }
    let (end_off, offsets) = place_in_order(fields, &order);
    let align = fields.iter().map(|f| f.align).max().unwrap_or(1);
    // The largest niche, in placement order; the End bias takes the last
    // of equally large niches, the Start bias the first.
    let mut best: Option<Niche> = None;
    for &i in &order {
        if let Some(n) = fields[i].niche {
            let prefer = match bias {
                NicheBias::Start => best.is_none_or(|b| n.free > b.free),
                NicheBias::End => best.is_none_or(|b| n.free >= b.free),
            };
            if prefer {
                best = Some(Niche { offset: offsets[i] + n.offset, ..n });
            }
        }
    }
    let mut out = Layout::simple(align_up(end_off, align), align, best);
    out.field_offsets = offsets;
    out.uninhabited = fields.iter().any(|f| f.uninhabited);
    finish(out, repr)
}

/// rustc's univariant layout: a start-biased pass, then (for sized
/// types whose niche sits strictly inside) an end-biased pass, keeping
/// the layout whose niche is closer to an edge.
fn univariant_layout(fields: &[Layout], repr: Repr, kind: UnivKind) -> Result<Layout, ()> {
    let start = univariant_biased(fields, repr, kind, NicheBias::Start)?;
    if kind != UnivKind::MaybeUnsized && fields.len() > 1 {
        if let Some(n) = start.niche {
            let head = n.offset;
            let tail = start.size.saturating_sub(head + n.scalar_size);
            if head != 0 && tail > 0 {
                let alt = univariant_biased(fields, repr, kind, NicheBias::End)?;
                if let Some(an) = alt.niche
                    && an.offset > head
                    && an.offset > tail
                {
                    return Ok(alt);
                }
            }
        }
    }
    Ok(start)
}

/// The layout of a tuple type: repr(Rust) with the last element held
/// last, as rustc lays tuples out as possibly unsized.
pub fn tuple_layout(fields: &[Layout]) -> Result<Layout, ()> {
    univariant_layout(fields, Repr::default(), UnivKind::MaybeUnsized)
}

/// The layout of a struct (or one enum variant's fields) with the given
/// field layouts and repr. Tuple types use `tuple_layout` instead.
pub fn struct_layout(fields: &[Layout], repr: Repr) -> Result<Layout, ()> {
    // `repr(packed(N))`: every field's effective alignment is capped
    // at N, then the base layout (C order or Rust reorder) runs as
    // usual. At N = 1 that is declaration order with no padding.
    let capped: Vec<Layout>;
    let fields = match repr.pack {
        Some(n) => {
            capped = fields
                .iter()
                .map(|f| {
                    let mut f = f.clone();
                    f.align = f.align.min(n);
                    f
                })
                .collect();
            &capped[..]
        }
        None => fields,
    };
    match repr.kind {
        ReprKind::Transparent => {
            let mut out = zst_layout(fields.iter().map(|f| f.align).max().unwrap_or(1));
            for f in fields {
                if f.size > 0 {
                    out = f.clone();
                    break;
                }
            }
            out.field_offsets = vec![0; fields.len()];
            out.uninhabited = fields.iter().any(|f| f.uninhabited);
            return finish(out, repr);
        }
        ReprKind::C => {
            let order: Vec<usize> = (0..fields.len()).collect();
            let (end, offsets) = place_in_order(fields, &order);
            let align = fields.iter().map(|f| f.align).max().unwrap_or(1);
            let niche = best_niche(fields, &offsets, &order);
            let mut out = Layout::simple(align_up(end, align), align, niche);
            out.field_offsets = offsets;
            out.uninhabited = fields.iter().any(|f| f.uninhabited);
            return finish(out, repr);
        }
        ReprKind::Default => {}
    }
    univariant_layout(fields, repr, UnivKind::AlwaysSized)
}

/// The `repr(C)` tag: a C `int` when every discriminant fits in
/// it, else a C `unsigned int` when they all fit there, else an
/// 8-byte signed tag. This is rustc's rule (discriminants of a
/// `repr(C)` enum are `isize` values, so 8 signed bytes always
/// hold them); a fixed signed 4-byte tag truncates values past
/// `i32` and sign-extends values past `u32` on reads.
fn tag_width_c(discs: &[i128]) -> (u64, bool) {
    if discs.iter().all(|&d| d >= i32::MIN as i128 && d <= i32::MAX as i128) {
        (4, true)
    } else if discs.iter().all(|&d| d >= 0 && d <= u32::MAX as i128) {
        (4, false)
    } else {
        (8, true)
    }
}

/// The smallest tag width in bytes (1, 2, 4, 8, 16) whose range holds every
/// discriminant. Signedness is signed when any discriminant is negative.
fn tag_width(discs: &[i128]) -> Result<(u64, bool), ()> {
    let signed = discs.iter().any(|&d| d < 0);
    for w in [1u64, 2, 4, 8, 16] {
        let bits = w * 8;
        let (lo, hi): (i128, i128) = if signed {
            if bits >= 128 { (i128::MIN, i128::MAX) } else { (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1) }
        } else if bits >= 128 {
            (0, i128::MAX)
        } else {
            (0, (1i128 << bits) - 1)
        };
        if discs.iter().all(|&d| d >= lo && d <= hi) {
            return Ok((w, signed));
        }
    }
    Err(())
}

/// Free values and first free value of an explicit tag scalar, under rustc's
/// wrapping valid-range model (`WrappingRange::smallest_range_containing`):
/// the valid values form the smallest wrapping arc of bit patterns that
/// covers every discriminant (the plain `min..=max` arc wins a tie, then the
/// lowest wrapping one), and the niche is the rest of the circle. Negative
/// discriminants therefore keep their niche: the signed range, read as bit
/// patterns, is one wrapping arc. No discriminant at all (every variant
/// uninhabited) is the arc `0..=0`.
fn tag_niche(width: u64, discs: &[i128]) -> Option<Niche> {
    let bits = width * 8;
    let mask: u128 = if bits >= 128 { u128::MAX } else { (1u128 << bits) - 1 };
    let mut pats: Vec<u128> = discs.iter().map(|&d| (d as u128) & mask).collect();
    if pats.is_empty() {
        pats.push(0);
    }
    pats.sort_unstable();
    pats.dedup();
    let (min, max) = (pats[0], pats[pats.len() - 1]);
    // An arc as (start, end), and its value count less one.
    let span = |(s, e): (u128, u128)| e.wrapping_sub(s) & mask;
    let mut best = (min, max);
    if max - min > 1u128 << (bits - 1) {
        for w in pats.windows(2) {
            let cand = (w[1], w[0]);
            if span(cand) < span(best) {
                best = cand;
            }
        }
    }
    let free = mask - span(best);
    if free == 0 {
        return None;
    }
    Some(Niche { offset: 0, scalar_size: width, free, start: best.1.wrapping_add(1) & mask })
}

/// One variant laid out as a struct that starts with a tag of `tag_w` bytes:
/// tag first, then fields by smallest alignment first, ties in declaration
/// order (the tagged-enum rule of 4.4). Returns (end, align, field offsets).
fn tagged_variant(fields: &[Layout], tag_w: u64) -> (u64, u64, Vec<u64>) {
    let mut order: Vec<usize> = (0..fields.len()).collect();
    order.sort_by(|&a, &b| fields[a].align.cmp(&fields[b].align).then(a.cmp(&b)));
    let mut offsets = vec![0u64; fields.len()];
    let mut cur = tag_w;
    let mut align = tag_w.max(1);
    for &i in &order {
        align = align.max(fields[i].align);
        cur = align_up(cur, fields[i].align);
        offsets[i] = cur;
        cur += fields[i].size;
    }
    (cur, align, offsets)
}


/// The layout of an enum. `variants` holds each variant's field layouts in
/// declaration order; `discs` holds the discriminant of each variant.
pub fn enum_layout(variants: &[Vec<Layout>], discs: &[i128], repr: Repr) -> Result<Layout, ()> {
    if variants.is_empty() {
        let mut l = zst_layout(1);
        l.uninhabited = true;
        return finish(l, repr);
    }
    if discs.len() != variants.len() {
        return Err(());
    }
    if repr.kind == ReprKind::Transparent {
        // One variant (collect checks the count): the layout is
        // that variant's transparent struct layout, with no tag.
        if variants.len() != 1 {
            return Err(());
        }
        let mut l = struct_layout(&variants[0], repr)?;
        l.variant_offsets = vec![l.field_offsets.clone()];
        l.enum_tag = Some(EnumTag::None(0));
        return Ok(l);
    }
    // A fixed integer tag must hold every discriminant. A u128
    // discriminant at or above 2^127 is stored as a negative i128
    // and is in range.
    if let Some((bits, signed)) = repr.int {
        let in_range = |d: i128| {
            if signed {
                bits >= 128 || (d >= -(1i128 << (bits - 1)) && d < (1i128 << (bits - 1)))
            } else if bits >= 128 {
                true
            } else {
                d >= 0 && d < (1i128 << bits)
            }
        };
        if !discs.iter().all(|&d| in_range(d)) {
            return Err(());
        }
    }
    // One variant under the default repr is just its fields as a plain struct.
    if variants.len() == 1 && !repr.fixes_tag() {
        let mut l = struct_layout(&variants[0], Repr::default())?;
        l.variant_offsets = vec![l.field_offsets.clone()];
        l.enum_tag = Some(EnumTag::None(0));
        return finish(l, repr);
    }
    // An absent variant (uninhabited, only 1-ZST fields) takes no part in the
    // layout under the default repr. With one inhabited variant
    // left, the enum is that variant with no tag; with none, it is
    // itself uninhabited. With two or more left, the candidates
    // below run over every variant as before.
    if !repr.fixes_tag() && variants.len() > 1 {
        let kept: Vec<usize> = (0..variants.len()).filter(|&i| !absent(&variants[i])).collect();
        if kept.len() < variants.len() {
            if kept.is_empty() {
                let mut l = zst_layout(1);
                l.uninhabited = true;
                l.variant_offsets = variants.iter().map(|_| Vec::new()).collect();
                return finish(l, repr);
            }
            if kept.len() == 1 {
                let vi = kept[0];
                let mut l = struct_layout(&variants[vi], Repr::default())?;
                let kept_offsets = l.field_offsets.clone();
                l.variant_offsets = (0..variants.len())
                    .map(|i| if i == vi { kept_offsets.clone() } else { vec![0; variants[i].len()] })
                    .collect();
                l.enum_tag = Some(EnumTag::None(vi));
                return finish(l, repr);
            }
        }
    }
    let tagged = tagged_enum_layout(variants, discs, repr)?;
    if repr.fixes_tag() {
        return Ok(tagged);
    }
    match niche_enum_layout(variants, discs, repr) {
        Some(niche) => {
            let pick_niche = niche.size < tagged.size
                || (niche.size == tagged.size && niche.niche.map_or(0, |n| n.free) > tagged.niche.map_or(0, |n| n.free));
            Ok(if pick_niche { niche } else { tagged })
        }
        None => Ok(tagged),
    }
}

/// The tagged candidate layout (and the only candidate when the repr fixes the tag).
fn tagged_enum_layout(variants: &[Vec<Layout>], discs: &[i128], repr: Repr) -> Result<Layout, ()> {
    let l = tagged_enum_layout_inner(variants, discs, repr)?;
    // The tag's valid range covers only the discriminants of inhabited
    // variants (all of them under repr(C)); that decides the enum's niche.
    let tag_discs: Vec<i128> = (0..variants.len())
        .filter(|&i| repr.kind == ReprKind::C || !variants[i].iter().any(|f| f.uninhabited))
        .map(|i| discs[i])
        .collect();
    let Some(EnumTag::Explicit { size, .. }) = l.enum_tag else { return Ok(l) };
    let uninhabited = variants.iter().all(|v| v.iter().any(|f| f.uninhabited));
    Ok(Layout { niche: tag_niche(size, &tag_discs), uninhabited, ..l })
}

fn tagged_enum_layout_inner(variants: &[Vec<Layout>], discs: &[i128], repr: Repr) -> Result<Layout, ()> {
    let (mut tag_w, signed) = if let Some((bits, signed)) = repr.int {
        ((bits / 8) as u64, signed)
    } else if repr.kind == ReprKind::C {
        tag_width_c(discs)
    } else {
        tag_width(discs)?
    };
    let fieldless = variants.iter().all(|v| v.is_empty());
    if fieldless {
        let mut l = Layout::simple(tag_w, tag_w, tag_niche(tag_w, discs));
        l.variant_offsets = variants.iter().map(|_| Vec::new()).collect();
        l.enum_tag = Some(EnumTag::Explicit { size: tag_w, signed });
        return finish(l, repr);
    }
    if repr.pack.is_some() {
        // Tag first, then fields in declaration order, no padding anywhere.
        // (Packed enums reject in collect; this stays for shape queries.)
        let mut size = 0u64;
        let mut variant_offsets = Vec::new();
        for v in variants {
            let mut cur = tag_w;
            let mut offs = Vec::new();
            for f in v {
                offs.push(cur);
                cur += f.size;
            }
            size = size.max(cur);
            variant_offsets.push(offs);
        }
        let mut l = Layout::simple(size, 1, tag_niche(tag_w, discs));
        l.variant_offsets = variant_offsets;
        l.enum_tag = Some(EnumTag::Explicit { size: tag_w, signed });
        return finish(l, repr);
    }
    if repr.kind == ReprKind::C {
        // repr(C) data enums are `struct { tag; union { variant structs } }`:
        // each variant payload is a repr(C) struct of its fields (no tag
        // inside), and the union starts after the tag, aligned to the
        // largest payload alignment. Only repr(int) alone keeps the tag
        // first inside each variant struct (the `fixed` path below).
        let mut union_size = 0u64;
        let mut union_align = 1u64;
        let mut payloads: Vec<Layout> = Vec::with_capacity(variants.len());
        for v in variants {
            let pl = struct_layout(v, Repr { kind: ReprKind::C, ..Repr::default() })?;
            union_size = union_size.max(pl.size);
            union_align = union_align.max(pl.align);
            payloads.push(pl);
        }
        let payload_off = align_up(tag_w, union_align);
        let align = tag_w.max(1).max(union_align);
        let size = align_up(payload_off + union_size, align);
        let variant_offsets: Vec<Vec<u64>> =
            payloads.iter().map(|pl| pl.field_offsets.iter().map(|&o| payload_off + o).collect()).collect();
        let mut l = Layout::simple(size, align, tag_niche(tag_w, discs));
        l.variant_offsets = variant_offsets;
        l.enum_tag = Some(EnumTag::Explicit { size: tag_w, signed });
        return finish(l, repr);
    }
    let fixed = repr.int.is_some();
    let layout_with = |tag_w: u64| -> (u64, u64, Vec<Vec<u64>>) {
        let mut size = 0u64;
        let mut align = tag_w.max(1);
        let mut variant_offsets = Vec::new();
        for v in variants {
            let (end, a, offs) = if fixed {
                // repr(int) keeps declaration order after the tag.
                let mut offs = vec![0u64; v.len()];
                let mut cur = tag_w;
                let mut a = tag_w.max(1);
                for (i, f) in v.iter().enumerate() {
                    a = a.max(f.align);
                    cur = align_up(cur, f.align);
                    offs[i] = cur;
                    cur += f.size;
                }
                (cur, a, offs)
            } else {
                tagged_variant(v, tag_w)
            };
            align = align.max(a);
            size = size.max(align_up(end, a));
            variant_offsets.push(offs);
        }
        (align_up(size, align), align, variant_offsets)
    };
    let (size, align, mut variant_offsets) = layout_with(tag_w);
    if repr.int.is_none() && repr.kind == ReprKind::Default {
        // rustc widens the tag to the smallest alignment among the
        // variants' first fields (by offset) that are not 1-ZSTs, when
        // that alignment is an integer size above the tag's. Every other
        // field is at least as aligned, so no field moves and the size
        // does not change; the enum's own niche grows.
        let mut start_align: Option<u64> = None;
        for (v, offs) in variants.iter().zip(&variant_offsets) {
            let first = (0..v.len()).filter(|&i| !(v[i].size == 0 && v[i].align == 1)).min_by_key(|&i| (offs[i], v[i].align, i));
            if let Some(i) = first {
                start_align = Some(start_align.map_or(v[i].align, |a: u64| a.min(v[i].align)));
            }
        }
        if let Some(a) = start_align
            && a > tag_w
            && a <= 16
        {
            tag_w = a;
            let (_, _, offs) = layout_with(tag_w);
            variant_offsets = offs;
        }
    }
    let mut l = Layout::simple(size, align, tag_niche(tag_w, discs));
    l.variant_offsets = variant_offsets;
    l.enum_tag = Some(EnumTag::Explicit { size: tag_w, signed });
    finish(l, repr)
}

/// The niche-filled candidate layout, when one exists (rustc's rule):
/// every variant is laid out as a plain univariant under the enum repr,
/// the largest variant by size (the last on a tie) donates its niche,
/// and every other variant's whole layout must fit either before the
/// niche scalar or shifted to just after it. Niche values are reserved
/// (`Niche::reserve`) for the whole index span from the first to the
/// last variant that needs a tag (not the donor, not absent).
fn niche_enum_layout(variants: &[Vec<Layout>], discs: &[i128], repr: Repr) -> Option<Layout> {
    let _ = discs;
    if variants.len() < 2 {
        return None;
    }
    let vrepr = Repr { kind: ReprKind::Default, int: None, word: false, align: repr.align, pack: None };
    let mut layouts: Vec<Layout> = Vec::with_capacity(variants.len());
    for v in variants {
        layouts.push(univariant_layout(v, vrepr, UnivKind::AlwaysSized).ok()?);
    }
    let mut vi = 0;
    for i in 1..layouts.len() {
        if layouts[i].size >= layouts[vi].size {
            vi = i;
        }
    }
    let base = layouts[vi].clone();
    let needs_tag = |i: usize| i != vi && !absent(&variants[i]);
    let first = (0..variants.len()).find(|&i| needs_tag(i))?;
    let last = (0..variants.len()).rev().find(|&i| needs_tag(i))?;
    let niche = base.niche?;
    let (start, left) = niche.reserve((last - first + 1) as u128)?;
    // Another variant may need a larger alignment than the niche variant
    // (a u128 payload next to an align-8 niche variant): the enum then
    // takes that alignment, and its size is rounded up to it.
    let eff_align = layouts.iter().map(|l| l.align).max().unwrap_or(1);
    let eff_size = align_up(base.size, eff_align);
    let mut variant_offsets: Vec<Vec<u64>> = Vec::with_capacity(variants.len());
    for (i, l) in layouts.iter().enumerate() {
        if i == vi || l.size <= niche.offset {
            variant_offsets.push(l.field_offsets.clone());
        } else {
            let shift = align_up(niche.offset + niche.scalar_size, l.align);
            if shift + l.size > eff_size {
                return None;
            }
            variant_offsets.push(l.field_offsets.iter().map(|&o| o + shift).collect());
        }
    }
    let mut l = Layout::simple(eff_size, eff_align, left);
    l.variant_offsets = variant_offsets;
    l.enum_tag = Some(EnumTag::NicheFilled { variant: vi, niche, first, last, start });
    l.uninhabited = layouts.iter().all(|v| v.uninhabited);
    finish(l, repr).ok()
}

/// rustc's absent variant: uninhabited with only 1-ZST fields. It needs
/// no tag value and, under the default repr, no space.
fn absent(fields: &[Layout]) -> bool {
    fields.iter().any(|f| f.uninhabited) && fields.iter().all(|f| f.size == 0 && f.align == 1)
}

/// `l` behind an `UnsafeCell`: the same size, alignment and
/// inhabitedness (`Cell<!>` has no value), with no niche for an
/// enclosing enum to use.
fn no_niche(l: Layout) -> Layout {
    Layout { uninhabited: l.uninhabited, ..Layout::simple(l.size, l.align, None) }
}

/// Applies `repr(align(n))` and the final size round-up.
fn finish(mut l: Layout, repr: Repr) -> Result<Layout, ()> {
    if let Some(a) = repr.align {
        if a == 0 || !a.is_power_of_two() {
            return Err(());
        }
        if a > l.align {
            l.align = a;
            l.size = align_up(l.size, a);
        }
    }
    Ok(l)
}

/// The shape of one Adt for layout: its repr, each variant's field types in
/// declaration order, and each variant's discriminant. A struct has one variant.
/// `hide_niche`: the real type holds these fields in an `UnsafeCell` (a std
/// cell type whose source form is plain), so its niche is not usable.
pub struct AdtShape {
    pub repr: Repr,
    pub is_enum: bool,
    pub variants: Vec<Vec<Ty>>,
    pub discs: Vec<i128>,
    pub hide_niche: bool,
}

/// Adt shapes by session-local id, supplied by collect's tables.
pub trait AdtShapes {
    fn adt_shape(&self, id: AdtId) -> Option<AdtShape>;
}

/// The layout of a semantic type. `Err(())` means the type is outside the
/// supported-shape grammar of 4.4 (unsized, a generic parameter with no
/// argument in scope, an unknown Adt such as `Box` before its wave, a user
/// union, and so on). A generic Adt is laid out from its shape with the
/// application's arguments bound to the Adt's params (a frame below), so
/// `W<u64>` gets its own layout with no substituted type built.
pub fn layout_of_ty(tys: &impl TyIntern, shapes: &impl AdtShapes, t: Ty) -> Result<Layout, ()> {
    LAST_FAILURE.with(|f| f.set(None));
    let mut frames: Vec<Frame> = Vec::new();
    let mut memo: Vec<((Ty, Option<usize>), Option<Layout>)> = Vec::new();
    let mut active: Vec<(Ty, Option<usize>)> = Vec::new();
    go(tys, shapes, t, None, &mut frames, &mut memo, &mut active)
}

thread_local! {
    /// The innermost type whose layout failed in this thread's last
    /// `layout_of_ty` call, and whether the failure is a type that did not
    /// resolve inside it. Only diagnostics read it (`last_failure`).
    static LAST_FAILURE: std::cell::Cell<Option<(Ty, bool)>> = const { std::cell::Cell::new(None) };
}

/// The innermost type of the last failed `layout_of_ty` call on this
/// thread, so L0652 can name the form that has no layout. When the
/// failure is an unresolved type (already reported where it is
/// written), this is the innermost Adt around it, with `true`.
pub fn last_failure() -> Option<(Ty, bool)> {
    LAST_FAILURE.with(|f| f.get())
}

/// One generic application's arguments, each paired with the frame its
/// params read in (the frame of the type that wrote the application).
type Frame = Vec<(super::GenericArg, Option<usize>)>;

/// Follows `Param` links through frames to the first non-param type,
/// with the frame that type reads in.
fn peel_param(tys: &impl TyIntern, mut t: Ty, mut env: Option<usize>, frames: &[Frame]) -> (Ty, Option<usize>) {
    while let TyKind::Param(p) = tys.kind(t) {
        let Some(f) = env.and_then(|f| frames.get(f)) else { break };
        let Some(&(arg, aenv)) = f.get(p.0 as usize) else { break };
        let Some(u) = arg.as_ty() else { break };
        t = u;
        env = aenv;
    }
    (t, env)
}

/// An array length through frames: a const param reads its argument.
fn const_len(tys: &impl TyIntern, mut c: super::ConstId, mut env: Option<usize>, frames: &[Frame]) -> Option<u64> {
    loop {
        match tys.const_kind(c) {
            super::ConstKind::Int(n) => return u64::try_from(n).ok(),
            super::ConstKind::Param(i) => {
                let &(arg, aenv) = frames.get(env?)?.get(i as usize)?;
                let super::GenericArg::Const(c2) = arg else { return None };
                c = c2;
                env = aenv;
            }
            _ => return None,
        }
    }
}

fn go(
    tys: &impl TyIntern,
    shapes: &impl AdtShapes,
    t: Ty,
    env: Option<usize>,
    frames: &mut Vec<Frame>,
    memo: &mut Vec<((Ty, Option<usize>), Option<Layout>)>,
    active: &mut Vec<(Ty, Option<usize>)>,
) -> Result<Layout, ()> {
    if let Some((_, l)) = memo.iter().find(|(m, _)| *m == (t, env)) {
        return l.clone().ok_or(());
    }
    if active.contains(&(t, env)) {
        return Err(()); // Recursive type without indirection: collect rejects it first.
    }
    active.push((t, env));
    let r = compute(tys, shapes, t, env, frames, memo, active);
    if r.is_err() {
        let u = peel_param(tys, t, env, frames).0;
        LAST_FAILURE.with(|f| match f.get() {
            None => f.set(Some((u, matches!(tys.kind(u), TyKind::Error)))),
            Some((e, true)) if matches!(tys.kind(e), TyKind::Error) && matches!(tys.kind(u), TyKind::Adt(..)) => f.set(Some((u, true))),
            _ => {}
        });
    }
    active.pop();
    memo.push(((t, env), r.as_ref().ok().cloned()));
    r
}

fn compute(
    tys: &impl TyIntern,
    shapes: &impl AdtShapes,
    t: Ty,
    env: Option<usize>,
    frames: &mut Vec<Frame>,
    memo: &mut Vec<((Ty, Option<usize>), Option<Layout>)>,
    active: &mut Vec<(Ty, Option<usize>)>,
) -> Result<Layout, ()> {
    match tys.kind(t) {
        TyKind::Param(_) => {
            let (u, uenv) = peel_param(tys, t, env, frames);
            if u == t {
                return Err(()); // No argument in scope: the caller (a template check) owns the placeholder.
            }
            go(tys, shapes, u, uenv, frames, memo, active)
        }
        TyKind::Unit => Ok(zst_layout(1)),
        TyKind::Never => Ok(never_layout()),
        TyKind::Bool => Ok(bool_layout()),
        TyKind::Char => Ok(char_layout()),
        TyKind::Int(i) => Ok(int_layout(i)),
        TyKind::Float(f) => Ok(float_layout(f)),
        TyKind::Ref(_, _, e) => match tys.kind(peel_param(tys, e, env, frames).0) {
            TyKind::Slice(_) | TyKind::Str | TyKind::Dyn(..) => Ok(fat_ptr_layout()),
            _ => Ok(ptr_layout()),
        },
        TyKind::RawPtr(_, e) => match tys.kind(peel_param(tys, e, env, frames).0) {
            TyKind::Slice(_) | TyKind::Str => Ok(fat_raw_ptr_layout()),
            TyKind::Dyn(..) => Ok(dyn_raw_ptr_layout()),
            _ => Ok(raw_ptr_layout()),
        },
        TyKind::FnPtr(_) => Ok(ptr_layout()),
        TyKind::FnDef(..) => Ok(zst_layout(1)),
        TyKind::Array(e, c) => {
            let len = const_len(tys, c, env, frames).ok_or(())?;
            let el = go(tys, shapes, e, env, frames, memo, active)?;
            array_layout(&el, len)
        }
        TyKind::Tuple(ts) => {
            let elems: Vec<Ty> = tys.tys_of(ts).to_vec();
            let mut fields = Vec::with_capacity(elems.len());
            for e in elems {
                fields.push(go(tys, shapes, e, env, frames, memo, active)?);
            }
            tuple_layout(&fields)
        }
        // A closure is laid out like a tuple of its captures, in
        // capture order (ARCH 4.5): no captures is size 0.
        TyKind::Closure(_, a) => {
            let elems: Vec<Ty> = tys.args(a).iter().filter_map(|g| g.as_ty()).collect();
            if elems.is_empty() {
                return Ok(zst_layout(1));
            }
            let mut fields = Vec::with_capacity(elems.len());
            for e in elems {
                fields.push(go(tys, shapes, e, env, frames, memo, active)?);
            }
            tuple_layout(&fields)
        }
        TyKind::Adt(..) => match tys.builtin_of(t) {
            // String and Vec are (ptr, len, cap) words in this order for layout.
            // `Box<dyn Tr>` and `Rc<dyn Tr>` are fat pointers (ARCH 2.8).
            Some(BuiltinTy::Boxed) | Some(BuiltinTy::Rc)
                if tys.adt_first_ty_arg(t).is_some_and(|e| matches!(tys.kind(e), TyKind::Dyn(..))) =>
            {
                Ok(fat_ptr_layout())
            }
            // `Box<[T]>` is a fat pointer, like `&[T]`.
            Some(BuiltinTy::Boxed) if tys.boxed_slice_elem(t).is_some() => Ok(fat_ptr_layout()),
            // An owned fat pointer to a slice or str (`Box<str>`,
            // `Rc<[T]>`), as above.
            Some(BuiltinTy::Boxed) | Some(BuiltinTy::Rc)
                if tys.adt_first_ty_arg(t).is_some_and(|e| matches!(tys.kind(e), TyKind::Slice(_) | TyKind::Str)) =>
            {
                Ok(fat_ptr_layout())
            }
            Some(BuiltinTy::Boxed) | Some(BuiltinTy::Rc) => Ok(ptr_layout()),
            // `rc::Weak<T>`: one `NonNull` pointer (null is its niche;
            // `Weak::new` dangles at usize::MAX), fat for an unsized `T`.
            Some(BuiltinTy::Weak) => match tys.adt_first_ty_arg(t).map(|e| tys.kind(peel_param(tys, e, env, frames).0)) {
                Some(TyKind::Slice(_) | TyKind::Str | TyKind::Dyn(..)) => Ok(fat_ptr_layout()),
                Some(_) => Ok(ptr_layout()),
                None => Err(()),
            },
            // `Ref<T>` and `RefMut<T>` are `{ value: NonNull<T>, borrow: &Cell<isize> }`
            // (RefMut adds a PhantomData): two pointers, the first fat for an unsized `T`.
            Some(BuiltinTy::RefGuard | BuiltinTy::RefMutGuard) => {
                let e = tys.adt_first_ty_arg(t).ok_or(())?;
                let value = match tys.kind(peel_param(tys, e, env, frames).0) {
                    TyKind::Slice(_) | TyKind::Str | TyKind::Dyn(..) => fat_ptr_layout(),
                    _ => ptr_layout(),
                };
                let mut l = struct_layout(&[value, ptr_layout()], Repr::default())?;
                l.field_offsets.clear();
                Ok(l)
            }
            // rustc 1.99 lays `Vec` (and `String`) out as `{cap, ptr,
            // len}`: the capacity niche is at offset 0, which decides
            // where an enclosing enum's other variants fit. (cgen's C
            // value is `{ptr, len, cap}`; only its size must agree.)
            Some(BuiltinTy::String) | Some(BuiltinTy::Vec) => struct_layout(
                &[cap_layout(), ptr_layout(), int_layout(IntTy::Usize)],
                Repr { kind: ReprKind::C, ..Repr::default() },
            ),
            Some(BuiltinTy::Option) => {
                let e = tys.adt_first_ty_arg(t).ok_or(())?;
                let el = go(tys, shapes, e, env, frames, memo, active)?;
                enum_layout(&[vec![], vec![el]], &[0, 1], Repr::default())
            }
            Some(BuiltinTy::Result) => {
                let (ok_t, err_t) = tys.result_args(t).ok_or(())?;
                let ok_l = go(tys, shapes, ok_t, env, frames, memo, active)?;
                let err_l = go(tys, shapes, err_t, env, frames, memo, active)?;
                enum_layout(&[vec![ok_l], vec![err_l]], &[0, 1], Repr::default())
            }
            // An atomic: its value's size and alignment, with no niche
            // (`UnsafeCell`), as std lays out `Atomic<T>` (W-THREADS).
            Some(BuiltinTy::Sync(k)) if let Some(v) = k.atomic_value() => Ok(Layout::simple(v.bytes() as u64, v.bytes() as u64, None)),
            // `Arc<T>` and `sync::Weak<T>`: one non-null pointer, or a
            // fat one for an unsized `T`.
            Some(BuiltinTy::Sync(crate::ty::sync::SyncTy::Arc | crate::ty::sync::SyncTy::ArcWeak))
                if tys.adt_first_ty_arg(t).is_some_and(|e| matches!(tys.kind(e), TyKind::Slice(_) | TyKind::Str | TyKind::Dyn(..))) =>
            {
                Ok(fat_ptr_layout())
            }
            Some(BuiltinTy::Sync(crate::ty::sync::SyncTy::Arc | crate::ty::sync::SyncTy::ArcWeak)) => Ok(ptr_layout()),
            // `Cell<T>` is `UnsafeCell<T>`: T's size and alignment, its
            // niche hidden.
            Some(BuiltinTy::Cell) => Ok(no_niche(go(tys, shapes, tys.adt_first_ty_arg(t).ok_or(())?, env, frames, memo, active)?)),
            // `RefCell<T>`: `{ borrow: Cell<isize>, value: UnsafeCell<T> }`,
            // laid out as a Rust struct.
            Some(BuiltinTy::RefCell) => {
                let v = no_niche(go(tys, shapes, tys.adt_first_ty_arg(t).ok_or(())?, env, frames, memo, active)?);
                struct_layout(&[int_layout(IntTy::Isize), v], Repr::default())
            }
            // `Mutex<T>` is `{ futex, poison: AtomicBool, data: UnsafeCell<T> }`.
            // The futex is an `AtomicU32` on Unix and a `SmallFutex`
            // (`AtomicU8`) on Windows. `RwLock<T>` is two `AtomicU32`s
            // (8 bytes) plus the poison flag on every host.
            // `OnceLock<T>`: `{ once: Once (an AtomicU32 futex), value:
            // UnsafeCell<MaybeUninit<T>> }`.
            Some(BuiltinTy::Sync(crate::ty::sync::SyncTy::OnceLock)) => {
                // The value is a `MaybeUninit<T>`: inhabited for any `T`.
                let v = Layout { uninhabited: false, ..no_niche(go(tys, shapes, tys.adt_first_ty_arg(t).ok_or(())?, env, frames, memo, active)?) };
                struct_layout(&[Layout::simple(4, 4, None), v], Repr::default())
            }
            Some(BuiltinTy::Sync(k @ (crate::ty::sync::SyncTy::Mutex | crate::ty::sync::SyncTy::RwLock))) => {
                let v = no_niche(go(tys, shapes, tys.adt_first_ty_arg(t).ok_or(())?, env, frames, memo, active)?);
                let (state, state_align) = if k == crate::ty::sync::SyncTy::Mutex {
                    if cfg!(windows) { (1, 1) } else { (4, 4) }
                } else {
                    (8, 4)
                };
                struct_layout(&[Layout::simple(state, state_align, None), Layout::simple(1, 1, None), v], Repr::default())
            }
            // The std types below as Rust 1.99 lays them out on 64-bit
            // Linux (checked with size_of and Option niches, tf-build): the
            // field lists give rustc's reordering and largest niche.
            Some(b @ (BuiltinTy::Hash(_) | BuiltinTy::Sync(_) | BuiltinTy::Iter(_) | BuiltinTy::Chars | BuiltinTy::ExitCode)) => {
                let arg = |i: usize| tys.adt_parts(t).and_then(|(_, a)| tys.args(a).get(i).and_then(|g| g.as_ty()));
                let usize_l = || int_layout(IntTy::Usize);
                let fields: Vec<Layout> = match b {
                    // hashbrown: the hasher, then RawTableInner { ctrl:
                    // NonNull<u8>, bucket_mask, growth_left, items }.
                    BuiltinTy::Hash(HashTy::HashMap | HashTy::HashSet) => {
                        let s = arg(if b == BuiltinTy::Hash(HashTy::HashMap) { 2 } else { 1 }).ok_or(())?;
                        vec![go(tys, shapes, s, env, frames, memo, active)?, ptr_layout(), usize_l(), usize_l(), usize_l()]
                    }
                    BuiltinTy::Hash(HashTy::RandomState) => return Ok(Layout::simple(16, 8, None)),
                    // SipHasher13: k0, k1, length, v0..v3, tail, ntail.
                    BuiltinTy::Hash(HashTy::DefaultHasher) => return Ok(Layout::simple(72, 8, None)),
                    BuiltinTy::Hash(HashTy::BuildHasherDefault) => return Ok(zst_layout(1)),
                    // JoinInner { native: pthread_t, thread: Thread, packet: Arc<..> }.
                    BuiltinTy::Sync(SyncTy::JoinHandle) => vec![int_layout(IntTy::U64), ptr_layout(), ptr_layout()],
                    BuiltinTy::Sync(SyncTy::Thread) => return Ok(ptr_layout()),
                    BuiltinTy::Sync(SyncTy::ThreadId) => return Ok(nonzero_layout(IntTy::U64)),
                    // The mpmc flavor enum: a u8 tag (3 variants) and a pointer.
                    BuiltinTy::Sync(SyncTy::Sender | SyncTy::SyncSender | SyncTy::Receiver) => {
                        return Ok(Layout::simple(16, 8, Some(Niche { offset: 0, scalar_size: 1, free: 253, start: 3 })));
                    }
                    BuiltinTy::Sync(SyncTy::Condvar) => return Ok(Layout::simple(4, 4, None)),
                    // { lock: Mutex<BarrierState>, cvar: Condvar, num_threads }.
                    BuiltinTy::Sync(SyncTy::Barrier) => return Ok(Layout::simple(40, 8, None)),
                    BuiltinTy::Sync(SyncTy::BarrierWaitResult) => return Ok(bool_layout()),
                    BuiltinTy::Iter(IterKind::Range) => {
                        let e = go(tys, shapes, arg(0).ok_or(())?, env, frames, memo, active)?;
                        vec![e.clone(), e]
                    }
                    BuiltinTy::Iter(IterKind::RangeInclusive) => {
                        let e = go(tys, shapes, arg(0).ok_or(())?, env, frames, memo, active)?;
                        vec![e.clone(), e, bool_layout()]
                    }
                    // slice::Iter<u8> { ptr: NonNull<u8>, end_or_len }.
                    BuiltinTy::Chars => vec![ptr_layout(), raw_ptr_layout()],
                    BuiltinTy::Iter(IterKind::CharIndices) => vec![usize_l(), ptr_layout(), raw_ptr_layout()],
                    // { buf: NonNull<T>, cap, ptr: NonNull<T>, end }.
                    BuiltinTy::Iter(IterKind::VecIntoIter) => vec![ptr_layout(), usize_l(), ptr_layout(), raw_ptr_layout()],
                    // Unix `ExitCode` is a `u8`. Windows stores a `u32`
                    // (`ExitCode(ExitCode(n))`, size 4, align 4).
                    BuiltinTy::ExitCode => {
                        return Ok(int_layout(if cfg!(windows) { IntTy::U32 } else { IntTy::U8 }));
                    }
                    _ => return Err(()),
                };
                let mut l = struct_layout(&fields, Repr::default())?;
                l.field_offsets.clear();
                Ok(l)
            }
            Some(_) => Err(()),
            // `core::num::NonZero<T>` (TRIAGE T5): the integer
            // itself, with zero as the niche.
            // `core::mem::MaybeUninit<T>` (TRIAGE T5): T's layout
            // with no niche (it is a union).
            None if tys.adt_parts(t).is_some_and(|(def, _)| def == super::maybe_uninit_def()) => {
                let e = tys.adt_first_ty_arg(t).ok_or(())?;
                let mut l = go(tys, shapes, e, env, frames, memo, active)?;
                l.niche = None;
                // A union: `MaybeUninit<!>` still has a value (uninit).
                l.uninhabited = false;
                Ok(l)
            }
            None if tys.adt_parts(t).is_some_and(|(def, _)| def == super::nonzero_def()) => {
                match tys.adt_first_ty_arg(t).map(|a| tys.kind(a)) {
                    Some(TyKind::Int(i)) => Ok(nonzero_layout(i)),
                    _ => Err(()),
                }
            }
            None => {
                let id = tys.local_adt(t).ok_or(())?;
                let shape = shapes.adt_shape(id).ok_or(())?;
                // The shape's field types read in a new frame that binds
                // the Adt's params to this application's arguments.
                let args: Vec<super::GenericArg> = tys.adt_parts(t).map(|p| tys.args(p.1).to_vec()).ok_or(())?;
                let frame = if args.is_empty() {
                    None
                } else {
                    frames.push(args.into_iter().map(|g| (g, env)).collect());
                    Some(frames.len() - 1)
                };
                let mut variants = Vec::with_capacity(shape.variants.len());
                for v in &shape.variants {
                    let mut fields = Vec::with_capacity(v.len());
                    for &f in v {
                        fields.push(go(tys, shapes, f, frame, frames, memo, active)?);
                    }
                    variants.push(fields);
                }
                let l = if shape.is_enum {
                    enum_layout(&variants, &shape.discs, shape.repr)
                } else {
                    struct_layout(&variants.into_iter().next().unwrap_or_default(), shape.repr)
                };
                if shape.hide_niche { l.map(no_niche) } else { l }
            }
        },
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(ts: &[IntTy]) -> Vec<Layout> {
        ts.iter().map(|&t| int_layout(t)).collect()
    }
    fn sz(l: &Layout) -> (u64, u64) {
        (l.size, l.align)
    }

    #[test]
    fn scalars() {
        assert_eq!(sz(&bool_layout()), (1, 1));
        assert_eq!(sz(&char_layout()), (4, 4));
        assert_eq!(sz(&int_layout(IntTy::U128)), (16, 16));
        assert_eq!(sz(&int_layout(IntTy::I128)), (16, 16));
        assert_eq!(sz(&float_layout(FloatTy::F32)), (4, 4));
        assert_eq!(sz(&float_layout(FloatTy::F64)), (8, 8));
        assert_eq!(sz(&ptr_layout()), (8, 8));
        assert_eq!(sz(&fat_ptr_layout()), (16, 8));
    }

    #[test]
    fn rust_struct_reorder() {
        // struct S { a: u8, b: u64, c: u8 } is 16, not 24.
        let l = struct_layout(&ints(&[IntTy::U8, IntTy::U64, IntTy::U8]), Repr::default()).unwrap();
        assert_eq!(sz(&l), (16, 8));
        assert_eq!(l.field_offsets, vec![8, 0, 9]);
        // The same fields under repr(C) keep declaration order: 24.
        let c = struct_layout(&ints(&[IntTy::U8, IntTy::U64, IntTy::U8]), Repr { kind: ReprKind::C, ..Repr::default() }).unwrap();
        assert_eq!(sz(&c), (24, 8));
        assert_eq!(c.field_offsets, vec![0, 8, 16]);
        // repr(packed): no padding, align 1.
        let p = struct_layout(&ints(&[IntTy::U8, IntTy::U64, IntTy::U8]), Repr { pack: Some(1), ..Repr::default() }).unwrap();
        assert_eq!(sz(&p), (10, 1));
        assert_eq!(p.field_offsets, vec![0, 1, 9]);
        // repr(packed(4)): Rust reorder by capped alignment alone:
        // the u64 (capped to 4) leads, size rounds to the align.
        let p = struct_layout(&ints(&[IntTy::U8, IntTy::U64, IntTy::U8]), Repr { pack: Some(4), ..Repr::default() }).unwrap();
        assert_eq!(sz(&p), (12, 4));
        assert_eq!(p.field_offsets, vec![8, 0, 9]);
        // repr(C, packed(4)): declaration order, capped alignments.
        let p =
            struct_layout(&ints(&[IntTy::U8, IntTy::U64, IntTy::U8]), Repr { kind: ReprKind::C, pack: Some(4), ..Repr::default() }).unwrap();
        assert_eq!(sz(&p), (16, 4));
        assert_eq!(p.field_offsets, vec![0, 4, 12]);
    }

    #[test]
    fn pinned_tagged_enums() {
        // enum { A(u8, u32), B } is 8: tag, u8, padding, u32.
        let l = enum_layout(&[ints(&[IntTy::U8, IntTy::U32]), vec![]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(sz(&l), (8, 4));
        // Variant (u16, [u8; 3]) gives a 6-byte enum.
        let arr3 = array_layout(&int_layout(IntTy::U8), 3).unwrap();
        let l = enum_layout(&[vec![int_layout(IntTy::U16), arr3], vec![]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 6);
        // Variants (u8, u64, u8) and (u32, u64) give 16 each.
        let l = enum_layout(&[ints(&[IntTy::U8, IntTy::U64, IntTy::U8])], &[0], Repr::default()).unwrap();
        assert_eq!(l.size, 16);
        let l = enum_layout(&[ints(&[IntTy::U32, IntTy::U64]), vec![]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 16);
        // TypeMapper-like: Array(Box, bool) variant gives 16 (tag, bool, pad, Box).
        let l = enum_layout(&[vec![ptr_layout(), bool_layout()], vec![]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(sz(&l), (16, 8));
    }

    #[test]
    fn pinned_niche_enums() {
        // Option<&u8> is 8; Option<bool> is 1.
        let l = enum_layout(&[vec![], vec![ptr_layout()]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(sz(&l), (8, 8));
        assert!(matches!(l.enum_tag, Some(EnumTag::NicheFilled { .. })));
        let l = enum_layout(&[vec![], vec![bool_layout()]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 1);
        // Pointer(Byte) does not niche-fill: the u8 has nowhere to sit. 16.
        let l = enum_layout(&[vec![ptr_layout()], ints(&[IntTy::U8])], &[0, 1], Repr::default()).unwrap();
        assert_eq!(sz(&l), (16, 8));
        // repr(u8) and repr(C) turn the niche candidate off: 16 each.
        let l = enum_layout(&[vec![ptr_layout()], vec![]], &[0, 1], Repr { int: Some((8, false)), ..Repr::default() }).unwrap();
        assert_eq!(l.size, 16);
        let l = enum_layout(&[vec![ptr_layout()], vec![]], &[0, 1], Repr { kind: ReprKind::C, ..Repr::default() }).unwrap();
        assert_eq!(l.size, 16);
    }

    #[test]
    fn rustc_niche_values() {
        // The tag value of variant `vi` of a niche-filled layout.
        let tag = |l: &Layout, vi: usize| match l.enum_tag {
            Some(EnumTag::NicheFilled { niche, first, start, .. }) => niche_tag_value(niche, first, start, vi),
            ref t => panic!("not niche-filled: {t:?}"),
        };
        let opt = |l: &Layout| enum_layout(&[vec![], vec![l.clone()]], &[0, 1], Repr::default()).unwrap();
        // Values as rustc 1.99 stores them (checked with transmute).
        let oc = opt(&char_layout());
        assert_eq!(tag(&oc, 0), 0xFFFF_FFFF);
        assert_eq!(tag(&opt(&oc), 0), 0xFFFF_FFFE);
        let ob = opt(&bool_layout());
        assert_eq!(tag(&ob, 0), 2);
        assert_eq!(tag(&opt(&ob), 0), 255);
        assert_eq!(tag(&opt(&ptr_layout()), 0), 0);
        assert_eq!(tag(&opt(&cap_layout()), 0), u64::MAX as u128);
        // enum Y { A, B(char), C, D }: numbered by variant index from A.
        let y = enum_layout(&[vec![], vec![char_layout()], vec![], vec![]], &[0, 1, 2, 3], Repr::default()).unwrap();
        assert_eq!([tag(&y, 0), tag(&y, 2), tag(&y, 3)], [0x110000, 0x110002, 0x110003]);
        // A tag range 5..=7 with two values to take: below it (3, 4).
        let e = enum_layout(&[vec![], vec![], vec![]], &[5, 6, 7], Repr::default()).unwrap();
        let t = enum_layout(&[vec![], vec![], vec![e]], &[0, 1, 2], Repr::default()).unwrap();
        assert_eq!([tag(&t, 0), tag(&t, 1)], [3, 4]);
    }

    #[test]
    fn vec_capacity_niche() {
        // A Vec/String-shaped (ptr, len, cap) value reports the
        // capacity niche, so two nested Options stay 24 bytes.
        let v = struct_layout(
            &[ptr_layout(), int_layout(IntTy::Usize), cap_layout()],
            Repr { kind: ReprKind::C, ..Repr::default() },
        )
        .unwrap();
        assert_eq!(sz(&v), (24, 8));
        let n = v.niche.expect("a niche");
        assert_eq!((n.offset, n.start), (16, 1u128 << 63));
        let one = enum_layout(&[vec![], vec![v]], &[0, 1], Repr::default()).unwrap();
        let two = enum_layout(&[vec![], vec![one]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(two.size, 24);
    }

    #[test]
    fn uninhabited_variants() {
        // enum { A(Never), B(u8) } is just the u8, with no tag; a
        // variant whose payload is a non-ZST uninhabited type is kept.
        let l = enum_layout(&[vec![never_layout()], ints(&[IntTy::U8])], &[0, 1], Repr::default()).unwrap();
        assert_eq!(sz(&l), (1, 1));
        assert_eq!(l.enum_tag, Some(EnumTag::None(1)));
        let pair = tuple_layout(&[int_layout(IntTy::U8), never_layout()]).unwrap();
        assert!(pair.uninhabited);
        let l = enum_layout(&[vec![pair], ints(&[IntTy::U8])], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 2);
        // The kept uninhabited variant needs no tag value: the tag's valid
        // range is 1..=1, so Option of it takes 0 (rustc 1.99).
        let o = enum_layout(&[vec![], vec![l]], &[0, 1], Repr::default()).unwrap();
        assert!(matches!(o.enum_tag, Some(EnumTag::NicheFilled { start: 0, .. })));
        // Every variant uninhabited: the enum itself is.
        let l = enum_layout(&[vec![never_layout()], vec![never_layout()]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 0);
        assert!(l.uninhabited);
    }

    #[test]
    fn most_free_values_niche() {
        // Payload { pointer: &u8, flag: bool } reports the bool niche (254 free),
        // so Data(Payload) plus two empty variants stays 16.
        let payload = struct_layout(&[ptr_layout(), bool_layout()], Repr::default()).unwrap();
        assert_eq!(payload.size, 16);
        assert_eq!(payload.niche.map(|n| n.free), Some(254));
        let l = enum_layout(&[vec![payload], vec![], vec![]], &[0, 1, 2], Repr::default()).unwrap();
        assert_eq!(l.size, 16);
        // Option<Option<S>> for S { a: bool, b: &u8 } is 16.
        let s = struct_layout(&[bool_layout(), ptr_layout()], Repr::default()).unwrap();
        let one = enum_layout(&[vec![], vec![s]], &[0, 1], Repr::default()).unwrap();
        let two = enum_layout(&[vec![], vec![one]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(two.size, 16);
    }

    #[test]
    fn tag_widening_and_discriminants() {
        // enum { A(u32), B(u32) } under three nested Options stays 8.
        let e = enum_layout(&[ints(&[IntTy::U32]), ints(&[IntTy::U32])], &[0, 1], Repr::default()).unwrap();
        assert_eq!(sz(&e), (8, 4));
        assert_eq!(e.enum_tag, Some(EnumTag::Explicit { size: 4, signed: false }));
        let o1 = enum_layout(&[vec![], vec![e]], &[0, 1], Repr::default()).unwrap();
        let o2 = enum_layout(&[vec![], vec![o1]], &[0, 1], Repr::default()).unwrap();
        let o3 = enum_layout(&[vec![], vec![o2]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(o3.size, 8);
        // enum { A = 1000, B } needs a 2-byte tag.
        let l = enum_layout(&[vec![], vec![]], &[1000, 1001], Repr::default()).unwrap();
        assert_eq!(sz(&l), (2, 2));
        // A negative discriminant forces a signed tag.
        let l = enum_layout(&[vec![], vec![]], &[-1, 0], Repr::default()).unwrap();
        assert_eq!(l.enum_tag, Some(EnumTag::Explicit { size: 1, signed: true }));
        // repr(u64) with few variants leaves a large niche for outer Options.
        let l = enum_layout(&[vec![], vec![], vec![]], &[0, 1, 2], Repr { int: Some((64, false)), ..Repr::default() }).unwrap();
        assert_eq!(sz(&l), (8, 8));
        let o = enum_layout(&[vec![], vec![l]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(o.size, 8);
    }

    #[test]
    fn repr_c_data_enum_is_tag_plus_union() {
        // #[repr(C)] enum { V0(i16, f64) }: tag 4, payload at 8, f64 at 16. 24.
        let l = enum_layout(
            &[vec![int_layout(IntTy::I16), float_layout(FloatTy::F64)]],
            &[0],
            Repr { kind: ReprKind::C, ..Repr::default() },
        )
        .unwrap();
        assert_eq!(sz(&l), (24, 8));
        assert_eq!(l.variant_offsets, vec![vec![8, 16]]);
        // #[repr(C, u8)] enum { V0(bool, u8), V1(Option<char>, u128, u128) } is 64.
        let opt_char = enum_layout(&[vec![], vec![char_layout()]], &[0, 1], Repr::default()).unwrap();
        let l = enum_layout(
            &[vec![bool_layout(), int_layout(IntTy::U8)], vec![opt_char, int_layout(IntTy::U128), int_layout(IntTy::U128)]],
            &[0, 1],
            Repr { kind: ReprKind::C, int: Some((8, false)), ..Repr::default() },
        )
        .unwrap();
        assert_eq!(sz(&l), (64, 16));
        assert_eq!(l.variant_offsets[1], vec![16, 32, 48]);
        // repr(u8) alone keeps the tag in each variant struct: same shape is 48.
        let opt_char = enum_layout(&[vec![], vec![char_layout()]], &[0, 1], Repr::default()).unwrap();
        let l = enum_layout(
            &[vec![bool_layout(), int_layout(IntTy::U8)], vec![opt_char, int_layout(IntTy::U128), int_layout(IntTy::U128)]],
            &[0, 1],
            Repr { int: Some((8, false)), ..Repr::default() },
        )
        .unwrap();
        assert_eq!(sz(&l), (48, 16));
    }

    #[test]
    fn array_carries_element_niche() {
        // Option<[bool; 2]> is 2 and Option<[char; 3]> is 12.
        let a = array_layout(&bool_layout(), 2).unwrap();
        let l = enum_layout(&[vec![], vec![a]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 2);
        let a = array_layout(&char_layout(), 3).unwrap();
        let l = enum_layout(&[vec![], vec![a]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 12);
        // An empty array carries no niche: Option<[bool; 0]> is 1 (the tag).
        let a = array_layout(&bool_layout(), 0).unwrap();
        assert_eq!(a.niche, None);
        let l = enum_layout(&[vec![], vec![a]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 1);
    }

    #[test]
    fn niche_fit_allows_align16_other_variant() {
        // enum { V0(u128), V1([u64; 2], (bool, u64)) } is 32, align 16.
        let arr = array_layout(&int_layout(IntTy::U64), 2).unwrap();
        let tup = struct_layout(&[bool_layout(), int_layout(IntTy::U64)], Repr::default()).unwrap();
        let l = enum_layout(&[vec![int_layout(IntTy::U128)], vec![arr, tup]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(sz(&l), (32, 16));
        assert!(matches!(l.enum_tag, Some(EnumTag::NicheFilled { .. })));
    }

    #[test]
    fn negative_discriminants_keep_tag_niche() {
        // Option of each of these fieldless enums is 1 byte.
        for discs in [&[-1i128, 127][..], &[-128, 127][..], &[0, 255][..], &[-100, 100][..]] {
            let e = enum_layout(&[vec![], vec![]], discs, Repr::default()).unwrap();
            assert_eq!(e.size, 1, "discs {discs:?}");
            let o = enum_layout(&[vec![], vec![e]], &[0, 1], Repr::default()).unwrap();
            assert_eq!(o.size, 1, "discs {discs:?}");
        }
        // The niche is the largest gap in bit order: {-100, 100} in one
        // byte leaves 199 free values (the small arc is 100..=156).
        let e = enum_layout(&[vec![], vec![]], &[-100, 100], Repr::default()).unwrap();
        assert_eq!(e.niche.map(|n| n.free), Some(199));
    }

    #[test]
    fn tuple_last_element_stays_last() {
        // (usize, Option<()>, usize) as layouts (u64, bool-sized enum, u64):
        // the middle element is not moved after the last one.
        let opt_unit = enum_layout(&[vec![], vec![zst_layout(1)]], &[0, 1], Repr::default()).unwrap();
        let t = tuple_layout(&[int_layout(IntTy::U64), opt_unit, int_layout(IntTy::U64)]).unwrap();
        assert_eq!(sz(&t), (24, 8));
        assert_eq!(t.field_offsets, vec![0, 8, 16]);
        // (u8, u32, u8, u64): the first three reorder, the u64 stays last.
        let t = tuple_layout(&ints(&[IntTy::U8, IntTy::U32, IntTy::U8, IntTy::U64])).unwrap();
        assert_eq!(t.field_offsets, vec![4, 0, 5, 8]);
        // The same fields as a struct reorder fully: u64 first.
        let s = struct_layout(&ints(&[IntTy::U8, IntTy::U32, IntTy::U8, IntTy::U64]), Repr::default()).unwrap();
        assert_eq!(s.field_offsets, vec![12, 8, 13, 0]);
        assert_eq!(sz(&t), sz(&s));
    }

    #[test]
    fn niche_fit_is_whole_variant_before_or_after() {
        // enum { A([bool; 4]), B(u8, u8, u8) } niche-fills by shifting
        // B's whole layout past the niche byte: offsets 1, 2, 3.
        let arr = array_layout(&bool_layout(), 4).unwrap();
        let l = enum_layout(&[vec![arr], ints(&[IntTy::U8, IntTy::U8, IntTy::U8])], &[0, 1], Repr::default()).unwrap();
        assert_eq!(l.size, 4);
        assert_eq!(l.variant_offsets[1], vec![1, 2, 3]);
        // The largest variant is the last on a size tie: { A((..32..)),
        // B(i128, char) } with both at 32 picks B, whose char niche
        // cannot hold A's tuple, so the enum stays tagged at 48.
        let oo = enum_layout(&[vec![], vec![enum_layout(&[vec![], vec![int_layout(IntTy::U64)]], &[0, 1], Repr::default()).unwrap()]], &[0, 1], Repr::default())
            .unwrap();
        let tup = tuple_layout(&[oo, int_layout(IntTy::Usize), array_layout(&int_layout(IntTy::U8), 5).unwrap()]).unwrap();
        let l = enum_layout(&[vec![tup], vec![int_layout(IntTy::I128), char_layout()]], &[0, 1], Repr::default()).unwrap();
        assert_eq!(sz(&l), (48, 16));
        assert!(matches!(l.enum_tag, Some(EnumTag::Explicit { .. })));
    }

    #[test]
    fn repr_align_applies_to_niche_variant_layouts() {
        // #[repr(align(4))] enum { V0, V1(u16, [i8; 0]), V2((Option<u16>,)) }
        // is 8: with the align applied per variant, V1 no longer fits.
        let opt16 = enum_layout(&[vec![], vec![int_layout(IntTy::U16)]], &[0, 1], Repr::default()).unwrap();
        let tup = tuple_layout(&[opt16]).unwrap();
        let arr0 = array_layout(&int_layout(IntTy::I8), 0).unwrap();
        let l = enum_layout(
            &[vec![], vec![int_layout(IntTy::U16), arr0], vec![tup]],
            &[0, 1, 2],
            Repr { align: Some(4), ..Repr::default() },
        )
        .unwrap();
        assert_eq!(sz(&l), (8, 4));
        // Without the align repr the same enum niche-fills to 4.
        let opt16 = enum_layout(&[vec![], vec![int_layout(IntTy::U16)]], &[0, 1], Repr::default()).unwrap();
        let tup = tuple_layout(&[opt16]).unwrap();
        let arr0 = array_layout(&int_layout(IntTy::I8), 0).unwrap();
        let l = enum_layout(&[vec![], vec![int_layout(IntTy::U16), arr0], vec![tup]], &[0, 1, 2], Repr::default()).unwrap();
        assert_eq!(sz(&l), (4, 2));
    }

    #[test]
    fn nested_type_data_shape() {
        // The 4.4 standalone shape, built from field layouts directly:
        // a repr(u64) memo enum gives its outer enum a niche-filled tag.
        // GenericArgumentsMemo-like: repr(u64) enum with 3 unit variants.
        let memo = enum_layout(&[vec![], vec![], vec![]], &[0, 1, 2], Repr { int: Some((64, false)), ..Repr::default() }).unwrap();
        // A payload struct that carries the memo plus a pointer word.
        let payload = struct_layout(&[memo, ptr_layout()], Repr::default()).unwrap();
        assert_eq!(payload.size, 16);
        // TypeData-like: Data(payload) plus two unit variants niche-fills into 16.
        let data = enum_layout(&[vec![payload], vec![], vec![]], &[0, 1, 2], Repr::default()).unwrap();
        assert_eq!(data.size, 16);
        // TypeReference-like: a plain struct of a thin and a fat pointer.
        let reference = struct_layout(&[ptr_layout(), fat_ptr_layout()], Repr::default()).unwrap();
        assert_eq!(reference.size, 24);
    }
}
