#!/usr/bin/env python3
# Smoke tests for daemon wire and base-library behavior that the random-edit fuzzer
# does not reach: odd file names, odd run arguments, and old binaries staying valid
# after the daemon rebuilds its base library.
# Usage: python3 scripts/fuzz/smoke.py   (exit 0 = pass)
# Lust calls run under the one timeout rule (run_timed): a timeout retries
# once alone at 6x the limit and is reported as "slow" when it finishes; a
# second timeout raises, which fails the suite as a hang.
import glob, os, re, shutil, subprocess, sys, tempfile, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LUST = os.path.join(ROOT, "target/release/lust")
failures = []


def check(name, cond, detail=""):
    print(f"{'ok' if cond else 'FAIL'} {name}" + (f": {detail}" if detail and not cond else ""))
    if not cond:
        failures.append(name)


# run_timed <args> <timeout>: subprocess.run under the timeout rule. This
# suite is serial, so the retry is alone. A second timeout raises
# AssertionError (a hang), which fails the suite.
def run_timed(args, timeout, **kw):
    try:
        return subprocess.run(args, timeout=timeout, **kw)
    except subprocess.TimeoutExpired:
        print(f"slow: {args[1:3]} timed out at {timeout:g}s, retrying alone at {timeout * 6:g}s", flush=True)
    try:
        r = subprocess.run(args, timeout=timeout * 6, **kw)
    except subprocess.TimeoutExpired:
        raise AssertionError(f"HANG: {args} timed out alone at {timeout * 6:g}s (first at {timeout:g}s)")
    print(f"slow: {args[1:3]} finished alone", flush=True)
    return r


