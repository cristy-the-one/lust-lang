// `--sandbox` (ProfileFlags::sandbox): a program that may read stdin and
// write stdout and stderr, and use no other OS service. Built for running
// code an LLM or a search loop wrote (docs/sandbox.md).
//
// Three layers, each enough for what it covers:
//
// 1. Check (this file). User source that names a std path to the file
//    system, the network, the environment, processes or OS extensions
//    (`std::fs`, `std::net`, `std::env`, `std::process` other than
//    `exit`/`abort`/`ExitCode`, `std::os`) is refused with L0532 where a
//    body uses it. The resolver has already turned every std path into
//    one flat name, however a `use` spelled it, so an import alias or a
//    glob changes nothing here. So is a call that spawns a thread
//    (`thread::spawn`, `thread::scope`, `Builder::spawn`, a scope's
//    `spawn`), found by its resolved builtin, so no import spelling
//    hides it. These are the errors an author sees and can fix.
// 2. Expand (expand/builtin.rs). `include!`, `include_str!` and
//    `include_bytes!` read no file (L0532), and `env!`/`option_env!` see
//    an empty client environment (protocol.rs), so a build copies no
//    host file or variable into the program.
// 3. Codegen (cgen.rs `intrinsic`). Every OS intrinsic of those families
//    that any reachable std or adapter body calls becomes a stop
//    (`lust_sandbox_deny`) ahead of the call. A path check cannot see
//    `Path::exists` reaching `fs::metadata`; codegen sees every call, so
//    the program stops there instead of touching the file system.
use crate::ast::{self, Expr, Pat, PathSeg};
use crate::check::collect::Program;
use crate::check::sync::SyncOp;
use crate::check::{Builtin, Checked, Res};
use crate::def::LocalDefId;
use crate::diag::{Diag, Span};

const CODE: &str = "L0532";

/// What a std path reaches, when the sandbox refuses it. `segs` is the
/// path as written, from its first segment.
fn refused(segs: &[&str]) -> Option<&'static str> {
    if segs.first() != Some(&"std") {
        return None;
    }
    match segs.get(1).copied()? {
        "fs" => Some("the file system"),
        "net" => Some("the network"),
        "env" => Some("the environment"),
        "os" => Some("OS extensions"),
        // `std::process::exit` and friends end the program; the rest
        // starts or controls processes. A `use std::process;` would let
        // a body reach `process::Command` by a path this check does not
        // see as std, so the module itself is refused too.
        "process" => match segs.get(2).copied() {
            Some("exit" | "abort" | "ExitCode" | "Termination" | "id") => None,
            _ => Some("processes"),
        },
        _ => None,
    }
}

fn path_text(segs: &[&str]) -> String {
    segs.join("::")
}

fn refusal(span: Span, what: &str, path: &str) -> Diag {
    Diag::new(
        CODE,
        span,
        format!("`{path}` reaches {what}, and this build is sandboxed"),
        "A `--sandbox` program reads stdin and writes stdout and stderr only. Pass data in on stdin and answer on stdout.",
    )
}

/// The std path a resolved path names, from `std`: the resolver turns a
/// std path into one flat segment, `k<std hash>$fs$read` for a module of
/// std source or `__lust_std_net$TcpStream` for a spliced std module
/// (src/stdmods.rs), whatever `use` spelled it. Other paths keep their
/// written segments.
fn std_path(p: &Program, segs: &[PathSeg]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (k, s) in segs.iter().enumerate() {
        let text = p.interner.get(s.name);
        if k == 0
            && let Some((first, rest)) = text.split_once('$')
        {
            let std_prefix = format!("k{:016x}", crate::ty::lang::ShippedCrate::Std.stable_hash().0);
            if first == std_prefix {
                out.push("std".into());
            } else if let Some(shown) = crate::stdmods::display_path(first) {
                out.extend(shown.split("::").map(str::to_string));
            } else {
                out.push(text.to_string());
                continue;
            }
            out.extend(rest.split('$').map(str::to_string));
            continue;
        }
        out.push(text.to_string());
    }
    out
}

/// True for a span in user source: not spliced std text, not a shipped
/// std or adapter crate.
fn user_span(p: &Program, span: Span) -> bool {
    !p.std_source_span(span) && p.shipped_crate_at(span).is_none()
}

/// The spawn forms of `std::thread`, as check resolves them.
fn spawns(b: Builtin) -> bool {
    matches!(
        b,
        Builtin::Sync(SyncOp::ThreadSpawn | SyncOp::ScopeSpawn | SyncOp::ThreadScope | SyncOp::BuilderSpawn | SyncOp::BuilderSpawnScoped)
    )
}

