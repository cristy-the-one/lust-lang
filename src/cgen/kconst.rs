// Constant storage with rustc's addresses (runtime/kconst.c): equal
// constants share one address program-wide, as rustc gives them in one
// codegen unit. A promoted temp or byte string temp keeps its function
// static, and the place a borrow takes is the canonical copy (interned
// on the site's first run, then read from its slot); a function's str
// literals intern once, on its first call. The pointees of statics are the canonical storage of
// their content: the glue unit registers them before main
// (`lust_kstatics_init`), most by a hash computed here (`khash`), so startup does not read
// them. Comparing equal constants used from different
// fns gives a config-dependent answer in rustc (docs/ARCH.md, rule
// zero), so tests compare them only inside one fn.
use super::*;

/// The layout descriptor of a value of C type `t` (the words that
/// runtime/kconst.c reads), or None when every byte is init and no word
/// is a pointer.
pub(super) fn kdesc(p: &Program, t: &CTy) -> Option<Vec<u64>> {
    let mut ops = Vec::new();
    walk(p, t, 0, &mut ops);
    if let [Op::Bytes(0, n)] = ops[..]
        && n == c_size_align(p, t).0
    {
        return None;
    }
    let mut words = Vec::new();
    emit(&ops, &mut words);
    let mut d = vec![words.len() as u64];
    d.extend(words);
    Some(d)
}

/// A descriptor as a C initializer list.
pub(super) fn kdesc_init(d: &[u64]) -> String {
    let words: Vec<String> = d.iter().map(|w| format!("{w}ull")).collect();
    format!("{{{}}}", words.join(", "))
}

/// One descriptor op (runtime/kconst.c has the word forms).
#[derive(PartialEq)]
enum Op {
    /// Init bytes: offset, count.
    Bytes(u64, u64),
    /// A pointer word at the offset.
    Ptr(u64),
    /// An array: offset, element count, stride, the element's ops.
    Arr(u64, u64, u64, Vec<Op>),
    /// A tagged enum: offset, tag size, each variant's discriminant and ops.
    Tag(u64, u64, Vec<u128>, Vec<Vec<Op>>),
    /// A niche enum: offset, niche offset, niche size, niche start, the
    /// first niche variant, the span of niche variants, the dataful
    /// variant, each variant's ops.
    Niche(u64, u64, u64, u128, u64, u64, u64, Vec<Vec<Op>>),
}

/// The words of `ops`.
fn emit(ops: &[Op], w: &mut Vec<u64>) {
    let sub = |ops: &[Op], w: &mut Vec<u64>| {
        let mut s = Vec::new();
        emit(ops, &mut s);
        w.push(s.len() as u64);
        w.extend(s);
    };
    for op in ops {
        match op {
            Op::Bytes(off, n) => w.extend([1, *off, *n]),
            Op::Ptr(off) => w.extend([2, *off]),
            Op::Arr(off, count, stride, el) => {
                w.extend([3, *off, *count, *stride]);
                sub(el, w);
            }
            Op::Tag(off, size, discs, vars) => {
                w.extend([4, *off, *size, vars.len() as u64]);
                for &d in discs {
                    w.push(d as u64);
                    if *size == 16 {
                        w.push((d >> 64) as u64);
                    }
                }
                for v in vars {
                    sub(v, w);
                }
            }
            Op::Niche(off, noff, nsize, start, first, span, dataful, vars) => {
                w.extend([5, *off, *noff, *nsize, *start as u64]);
                if *nsize == 16 {
                    w.push((*start >> 64) as u64);
                }
                w.extend([*first, *span, *dataful, vars.len() as u64]);
                for v in vars {
                    sub(v, w);
                }
            }
        }
    }
}

/// Pushes `n` init bytes at `off`, joined to init bytes that end there.
fn init_bytes(out: &mut Vec<Op>, off: u64, n: u64) {
    if n == 0 {
        return;
    }
    if let Some(Op::Bytes(o, m)) = out.last_mut()
        && *o + *m == off
    {
        *m += n;
        return;
    }
    out.push(Op::Bytes(off, n));
}

/// The descriptor ops of a value of type `t` at offset `off`.
fn walk(p: &Program, t: &CTy, off: u64, out: &mut Vec<Op>) {
    let size = c_size_align(p, t).0;
    if size == 0 {
        return;
    }
    match t {
        CTy::Int(_) | CTy::Float(_) | CTy::Bool | CTy::Char => init_bytes(out, off, size),
        // A fat `&str` or `&[T]`: the data pointer, then the length.
        CTy::Ref(inner) if **inner == CTy::Str => {
            out.push(Op::Ptr(off));
            init_bytes(out, off + 8, 8);
        }
        CTy::Slice(_) => {
            out.push(Op::Ptr(off));
            init_bytes(out, off + 8, 8);
        }
        CTy::Ref(_) | CTy::FnPtr(..) | CTy::Ptr(_) => out.push(Op::Ptr(off)),
        // An empty `Vec::new()` or `String::new()` of a const value: its
        // dangling pointer, then the length and capacity.
        CTy::String | CTy::Vec(_) => {
            out.push(Op::Ptr(off));
            init_bytes(out, off + 8, 16);
        }
        CTy::Tuple(es) => match c_layout(p, t) {
            Some(l) => {
                for (e, fo) in es.iter().zip(&l.field_offsets) {
                    walk(p, e, off + fo, out);
                }
            }
            None => init_bytes(out, off, size),
        },
        CTy::Array(e, n) => {
            let es = c_size_align(p, e).0;
            let mut el = Vec::new();
            walk(p, e, 0, &mut el);
            if el == [Op::Bytes(0, es)] {
                init_bytes(out, off, es * n);
            } else if !el.is_empty() {
                out.push(Op::Arr(off, *n, es, el));
            }
        }
        CTy::Adt(a) if !p.adt(*a).is_enum => match c_layout(p, t) {
            Some(l) => {
                for (f, fo) in p.adt(*a).variants[0].fields.iter().zip(&l.field_offsets) {
                    walk(p, &cty(&p.tys, f.ty), off + fo, out);
                }
            }
            None => init_bytes(out, off, size),
        },
        CTy::Adt(_) | CTy::Option(_) | CTy::Result(..) => enum_ops(p, t, off, size, out),
        _ => init_bytes(out, off, size),
    }
}

/// The field types of each variant of an enum-like C type.
fn variant_fields(p: &Program, t: &CTy) -> Vec<Vec<CTy>> {
    match t {
        CTy::Option(e) => vec![vec![], vec![(**e).clone()]],
        CTy::Result(a, b) => vec![vec![(**a).clone()], vec![(**b).clone()]],
        CTy::Adt(a) => p.adt(*a).variants.iter().map(|v| v.fields.iter().map(|f| cty(&p.tys, f.ty)).collect()).collect(),
        _ => Vec::new(),
    }
}

