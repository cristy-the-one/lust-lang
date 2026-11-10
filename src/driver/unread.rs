// `--mark-unread <path>`: which calls of a named fn have a result the
// program never reads.
//
// A host that runs lust-built programs can hold back the work behind such
// a call, because the program cannot see when it happened (Lime Slice
// queues the solid operations of a recipe whose `apply` replies nobody
// reads). The host named these calls by scanning the source text; this
// reads them from the checked program instead, so a call is the named fn
// by resolution, not by its spelling, and comments, strings and line
// breaks do not matter.
//
// For a build or check that passes, with `--json`, each such call is one
// line on stdout after the diagnostics:
//
//     {"schema":1,"kind":"unread_result","fn":"<path as given>","file":..,"span":{..}}
//
// The span starts where the call reports its caller location
// (`Location::caller()`): the method name of a method call, the whole
// expression of a path call. That is the line a `#[track_caller]` fn sees.
//
// "Never read" is decided from the shape around the call, and a shape
// that is not listed counts as read:
//
// - the call is a statement (`f(..);`) or the value of `let _ = ..;`;
// - `?`, `.unwrap()` or `.expect(..)` on it, and that value is a statement
//   or the value of `let _ = ..;`;
// - `.is_ok()` or `.is_err()` on it, anywhere: a bool says nothing of the
//   value;
// - `match` on it where every arm's pattern is `Ok(_)`, `Ok(..)`, `Err(..)`
//   or `_`; `if let` or `while let` on it with `Ok(_)`, `Ok(..)` or
//   `Err(..)`.
//
// A binding that is never used and a helper that returns the result are
// read, for now. Nothing here changes what is built: it is a report on a
// valid Rust program.

use std::sync::Arc;

use super::Session;
use super::patch::KeptProgram;
use crate::ast::{Ast, BlockId, Expr, ExprId, Pat, PatId, Stmt};
use crate::check::collect::Program;
use crate::check::{Checked, PatRes, Res};
use crate::def::LocalDefId;
use crate::diag::{CtxtId, Span, json_str, span_json};
use crate::intern::{FxMap, Sym};
use crate::protocol::{Request, Response};

/// Appends the `unread_result` records of `req.mark_unread` to a passing
/// `--json` answer. Any other answer is returned as it is.
pub fn append(req: &Request, session: &mut Session, resp: &mut Response) {
    if req.mark_unread.is_empty() || !req.json || resp.code != 0 {
        return;
    }
    let front = &mut session.front;
    let Some(p) = kept(&mut front.kept, &front.ast) else { return };
    let (map, ast_fns) = (&front.map, front.ast_fn_count);
    let found = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scan(p, ast_fns, &req.mark_unread)));
    let Ok(mut found) = found else { return };
    found.sort_by_key(|(_, span)| (span.file.0, span.lo));
    found.dedup();
    for (name, span) in found {
        let line =
            format!("{{\"schema\":1,\"kind\":\"unread_result\",\"fn\":{},\"file\":{},\"span\":{}}}\n", json_str(&name), json_str(map.path(span.file)), span_json(map, span));
        resp.stdout.push_str(&line);
    }
}

fn kept<'a>(kept: &'a mut [Option<KeptProgram>; 2], ast: &Arc<Ast>) -> Option<&'a mut Program> {
    kept.iter_mut().flatten().find(|k| k.attached_to(ast)).map(|k| &mut k.program)
}

/// True when `shown`, a fn's path as lust shows it, is the fn `wanted`
/// names: the same path, or the same crate and the same last two segments
/// (a type's method, where the host names the type by its public path and
/// lust shows the module it is defined in). A two-segment `wanted` matches
/// by its last segment the same way.
fn names(shown: &str, wanted: &str) -> bool {
    if shown == wanted {
        return true;
    }
    let (s, w): (Vec<&str>, Vec<&str>) = (shown.split("::").collect(), wanted.split("::").collect());
    if w.len() < 2 || s.len() < w.len() || s[0] != w[0] {
        return false;
    }
    let tail = if w.len() == 2 { 1 } else { 2 };
    s[s.len() - tail..] == w[w.len() - tail..]
}

