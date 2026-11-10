#!/usr/bin/env bash
# Time bench/opt-recipes/compute.rs under tcc and under --opt.
# Prints one row per run and the median ratio. Exits 1 when the checksums differ.
# Usage: scripts/opt-bench.sh [lust-binary]
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=${1:-$root/target/release/lust}
if [ ! -x "$lust" ] && [ -x "$lust.exe" ]; then lust=$lust.exe; fi
src=$root/bench/opt-recipes/compute.rs
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo "building tcc..."
"$lust" build --sandbox "$src" -o "$work/tcc"
echo "building opt..."
"$lust" build --sandbox --opt "$src" -o "$work/opt"
: >"$work/tcc.out"
: >"$work/opt.out"
echo "run tcc opt"
for i in 1 2 3 4 5; do
  "$work/tcc" | tee -a "$work/tcc.out"
  "$work/opt" | tee -a "$work/opt.out"
done
python3 - "$work/tcc.out" "$work/opt.out" <<'PY'
import statistics, sys
def load(path):
    rows = {"gear": [], "geom": []}
    bits = {}
    for line in open(path):
        p = line.split()
        kind = p[0]
        rows[kind].append(int(p[1]))
        key = tuple(p[2:])
        bits.setdefault(kind, key)
        if bits[kind] != key:
            raise SystemExit(f"checksum changed inside {path}: {bits[kind]} vs {key}")
    return rows, bits
tcc, tb = load(sys.argv[1])
opt, ob = load(sys.argv[2])
if tb != ob:
    raise SystemExit(f"checksums differ\n tcc {tb}\n opt {ob}")
print()
print(f"{'workload':<8} {'tcc median us':>14} {'opt median us':>14} {'opt/tcc':>8}")
for kind in ("gear", "geom"):
    tm = statistics.median(tcc[kind])
    om = statistics.median(opt[kind])
    print(f"{kind:<8} {tm:14.0f} {om:14.0f} {tm/om:8.2f}x")
print("checksums match")
PY
