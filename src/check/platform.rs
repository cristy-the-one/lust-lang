// Checks for the std platform crate's generic code (W-SHIM; the crate
// is library/std/src/platform.rs, loaded by the resolver).
//
// `io::Write::write_fmt` in lust formats the whole `write!` text first
// and writes it with one `write_all`, except on the std stream types
// (`Stdout`, `Stderr`, and their locks), which lowering streams piece
// by piece. For std's other writers that is the
// output std gives, but std calls a writer's `write` once per format
// piece, so a user writer that counts or logs its `write` calls would
// see a different sequence. Materialize asks `user_write_fmt_instance`
// about every new instance; the default `write_fmt` for a user writer
// type rejects with L0641 instead of running with different behavior.
use crate::check::collect::Program;
use crate::def::LocalDefId;
use crate::diag::{Diag, Span};
use crate::ty::lang::ShippedCrate;
use super::Ctx;
use crate::ty::{AdtId, Mut, Ty, TyIntern, TyKind};

/// L0641 when `inst` is the default `io::Write::write_fmt` body of the
/// platform crate instantiated for a writer that std does not define.
/// The `&mut W` forwarding impl is not the default: its inner writer
/// gets its own instance, which this check sees.
pub fn user_write_fmt_instance(p: &Program, inst: LocalDefId) -> Option<Diag> {
    if !is_platform_write_fmt(p, inst) {
        return None;
    }
    let writer = writer_of(p, inst)?;
    if matches!(p.tys.kind(writer), TyKind::Ref(..)) {
        return None;
    }
    user_writer_diag(p, &p.tys, writer, p.fn_info(inst).def(&p.ast).name_span)
}

/// The type behind a method instance's `&self` or `&mut self` param.
pub fn writer_of(p: &Program, inst: LocalDefId) -> Option<Ty> {
    match p.tys.kind(*p.fn_info(inst).params.first()?) {
        TyKind::Ref(_, _, t) => Some(t),
        _ => None,
    }
}

/// The same rule at a `write!` call `lust check` sees, for a receiver
/// whose type is already concrete (`f` is the method the call picked,
/// `writer` the receiver with references peeled). Generic and `dyn`
/// receivers are left to `user_write_fmt_instance` in a build.
pub fn user_write_fmt_call(p: &Program, tys: &impl TyIntern, f: LocalDefId, writer: Ty, span: Span) -> Option<Diag> {
    if !is_platform_write_fmt(p, f) {
        return None;
    }
    // At the call, not at the type: each `write!` is a site to rewrite
    // (src/rewrite.rs reads the call's text there).
    let mut d = user_writer_diag(p, tys, writer, span)?;
    if d.span != span {
        d.labels.push(crate::diag::Label { span: d.span, text: "the writer type".into() });
        d.span = span;
    }
    Some(d)
}

/// True for a `write_fmt` fn of the platform crate (the trait default,
/// the `&mut W` forwarder, or one of their instances).
pub(crate) fn is_platform_write_fmt(p: &Program, f: LocalDefId) -> bool {
    let info = p.fn_info(f);
    p.name(info.name) == "write_fmt" && p.shipped_crate_at(info.def(&p.ast).name_span) == Some(ShippedCrate::Std)
}

/// L0641 unless `writer` is a std writer: `Vec<u8>` or a type the
/// platform crate defines. Params and other non-Adt types say nothing.
fn user_writer_diag(p: &Program, tys: &impl TyIntern, writer: Ty, fallback: Span) -> Option<Diag> {
    if tys.vec_elem(writer).is_some() {
        return None;
    }
    let a = tys.local_adt(writer)?;
    if p.shipped_crate_at(p.adt(a).span) == Some(ShippedCrate::Std) {
        return None;
    }
    let span = if p.adt(a).span == Span::default() { fallback } else { p.adt(a).span };
    Some(Diag::new(
        "L0641",
        span,
        crate::rewrite::WRITE_USER,
        "Format into a `String` with `format!`, then call `write_all` with its bytes.",
    ))
}

/// True for std's `io::copy` in the platform crate (library/std/src/io.rs),
/// or one of its instances.
pub(crate) fn is_platform_io_copy(p: &Program, f: LocalDefId) -> bool {
    let info = p.fn_info(f);
    p.shipped_crate_at(info.def(&p.ast).name_span) == Some(ShippedCrate::Std) && p.display_path(info.name) == "std::io::copy"
}