/// Every call of a fn `wanted` names whose result is never read: the
/// name as given and the call's caller-location span.
fn scan(p: &mut Program, ast_fns: usize, wanted: &[String]) -> Vec<(String, Span)> {
    // A body can only call the fn if it writes its last segment.
    let last: Vec<Sym> = wanted.iter().filter_map(|w| p.interner.find(w.rsplit("::").next().unwrap_or(w))).collect();
    let mut out = Vec::new();
    if last.is_empty() {
        return out;
    }
    for fi in 0..ast_fns.min(p.fns.len()) {
        let def = p.fns[fi].loc.def(&p.ast);
        let (Some(body), exprs) = (def.body, def.exprs) else { continue };
        // User source only: the shipped crates' own calls are not the host's.
        if p.std_source_span(def.name_span) || p.in_lang_methods(def.name_span) || p.shipped_crate_at(def.name_span).is_some() {
            continue;
        }
        let ids = || (exprs.0..exprs.1).map(ExprId);
        let mentions = ids().any(|e| match p.ast.expr(e) {
            Expr::MethodCall { name, .. } => last.contains(name),
            Expr::Path(segs) => segs.last().is_some_and(|s| last.iter().any(|l| p.interner.get(s.name).ends_with(p.interner.get(*l)))),
            _ => false,
        });
        if !mentions {
            continue;
        }
        crate::driver::graph::FpMemo::sync(p);
        let p: &Program = p;
        let (ck, errs) = crate::check::check_fn(p, LocalDefId(fi as u32));
        if !errs.is_empty() {
            continue;
        }
        let uses = Uses::of(p, &ck, body, exprs);
        for e in ids() {
            if p.ast.expr_span(e).ctxt != CtxtId::ROOT || !uses.has(&ck, e) {
                continue;
            }
            let (f, span) = match (p.ast.expr(e), ck.res(e)) {
                (Expr::MethodCall { name_span, .. }, Res::Method { f, .. }) => (f, *name_span),
                (Expr::Call { callee, .. }, res) => match (res, ck.res(*callee)) {
                    (Res::Fn(f), _) | (_, Res::Fn(f)) => (f, p.ast.expr_span(e)),
                    _ => continue,
                },
                _ => continue,
            };
            // A method shows as its type's path and its own name.
            let info = &p.fns[f.0 as usize];
            let shown = match info.owner.and_then(|a| p.adts.get(a.0 as usize)) {
                Some(adt) => format!("{}::{}", p.display_path(adt.name), p.display_name(info.name)),
                None => p.display_path(info.name),
            };
            if let Some(w) = wanted.iter().find(|w| names(&shown, w))
                && uses.unread(p, &ck, e)
            {
                out.push((w.clone(), span));
            }
        }
    }
    out
}

/// What consumes an expression's value.
enum Use {
    /// `?`, `.unwrap()`, `.expect(..)`: the `Ok` value goes on as this expression.
    Unwrap(ExprId),
    /// `.is_ok()`, `.is_err()`.
    Test,
    /// A `match` on it, with the arm patterns.
    Match(Vec<PatId>),
    /// `if let` / `while let` on it.
    Let(PatId),
}

/// The uses of the expressions of one fn body, by shape.
struct Uses {
    /// Values a statement throws away: `e;` and `let _ = e;`.
    dropped: Vec<ExprId>,
    consumer: FxMap<ExprId, Use>,
}