/// The ops of an enum: its tag, then each variant's fields.
fn enum_ops(p: &Program, t: &CTy, off: u64, size: u64, out: &mut Vec<Op>) {
    let Some(l) = c_layout(p, t) else { return init_bytes(out, off, size) };
    let vars = variant_fields(p, t);
    let fields_at = |vi: usize, base: u64, out: &mut Vec<Op>| {
        for (f, fo) in vars[vi].iter().zip(&l.variant_offsets[vi]) {
            walk(p, f, base + fo, out);
        }
    };
    let subs = || {
        (0..vars.len())
            .map(|vi| {
                let mut s = Vec::new();
                fields_at(vi, 0, &mut s);
                s
            })
            .collect()
    };
    match l.enum_tag {
        Some(EnumTag::None(v)) => fields_at(v, off, out),
        Some(EnumTag::Explicit { size: ts, .. }) if matches!(ts, 1 | 2 | 4 | 8 | 16) => {
            // With no variant data, a value's bytes are its tag's (a tag op marks those and its
            // variant adds none): plain bytes, not a table of every discriminant (tsgo's
            // SyntaxKind has 352).
            let vars: Vec<Vec<Op>> = subs();
            if vars.iter().all(Vec::is_empty) {
                return init_bytes(out, off, ts);
            }
            let mask = scalar_mask(ts);
            out.push(Op::Tag(off, ts, adt_discs(p, t).iter().map(|&d| d as u128 & mask).collect(), vars));
        }
        Some(EnumTag::NicheFilled { variant, niche, first, last, start }) if matches!(niche.scalar_size, 1 | 2 | 4 | 8 | 16) => {
            let span = (last - first + 1) as u64;
            out.push(Op::Niche(off, niche.offset, niche.scalar_size, start, first as u64, span, variant as u64, subs()));
        }
        _ => init_bytes(out, off, size),
    }
}

impl FnGen<'_, '_> {
    /// True when local `i` is constant storage a borrow may observe: a
    /// promoted temp or byte string temp that is not zero-sized.
    pub(super) fn kconst_local(&self, i: usize) -> bool {
        (self.body.promoted.contains(&(i as Local)) || self.byte_blobs.get(i).is_some_and(|b| b.is_some()))
            && c_size_align(self.p, &self.local_cty[i]).0 > 0
    }

    /// True when constant local `i` is interned at the fn's entry,
    /// with its str literals (`kstr_finish`), not at each borrow: a
    /// release build's promoted local whose value codegen computed
    /// at compile time (`kstatic`), or a byte string temp (its static
    /// is initialized). Its borrows read the slot with no check.
    /// Identity is the run-time table's, as in dev builds; only the
    /// time of the interning moves.
    pub(super) fn kbatch_local(&self, i: usize) -> bool {
        names::release()
            && self.kconst_local(i)
            && (self.kstatic.contains_key(&(i as Local)) || self.byte_blobs.get(i).is_some_and(|b| b.is_some()))
    }

    /// The declaration of a promoted local `i` with a compile-time
    /// value (`kstatic`), None when `i` is not one. A value with no
    /// pointer is a static const with that C initializer. A value that
    /// holds strs is a zeroed static the fn's entry writes
    /// (`kentry`), with each str read from its canonical literal slot
    /// (`lks`), so the value interns as the dev path's does.
    pub(super) fn kstatic_decl(&mut self, i: usize) -> Option<String> {
        let cv = self.kstatic.get(&(i as Local))?.clone();
        let ct = self.local_cty[i].clone();
        if static_ok(&cv) {
            let init = const_init(self.p, &cv, &ct, &mut self.prelude, &mut self.kcx, self.slots.as_slice(), None);
            return Some(format!("  static const {} l{i} = {init};\n", ctype(&ct)));
        }
        self.kcx.lits = Some(std::mem::take(&mut self.kstrs));
        let init = const_init(self.p, &cv, &ct, &mut self.prelude, &mut self.kcx, self.slots.as_slice(), None);
        self.kstrs = self.kcx.lits.take().unwrap_or_default();
        self.kentry.insert(i as Local, init);
        Some(format!("  static {} l{i};\n", ctype(&ct)))
    }

    /// Release C with the C backend and statics in parts: true when every constant local
    /// has its value at compile time (`kbatch_local`) and an alignment of at most `KALIGN`.
    /// Then the fn interns nothing at run time: its constants and str literals are data
    /// named by content (`kdata_local`, `kstr`), as a static's pointees are
    /// (`content_name`), so equal content keeps one address with no first-run test.
    pub(super) fn kdata_ok(&self) -> bool {
        names::release()
            && !ll::on(self.p)
            && crate::driver::release_c::statics_bytes() > 0
            && (0..self.local_cty.len())
                .all(|i| !self.kconst_local(i) || (self.kbatch_local(i) && c_size_align(self.p, &self.local_cty[i]).1 <= KALIGN))
    }

    /// The content name of constant local `i` in a `kdata` fn, with its definition (and
    /// those of the strs its value holds) added to the fn's externs (`kdata_def`).
    pub(super) fn kdata_local(&mut self, i: usize) -> String {
        let ct = self.local_cty[i].clone();
        let c = ctype(&ct);
        let mut pre = String::new();
        let (init, key) = match self.byte_blobs.get(i).and_then(|b| b.as_ref()) {
            Some(b) => (format!("{{ .v = {} }}", c_bytes_lit(b.as_slice())), image_key(b.as_slice().to_vec(), vec![1; b.len()])),
            None => {
                let cv = self.kstatic[&(i as Local)].clone();
                let mut cx = StaticCx { weak: true, ..Default::default() };
                let init = const_init(self.p, &cv, &ct, &mut pre, &mut cx, self.slots.as_slice(), None);
                let key = static_key(self.p, &cx, &cv, &ct, &c, &init);
                (init, key)
            }
        };
        let name = content_sym(&key);
        pre.push_str(&kdata_def(&name, &format!("const {c} {name}"), &init));
        self.slots.externs.borrow_mut().insert(pre);
        name
    }

    /// The call that interns constant local `i` into its slot `lk{i}`.
    fn kintern_call(&self, i: usize) -> String {
        let lt = &self.local_cty[i];
        let (n, a) = c_size_align(self.p, lt);
        let blob = self.byte_blobs.get(i).is_some_and(|b| b.is_some());
        let desc = if !blob && kdesc(self.p, lt).is_some() { format!("lkd{i}") } else { "0".into() };
        format!("lust_kintern(&lk{i}, &l{i}, {n}, {}, {desc});", a.max(1))
    }

