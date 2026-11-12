// `lust fix <file.rs> [--all] [--dry-run] [crate flags] [--sandbox]`: applies
// the edits the diagnostics of a check carry (`Diag::edits`), then checks
// again, until a check has no edit left to apply.
//
// Every flag other than `--all` and `--dry-run` goes to that check, as
// `lust check` takes it. With `--sandbox` each round is a sandboxed check, so
// a rewrite that is valid Rust but reaches a refused service (L0532) still
// fails the command, as `lust check --sandbox` would.
//
// It applies the `auto` edits: the ones a tool may apply without asking
// (a lossless numeric cast, a `.0` on an integer literal). `--all` also
// applies the rest, which change something the program can observe; the
// rewrites for rejects are of that kind (src/rewrite.rs). `--dry-run`
// prints what one round would change and writes nothing.
//
// Every round is a cold check in this process: an edit moves the text
// under the next diagnostic, and errors come in layers (a collect error
// hides the body errors behind it), so one check never lists every edit.
// The command ends like the last check: its diagnostics on stderr, exit
// code 0 when the crate checks.

use crate::diag::FileEdit;
use crate::driver;
use crate::protocol::{self, Invocation};

/// The most check rounds one run makes. Each round with an edit removes
/// at least one diagnostic's cause, so this only stops a rewrite that
/// keeps offering itself.
const MAX_ROUNDS: usize = 32;

pub fn run(args: &[String]) -> i32 {
    let (mut all, mut dry) = (false, false);
    let mut check: Vec<String> = vec!["check".into()];
    for a in args {
        match a.as_str() {
            "--all" => all = true,
            "--dry-run" => dry = true,
            _ => check.push(a.clone()),
        }
    }
    let Some(Invocation::Compile { mut req, .. }) = protocol::parse_args(&check) else {
        eprintln!("usage: lust fix <file.rs> [--all] [--dry-run] [crate flags] [--sandbox]");
        return 2;
    };
    // Every diagnostic, as text: the last round's answer is this command's.
    (req.json, req.timings, req.max_errors) = (false, false, 0);
    req.client.env = None;
    let mut applied = 0usize;
    for round in 0.. {
        let dir = std::env::temp_dir().join(format!("lust-fix-{}-{round}", std::process::id()));
        let mut session = driver::Session::new(dir, false, req.session.clone());
        let (resp, edits) = crate::diag::with_edit_sink(|| driver::compile(&req, &mut session));
        let _ = std::fs::remove_dir_all(session.obj_dir());
        drop(session);
        let picked: Vec<FileEdit> = edits.into_iter().filter(|e| all || e.edit.auto).collect();
        let done = |resp: &protocol::Response| {
            eprint!("{}", resp.stderr);
            if applied > 0 {
                eprintln!("lust fix: fixed {applied} diagnostic(s)");
            }
            resp.code
        };
        if resp.code == 0 || picked.is_empty() || round + 1 == MAX_ROUNDS {
            return done(&resp);
        }
        match apply(&picked, dry) {
            Ok(0) => return done(&resp),
            Ok(n) if dry => {
                eprintln!("lust fix: {n} diagnostic(s) to fix (dry run, nothing written)");
                return 1;
            }
            Ok(n) => applied += n,
            Err(e) => {
                eprintln!("error: {e}");
                return 2;
            }
        }
    }
    unreachable!()
}

/// Applies `edits`, one diagnostic's edits together or not at all: a
/// diagnostic whose edits touch text that an earlier one's already change
/// waits for the next round (the same edit offered twice counts once).
/// Each file is rewritten from its end to its start, so earlier offsets
/// stay true. Returns how many diagnostics it fixed (or would, in a dry
/// run).
fn apply(edits: &[FileEdit], dry: bool) -> Result<usize, String> {
    // Ranges conflict when they share text; so do two insertions at one
    // offset, whose order nothing says.
    let clash = |a: &FileEdit, b: &FileEdit| {
        let (a, b) = (&a.edit, &b.edit);
        if (a.start, a.end, &a.replacement) == (b.start, b.end, &b.replacement) {
            return false;
        }
        (a.start < b.end && b.start < a.end) || (a.start == a.end && b.start == b.end && a.start == b.start)
    };
    let mut groups: Vec<&[FileEdit]> = edits.chunk_by(|a, b| a.group == b.group).collect();
    let mut taken: Vec<&FileEdit> = Vec::new();
    let mut fixed = 0;
    groups.retain(|g| {
        let free = g.iter().all(|e| e.edit.start <= e.edit.end && !taken.iter().any(|t| t.path == e.path && clash(t, e)));
        if free {
            taken.extend(g.iter());
        }
        free
    });
    for g in &groups {
        let e = &g[0];
        eprintln!("{} {}:{}:{}: {}", if dry { "would fix" } else { "fixed" }, e.path, e.line, e.col, e.code);
        fixed += 1;
    }
    if dry {
        return Ok(fixed);
    }
    let mut by_file: std::collections::BTreeMap<&str, Vec<&FileEdit>> = Default::default();
    for e in taken {
        by_file.entry(&e.path).or_default().push(e);
    }
    for (path, mut list) in by_file {
        let mut text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let key = |e: &FileEdit| (e.edit.start, e.edit.end, e.edit.replacement.clone());
        list.sort_by_key(|e| std::cmp::Reverse(key(e)));
        list.dedup_by_key(|e| key(e));
        for e in list {
            let (start, end) = (e.edit.start as usize, e.edit.end as usize);
            if end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                return Err(format!("{path}: an edit of {} does not fit the file (did it change during the check?)", e.code));
            }
            text.replace_range(start..end, &e.edit.replacement);
        }
        std::fs::write(path, text).map_err(|e| format!("cannot write {path}: {e}"))?;
    }
    Ok(fixed)
}
