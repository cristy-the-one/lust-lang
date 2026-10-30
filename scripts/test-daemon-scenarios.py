#!/usr/bin/env python3
# Daemon edit scenarios: every tests/daemon/<name>/ version sequence runs
# against one warm daemon, a cold lust, and rustc. The warm answer must
# match the cold answer (a check or run that rejects must give the same
# first error code) and, when lust accepts the program, the rustc answer. This is the regression net for the dependency edges of
# docs/ARCH.md 8.1-8.3 (const/static values from const fns, consts in
# patterns, derived impls under type edits, layout answers in bodies).
#
# Restart mode (DAEMON_SCENARIO_RESTART=1): no daemon; every warm request
# is its own one-shot process with a front image (LUST_FRONT_STATE, a state
# dir per scenario, src/image.rs), so each version's check and run start
# from the session the request before them saved: the daemon's incremental
# paths across a process exit. Same comparisons as daemon mode.
#
# Usage: python3 scripts/test-daemon-scenarios.py [scenario ...]
# Env: LUST_BIN (default target/release/lust), DAEMON_SCENARIO_JOBS,
# DAEMON_SCENARIO_RESTART.
# Exit 0 = pass, 1 = a mismatch, 2 = setup error.
import os, re, shutil, signal, subprocess, sys, tempfile, time
from concurrent.futures import ProcessPoolExecutor
import oracle_log  # scripts/oracle_log.py

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LUST = os.environ.get("LUST_BIN", os.path.join(ROOT, "target/release/lust"))
SCEN = os.path.join(ROOT, "tests/daemon")
RESTART = os.environ.get("DAEMON_SCENARIO_RESTART", "") not in ("", "0")


def run(cmd, cwd, env, timeout=60):
    # errors="replace": a wrong program may print bytes that are not UTF-8.
    r = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, errors="replace", timeout=timeout)
    return r.returncode, r.stdout, r.stderr


# A Unix socket path holds at most 107 bytes (sun_path). A work dir under a long TMPDIR (a local
# gate slot's /home/theo/lust-scratch/gate-tmp/<pkg>/lane-<x>) plus a scenario's name in its
# prefix passes that, and the daemon cannot bind: "daemon start failed".
SUN_PATH_MAX = 107


def sock_path(work):
    """`work`/d.sock, or d.sock in a new short dir under /tmp when that path is too long for a
    Unix socket (`sock_dir_of` gives the dir to remove)."""
    p = os.path.join(work, "d.sock")
    if len(os.fsencode(p)) <= SUN_PATH_MAX:
        return p
    return os.path.join(tempfile.mkdtemp(prefix="lsd-", dir="/tmp"), "d.sock")


def sock_dir_of(sock, work):
    """The dir `sock_path` made for `sock`, or None when the socket is in `work`."""
    d = os.path.dirname(sock)
    return None if d == work else d


def cold_of(env):
    """The env of a cold request: no daemon, and no front image."""
    cold = dict(env, LUST_NO_DAEMON="1")
    cold.pop("LUST_FRONT_STATE", None)
    cold.pop("LUST_FRONT_STATE_VERBOSE", None)
    return cold


def warm_problems(tag, err, first):
    """Restart mode: every warm request but the first must start from the
    image the one before it saved (`restored`), unless the crate selection
    changed (one image holds one session; a daemon keeps one per
    selection); a failed save is a failure too. An oracle failure answers
    as an internal error there, as in a daemon."""
    if not RESTART:
        return []
    lines = [l.strip() for l in err.splitlines() if l.startswith("lust: front state")]
    out = [f"{tag}: {l[:300]}" for l in lines if l.startswith("lust: front state not saved")]
    load = next((l for l in lines if l.startswith("lust: front state restored") or l.startswith("lust: front state cold")), None)
    if load is None:
        out.append(f"{tag}: no front state line in the warm request")
    elif not first and "restored" not in load and "crate selection" not in load:
        out.append(f"{tag}: warm request did not restore: {load[:300]}")
    return out


