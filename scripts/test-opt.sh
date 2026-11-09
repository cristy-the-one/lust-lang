#!/usr/bin/env bash
# `--opt` and `--opt=bg` (docs/opt.md): the optimised build keeps the sandbox,
# the dev overflow checks, and the bytes of a numeric program. A host with no
# gcc/clang/zig skips the compile on anything but Linux.
# Usage: scripts/test-opt.sh
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
# shellcheck source=scripts/gnu-tools.sh
source "$root/scripts/gnu-tools.sh"
export LUST_NO_DAEMON=1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pass=0
fail=0
report() {
  if [ "$1" = ok ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $2: $3"; fi
}

if [ ! -x "$lust" ] && [ -x "$lust.exe" ]; then lust=$lust.exe; fi
if [ ! -x "$lust" ]; then
  echo "opt: no $lust (build with cargo build --release)" >&2
  exit 1
fi

# Usage: --opt is not --release, and check refuses it.
out=$( "$lust" build --release --opt "$work/no.rs" 2>&1 ) || true
if grep -q "usage:" <<<"$out"; then report ok "usage-release"; else report bad "usage-release" "$out"; fi
out=$( "$lust" check --opt "$work/no.rs" 2>&1 ) || true
if grep -q "usage:" <<<"$out"; then report ok "usage-check"; else report bad "usage-check" "$out"; fi

# A plain build does not take the opt path.
cat >"$work/plain.rs" <<'EOF'
fn main() {
    println!("plain");
}
EOF
out=$(timeout 180 "$lust" build "$work/plain.rs" -o "$work/plain" 2>&1) || true
if grep -q "^opt:" <<<"$out"; then
  report bad "plain" "a default build printed an opt line: $out"
else
  report ok "plain"
fi

has_cc=0
if command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || command -v clang >/dev/null 2>&1 || command -v zig >/dev/null 2>&1; then
  has_cc=1
fi
if [ "$has_cc" -eq 0 ]; then
  case "$(uname -s)" in
    Linux)
      echo "opt: no C compiler on Linux" >&2
      exit 1
      ;;
    *)
      echo "opt: SKIP compile tests (no gcc, clang or zig)"
      echo "opt: $pass passed, $fail failed"
      [ "$fail" -eq 0 ]
      exit $?
      ;;
  esac
fi

# libm: constant and runtime sin/cos/atan2/sqrt/powi/powf. --opt must not
# constant-fold these with the compiler's own maths; tcc calls libm.
cat >"$work/libm.rs" <<'EOF'
fn main() {
    let s = (1.0_f64).sin();
    let c = (0.5_f64).cos();
    let a = (0.3_f64).atan2(0.4);
    let q = (2.0_f64).sqrt();
    let h = (3.0_f64).hypot(4.0);
    let p = (1.5_f64).powi(7);
    let f = (1.5_f64).powf(2.5);
    println!("{:x} {:x} {:x} {:x} {:x} {:x} {:x}", s.to_bits(), c.to_bits(), a.to_bits(), q.to_bits(), h.to_bits(), p.to_bits(), f.to_bits());
    let mut x = 0.13_f64;
    let mut acc = 0u64;
    let mut i = 0;
    while i < 16 {
        acc = acc ^ (x.sin().to_bits());
        acc = acc ^ (x.cos().to_bits());
        acc = acc ^ (x.atan2(0.25).to_bits());
        acc = acc ^ (x.sqrt().to_bits());
        acc = acc ^ (x.hypot(0.5).to_bits());
        acc = acc ^ (x.powi(5).to_bits());
        acc = acc ^ (x.powf(1.5).to_bits());
        x = x + 0.07;
        i = i + 1;
    }
    println!("{acc:x}");
}
EOF
if ! timeout 300 "$lust" build --sandbox "$work/libm.rs" -o "$work/libm-tcc" >"$work/libm-tcc.log" 2>&1; then
  report bad "libm-tcc" "$(tail -c 400 "$work/libm-tcc.log")"
else
  report ok "libm-tcc"
fi
if ! timeout 300 "$lust" build --sandbox --opt "$work/libm.rs" -o "$work/libm-opt" >"$work/libm-opt.log" 2>&1; then
  report bad "libm-opt" "$(tail -c 500 "$work/libm-opt.log")"
else
  report ok "libm-opt"