impl Uses {
    fn of(p: &Program, ck: &Checked, body: BlockId, exprs: (u32, u32)) -> Uses {
        let ast = &*p.ast;
        let mut u = Uses { dropped: Vec::new(), consumer: FxMap::default() };
        let mut blocks = vec![body];
        let word = |s: Sym| p.interner.get(s);
        for e in (exprs.0..exprs.1).map(ExprId) {
            match ast.expr(e) {
                Expr::Block(b) | Expr::UnsafeBlock(b) | Expr::ConstBlock(b) => blocks.push(*b),
                Expr::If { then: b, .. } | Expr::While { body: b, .. } | Expr::Loop { body: b, .. } | Expr::For { body: b, .. } => blocks.push(*b),
                Expr::AsyncBlock { body: b, .. } | Expr::LabeledBlock { body: b, .. } => blocks.push(*b),
                Expr::Try(x) => {
                    u.consumer.insert(*x, Use::Unwrap(e));
                }
                Expr::MethodCall { recv, name, args, .. } => match (word(*name), args.len()) {
                    ("unwrap", 0) | ("expect", 1) => {
                        u.consumer.insert(*recv, Use::Unwrap(e));
                    }
                    ("is_ok" | "is_err", 0) => {
                        u.consumer.insert(*recv, Use::Test);
                    }
                    _ => {}
                },
                Expr::Match { scrut, arms } => {
                    u.consumer.insert(*scrut, Use::Match(arms.iter().map(|a| a.pat).collect()));
                }
                Expr::Let { pat, scrut } => {
                    u.consumer.insert(*scrut, Use::Let(*pat));
                }
                _ => {}
            }
        }
        let mut i = 0;
        while i < blocks.len() {
            let b = ast.block(blocks[i]);
            i += 1;
            for s in &b.stmts {
                match s {
                    Stmt::Expr { expr, semi: true, .. } => u.dropped.push(*expr),
                    Stmt::Let { pat, init: Some(x), els, .. } => {
                        if matches!(ast.pat(*pat), Pat::Wild) && ck_has_pat(ck, *pat) {
                            u.dropped.push(*x);
                        }
                        blocks.extend(*els);
                    }
                    _ => {}
                }
            }
        }
        u
    }

    /// True when the checker typed `e` in this body.
    fn has(&self, ck: &Checked, e: ExprId) -> bool {
        e.0 >= ck.expr_base && ((e.0 - ck.expr_base) as usize) < ck.expr_res.len()
    }

    /// True when the value of call `e` is never read (the module comment).
    fn unread(&self, p: &Program, ck: &Checked, e: ExprId) -> bool {
        // `Ok(_)`, `Ok(..)`, `Err(anything)` or `_`: binds nothing of an `Ok` value.
        let blind = |pat: PatId, wild_ok: bool| {
            let mut pat = pat;
            while let Pat::Paren(inner) = p.ast.pat(pat) {
                pat = *inner;
            }
            if !ck_has_pat(ck, pat) {
                return false;
            }
            match (p.ast.pat(pat), ck.pat_res[(pat.0 - ck.pat_base) as usize]) {
                (Pat::Wild, _) => wild_ok,
                (Pat::TupleStruct { .. }, PatRes::Res(false)) => true,
                (Pat::TupleStruct { elems, .. }, PatRes::Res(true)) => matches!(elems[..], [x] if matches!(p.ast.pat(x), Pat::Wild | Pat::Rest)),
                _ => false,
            }
        };
        let mut at = e;
        // The `Result` itself, until a `?`, `unwrap` or `expect` takes its `Ok` value.
        let mut whole = true;
        loop {
            if self.dropped.contains(&at) {
                return true;
            }
            match self.consumer.get(&at) {
                Some(Use::Unwrap(next)) if whole => {
                    at = *next;
                    whole = false;
                }
                Some(Use::Test) if whole => return true,
                Some(Use::Match(pats)) if whole => return pats.iter().all(|&pat| blind(pat, true)),
                Some(Use::Let(pat)) if whole => return blind(*pat, false),
                _ => return false,
            }
        }
    }
}

fn ck_has_pat(ck: &Checked, pat: PatId) -> bool {
    pat.0 >= ck.pat_base && ((pat.0 - ck.pat_base) as usize) < ck.pat_res.len()
}

#[cfg(test)]
mod tests {
    use super::names;

    #[test]
    fn a_path_names_a_fn() {
        assert!(names("Session::apply", "Session::apply"));
        assert!(names("lime_recipe::Session::apply", "lime_recipe::Session::apply"));
        // The type by its public path, the fn shown in its module.
        assert!(names("lime_recipe::session::Session::apply", "lime_recipe::Session::apply"));
        assert!(names("dep::util::run", "dep::run"));
        assert!(!names("other::Session::apply", "lime_recipe::Session::apply"));
        assert!(!names("lime_recipe::Other::apply", "lime_recipe::Session::apply"));
        assert!(!names("lime_recipe::Session::apply_all", "lime_recipe::Session::apply"));
        assert!(!names("apply", "Session::apply"));
    }
}