def first_code(stderr):
    """The first `error[L####]` code in a diagnostic text, or None."""
    m = re.search(r"error\[(L\d+)\]", stderr)
    return m.group(1) if m else None


def apply_overlay(vpath, work, mtime):
    """Cargo mode: copies the files under `vpath` into `work` (same
    relative paths, mtime `mtime`) and reads its `.scen` directives:
    `rm <path>` deletes a file or dir, `cmd <root> <flags...>` sets this
    version's lust command (kept for later versions), and `env K=V` sets
    a client env var for this version only."""
    directives = []
    for dirpath, _, files in os.walk(vpath):
        for f in files:
            src = os.path.join(dirpath, f)
            rel = os.path.relpath(src, vpath)
            if rel == ".scen":
                directives = open(src).read().splitlines()
                continue
            dst = os.path.join(work, rel)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copyfile(src, dst)
            os.utime(dst, (mtime, mtime))
    cmd, env = None, {}
    for line in directives:
        verb, _, rest = line.strip().partition(" ")
        if verb == "rm":
            path = os.path.join(work, rest.strip())
            shutil.rmtree(path) if os.path.isdir(path) else os.remove(path)
            # Bump the dir mtime past the last version's, as an editor would.
            os.utime(os.path.dirname(path), (mtime, mtime))
        elif verb == "cmd":
            cmd = rest.split()
        elif verb == "env":
            k, _, v = rest.partition("=")
            env[k.strip()] = v.strip()
    return cmd, env


def cargo_scenario(srcdir, name, work, env, daemon):
    """Cargo mode (a `cargo` file in the scenario dir): each `vN/` is an
    overlay on one package tree in `work`, and the command is
    `lust check|run <root> --manifest-path Cargo.toml <flags>` from the
    `cargo` file or a `cmd` directive. The oracle is `cargo run` (dev
    profile, offline) of the same bin. Warm must equal cold; an accepted
    program must give cargo's exit code and stdout."""
    problems, notes = [], []
    cmd = open(os.path.join(srcdir, "cargo")).read().split()
    versions = sorted(f for f in os.listdir(srcdir) if f.startswith("v") and os.path.isdir(os.path.join(srcdir, f)))
    mtime = int(time.time())
    for v in versions:
        # Rising, and ahead of the clock: Cargo calls a file older than its last build fresh.
        mtime = max(mtime + 2, int(time.time()) + 2)
        vcmd, venv = apply_overlay(os.path.join(srcdir, v), work, mtime)
        cmd = vcmd or cmd
        root, flags = cmd[0], ["--manifest-path", "Cargo.toml"] + cmd[1:]
        wenv = dict(env, **venv)
        cenv = cold_of(wenv)
        tag = f"{name} {v}"
        wkc, _, wkerr = run([LUST, "check", root, *flags], work, wenv)
        problems.extend(warm_problems(tag, wkerr, v == versions[0]))
        ckc, _, ckerr = run([LUST, "check", root, *flags], work, cenv)
        if (wkc == 0) != (ckc == 0) or first_code(wkerr) != first_code(ckerr):
            problems.append(f"{tag}: warm check != cold check: warm rc={wkc} {wkerr[:200]!r}, cold rc={ckc} {ckerr[:200]!r}")
        wrc, wout, werr = run([LUST, "run", root, *flags, "-o", os.path.join(work, "w.bin")], work, wenv)
        problems.extend(warm_problems(tag, werr, False))
        crc, cout, cerr = run([LUST, "run", root, *flags, "-o", os.path.join(work, "c.bin")], work, cenv)
        if (wrc, wout, first_code(werr)) != (crc, cout, first_code(cerr)):
            problems.append(f"{tag}: warm != cold: warm=({wrc},{wout!r},{werr[:200]!r}) cold=({crc},{cout!r},{cerr[:200]!r})")
        # A compile reject, not a run that failed (a panic exits 101 after a build that passed).
        rejects = lambda rc, err: rc != 0 and (first_code(err) is not None or err.lstrip().startswith("error"))  # noqa: E731
        sel = [f for f in cmd[1:] if f != "--offline"]
        genv = dict(wenv, CARGO_TARGET_DIR=work + "-cargo-target")
        brc, _, berr = run(["cargo", "build", "-q", "--offline", *sel], work, genv, timeout=300)
        if brc != 0:
            for mode, rc, out, err in (("warm", wrc, wout, werr), ("cold", crc, cout, cerr)):
                if not rejects(rc, err):
                    problems.append(f"{tag}: {mode} accepts but cargo rejects; {mode} out={out!r} cargo={berr[-300:]!r}")
            continue
        grc, gout, _ = run(["cargo", "run", "-q", "--offline", *sel], work, genv, timeout=300)
        grc = 128 - grc if grc < 0 else grc  # killed by a signal: the shell's code, as lust's client gives it
        # A compile reject in both modes: lust is a subset.
        if rejects(wrc, werr) and rejects(crc, cerr):
            notes.append(f"{tag}: lust rejects what cargo accepts: {werr[:160]!r}")
            continue
        for mode, rc, out in (("warm", wrc, wout), ("cold", crc, cout)):
            if (rc, out) != (grc, gout):
                problems.append(f"{tag}: {mode} wrong: {mode}=({rc},{out!r},{(werr if mode == 'warm' else cerr)[:200]!r}) cargo=({grc},{gout!r})")
    return problems, notes