/// L0641 when std 1.99 would not run `io::copy(&mut R, &mut W)` as lust's
/// `stack_buffer_copy` port does: std specializes on the static types
/// (alloc/src/io/copy/generic.rs, std/src/sys/io/kernel_copy/linux.rs).
/// An in-memory or buffered reader (`&[u8]`, `VecDeque<u8>`, `BufReader`)
/// or writer (`BufWriter`, `Vec<u8>`) takes another loop, and an OS
/// handle on both sides copies in the kernel. A reader or writer still
/// generic says nothing here (its instance answers in a build).
pub fn io_copy_diag(p: &Program, tys: &impl TyIntern, r: Ty, w: Ty, span: Span) -> Option<Diag> {
    let name = |t: Ty| -> Option<String> {
        let a = tys.local_adt(t)?;
        (p.shipped_crate_at(p.adt(a).span).is_some()).then(|| p.display_path(p.adt(a).name))
    };
    let unknown = |t: Ty| matches!(tys.kind(t), TyKind::Param(_) | TyKind::Opaque(..) | TyKind::Projection(..) | TyKind::Infer(_));
    if unknown(r) || unknown(w) {
        return None;
    }
    let ends = |n: &Option<String>, list: &[&str]| n.as_deref().is_some_and(|n| list.iter().any(|x| n.ends_with(x)));
    let in_memory_reader = match tys.kind(r) {
        TyKind::Ref(_, _, inner) => tys.kind(inner) == TyKind::Slice(crate::ty::int(crate::ty::IntTy::U8)),
        _ => ends(&name(r), &["::BufReader", "::VecDeque"]),
    };
    let buffered_writer = tys.vec_elem(w) == Some(crate::ty::int(crate::ty::IntTy::U8)) || ends(&name(w), &["::BufWriter"]);
    // An OS handle (std's `CopyRead` / `CopyWrite` impls), through `&` for
    // the shared handles and `&mut` for every one.
    fn handle(p: &Program, tys: &impl TyIntern, t: Ty, list: &[&str], by_ref: &[&str]) -> bool {
        let named = |t: Ty, list: &[&str]| {
            tys.local_adt(t).is_some_and(|a| p.shipped_crate_at(p.adt(a).span).is_some() && list.iter().any(|x| p.display_path(p.adt(a).name).ends_with(x)))
        };
        match tys.kind(t) {
            TyKind::Ref(_, Mut::Mut, inner) => handle(p, tys, inner, list, by_ref),
            TyKind::Ref(_, _, inner) => named(inner, by_ref),
            _ => named(t, list),
        }
    }
    const SHARED: &[&str] = &["::File", "::TcpStream", "::UnixStream"];
    let os_reader = handle(p, tys, r, &["::File", "::TcpStream", "::UnixStream", "::ChildStdout", "::ChildStderr", "::StdinLock"], SHARED);
    let os_writer = handle(p, tys, w, &["::File", "::TcpStream", "::UnixStream", "::ChildStdin", "::StdoutLock", "::StderrLock"], SHARED);
    let what = if in_memory_reader {
        "an in-memory or buffered reader"
    } else if buffered_writer {
        "a `Vec<u8>` or `BufWriter` writer"
    } else if os_reader && os_writer {
        "an OS handle on both sides (std copies in the kernel)"
    } else {
        return None;
    };
    Some(Diag::new(
        "L0641",
        span,
        format!("`std::io::copy` with {what} is not supported yet"),
        "Copy through a plain reader and writer, or loop with `read` and `write_all`.",
    ))
}

/// `io_copy_diag` for an instance of `io::copy` a build materializes.
pub fn io_copy_instance(p: &Program, inst: LocalDefId) -> Option<Diag> {
    if !is_platform_io_copy(p, inst) {
        return None;
    }
    let f = p.fn_info(inst);
    let peel = |t: Ty| match p.tys.kind(t) {
        TyKind::Ref(_, _, inner) => Some(inner),
        _ => None,
    };
    let (r, w) = (peel(*f.params.first()?)?, peel(*f.params.get(1)?)?);
    io_copy_diag(p, &p.tys, r, w, f.def(&p.ast).name_span)
}

/// Fns that are `unsafe fn` in rustc's std but safe fns in lust's std
/// source (lust source cannot spell `unsafe fn`): `FromRawFd::from_raw_fd`
/// (the trait method and every impl of it) and `BorrowedFd::borrow_raw`.
/// Shipped crates call them; user code that names one rejects, as rustc
/// rejects the call outside an `unsafe` block (E0133).
const UPSTREAM_UNSAFE_FNS: &[&str] = &["from_raw_fd", "borrow_raw"];

