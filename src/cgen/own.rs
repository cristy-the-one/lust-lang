// Owning pointers to unsized values (w-unsized): `Box<str>`, `Rc<str>`,
// `Arc<str>`, `Rc<[T]>`, `Arc<[T]>`, and `Arc<dyn Tr>` (`CTy::Own`).
// `Box<dyn Tr>`, `Rc<dyn Tr>`, and `Box<[T]>` keep their older forms
// (`BoxDyn`, `RcDyn`, `BoxSlice`), with the same C layout.
//
// The C value is the pointee's fat form: `lust_str`, the slice struct,
// or `lust_dyn`. A Box points at its own allocation. An Rc or Arc
// points at the value inside the counted allocation, after its two
// counts (`strong`, then `weak` with the strong refs' implicit weak
// ref, as `RcI_T` lays them out), so `Rc<T>` to `Rc<[T]>` or `Rc<dyn
// Tr>` only takes the value's address. Value glue (Eq, Cmp, Debug,
// Display, Hash) is the pointee's, through the same fat value.
pub use crate::check::own::Owner;

use super::*;

/// The `CTy::Own` form of `t`, if `t` is one of the owning pointers to
/// an unsized value this file emits.
pub(super) fn own_cty(tys: &impl TyIntern, t: Ty) -> Option<CTy> {
    let (k, e) = crate::check::own::own_parts(tys, t)?;
    let x = match (k, tys.kind(e)) {
        (_, TyKind::Str) => CTy::Str,
        (Owner::Rc | Owner::Arc, TyKind::Slice(el)) => CTy::Slice(Box::new(cty(tys, el))),
        (Owner::Arc, TyKind::Dyn(..)) => CTy::Dyn,
        _ if crate::check::own::dyn_cell_elem(tys, e).is_some() => CTy::DynCell,
        _ if crate::check::own::dyn_lock_elem(tys, e).is_some() => CTy::DynLock(false),
        _ => return None,
    };
    Some(CTy::Own(k, Box::new(x)))
}

/// The `CTy::WeakDyn` form of `t`, if `t` is a `rc::Weak<dyn Tr>` or a
/// `sync::Weak<dyn Tr>`, and the `CTy::WeakOwn` form of a Weak of `str`
/// or `[T]`.
pub(super) fn weak_dyn_cty(tys: &impl TyIntern, t: Ty) -> Option<CTy> {
    let arc = match tys.builtin_of(t)? {
        BuiltinTy::Weak => false,
        BuiltinTy::Sync(crate::ty::sync::SyncTy::ArcWeak) => true,
        _ => return None,
    };
    match tys.kind(tys.adt_first_ty_arg(t)?) {
        TyKind::Dyn(..) => Some(CTy::WeakDyn(arc)),
        TyKind::Str => Some(CTy::WeakOwn(arc, Box::new(CTy::Str))),
        TyKind::Slice(e) => Some(CTy::WeakOwn(arc, Box::new(CTy::Slice(Box::new(cty(tys, e)))))),
        _ => None,
    }
}

/// The glue `g` of a `Weak<str>` or `Weak<[T]>` (`arc`: `sync::Weak`) whose
/// pointee form is `x`: Drop gives up its weak ref and frees the counted
/// allocation after the last one (as the `Rc`/`Arc` drop of `own_glue_def`
/// frees it); Clone takes one more; Debug prints `(Weak)`.
pub(super) fn weak_own_glue_def(g: Glue, arc: bool, x: &CTy, protos: &mut String, defs: &mut String) {
    let t = CTy::WeakOwn(arc, Box::new(x.clone()));
    let (m, ct) = (mangle(&t), ctype(&t));
    let h = counts(x, "(*p)");
    let k = if arc { Owner::Arc } else { Owner::Rc };
    let (proto, body) = match g {
        Glue::Drop => (
            format!("void drop_{m}({ct} *p)"),
            format!(
                "  uint64_t *h = {h}; if ({}) lust_dealloc((void *)h, {} + {}, 16);\n",
                dec(k, "&h[1]"),
                counts_off(x, "(*p)"),
                value_size(x, "(*p)")
            ),
        ),
        Glue::Clone => {
            let inc = if arc { format!("lust_arc_inc(&{h}[1])") } else { format!("{h}[1]++") };
            (format!("{ct} clone_{m}({ct} *p)"), format!("  {inc}; return *p;\n"))
        }
        Glue::Debug => (format!("int dbg_{m}(lust_Formatter *f, {ct} *p)"), "  (void)p; return lust_f_write_str(f, (lust_str){(uint8_t *)\"(Weak)\", 6});\n".to_string()),
        _ => unreachable!("Weak<str> and Weak<[T]> have only Drop, Clone, and Debug glue"),
    };
    let _ = writeln!(protos, "{proto};");
    let _ = writeln!(defs, "{proto} {{\n{body}}}\n");
}