def daemon_env(srcdir):
    """`daemon_env` in a scenario dir: K=V lines for the daemon only (for
    example LUST_GUARD_HOLD_MS, which keeps guard checks in flight while
    later versions are written)."""
    path = os.path.join(srcdir, "daemon_env")
    if not os.path.exists(path):
        return {}
    pairs = (l.strip().partition("=") for l in open(path) if l.strip())
    return {k.strip(): v.strip() for k, _, v in pairs}


def expect_problems(tag, path, stderr):
    """The `vN.expect` lines the last `lust-stats:` line of a warm request breaks."""
    lines = [l for l in stderr.splitlines() if l.startswith("lust-stats:")]
    if not lines:
        return [f"{tag}: no lust-stats line in the warm run (set LUST_STATS=1 in daemon_env)"]
    stats = {k: int(v) for k, _, v in (kv.partition("=") for kv in lines[-1].split()[1:]) if v.isdigit()}
    out = []
    for line in open(path):
        m = re.fullmatch(r"\s*(\w+)\s*(>=|<=|==)\s*(\d+)\s*", line)
        if not m:
            continue
        k, op, n = m.group(1), m.group(2), int(m.group(3))
        got = stats.get(k)
        ok = got is not None and {">=": got >= n, "<=": got <= n, "==": got == n}[op]
        if not ok:
            out.append(f"{tag}: expected {k}{op}{n}, warm run has {k}={got}")
    return out


def guard_problems(log_path, name, wait=120):
    """Waits until every started background guard check is done, then
    returns one problem per `guard:` line (a warm answer that a fresh
    session disagreed with, or a panic), as the fuzz gate counts them."""
    t0 = time.time()
    while True:
        text = open(log_path).read()
        started = len(re.findall(r"^guard-check: start ", text, re.M))
        done = len(re.findall(r"^guard-check: done ", text, re.M))
        if done >= started or time.time() - t0 > wait:
            break
        time.sleep(0.05)
    problems = [f"{name}: {l.strip()[:300]}" for l in text.splitlines() if l.startswith("guard:")]
    if done < started:
        problems.append(f"{name}: {started - done} guard check(s) still running after {wait} s")
    return problems