    /// The prologue lines of a constant local `i`: its site cache, and
    /// its layout descriptor when it has one.
    pub(super) fn kconst_decls(&self, i: usize) -> String {
        let mut out = format!("  static const void *lk{i};\n");
        if !self.byte_blobs.get(i).is_some_and(|b| b.is_some())
            && let Some(d) = kdesc(self.p, &self.local_cty[i])
        {
            let _ = writeln!(out, "  static const uint64_t lkd{i}[] = {};", kdesc_init(&d));
        }
        out
    }

    /// `place` for a borrow: the place inside the canonical copy of a
    /// constant local (no Deref on the way), else the place itself. It
    /// writes the site's interning statement first.
    pub(super) fn kplace(&mut self, pl: &Place, loc: &str) -> (String, CTy) {
        let (e, t) = self.place(pl, loc);
        let i = pl.local as usize;
        if pl.proj.iter().any(|pr| matches!(pr, Proj::Deref)) || !self.kconst_local(i) {
            return (e, t);
        }
        let base = format!("l{i}");
        let Some(rest) = e.strip_prefix(&base) else { return (e, t) };
        // The first run interns (a statement before the use); every run
        // then reads the cached address. A batched local's slot is set at
        // the fn's entry.
        if !self.kbatch_local(i) {
            let call = self.kintern_call(i);
            let _ = writeln!(self.out, "    if (!lk{i}) {call}");
        }
        (format!("(*({} *)lk{i}){rest}", ctype(&self.local_cty[i])), t)
    }

    /// A `&str` literal operand: its bytes interned, so equal literals
    /// share an address. The function's literals intern once, at its
    /// prologue's first run (`kstr_finish`); a use reads its slot.
    /// Outside a function body, the plain C literal.
    pub(super) fn kstr(&mut self, bytes: &[u8]) -> String {
        if bytes.is_empty() || self.kdecl_at.is_none() {
            return str_lit_c(bytes);
        }
        if self.kdata {
            let (mut pre, mut cx) = (String::new(), StaticCx { weak: true, ..Default::default() });
            let s = kstatic_str(&mut cx, &mut pre, bytes);
            self.slots.externs.borrow_mut().insert(pre);
            return s;
        }
        let n = self.kstrs.len();
        let id = *self.kstrs.entry(bytes.to_vec()).or_insert(n);
        format!("((lust_str){{(uint8_t*)lks[{id}], {}}})", bytes.len())
    }

    /// Inserts the slots of the literals `kstr` made, and their interning,
    /// at the prologue's end, with the batched constant locals
    /// (`kbatch_local`). Two threads may both intern on a first run:
    /// both get the same addresses.
    pub(super) fn kstr_finish(&mut self) {
        let Some(at) = self.kdecl_at.take() else { return };
        let text = self.kintern_block("  ");
        self.out.insert_str(at, &text);
        self.kstrs.clear();
    }

    /// The fn's first-run interning (`kstr_finish`; ll.rs puts it in a
    /// helper): the literal slots `lks`, the `lks_ok` flag, and one
    /// intern call per literal and batched local. Empty when it has none.
    pub(super) fn kintern_block(&self, ind: &str) -> String {
        if self.kdata {
            return String::new();
        }
        let batch: Vec<usize> = (0..self.local_cty.len()).filter(|&i| self.kbatch_local(i)).collect();
        if self.kstrs.is_empty() && batch.is_empty() {
            return String::new();
        }
        let mut lits: Vec<(&Vec<u8>, &usize)> = self.kstrs.iter().collect();
        lits.sort_by_key(|(_, id)| **id);
        let mut text = String::new();
        if !lits.is_empty() {
            let _ = writeln!(text, "{ind}static const void *lks[{}];", lits.len());
        }
        let _ = writeln!(text, "{ind}static int32_t lks_ok;\n{ind}if (!lks_ok) {{");
        for (b, id) in lits {
            let _ = writeln!(text, "{ind}  lust_kintern(&lks[{id}], {}, {}, 1, 0);", c_string_lit(b), b.len());
        }
        for i in batch {
            if let Some(init) = self.kentry.get(&(i as Local)) {
                let _ = writeln!(text, "{ind}  l{i} = {init};");
            }
            let _ = writeln!(text, "{ind}  {}", self.kintern_call(i));
        }
        let _ = writeln!(text, "{ind}  lks_ok = 1;\n{ind}}}");
        text
    }
}

/// The state of one `static_defs` pass: names for promoted values, the
/// pointee definitions so far by content key (`static_key`; equal
/// content gets one definition), the macro names that stand for an
/// earlier definition, and the pointees to register as canonical storage.
#[derive(Default)]
pub(super) struct StaticCx {
    pub counter: u32,
    /// The fn's str literal slots (`FnGen::kstrs`) while a promoted
    /// value is rendered for its entry (`kstatic_decl`): a str then
    /// reads its canonical slot `lks[id]`, not a static copy.
    pub lits: Option<HashMap<Vec<u8>, usize>>,
    defs: HashMap<Vec<u8>, String>,
    alias: HashMap<String, String>,
    /// The fn pointer trampolines (`stramp<tag>`) this pass's prelude defines.
    pub stramps: std::collections::HashSet<String>,
    /// The pointees to register (`kstatics_init`), in order: name, size, alignment, and how.
    regs: Vec<(String, u64, u64, KReg)>,
    /// Release C statics in parts (`names::statics_parts`): the pass defines its pointees, str
    /// arrays and trampolines global, so another part can name them, names an unnamed pointee
    /// by its content key (`content_name`), and keeps here, in order, each name it defines
    /// with its declaration (an extern declaration, a prototype, or the alias macro itself) and
    /// the C type the declaration names.
    pub parts: bool,
    pub decls: Vec<(String, String, Option<CTy>)>,
    /// A `kdata` fn's constant (`FnGen::kdata_local`): each str it holds is a definition
    /// in `pre` by content name (`kdata_def`), and nothing is registered.
    pub weak: bool,
}

/// How `lust_kstatics_init` registers a static pointee. `Hash`: its bytes are all init and hold
/// no pointer, and codegen knows them, so the call gives the run-time table's kind and hash of
/// its key (`khash`) and startup reads none of its bytes (`lust_kreg_h`). `Desc`: the table
/// reads the pointee for its key, by its layout descriptor (`kdesc`; None: every byte init).
enum KReg {
    Hash(u8, u64),
    Desc(Option<Vec<u64>>),
}

/// The registration of a pointee of type `ct` with byte image `img` (`image`).
fn kreg(p: &Program, ct: &CTy, img: Option<&(Vec<u8>, Vec<u8>)>) -> KReg {
    let desc = kdesc(p, ct);
    match img {
        Some((b, cls)) if desc.is_none() && cls.iter().all(|&c| c == 1) => khash(b),
        _ => KReg::Desc(desc),
    }
}

