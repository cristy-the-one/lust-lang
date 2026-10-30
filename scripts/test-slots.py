#!/usr/bin/env python3
# Slot identity tests (docs/ARCH.md sections 1.7 and 4.2, serial refactor S2):
# daemon function slots are keyed by the stable disk pair
# (StableCrateHash, DefPathHash), so:
#   1. a restarted daemon gives the same source the same slots
#      (serialize the slot map through --emit-c, restart, compare), and
#   2. an edit that adds a function keeps every old function's slot
#      (append-only assignment) and gives the new one a fresh slot.
# Slot numbers are read from the emitted C: each test function prints a unique
# marker string, and the marker's enclosing `f<slot>` definition names its slot.
# Usage: python3 scripts/test-slots.py   (exit 0 = pass)
import glob, os, re, subprocess, sys, tempfile, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LUST = os.path.join(ROOT, "target/release/lust")
failures = []

SRC = """\
struct Cell { v: i64 }
fn alpha() -> i64 { println!("marker-alpha"); 1 }
impl Cell { fn get(&self) -> i64 { println!("marker-get"); self.v } }
fn beta() -> i64 { println!("marker-beta"); 2 }
fn main() { let c = Cell { v: 4 }; println!("{}", alpha() + beta() + c.get()); }
impl Cell { fn set(&mut self, v: i64) { println!("marker-set"); self.v = v; } }
"""
EDIT = 'fn gamma() -> i64 { println!("marker-gamma"); 8 }\n' + SRC.replace(
    "alpha() + beta()", "alpha() + beta() + gamma()"
)
MARKERS = ["marker-alpha", "marker-get", "marker-beta", "marker-set"]


def check(name, cond, detail=""):
    print(f"{'ok' if cond else 'FAIL'} {name}" + (f": {detail}" if detail and not cond else ""))
    if not cond:
        failures.append(name)


def slots_of(prefix):
    """marker string -> slot, from the emitted C function units at `prefix`."""
    out = {}
    for path in sorted(glob.glob(prefix + ".u*.c")):
        text = open(path).read()
        defs = [(m.start(), int(m.group(1))) for m in re.finditer(r"(?m)^static .* f(\d+)\(.*\) \{", text)]
        for marker in MARKERS + ["marker-gamma"]:
            at = text.find(f'"{marker}"')
            if at < 0:
                continue
            earlier = [s for pos, s in defs if pos < at]
            if earlier:
                out[marker] = earlier[-1]
    return out


def main():
    work = tempfile.mkdtemp(prefix="lust-slots-")
    sock = os.path.join(work, "d.sock")
    env = dict(os.environ, LUST_DAEMON_SOCKET=sock, LUST_NO_CACHE="1")
    src = os.path.join(work, "prog.rs")

    def start_daemon():
        # A killed daemon leaves its socket file behind; remove it so the wait
        # below only succeeds for the new daemon's socket.
        if os.path.exists(sock):
            os.remove(sock)
        log = open(os.path.join(work, "daemon.log"), "a")
        d = subprocess.Popen([LUST, "daemon"], cwd=work, env=env, stderr=log)
        t0 = time.time()
        while not os.path.exists(sock):
            if time.time() - t0 > 10 or d.poll() is not None:
                print("daemon start failed")
                sys.exit(2)
            time.sleep(0.005)
        return d

    def build(tag):
        out = os.path.join(work, tag)
        r = subprocess.run(
            [LUST, "build", src, "-o", out + ".bin", "--emit-c", out],
            cwd=work, env=env, capture_output=True, text=True, timeout=30,
        )
        if r.returncode != 0:
            check(f"build {tag}", False, r.stderr.strip())
            return {}, ""
        run = subprocess.run([out + ".bin"], capture_output=True, text=True, timeout=10)
        return slots_of(out), run.stdout

    open(src, "w").write(SRC)
    daemon = start_daemon()
    try:
        first, stdout = build("a")
        check("first build runs", stdout.strip().endswith("7"), repr(stdout))
        check("first build slots found", sorted(first) == sorted(MARKERS), repr(first))
        check("first build slots distinct", len(set(first.values())) == len(MARKERS), repr(first))

        # Restart the daemon and rebuild the same source: same slots.
        daemon.terminate()
        daemon.wait(timeout=10)
        daemon = start_daemon()
        again, stdout = build("b")
        check("restart build runs", stdout.strip().endswith("7"), repr(stdout))
        check("restart daemon: same slots", again == first, f"first={first} again={again}")

        # Add a function at the top: old slots stay, the new fn gets a fresh one.
        open(src, "w").write(EDIT)
        edited, stdout = build("c")
        check("edited build runs", stdout.strip().endswith("15"), repr(stdout))
        kept = {m: edited.get(m) for m in MARKERS}
        check("edit keeps old slots", kept == first, f"first={first} edited={kept}")
        check(
            "edit gives the new fn a fresh slot",
            edited.get("marker-gamma") is not None and edited["marker-gamma"] not in first.values(),
            repr(edited),
        )
    finally:
        daemon.terminate()

    print(f"test-slots: {'FAILED ' + repr(failures) if failures else 'all passed'}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
