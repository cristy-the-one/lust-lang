// The const-context check for `const fn` bodies (rustc E0015 and kin).
//
// The bounded const evaluator (`ty::consteval`) only runs when a const
// value is needed, so a `const fn` that is only called at runtime never
// went through it. This pass walks every checked `const fn` body once,
// after type checking, and rejects the forms rustc 1.99 rejects in a
// constant function:
//
// - calls to non-const functions, methods, and associated functions
//   (user fns read their `const` marker; the std surface lust implements
//   as builtins reads the one table below, `builtin_is_const`),
// - trait method calls, including operators and comparisons through
//   trait impls, `dyn` calls, closure calls, and function pointer calls,
// - `for` loops and the `?` operator,
// - non-const macros (`println!`, `format!`, `vec!`, `assert_eq!`, ...),
// - indexing and dereferencing through non-const `Index`/`Deref` impls,
//   and range indexing,
// - values whose destructor would run inside the function (E0493):
//   a by-value parameter, local, or temporary of a drop type that is
//   not moved out of the function.
//
// `const`/`static` initializers, inline `const { }` blocks, array
// lengths, and const generic arguments already run through the const
// evaluator at check time, which rejects the same calls there, so this
// pass does not walk them. Closure bodies and nested fn items are not
// const contexts: the walk does not descend into them (a nested
// `const fn` gets its own walk when its body is checked).

use crate::ast::{self, Expr, ExprId, MacroKind, Pat, PatId, Stmt, UnOp};
use crate::check::collect::{FnInfo, Program, needs_drop, ty_name};
use crate::check::{Autoref, BindMode, Builtin, Checked, OrderingOpKind, PatRes, Res};
use crate::def::LocalDefId;
use crate::diag::{Diag, Span};
use crate::ty::consteval::{C_NONCONST, ConstMacroForm, const_macro_form, is_fmt_capture};
use crate::ty::{Ty, TyIntern, TyKind};

/// Checks one checked `const fn` body and returns its diagnostics.
/// `prior_errs` is true when the body already failed type checking:
/// unresolved calls then stay silent instead of adding a cascade.
pub fn check_const_fn(p: &Program, ck: &Checked, f: &FnInfo, prior_errs: bool) -> Vec<Diag> {
    let Some(body) = f.def(&p.ast).body else { return Vec::new() };
    let mut w = Walk { p, ck, errs: Vec::new(), drop_locals: Vec::new(), moved_out: Vec::new(), loop_consumed: Vec::new(), prior_errs };
    // Parameters by value are locals the function may drop. A pattern
    // parameter's argument local only feeds the pattern, so the
    // pattern bindings stand in for it.
    let mut pi = 0;
    if f.def(&p.ast).self_param.is_some() {
        if let Some(&l) = ck.param_locals.first() {
            w.note_local(l, 0);
        }
        pi = 1;
    }
    for param in &f.def(&p.ast).params {
        match param.pat {
            Some(pat) => w.pat_bindings(pat, 0),
            None => {
                if let Some(&l) = ck.param_locals.get(pi) {
                    w.note_local(l, 0);
                }
            }
        }
        pi += 1;
    }
    w.block(body, true, 0);
    // A drop local that was not moved out at its own conditional depth
    // is dropped inside the function on some path (rustc E0493).
    let mut drop_errs: Vec<Diag> = Vec::new();
    for &(local, depth) in &w.drop_locals {
        if w.moved_out.iter().any(|&(l, d)| l == local && d == depth) {
            continue;
        }
        let info = &ck.locals[local as usize];
        let tn = ty_name(p, &ck.tys, info.ty);
        drop_errs.push(Diag::new(
            C_NONCONST,
            info.span,
            format!("destructor of `{tn}` cannot run in a const function"),
            "Move the value out of the function, take it by reference, or remove the `const`.",
        ));
    }
    w.errs.extend(drop_errs);
    w.errs
}

