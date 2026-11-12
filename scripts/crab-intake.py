#!/usr/bin/env python3
"""Turns Crab Cage output directories into lust work: divergences and the codes models hit.

A Crab Cage run (`crab-cage run`) writes, in its output directory:

    journal.jsonl                  one verdict per candidate: n, hash, status
                                   (rejected|scored|proven|diverged), gate, score,
                                   diagnostics [{code, message, line, col, fix}], note, proposer
    work/<hash>.rs                 the candidate spliced into its program
    work/divergences/<hash>/       source.rs, case.in, fast.stdout, oracle.stdout, README.txt,
                                   and rustc.stderr when rustc rejected a program lust accepted

For each directory given (or every directory below it that holds a journal.jsonl) this prints:

  - each divergence: its hash, README, and lust's and rustc's output side by side (or rustc's
    stderr). Each one is a rule-zero bug to fix or to turn into a reject. With
    `--copy-to tests/crab/diverged` its source.rs is copied there as <hash>.rs.
  - the diagnostic codes ranked by how many rejected candidates each one rejected, with the
    message most candidates got and one example source: what lust should support next,
    measured on what models write.
  - totals: candidates, per status, per gate.

A line that is not JSON (a run cut off mid-write leaves a truncated last line) is skipped and
counted; a missing work/ file is reported, never fatal. Python 3 standard library only.

Usage: scripts/crab-intake.py <crab-out-dir>... [--copy-to DIR]
"""
import argparse
import collections
import json
import pathlib
import shutil
import sys

WIDTH = 38  # one column of the side-by-side view
MAX_LINES = 40  # lines of each output shown


def find_journals(paths):
    """Every journal.jsonl in or below `paths`, each once, in a stable order."""
    seen, out = set(), []
    for p in paths:
        p = pathlib.Path(p)
        if p.is_file() and p.name == "journal.jsonl":
            found = [p]
        elif (p / "journal.jsonl").is_file():
            found = [p / "journal.jsonl"]
        else:
            found = sorted(p.rglob("journal.jsonl")) if p.is_dir() else []
        for j in found:
            key = j.resolve()
            if key not in seen:
                seen.add(key)
                out.append(j)
    return out


def read_journal(path):
    """The verdicts in `path`, and how many lines were not a JSON object."""
    rows, skipped = [], 0
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                skipped += 1
                continue
            if isinstance(row, dict):
                rows.append(row)
            else:
                skipped += 1
    return rows, skipped


def read_text(path):
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None


def clip(s):
    s = s.replace("\t", "    ")
    return s if len(s) <= WIDTH else s[: WIDTH - 1] + "~"


def side_by_side(left, right, left_name, right_name):
    """Two outputs in two columns; `|` marks a line that differs."""
    a, b = left.splitlines(), right.splitlines()
    out = [f"    {clip(left_name):<{WIDTH}}   {clip(right_name)}"]
    n = max(len(a), len(b))
    for i in range(min(n, MAX_LINES)):
        x = a[i] if i < len(a) else ""
        y = b[i] if i < len(b) else ""
        mark = "   " if i < len(a) and i < len(b) and x == y else " | "
        out.append(f"    {clip(x):<{WIDTH}}{mark}{clip(y)}".rstrip())
    if n > MAX_LINES:
        out.append(f"    ... {n - MAX_LINES} more lines")
    if left == right:
        out.append("    (the two outputs are the same bytes; the difference is the exit kind, see README)")
    return out


def indent(text, by="    "):
    return [by + line for line in text.rstrip("\n").splitlines()] or [by + "(empty)"]


def show_divergence(hash_, ddir, run_label, copy_to):
    lines = [f"-- {hash_}  ({run_label})"]
    if ddir is None or not ddir.is_dir():
        lines.append("    the journal says diverged, but there is no work/divergences directory for it")
        return lines
    readme = read_text(ddir / "README.txt")
    lines += indent(readme) if readme is not None else ["    (no README.txt)"]
    stderr = read_text(ddir / "rustc.stderr")
    if stderr is not None:
        lines.append("    rustc rejected the program lust accepted; rustc.stderr:")
        lines += indent(stderr, "      ")
    else:
        fast, oracle = read_text(ddir / "fast.stdout"), read_text(ddir / "oracle.stdout")
        if fast is None or oracle is None:
            lines.append("    (fast.stdout or oracle.stdout is missing)")
        else:
            lines += side_by_side(fast, oracle, "fast.stdout (lust)", "oracle.stdout (rustc)")
    if copy_to is not None:
        src = ddir / "source.rs"
        if src.is_file():
            copy_to.mkdir(parents=True, exist_ok=True)
            dest = copy_to / f"{hash_}.rs"
            shutil.copyfile(src, dest)
            lines.append(f"    copied to {dest}")
        else:
            lines.append("    not copied: no source.rs")
    return lines