def main():
    work = tempfile.mkdtemp(prefix="lust-smoke-")
    sock = os.path.join(work, "d.sock")
    env = dict(os.environ, LUST_DAEMON_SOCKET=sock, LUST_NO_CACHE="1")
    log = open(os.path.join(work, "daemon.log"), "w")
    daemon = subprocess.Popen([LUST, "daemon"], cwd=work, env=env, stderr=log)
    t0 = time.time()
    while not os.path.exists(sock):
        if time.time() - t0 > 10 or daemon.poll() is not None:
            print("daemon start failed")
            return 2
        time.sleep(0.005)

    def run_lust(args, timeout=20):
        r = run_timed([LUST] + args, timeout, cwd=work, env=env, capture_output=True, text=True)
        return r.returncode, r.stdout, r.stderr

    try:
        # 1. A tab (and a newline) in a file name goes through the daemon.
        for name in ("with\ttab.rs", "with newline\nname.rs"):
            p = os.path.join(work, name)
            open(p, "w").write('fn main() { println!("hi"); }\n')
            rc, out, err = run_lust(["run", p, "-o", os.path.join(work, "x.bin")])
            check(f"file name {name!r}", rc == 0 and out == "hi\n", f"rc={rc} out={out!r} err={err!r}")

        # 2. Run args with separator bytes survive: one arg, argc must stay 2.
        src = os.path.join(work, "args.rs")
        open(src, "w").write('fn main() { println!("argc={}", std::env::args().count()); }\n')
        for arg in ("a\x1fb", "a\tb", "a b", "a\nb"):
            rc, out, err = run_lust(["run", src, "-o", os.path.join(work, "a.bin"), "--", arg])
            check(f"run arg {arg!r}", rc == 0 and out == "argc=2\n", f"rc={rc} out={out!r} err={err!r}")

        # 3. A file with one `(` gets an error fast, and no internal ids in any error.
        bad = os.path.join(work, "bad.rs")
        open(bad, "w").write("(")
        t0 = time.time()
        rc, out, err = run_lust(["check", bad], timeout=10)
        check("paren file errors fast", rc == 1 and time.time() - t0 < 5, f"rc={rc}")
        open(bad, "w").write("fn main() { let x = some_name + ; }\n")
        rc, out, err = run_lust(["check", bad])
        check("no Sym(id) in errors", "Sym(" not in err and rc == 1, f"err={err!r}")

        # 4. Rebuilding the base library must not change a binary built earlier, and old
        # base generations must be cleaned up (at most 4 kept). Bases live in the
        # persistent `.lust-cache/bases` dir under content names, not in the
        # daemon's temp dir, so they survive the daemon (section 12).
        bases_dir = os.path.join(work, ".lust-cache", "bases")

        def base_of(binpath):
            m = re.search(rb"libbase-\d+-[0-9a-f]+\.so", open(binpath, "rb").read())
            return m.group(0).decode() if m else None

        bases_before = set(glob.glob(os.path.join(bases_dir, "libbase-*.so")))
        prog = os.path.join(work, "prog.rs")
        open(prog, "w").write('fn val() -> i64 { 1 }\nfn main() { println!("v{}", val()); }\n')
        rc, out, err = run_lust(["build", prog, "-o", os.path.join(work, "p1.bin")])
        check("build v1", rc == 0, err)
        r = subprocess.run([os.path.join(work, "p1.bin")], capture_output=True, text=True)
        check("v1 runs", r.stdout == "v1\n", r.stdout)
        # Many signature edits force several base rebuilds (overlay cap is 32 units, so
        # add many functions to outgrow the table instead).
        for v in range(2, 10):
            fns = "\n".join(f"fn filler_{v}_{i}() -> i64 {{ {i} }}" for i in range(40))
            open(prog, "w").write(f'fn val() -> i64 {{ {v} }}\nfn main() {{ println!("v{{}}", val()); }}\n{fns}\n')
            rc, out, err = run_lust(["build", prog, "-o", os.path.join(work, "p2.bin")])
            if rc != 0:
                break
        check("builds after base rebuilds", rc == 0, err)
        r = subprocess.run([os.path.join(work, "p1.bin")], capture_output=True, text=True)
        check("old binary unchanged after base rebuilds", r.stdout == "v1\n",
              f"stdout={r.stdout!r} stderr={r.stderr!r}")
        r = subprocess.run([os.path.join(work, "p2.bin")], capture_output=True, text=True)
        check("new binary has new output", r.stdout == "v9\n", r.stdout)
        # This session's bases: the newest 4 generations plus any still referenced by a
        # live binary (here p1.bin's base). Unreferenced old generations must be gone.
        new_bases = set(glob.glob(os.path.join(bases_dir, "libbase-*.so"))) - bases_before
        p1_base = base_of(os.path.join(work, "p1.bin"))
        check("base generations bounded per session",
              0 < len(new_bases) <= 5 and p1_base is not None
              and os.path.exists(os.path.join(bases_dir, p1_base)),
              f"new bases: {sorted(os.path.basename(b) for b in new_bases)}, p1 base: {p1_base}")

        # 5. Regression: a chunk that ends inside a `//` comment must not be reused in front
        # of new text. The `/*` below is inside the line comment, so `b` is real code and
        # both cold and daemon must reject the second state.
        lc = os.path.join(work, "lc.rs")
        open(lc, "w").write("fn main() {}\n// note")
        rc1, _, _ = run_lust(["check", lc])
        open(lc, "w").write("fn main() {}\n// note/* a\nb\nfn fake() {}\n*/\n")
        rc2, _, err2 = run_lust(["check", lc])
        check("line-comment chunk reuse", rc1 == 0 and rc2 == 1 and "L0001" in err2,
              f"rc1={rc1} rc2={rc2}")

        # 6. Regression: a body-only edit that deletes a byte must move the spans of
        # unchanged functions later in the same chunk. Warm and cold panic locations
        # must match; the warm column used to be one too high (fuzz seed 21, iter 774).
        pc = os.path.join(work, "panic_col.rs")
        open(pc, "w").write("fn price(c: i64) -> i64 { c / 100 }\nfn main() {\n    assert_eq!(price(500), 5);\n}\n")
        rc1, _, _ = run_lust(["check", pc])
        open(pc, "w").write("fn price(c: i64) -> i64 { c / 20 }\nfn main() {\n    assert_eq!(price(500), 5);\n}\n")
        rc, out, err = run_lust(["run", pc, "-o", os.path.join(work, "pc.bin")])
        cold_env = dict(env, LUST_NO_DAEMON="1", LUST_DAEMON_SOCKET=os.path.join(work, "none"))
        r = run_timed([LUST, "run", pc, "-o", os.path.join(work, "pc2.bin")], 20, cwd=work,
                           env=cold_env, capture_output=True, text=True)
        warm_loc = re.search(r"panicked at [^:\n]*:(\d+):(\d+)", err)
        cold_loc = re.search(r"panicked at [^:\n]*:(\d+):(\d+)", r.stderr)
        check("panic column after body edit",
              rc1 == 0 and warm_loc is not None and cold_loc is not None
              and warm_loc.groups() == cold_loc.groups() == ("3", "5"),
              f"rc1={rc1} warm={warm_loc.groups() if warm_loc else None} "
              f"cold={cold_loc.groups() if cold_loc else None}")

        # 7. Routing (S6): two feature selections of one file in one daemon
        # stay in separate sessions, and a repeat selection reuses its own.
        # Proof is the daemon's own session records: one "new session" log
        # line per key naming a distinct session dir, plus each binary
        # naming a distinct base library. Since W-RESOLVE the program also
        # branches on cfg(feature), so the two binaries must print
        # different lines.
        feat = os.path.join(work, "feat.rs")
        feat_src = ('#[cfg(feature = "alpha")]\nfn which() -> i64 { 11 }\n'
                    '#[cfg(not(feature = "alpha"))]\nfn which() -> i64 { 22 }\n'
                    'fn main() { println!("feat {}", which()); }\n')
        open(feat, "w").write(feat_src)
        logpath = os.path.join(work, "daemon.log")

        def session_dirs_for(root_path):
            log.flush()
            dirs = {}
            for line in open(logpath):
                if "new session for crate" in line and f"root={root_path}" in line:
                    m = re.search(r"features=\[([^\]]*)\].* in (\S+)$", line.strip())
                    if m:
                        dirs[m.group(1)] = m.group(2)
            return dirs

        rc, out, err = run_lust(["build", feat, "-o", os.path.join(work, "fa.bin"), "--features", "alpha"])
        check("routing: alpha build", rc == 0, err)
        rc, out, err = run_lust(["build", feat, "-o", os.path.join(work, "fb.bin"), "--features", "beta"])
        check("routing: beta build", rc == 0, err)
        dirs = session_dirs_for(feat)
        check("routing: alpha and beta are separate sessions",
              set(dirs) == {"alpha", "beta"} and dirs.get("alpha") != dirs.get("beta"),
              f"dirs={dirs}")
        fa_base, fb_base = base_of(os.path.join(work, "fa.bin")), base_of(os.path.join(work, "fb.bin"))
        check("routing: each session built its own base",
              fa_base is not None and fb_base is not None and fa_base != fb_base
              and os.path.exists(os.path.join(bases_dir, fa_base))
              and os.path.exists(os.path.join(bases_dir, fb_base)),
              f"alpha base={fa_base} beta base={fb_base}")
        r = subprocess.run([os.path.join(work, "fa.bin")], capture_output=True, text=True)
        r2 = subprocess.run([os.path.join(work, "fb.bin")], capture_output=True, text=True)
        check("routing: cfg(feature) differs per session",
              r.stdout == "feat 11\n" and r2.stdout == "feat 22\n",
              f"alpha={r.stdout!r} beta={r2.stdout!r}")
        # Two targets in one daemon: the host triple named explicitly and
        # the default also key separate sessions (lust builds host only).
        host = subprocess.run(["rustc", "-vV"], capture_output=True, text=True).stdout
        host = re.search(r"host: (\S+)", host).group(1)
        rc, out, err = run_lust(["build", feat, "-o", os.path.join(work, "ft.bin"), "--target", host])
        check("routing: named host target build", rc == 0, err)
        log.flush()
        target_dirs = set()
        for line in open(logpath):
            if "new session for crate" in line and f"root={feat}" in line:
                m = re.search(r"target=(\S+) .* in (\S+)$", line.strip())
                if m:
                    target_dirs.add(m.group(2))
        check("routing: two targets, separate sessions", len(target_dirs) >= 3,
              f"dirs={target_dirs}")
        before = session_dirs_for(feat)
        rc, out, err = run_lust(["check", feat, "--features", "alpha"])
        check("routing: alpha again ok", rc == 0, err)
        dirs = session_dirs_for(feat)
        check("routing: repeat selection reuses its session", dirs == before,
              f"dirs={dirs} before={before}")
        # An error under alpha must not poison beta: the same source error is
        # reported by both, and after the fix each answers from its own state.
        open(feat, "w").write('fn main() { let x: i64 = "s"; println!("{}", x); }\n')
        rca, _, ea = run_lust(["check", feat, "--features", "alpha"])
        rcb, _, eb = run_lust(["check", feat, "--features", "beta"])
        check("routing: both selections reject the bad edit",
              rca == 1 and rcb == 1 and ea.splitlines()[:1] == eb.splitlines()[:1],
              f"alpha rc={rca}, beta rc={rcb}")
        open(feat, "w").write(feat_src)
        rca, _, ea = run_lust(["check", feat, "--features", "alpha"])
        rcb, _, eb = run_lust(["check", feat, "--features", "beta"])
        check("routing: both selections accept the fix", rca == 0 and rcb == 0,
              f"alpha rc={rca} {ea!r}, beta rc={rcb} {eb!r}")
        # Background guards for the two error answers must settle with no
        # mismatch: wait for every started check's done line in the log.
        t0 = time.time()
        settled = False
        while time.time() - t0 < 30:
            text = open(logpath).read()
            if text.count("guard-check: start ") == text.count("guard-check: done ") \
                    and "guard-check: start " in text:
                settled = True
                break
            time.sleep(0.02)
        text = open(logpath).read()
        check("routing: background guards settled, none fired",
              settled and "guard:" not in text,
              f"settled={settled} guard_lines={[l for l in text.splitlines() if 'guard:' in l]}")

        # 8. Regression (fuzz seed 42, iter 1034): a return-type edit must
        # recheck the changed fn's own body. The body of `classify` returns
        # an i64 expression, so `-> i32` makes the program invalid; the
        # daemon reused the body on its clean read set (its own signature
        # was not in the set) and accepted it while cold rejected with
        # L0110. Both sides must now answer the same, byte for byte.
        sig_prev = '''struct Point {
    x: i64,
    y: i64,
}

fn main() {
    let mut pts: Vec<Point> = Vec::new();
    pts.push(Point { x: 1, y: 2 });
    pts.push(Point { x: 3, y: 4 });
    pts[0].bump(5);
    let t = total(&pts);
    let c = classify(Shape::Pair(2, 3));
    let nums = vec![2, 20, 30];
    println!("{} {} {} {}", label(t), c, pick(&nums, 1), pts.len());
}

enum Shape {
    Dot,
    Pair(i64, i64),
}

impl Point {
    fn sum(&self) -> i64 {
        self.x + self.y
    }

    fn bump(&mut self, d: i64) {
        self.x += d;
        self.y += d * 2;
    }
}

fn classify(s: Shape) -> i64 {
    match s {
        Shape::Dot => 0,
        Shape::Pair(a, b) => a * 2 + b,
    }
}

fn total(pts: &Vec<Point>) -> i64 {
    let mut acc = 0;
    let mut i = 0;
    while i < pts.len() {
        acc += pts[i].sum();
        i += 1;
    }
    acc
}

fn added_75576() -> i64 { 13 }
fn label(n: i64) -> String {
    let mut s = String::from("n=");
    s.push_str(&n.to_string());
    s
}

fn pick(v: &Vec<i64>, i: usize) -> i64 {
    v[i]
}

fn grown() -> i64 { 1 }
'''
        sig_cur = sig_prev.replace("fn classify(s: Shape) -> i64", "fn classify(s: Shape) -> i32")
        assert sig_cur != sig_prev
        sg = os.path.join(work, "sigedit.rs")
        open(sg, "w").write(sig_prev)
        rc1, _, _ = run_lust(["check", sg])
        open(sg, "w").write(sig_cur)
        rcw, _, errw = run_lust(["check", sg])
        cold_env = dict(env, LUST_NO_DAEMON="1", LUST_DAEMON_SOCKET=os.path.join(work, "none"))
        r = run_timed([LUST, "check", sg], cwd=work, env=cold_env,
                           capture_output=True, text=True, timeout=20)
        check("own return-type edit rechecks the body (seed 42 iter 1034)",
              rc1 == 0 and rcw == 1 and r.returncode == 1
              and "L0110" in errw and errw == r.stderr,
              f"rc1={rc1} warm rc={rcw} err={errw[:120]!r} cold rc={r.returncode}")

        # 9. The daemon is still alive and correct after all of it.
        check("daemon alive", daemon.poll() is None)

        # 10. Regression (fuzz seed 29, iter 562): a bodies-only edit whose
        # later function has a body syntax error must not mark the earlier
        # changed bodies as up to date. Step 2 changes `label` to a body
        # with a check error and breaks `main`'s body syntax, so the
        # compile stops at the syntax error. Step 3 only fixes `main`;
        # the daemon must still report `label`'s error, like cold does.
        # The bug: step 2 stored `label`'s new body hash while `ok` stayed
        # true from step 1, so step 3 never rechecked it.
        pz = os.path.join(work, "poison.rs")
        open(pz, "w").write('fn label() -> i64 { 1 }\nfn main() { println!("{}", label()); }\n')
        rc1, _, _ = run_lust(["check", pz])
        open(pz, "w").write('fn label() -> i64 { x }\nfn main() { println!("{}", label());\n')
        rc2, _, _ = run_lust(["check", pz])
        open(pz, "w").write('fn label() -> i64 { x }\nfn main() { println!("{}", label()); }\n')
        rcw, _, errw = run_lust(["check", pz])
        cold_env = dict(env, LUST_NO_DAEMON="1", LUST_DAEMON_SOCKET=os.path.join(work, "none"))
        r = run_timed([LUST, "check", pz], cwd=work, env=cold_env,
                           capture_output=True, text=True, timeout=20)
        check("syntax-error edit does not poison earlier bodies (seed 29 iter 562)",
              rc1 == 0 and rc2 == 1 and rcw == 1 and r.returncode == 1
              and "L0101" in errw and errw == r.stderr,
              f"rc1={rc1} rc2={rc2} warm rc={rcw} err={errw[:120]!r} cold rc={r.returncode}")

        # 11. Regression (fuzz realish seed 107, iter 585): resolver routing
        # must not stay sticky after the trigger item is deleted. Step 1
        # routes to the resolver (a `static` item) and fails with the
        # struct syntax error. Step 2 deletes the static and breaks a
        # macro name. A fresh session compiles step 2 through the chunk
        # front end and reports the unknown macro (L0802) first; the
        # sticky warm session kept the resolver front end and reported
        # the syntax error. Both sides must answer the same, byte for byte.
        rt = os.path.join(work, "route.rs")
        open(rt, "w").write('static S: i64 = 1;\nstruct P {\n    fn f() {}\n}\nfn main() { println!("{}", format!("{}", S)); }\n')
        rc1, _, _ = run_lust(["check", rt])
        open(rt, "w").write('struct P {\n    fn f() {}\n}\nfn main() { println!("{}", xformat!("{}", 1)); }\n')
        rcw, _, errw = run_lust(["check", rt])
        cold_env = dict(env, LUST_NO_DAEMON="1", LUST_DAEMON_SOCKET=os.path.join(work, "none"))
        r = run_timed([LUST, "check", rt], cwd=work, env=cold_env,
                           capture_output=True, text=True, timeout=20)
        check("resolver routing unsticks when the trigger goes (realish seed 107 iter 585)",
              rc1 == 1 and rcw == 1 and r.returncode == 1
              and "L0802" in errw and errw == r.stderr,
              f"rc1={rc1} warm rc={rcw} err={errw[:120]!r} cold rc={r.returncode}")
        # The route fall-back drops resolver-built session state; the
        # session must still build and run the fixed program after it.
        open(rt, "w").write('fn main() { println!("route {}", 40 + 2); }\n')
        rc, out, err = run_lust(["run", rt, "-o", os.path.join(work, "rt.bin")])
        check("session builds after the route fall-back",
              rc == 0 and out == "route 42\n", f"rc={rc} out={out!r} err={err!r}")

        # 11b. A deleted bases dir under a live session heals: outside
        # tools may delete `.lust-cache` as a cache (the multi-file edit
        # fuzzer rewrites the crate tree every edit, seed 7 iter 25).
        # The next build relinks the same base from its objects. A
        # subdirectory source keeps this check away from section 12's
        # bases.
        sub = os.path.join(work, "heal")
        os.makedirs(sub)
        heal = os.path.join(sub, "heal.rs")
        open(heal, "w").write('fn main() { println!("heal {}", 1); }\n')
        rc, out, err = run_lust(["build", heal, "-o", os.path.join(sub, "h1.bin")])
        check("vanished bases dir: first build ok", rc == 0, err)
        shutil.rmtree(os.path.join(sub, ".lust-cache", "bases"))
        open(heal, "w").write('fn main() { println!("heal {}", 2); }\n')
        rc, out, err = run_lust(["build", heal, "-o", os.path.join(sub, "h2.bin")])
        check("vanished bases dir: build still works", rc == 0, err)
        r = subprocess.run([os.path.join(sub, "h2.bin")], capture_output=True, text=True)
        check("vanished bases dir: binary runs", r.stdout == "heal 2\n",
              f"stdout={r.stdout!r} stderr={r.stderr!r}")

        # 13. Regression (fuzz realish seed 1, iters 358/477): an impl
        # inside a fn body makes the token scan count one more fn than
        # the Ast fn list. slot_update indexed the Ast-side arrays by
        # the scan count and the compiler panicked (cold rc 101, the
        # daemon's fresh session reported an internal error). The
        # resolver now hoists the impl to the module items (tf-fnimpl):
        # both sides must accept, with the same output.
        ib = os.path.join(work, "implbody.rs")
        open(ib, "w").write('fn first() {}\nfn main() {}\n')
        rc1, _, _ = run_lust(["check", ib])
        open(ib, "w").write('struct S;\nfn outer() {\nfn inner() {\n}\nimpl S {\n    fn m(&self) {\n    }\n}\n}\nfn main() { outer(); }\n')
        rcw, _, errw = run_lust(["check", ib])
        cold_env = dict(env, LUST_NO_DAEMON="1", LUST_DAEMON_SOCKET=os.path.join(work, "none"))
        r = run_timed([LUST, "check", ib], cwd=work, env=cold_env,
                           capture_output=True, text=True, timeout=20)
        check("impl in fn body: scan/Ast count split does not panic (realish seed 1)",
              rc1 == 0 and rcw == 0 and r.returncode == 0
              and errw == r.stderr and "panicked" not in r.stderr,
              f"rc1={rc1} warm rc={rcw} err={errw[:120]!r} cold rc={r.returncode}")

        # 14. Duplicate definitions through warm edits: adding a second
        # trait of one name (or a second `mod` of one name) must reject
        # with L0106 warm exactly as cold does, byte for byte, and
        # removing the duplicate must accept again. The cold duplicate
        # trait used to panic in collect (pass 1.5 looked the second
        # trait's methods up in the first trait's record).
        dp = os.path.join(work, "dup.rs")
        ok_src = 'trait T {\n    fn a(&self) -> i64;\n}\nstruct S;\nimpl T for S {\n    fn a(&self) -> i64 { 1 }\n}\nfn main() { println!("{}", S.a()); }\n'
        dup_src = ok_src.replace(
            'struct S;\n', 'trait T {\n    fn b(&self) -> i64;\n}\nstruct S;\n')
        assert dup_src != ok_src
        cold_env = dict(env, LUST_NO_DAEMON="1", LUST_DAEMON_SOCKET=os.path.join(work, "none"))
        open(dp, "w").write(ok_src)
        rc1, _, _ = run_lust(["check", dp])
        open(dp, "w").write(dup_src)
        rcw, _, errw = run_lust(["check", dp])
        r = run_timed([LUST, "check", dp], cwd=work, env=cold_env,
                           capture_output=True, text=True, timeout=20)
        check("warm duplicate trait rejects like cold (no panic)",
              rc1 == 0 and rcw == 1 and r.returncode == 1
              and "L0106" in errw and errw == r.stderr and "panicked" not in r.stderr,
              f"rc1={rc1} warm rc={rcw} err={errw[:120]!r} cold rc={r.returncode}")
        dup_mod_src = ok_src.replace(
            'fn main()', 'mod m {}\nmod m {}\nfn main()')
        open(dp, "w").write(dup_mod_src)
        rcw, _, errw = run_lust(["check", dp])
        r = run_timed([LUST, "check", dp], cwd=work, env=cold_env,
                           capture_output=True, text=True, timeout=20)
        check("warm duplicate mod rejects like cold",
              rcw == 1 and r.returncode == 1
              and "L0106" in errw and errw == r.stderr,
              f"warm rc={rcw} err={errw[:120]!r} cold rc={r.returncode}")
        open(dp, "w").write(ok_src)
        rc, out, err = run_lust(["run", dp, "-o", os.path.join(work, "dup.bin")])
        check("duplicate removed: warm accepts and runs again",
              rc == 0 and out == "1\n", f"rc={rc} out={out!r} err={err!r}")

        # 15. Regression (fx-priv): a private field read inside a
        # generic fn of another module passed `lust check` while the
        # build rejected it. An edit that makes the field private
        # must make the warm check reject with L0510, like the cold
        # check, and the edit back must pass warm again.
        pv_a = os.path.join(work, "pv_a.rs")
        pv_main = os.path.join(work, "pv_main.rs")
        open(pv_main, "w").write(
            'mod pv_a;\n'
            'fn get<T>(s: &pv_a::S, t: T) -> T { let _ = s.x; t }\n'
            'fn main() { let s = pv_a::S::new(); println!("{}", get(&s, 3)); }\n')
        open(pv_a, "w").write(
            'pub struct S { pub x: i32 }\n'
            'impl S { pub fn new() -> S { S { x: 1 } } }\n')
        rc1, _, _ = run_lust(["check", pv_main])
        open(pv_a, "w").write(
            'pub struct S { x: i32 }\n'
            'impl S { pub fn new() -> S { S { x: 1 } } }\n')
        rcw, _, errw = run_lust(["check", pv_main])
        r = run_timed([LUST, "check", pv_main], cwd=work, env=cold_env,
                           capture_output=True, text=True, timeout=20)
        check("field made private: warm check rejects the generic reader (fx-priv)",
              rc1 == 0 and rcw == 1 and r.returncode == 1
              and "L0510" in errw and "L0510" in r.stderr,
              f"rc1={rc1} warm rc={rcw} err={errw[:120]!r} cold rc={r.returncode}")
        open(pv_a, "w").write(
            'pub struct S { pub x: i32 }\n'
            'impl S { pub fn new() -> S { S { x: 1 } } }\n')
        rc3, _, _ = run_lust(["check", pv_main])
        check("field made public again: warm check passes (fx-priv)",
              rc3 == 0, f"rc3={rc3}")

        # 16. Regression (fx-vis-fp): a visibility-only edit must reach the
        # warm answer for plain (non-generic) readers. Each kind flips one
        # visibility and walks pub -> narrowed -> pub -> narrowed (or the
        # reverse, `start_narrow`) under the warm daemon; every step must
        # give the cold answer (rc and first error code). Narrowed must
        # reject warm; widened must accept warm. The body checks record a
        # `Sig(adt)` read whose fingerprint covers member visibility.
        vis_pa = ('pub struct S { pub x: i32, pub y: i32 }\n'
                  'impl S { pub fn new() -> S { S { x: 1, y: 2 } } pub fn get(&self) -> i32 { self.x } }\n'
                  'pub fn make() -> S { S::new() }\n'
                  'pub struct T(pub i32, pub i32);\n'
                  'pub fn mk() -> T { T(1, 2) }\n'
                  'pub fn f() -> i32 { 1 }\n'
                  'pub mod inner {\n'
                  '    pub struct R { pub z: i32 }\n'
                  '    pub fn r() -> R { R { z: 1 } }\n'
                  '    pub fn g() -> i32 { 2 }\n'
                  '}\n'
                  'pub fn read_inner() -> i32 { inner::r().z }\n'
                  'mod hidden { pub fn h() -> i32 { 3 } }\n'
                  'pub use self::hidden::h;\n')
        vis_cases = [
            # (name, reader body of `show() -> i32`, old text, new text, start_narrow)
            ("named field read", "pa::make().x", "pub x", "x", False),
            ("named field read, private first", "pa::make().x", "pub x", "x", True),
            ("field write", "let mut s = pa::make(); s.y = 4; s.y", "pub y", "pub(self) y", False),
            ("struct literal", "let s = pa::S { x: 1, y: 2 }; s.y", "pub y", "y", False),
            ("struct pattern", "let pa::S { x, .. } = pa::make(); x", "pub x", "x", False),
            ("functional update", "let s = pa::S { x: 5, ..pa::make() }; s.x", "pub y", "y", False),
            ("tuple field read", "pa::mk().1", "pub i32);", "i32);", False),
            ("tuple ctor", "let t = pa::T(3, 4); t.0", "pub i32);", "i32);", False),
            ("tuple pattern", "let pa::T(a, _) = pa::mk(); a", "pub i32, pub i32", "i32, pub i32", False),
            ("method call", "pa::make().get()", "pub fn get", "fn get", False),
            ("method call, private first", "pa::make().get()", "pub fn get", "fn get", True),
            ("method pub to pub(in crate::pa)", "pa::make().get()", "pub fn get", "pub(in crate::pa) fn get", False),
            ("field read from the parent module", "pa::read_inner()", "pub z", "z", False),
            ("field pub(in) narrowed", "pa::read_inner()", "pub z", "pub(in crate::pa::inner) z", False),
            ("assoc fn path", "pa::S::new().y", "pub fn new", "fn new", False),
            ("free fn path", "pa::f()", "pub fn f()", "fn f()", False),
            ("fn path pub to pub(super)", "pa::inner::g()", "pub fn g()", "pub(super) fn g()", False),
            ("module visibility", "pa::inner::g()", "pub mod inner", "mod inner", False),
            ("re-export visibility", "pa::h()", "pub use", "use", False),
        ]
        vis_dir = os.path.join(work, "vis")
        os.makedirs(vis_dir, exist_ok=True)

        def first_code(s):
            m = re.search(r"error\[(L\d+)\]", s)
            return m.group(1) if m else None

        # Each kind runs twice: a plain program, and one with a trait
        # (a generic program, which takes the generic reuse path).
        marker = 'trait Marker { fn mark(&self) -> i32 { 1 } }\nimpl Marker for u8 {}\n'
        for k, (name, body, old, new, start_narrow) in enumerate(vis_cases * 2):
            generic = k >= len(vis_cases)
            main_p = os.path.join(vis_dir, f"vmain{k}.rs")
            pa_p = os.path.join(vis_dir, "pa.rs")
            open(main_p, "w").write(
                'mod pa;\n'
                f'fn show() -> i32 {{ {body} }}\n'
                'fn main() { println!("{}", show()); }\n' + (marker if generic else ''))
            assert old in vis_pa, old
            narrow = vis_pa.replace(old, new, 1)
            states = [narrow, vis_pa, narrow] if start_narrow else [vis_pa, narrow, vis_pa, narrow, vis_pa]
            steps, ok = [], True
            for text in states:
                open(pa_p, "w").write(text)
                rcw, _, errw = run_lust(["check", main_p])
                r = run_timed([LUST, "check", main_p], cwd=work, env=cold_env,
                                   capture_output=True, text=True, timeout=20)
                want = 1 if text is narrow else 0
                ok &= rcw == r.returncode == want and first_code(errw) == first_code(r.stderr)
                steps.append(f"{'narrow' if text is narrow else 'pub'}: warm {rcw} {first_code(errw)} "
                             f"cold {r.returncode} {first_code(r.stderr)}")
            kind = f"{name}{', program with a trait' if generic else ''}"
            check(f"visibility edit, {kind}: warm answer equals cold (fx-vis-fp)", ok, "; ".join(steps))

        # 19. Regression (fx-vis-fp audit): `Self::item` paths from an impl
        # in another module check the item's visibility (rustc E0624) and
        # read `Sig(adt)`, so a warm check sees a visibility flip; and an
        # associated const read bakes the value in, so it records the
        # const's `Sig` read: a value edit must reach the warm run (the
        # warm run printed the old value before).
        sp_pa = 'pub struct S { pub x: i32 }\nimpl S { pub const C: i32 = 3; pub fn get(&self) -> i32 { self.x } }\n'
        sp_dir = os.path.join(work, "selfpath")
        os.makedirs(sp_dir, exist_ok=True)
        sp_main_p = os.path.join(sp_dir, "main.rs")
        for name, reader, old, new, rejects in (
                ("Self:: method made private", "Self::get(self)", "pub fn get", "fn get", True),
                ("Self:: assoc const made private", "Self::C", "pub const C", "const C", True),
                ("assoc const value edit", "Self::C * 10 + pa::S::C", "C: i32 = 3", "C: i32 = 4", False)):
            open(sp_main_p, "w").write(
                'mod pa;\n'
                f'impl pa::S {{ fn other(&self) -> i32 {{ {reader} }} }}\n'
                'fn main() { println!("{}", pa::S { x: 1 }.other()); }\n')
            steps, ok = [], True
            edited = sp_pa.replace(old, new, 1)
            for text in (sp_pa, edited, sp_pa, edited):
                open(os.path.join(sp_dir, "pa.rs"), "w").write(text)
                outs = []
                for e in (env, cold_env):
                    r = run_timed([LUST, "run", sp_main_p, "-o", os.path.join(sp_dir, "x.bin")], cwd=work, env=e,
                                       capture_output=True, text=True, timeout=30)
                    outs.append((r.returncode, r.stdout, first_code(r.stderr)))
                want_rc = 1 if rejects and text is edited else 0
                ok &= outs[0] == outs[1] and outs[0][0] == want_rc
                steps.append(f"warm {outs[0]} cold {outs[1]}")
            check(f"{name}: warm run equals cold (fx-vis-fp)", ok, "; ".join(steps))

        # 17. Regression (fuzz seed 658, iter 374): the slot count crossed
        # a unit boundary on dead slots only. A check gave 40 new fns slots
        # past the first fn unit, the next edit removed them, and the warm
        # run relinked the base over a new unit that no build compiled
        # (`u5.o` not found). Every new unit must get an object.
        gr = os.path.join(work, "grow_dead.rs")
        gr_base = "".join(f"fn f{i}() -> i64 {{ {i} }}\n" for i in range(14))
        gr_base += 'fn main() { println!("{}", ' + " + ".join(f"f{i}()" for i in range(14)) + "); }\n"
        gr_dead = "".join(f"fn x{i}() -> i64 {{ {i} }}\n" for i in range(40))
        open(gr, "w").write(gr_base)
        rc1, out1, _ = run_lust(["run", gr, "-o", os.path.join(work, "gr.bin")])
        open(gr, "w").write(gr_base + gr_dead)
        rc2, _, _ = run_lust(["check", gr])
        open(gr, "w").write(gr_base)
        rc3, out3, err3 = run_lust(["run", gr, "-o", os.path.join(work, "gr.bin")])
        check("dead slots past a unit boundary: warm run links and runs (seed 658)",
              rc1 == 0 and out1 == "91\n" and rc2 == 0 and rc3 == 0 and out3 == "91\n",
              f"rc1={rc1} rc2={rc2} rc3={rc3} out3={out3!r} err3={err3[:160]!r}")

        # 18. Regression (fx-seed658): file inner attributes under the chunked
        # parse. Each chunk parses on its own, so a warm chunk that started at
        # a `#![..]` took it as file attributes even after an item, and a kept
        # chunk kept the crate allow it was parsed under after the allow was
        # removed (both warm false accepts). A crate allow must also reach fns
        # in other chunks, warm and in a cold file over 64 KB. Each walk runs
        # in its own session; every step must give the rustc answer (`want`)
        # warm and cold, with the same first error code.
        ia_main = 'fn a() -> i64 {\n    1\n}\n\nfn main() {\n    println!("{}", a());\n}\n'
        ia_over = 'fn g() -> u8 {\n    let x: u8 = 255 + 1;\n    x\n}\n\nfn main() {\n    println!("{}", g());\n}\n'
        ia_ok = ia_over.replace("let x: u8 = 255 + 1;\n    x", "1")
        ia_walks = [
            ("item put above a crate attribute",
             [("#![allow(dead_code)]\n" + ia_main, 0), ("struct Zed;\n\n#![allow(dead_code)]\n" + ia_main, 1)]),
            ("inner attribute put after a kept item",
             [(ia_main, 0), ("struct Zed;\n\n" + ia_main, 0), ("struct Zed;\n\n#![allow(dead_code)]\n" + ia_main, 1)]),
            ("crate allow put above a kept fn",
             [(ia_over, 1), ("#![allow(arithmetic_overflow)]\n" + ia_over, 0)]),
            # The kept second chunk was parsed under the crate allow; removing
            # the allow re-parsed only the first chunk (signatures mode).
            ("crate allow removed over a kept chunk",
             [(ia_ok, 0),
              ("#![allow(arithmetic_overflow)]\nfn zed() -> i64 {\n    3\n}\n\n" + ia_ok, 0),
              ("#![allow(arithmetic_overflow)]\nfn zed() -> i64 {\n    3\n}\n\n" + ia_over + "struct S;\n", 0),
              ("fn zed() -> i64 {\n    3\n}\n\n" + ia_over + "struct S;\n", 1)]),
            ("crate allow over a file of many chunks",
             [("#![allow(arithmetic_overflow)]\n"
               + "".join(f"fn filler{i}(x: i64) -> i64 {{\n    x * {i} + 1\n}}\n\n" for i in range(2500)) + ia_over, 0)]),
        ]
        for k, (name, walk) in enumerate(ia_walks):
            ia = os.path.join(work, f"inner_attr{k}.rs")
            steps, ok = [], True
            for text, want in walk:
                open(ia, "w").write(text)
                rcw, _, errw = run_lust(["check", ia])
                r = run_timed([LUST, "check", ia], cwd=work, env=cold_env,
                                   capture_output=True, text=True, timeout=20)
                ok &= rcw == r.returncode == want and first_code(errw) == first_code(r.stderr)
                steps.append(f"want {want}: warm {rcw} {first_code(errw)} cold {r.returncode} {first_code(r.stderr)}")
            check(f"inner attributes, {name}: warm and cold give the rustc answer", ok, "; ".join(steps))
        # 19. Regression (fx-append-chunk, found by fx-seed658): a chunk
        # re-parse must replace that chunk's body items, not append next
        # to the old ones. Step 1 checks a file with a fn nested in
        # `main`; step 2 changes another fn's signature and runs. The
        # stale nested fn made collect see one fn more than `fn_slot`
        # and the warm build panicked (index out of bounds, caught by
        # the daemon guard). A body-only edit of the outer fn and a
        # delete/re-add of the nested fn must also stay exact.
        logpath = os.path.join(work, "daemon.log")

        def panics():
            log.flush()
            return open(logpath).read().count("panicked")

        before_panics = panics()
        nx = os.path.join(work, "nest.rs")
        open(nx, "w").write(
            'fn f0(x: i64) -> i64 {\n    x\n}\n\n'
            'fn main() {\n    let p = 1;\n    fn zed() -> i64 { 3 }\n'
            '    println!("{} {}", p + f0(1), zed());\n}\n')
        rc1, _, _ = run_lust(["check", nx])
        open(nx, "w").write(
            'fn f0(x: i64, _z: i64) -> i64 {\n    x\n}\n\n'
            'fn main() {\n    let p = 1;\n    fn zed() -> i64 { 3 }\n'
            '    println!("{} {}", p + f0(1, 2), zed());\n}\n')
        rc, out, err = run_lust(["run", nx, "-o", os.path.join(work, "nx.bin")])
        check("signature edit with a nested fn runs warm (fx-append-chunk)",
              rc1 == 0 and rc == 0 and out == "2 3\n" and panics() == before_panics,
              f"rc1={rc1} rc={rc} out={out!r} err={err!r} panics={panics()}")
        open(nx, "w").write(
            'fn f0(x: i64, _z: i64) -> i64 {\n    x\n}\n\n'
            'fn main() {\n    let p = 10;\n    fn zed() -> i64 { 3 }\n'
            '    println!("{} {}", p + f0(1, 2), zed());\n}\n')
        rc, out, err = run_lust(["run", nx, "-o", os.path.join(work, "nx.bin")])
        check("body edit of the outer fn runs warm (fx-append-chunk)",
              rc == 0 and out == "11 3\n" and panics() == before_panics,
              f"rc={rc} out={out!r} err={err!r}")
        open(nx, "w").write('fn main() {\n    println!("{}", 7);\n}\n')
        rc, out, err = run_lust(["run", nx, "-o", os.path.join(work, "nx.bin")])
        check("nested fn deleted: warm runs the new body (fx-append-chunk)",
              rc == 0 and out == "7\n" and panics() == before_panics,
              f"rc={rc} out={out!r} err={err!r}")
        open(nx, "w").write(
            'fn main() {\n    fn zed() -> i64 { 9 }\n    println!("{}", zed());\n}\n')
        rc, out, err = run_lust(["run", nx, "-o", os.path.join(work, "nx.bin")])
        check("nested fn re-added: warm runs the new body (fx-append-chunk)",
              rc == 0 and out == "9\n" and panics() == before_panics,
              f"rc={rc} out={out!r} err={err!r}")

        # 19b. The same value edit through a qualified path (`<S>::C`).
        qp_p = os.path.join(sp_dir, "qp.rs")
        steps, ok = [], True
        for val in ("3", "4", "3", "4"):
            open(qp_p, "w").write(f"struct S;\nimpl S {{\n    const C: i32 = {val};\n}}\n"
                                  "fn show() -> i32 {\n    <S>::C\n}\nfn main() {\n    println!(\"{}\", show());\n}\n")
            outs = []
            for e in (env, cold_env):
                r = run_timed([LUST, "run", qp_p, "-o", os.path.join(sp_dir, "q.bin")], cwd=work, env=e,
                                   capture_output=True, text=True, timeout=30)
                outs.append((r.returncode, r.stdout))
            ok &= outs[0] == outs[1] == (0, f"{val}\n")
            steps.append(f"warm {outs[0]} cold {outs[1]}")
        check("qualified path assoc const value edit: warm run equals cold (fx-vis-fp)", ok, "; ".join(steps))

        # 20. Regression (fx-vis-fp, next 1c77df7): since generic reuse
        # keeps bodies across item edits outside the trait system, a
        # trait `use` edit must reach the warm answer. `trait_in_scope`
        # records a `TraitInScope` read: removing the import (plain,
        # `as _`, glob, or in another module) must reject warm like cold
        # (L0104), and the import back must accept warm.
        tu_dir = os.path.join(work, "traituse")
        os.makedirs(tu_dir, exist_ok=True)
        open(os.path.join(tu_dir, "pa.rs"), "w").write(
            'pub trait Tr { fn tr(&self) -> i32; }\npub struct S;\nimpl Tr for S { fn tr(&self) -> i32 { 5 } }\n')
        open(os.path.join(tu_dir, "pb.rs"), "w").write(
            'use crate::pa::Tr;\npub fn show() -> i32 { crate::pa::S.tr() }\n')
        tu_cases = [
            ("plain use", "main.rs", 'mod pa;\nuse pa::Tr;\nfn main() { println!("{}", pa::S.tr()); }\n', "use pa::Tr;\n"),
            ("use as _", "main.rs", 'mod pa;\nuse pa::Tr as _;\nfn main() { println!("{}", pa::S.tr()); }\n', "use pa::Tr as _;\n"),
            ("glob use", "main.rs", 'mod pa;\nuse pa::*;\nfn main() { println!("{}", S.tr()); }\n', "use pa::*;\n"),
            ("use in another module", "pb.rs", None, "use crate::pa::Tr;\n"),
        ]
        for k, (name, rel, main_text, use_line) in enumerate(tu_cases):
            main_p = os.path.join(tu_dir, f"tmain{k}.rs")
            if main_text is None:
                main_text = 'mod pa;\nmod pb;\nfn main() { println!("{}", pb::show()); }\n'
            open(main_p, "w").write(main_text)
            edit_p = main_p if rel == "main.rs" else os.path.join(tu_dir, rel)
            base = open(edit_p).read()
            # The glob case loses `S` too; keep it nameable without the trait.
            without = base.replace(use_line, "use pa::S;\n" if use_line == "use pa::*;\n" else "", 1)
            steps, ok = [], True
            for text in (base, without, base, without, base):
                open(edit_p, "w").write(text)
                rcw, _, errw = run_lust(["check", main_p])
                r = run_timed([LUST, "check", main_p], cwd=work, env=cold_env,
                                   capture_output=True, text=True, timeout=20)
                want = 1 if text is without else 0
                ok &= rcw == r.returncode == want and first_code(errw) == first_code(r.stderr)
                steps.append(f"{'without' if text is without else 'with'}: warm {rcw} {first_code(errw)} "
                             f"cold {r.returncode} {first_code(r.stderr)}")
            open(edit_p, "w").write(base)
            check(f"trait use edit, {name}: warm answer equals cold (fx-vis-fp)", ok, "; ".join(steps))

        # 21. Regression (fuzz-sheriff3 small3, w-borrow note 08:49): a
        # lifetime-only signature edit (`&str` -> `&'static str` param)
        # changes no erased type, so the caller's borrowck verdict must
        # still be redone warm: main passes a temporary, cold rejects
        # L0306, warm used to accept. Region signatures are in the Sig
        # fingerprint since e86a33fc.
        lf = os.path.join(work, "lifetime_sig.rs")
        lf_base = ('fn take_s(s: &str) -> usize { s.len() }\n'
                   'fn lab() -> String { String::from("tmp") }\n'
                   'fn main() { println!("{}", take_s(&lab())); }\n')
        lf_static = lf_base.replace("fn take_s(s: &str)", "fn take_s(s: &'static str)", 1)
        steps, ok = [], True
        for text in (lf_base, lf_static, lf_base):
            open(lf, "w").write(text)
            rcw, _, errw = run_lust(["check", lf])
            r = run_timed([LUST, "check", lf], cwd=work, env=cold_env,
                               capture_output=True, text=True, timeout=20)
            want = 1 if text is lf_static else 0
            ok &= rcw == r.returncode == want and first_code(errw) == first_code(r.stderr)
            steps.append(f"want {want}: warm {rcw} {first_code(errw)} cold {r.returncode} {first_code(r.stderr)}")
        check("lifetime-only signature edit: warm answer equals cold (fuzz-sheriff3)", ok, "; ".join(steps))

        # 12. Temp cleanup: SIGTERM removes the daemon's temp dir, and
        # executables built against its base libraries still run (bases
        # live in `.lust-cache/bases`, section 4).
        daemon_dir = os.path.join(tempfile.gettempdir(), f"lust-daemon-{daemon.pid}")
        check("temp: daemon temp dir exists while running", os.path.isdir(daemon_dir), daemon_dir)
        daemon.terminate()
        daemon.wait(timeout=10)
        check("temp: daemon temp dir removed on SIGTERM", not os.path.exists(daemon_dir), daemon_dir)
        r = subprocess.run([os.path.join(work, "p1.bin")], capture_output=True, text=True)
        check("temp: kept binary still runs after daemon exit", r.stdout == "v1\n",
              f"stdout={r.stdout!r} stderr={r.stderr!r}")
        p1_base_path = os.path.join(bases_dir, p1_base) if p1_base else ""
        check("temp: kept binary's base survives daemon exit",
              bool(p1_base_path) and os.path.exists(p1_base_path), p1_base_path)
        # Once the executable is deleted, the next daemon's first base
        # build sweeps its base via the manifest, while bases of kept
        # executables (fa.bin) stay.
        os.remove(os.path.join(work, "p1.bin"))
        sock2 = os.path.join(work, "d2.sock")
        env2 = dict(env, LUST_DAEMON_SOCKET=sock2)
        with open(os.path.join(work, "daemon2.log"), "w") as log2:
            daemon2 = subprocess.Popen([LUST, "daemon"], cwd=work, env=env2, stderr=log2)
            try:
                t0 = time.time()
                while not os.path.exists(sock2):
                    if time.time() - t0 > 10 or daemon2.poll() is not None:
                        break
                    time.sleep(0.005)
                open(prog, "w").write('fn val() -> i64 { 10 }\nfn main() { println!("v{}", val()); }\n')
                r = run_timed([LUST, "build", prog, "-o", os.path.join(work, "p3.bin")],
                                   cwd=work, env=env2, capture_output=True, text=True, timeout=20)
                check("temp: second daemon builds", r.returncode == 0, r.stderr)
            finally:
                daemon2.terminate()
                daemon2.wait(timeout=10)
        check("temp: deleted binary's base swept by the next daemon",
              bool(p1_base_path) and not os.path.exists(p1_base_path), p1_base_path)
        r = subprocess.run([os.path.join(work, "fa.bin")], capture_output=True, text=True)
        check("temp: another kept binary still runs after the sweep", r.stdout == "feat 11\n",
              f"stdout={r.stdout!r} stderr={r.stderr!r}")
    finally:
        daemon.terminate()
        log.close()
        shutil.rmtree(work, ignore_errors=True)
    print(f"smoke: {'FAIL ' + ', '.join(failures) if failures else 'all passed'}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