/// The glue `g` of a `Weak<dyn Tr>` (`arc`: `sync::Weak`): Drop gives
/// up its weak ref and frees the allocation after the last one; Clone
/// takes one more; Debug prints `(Weak)`. A dangling Weak (data
/// `UINTPTR_MAX`, from `Weak::new()`) owns nothing.
pub(super) fn weak_dyn_glue_def(g: Glue, arc: bool, protos: &mut String, defs: &mut String) {
    let t = CTy::WeakDyn(arc);
    let (m, ct) = (mangle(&t), ctype(&t));
    let live = "p->data != (void *)~(uintptr_t)0";
    let h = counts(&CTy::Dyn, "(*p)");
    let k = if arc { Owner::Arc } else { Owner::Rc };
    let (proto, body) = match g {
        Glue::Drop => (
            format!("void drop_{m}({ct} *p)"),
            format!(
                "  if ({live}) {{ uint64_t *h = {h}; if ({}) lust_dealloc((void *)h, {} + (uint64_t)(uintptr_t)p->vt[1], (uint64_t)(uintptr_t)p->vt[2]); }}\n",
                dec(k, "&h[1]"),
                counts_off(&CTy::Dyn, "(*p)")
            ),
        ),
        Glue::Clone => {
            let inc = if arc { format!("lust_arc_inc(&{h}[1])") } else { format!("{h}[1]++") };
            (format!("{ct} clone_{m}({ct} *p)"), format!("  if ({live}) {inc}; return *p;\n"))
        }
        Glue::Debug => (format!("int dbg_{m}(lust_Formatter *f, {ct} *p)"), "  (void)p; return lust_f_write_str(f, (lust_str){(uint8_t *)\"(Weak)\", 6});\n".to_string()),
        _ => unreachable!("Weak<dyn Tr> has only Drop, Clone, and Debug glue"),
    };
    let _ = writeln!(protos, "{proto};");
    let _ = writeln!(defs, "{proto} {{\n{body}}}\n");
}

/// The data pointer of the fat value `v` of pointee place `x`, as a
/// `uint64_t *` (an Rc's or Arc's counts are at `counts`).
pub(super) fn data_words(x: &CTy, v: &str) -> String {
    match x {
        CTy::Dyn | CTy::DynCell | CTy::DynLock(_) => format!("((uint64_t *)({v}).data)"),
        _ => format!("((uint64_t *)({v}).ptr)"),
    }
}

/// The byte offset of the value in the counted allocation of an Rc or
/// Arc whose fat value `v` has pointee form `x`: the counts take 16
/// bytes, and the value starts at the first offset its alignment
/// allows (`RcI_T`'s layout, so `Rc<T>` to `Rc<dyn Tr>` keeps the
/// value's address).
pub(super) fn counts_off(x: &CTy, v: &str) -> String {
    let at_least_16 = |a: String| format!("({a} > 16 ? {a} : 16)");
    match x {
        CTy::Dyn => at_least_16(format!("(uint64_t)(uintptr_t)({v}).vt[2]")),
        CTy::DynCell => at_least_16(cell_off(&format!("({v}).vt"))),
        CTy::DynLock(_) => at_least_16(lock_align(&format!("({v}).vt"))),
        _ => "16".to_string(),
    }
}

/// The counts of the Rc or Arc whose fat value is `v` (pointee form
/// `x`), as a `uint64_t *`: `[0]` strong, `[1]` weak (with the strong
/// refs' implicit weak ref).
pub(super) fn counts(x: &CTy, v: &str) -> String {
    format!("((uint64_t *)((char *){} - {}))", data_words(x, v), counts_off(x, v))
}

/// The byte size of the value behind the fat value `v` of pointee `x`.
fn value_size(x: &CTy, v: &str) -> String {
    match x {
        CTy::Str => format!("({v}).len"),
        CTy::Slice(e) => format!("(({v}).len * sizeof({}))", ctype(e)),
        // The flag, the value, and the padding to the cell's alignment.
        CTy::DynCell => {
            let a = cell_off(&format!("({v}).vt"));
            format!("(({a} + (uint64_t)(uintptr_t)({v}).vt[1] + {a} - 1) / {a} * {a})")
        }
        // The lock words, the value, and the padding to the lock's alignment.
        CTy::DynLock(_) => {
            let (o, a) = (lock_off(&format!("({v}).vt")), lock_align(&format!("({v}).vt")));
            format!("(({o} + (uint64_t)(uintptr_t)({v}).vt[1] + {a} - 1) / {a} * {a})")
        }
        _ => format!("(uint64_t)(uintptr_t)({v}).vt[1]"),
    }
}

/// C statements that drop the value behind the fat value `v` in place.
fn drop_value(p: &Program, x: &CTy, v: &str, need: &mut dyn FnMut(Glue, &CTy)) -> String {
    match x {
        CTy::Slice(e) if needs_drop_c(p, e) => {
            need(Glue::Drop, e);
            format!("for (uint64_t i = 0; i < ({v}).len; i++) drop_{}(&({v}).ptr[i]); ", mangle(e))
        }
        CTy::Dyn => format!("if (({v}).vt[0]) ((void (*)(void *))({v}).vt[0])(({v}).data); "),
        CTy::DynCell | CTy::DynLock(_) => {
            let d = if *x == CTy::DynCell { dyn_cell_value(v) } else { dyn_lock_value(v) };
            format!("{{ lust_dyn c_ = {d}; if (c_.vt[0]) ((void (*)(void *))c_.vt[0])(c_.data); }} ")
        }
        _ => String::new(),
    }
}

/// The C condition that takes one from the count at `c` (a `uint64_t *`)
/// and is true when that was the last.
fn dec(k: Owner, c: &str) -> String {
    match k {
        Owner::Arc => format!("lust_arc_dec({c})"),
        _ => format!("(--*({c}) == 0)"),
    }
}