fi
if [ -x "$work/libm-tcc" ] && [ -x "$work/libm-opt" ]; then
  "$work/libm-tcc" >"$work/libm-tcc.out" 2>"$work/libm-tcc.err"
  st_tcc=$?
  "$work/libm-opt" >"$work/libm-opt.out" 2>"$work/libm-opt.err"
  st_opt=$?
  if [ "$st_tcc" -eq 0 ] && [ "$st_opt" -eq 0 ] && cmp -s "$work/libm-tcc.out" "$work/libm-opt.out"; then
    report ok "libm-bytes"
  else
    report bad "libm-bytes" "tcc($st_tcc)=$(cat "$work/libm-tcc.out") opt($st_opt)=$(cat "$work/libm-opt.out") err=$(cat "$work/libm-tcc.err" "$work/libm-opt.err")"
  fi
fi

# Numeric program: tcc and --opt print the same bits. The polynomial is the
# shape of a libm-free kernel (products and adds, no contraction wanted).
cat >"$work/num.rs" <<'EOF'
fn poly(z: f64) -> f64 {
    let r = 0.00833333333332249 + z * (-0.0001984126982985795 + z * 2.7557313707070068e-06);
    z * z * (-0.16666666666666632 + z * r)
}
fn main() {
    let mut x = 0.1_f64;
    let mut acc = 0.0_f64;
    let mut i = 0;
    while i < 200 {
        acc = acc + poly(x);
        x = x + 0.01;
        i = i + 1;
    }
    println!("{:x}", acc.to_bits());
    println!("{}", 40 + 2);
}
EOF
if ! timeout 300 "$lust" build --sandbox "$work/num.rs" -o "$work/num-tcc" >"$work/num-tcc.log" 2>&1; then
  report bad "num-tcc" "$(tail -c 400 "$work/num-tcc.log")"
else
  report ok "num-tcc"
fi
if ! timeout 300 "$lust" build --sandbox --opt "$work/num.rs" -o "$work/num-opt" >"$work/num-opt.log" 2>&1; then
  report bad "num-opt" "$(tail -c 500 "$work/num-opt.log")"
else
  if grep -q "^opt: optimised build" "$work/num-opt.log"; then report ok "num-opt"; else report bad "num-opt" "missing opt line: $(tail -c 300 "$work/num-opt.log")"; fi
fi
if [ -x "$work/num-tcc" ] && [ -x "$work/num-opt" ]; then
  "$work/num-tcc" >"$work/num-tcc.out" 2>"$work/num-tcc.err"
  st_tcc=$?
  "$work/num-opt" >"$work/num-opt.out" 2>"$work/num-opt.err"
  st_opt=$?
  if [ "$st_tcc" -eq 0 ] && [ "$st_opt" -eq 0 ] && cmp -s "$work/num-tcc.out" "$work/num-opt.out"; then
    report ok "num-bytes"
  else
    report bad "num-bytes" "tcc($st_tcc)=$(cat "$work/num-tcc.out") opt($st_opt)=$(cat "$work/num-opt.out") stderr=$(cat "$work/num-opt.err" "$work/num-tcc.err")"
  fi
fi

# Overflow checks stay on: both builds panic, neither prints a wrapped sum.
cat >"$work/ovf.rs" <<'EOF'
fn main() {
    let x: i32 = 2_000_000_000;
    println!("{}", x + x);
}
EOF
timeout 180 "$lust" build --sandbox "$work/ovf.rs" -o "$work/ovf-tcc" >"$work/ovf-tcc-build.log" 2>&1 || true
timeout 300 "$lust" build --sandbox --opt "$work/ovf.rs" -o "$work/ovf-opt" >"$work/ovf-opt-build.log" 2>&1 || true
if [ -x "$work/ovf-tcc" ] && [ -x "$work/ovf-opt" ]; then
  "$work/ovf-tcc" >"$work/ovf-tcc.out" 2>"$work/ovf-tcc.err" || st_tcc=$?
  st_tcc=${st_tcc:-0}
  "$work/ovf-opt" >"$work/ovf-opt.out" 2>"$work/ovf-opt.err" || st_opt=$?
  st_opt=${st_opt:-0}
  if [ "$st_tcc" -ne 0 ] && [ "$st_opt" -ne 0 ] && grep -q panicked "$work/ovf-tcc.err" && grep -q panicked "$work/ovf-opt.err"; then
    report ok "overflow"
  else
    report bad "overflow" "tcc=$st_tcc opt=$st_opt tccerr=$(head -c 200 "$work/ovf-tcc.err") opterr=$(head -c 200 "$work/ovf-opt.err")"
  fi
fi

