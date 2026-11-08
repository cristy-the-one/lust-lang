#!/usr/bin/env bash
# lust watch check. Builds tools/lust-watch (a separate package: notify is not
# linked into lust or lust-core), runs its tests against target/release/lust,
# and checks that a normal lust command does not need the watcher binary.
# Usage: scripts/test-watch.sh
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
# shellcheck source=scripts/os-sh.sh
source "$root/scripts/os-sh.sh"
# shellcheck source=scripts/gnu-tools.sh
source "$root/scripts/gnu-tools.sh"

pass=0
fail=0
ok() { pass=$((pass + 1)); echo "ok   $1"; }
bad() { fail=$((fail + 1)); echo "FAIL $1: $2"; }

lust=$root/target/release/lust
if [ -f "$lust.exe" ]; then lust=$lust.exe; fi
if [ ! -f "$lust" ] && [ ! -x "$lust" ]; then
  echo "error: $lust not built. Run cargo build --release first."
  exit 2
fi

echo "test-watch: build lust-watch"
if ! cargo build --release --manifest-path "$root/tools/lust-watch/Cargo.toml" --bin lust-watch; then
  echo "FAIL test-watch: lust-watch did not build"
  exit 1
fi
rel=$root/tools/lust-watch/target/release
mkdir -p "$root/target/release"
if [ -f "$rel/lust-watch.exe" ]; then
  cp "$rel/lust-watch.exe" "$root/target/release/lust-watch.exe"
  cp "$rel/lust-watch.exe" "$root/target/release/lust-watch"
else
  cp "$rel/lust-watch" "$root/target/release/lust-watch"
fi
ok "lust-watch is next to lust"

# Permissive licenses only. CC0, ISC, MIT, Apache-2.0, and Unlicense are
# in the tree today. GPL and LGPL are not.
lic=$(cargo tree --manifest-path "$root/tools/lust-watch/Cargo.toml" -f '{l}' || true)
if printf '%s\n' "$lic" | grep -E 'GPL|LGPL' >/dev/null; then
  bad "watcher license" "GPL/LGPL in the tree"
else
  ok "watcher license is permissive"
fi

if command -v nm >/dev/null 2>&1; then
  if nm -a "$root/target/release/lust" ${root}/target/release/lust-core "$root/target/release/lust.exe" "$root/target/release/lust-core.exe" 2>/dev/null | grep -E 'notify[0-9_]|notify::|inotify_init' >/dev/null; then
    bad "watcher not linked" "notify is in lust or lust-core"
  else
    ok "notify is not in lust or lust-core"
  fi
else
  ok "nm not on PATH; skipped the symbol check"
fi