/// The registration of a pointee whose bytes `b` are all init and hold no pointer: the
/// run-time table's kind and hash of its key, as runtime/kconst.c computes them (`lust_kkey`
/// with no descriptor, the seed in `lust_kfind`, `lust_khash`). A 4, 8, 16 or 32 byte image
/// that is not a C string is kind 1, keyed by its bytes; any other is kind 0, keyed by its
/// bytes and then one init class (1) per byte.
fn khash(b: &[u8]) -> KReg {
    let n = b.len();
    let cstr = b.iter().position(|&x| x == 0) == Some(n - 1);
    let kind = !cstr && matches!(n, 4 | 8 | 16 | 32);
    let mut key = b.to_vec();
    if !kind {
        key.resize(2 * n, 1);
    }
    let mut h = 0xcbf2_9ce4_8422_2325 ^ n as u64 ^ (u64::from(kind) << 63);
    let mut words = key.chunks_exact(8);
    for w in &mut words {
        h = (h ^ u64::from_le_bytes(w.try_into().unwrap())).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        h ^= h >> 32;
    }
    for &x in words.remainder() {
        h = (h ^ u64::from(x)).wrapping_mul(0x1_0000_0001_b3);
    }
    h ^= h >> 29;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    KReg::Hash(u8::from(kind), h ^ (h >> 32))
}

/// The symbol of constant data with content key `key` (`static_key`): a static's pointee or
/// str array in parts, or a `kdata` fn's constant. One name per content, whatever the C type.
pub(super) fn content_sym(key: &[u8]) -> String {
    let (a, b) = crate::driver::cache::hash128(&[key], 0);
    format!("lust_kc{a:016x}{b:016x}")
}

/// The definition of `kdata` constant `name` (`decl` names its C type): each unit that uses
/// it defines it (`LUST_KDATA`, runtime/inl.h: weak, or selectany on Windows), so the link
/// keeps one and equal content keeps one address; the guard keeps one per unit, where
/// copies and the fns of a unit may share it.
fn kdata_def(name: &str, decl: &str, init: &str) -> String {
    format!("#ifndef K{name}\n#define K{name}\nLUST_KDATA {decl} __attribute__((aligned({KALIGN}))) = {init};\n#endif\n")
}

/// Every static pointee is at least 16-aligned, so a site of a stricter
/// type finds it (rustc raises a shared constant's alignment to its
/// strictest use).
const KALIGN: u64 = 16;

/// Defines the static pointee `name` (value `v` at type `ct`, C
/// initializer `init`) in `pre`, or makes `name` a macro for an earlier
/// pointee with equal content.
pub(super) fn kdef(p: &Program, cx: &mut StaticCx, pre: &mut String, name: &str, v: &crate::ty::consteval::ConstValue, ct: &CTy, init: &str) {
    let c = ctype(ct);
    let (n, a) = c_size_align(p, ct);
    let link = if cx.parts { "" } else { "static " };
    if n == 0 {
        // A content name may come again (`content_name`): one definition.
        if cx.parts && cx.decls.iter().any(|(d, _, _)| d == name) {
            return;
        }
        let _ = writeln!(pre, "{link}const {c} {name} = {init};");
        if cx.parts {
            cx.decls.push((name.to_string(), format!("extern const {c} {name};\n"), Some(ct.clone())));
        }
        return;
    }
    let img = image(p, v, ct);
    let key = match &img {
        Some((b, cls)) => image_key(b.clone(), cls.clone()),
        None => init_key(cx, &c, init),
    };
    // Statics in parts: the data has its content's name (`content_sym`), as an equal
    // constant of a `kdata` fn does, and a path name (`promoted_name`) is a macro for it.
    if cx.parts && !cx.defs.contains_key(&key) {
        let cname = content_sym(&key);
        let _ = writeln!(pre, "const {c} {cname} __attribute__((aligned({}))) = {init};", KALIGN.max(a));
        cx.regs.push((cname.clone(), n, KALIGN.max(a), kreg(p, ct, img.as_ref())));
        cx.decls.push((cname.clone(), format!("extern const {c} {cname};\n"), Some(ct.clone())));
        cx.defs.insert(key.clone(), cname);
    }
    if let Some(prev) = cx.defs.get(&key) {
        if prev == name {
            return;
        }
        let line = format!("#define {name} (*({c} *)&{prev})\n");
        pre.push_str(&line);
        cx.alias.insert(name.to_string(), prev.clone());
        if cx.parts {
            cx.decls.push((name.to_string(), line, Some(ct.clone())));
        }
        return;
    }
    cx.defs.insert(key, name.to_string());
    let _ = writeln!(pre, "{link}const {c} {name} __attribute__((aligned({}))) = {init};", KALIGN.max(a));
    cx.regs.push((name.to_string(), n, KALIGN.max(a), kreg(p, ct, img.as_ref())));
    if cx.parts {
        cx.decls.push((name.to_string(), format!("extern const {c} {name};\n"), Some(ct.clone())));
    }
}

/// The name of an unnamed pointee in release C statics in parts (`StaticCx::parts`): its content
/// key's (`static_key`), so it does not move when a pointee before it in the pass comes or goes.
pub(super) fn content_name(p: &Program, cx: &StaticCx, v: &crate::ty::consteval::ConstValue, ct: &CTy, init: &str) -> String {
    content_sym(&static_key(p, cx, v, ct, &ctype(ct), init))
}

/// A `&str` in static data: its bytes in one registered array per
/// distinct content (shared with an equal byte array pointee).
pub(super) fn kstatic_str(cx: &mut StaticCx, pre: &mut String, bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return str_lit_c(bytes);
    }
    if let Some(lits) = &mut cx.lits {
        let n = lits.len();
        let id = *lits.entry(bytes.to_vec()).or_insert(n);
        return format!("((lust_str){{(uint8_t*)lks[{id}], {}}})", bytes.len());
    }
    let key = image_key(bytes.to_vec(), vec![1; bytes.len()]);
    if cx.weak {
        let name = content_sym(&key);
        pre.push_str(&kdata_def(&name, &format!("const uint8_t {name}[{}]", bytes.len()), &c_string_lit(bytes)));
        return format!("((lust_str){{(uint8_t*)&{name}, {}}})", bytes.len());
    }
    let name = match cx.defs.get(&key) {
        Some(n) => n.clone(),
        None => {
            let name = if cx.parts { content_sym(&key) } else { format!("lust_ks{}", cx.regs.len()) };
            let link = if cx.parts { "" } else { "static " };
            let _ = writeln!(pre, "{link}const uint8_t {name}[{}] __attribute__((aligned({KALIGN}))) = {};", bytes.len(), c_string_lit(bytes));
            cx.regs.push((name.clone(), bytes.len() as u64, KALIGN, khash(bytes)));
            cx.defs.insert(key, name.clone());
            if cx.parts {
                cx.decls.push((name.clone(), format!("extern const uint8_t {name}[{}];\n", bytes.len()), None));
            }
            name
        }
    };
    format!("((lust_str){{(uint8_t*)&{name}, {}}})", bytes.len())
}