/// True when a call to the std builtin `b` is a `const fn` call in
/// rustc 1.99. This is the one table of std const operations for the
/// checker: every other builtin is a non-const call in a const
/// function. The const evaluator keeps its own name-based list of
/// the primitive methods it can run; the two lists agree on the
/// families both sides support, and each side rejects the rest on
/// its own. Entries were probed against rustc 1.99 (`const fn`
/// bodies that call each method); keep the table conservative,
/// because a wrong `true` is a false accept.
fn builtin_is_const(b: Builtin) -> bool {
    use Builtin::*;
    matches!(
        b,
        // `String::new` and `Vec::new` are const; the empty value is
        // still a drop type, so destroying it here rejects elsewhere.
        StringNew
            | VecNew
            // `Cell::new` (its const value is the inner value's, ty/consteval.rs).
            | CellNew
            // `HashMap::with_hasher` and `HashSet::with_hasher` (const
            // since Rust 1.85); `new` and `default` are not.
            | HashWithHasher
            // `AtomicX::new` and `into_inner` are `const fn` (W-THREADS).
            | Sync(crate::check::sync::SyncOp::AtomicNew(_) | crate::check::sync::SyncOp::AtomicIntoInner | crate::check::sync::SyncOp::LockNew(_) | crate::check::sync::SyncOp::SyncNew(_))
            // Length and view queries on str, String, Vec, and slices.
            | StrLen
            | StrIsEmpty
            | StrIsCharBoundary
            // `str::as_bytes`; the same builtin serves `AsRef<[u8]>`,
            // which `call_res` tells apart by the written name.
            | ConvStrBytes
            | StringLen
            | StringIsEmpty
            | StringAsStr
            | StringAsBytes
            | StringCapacity
            | VecLen
            | VecIsEmpty
            | VecCapacity
            | VecAsSlice
            | SliceLen
            | SliceIsEmpty
            | VecFirst
            | VecLast
            // Integer inherent methods: the wrapping, checked,
            // saturating, and overflowing families and the bit queries
            // are all `const fn` in core.
            | WrappingAdd
            | WrappingSub
            | WrappingMul
            | WrappingDiv
            | WrappingRem
            | WrappingNeg
            | WrappingPow
            | WrappingShl
            | WrappingShr
            | WrappingAbs
            | CheckedAdd
            | CheckedSub
            | CheckedMul
            | CheckedDiv
            | CheckedRem
            | CheckedNeg
            | CheckedAbs
            | CheckedPow
            | CheckedShl
            | CheckedShr
            | SaturatingAdd
            | SaturatingSub
            | SaturatingMul
            | SaturatingDiv
            | SaturatingNeg
            | SaturatingAbs
            | SaturatingPow
            | OverflowingAdd
            | OverflowingSub
            | OverflowingMul
            | OverflowingDiv
            | OverflowingRem
            | OverflowingNeg
            | OverflowingAbs
            | OverflowingPow
            | OverflowingShl
            | OverflowingShr
            | CountOnes
            | CountZeros
            | LeadingZeros
            | TrailingZeros
            | LeadingOnes
            | TrailingOnes
            | RotateLeft
            | RotateRight
            | SwapBytes
            | ReverseBits
            | IntToLeBytes
            | IntToBeBytes
            | IntToNeBytes
            | IntFromLeBytes
            | IntFromBeBytes
            | IntFromNeBytes
            | DivEuclid
            | RemEuclid
            | Signum
            | IsPositive
            | IsNegative
            | IsPowerOfTwo
            | NextPowerOfTwo
            | CheckedNextPowerOfTwo
            | Ilog2
            | Ilog10
            | Ilog
            | CheckedIlog2
            | CheckedIlog10
            | CheckedIlog
            | Abs
            | Pow
            // Float inherent methods that are const in 1.99. The
            // transcendentals (`sqrt`, `powi`, `exp`, ...) are not.
            | FloatAbs
            | FloatSignum
            | FloatFloor
            | FloatCeil
            | FloatRound
            | FloatTrunc
            | FloatFract
            | FloatRecip
            | FloatMin
            | FloatMax
            // `f32::clamp` and `f64::clamp` are const. A bound that is
            // NaN or above the other panics; the evaluator reports that
            // (ty/consteval.rs) instead of returning a value.
            | FloatClamp
            | FloatIsNan
            | FloatIsInfinite
            | FloatIsFinite
            | FloatIsSignPositive
            | FloatIsSignNegative
            | FloatToBits
            | FloatCopysign
            | F32FromBits
            | F64FromBits
            // `char` inherent queries that are const in core. Of the
            // Unicode predicates only these four are const in 1.99;
            // is_alphabetic, is_numeric, and is_alphanumeric are not.
            | CharLenUtf8
            | CharLenUtf16
            | CharIsAscii
            | CharIsAsciiPunctuation
            | CharIsAsciiGraphic
            | CharIsAsciiHexdigit
            | CharIsAsciiControl
            | CharIsDigit
            | CharIsDigitRadix
            | CharToDigit
            | CharIsLowercaseUnicode
            | CharIsUppercaseUnicode
            | CharIsWhitespaceUnicode
            | CharIsControlUnicode
            | CharFromU32
            | CharFromDigit
            // `core::mem::swap` and `replace` are const; `take` needs
            // `Default`, which is only conditionally const in 1.99.
            | MemSwap
            | MemReplace
            | MemForget
            // `NonZero::new` and `NonZero::get`.
            | NonZeroNew
            | NonZeroGet
            // More integer methods that are const in 1.99 (probed with
            // rustc 1.99 in const fn bodies, tf-v07).
            | DivCeil
            | IsMultipleOf
            | NextMultipleOf
            | CheckedNextMultipleOf
            | UnsignedAbs
            | UnboundedShl
            | UnboundedShr
            | CastSigned
            | CastUnsigned
            | Isqrt
            | CheckedIsqrt
            | BitWidth
            | HighestOne
            | LowestOne
            | IsolateHighestOne
            | IsolateLowestOne
            | IntMidpoint
            | IntAbsDiff
            | StrictAdd
            | StrictSub
            | StrictMul
            | StrictDiv
            | StrictRem
            | StrictDivEuclid
            | StrictRemEuclid
            | StrictNeg
            | StrictAbs
            | StrictPow
            | StrictShl
            | StrictShr
            | MixedCheckedAdd
            | MixedCheckedSub
            | MixedWrappingAdd
            | MixedWrappingSub
            | MixedSaturatingAdd
            | MixedSaturatingSub
            | MixedOverflowingAdd
            | MixedOverflowingSub
            | MixedStrictAdd
            | MixedStrictSub
            | CheckedSignedDiff
            | FloatMulAdd
            // `char` and `u8` ASCII methods and the UTF encoders.
            | CharEncodeUtf8
            | CharEncodeUtf16
            | CharIsWhitespace
            | CharIsUppercase
            | CharIsLowercase
            | CharIsAlphabetic
            | CharIsAlphanumeric
            | CharToUppercase
            | CharToLowercase
            | MakeAsciiLowercase
            | MakeAsciiUppercase
            // Pointers, cells, `MaybeUninit`, ranges, UTF-8, and slices.
            // A slice method reached through a `Vec`, `String`, or smart
            // pointer is still a non-const `Deref` (`deref_receiver`).
            | HintBlackBox
            | PtrNull
            | PtrIsNull
            | PtrFromRef
            | PtrCast
            | RangeInclStart
            | RangeInclEnd
            | RefCellNew
            | RefCellIntoInner
            | CellGet
            | CellReplace
            | CellIntoInner
            | CellGetMut
            | MaybeUninitUninit
            | MaybeUninitZeroed
            | MaybeUninitNew
            | MaybeUninitWrite
            | MaybeUninitAsPtr
            | MaybeUninitAsMutPtr
            | WeakNew
            | StrFromUtf8
            | Utf8ErrorValidUpTo
            | Utf8ErrorErrorLen
            | LocationCaller
            | LocationFile
            | LocationLine
            | LocationColumn
            | SliceFromRef
            | SliceIsAscii
            | StrIsAscii
            | SliceEqIgnoreAsciiCase
            | StrEqIgnoreAsciiCase
            | SliceRotateLeft
            | SliceRotateRight
            | VecSwap
            | SliceCopyFromSlice
            | VecFirstMut
            | SeqAsPtr
            | StrAsPtr
            // The reads behind std source's `const fn discriminant`.
            | DiscriminantValue
            | DiscriminantKind
    )
}