/// The glue `g` of `CTy::Own(k, x)`: its prototype to `protos` and its
/// definition to `defs`. Drop, Clone, and Default are the pointer's own;
/// the rest forward to the pointee's glue over the same fat value (`str`
/// through `&str`'s).
pub(super) fn own_glue_def(
    p: &Program,
    g: Glue,
    k: Owner,
    x: &CTy,
    protos: &mut String,
    defs: &mut String,
    need: &mut dyn FnMut(Glue, &CTy),
) {
    let t = CTy::Own(k, Box::new(x.clone()));
    let (m, ct) = (mangle(&t), ctype(&t));
    let (proto, body) = match g {
        Glue::Drop => {
            let body = if k == Owner::Box {
                // An empty Box<str> holds a dangling pointer: size 0 frees nothing.
                format!(
                    "  {}lust_dealloc((void *){}, {}, 1);\n",
                    drop_value(p, x, "(*p)", need),
                    data_words(x, "(*p)"),
                    value_size(x, "(*p)")
                )
            } else {
                let align = match x {
                    CTy::Dyn => "(uint64_t)(uintptr_t)p->vt[2]".to_string(),
                    CTy::DynCell => cell_off("p->vt"),
                    CTy::DynLock(_) => lock_align("p->vt"),
                    _ => "16".to_string(),
                };
                format!(
                    "  uint64_t *h = {};\n  if ({}) {{ {}if ({}) lust_dealloc((void *)h, {} + {}, {align}); }}\n",
                    counts(x, "(*p)"),
                    dec(k, "&h[0]"),
                    drop_value(p, x, "(*p)", need),
                    dec(k, "&h[1]"),
                    counts_off(x, "(*p)"),
                    value_size(x, "(*p)"),
                )
            };
            (format!("void drop_{m}({ct} *p)"), body)
        }
        Glue::Clone => {
            let body = match k {
                Owner::Box => format!(
                    "  {ct} r = *p; r.ptr = ({})lust_alloc(p->len, 1); lust_memcpy((void *)r.ptr, (void *)p->ptr, p->len); return r;\n",
                    if *x == CTy::Str { "uint8_t *".to_string() } else { format!("{}*", ctype(x)) }
                ),
                Owner::Rc => format!("  {}[0]++; return *p;\n", counts(x, "(*p)")),
                Owner::Arc => format!("  lust_arc_inc(&{}[0]); return *p;\n", counts(x, "(*p)")),
            };
            (format!("{ct} clone_{m}({ct} *p)"), body)
        }
        // `Box<str>` is empty and allocates nothing; `Rc<str>` and
        // `Rc<[T]>` allocate their counts; every empty `Arc<str>` and
        // `Arc<[T]>` shares one static allocation, as std's do.
        Glue::Default => {
            let ptr = match x {
                CTy::Slice(e) => format!("{}*", ctype(e)),
                _ => "uint8_t *".into(),
            };
            let body = match k {
                Owner::Box => format!("  return ({ct}){{ ({ptr})1, 0 }};\n"),
                Owner::Rc => {
                    format!("  uint64_t *h = (uint64_t *)lust_alloc(16, 16); h[0] = 1; h[1] = 1; return ({ct}){{ ({ptr})(h + 2), 0 }};\n")
                }
                Owner::Arc => format!("  return ({ct}){{ ({ptr})lust_arc_empty(), 0 }};\n"),
            };
            (format!("{ct} default_{m}(void)"), body)
        }
        _ => {
            let view = match x {
                CTy::Str => CTy::Ref(Box::new(CTy::Str)),
                other => other.clone(),
            };
            need(g, &view);
            let vm = mangle(&view);
            match g {
                Glue::Eq => (format!("uint8_t eq_{m}({ct} *a, {ct} *b)"), format!("  return eq_{vm}(a, b);\n")),
                Glue::Cmp => (format!("int8_t cmp_{m}({ct} *a, {ct} *b)"), format!("  return cmp_{vm}(a, b);\n")),
                Glue::PCmp => (format!("int8_t pcmp_{m}({ct} *a, {ct} *b)"), format!("  return pcmp_{vm}(a, b);\n")),
                Glue::Debug => (format!("int dbg_{m}(lust_Formatter *f, {ct} *p)"), format!("  return dbg_{vm}(f, p);\n")),
                Glue::Display => (format!("int disp_{m}(lust_Formatter *f, {ct} *p)"), format!("  return disp_{vm}(f, p);\n")),
                _ => unreachable!("glue {g:?} of an owning fat pointer"),
            }
        }
    };
    let _ = writeln!(protos, "{proto};");
    let _ = writeln!(defs, "{proto} {{\n{body}}}\n");
}