/// The content key of a static pointee: its byte image under rustc's
/// rule (`image_key`) when the value holds no pointer, else its C type
/// and initializer, with alias names read as the names they stand for.
fn static_key(p: &Program, cx: &StaticCx, v: &crate::ty::consteval::ConstValue, ct: &CTy, c: &str, init: &str) -> Vec<u8> {
    match image(p, v, ct) {
        Some((b, cls)) => image_key(b, cls),
        None => init_key(cx, c, init),
    }
}

/// The byte image of const value `v` at type `ct` (`put`): its bytes and their init classes.
fn image(p: &Program, v: &crate::ty::consteval::ConstValue, ct: &CTy) -> Option<(Vec<u8>, Vec<u8>)> {
    let n = c_size_align(p, ct).0 as usize;
    let (mut b, mut cls) = (vec![0u8; n], vec![0u8; n]);
    put(p, v, ct, 0, &mut b, &mut cls)?;
    Some((b, cls))
}

/// The content key of a value with no byte image (`static_key`): its C type `c` and
/// initializer, with alias names read as the names they stand for.
fn init_key(cx: &StaticCx, c: &str, init: &str) -> Vec<u8> {
    let mut key = format!("T{c}=");
    let mut word = String::new();
    for ch in init.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
            continue;
        }
        key.push_str(cx.alias.get(&word).unwrap_or(&word));
        word.clear();
        key.push(ch);
    }
    key.into_bytes()
}

/// rustc's equality of constants as a key (runtime/kconst.c `lust_kkey`
/// does the same at run time): a 4, 8, 16 or 32 byte image is its
/// bytes, unless it is a C string (all init, one zero byte, at the end;
/// LLVM puts it in a string section); any other is its bytes and init
/// classes, where more than 16 init and uninit chunks make all of it init.
/// On Windows the byte-only key is not used: link.exe does not merge
/// `.rdata` the way lld merges `.rodata.cstN`, so an uninit byte stays
/// in the key. Equal C strings still share one key.
fn image_key(b: Vec<u8>, mut cls: Vec<u8>) -> Vec<u8> {
    let n = b.len();
    let cstr = cls.iter().all(|&c| c == 1) && b.iter().position(|&x| x == 0) == Some(n - 1);
    let mut key = vec![b'B'];
    key.extend((n as u64).to_le_bytes());
    key.extend(&b);
    if !cfg!(windows) && matches!(n, 4 | 8 | 16 | 32) && !cstr {
        return key;
    }
    let chunks = 1 + cls.windows(2).filter(|w| w[0] != w[1]).count();
    if chunks > 16 {
        cls.iter_mut().for_each(|c| *c = 1);
    }
    key.extend(cls);
    key
}

/// Writes the byte image of const value `v` at type `t` at offset `off`
/// (`cls` 1 for an init byte). None for a value with a pointer or a
/// shape this does not lay out.
fn put(p: &Program, v: &crate::ty::consteval::ConstValue, t: &CTy, off: usize, b: &mut [u8], cls: &mut [u8]) -> Option<()> {
    use crate::ty::consteval::ConstValue as CV;
    let mut bytes = |at: usize, val: u128, n: usize, b: &mut [u8], cls: &mut [u8]| {
        for k in 0..n {
            b[at + k] = (val >> (8 * k)) as u8;
            cls[at + k] = 1;
        }
    };
    let size = c_size_align(p, t).0 as usize;
    match (v, t) {
        (_, CTy::Cell(inner)) => put(p, v, inner, off, b, cls),
        (CV::Int(i), CTy::Int(_)) => Some(bytes(off, *i as u128, size, b, cls)),
        (CV::Float(bits, _), CTy::Float(_)) => Some(bytes(off, *bits as u128, size, b, cls)),
        (CV::Bool(x), CTy::Bool) => Some(bytes(off, *x as u128, 1, b, cls)),
        (CV::Char(ch), CTy::Char) => Some(bytes(off, *ch as u128, 4, b, cls)),
        (CV::Unit, _) if size == 0 => Some(()),
        (CV::Bytes(bs), CTy::Array(..)) => {
            for (k, x) in bs.as_slice().iter().enumerate() {
                bytes(off + k, *x as u128, 1, b, cls);
            }
            Some(())
        }
        (CV::Aggregate(vals), CTy::Array(e, _)) => {
            let es = c_size_align(p, e).0 as usize;
            vals.iter().enumerate().try_for_each(|(k, x)| put(p, x, e, off + k * es, b, cls))
        }
        (CV::Repeat(x, n), CTy::Array(e, _)) => {
            let es = c_size_align(p, e).0 as usize;
            (0..*n as usize).try_for_each(|k| put(p, x, e, off + k * es, b, cls))
        }
        (CV::Aggregate(vals), CTy::Tuple(es)) => {
            let l = c_layout(p, t)?;
            vals.iter().zip(es).zip(&l.field_offsets).try_for_each(|((x, e), fo)| put(p, x, e, off + *fo as usize, b, cls))
        }
        (CV::Aggregate(vals), CTy::Adt(a)) if !p.adt(*a).is_enum => {
            let l = c_layout(p, t)?;
            let fields = &p.adt(*a).variants[0].fields;
            vals.iter().zip(fields).zip(&l.field_offsets).try_for_each(|((x, f), fo)| put(p, x, &cty(&p.tys, f.ty), off + *fo as usize, b, cls))
        }
        (CV::Variant(vi, vals), CTy::Adt(_) | CTy::Option(_) | CTy::Result(..)) => {
            let l = c_layout(p, t)?;
            let vi = *vi as usize;
            let vars = variant_fields(p, t);
            match l.enum_tag? {
                EnumTag::None(_) => {}
                EnumTag::Explicit { size: ts, .. } => bytes(off, adt_discs(p, t)[vi] as u128, ts as usize, b, cls),
                EnumTag::NicheFilled { variant, niche, first, start, .. } => {
                    if vi != variant {
                        bytes(off + niche.offset as usize, niche_tag_value(niche, first, start, vi), niche.scalar_size as usize, b, cls);
                    }
                }
            }
            vals.iter().zip(vars.get(vi)?).zip(l.variant_offsets.get(vi)?).try_for_each(|((x, f), fo)| put(p, x, f, off + *fo as usize, b, cls))
        }
        _ => None,
    }
}

/// The pointees `kstatics_init` registers, in its order.
pub(super) fn kstatics_regs(cx: &StaticCx) -> impl Iterator<Item = &str> {
    cx.regs.iter().map(|(name, ..)| name.as_str())
}