struct Walk<'p, 'c> {
    p: &'p Program,
    ck: &'c Checked<'c>,
    errs: Vec<Diag>,
    /// Locals of a drop type, with the conditional depth of the binding.
    drop_locals: Vec<(u32, u32)>,
    /// Locals used in a moved-out position, with the depth of the use.
    moved_out: Vec<(u32, u32)>,
    /// Whether each enclosing loop's value is moved out (for `break`).
    loop_consumed: Vec<bool>,
    prior_errs: bool,
}

impl Walk<'_, '_> {
    fn err(&mut self, span: Span, msg: impl Into<String>, fix: &'static str) {
        self.errs.push(Diag::new(C_NONCONST, span, msg.into(), fix));
    }

    fn ty(&self, e: ExprId) -> Ty {
        self.ck.ty(e)
    }

    /// A plain scalar a built-in operator may run on. The error type
    /// passes so one mistake stays one error.
    fn scalar(&self, t: Ty) -> bool {
        match self.ck.tys.kind(t) {
            TyKind::Int(_) | TyKind::Float(_) | TyKind::Bool | TyKind::Char | TyKind::Never | TyKind::Error => true,
            TyKind::Infer(v) => v.is_int() || v.1 == crate::ty::InferKind::Float,
            _ => false,
        }
    }