impl FnGen<'_, '_> {
    /// The builtins that take or build an owning fat pointer: the counts
    /// and `ptr_eq` of an `Rc` or `Arc` over an unsized value, and the
    /// `From` conversions (`OwnFrom`, `OwnIntoString`). None for any
    /// other call.
    pub(super) fn own_builtin(&mut self, b: Builtin, vals: &[String], tys: &[CTy], loc: &str) -> Option<String> {
        use crate::check::sync::SyncOp as S;
        use Builtin as B;
        match b {
            B::OwnFrom(k) => return Some(self.own_from(k, &vals[0], &tys[0])),
            B::OwnSliceAlloc(k) => return Some(self.own_slice_alloc(k, vals, tys)),
            B::OwnSlicePush => {
                let CTy::Ref(r) = &tys[0] else { unreachable!("OwnSlicePush takes &mut Rc<[T]>") };
                let CTy::Own(_, s) = &**r else { unreachable!("OwnSlicePush takes &mut Rc<[T]>") };
                let CTy::Slice(e) = &**s else { unreachable!("OwnSlicePush takes &mut Rc<[T]>") };
                let et = ctype(e);
                let _ = writeln!(self.out, "    {{ {et} x_ = {}; ({p})->ptr[({p})->len++] = x_; }}", vals[1], p = vals[0]);
                return Some("((lust_Unit){})".into());
            }
            // `RefCell<dyn Tr>::borrow` and `borrow_mut`: the flag is the
            // cell's first word; the guard is the same fat value.
            B::RefCellBorrow | B::RefCellBorrowMut if tys.first() == Some(&CTy::DynCell) => {
                let t = self.tmp();
                let (busy, take, msg) = if b == B::RefCellBorrowMut {
                    ("*f_ != 0", "*f_ = -1", "RefCell already borrowed")
                } else {
                    ("*f_ < 0 || *f_ == 0x7fffffffffffffffLL", "(*f_)++", "RefCell already mutably borrowed")
                };
                let _ = writeln!(
                    self.out,
                    "    lust_dyn {t} = {}; {{ int64_t *f_ = (int64_t *){t}.data; if ({busy}) lust_panic_cstr_dyn(\"{msg}\", {loc}); {take}; }}",
                    vals[0]
                );
                return Some(t);
            }
            B::OwnIntoString => {
                let v = &vals[0];
                return Some(format!("((lust_String){{ ({v}).ptr, ({v}).len, ({v}).len }})"));
            }
            _ => {}
        }
        // `Mutex<dyn Tr>::lock` and `is_poisoned`: the futex word is the
        // lock's first word. The poison byte follows it: offset 4 after a
        // `u32` futex, offset 1 after Windows's `u8` futex.
        if tys.first() == Some(&CTy::DynLock(false)) {
            let v = &vals[0];
            let poff = if cfg!(windows) { 1 } else { 4 };
            return Some(match b {
                B::Sync(S::MutexLock) => {
                    let guard = CTy::DynLock(true);
                    let res =
                        CTy::Result(Box::new(guard.clone()), Box::new(CTy::Sync(crate::ty::sync::SyncTy::PoisonError, Box::new(guard))));
                    self.note_held(&res);
                    let (g, t) = (self.tmp(), self.tmp());
                    let (f, e) = (variant_field(self.p, &res, &t, 0, 0), variant_field(self.p, &res, &t, 1, 0));
                    let _ = writeln!(
                        self.out,
                        "    lust_dyn {g} = {v}; lust_mutex_lock((void *){g}.data); lust_poison_enter((uint8_t *){g}.data + {poff});"
                    );
                    let _ = writeln!(
                        self.out,
                        "    {} {t}; if (((uint8_t *){g}.data)[{poff}] & 1) {{ {} {e} = {g}; }} else {{ {} {f} = {g}; }}",
                        ctype(&res),
                        disc_set(self.p, &res, &t, 1),
                        disc_set(self.p, &res, &t, 0)
                    );
                    t
                }
                B::Sync(S::LockIsPoisoned) => format!("((uint8_t)(((uint8_t *)({v}).data)[{poff}] & 1))"),
                _ => return None,
            });
        }
        if let Some(v) = self.weak_dyn_builtin(b, vals, tys) {
            return Some(v);
        }
        // The count ops take `&Rc<T>` or `&Arc<T>`.
        let CTy::Ref(r) = tys.first()? else { return None };
        let CTy::Own(k, x) = &**r else { return None };
        let w = |v: &str| data_words(x, &format!("(*{v})"));
        let c = |v: &str| counts(x, &format!("(*{v})"));
        // The `&mut` view of the pointee: the same fat value.
        let view = match &**x {
            CTy::Str => CTy::Ref(Box::new(CTy::Str)),
            other => other.clone(),
        };
        Some(match b {
            B::RcGetMut | B::Sync(S::ArcGetMut) => {
                let opt = CTy::Option(Box::new(view));
                self.note_held(&opt);
                let t = self.tmp();
                let field = variant_field(self.p, &opt, &t, 1, 0);
                let some = disc_set(self.p, &opt, &t, 1);
                let h = c(&vals[0]);
                let unique = if *k == Owner::Arc { format!("lust_arc_is_unique({h})") } else { format!("({h}[0] == 1 && {h}[1] == 1)") };
                let _ = writeln!(self.out, "    {} {t}; {}", ctype(&opt), disc_set(self.p, &opt, &t, 0));
                let _ = writeln!(self.out, "    if ({unique}) {{ {field} = *{}; {some} }}", vals[0]);
                t
            }
            B::RcMakeMut | B::Sync(S::ArcMakeMut) => self.own_make_mut(*k, x, &vals[0]),
            B::RcStrongCount => format!("{}[0]", c(&vals[0])),
            B::RcWeakCount => format!("({}[1] - 1)", c(&vals[0])),
            B::RcPtrEq | B::Sync(S::ArcPtrEq) => format!("({} == {})", w(&vals[0]), w(&vals[1])),
            B::Sync(S::ArcStrongCount) => format!("lust_arc_load(&{}[0])", c(&vals[0])),
            B::Sync(S::ArcWeakCount) => format!("lust_arc_weak_count({})", c(&vals[0])),
            _ => return None,
        })
    }

