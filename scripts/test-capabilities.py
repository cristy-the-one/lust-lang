#!/usr/bin/env python3
"""Keeps `lust capabilities` true: runs the probes in tests/capabilities/.

Each probe names the capability lines it shows, then what lust must do:

    // capability: <section>: <line text, exactly as `lust capabilities --json` prints it>
    // expect: ok | L####
    // command: test        (optional; default `run` for ok, `check` for a code)

`ok` means the command exits 0 (the supported probes assert their own results and
print them, so a sweep can compare them with rustc). `L####` means lust reports that
code; for a rejected line it must be the line's own code. The test fails when:
  - a probe names a line the list does not have (the line changed or went away),
  - a supported probe does not run or a rejected probe is not rejected with its code,
  - a line in a checked section has no probe and is not in UNPROBED.

It also checks the `"interface"` object of `lust capabilities --json` (the version,
the build hash, the commands and each command's flags) against `lust --version` and
the usage text, and `lust capabilities --sandbox --summary`.

Usage: scripts/test-capabilities.py [lust binary]   (default target/release/lust)
"""
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

ROOT = pathlib.Path(__file__).resolve().parent.parent
PROBES = ROOT / "tests" / "capabilities"
SECTIONS = ["items", "types", "attributes", "derives", "control_flow", "patterns", "expressions", "rejected"]

# Rejected lines with no single-file probe, and why. Keep this list short.
UNPROBED = {
    "moving a field out of a struct or enum (a fallback that no known program reaches)":
        "borrowck fallback; scripts/agentkit-test.sh needs the unsupported(..) text listed",
    "the `BTreeMap` or `Arc` type where the resolver has no rule for it (a fallback that no known program reaches)":
        "resolver fallback; scripts/agentkit-test.sh needs the names listed",
    "#[test] fns in a module that a macro writes, in `lust test` (a fallback that no known program reaches)":
        "testrun guard; a macro-written test module runs (the brace and name checks pass)",
    "dependency crates with no lust adapter or shim yet (named in the message)":
        "needs a Cargo project: tests/crates/adapter-missing covers it",
}


def lines_of(cap):
    out = {}
    for s in SECTIONS:
        out[s] = {r["feature"]: r["code"] for r in cap[s]} if s == "rejected" else {l: None for l in cap[s]}
    return out


def parse(path):
    caps, expect, command = [], None, None
    for line in path.read_text().splitlines():
        if not line.startswith("//"):
            break
        if m := re.match(r"// capability: (\w+): (.*)$", line):
            caps.append((m.group(1), m.group(2)))
        elif m := re.match(r"// expect: (ok|L\d{4})$", line):
            expect = m.group(1)
        elif m := re.match(r"// command: (\w+)$", line):
            command = m.group(1)
    return caps, expect, command


def run_probe(lust, work, path, expect, command):
    """Runs one probe from `work`, a copy of tests/capabilities: `lust run` can leave a
    binary next to the source, and that must not land in the tree."""
    command = command or ("run" if expect == "ok" else "check")
    with tempfile.TemporaryDirectory() as tmp:
        try:
            p = subprocess.run([lust, command, path.name], cwd=work, env=dict(os.environ, LUST_NO_DAEMON="1", TMPDIR=tmp),
                               capture_output=True, text=True, timeout=120)
        except subprocess.TimeoutExpired:
            return "timed out"
    err = p.stderr + p.stdout
    if expect == "ok":
        return None if p.returncode == 0 else f"`lust {command}` exited {p.returncode}: {first_error(err)}"
    if p.returncode == 0:
        return f"`lust {command}` accepted it (expected {expect})"
    if f"[{expect}]" not in err:
        return f"expected {expect}, got: {first_error(err)}"
    return None


def first_error(text):
    for line in text.splitlines():
        if "error" in line:
            return line.strip()
    return text.strip().splitlines()[0] if text.strip() else "(no output)"