/// `lust_kstatics_init`, which registers the static pointees in
/// definition order (an inner pointee before the values that point at
/// it, so their pointer words read as relocs), each as its `KReg` says.
pub(super) fn kstatics_init(cx: &StaticCx) -> String {
    let mut out = String::new();
    for (i, (_, _, _, r)) in cx.regs.iter().enumerate() {
        if let KReg::Desc(Some(d)) = r {
            let _ = writeln!(out, "static const uint64_t lust_ksd{i}[] = {};", kdesc_init(d));
        }
    }
    out.push_str("void lust_kstatics_init(void) {\n");
    for (i, (name, n, a, r)) in cx.regs.iter().enumerate() {
        let _ = match r {
            KReg::Hash(kind, h) => writeln!(out, "  lust_kreg_h(&{name}, {n}, {a}, {kind}, {h:#x}ull);"),
            KReg::Desc(Some(_)) => writeln!(out, "  lust_kreg(&{name}, {n}, {a}, lust_ksd{i});"),
            KReg::Desc(None) => writeln!(out, "  lust_kreg(&{name}, {n}, {a}, 0);"),
        };
    }
    out.push_str("}\n");
    out
}

/// The promoted locals of `body` whose value this evaluator can
/// compute at compile time, as ConstValues (release builds give
/// those a static initializer and intern them at the fn's entry,
/// `kbatch_local`). A local's value is read only when the local is
/// a temp written by exactly one whole-local assignment, never
/// through a projection, a call, or a mutable borrow
/// (`single_defs`), so a value read is the only value it can hold.
/// One forward pass in block order; a definition that order has not
/// reached is a miss. Literals, aggregates, field and index reads,
/// and scalar arithmetic and casts evaluate; integers are kept in
/// their type's range (wrapped at its width), and an operation
/// that overflows, panics, or that the evaluator does not know is
/// a miss: the local keeps the run-time path, never a wrong value.
pub(super) fn eval_promoted(p: &Program, body: &Body) -> crate::intern::FxMap<Local, crate::ty::consteval::ConstValue> {
    let single = single_defs(body);
    let mut env: crate::intern::FxMap<Local, crate::ty::consteval::ConstValue> = crate::intern::FxMap::default();
    for block in &body.blocks {
        for stmt in &block.stmts {
            let StmtKind::Assign(dest, rv) = &stmt.kind else { continue };
            if !dest.proj.is_empty() || !single[dest.local as usize] {
                continue;
            }
            let ty = body.locals[dest.local as usize].ty;
            if let Some(v) = eval_rvalue(p, body, rv, &env).and_then(|v| fit(v, int_ty(body, ty))) {
                env.insert(dest.local, v);
            }
        }
    }
    env.retain(|l, _| body.promoted.contains(l));
    env
}

/// True when the assignment that defines promoted local `l` moves a value that needs
/// drop (`&Some(D(2))` moves its `D` temp in). Skipping that assignment (`kstatic`)
/// would skip the move too, and a dynamic drop flag (the LLVM backend's) would then drop
/// the temp at its scope end: such a local keeps the run-time path.
pub(super) fn moves_drop_value(p: &Program, body: &Body, local_cty: &[CTy], l: Local) -> bool {
    let moved = |op: &Operand| matches!(op, Operand::Move(pl) if needs_drop_c(p, &local_cty[pl.local as usize]));
    body.blocks.iter().flat_map(|b| &b.stmts).any(|s| match &s.kind {
        StmtKind::Assign(d, rv) if d.local == l => match rv {
            Rvalue::Use(op) | Rvalue::ArrayRepeat(_, op, _) | Rvalue::Res(_, _, op) | Rvalue::Un(_, op) | Rvalue::Cast(op, _) => moved(op),
            Rvalue::Opt(_, x) => x.as_ref().is_some_and(moved),
            Rvalue::Aggregate(_, _, ops) | Rvalue::TupleAgg(_, ops) | Rvalue::ArrayAgg(_, ops) => ops.iter().any(moved),
            Rvalue::Bin(_, a, b) => moved(a) || moved(b),
            _ => false,
        },
        _ => false,
    })
}

/// Per local: true for an unnamed non-parameter temp with exactly one
/// whole-local assignment and no other write (a projection write, a
/// call result, a mutable borrow), as `ir::byte_blob_locals` asks.
fn single_defs(body: &Body) -> Vec<bool> {
    let n = body.locals.len();
    let mut count = vec![0u32; n];
    let mut bad: Vec<bool> = body.locals.iter().enumerate().map(|(i, l)| i <= body.arg_count || l.name.is_some()).collect();
    for b in &body.blocks {
        for s in &b.stmts {
            let StmtKind::Assign(dest, rv) = &s.kind else { continue };
            match rv {
                Rvalue::Ref(true, pl) => bad[pl.local as usize] = true,
                Rvalue::Subslice { base, mutable: true, .. } => bad[base.local as usize] = true,
                _ => {}
            }
            if dest.proj.is_empty() {
                count[dest.local as usize] += 1;
            } else {
                bad[dest.local as usize] = true;
            }
        }
        if let Terminator::Call { dest, .. } = &b.term {
            bad[dest.local as usize] = true;
        }
    }
    (0..n).map(|i| !bad[i] && count[i] == 1).collect()
}

/// True when a compile-time value can be a static initializer that
/// interns like the run-time value (`kbatch_local`): no float
/// (`const_init` renders one as a union read, not a C constant) and
/// no pointer (a pointer word would name a copy of its pointee, not
/// the canonical one the run-time value points at).
pub(super) fn static_ok(v: &crate::ty::consteval::ConstValue) -> bool {
    kvalue_ok(v, false)
}

/// True when a compile-time value can intern at the fn's entry: as
/// `static_ok`, or with strs as its only pointers (the entry builds
/// it from the canonical literal slots, `kstatic_decl`).
pub(super) fn entry_ok(v: &crate::ty::consteval::ConstValue) -> bool {
    kvalue_ok(v, true)
}

fn kvalue_ok(v: &crate::ty::consteval::ConstValue, strs: bool) -> bool {
    use crate::ty::consteval::ConstValue as CV;
    match v {
        CV::Int(_) | CV::Bool(_) | CV::Char(_) | CV::Unit | CV::Bytes(_) => true,
        CV::Str(_) | CV::StrBytes(_) => strs,
        CV::Aggregate(vals) | CV::Variant(_, vals) => vals.iter().all(|x| kvalue_ok(x, strs)),
        CV::Repeat(x, _) => kvalue_ok(x, strs),
        _ => false,
    }
}