# The runtime sandbox stop still fires under --opt. Path::exists is allowed
# at check and reaches an OS intrinsic the codegen stop refuses.
cat >"$work/sand.rs" <<'EOF'
fn main() {
    let p = std::path::Path::new("nope");
    println!("{}", p.exists());
}
EOF
timeout 180 "$lust" build --sandbox "$work/sand.rs" -o "$work/sand-tcc" >"$work/sand-tcc-build.log" 2>&1 || true
timeout 300 "$lust" build --sandbox --opt "$work/sand.rs" -o "$work/sand-opt" >"$work/sand-opt-build.log" 2>&1 || true
if [ -x "$work/sand-tcc" ] && [ -x "$work/sand-opt" ]; then
  "$work/sand-tcc" >"$work/sand-tcc.out" 2>"$work/sand-tcc.err" || st_tcc=$?
  st_tcc=${st_tcc:-0}
  "$work/sand-opt" >"$work/sand-opt.out" 2>"$work/sand-opt.err" || st_opt=$?
  st_opt=${st_opt:-0}
  if [ "$st_tcc" -eq 101 ] && [ "$st_opt" -eq 101 ] && grep -q "error\\[L0532\\]" "$work/sand-tcc.err" && grep -q "error\\[L0532\\]" "$work/sand-opt.err"; then
    report ok "sandbox-stop"
  else
    report bad "sandbox-stop" "tcc=$st_tcc opt=$st_opt tccerr=$(head -c 240 "$work/sand-tcc.err") opterr=$(head -c 240 "$work/sand-opt.err") tccbuild=$(tail -c 200 "$work/sand-tcc-build.log") optbuild=$(tail -c 200 "$work/sand-opt-build.log")"
  fi
fi

# Background: the daemon links tcc and starts --opt; once it publishes, the
# next run installs the cached binary. The cache is LUST_CACHE_DIR, not the
# project. The daemon is killed at the end, and that kills the background
# compile if it is still going.
cat >"$work/bg.rs" <<'EOF'
fn main() {
    let mut acc = 1.0_f64;
    let mut i = 0;
    while i < 50 {
        acc = acc * 1.0000001 + 0.125;
        i = i + 1;
    }
    println!("{:x}", acc.to_bits());
}
EOF
# The rest of this script turns the daemon off. This part wants one, in
# the work directory, so the background compile is the daemon's child.
bg_env() {
  env -u LUST_NO_DAEMON LUST_DAEMON_SOCKET="$work/bg.sock" LUST_CACHE_DIR="$work/cache" "$@"
}
cleanup_bg() {
  if [ -d /proc ]; then
    for d in /proc/[0-9]*; do
      [ -r "$d/environ" ] || continue
      text=$(cat "$d/environ" 2>/dev/null | tr '\0' '\n') || continue
      printf '%s\n' "$text" | grep -qx "LUST_DAEMON_SOCKET=$work/bg.sock" && kill "$(basename "$d")" 2>/dev/null || true
    done
  fi
}
trap 'cleanup_bg; rm -rf "$work"' EXIT
if bg_env timeout 180 "$lust" build --sandbox --opt=bg "$work/bg.rs" -o "$work/bg" >"$work/bg1.log" 2>&1; then
  if grep -q "opt: optimised build started in the background" "$work/bg1.log"; then
    report ok "bg-start"
  else
    report bad "bg-start" "$(tail -c 400 "$work/bg1.log")"
  fi
else
  report bad "bg-start" "$(tail -c 400 "$work/bg1.log")"
fi
if [ -x "$work/bg" ]; then
  "$work/bg" >"$work/bg1.out" 2>"$work/bg1.err" || true
fi
# The marker sits next to the cached binary. Poll the log and a second build.
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if bg_env timeout 180 "$lust" build --sandbox --opt=bg "$work/bg.rs" -o "$work/bg" >"$work/bg2.log" 2>&1; then
    if grep -q "opt: cached optimised build" "$work/bg2.log"; then
      ready=1
      break
    fi
  fi
  sleep 2
done
if [ "$ready" -eq 1 ]; then
  report ok "bg-cached"
  if [ -x "$work/bg" ]; then
    "$work/bg" >"$work/bg2.out" 2>"$work/bg2.err" || true
    if cmp -s "$work/bg1.out" "$work/bg2.out"; then
      report ok "bg-bytes"
    else
      report bad "bg-bytes" "first=$(cat "$work/bg1.out") cached=$(cat "$work/bg2.out")"
    fi
  fi
else
  log=""
  if [ -f "$work/cache/opt/opt-bg.log" ]; then log=$(tail -c 400 "$work/cache/opt/opt-bg.log"); fi
  report bad "bg-cached" "never published. last=$(tail -c 200 "$work/bg2.log") bglog=$log"
fi

echo "opt: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