def check_interface(lust, cap):
    """The `interface` object and `--sandbox --summary`. Returns failure lines."""
    fails = []
    run = lambda *args: subprocess.run([lust, *args], capture_output=True, text=True, timeout=60)
    iface = cap.get("interface")
    if not isinstance(iface, dict):
        return ["interface: `lust capabilities --json` has no `interface` object"]
    if set(iface) != {"version", "build", "commands", "flags"}:
        fails.append(f"interface: keys are {sorted(iface)}")
    version = run("--version").stdout.strip()
    if version != f"lust {iface.get('version')} (build {iface.get('build')})":
        fails.append(f"interface: version {iface.get('version')!r} and build {iface.get('build')!r} do not match `lust --version`: {version!r}")
    commands, flags = iface.get("commands", []), iface.get("flags", {})
    if sorted(flags) != sorted(commands):
        fails.append(f"interface: flags has commands {sorted(flags)}, commands is {sorted(commands)}")
    # The existing top-level `commands` list keeps its meaning; every name in it is a command.
    if not set(cap["commands"]) <= set(commands):
        fails.append(f"interface: the top-level commands {cap['commands']} are not all in {commands}")
    want = {
        "check": {"--json", "--sandbox", "--manifest-path", "--features", "--rustc"},
        "build": {"--json", "--sandbox", "--max-memory", "--time-limit", "--opt", "-o", "--release"},
        "run": {"--sandbox", "--max-memory", "--time-limit", "--"},
        "test": {"--json", "--timeout", "--manifest-path"},
        "fix": {"--all", "--dry-run", "--sandbox", "--manifest-path"},
        "capabilities": {"--json", "--summary", "--sandbox"},
    }
    for cmd, names in want.items():
        got = flags.get(cmd, [])
        if len(got) != len(set(got)):
            fails.append(f"interface: `{cmd}` lists a flag twice: {got}")
        if not names <= set(got):
            fails.append(f"interface: `{cmd}` misses {sorted(names - set(got))}")
    for cmd, never in {"check": {"--max-memory", "--time-limit", "--opt", "-o"}, "explain": {"--json"}}.items():
        if never & set(flags.get(cmd, [])):
            fails.append(f"interface: `{cmd}` lists {sorted(never & set(flags.get(cmd, [])))}, which it does not take")
    # Every flag is in the usage text, which `lust` with no arguments prints.
    usage = run()
    text = usage.stderr + usage.stdout
    words = set(re.findall(r"(?<![\w-])(--?[A-Za-z][\w=-]*|--)(?![\w-])", text))
    for cmd, names in flags.items():
        missing = [n for n in names if n not in words]
        if missing:
            fails.append(f"interface: `{cmd}` flags not in the usage text: {missing}")
    # `--sandbox --summary`: the sandbox refusals are in the main list.
    plain, sandboxed = run("capabilities", "--summary"), run("capabilities", "--sandbox", "--summary")
    if sandboxed.returncode != 0 or run("capabilities", "--summary", "--sandbox").stdout != sandboxed.stdout:
        fails.append(f"--sandbox --summary: exit {sandboxed.returncode}, or the flag order changes the text: {sandboxed.stderr.strip()}")
    refused = sandboxed.stdout.split("Refused:\n", 1)[-1]
    for line in ["- `std::fs`", "- `std::net`", "- `std::env`", "- `std::process` other than `exit`", "- spawning a thread", "`include_str!`", "(L0533)"]:
        if line not in refused:
            fails.append(f"--sandbox --summary: no `{line}` in the Refused list")
    if "With --sandbox" in sandboxed.stdout or "With --sandbox" not in plain.stdout:
        fails.append("--sandbox --summary: the trailing `With --sandbox` line belongs to the plain summary only")
    if run("capabilities", "--sandbox").returncode != 2:
        fails.append("capabilities: `--sandbox` alone must be a usage error (exit 2)")
    return fails


def main():
    lust = sys.argv[1] if len(sys.argv) > 1 else str(ROOT / "target" / "release" / "lust")
    cap = json.loads(subprocess.run([lust, "capabilities", "--json"], capture_output=True, text=True, check=True).stdout)
    lines = lines_of(cap)
    fails = check_interface(lust, cap)
    covered = set()
    jobs = []
    for path in sorted(PROBES.glob("*.rs")):
        caps, expect, command = parse(path)
        if not caps or expect is None:
            fails.append(f"{path.name}: needs `// capability:` and `// expect:` header lines")
            continue
        for section, text in caps:
            if section not in lines:
                fails.append(f"{path.name}: unknown section `{section}`")
            elif text not in lines[section]:
                fails.append(f"{path.name}: `lust capabilities` has no {section} line `{text}`")
            elif section == "rejected" and lines[section][text] != expect:
                fails.append(f"{path.name}: the rejected line has code {lines[section][text]}, the probe expects {expect}")
            elif (section == "rejected") != (expect != "ok"):
                fails.append(f"{path.name}: a {section} line needs expect {'a code' if section == 'rejected' else 'ok'}")
            else:
                covered.add((section, text))
        jobs.append((path, expect, command))
    for text in UNPROBED:
        if text not in lines["rejected"]:
            fails.append(f"UNPROBED names a line the rejected list does not have: `{text}`")
        covered.add(("rejected", text))
    for section in SECTIONS:
        for text in lines[section]:
            if (section, text) not in covered:
                fails.append(f"{section} line has no probe in tests/capabilities/: `{text}`")
    with tempfile.TemporaryDirectory() as tmp, ThreadPoolExecutor(max_workers=min(16, os.cpu_count() or 4)) as pool:
        work = pathlib.Path(tmp) / "capabilities"
        shutil.copytree(PROBES, work)
        results = pool.map(lambda j: (j[0], run_probe(lust, work, *j)), jobs)
        fails += [f"{path.name}: {why}" for path, why in results if why]
    for f in fails:
        print(f"FAIL {f}")
    print(f"test-capabilities: {len(jobs)} probes, {len(covered)} lines, {len(fails)} failed")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