/// The integer type of `ty`, if it is one.
fn int_ty(body: &Body, ty: Ty) -> Option<crate::ty::IntTy> {
    match body.tys.kind(ty) {
        TyKind::Int(it) => Some(it),
        _ => None,
    }
}

/// `i` wrapped to `it`'s width: sign-extended for a signed type,
/// zero-extended for an unsigned one (the form every Int here has).
fn wrap(i: i128, it: crate::ty::IntTy) -> i128 {
    let bits = it.bits();
    if bits >= 128 {
        return i;
    }
    let m = (i as u128) & ((1u128 << bits) - 1);
    if it.signed() && (m >> (bits - 1)) & 1 == 1 { (m as i128) - (1i128 << bits) } else { m as i128 }
}

/// A value stored at a local of integer type `it`: an Int must already
/// be in its range (else the operation overflowed: a miss). 128-bit
/// types are a miss (u128 does not fit the i128 form).
fn fit(v: crate::ty::consteval::ConstValue, it: Option<crate::ty::IntTy>) -> Option<crate::ty::consteval::ConstValue> {
    use crate::ty::consteval::ConstValue as CV;
    match (&v, it) {
        (_, Some(it)) if it.bits() >= 128 => None,
        (CV::Int(i), Some(it)) if wrap(*i, it) != *i => None,
        _ => Some(v),
    }
}

/// The integer type of an operand: a literal's, or a whole local's.
/// None for anything else (arithmetic on it is a miss).
fn op_int_ty(body: &Body, op: &Operand) -> Option<crate::ty::IntTy> {
    match op {
        Operand::Const(Const::Int(_, it)) => Some(*it),
        Operand::Copy(pl) | Operand::Move(pl) if pl.proj.is_empty() => int_ty(body, body.locals[pl.local as usize].ty),
        _ => None,
    }
}

/// One operand's const value, if it has one under `env`.
fn eval_operand(p: &Program, body: &Body, op: &Operand, env: &crate::intern::FxMap<Local, crate::ty::consteval::ConstValue>) -> Option<crate::ty::consteval::ConstValue> {
    use crate::ty::consteval::ConstValue as CV;
    match op {
        Operand::Const(c) => Some(match c {
            Const::Int(v, it) => {
                if it.bits() >= 128 {
                    return None;
                }
                CV::Int(wrap(*v as i128, *it))
            }
            Const::Float(bits, f) => CV::Float(*bits, *f),
            Const::Bool(b) => CV::Bool(*b),
            Const::Char(ch) => CV::Char(*ch),
            Const::Str(i) => CV::Str(*i),
            Const::StrBytes(b) => CV::StrBytes(b.clone()),
            Const::Bytes(b) => CV::Bytes(b.clone()),
            Const::Unit => CV::Unit,
        }),
        Operand::Copy(pl) | Operand::Move(pl) => eval_place(p, body, pl, env),
    }
}

/// A place's const value: a whole local, a field of a struct or of the
/// variant a downcast names, an element at a known index, or a deref
/// of a reference value.
fn eval_place(p: &Program, body: &Body, pl: &Place, env: &crate::intern::FxMap<Local, crate::ty::consteval::ConstValue>) -> Option<crate::ty::consteval::ConstValue> {
    use crate::ty::consteval::ConstValue as CV;
    let _ = (p, body);
    let mut v = env.get(&pl.local)?.clone();
    let mut variant: Option<u32> = None;
    for proj in &pl.proj {
        let at = |i: &Local| match env.get(i) {
            Some(CV::Int(k)) if *k >= 0 => Some(*k as usize),
            _ => None,
        };
        v = match (proj, v) {
            (Proj::Downcast(k), v @ CV::Variant(..)) => {
                variant = Some(*k);
                v
            }
            (Proj::Field(i), CV::Aggregate(mut vals)) if variant.is_none() && (*i as usize) < vals.len() => vals.swap_remove(*i as usize),
            (Proj::Field(i), CV::Variant(k, mut vals)) if variant == Some(k) && (*i as usize) < vals.len() => vals.swap_remove(*i as usize),
            (Proj::Deref, CV::Ref(inner)) if variant.is_none() => *inner,
            (Proj::Index(idx), CV::Aggregate(vals)) if variant.is_none() => vals.get(at(idx)?)?.clone(),
            (Proj::Index(idx), CV::Repeat(x, n)) if variant.is_none() && (at(idx)? as u64) < n => *x,
            (Proj::Index(idx), CV::Bytes(b)) if variant.is_none() => CV::Int(*b.as_slice().get(at(idx)?)? as i128),
            _ => return None,
        };
        if !matches!(proj, Proj::Downcast(_)) {
            variant = None;
        }
    }
    if variant.is_some() {
        return None;
    }
    Some(v)
}

/// An rvalue's const value, for the forms promotion allows
/// (literals, aggregates, references, casts, scalar arithmetic).
fn eval_rvalue(p: &Program, body: &Body, rv: &Rvalue, env: &crate::intern::FxMap<Local, crate::ty::consteval::ConstValue>) -> Option<crate::ty::consteval::ConstValue> {
    use crate::ty::consteval::ConstValue as CV;
    let ev = |op: &Operand| eval_operand(p, body, op, env);
    let all = |ops: &[Operand]| ops.iter().map(ev).collect::<Option<Vec<CV>>>();
    match rv {
        Rvalue::Use(op) => ev(op),
        Rvalue::Ref(false, pl) => Some(CV::Ref(Box::new(eval_place(p, body, pl, env)?))),
        Rvalue::Aggregate(a, variant, ops) => {
            let vals = all(ops)?;
            Some(if p.adt(*a).is_enum { CV::Variant(*variant, vals) } else { CV::Aggregate(vals) })
        }
        Rvalue::TupleAgg(_, ops) | Rvalue::ArrayAgg(_, ops) => Some(CV::Aggregate(all(ops)?)),
        Rvalue::ArrayRepeat(_, op, n) => Some(CV::Repeat(Box::new(ev(op)?), *n)),
        Rvalue::Opt(_, x) => match x {
            Some(op) => Some(CV::Variant(1, vec![ev(op)?])),
            None => Some(CV::Variant(0, Vec::new())),
        },
        Rvalue::Res(_, ok, op) => Some(CV::Variant(if *ok { 0 } else { 1 }, vec![ev(op)?])),
        Rvalue::Bin(op, a, b) => eval_binop(body, *op, a, b, ev(a)?, ev(b)?),
        Rvalue::Un(op, a) => {
            let it = op_int_ty(body, a);
            match (op, ev(a)?) {
                (crate::ast::UnOp::Neg, CV::Int(i)) if it.is_some_and(|t| t.signed()) => Some(CV::Int(i.checked_neg()?)),
                (crate::ast::UnOp::Neg, CV::Float(bits, f)) => Some(CV::Float(neg_float(bits, f), f)),
                (crate::ast::UnOp::Not, CV::Bool(b)) => Some(CV::Bool(!b)),
                (crate::ast::UnOp::Not, CV::Int(i)) => Some(CV::Int(wrap(!i, it?))),
                _ => None,
            }
        }
        Rvalue::Cast(op, ty) => eval_cast(body, ev(op)?, *ty),
        _ => None,
    }
}