    fn peel_refs(&self, mut t: Ty) -> Ty {
        while let TyKind::Ref(_, _, inner) = self.ck.tys.kind(t) {
            t = inner;
        }
        t
    }

    fn note_local(&mut self, local: u32, depth: u32) {
        let Some(info) = self.ck.locals.get(local as usize) else { return };
        if needs_drop(self.p, &self.ck.tys, info.ty) && !self.drop_locals.iter().any(|&(l, _)| l == local) {
            self.drop_locals.push((local, depth));
        }
    }

    /// Records the by-value bindings of a pattern as drop locals.
    fn pat_bindings(&mut self, pat: PatId, depth: u32) {
        if let PatRes::Binding { local, mode: BindMode::Move } = self.ck.pat_res(pat) {
            self.note_local(local, depth);
        }
        match self.p.ast.pat(pat) {
            Pat::TupleStruct { elems, .. } | Pat::Or(elems) | Pat::Tuple(elems) => {
                for &s in elems {
                    self.pat_bindings(s, depth);
                }
            }
            Pat::Struct { fields, .. } => {
                for &(_, s) in fields {
                    self.pat_bindings(s, depth);
                }
            }
            Pat::Slice { prefix, middle, suffix } => {
                for &s in prefix.iter().chain(suffix) {
                    self.pat_bindings(s, depth);
                }
                if let Some(m) = middle {
                    self.pat_bindings(*m, depth);
                }
            }
            Pat::Ref { inner, .. } | Pat::At { inner, .. } | Pat::Paren(inner) => self.pat_bindings(*inner, depth),
            _ => {}
        }
    }

    /// Walks a pattern's expressions (literal and range bounds).
    fn pat_exprs(&mut self, pat: PatId, depth: u32) {
        match self.p.ast.pat(pat) {
            Pat::Lit(e) => self.expr(*e, false, depth, false),
            Pat::Range { lo, hi, .. } => {
                for e in [lo, hi].into_iter().flatten() {
                    self.expr(*e, false, depth, false);
                }
            }
            Pat::TupleStruct { elems, .. } | Pat::Or(elems) | Pat::Tuple(elems) => {
                for &s in elems {
                    self.pat_exprs(s, depth);
                }
            }
            Pat::Struct { fields, .. } => {
                for &(_, s) in fields {
                    self.pat_exprs(s, depth);
                }
            }
            Pat::Slice { prefix, middle, suffix } => {
                for &s in prefix.iter().chain(suffix) {
                    self.pat_exprs(s, depth);
                }
                if let Some(m) = middle {
                    self.pat_exprs(*m, depth);
                }
            }
            Pat::Ref { inner, .. } | Pat::At { inner, .. } | Pat::Paren(inner) => self.pat_exprs(*inner, depth),
            _ => {}
        }
    }

    fn block(&mut self, b: ast::BlockId, consumed: bool, depth: u32) {
        let blk = self.p.ast.block(b);
        for stmt in &blk.stmts {
            match stmt {
                Stmt::Let { pat, init, els, .. } => {
                    self.pat_bindings(*pat, depth);
                    self.pat_exprs(*pat, depth);
                    if let Some(init) = init {
                        // A whole-value binding owns the initializer's
                        // value, so its drop is judged by the binding
                        // rule, not the temporary rule. Any other
                        // pattern (`_`, a destructuring) destroys what
                        // it does not move out, so the temporary rule
                        // judges the initializer.
                        let held = crate::ty::consteval::pat_is_whole_binding(&self.p.ast, *pat);
                        self.expr(*init, false, depth, held);
                    }
                    if let Some(els) = els {
                        self.block(*els, false, depth + 1);
                    }
                }
                Stmt::Expr { expr, .. } => self.expr(*expr, false, depth, false),
                // Nested items are their own items: a nested const or
                // static initializer runs through the const evaluator,
                // and a nested fn body is checked as that fn.
                Stmt::Item(_) | Stmt::Macro { .. } => {}
            }
        }
        if let Some(tail) = blk.tail {
            self.expr(tail, consumed, depth, false);
        }
    }

