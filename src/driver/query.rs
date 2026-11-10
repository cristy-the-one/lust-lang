// `lust query`: what the checked program says about one source position.
//
// A query is a check request with a position (`protocol::Query`). The check
// runs first, through the same session as any `lust check` of the crate, so
// a warm daemon answers from the program it already holds and an edit since
// the last check is taken in on the way. Then `answer` reads the program the
// check kept (`patch::KeptProgram`): it finds the fn body the position is in
// and the smallest expression or pattern there, checks that one body again
// (the session keeps read sets and IR, not the checker's tables), and reads
// the answer from that check.
//
// Rule zero is untouched: this reads what the checker decided about valid
// Rust and adds no language. What it does not answer yet, it refuses:
// positions outside fn bodies (signatures, items, fields), names inside a
// macro call, field and const definitions, and anything defined in std.

use std::path::Path;
use std::sync::Arc;

use super::Session;
use super::patch::KeptProgram;
use crate::ast::{Ast, Expr, ExprId, PatId};
use crate::check::collect::{Program, ty_name};
use crate::check::{Checked, PatRes, Res};
use crate::def::LocalDefId;
use crate::diag::{CtxtId, FileId, SourceMap, Span, json_str, span_json};
use crate::protocol::{Query, QueryKind, Request, Response};

/// Before the check of a query request: forgets which program the session
/// kept, so `answer` can tell this check's program from an older one.
pub fn begin(session: &mut Session) {
    session.front.kept_fresh = false;
}

/// The answer to `req.query` after its check gave `check` in `session`.
/// Exit code 0 with the answer on stdout, or 1 with the reason on stderr.
/// The check's own diagnostics are not part of the answer.
pub fn answer(req: &Request, session: &mut Session, check: &Response) -> Response {
    let Some(q) = &req.query else { return check.clone() };
    let front = &mut session.front;
    // A clean check always leaves its program. An error answer leaves one
    // only when collect finished (`keep_err`): an older program would have
    // the positions of an older text.
    let usable = check.code == 0 || front.kept_fresh;
    let program = if usable { kept(&mut front.kept, &front.ast) } else { None };
    let Some(p) = program else {
        return refuse("the crate does not check far enough to answer; see `lust check`");
    };
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lookup(p, &front.map, front.ast_fn_count, q, req.json)));
    match out {
        Ok(Ok(stdout)) => Response { stdout, ..Default::default() },
        Ok(Err(why)) => refuse(&why),
        Err(_) => refuse("the checker could not answer for this position"),
    }
}

fn refuse(why: &str) -> Response {
    Response { stderr: format!("error: {why}\n"), code: 1, ..Default::default() }
}

/// The kept program that describes the session's current Ast: the check's,
/// else the build's. A program a body patch detached, or one of an older
/// Ast, is not it.
fn kept<'a>(kept: &'a mut [Option<KeptProgram>; 2], ast: &Arc<Ast>) -> Option<&'a mut Program> {
    kept.iter_mut().flatten().find(|k| k.attached_to(ast)).map(|k| &mut k.program)
}

/// The node a position names.
#[derive(Clone, Copy)]
enum Node {
    Expr(ExprId),
    Pat(PatId),
}