def main():
    ap = argparse.ArgumentParser(description="Turn Crab Cage journals into lust cases.")
    ap.add_argument("dirs", nargs="+", help="Crab Cage output directories, or directories above them")
    ap.add_argument("--copy-to", type=pathlib.Path, help="copy each divergence's source.rs here as <hash>.rs")
    args = ap.parse_args()

    journals = find_journals(args.dirs)
    if not journals:
        print(f"crab-intake: no journal.jsonl in or below {' '.join(args.dirs)}", file=sys.stderr)
        return 2

    total, skipped = 0, 0
    status, gate = collections.Counter(), collections.Counter()
    # code -> number of rejected candidates it rejected; its messages; one example source
    code_count, code_msgs, code_example = collections.Counter(), collections.defaultdict(collections.Counter), {}
    divergences = []  # (hash, dir or None, run label)
    missing_work = 0
    print(f"crab-intake: {len(journals)} journal(s)")
    for j in journals:
        out_dir = j.parent
        work = out_dir / "work"
        rows, bad = read_journal(j)
        print(f"  {j}: {len(rows)} candidate(s)" + (f", {bad} line(s) skipped (not JSON)" if bad else ""))
        total += len(rows)
        skipped += bad
        diverged_hashes = set()
        for r in rows:
            st, g, h = str(r.get("status", "?")), str(r.get("gate", "?")), str(r.get("hash", ""))
            status[st] += 1
            gate[g] += 1
            if st == "diverged" and h:
                diverged_hashes.add(h)
            if st != "rejected":
                continue
            diags = r.get("diagnostics") or []
            codes = {}
            for d in diags if isinstance(diags, list) else []:
                if isinstance(d, dict) and d.get("code"):
                    codes.setdefault(str(d["code"]), str(d.get("message", "")))
            src = work / f"{h}.rs" if h else None
            has_src = src is not None and src.is_file()
            missing_work += not has_src
            for code, msg in codes.items():
                code_count[code] += 1
                code_msgs[code][msg] += 1
                if has_src:
                    code_example.setdefault(code, src)
        ddirs = {}
        droot = work / "divergences"
        if droot.is_dir():
            ddirs = {d.name: d for d in sorted(droot.iterdir()) if d.is_dir()}
        for h in sorted(set(ddirs) | diverged_hashes):
            divergences.append((h, ddirs.get(h), str(out_dir)))

    print()
    print(f"== divergences ({len(divergences)}) ==")
    if not divergences:
        print("  none")
    for h, d, label in divergences:
        print("\n".join(show_divergence(h, d, label, args.copy_to)))

    print()
    print(f"== codes, by rejected candidates ({status['rejected']} rejected) ==")
    if not code_count:
        print("  none")
    for code, n in sorted(code_count.items(), key=lambda kv: (-kv[1], kv[0])):
        msg, times = code_msgs[code].most_common(1)[0]
        variants = len(code_msgs[code])
        more = f" (and {variants - 1} other message(s))" if variants > 1 else ""
        print(f"  {n:>5}  {code}  {msg}{more}")
        ex = code_example.get(code)
        print(f"         example: {ex}" if ex else "         example: (no work/<hash>.rs for any of them)")

    print()
    print("== totals ==")
    print(f"  candidates: {total}" + (f" ({skipped} line(s) skipped)" if skipped else ""))
    print("  status: " + (", ".join(f"{k} {v}" for k, v in sorted(status.items(), key=lambda kv: (-kv[1], kv[0]))) or "none"))
    print("  gate:   " + (", ".join(f"{k} {v}" for k, v in sorted(gate.items(), key=lambda kv: (-kv[1], kv[0]))) or "none"))
    if missing_work:
        print(f"  {missing_work} rejected candidate(s) had no work/<hash>.rs (a prefilter reject never has one)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