    /// `Rc::downgrade` and `Arc::downgrade` of a pointer to a dyn value,
    /// and the `Weak<dyn Tr>` methods: the Weak is the pointer's fat
    /// value, its counts found by the vtable's alignment (`counts`).
    /// None for any other call.
    fn weak_dyn_builtin(&mut self, b: Builtin, vals: &[String], tys: &[CTy]) -> Option<String> {
        use crate::check::sync::SyncOp as S;
        use Builtin as B;
        let first = deref_cty(tys.first()?);
        let h = |v: &str| counts(&CTy::Dyn, v);
        let dangling = |v: &str| format!("(({v}).data == (void *)~(uintptr_t)0)");
        match (b, &first) {
            (B::RcDowngrade, CTy::RcDyn) | (B::Sync(S::ArcDowngrade), CTy::Own(Owner::Arc, _)) if first == CTy::RcDyn || first == CTy::Own(Owner::Arc, Box::new(CTy::Dyn)) => {
                let t = self.tmp();
                let inc = if b == B::RcDowngrade { format!("{}[1]++", h(&t)) } else { format!("lust_arc_downgrade({})", h(&t)) };
                let _ = writeln!(self.out, "    lust_dyn {t} = *{}; {inc};", vals[0]);
                Some(t)
            }
            (B::WeakUpgrade | B::Sync(S::ArcWeakUpgrade), CTy::WeakDyn(arc)) => {
                let ptr = if *arc { CTy::Own(Owner::Arc, Box::new(CTy::Dyn)) } else { CTy::RcDyn };
                let opt = CTy::Option(Box::new(ptr));
                self.note_held(&opt);
                let (t, w) = (self.tmp(), self.tmp());
                let field = variant_field(self.p, &opt, &t, 1, 0);
                let some = disc_set(self.p, &opt, &t, 1);
                let up = if *arc { format!("lust_arc_upgrade({})", h(&w)) } else { format!("({0}[0] > 0 && ++{0}[0])", h(&w)) };
                let _ = writeln!(self.out, "    {} {t}; {}", ctype(&opt), disc_set(self.p, &opt, &t, 0));
                let _ = writeln!(self.out, "    lust_dyn {w} = *{}; if (!{} && {up}) {{ {field} = {w}; {some} }}", vals[0], dangling(&w));
                Some(t)
            }
            (B::WeakStrongCount | B::Sync(S::ArcWeakStrongCount), CTy::WeakDyn(arc)) => {
                let v = format!("(*{})", vals[0]);
                let n = if *arc { format!("lust_arc_load(&{}[0])", h(&v)) } else { format!("{}[0]", h(&v)) };
                Some(format!("({} ? (uint64_t)0 : {n})", dangling(&v)))
            }
            (B::WeakWeakCount | B::Sync(S::ArcWeakWeakCount), CTy::WeakDyn(arc)) => {
                let v = format!("(*{})", vals[0]);
                let n = if *arc { format!("lust_arc_weak_weak_count({})", h(&v)) } else { format!("({0}[0] == 0 ? (uint64_t)0 : {0}[1] - 1)", h(&v)) };
                Some(format!("({} ? (uint64_t)0 : {n})", dangling(&v)))
            }
            // The addresses, not the vtables (`ptr::addr_eq`).
            (B::WeakPtrEq | B::Sync(S::ArcWeakPtrEq), CTy::WeakDyn(_)) => Some(format!("((*{}).data == (*{}).data)", vals[0], vals[1])),
            // `Weak<str>` and `Weak<[T]>`: the fat value of the pointer,
            // its counts 16 bytes before the data.
            (B::RcDowngrade, CTy::Own(Owner::Rc, x)) | (B::Sync(S::ArcDowngrade), CTy::Own(Owner::Arc, x)) if matches!(**x, CTy::Str | CTy::Slice(_)) => {
                let t = self.tmp();
                let hc = counts(x, &t);
                let inc = if b == B::RcDowngrade { format!("{hc}[1]++") } else { format!("lust_arc_downgrade({hc})") };
                let _ = writeln!(self.out, "    {} {t} = *{}; {inc};", ctype(x), vals[0]);
                Some(t)
            }
            (B::WeakUpgrade | B::Sync(S::ArcWeakUpgrade), CTy::WeakOwn(arc, x)) => {
                let k = if *arc { Owner::Arc } else { Owner::Rc };
                let opt = CTy::Option(Box::new(CTy::Own(k, x.clone())));
                self.note_held(&opt);
                let (t, w) = (self.tmp(), self.tmp());
                let hc = counts(x, &w);
                let field = variant_field(self.p, &opt, &t, 1, 0);
                let some = disc_set(self.p, &opt, &t, 1);
                let up = if *arc { format!("lust_arc_upgrade({hc})") } else { format!("({hc}[0] > 0 && ++{hc}[0])") };
                let _ = writeln!(self.out, "    {} {t}; {}", ctype(&opt), disc_set(self.p, &opt, &t, 0));
                let _ = writeln!(self.out, "    {} {w} = *{}; if ({up}) {{ {field} = {w}; {some} }}", ctype(x), vals[0]);
                Some(t)
            }
            (B::WeakStrongCount | B::Sync(S::ArcWeakStrongCount), CTy::WeakOwn(arc, x)) => {
                let hc = counts(x, &format!("(*{})", vals[0]));
                Some(if *arc { format!("lust_arc_load(&{hc}[0])") } else { format!("{hc}[0]") })
            }
            (B::WeakWeakCount | B::Sync(S::ArcWeakWeakCount), CTy::WeakOwn(arc, x)) => {
                let hc = counts(x, &format!("(*{})", vals[0]));
                Some(if *arc { format!("lust_arc_weak_weak_count({hc})") } else { format!("({0}[0] == 0 ? (uint64_t)0 : {0}[1] - 1)", hc) })
            }
            (B::WeakPtrEq | B::Sync(S::ArcWeakPtrEq), CTy::WeakOwn(_, x)) => {
                Some(format!("({} == {})", data_words(x, &format!("(*{})", vals[0])), data_words(x, &format!("(*{})", vals[1]))))
            }
            _ => None,
        }
    }

