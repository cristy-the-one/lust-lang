// `lust fix <file.rs> [check flags] [--all] [--dry-run]`: applies the edits
// the diagnostics of a check carry (`Diag::edits`), then checks again,
// until a check has no edit left to apply.
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
        eprintln!("usage: lust fix <file.rs> [--all] [--dry-run] [crate flags]");
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
                eprintln!("lust fix: applied {applied} edit(s)");
            }
            resp.code
        };
        if resp.code == 0 || picked.is_empty() || round + 1 == MAX_ROUNDS {
            return done(&resp);
        }
        match apply(&picked, dry) {
            Ok(0) => return done(&resp),
            Ok(n) if dry => {
                eprintln!("lust fix: {n} edit(s) to apply (dry run, nothing written)");
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

/// Applies `edits` file by file, from the end of each file to its start so
/// earlier offsets stay true. An edit that overlaps one already applied
/// waits for the next round. Returns how many it applied (or would, in a
/// dry run).
fn apply(edits: &[FileEdit], dry: bool) -> Result<usize, String> {
    let mut by_file: std::collections::BTreeMap<&str, Vec<&FileEdit>> = Default::default();
    for e in edits {
        by_file.entry(&e.path).or_default().push(e);
    }
    let mut n = 0;
    for (path, mut list) in by_file {
        let mut text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        list.sort_by(|a, b| (b.edit.start, b.edit.end, &b.edit.replacement).cmp(&(a.edit.start, a.edit.end, &a.edit.replacement)));
        list.dedup_by(|a, b| (a.edit.start, a.edit.end, &a.edit.replacement) == (b.edit.start, b.edit.end, &b.edit.replacement));
        // Everything from `floor` on is already rewritten.
        let mut floor = text.len() + 1;
        let before = n;
        for e in list {
            let (start, end) = (e.edit.start as usize, e.edit.end as usize);
            // `>=`: a second insertion at the same offset has no order.
            if start > end || end >= floor && !(end == floor && start < end) || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                continue;
            }
            eprintln!("{} {path}:{}:{}: {}", if dry { "would fix" } else { "fixed" }, e.line, e.col, e.code);
            text.replace_range(start..end, &e.edit.replacement);
            floor = start;
            n += 1;
        }
        if !dry && n > before {
            std::fs::write(path, text).map_err(|e| format!("cannot write {path}: {e}"))?;
        }
    }
    Ok(n)
}