def scenario(srcdir):
    """Returns (name, problems, notes). One temp dir and one daemon per
    scenario; versions are applied in order with strictly rising mtimes."""
    name = os.path.basename(srcdir.rstrip("/"))
    work = tempfile.mkdtemp(prefix=f"lust-scen-{name}-")
    sock = sock_path(work)
    env = dict(os.environ, LUST_DAEMON_SOCKET=sock, LUST_NO_CACHE="1")
    env.pop("LUST_NO_DAEMON", None)
    oracle_path = oracle_log.setup(env, work)
    log = open(os.path.join(work, "daemon.log"), "w")
    if RESTART:
        # Warm requests: one-shot processes on the scenario's front image.
        # The daemon-only env applies to them; cold ones stay plain.
        daemon = None
        env = dict(env, **daemon_env(srcdir), LUST_FRONT_STATE=os.path.join(work, "front-state"), LUST_FRONT_STATE_VERBOSE="1")
        env.pop("LUST_DAEMON_SOCKET", None)
        env["LUST_NO_DAEMON"] = "1"
    else:
        daemon = subprocess.Popen([LUST, "daemon"], cwd=work, env=dict(env, **daemon_env(srcdir)), stderr=log,
                                  start_new_session=True)
    main = os.path.join(work, "main.rs")
    problems, notes = [], []
    try:
        t0 = time.time()
        while daemon is not None and not os.path.exists(sock):
            if time.time() - t0 > 10 or daemon.poll() is not None:
                return name, [f"{name}: daemon start failed"], notes
            time.sleep(0.005)
        if os.path.exists(os.path.join(srcdir, "cargo")):
            problems, notes = cargo_scenario(srcdir, name, work, env, daemon)
            return name, problems, notes
        versions = sorted(
            f for f in os.listdir(srcdir)
            if f.startswith("v") and (f.endswith(".rs") or os.path.isdir(os.path.join(srcdir, f))))
        mtime = int(time.time())
        oracle_seen = 0
        for v in versions:
            mtime += 2
            vpath = os.path.join(srcdir, v)
            if os.path.isdir(vpath):
                new = set(os.listdir(vpath))
                for f in os.listdir(work):
                    if f.endswith(".rs") and f not in new:
                        os.remove(os.path.join(work, f))
                # A file with the same bytes is left alone, as an editor
                # would (guard01 edits only a module file).
                for f in new:
                    dst = os.path.join(work, f)
                    src = open(os.path.join(vpath, f), "rb").read()
                    if os.path.exists(dst) and open(dst, "rb").read() == src:
                        continue
                    with open(dst, "wb") as out:
                        out.write(src)
                    os.utime(dst, (mtime, mtime))
            else:
                shutil.copyfile(vpath, main)
                os.utime(main, (mtime, mtime))
            # `vN.env` next to `vN.rs`: K=V lines of client env for this version only.
            venv_path = vpath.removesuffix(".rs") + ".env"
            venv = {}
            if os.path.isfile(venv_path):
                pairs = (l.strip().partition("=") for l in open(venv_path) if l.strip())
                venv = {k.strip(): v.strip() for k, _, v in pairs}
            wenv = dict(env, **venv)
            # Check first, then run, like an agent loop: a check request
            # consumes the edit, so the build request after it sees an
            # unchanged file. Built state (statics, glue) must still be
            # current (this ordering caught a real statics bug).
            # `vN.ops` next to `vN.rs`: the warm requests for this version,
            # `check`, `run` or both (the default). The cold and rustc
            # answers are always computed; they do not touch the daemon.
            ops_path = vpath.removesuffix(".rs") + ".ops"
            ops = open(ops_path).read().split() if os.path.isfile(ops_path) else ["check", "run"]
            if "check" in ops:
                wkc, _, wkerr = run([LUST, "check", main], work, wenv)
            cold_env = cold_of(wenv)
            ckc, _, ckerr = run([LUST, "check", main], work, cold_env)
            if "check" not in ops:
                wkc, wkerr = ckc, ckerr
            # Same rule as the edit fuzzer: both accept, or both reject with the same first code.
            if (wkc == 0) != (ckc == 0) or first_code(wkerr) != first_code(ckerr):
                problems.append(f"{name} {v}: warm check != cold check: warm rc={wkc} {wkerr[:160]!r}, "
                                f"cold rc={ckc} {ckerr[:160]!r}")
            crc, cout, cerr = run([LUST, "run", main, "-o", os.path.join(work, "c.bin")], work, cold_env)
            if "run" in ops:
                wrc, wout, werr = run([LUST, "run", main, "-o", os.path.join(work, "w.bin")], work, wenv)
            else:
                wrc, wout, werr = crc, cout, cerr
            rrc, rout, _ = run(["rustc", "--edition", "2024",
                                "-C", "overflow-checks=on", main, "-o", os.path.join(work, "r.bin")], work, wenv)
            rustc_ok = rrc == 0
            if rustc_ok:
                rrc, rout, _ = run([os.path.join(work, "r.bin")], work, wenv)
            tag = f"{name} {v}"
            if "check" in ops:
                problems.extend(warm_problems(tag + " check", wkerr, v == versions[0]))
            if "run" in ops:
                problems.extend(warm_problems(tag + " run", werr, v == versions[0] and "check" not in ops))
            if (wrc == 0) != (crc == 0) or (wrc == 0 and wout != cout) or first_code(werr) != first_code(cerr):
                problems.append(
                    f"{tag}: warm != cold: warm=({wrc},{wout!r},{werr[:160]!r}) "
                    f"cold=({crc},{cout!r},{cerr[:160]!r})")
            if not rustc_ok:
                if wrc == 0:
                    problems.append(f"{tag}: warm accepts but rustc rejects; warm out={wout!r}")
                if crc == 0:
                    problems.append(f"{tag}: cold accepts but rustc rejects; cold out={cout!r}")
            elif wrc == 0 or crc == 0:
                if wrc == 0 and (wrc, wout) != (rrc, rout):
                    problems.append(f"{tag}: warm wrong: warm=({wrc},{wout!r}) rustc=({rrc},{rout!r})")
                if crc == 0 and (crc, cout) != (rrc, rout):
                    problems.append(f"{tag}: cold wrong: cold=({crc},{cout!r}) rustc=({rrc},{rout!r})")
            else:
                notes.append(f"{tag}: lust rejects valid Rust in both modes (unsupported today)")
            # `vN.expect` next to `vN`: `<stat><op><n>` lines (op >=, <=, ==) on the
            # warm run's `lust-stats:` counters (set LUST_STATS=1 in daemon_env),
            # or the warm check's when the version only checks; e.g.
            # `reemitted>=1` proves the cached-IR re-emit ran.
            expect_path = vpath.removesuffix(".rs") + ".expect"
            if os.path.isfile(expect_path):
                warm_err = werr if "run" in ops else wkerr if "check" in ops else ""
                problems.extend(expect_problems(tag, expect_path, warm_err))
            # Under LUST_CHECK_ITEMPATCH=1 the daemon logs each oracle failure
            # (and answers that request again from a fresh session).
            log.flush()
            with open(os.path.join(work, "daemon.log")) as f:
                hits = [l.strip() for l in f if "itempatch oracle:" in l or "resolve oracle:" in l]
            if len(hits) > oracle_seen:
                problems.append(f"{tag}: {hits[oracle_seen][:300]}")
                oracle_seen = len(hits)
    finally:
        # Every scenario, cargo mode too (the returned tuple holds this same list).
        log.flush()
        if daemon is not None:
            problems.extend(guard_problems(os.path.join(work, "daemon.log"), name))
            try:
                os.killpg(os.getpgid(daemon.pid), signal.SIGTERM)
            except Exception:
                daemon.terminate()
        log.close()
        # Under LUST_CHECK_ITEMPATCH=1 a scenario the oracle never checked
        # fails (the returned tuple holds this same list).
        problems.extend(oracle_log.problems(*oracle_log.collect(oracle_path), name))
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(work + "-cargo-target", ignore_errors=True)
        if sock_dir_of(sock, work):
            shutil.rmtree(sock_dir_of(sock, work), ignore_errors=True)
    return name, problems, notes