    /// True when evaluation of `e` as a place reads an existing value
    /// instead of making a new one. A const item path is a value: each
    /// use makes a fresh copy of the const.
    fn is_place(&self, e: ExprId) -> bool {
        let ex = self.p.ast.expr(e);
        if matches!(ex, Expr::Path(_) | Expr::QPath { .. }) && self.ck.const_value(e).is_some() {
            return false;
        }
        matches!(
            ex,
            Expr::Path(_)
                | Expr::SelfValue
                | Expr::Field { .. }
                | Expr::TupleField { .. }
                | Expr::Deref(_)
                | Expr::Index { .. }
                | Expr::QPath { .. }
        )
    }

    /// Judges one callable: the resolution the checker put on a call
    /// or method call expression. `written` is the called name as
    /// written, when the call has one.
    fn call_res(&mut self, e: ExprId, what: &str, written: Option<&str>) {
        let span = self.p.ast.expr_span(e);
        match self.ck.res(e) {
            Res::Fn(f) => self.fn_call(f, span),
            Res::Method { f, .. } => self.fn_call(f, span),
            Res::Builtin(b) | Res::BuiltinMethod { b, .. } => {
                // Some builtins serve both an inherent const method
                // and a conversion trait method (`String::as_str` is
                // const, `<String as AsRef<str>>::as_ref` is not).
                // The written name tells them apart: a conversion
                // trait method is never a const call.
                let conv_trait = matches!(written, Some("as_ref" | "as_mut" | "borrow" | "borrow_mut" | "to_owned"));
                if conv_trait || !builtin_is_const(b) {
                    self.err(span, format!("cannot call non-const {what} in a const function"), "Move the call out of the const function.");
                }
            }
            // A primitive's associated fn by path (`u64::from_le_bytes`).
            Res::PrimAssoc { b, .. } => {
                if !builtin_is_const(b) {
                    self.err(span, format!("cannot call non-const {what} in a const function"), "Move the call out of the const function.");
                }
            }
            Res::Variant(..) | Res::OptSome | Res::OptNone | Res::ResOk | Res::ResErr | Res::ConstUsize(_) => {}
            Res::SizeOfVal { len, .. } => {
                if !builtin_is_const(len) {
                    self.err(span, format!("cannot call non-const {what} in a const function"), "Move the call out of the const function.");
                }
            }
            Res::OrderingOp { kind, .. } => {
                if kind == OrderingOpKind::ThenWith {
                    self.err(span, format!("cannot call non-const {what} in a const function"), "Move the call out of the const function.");
                }
            }
            Res::ClosureCall { .. } | Res::Indirect => {
                self.err(span, "cannot call a closure or function pointer in a const function", "Call a `const fn` by name instead.");
            }
            Res::None => {
                if !self.prior_errs {
                    self.err(
                        span,
                        format!("cannot call non-const {what} in a const function"),
                        "Call only `const fn` from a const function.",
                    );
                }
            }
            _ => {
                self.err(span, format!("cannot call non-const {what} in a const function"), "Call only `const fn` from a const function.");
            }
        }
    }

    /// A builtin method reached through a `Vec`, `String`, `Rc`, or `Arc`
    /// receiver that the type does not define itself (`v.first()` on a
    /// `Vec`) runs through `Deref`, which is not const (rustc E0658). A
    /// `Box` deref is built in, so it is allowed.
    fn deref_receiver(&mut self, e: ExprId, recv: ExprId) {
        use crate::ty::BuiltinTy;
        let Res::BuiltinMethod { b, .. } = self.ck.res(e) else { return };
        let t = self.peel_refs(self.ty(recv));
        let own = match self.ck.tys.builtin_of(t) {
            Some(BuiltinTy::Vec) => matches!(b, Builtin::VecLen | Builtin::VecIsEmpty | Builtin::VecCapacity | Builtin::VecAsSlice | Builtin::SeqAsPtr),
            Some(BuiltinTy::Boxed) => return,
            Some(BuiltinTy::String) => {
                matches!(b, Builtin::StringLen | Builtin::StringIsEmpty | Builtin::StringAsStr | Builtin::StringAsBytes | Builtin::StringCapacity)
            }
            _ if self.ck.tys.smart_elem(t).is_some() => false,
            _ => return,
        };
        if !own && builtin_is_const(b) {
            let tn = ty_name(self.p, &self.ck.tys, t);
            self.err(
                self.p.ast.expr_span(e),
                format!("cannot call this method through a deref of `{tn}` in a const function: its `Deref` impl is not const"),
                "Call the method on a slice or `str` reference, or move the call out of the const function.",
            );
        }
    }