work=$(mktemp -d)
cleanup() {
  cd "$root" || true
  local pid
  pid=$(cat "$work"/.lust-daemon-*.sock.lock 2>/dev/null || true)
  if [ -n "${pid:-}" ]; then
    rm -f "$work"/.lust-daemon-*.sock 2>/dev/null || true
    i=0
    while pid_alive "$pid" && [ "$i" -lt 30 ]; do sleep 0.1; i=$((i + 1)); done
    pid_alive "$pid" && kill_hard "$pid" || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT
printf 'fn main() {}\n' >"$work/hello.rs"
# Checks and the watcher use this directory as their cwd, so the daemon
# socket is created here and not in the repository.
cd "$work"

# Ordinary commands do not start the watcher. Hiding the binary must not
# change --version or check. `lust watch` is the command that needs it.
aside=$root/target/release/lust-watch.aside
if [ -f "$root/target/release/lust-watch.exe" ]; then
  mv "$root/target/release/lust-watch.exe" "$aside.exe"
  mv "$root/target/release/lust-watch" "$aside" 2>/dev/null || true
else
  mv "$root/target/release/lust-watch" "$aside"
fi
if "$lust" --version >/dev/null; then ok "lust --version without lust-watch"; else bad "lust --version without lust-watch" "failed"; fi
if "$lust" check "$work/hello.rs" >/dev/null; then ok "lust check without lust-watch"; else bad "lust check without lust-watch" "failed"; fi
if "$lust" watch check "$work/hello.rs" >/dev/null 2>"$work/watch.err"; then
  bad "lust watch without its binary" "it ran"
else
  ok "lust watch reports a missing binary"
fi
if [ -f "$aside.exe" ]; then
  mv "$aside.exe" "$root/target/release/lust-watch.exe"
  cp "$root/target/release/lust-watch.exe" "$root/target/release/lust-watch"
else
  mv "$aside" "$root/target/release/lust-watch"
fi

# One real watch through the client: error, then a fix, then Ctrl-C.
# The Rust tests cover coalescing. This checks the exec from `lust`.
printf 'fn main() { let x: i32 = "no"; }\n' >"$work/hello.rs"
LUST_WATCH_DEBOUNCE_MS=30 LUST_DAEMON_IDLE=60 "$lust" watch check hello.rs >"$work/out" 2>"$work/err" &
wpid=$!
i=0
while [ "$i" -lt 50 ]; do
  if grep -q 'watch-status:' "$work/err" 2>/dev/null; then break; fi
  if ! pid_alive "$wpid" && ! kill -0 "$wpid" 2>/dev/null; then break; fi
  sleep 0.1
  i=$((i + 1))
done
if grep -q 'L0110' "$work/err" && grep -q 'watch-status: current code=1' "$work/err"; then
  ok "client watch reports the type error as current"
else
  bad "client watch reports the type error as current" "$(tail -20 "$work/err")"
fi
printf 'fn main() {}\n' >"$work/hello.rs"
i=0
while [ "$i" -lt 50 ]; do
  if grep -c 'watch-status:' "$work/err" 2>/dev/null | grep -q '^2$'; then break; fi
  # grep -c with a count of 2 or more
  n=$(grep -c 'watch-status:' "$work/err" 2>/dev/null || true)
  if [ "${n:-0}" -ge 2 ]; then break; fi
  sleep 0.1
  i=$((i + 1))
done
if grep -q 'watch-status: current code=0' "$work/err"; then
  ok "client watch clears the error"
else
  bad "client watch clears the error" "$(tail -20 "$work/err")"
fi
# Ctrl-C. On Windows Git bash, kill -INT is not the console handler; the
# Rust tests send CTRL_BREAK. Here, SIGTERM is the same handler on Unix,
# and on Windows we remove the socket after the process is stopped below.
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    winpid=$wpid
    if [ -r "/proc/$wpid/winpid" ]; then winpid=$(cat "/proc/$wpid/winpid"); fi
    taskkill //T //PID "$winpid" >/dev/null 2>&1 || true
    ;;
  *) kill -INT "$wpid" 2>/dev/null || true ;;
esac
i=0
while pid_alive "$wpid" || kill -0 "$wpid" 2>/dev/null; do
  i=$((i + 1))
  if [ "$i" -gt 30 ]; then break; fi
  sleep 0.1
done
if pid_alive "$wpid" || kill -0 "$wpid" 2>/dev/null; then
  kill_hard "$wpid" || true
  bad "watch stops on interrupt" "still running"
else
  ok "watch stops on interrupt"
fi
wait "$wpid" 2>/dev/null || true

cd "$root"
echo "test-watch: cargo test"
# Debug rebuild plus the tests. A stuck lust-watch used to hold the step
# until the job was killed; 124 is that bound, and the timing line below
# still runs.
test_timeout=${WATCH_TEST_TIMEOUT:-600}
if timeout "$test_timeout" env LUST="$lust" LUST_WATCH_REQUIRE_COMPILER=1 cargo test --manifest-path "$root/tools/lust-watch/Cargo.toml" -- --test-threads=1 --nocapture; then
  ok "lust-watch tests"
else
  st=$?
  if [ "$st" -eq 124 ]; then
    bad "lust-watch tests" "timed out after ${test_timeout}s"
  else
    bad "lust-watch tests" "cargo test failed"
  fi
fi

# Startup and a one-file check, so a later run can compare. The watcher is
# not on this path. Numbers are milliseconds, mean of the loop.
time_ms() {
  local n=$1
  shift
  local t0 t1
  t0=$(date +%s%N)
  local i
  for i in $(seq 1 "$n"); do "$@" >/dev/null; done
  t1=$(date +%s%N)
  python3 -c "print(round(($t1-$t0)/1e6/$n, 3))"
}
ver=$(time_ms 20 "$lust" --version)
cold=$(LUST_NO_DAEMON=1 LUST_NO_CACHE=1 time_ms 5 "$lust" check "$work/hello.rs")
unset LUST_NO_DAEMON LUST_NO_CACHE
(cd "$work" && "$lust" check hello.rs >/dev/null 2>&1) || true
warm=$(cd "$work" && time_ms 10 "$lust" check hello.rs)
echo "WATCH_REGRESSION version_ms=$ver cold_ms=$cold warm_ms=$warm"
ok "recorded non-watch timings"

echo "test-watch: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