    /// `Rc::make_mut` or `Arc::make_mut` on `*v`, an `Rc` or `Arc` (owner
    /// `k`) of `str` or `[T]`: a shared value is cloned into a fresh
    /// allocation (this pointer's count drops by one); a value only Weaks
    /// share moves to a fresh allocation. Returns the `&mut` fat value.
    fn own_make_mut(&mut self, k: Owner, x: &CTy, v: &str) -> String {
        let t = CTy::Own(k, Box::new(x.clone()));
        let (ct, h, n, st) = (ctype(&t), self.tmp(), self.tmp(), self.tmp());
        let (e, et) = match x {
            CTy::Slice(e) => ((**e).clone(), ctype(e)),
            _ => (CTy::Int(IntTy::U8), "uint8_t".to_string()),
        };
        let size = format!("(*{v}).len * sizeof({et})");
        let copy = format!("lust_memcpy((void *)({n} + 2), (void *)(*{v}).ptr, {size});");
        let clone = if is_copy_c(self.p, &e) {
            copy.clone()
        } else {
            self.want(Glue::Clone, &e);
            format!("for (uint64_t i_ = 0; i_ < (*{v}).len; i_++) (({et}*)({n} + 2))[i_] = clone_{}(&(*{v}).ptr[i_]);", mangle(&e))
        };
        let fresh = format!("uint64_t *{n} = (uint64_t *)lust_alloc(16 + {size}, 16); {n}[0] = 1; {n}[1] = 1;");
        let _ = writeln!(self.out, "    uint64_t *{h} = {};", counts(x, &format!("(*{v})")));
        match k {
            Owner::Arc => {
                // 0: shared, 1: only Weaks, 2: unique (runtime/sync.c).
                self.want(Glue::Drop, &t);
                let m = mangle(&t);
                let _ = writeln!(self.out, "    int {st} = lust_arc_make_mut_state({h});");
                let _ = writeln!(
                    self.out,
                    "    if ({st} != 2) {{ {fresh} {ct} o_ = *{v};\n      if ({st} == 0) {{ {clone} drop_{m}(&o_); }} else {{ {copy} if (lust_arc_dec(&{h}[1])) lust_dealloc((void *){h}, 16 + {size}, 16); }}\n      (*{v}).ptr = ({et}*)({n} + 2); }}"
                );
            }
            _ => {
                let _ = writeln!(
                    self.out,
                    "    if ({h}[0] > 1) {{ {fresh} {clone} {h}[0]--; (*{v}).ptr = ({et}*)({n} + 2); }}\n    else if ({h}[1] > 1) {{ {fresh} {copy} {h}[0] = 0; {h}[1]--; (*{v}).ptr = ({et}*)({n} + 2); }}"
                );
            }
        }
        format!("(*{v})")
    }

