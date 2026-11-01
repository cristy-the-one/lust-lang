#!/bin/sh
# Same-shim oracle for the OCCT first slice (docs/occt-shim.md).
# Skips when the cache is absent so CI stays green without Open CASCADE.
# Linux only here. Windows steps are in scripts/build-occt-shim.sh;
# that build was not run on this VM.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"
export LUST_TCC_HOME=${LUST_TCC_HOME:-$root/third_party/tcc}
unset SANITIZE || true

cache=${LUST_OCCT_CACHE:-${HOME:-/tmp}/.cache/lust/occt}
shim=${LUST_OCCT_SHIM:-}
if [ -z "$shim" ] && [ -f "$cache/current" ]; then
  shim=$(head -n 1 "$cache/current")
fi
if [ -z "$shim" ] || [ ! -f "$shim" ]; then
  echo "occt oracle: skip (no shim; run scripts/build-occt-shim.sh)"
  exit 0
fi

lust_bin=${LUST:-$root/target/release/lust}
if [ ! -x "$lust_bin" ]; then
  echo "occt oracle: lust binary not found at $lust_bin" >&2
  exit 1
fi

dir=$(CDPATH= cd -- "$(dirname "$shim")" && pwd)
shim="$dir/$(basename "$shim")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

py_time() {
  python3 -c 'import time; print("%.6f" % time.perf_counter())'
}

echo "occt oracle: rustc compile"
t0=$(py_time)
rustc --edition 2024 -C debuginfo=0 "$root/tests/occt/rustc_main.rs" -o "$work/rustc-occt" \
  -L "native=$dir" -l lust_occt -C link-arg=-Wl,-rpath,"$dir" \
  2>"$work/rustc.err" || { cat "$work/rustc.err" >&2; exit 1; }
t1=$(py_time)
python3 -c 'import sys; print("occt oracle: rustc compile %.3fs" % (float(sys.argv[2]) - float(sys.argv[1])))' "$t0" "$t1"

echo "occt oracle: lust compile"
t0=$(py_time)
LUST_NO_DAEMON=1 LUST_OCCT_SHIM="$shim" "$lust_bin" build --timings \
  --manifest-path "$root/tests/occt/lust/Cargo.toml" \
  "$root/tests/occt/lust/src/main.rs" -o "$work/lust-occt" \
  >"$work/lust.out" 2>"$work/lust.err" || { cat "$work/lust.out" "$work/lust.err" >&2; exit 1; }
t1=$(py_time)
python3 -c 'import sys; print("occt oracle: lust compile wall %.3fs" % (float(sys.argv[2]) - float(sys.argv[1])))' "$t0" "$t1"
# Compiler-reported total, when --timings prints one.
rg -n "total" "$work/lust.err" || true

"$work/rustc-occt" >"$work/rustc.txt"
LUST_OCCT_SHIM="$shim" "$work/lust-occt" >"$work/lust.txt"
if cmp -s "$work/rustc.txt" "$work/lust.txt"; then
  echo "occt oracle: bit-exact (status, counts, mesh bytes)"
  wc -c "$work/rustc.txt"
else
  echo "occt oracle: MISMATCH" >&2
  diff -u "$work/rustc.txt" "$work/lust.txt" | head -80 >&2
  exit 1
fi