/// The first sandbox refusal in one checked user body, if any: a std path
/// to a refused module, or a thread spawn.
pub fn check_body(p: &Program, fid: LocalDefId, ck: &Checked) -> Option<Diag> {
    let def = p.fn_def(fid);
    if !user_span(p, def.name_span) {
        return None;
    }
    for i in def.exprs.0..def.exprs.1 {
        let span = p.ast.expr_spans[i as usize];
        let segs = match &p.ast.exprs[i as usize] {
            Expr::Path(segs) | Expr::QPath { segs, .. } => Some(segs),
            _ => None,
        };
        if let Some(segs) = segs {
            let n = std_path(p, segs);
            let n: Vec<&str> = n.iter().map(String::as_str).collect();
            if let Some(what) = refused(&n) {
                return Some(refusal(span, what, &path_text(&n)));
            }
        }
        let e = ast::ExprId(i);
        match ck.res(e) {
            Res::Builtin(b) | Res::BuiltinMethod { b, .. } if spawns(b) => {
                return Some(Diag::new(
                    CODE,
                    span,
                    "this spawns a thread, and this build is sandboxed",
                    "A `--sandbox` program runs on one thread. Do the work in a loop on the main thread.",
                ));
            }
            _ => {}
        }
    }
    for i in def.pats.0..def.pats.1 {
        let segs = match &p.ast.pats[i as usize] {
            Pat::Path(segs) => segs,
            _ => continue,
        };
        let n = std_path(p, segs);
        let n: Vec<&str> = n.iter().map(String::as_str).collect();
        if let Some(what) = refused(&n) {
            return Some(refusal(p.ast.pat_spans[i as usize], what, &path_text(&n)));
        }
    }
    None
}

/// The family an OS intrinsic belongs to, as `lust_sandbox_deny` names it
/// (runtime/core.c), when a `--sandbox` program may not call it: 0 the
/// file system, 1 the network, 2 the environment, 3 processes, 4 OS
/// extensions and signals. None for the intrinsics a sandboxed program
/// keeps: memory, formatting, the standard streams, the clock, exit.
pub fn intrinsic_family(name: &str) -> Option<i32> {
    if name.starts_with("sys_net_") {
        return Some(1);
    }
    if name.starts_with("sys_x_") {
        return Some(4);
    }
    match name {
        "sys_getcwd" | "sys_chdir" | "sys_readlink" | "sys_read_file" | "sys_write_file" | "sys_path_op" | "sys_rename" | "sys_stat" | "sys_realpath"
        | "sys_read_dir" | "sys_remove_dir_all" | "sys_current_exe" | "sys_fd_open" | "sys_fd_open_ext" | "sys_symlink" => Some(0),
        "sys_getenv" | "sys_environ" => Some(2),
        "sys_proc_spawn" | "sys_proc_exec" | "sys_proc_wait" | "sys_proc_kill" | "sys_fd_read_both" => Some(3),
        "sys_sig_register" | "sys_sig_register_pipe" | "sys_sig_unregister_pipe" | "sys_sig_pipe" | "sys_sig_poll_read" | "sys_sig_read_now" | "sys_sig_raise"
        | "sys_sig_restore_raise" => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{intrinsic_family, refused};

    #[test]
    fn families_cover_every_os_intrinsic() {
        // Every `sys_` intrinsic is either kept on purpose or in a family:
        // a new one must be placed here before it ships.
        let kept = [
            "sys_write_std", "sys_flush_std", "sys_fmt_arguments_bytes", "sys_str_as_bytes", "sys_bytes_as_str", "sys_string_from_utf8_unchecked",
            "sys_utf8_valid_up_to", "sys_utf8_lossy", "sys_argv", "sys_getpid", "sys_os_error_text", "sys_fd_read", "sys_fd_write", "sys_fd_close",
            "sys_fd_dup", "sys_fd_sync", "sys_fd_sync_data", "sys_fd_truncate", "sys_fd_seek", "sys_fd_pread", "sys_fd_pwrite", "sys_fd_isatty",
            "sys_fd_stat", "sys_stdin_read", "sys_stdin_fill", "sys_stdin_consume", "sys_sig_exit", "sys_sig_abort",
        ];
        for d in crate::ty::lang::INTRINSICS.iter().filter(|d| d.name.starts_with("sys_")) {
            assert!(
                kept.contains(&d.name) != intrinsic_family(d.name).is_some(),
                "intrinsic `{}` is neither kept nor in a sandbox family (or both)",
                d.name
            );
        }
    }

    #[test]
    fn refused_paths() {
        assert_eq!(refused(&["std", "fs", "read"]), Some("the file system"));
        assert_eq!(refused(&["std", "net"]), Some("the network"));
        assert_eq!(refused(&["std", "env", "var"]), Some("the environment"));
        assert_eq!(refused(&["std", "os", "unix", "fs"]), Some("OS extensions"));
        assert_eq!(refused(&["std", "process", "Command"]), Some("processes"));
        assert_eq!(refused(&["std", "process"]), Some("processes"));
        assert_eq!(refused(&["std", "process", "exit"]), None);
        assert_eq!(refused(&["std", "io", "stdin"]), None);
        assert_eq!(refused(&["std", "thread", "sleep"]), None);
        assert_eq!(refused(&["fs", "read"]), None, "a user module named fs is fine");
    }
}