    /// `Box`, `Rc`, or `Arc` of `str` or `[T]` (owner `k`) from `v`, a
    /// value of `src`: `&str`, `String`, `Box<str>`, `&[T]` (cloned),
    /// `Vec<T>`, `Box<[T]>`, or `[T; N]` (moved). Moved sources give up
    /// their buffers. Returns the temporary that holds the fat value.
    /// `OwnSliceAlloc`: alloc's `Rc::<[T]>::allocate_for_slice(n)` (and
    /// Arc's): `Layout::array::<T>(n).unwrap()`, then the `RcInner`
    /// layout (`capacity overflow` past `isize::MAX`), both panicking at
    /// std's lines, then one allocation of that size (its OOM message
    /// names it). The data sits after the two counts, as `own_from`.
    fn own_slice_alloc(&mut self, k: Owner, vals: &[String], tys: &[CTy]) -> String {
        let CTy::Ref(r) = &tys[0] else { unreachable!("OwnSliceAlloc takes &mut Rc<[T]>") };
        let CTy::Own(_, s) = &**r else { unreachable!("OwnSliceAlloc takes &mut Rc<[T]>") };
        let CTy::Slice(e) = &**s else { unreachable!("OwnSliceAlloc takes &mut Rc<[T]>") };
        let et = ctype(e);
        self.note_held(s);
        // (file, `Layout::array` unwrap line, `rc_inner_layout_for_value_layout` line)
        let (file, unwrap_line, extend_line) = match k {
            Owner::Arc => (super::STD_SYNC_RS, 2277, 416),
            _ => (super::STD_RC_RS, 2326, 299),
        };
        let (bad_layout, overflow) = (super::std_loc(file, unwrap_line, 41), super::std_loc(file, extend_line, 29));
        let _ = writeln!(
            self.out,
            "    {{ uint64_t n_ = {n}, s_ = sizeof({et}), a_ = _Alignof({et}), m_ = 0x7fffffffffffffffull;\n\
             \x20     if (s_ && n_ > (m_ - (a_ - 1)) / s_) lust_panic_cstr(\"called `Result::unwrap()` on an `Err` value: LayoutError\", {bad_layout});\n\
             \x20     uint64_t al_ = a_ > 8 ? a_ : 8, off_ = (16 + a_ - 1) & ~(a_ - 1);\n\
             \x20     if (off_ + n_ * s_ > m_ - (al_ - 1)) lust_panic_cstr(\"capacity overflow\", {overflow});\n\
             \x20     uint64_t *h_ = (uint64_t *)lust_alloc((off_ + n_ * s_ + al_ - 1) & ~(al_ - 1), 16); h_[0] = 1; h_[1] = 1;\n\
             \x20     ({p})->ptr = ({et}*)(h_ + 2); ({p})->len = 0; }}",
            n = vals[1],
            p = vals[0],
        );
        "((lust_Unit){})".into()
    }

    fn own_from(&mut self, k: Owner, v: &str, src: &CTy) -> String {
        if let Some(r) = self.own_from_box(v, src) {
            return r;
        }
        // (element type, data pointer, length, whether elements move,
        // statement that frees the source's buffer)
        let byte = CTy::Int(IntTy::U8);
        let (e, ptr, len, moved, free): (CTy, String, String, bool, String) = match src {
            CTy::Ref(s) if **s == CTy::Str => (byte, format!("({v}).ptr"), format!("({v}).len"), false, String::new()),
            CTy::Str => (byte, format!("({v}).ptr"), format!("({v}).len"), false, String::new()),
            CTy::String => {
                let s = self.tmp();
                let _ = writeln!(self.out, "    lust_String {s} = {v};");
                (byte, format!("{s}.ptr"), format!("{s}.len"), true, format!("lust_string_drop(&{s});"))
            }
            CTy::Own(Owner::Box, s) if **s == CTy::Str => {
                let s = self.tmp();
                let _ = writeln!(self.out, "    lust_str {s} = {v};");
                (byte, format!("{s}.ptr"), format!("{s}.len"), true, format!("lust_dealloc((void *){s}.ptr, {s}.len, 1);"))
            }
            CTy::Slice(e) => ((**e).clone(), format!("({v}).ptr"), format!("({v}).len"), false, String::new()),
            CTy::Vec(e) => {
                let (s, et) = (self.tmp(), ctype(e));
                let _ = writeln!(self.out, "    lust_Vec {s} = {v};");
                (
                    (**e).clone(),
                    format!("(({et}*){s}.ptr)"),
                    format!("{s}.len"),
                    true,
                    format!("lust_dealloc({s}.ptr, {s}.cap * sizeof({et}), _Alignof({et}));"),
                )
            }
            CTy::BoxSlice(e) => {
                let (s, et) = (self.tmp(), ctype(e));
                let _ = writeln!(self.out, "    {} {s} = {v};", ctype(src));
                (
                    (**e).clone(),
                    format!("{s}.ptr"),
                    format!("{s}.len"),
                    true,
                    format!("lust_dealloc((void *){s}.ptr, {s}.len * sizeof({et}), _Alignof({et}));"),
                )
            }
            CTy::Array(e, n) => {
                let s = self.tmp();
                let _ = writeln!(self.out, "    {} {s} = {v};", ctype(src));
                ((**e).clone(), format!("{s}.v"), format!("((uint64_t){n})"), true, String::new())
            }
            other => unreachable!("an owning pointer from {other:?}"),
        };
        let et = ctype(&e);
        let ft = if src == &CTy::String || matches!(src, CTy::Ref(_) | CTy::Str | CTy::Own(..)) {
            "lust_str".to_string()
        } else {
            ctype(&CTy::Slice(Box::new(e.clone())))
        };
        self.note_held(&CTy::Slice(Box::new(e.clone())));
        let (r, n) = (self.tmp(), self.tmp());
        let _ = writeln!(self.out, "    uint64_t {n} = {len};\n    {ft} {r};\n    {r}.len = {n};");
        let size = format!("{n} * sizeof({et})");
        match k {
            Owner::Box => {
                let _ = writeln!(self.out, "    {r}.ptr = ({et}*)lust_alloc({size}, _Alignof({et}));");
            }
            Owner::Rc | Owner::Arc => {
                let _ = writeln!(
                    self.out,
                    "    {{ uint64_t *h_ = (uint64_t *)lust_alloc(16 + {size}, 16); h_[0] = 1; h_[1] = 1; {r}.ptr = ({et}*)(h_ + 2); }}"
                );
            }
        }
        if moved || is_copy_c(self.p, &e) {
            let _ = writeln!(self.out, "    if ({n}) lust_memcpy((void *){r}.ptr, (void *){ptr}, {size});");
        } else {
            self.want(Glue::Clone, &e);
            let _ = writeln!(self.out, "    for (uint64_t i_ = 0; i_ < {n}; i_++) {r}.ptr[i_] = clone_{}(&{ptr}[i_]);", mangle(&e));
        }
        if !free.is_empty() {
            let _ = writeln!(self.out, "    {free}");
        }
        r
    }