/// Scalar binary ops on const values. Integer operands must have a
/// known type of at most 64 bits (`op_int_ty`); the result of `+ - *
/// / %` must fit the left operand's type (`fit` at the destination
/// checks it), and a shift amount must be under the width. Floats go
/// through host IEEE arithmetic at their own width, as Rust's.
fn eval_binop(
    body: &Body,
    op: crate::ast::BinOp,
    a: &Operand,
    b: &Operand,
    x: crate::ty::consteval::ConstValue,
    y: crate::ty::consteval::ConstValue,
) -> Option<crate::ty::consteval::ConstValue> {
    use crate::ast::BinOp as B;
    use crate::ty::consteval::ConstValue as CV;
    match (x, y) {
        (CV::Int(i), CV::Int(j)) => {
            let lt = op_int_ty(body, a)?;
            op_int_ty(body, b)?;
            let width = lt.bits() as i128;
            Some(match op {
                B::Add => CV::Int(i.checked_add(j)?),
                B::Sub => CV::Int(i.checked_sub(j)?),
                B::Mul => CV::Int(i.checked_mul(j)?),
                B::Div if j != 0 => CV::Int(i.checked_div(j)?),
                B::Rem if j != 0 => CV::Int(i.checked_rem(j)?),
                B::BitAnd => CV::Int(i & j),
                B::BitOr => CV::Int(i | j),
                B::BitXor => CV::Int(i ^ j),
                B::Shl if (0..width).contains(&j) => CV::Int(wrap(i << j, lt)),
                B::Shr if (0..width).contains(&j) => CV::Int(i >> j),
                B::Eq => CV::Bool(i == j),
                B::Ne => CV::Bool(i != j),
                B::Lt => CV::Bool(i < j),
                B::Le => CV::Bool(i <= j),
                B::Gt => CV::Bool(i > j),
                B::Ge => CV::Bool(i >= j),
                _ => return None,
            })
        }
        (CV::Float(i, f), CV::Float(j, g)) if f == g => {
            macro_rules! arith {
                ($x:expr, $y:expr, $bits:expr) => {{
                    let (x, y) = ($x, $y);
                    Some(match op {
                        B::Add => CV::Float($bits(x + y), f),
                        B::Sub => CV::Float($bits(x - y), f),
                        B::Mul => CV::Float($bits(x * y), f),
                        B::Div => CV::Float($bits(x / y), f),
                        B::Rem => CV::Float($bits(x % y), f),
                        B::Eq => CV::Bool(x == y),
                        B::Ne => CV::Bool(x != y),
                        B::Lt => CV::Bool(x < y),
                        B::Le => CV::Bool(x <= y),
                        B::Gt => CV::Bool(x > y),
                        B::Ge => CV::Bool(x >= y),
                        _ => return None,
                    })
                }};
            }
            match f {
                crate::ty::FloatTy::F64 => arith!(f64::from_bits(i), f64::from_bits(j), |r: f64| r.to_bits()),
                crate::ty::FloatTy::F32 => arith!(f32::from_bits(i as u32), f32::from_bits(j as u32), |r: f32| r.to_bits() as u64),
            }
        }
        (CV::Bool(i), CV::Bool(j)) => Some(match op {
            B::Eq => CV::Bool(i == j),
            B::Ne => CV::Bool(i != j),
            B::BitAnd => CV::Bool(i & j),
            B::BitOr => CV::Bool(i | j),
            B::BitXor => CV::Bool(i ^ j),
            _ => return None,
        }),
        (CV::Char(i), CV::Char(j)) => Some(match op {
            B::Eq => CV::Bool(i == j),
            B::Ne => CV::Bool(i != j),
            B::Lt => CV::Bool(i < j),
            B::Le => CV::Bool(i <= j),
            B::Gt => CV::Bool(i > j),
            B::Ge => CV::Bool(i >= j),
            _ => return None,
        }),
        _ => None,
    }
}

fn neg_float(bits: u64, f: crate::ty::FloatTy) -> u64 {
    match f {
        crate::ty::FloatTy::F64 => (-f64::from_bits(bits)).to_bits(),
        crate::ty::FloatTy::F32 => (-f32::from_bits(bits as u32)).to_bits() as u64,
    }
}

/// Numeric casts between scalar const values, as Rust's `as`:
/// wrapping int to int, saturating float to int (NaN is 0), round to
/// nearest int to float and float to float. Anything else is a miss.
fn eval_cast(body: &Body, v: crate::ty::consteval::ConstValue, dst: Ty) -> Option<crate::ty::consteval::ConstValue> {
    use crate::ty::FloatTy as F;
    use crate::ty::consteval::ConstValue as CV;
    match (v, body.tys.kind(dst)) {
        (_, TyKind::Int(it)) if it.bits() >= 128 => None,
        (CV::Int(i), TyKind::Int(it)) => Some(CV::Int(wrap(i, it))),
        (CV::Char(c), TyKind::Int(it)) => Some(CV::Int(wrap(c as i128, it))),
        (CV::Bool(b), TyKind::Int(_)) => Some(CV::Int(b as i128)),
        (CV::Int(i), TyKind::Char) if (0..256).contains(&i) => char::from_u32(i as u32).map(CV::Char),
        (CV::Int(i), TyKind::Float(F::F64)) => Some(CV::Float((i as f64).to_bits(), F::F64)),
        (CV::Int(i), TyKind::Float(F::F32)) => Some(CV::Float((i as f32).to_bits() as u64, F::F32)),
        (CV::Float(bits, f), TyKind::Int(it)) => {
            let x = match f {
                F::F64 => f64::from_bits(bits),
                F::F32 => f32::from_bits(bits as u32) as f64,
            };
            let bits = it.bits();
            let (lo, hi) = if it.signed() { (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1) } else { (0, (1i128 << bits) - 1) };
            // `as i128` saturates and maps NaN to 0; every target here is narrower.
            Some(CV::Int((x as i128).clamp(lo, hi)))
        }
        (CV::Float(bits, F::F32), TyKind::Float(F::F64)) => Some(CV::Float((f32::from_bits(bits as u32) as f64).to_bits(), F::F64)),
        (CV::Float(bits, F::F64), TyKind::Float(F::F32)) => Some(CV::Float((f64::from_bits(bits) as f32).to_bits() as u64, F::F32)),
        (v @ CV::Float(..), TyKind::Float(_)) => Some(v),
        _ => None,
    }
}