def rebuilt_binary():
    """A daemon must never answer a client of another lust build (tf-borrow
    08:10: an older daemon at an explicit socket kept answering after a
    rebuild). Copies lust and lust-core, warms a daemon from the copy, then
    replaces the copy's lust-core (a rebuild at the same path) and checks
    again: the answer must be cold and right, the old daemon gone."""
    name = "binary01_rebuilt_daemon"
    work = tempfile.mkdtemp(prefix=f"lust-scen-{name}-")
    problems, sock = [], None
    try:
        for b in ("lust", "lust-core"):
            shutil.copy2(os.path.join(os.path.dirname(LUST), b), os.path.join(work, b))
        lust = os.path.join(work, "lust")
        main = os.path.join(work, "main.rs")
        with open(main, "w") as f:
            f.write("fn main() {\n    let x: u8 = true;\n}\n")
        sock = sock_path(work)
        env = dict(os.environ, LUST_DAEMON_SOCKET=sock, LUST_NO_CACHE="1")
        env.pop("LUST_NO_DAEMON", None)
        log = open(os.path.join(work, "daemon.log"), "w")
        daemon = subprocess.Popen([lust, "daemon"], cwd=work, env=env, stderr=log, start_new_session=True)
        t0 = time.time()
        while not os.path.exists(env["LUST_DAEMON_SOCKET"]) and time.time() - t0 < 10:
            time.sleep(0.005)
        rc1, _, err1 = run([lust, "check", main], work, env)
        # The rebuild: a new file at the same path (new inode and mtime).
        core = os.path.join(work, "lust-core")
        shutil.copy2(core, core + ".new")
        os.replace(core + ".new", core)
        rc2, _, err2 = run([lust, "check", main], work, env)
        try:
            daemon.wait(timeout=10)
        except subprocess.TimeoutExpired:
            problems.append(f"{name}: the old daemon still runs after a client of another build")
            os.killpg(os.getpgid(daemon.pid), signal.SIGTERM)
        log.close()
        text = open(os.path.join(work, "daemon.log")).read()
        if (rc1, first_code(err1)) != (rc2, first_code(err2)) or rc1 == 0:
            problems.append(f"{name}: answers differ: before rc={rc1} {err1[:160]!r}, after rc={rc2} {err2[:160]!r}")
        if "another lust build" not in text:
            problems.append(f"{name}: the daemon did not see the other build: {text[-300:]!r}")
        if text.count("new session") != 1:
            problems.append(f"{name}: the old daemon compiled the rebuilt client's request")
    finally:
        shutil.rmtree(work, ignore_errors=True)
        if sock is not None and sock_dir_of(sock, work):
            shutil.rmtree(sock_dir_of(sock, work), ignore_errors=True)
    return name, problems, []


def main():
    if not os.path.exists(LUST):
        print(f"{LUST} missing; run: cargo build --release", file=sys.stderr)
        return 2
    names = sys.argv[1:]
    dirs = [os.path.join(SCEN, n) for n in names] if names else sorted(
        os.path.join(SCEN, d) for d in os.listdir(SCEN)
        if os.path.isdir(os.path.join(SCEN, d)))
    jobs = int(os.environ.get("DAEMON_SCENARIO_JOBS", "8"))
    with ProcessPoolExecutor(max_workers=jobs) as pool:
        results = list(pool.map(scenario, dirs))
    if not names and not RESTART:
        results.append(rebuilt_binary())
        dirs.append(None)
    results.sort()
    bad = 0
    notes = 0
    for name, problems, ns in results:
        notes += len(ns)
        if problems:
            bad += 1
            for p in problems:
                print(f"FAIL {p}")
        else:
            print(f"ok   {name}" + (f" ({len(ns)} unsupported version(s))" if ns else ""))
    print(f"daemon scenarios{' (restart)' if RESTART else ''}: {len(dirs) - bad}/{len(dirs)} ok"
          + (f", {notes} unsupported version(s)" if notes else ""))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
