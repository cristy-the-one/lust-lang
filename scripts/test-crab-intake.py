#!/usr/bin/env python3
"""Tests scripts/crab-intake.py on a fake Crab Cage output tree built in a temp dir.

The tree has two runs below one parent: one journal ends in a truncated line, one
candidate's work/<hash>.rs is missing, one divergence has two outputs and one has
rustc.stderr, and one journal line says diverged with no directory for it. Needs no
lust binary. Usage: python3 scripts/test-crab-intake.py   (exit 0 = pass)
"""
import json
import pathlib
import subprocess
import sys
import tempfile

SCRIPT = pathlib.Path(__file__).resolve().parent / "crab-intake.py"
failures = []
passed = 0


def check(name, cond, detail=""):
    global passed
    if cond:
        passed += 1
        print(f"ok   {name}")
    else:
        failures.append(name)
        print(f"FAIL {name}: {detail}")


def diag(code, message):
    return {"code": code, "message": message, "line": 2, "col": 5, "fix": "..."}


def verdict(n, hash_, status, gate, diags=(), score=None):
    return {"n": n, "hash": hash_, "status": status, "gate": gate, "score": score, "diagnostics": list(diags), "note": "", "proposer": "local"}


def write_run(out, rows, tail=""):
    (out / "work").mkdir(parents=True)
    (out / "journal.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows) + tail, encoding="utf-8")


def run(*args):
    return subprocess.run([sys.executable, str(SCRIPT), *map(str, args)], capture_output=True, text=True, timeout=60)


def main():
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        parent = tmp / "runs"
        a, b = parent / "a", parent / "deeper" / "b"
        mismatch = "mismatched types: expected `u32`, found `&str`"
        write_run(a, [
            verdict(1, "a000000000000001", "rejected", "check", [diag("L0110", mismatch), diag("L0110", "second L0110 of the same candidate")]),
            verdict(2, "a000000000000002", "rejected", "check", [diag("L0110", mismatch), diag("L0104", "no method `clamp` on type `f64`")]),
            verdict(3, "a000000000000003", "rejected", "check", [diag("L0110", "mismatched types: expected `i64`, found `i32`")]),
            verdict(4, "a000000000000004", "scored", "run", score=3),
            verdict(5, "a000000000000005", "diverged", "oracle", score=0),
            verdict(6, "a000000000000006", "diverged", "oracle", score=0),
        ], tail='{"n": 7, "hash": "a0000000000000')  # cut off mid-write
        for h in ["a000000000000002", "a000000000000003", "a000000000000004"]:  # ...001 has no source
            (a / "work" / f"{h}.rs").write_text("fn main() {}\n")
        d5 = a / "work" / "divergences" / "a000000000000005"
        d5.mkdir(parents=True)
        (d5 / "source.rs").write_text("fn main() { println!(\"1\\n4\"); }\n")
        (d5 / "case.in").write_text("")
        (d5 / "fast.stdout").write_text("1\n4\n")
        (d5 / "oracle.stdout").write_text("1\n5\n")
        (d5 / "README.txt").write_text("lust 0.3.0\nfast: lust build source.rs\noracle: rustc source.rs\n")
        d6 = a / "work" / "divergences" / "a000000000000006"
        d6.mkdir(parents=True)
        (d6 / "source.rs").write_text("fn main() {}\n")
        (d6 / "README.txt").write_text("lust accepted, rustc rejected\n")
        (d6 / "rustc.stderr").write_text("error[E0308]: mismatched types\n")
        write_run(b, [
            verdict(1, "b000000000000001", "rejected", "prefilter", [], None),
            verdict(2, "b000000000000002", "proven", "oracle", score=0),
            verdict(3, "b000000000000003", "diverged", "oracle", score=0),  # no directory
            verdict(4, "b000000000000004", "rejected", "check", [diag("L0104", "no method `clamp` on type `f64`")]),
        ])
        (b / "work" / "b000000000000004.rs").write_text("fn main() {}\n")

        dest = tmp / "tests" / "crab" / "diverged"
        p = run(parent, "--copy-to", dest)
        out = p.stdout
        print(out)
        check("exit 0", p.returncode == 0, p.stderr)
        check("finds both journals below the parent", "2 journal(s)" in out, out)
        check("skips the truncated last line", "1 line(s) skipped (not JSON)" in out, out)
        check("totals: candidates", "candidates: 10 (1 line(s) skipped)" in out, out)
        check("totals: per status", "status: rejected 5, diverged 3, proven 1, scored 1" in out, out)
        check("totals: per gate", "gate:   check 4, oracle 4, prefilter 1, run 1" in out, out)
        # L0110 rejected three candidates (two diagnostics of one count once), L0104 two.
        l110, l104 = out.find("  3  L0110  "), out.find("  2  L0104  ")
        check("codes ranked by candidates rejected", 0 <= l110 < l104, out)
        check("the most common message, and the others counted", f"L0110  {mismatch} (and 1 other message(s))" in out, out)
        ex110 = out[l110:].splitlines()[1]
        check("an example source that exists (not the missing one)", "example:" in ex110 and "a000000000000002.rs" in ex110, ex110)
        check("a missing work file is reported", "2 rejected candidate(s) had no work/<hash>.rs" in out, out)
        check("divergences: three, the one without a directory included", "== divergences (3) ==" in out, out)
        check("divergence README shown", "fast: lust build source.rs" in out, out)
        check("outputs side by side, the differing line marked", any(l.split() == ["4", "|", "5"] for l in out.splitlines()), out)
        check("the same line not marked", any(l.split() == ["1", "1"] for l in out.splitlines()), out)
        check("rustc.stderr shown instead of outputs", "error[E0308]: mismatched types" in out, out)
        check("a diverged verdict with no directory is named", "b000000000000003" in out and "no work/divergences directory" in out, out)
        check("--copy-to copies source.rs as <hash>.rs", (dest / "a000000000000005.rs").read_text() == (d5 / "source.rs").read_text())
        check("--copy-to copies the rustc-rejected one too", (dest / "a000000000000006.rs").is_file())

        one = run(a)
        check("one run directory works", one.returncode == 0 and "1 journal(s)" in one.stdout and "candidates: 6" in one.stdout, one.stdout + one.stderr)
        empty = tmp / "empty"
        empty.mkdir()
        none = run(empty)
        check("no journal: exit 2 with a message", none.returncode == 2 and "no journal.jsonl" in none.stderr, none.stdout + none.stderr)

    print(f"test-crab-intake: {passed} passed, {len(failures)} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