    /// alloc's `From<Box<T>>` for `Rc<T>` and `Arc<T>` (check/own.rs
    /// `own_from_box`; both owners share the layout): the value's bytes
    /// move into a new counted allocation (the counts, then the value at
    /// the offset its alignment gives, as `RcI_T` lays it out), a `dyn`
    /// value keeps its vtable, and the box's memory is freed with no
    /// drop. None for the other sources.
    fn own_from_box(&mut self, v: &str, src: &CTy) -> Option<String> {
        let t = self.tmp();
        match src {
            CTy::Boxed(e) => {
                let (it, et, b) = (format!("RcI_{}", mangle(e)), ctype(e), self.tmp());
                self.note_held(e);
                let (size, val) = (super::rc_size(&it, e), super::rc_value(&t, e));
                let _ = writeln!(
                    self.out,
                    "    {et} *{b} = {v}; {it} *{t} = ({it}*)lust_alloc({size}, _Alignof({it})); {t}->strong = 1; {t}->weak = 1; {val} = *{b}; lust_dealloc((void*){b}, sizeof({et}), _Alignof({et}));"
                );
            }
            CTy::BoxDyn => {
                let off = counts_off(&CTy::Dyn, &t);
                let _ = writeln!(
                    self.out,
                    "    lust_dyn {t} = {v}; {{ uint64_t s_ = (uint64_t)(uintptr_t){t}.vt[1], o_ = {off}; char *a_ = (char *)lust_alloc(o_ + s_, o_); ((uint64_t *)a_)[0] = 1; ((uint64_t *)a_)[1] = 1; if (s_) lust_memcpy((void *)(a_ + o_), {t}.data, s_); lust_dealloc({t}.data, s_, (uint64_t)(uintptr_t){t}.vt[2]); {t}.data = (void *)(a_ + o_); }}"
                );
            }
            _ => return None,
        }
        Some(t)
    }
}

/// The byte offset of the value inside a `RefCell` whose value's vtable
/// is `vt`: after the `isize` borrow flag, at the value's alignment.
fn cell_off(vt: &str) -> String {
    format!("((uint64_t)(uintptr_t){vt}[2] > 8 ? (uint64_t)(uintptr_t){vt}[2] : 8)")
}

/// The dyn place of the value in the `RefCell<dyn Tr>` (or its guard)
/// whose fat value is `v`.
pub(super) fn dyn_cell_value(v: &str) -> String {
    format!("((lust_dyn){{ (void *)((char *)({v}).data + {}), ({v}).vt }})", cell_off(&format!("({v}).vt")))
}

/// The drop glue of a `Ref<dyn Tr>` or `RefMut<dyn Tr>` (`t`), which
/// gives the borrow back through the cell's flag word, or of a
/// `MutexGuard<dyn Tr>`, which unlocks (poisoning the lock when a panic
/// drops it, as `lust_poison_leave` does for a sized guard).
pub(super) fn dyn_guard_drop(t: &CTy, protos: &mut String, defs: &mut String) {
    let (mn, ct) = (mangle(t), ctype(t));
    let body = match t {
        CTy::DynGuard(m) => format!("((int64_t *)p->data)[0] {}= 1;", if *m { "+" } else { "-" }),
        _ => {
            let poff = if cfg!(windows) { 1 } else { 4 };
            format!("lust_poison_leave((uint8_t *)p->data + {poff}); lust_mutex_unlock((void *)p->data);")
        }
    };
    let _ = writeln!(protos, "void drop_{mn}({ct} *p);");
    let _ = writeln!(defs, "void drop_{mn}({ct} *p) {{\n  {body}\n}}\n");
}

/// The byte offset of the value inside a `Mutex` whose value's vtable is
/// `vt`: after the futex word and the poison byte (`u32` + `u8` = 5, or
/// two `u8`s = 2 on Windows), at the value's alignment.
fn lock_off(vt: &str) -> String {
    let header = if cfg!(windows) { 2 } else { 5 };
    format!("(({header} + (uint64_t)(uintptr_t){vt}[2] - 1) & ~((uint64_t)(uintptr_t){vt}[2] - 1))")
}

/// The alignment of a `Mutex` whose value's vtable is `vt`.
fn lock_align(vt: &str) -> String {
    let min = if cfg!(windows) { 1 } else { 4 };
    format!("((uint64_t)(uintptr_t){vt}[2] > {min} ? (uint64_t)(uintptr_t){vt}[2] : {min})")
}

/// The dyn place of the value in the `Mutex<dyn Tr>` (or its guard)
/// whose fat value is `v`.
pub(super) fn dyn_lock_value(v: &str) -> String {
    format!("((lust_dyn){{ (void *)((char *)({v}).data + {}), ({v}).vt }})", lock_off(&format!("({v}).vt")))
}