/// The name prefix of adapter helper fns that upstream lacks (the hooks
/// of an adapter's sealed traits, rustix `Arg` and `Buffer`). User code
/// cannot name them, and cannot define one in an impl of a shipped trait.
const HIDDEN_PREFIX: &str = "__lust_";

/// The name prefix of std fns that only adapter crates may call (a std
/// hook upstream's std lacks, `io::Error::__lust_adapter_error`). User
/// code that names one rejects as an adapter's `__lust_` hook does.
const ADAPTER_ONLY_PREFIX: &str = "__lust_adapter_";

/// True when `span` is in shipped source: spliced std, the platform
/// crate, or an adapter crate's files.
fn shipped_span(p: &Program, span: Span) -> bool {
    p.shipped_crate_at(span).is_some() || p.adapter_at(span).is_some() || p.std_source_span(span)
}

/// The impl-level half of the rules above, for a user impl of trait
/// `trait_def` with methods `methods`: implementing std's `FromRawFd`
/// needs an `unsafe fn` (L0404), and a `__lust_` method in an impl of a
/// shipped trait names a member upstream's trait does not have.
pub fn user_impl_of_hidden(p: &Program, impl_span: Span, trait_def: crate::def::DefId, methods: &[LocalDefId]) -> Option<Diag> {
    if shipped_span(p, impl_span) {
        return None;
    }
    let tdata = p.defs.get(trait_def)?;
    if !shipped_span(p, tdata.span) {
        return None;
    }
    if p.display_path(tdata.name) == "std::os::fd::FromRawFd" {
        return Some(Diag::new(
            "L0404",
            impl_span,
            "implementing `FromRawFd` needs an `unsafe fn from_raw_fd`, and `unsafe` is not supported",
            "Build the value from an owned fd with a safe constructor instead.",
        ));
    }
    let hidden = methods.iter().find(|&&m| p.name(p.fn_info(m).name).starts_with(HIDDEN_PREFIX))?;
    let f = p.fn_def(*hidden);
    Some(Diag::new(
        "L0110",
        f.name_span,
        format!("method `{}` is not a member of the trait", p.name(p.fn_info(*hidden).name)),
        "Remove the method.",
    ))
}

impl Ctx<'_> {
    /// Body end: user code that names a fn of a shipped crate that is
    /// `unsafe` upstream (L0404) or a hidden `__lust_` helper (L0104).
    pub(super) fn hidden_fn_uses(&mut self) {
        let p = self.p;
        for i in 0..self.out.expr_res.len() {
            let f = match self.out.expr_res[i] {
                super::Res::Fn(f) | super::Res::Method { f, .. } => f,
                _ => continue,
            };
            // Hidden: an adapter's `__lust_` hook, or a std hook for adapters
            // (std's other internal `__lust_` fns sit behind public std paths
            // and stay usable).
            let def_span = p.fn_def(f).name_span;
            let name = p.name(p.fn_info(f).name);
            let hidden = name.starts_with(HIDDEN_PREFIX) && p.adapter_at(def_span).is_some() || name.starts_with(ADAPTER_ONLY_PREFIX);
            if !hidden && !UPSTREAM_UNSAFE_FNS.contains(&name) {
                continue;
            }
            let e = crate::ast::ExprId(self.out.expr_base + i as u32);
            let span = p.ast.expr_span(e);
            if shipped_span(p, span) || !shipped_span(p, p.fn_def(f).name_span) {
                continue;
            }
            if hidden {
                self.err("L0104", span, format!("no function or method `{name}`"), "Use the crate's public API.");
            } else {
                self.err(
                    "L0404",
                    span,
                    format!("`{name}` is an `unsafe fn` in std; calling it needs `unsafe`, which is not supported"),
                    "Use a safe std API instead.",
                );
            }
        }
    }

    /// True, after an L0104 error, when user code names a hidden variant
    /// of an enum a shipped crate defines: std's unstable `io::ErrorKind`
    /// kinds carry a `__` prefix in lust's source, and rustc has no such
    /// variant (E0599). The shipped crates themselves may name them.
    pub(super) fn hidden_variant(&mut self, a: AdtId, v: u32, span: Span) -> bool {
        let p = self.p;
        let name = p.adt(a).variants[v as usize].name;
        if !p.interner.get(name).starts_with("__") || p.shipped_crate_at(span).is_some() || p.shipped_crate_at(p.adt(a).span).is_none() {
            return false;
        }
        let shown = p.interner.get(name)[2..].to_string();
        self.err("L0104", span, format!("no variant named `{shown}`"), "Use a stable variant of the enum.");
        true
    }
}