fn lookup(p: &mut Program, map: &SourceMap, ast_fns: usize, q: &Query, json: bool) -> Result<String, String> {
    let file = file_id(map, &q.file).ok_or_else(|| format!("{} is not a file of this crate", q.file.display()))?;
    let no_pos = || format!("{}:{}:{} is not a position in the file", q.file.display(), q.line, q.col);
    let off = map.lines(file).offset_of(q.line, q.col).ok_or_else(no_pos)?;
    // The root file's index goes on into the std source spliced after the
    // user's text: those lines are not the file's.
    let here = Span { file, lo: off, hi: off, ctxt: CtxtId::ROOT };
    if p.std_source_span(here) || p.in_lang_methods(here) || p.shipped_crate_at(here).is_some() {
        return Err(no_pos());
    }
    let at =|sp: Span| sp.file == file && sp.ctxt == CtxtId::ROOT && sp.lo <= off && off < sp.hi;
    // The innermost fn whose body holds the position (a nested fn's body
    // lies inside its parent's).
    let mut best: Option<(u32, usize)> = None;
    for i in 0..ast_fns.min(p.fns.len()) {
        let def = p.fns[i].loc.def(&p.ast);
        let Some(body) = def.body else { continue };
        let sp = p.ast.blocks[body.0 as usize].span;
        if at(sp) && best.is_none_or(|(len, _)| sp.hi - sp.lo < len) {
            best = Some((sp.hi - sp.lo, i));
        }
    }
    let (_, fi) = best.ok_or("the position is not inside a fn body of this build (code a cfg turns off is not checked)")?;
    let fid = LocalDefId(fi as u32);
    let (exprs, pats) = {
        let def = p.fns[fi].loc.def(&p.ast);
        (def.exprs, def.pats)
    };
    // The smallest expression or pattern of that fn at the position. Among
    // equal spans a path wins: it is the node that names something.
    let mut node: Option<(u32, bool, Node)> = None;
    let mut offer = |sp: Span, path: bool, n: Node| {
        if !at(sp) {
            return;
        }
        let len = sp.hi - sp.lo;
        if node.is_none_or(|(l, was_path, _)| len < l || (len == l && path && !was_path)) {
            node = Some((len, path, n));
        }
    };
    for e in exprs.0..exprs.1 {
        let id = ExprId(e);
        offer(p.ast.expr_span(id), matches!(p.ast.expr(id), Expr::Path(_)), Node::Expr(id));
    }
    for i in pats.0..pats.1 {
        let id = PatId(i);
        offer(p.ast.pat_span(id), false, Node::Pat(id));
    }
    let (_, _, node) = node.ok_or("no expression or pattern at the position (a macro call's inside is not covered)")?;
    crate::driver::graph::FpMemo::sync(p);
    let p: &Program = p;
    let (ck, _) = crate::check::check_fn(p, fid);
    let covers = |n: Node| match n {
        Node::Expr(e) => e.0 >= ck.expr_base && ((e.0 - ck.expr_base) as usize) < ck.expr_ty.len(),
        Node::Pat(i) => i.0 >= ck.pat_base && ((i.0 - ck.pat_base) as usize) < ck.pat_ty.len(),
    };
    if !covers(node) {
        return Err("the checker has no entry for this position".into());
    }
    let span = match node {
        Node::Expr(e) => p.ast.expr_span(e),
        Node::Pat(i) => p.ast.pat_span(i),
    };
    match q.kind {
        QueryKind::Type => {
            let ty = match node {
                Node::Expr(e) => ck.ty(e),
                Node::Pat(i) => ck.pat_ty[(i.0 - ck.pat_base) as usize],
            };
            let name = ty_name(p, &ck.tys, ty);
            Ok(if json {
                format!("{{\"schema\":1,\"kind\":\"type\",\"type\":{},\"file\":{},\"span\":{}}}\n", json_str(&name), json_str(map.path(span.file)), span_json(map, span))
            } else {
                format!("{name}\n")
            })
        }
        QueryKind::Def => {
            let (of, def) = definition(p, &ck, node, exprs)?;
            if def.ctxt != CtxtId::ROOT || p.std_source_span(def) || p.in_lang_methods(def) || p.shipped_crate_at(def).is_some() {
                return Err(format!("the {of} is defined in the standard library or by a macro"));
            }
            Ok(if json {
                format!("{{\"schema\":1,\"kind\":\"definition\",\"of\":{},\"file\":{},\"span\":{}}}\n", json_str(of), json_str(map.path(def.file)), span_json(map, def))
            } else {
                let (line, col) = map.line_col(def);
                format!("{}:{line}:{col}\n", map.path(def.file))
            })
        }
    }
}

/// What `node` names and the span of its definition: a local's binding, a
/// fn's or method's name, a type's definition.
fn definition(p: &Program, ck: &Checked, node: Node, exprs: (u32, u32)) -> Result<(&'static str, Span), String> {
    let fn_span = |f: LocalDefId| p.fn_def(f).name_span;
    let adt_span = |a: crate::ty::AdtId| p.adts.get(a.0 as usize).map(|adt| adt.span);
    let none = || "no definition found for what is at the position".to_string();
    match node {
        Node::Pat(i) => match ck.pat_res[(i.0 - ck.pat_base) as usize] {
            PatRes::Binding { local, .. } => Ok(("local", ck.locals.get(local as usize).ok_or_else(none)?.span)),
            PatRes::Variant(a, _) => Ok(("type", adt_span(a).ok_or_else(none)?)),
            _ => Err(none()),
        },
        Node::Expr(e) => {
            // A callee path can leave the resolution on its call.
            let res = match ck.res(e) {
                Res::None if matches!(p.ast.expr(e), Expr::Path(_)) => (exprs.0..exprs.1)
                    .map(ExprId)
                    .find(|&c| matches!(p.ast.expr(c), Expr::Call { callee, .. } if *callee == e))
                    .filter(|c| ((c.0 - ck.expr_base) as usize) < ck.expr_res.len())
                    .map_or(Res::None, |c| ck.res(c)),
                r => r,
            };
            match res {
                Res::Local(i) => Ok(("local", ck.locals.get(i as usize).ok_or_else(none)?.span)),
                Res::Fn(f) => Ok(("fn", fn_span(f))),
                Res::Method { f, .. } => Ok(("method", fn_span(f))),
                Res::Variant(a, _) => Ok(("type", adt_span(a).ok_or_else(none)?)),
                _ => Err(none()),
            }
        }
    }
}

/// The session's id for `path`: by the path as the map holds it, else by
/// the file both paths name.
fn file_id(map: &SourceMap, path: &Path) -> Option<FileId> {
    let ids = || (0..map.file_count() as u32).map(FileId);
    if let Some(f) = ids().find(|&f| Path::new(map.path(f)) == path) {
        return Some(f);
    }
    let canon = |p: &Path| std::fs::canonicalize(p).ok();
    let want = canon(path)?;
    ids().find(|&f| canon(Path::new(map.path(f))).is_some_and(|c| c == want))
}