    fn fn_call(&mut self, f: LocalDefId, span: Span) {
        let info = self.p.fn_info(f);
        if !info.is_const {
            let name = self.p.name(info.name);
            self.err(
                span,
                format!("cannot call non-const function `{name}` in a const function"),
                "Mark the function `const fn`, or move the call out of the const function.",
            );
        }
    }

    /// The operator rule: built-in operators run on scalar values
    /// only. Every other operator goes through a trait impl, and no
    /// operator trait is const in rustc 1.99 without a const bound.
    fn operator(&mut self, e: ExprId, operands: &[ExprId]) {
        let trait_form = matches!(self.ck.res(e), Res::TraitBin { .. } | Res::TraitCmp { .. });
        if trait_form || operands.iter().any(|&o| !self.scalar(self.ty(o))) {
            self.err(
                self.p.ast.expr_span(e),
                "cannot use this operator in a const function: it calls a non-const trait method",
                "Compare or compute on scalar values, or move the operation out of the const function.",
            );
        }
    }

    /// Walks one expression. `consumed` says the value moves out of
    /// the function (a tail, a `return`, a call argument, or a field
    /// of a moved-out aggregate); `depth` counts the conditional and
    /// loop bodies around the expression. `held` exempts the top
    /// expression from the temporary-drop rule (a `let` initializer,
    /// whose value the bindings own).
    fn expr(&mut self, e: ExprId, consumed: bool, depth: u32, held: bool) {
        let ex = self.p.ast.expr(e);
        if consumed {
            if let Res::Local(l) = self.ck.res(e) {
                self.moved_out.push((l, depth));
            }
        }
        match ex {
            Expr::Int(..) | Expr::Bool(_) | Expr::Str(_) | Expr::ByteStr(_) | Expr::Char(_) | Expr::Unit | Expr::Float(_) | Expr::Underscore => {}
            Expr::Path(_) | Expr::SelfValue | Expr::QPath { .. } => {}
            Expr::Block(b) | Expr::UnsafeBlock(b) => self.block(*b, consumed, depth),
            Expr::LabeledBlock { body, .. } => self.block(*body, consumed, depth),
            // The const evaluator checks inline const blocks.
            Expr::ConstBlock(_) => {}
            Expr::If { cond, then, els } => {
                self.expr(*cond, false, depth, false);
                self.block(*then, consumed, depth + 1);
                if let Some(els) = els {
                    self.expr(*els, consumed, depth + 1, false);
                }
            }
            Expr::While { cond, body, .. } => {
                self.expr(*cond, false, depth, false);
                self.loop_consumed.push(false);
                self.block(*body, false, depth + 1);
                self.loop_consumed.pop();
            }
            Expr::Loop { body, .. } => {
                self.loop_consumed.push(consumed);
                self.block(*body, false, depth + 1);
                self.loop_consumed.pop();
            }
            Expr::For { pat, iter, body, .. } => {
                self.err(
                    self.p.ast.expr_span(e),
                    "`for` loops are not allowed in a const function",
                    "Use `while` or `loop` in a const function.",
                );
                self.pat_bindings(*pat, depth + 1);
                self.expr(*iter, false, depth, false);
                self.loop_consumed.push(false);
                self.block(*body, false, depth + 1);
                self.loop_consumed.pop();
            }
            Expr::Match { scrut, arms } => {
                self.expr(*scrut, false, depth, false);
                for arm in arms {
                    self.pat_bindings(arm.pat, depth + 1);
                    self.pat_exprs(arm.pat, depth);
                    if let Some(g) = arm.guard {
                        self.expr(g, false, depth + 1, false);
                    }
                    self.expr(arm.body, consumed, depth + 1, false);
                }
            }
            Expr::Let { pat, scrut } => {
                self.pat_bindings(*pat, depth + 1);
                self.pat_exprs(*pat, depth);
                self.expr(*scrut, false, depth, false);
            }
            Expr::Call { callee, args } => {
                let written = match self.p.ast.expr(*callee) {
                    Expr::Path(segs) | Expr::QPath { segs, .. } => segs.last().map(|s| self.p.name(s.name)),
                    _ => None,
                };
                self.call_res(e, "function", written);
                self.expr(*callee, false, depth, false);
                for &a in args {
                    self.expr(a, true, depth, false);
                }
            }
            Expr::MethodCall { recv, name, args, .. } => {
                let written = Some(self.p.name(*name));
                self.call_res(e, "method", written);
                self.deref_receiver(e, *recv);
                // A by-value receiver moves into the callee; a borrowed
                // receiver stays a place of the caller.
                let by_value = matches!(
                    self.ck.res(e),
                    Res::Method { autoref: Autoref::None, .. } | Res::BuiltinMethod { autoref: Autoref::None, .. }
                );
                self.expr(*recv, by_value, depth, false);
                for &a in args {
                    self.expr(a, true, depth, false);
                }
            }
            Expr::Field { base, .. } | Expr::TupleField { base, .. } => {
                let bt = self.peel_refs(self.ty(*base));
                if self.ck.tys.smart_elem(bt).is_some() {
                    self.err(
                        self.p.ast.expr_span(e),
                        "cannot dereference a smart pointer in a const function: its `Deref` impl is not const",
                        "Move the access out of the const function.",
                    );
                }
                self.expr(*base, false, depth, false);
            }
            Expr::Index { base, index } => {
                match self.ck.res(e) {
                    Res::SliceIndex { .. } => {
                        self.err(
                            self.p.ast.expr_span(e),
                            "range indexing is not allowed in a const function",
                            "Index one element, or move the indexing out of the const function.",
                        );
                    }
                    Res::None if self.prior_errs => {}
                    _ => {
                        let bt = self.peel_refs(self.ty(*base));
                        if !matches!(self.ck.tys.kind(bt), TyKind::Array(..) | TyKind::Slice(_) | TyKind::Error) {
                            self.err(
                                self.p.ast.expr_span(e),
                                "cannot index this type in a const function: its `Index` impl is not const",
                                "Index an array or a slice, or move the indexing out of the const function.",
                            );
                        }
                    }
                }
                self.expr(*base, false, depth, false);
                self.expr(*index, false, depth, false);
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.operator(e, &[*lhs, *rhs]);
                self.expr(*lhs, false, depth, false);
                self.expr(*rhs, false, depth, false);
            }
            Expr::Unary { op, expr } => {
                if matches!(op, UnOp::Neg | UnOp::Not) {
                    self.operator(e, &[*expr]);
                }
                self.expr(*expr, false, depth, false);
            }
            Expr::Cast { expr, .. } => {
                let inner = self.ck.tys.kind(self.ty(*expr));
                let outer = self.ck.tys.kind(self.ty(e));
                if matches!(inner, TyKind::RawPtr(..) | TyKind::Ref(..) | TyKind::FnPtr(_) | TyKind::FnDef(..))
                    && matches!(outer, TyKind::Int(_))
                {
                    self.err(
                        self.p.ast.expr_span(e),
                        "pointers cannot be cast to integers in a const function",
                        "Move the cast out of the const function.",
                    );
                }
                self.expr(*expr, consumed, depth, false);
            }
            Expr::Ref { expr, .. } => self.expr(*expr, false, depth, false),
            Expr::Deref(inner) => {
                match self.ck.tys.kind(self.ty(*inner)) {
                    TyKind::Ref(..) | TyKind::RawPtr(..) | TyKind::Error => {}
                    _ => {
                        self.err(
                            self.p.ast.expr_span(e),
                            "cannot dereference this type in a const function: its `Deref` impl is not const",
                            "Move the dereference out of the const function.",
                        );
                    }
                }
                self.expr(*inner, false, depth, false);
            }
            Expr::Assign { lhs, rhs } => {
                if needs_drop(self.p, &self.ck.tys, self.ty(*lhs)) {
                    let tn = ty_name(self.p, &self.ck.tys, self.ty(*lhs));
                    self.err(
                        self.p.ast.expr_span(e),
                        format!("destructor of `{tn}` cannot run in a const function"),
                        "Assign to a place of a scalar type, or remove the `const`.",
                    );
                }
                self.expr(*lhs, false, depth, false);
                self.expr(*rhs, false, depth, false);
            }
            Expr::AssignOp { lhs, rhs, .. } => {
                self.operator(e, &[*lhs, *rhs]);
                self.expr(*lhs, false, depth, false);
                self.expr(*rhs, false, depth, false);
            }
            Expr::StructLit { fields, update, .. } => {
                for &(_, _, fe) in fields {
                    self.expr(fe, consumed, depth, false);
                }
                if let Some(u) = update {
                    self.expr(*u, consumed, depth, false);
                }
            }
            Expr::Tuple(elems) | Expr::Array(elems) => {
                for &el in elems {
                    self.expr(el, consumed, depth, false);
                }
            }
            Expr::ArrayRepeat { value, len } => {
                self.expr(*value, consumed, depth, false);
                // The length is a const context of its own; the const
                // evaluator owns it.
                let _ = len;
            }
            Expr::Range { lo, hi, .. } => {
                for x in [lo, hi].into_iter().flatten() {
                    self.expr(*x, false, depth, false);
                }
            }
            Expr::Break { value, .. } => {
                if let Some(v) = value {
                    let c = self.loop_consumed.last().copied().unwrap_or(false);
                    self.expr(*v, c, depth, false);
                }
            }
            Expr::Continue(_) => {}
            Expr::Return(v) => {
                if let Some(v) = v {
                    self.expr(*v, true, depth, false);
                }
            }
            Expr::Macro { kind, args, fmt, .. } => {
                let nonconst = match kind {
                    MacroKind::Println => Some("println!"),
                    MacroKind::Print => Some("print!"),
                    MacroKind::Eprintln => Some("eprintln!"),
                    MacroKind::Eprint => Some("eprint!"),
                    MacroKind::Format => Some("format!"),
                    MacroKind::FormatArgs => Some("format_args!"),
                    MacroKind::Vec => Some("vec!"),
                    MacroKind::AssertEq => Some("assert_eq!"),
                    MacroKind::AssertNe => Some("assert_ne!"),
                    MacroKind::Panic | MacroKind::Assert | MacroKind::Todo | MacroKind::Unreachable => None,
                };
                if let Some(name) = nonconst {
                    self.err(
                        self.p.ast.expr_span(e),
                        format!("cannot call non-const macro `{name}` in a const function"),
                        "Move the macro call out of the const function.",
                    );
                }
                // The panic family is const only in its plain forms
                // (rustc E0015): a literal message, or exactly one
                // `{}` with a `&str` argument that is not a capture.
                if nonconst.is_none() {
                    let span = self.p.ast.expr_span(e);
                    match const_macro_form(*kind, fmt) {
                        ConstMacroForm::Bad => {
                            self.err(
                                span,
                                "cannot call non-const formatting macro in a const function",
                                "Use a plain literal message, such as `panic!(\"bad\")`, or move the macro out of the const function.",
                            );
                        }
                        ConstMacroForm::OneStrArg(index) => {
                            let arg = args[index];
                            if is_fmt_capture(&self.p.ast, arg, |s| self.p.name(s).len()) {
                                self.err(
                                    span,
                                    "cannot call non-const formatting macro in a const function",
                                    "Use a plain literal message, such as `panic!(\"bad\")`, or move the macro out of the const function.",
                                );
                            } else {
                                let t = self.ty(arg);
                                let is_str = match self.ck.tys.kind(t) {
                                    TyKind::Ref(_, _, inner) => matches!(self.ck.tys.kind(inner), TyKind::Str),
                                    TyKind::Str | TyKind::Error => true,
                                    _ => false,
                                };
                                if !is_str {
                                    self.err(
                                        span,
                                        "argument to `panic!()` in a const context must have type `&str`",
                                        "Use a `&str` message, or move the macro out of the const function.",
                                    );
                                }
                            }
                        }
                        ConstMacroForm::Plain => {}
                    }
                }
                for &a in args {
                    self.expr(a, false, depth, false);
                }
            }
            // A closure value may be made here; its body is not a const
            // context, and calling it rejects at the call.
            Expr::Closure { .. } => {}
            Expr::Try(inner) => {
                self.err(
                    self.p.ast.expr_span(e),
                    "`?` is not allowed in a const function",
                    "Use `match` on the value in a const function.",
                );
                self.expr(*inner, false, depth, false);
            }
            Expr::Await(inner) => {
                self.err(
                    self.p.ast.expr_span(e),
                    "`.await` is not allowed in a const function",
                    "Move the await out of the const function.",
                );
                self.expr(*inner, false, depth, false);
            }
            Expr::AsyncBlock { .. } => {
                self.err(
                    self.p.ast.expr_span(e),
                    "`async` blocks are not allowed in a const function",
                    "Move the block out of the const function.",
                );
            }
            Expr::MacroCall(_) | Expr::OffsetOf { .. } => {}
        }
        // The temporary rule (E0493): a new value of a drop type that
        // does not move out is destroyed inside the function.
        if !consumed && !held && !self.is_place(e) && needs_drop(self.p, &self.ck.tys, self.ty(e)) {
            let tn = ty_name(self.p, &self.ck.tys, self.ty(e));
            self.err(
                self.p.ast.expr_span(e),
                format!("destructor of `{tn}` cannot run in a const function"),
                "Move the value out of the function, or remove the `const`.",
            );
        }
    }
}
