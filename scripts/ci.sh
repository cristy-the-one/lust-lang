#!/usr/bin/env bash
# The full local gate, same on a dev box and in GitHub Actions:
#   1. build (pinned tcc if needed, then cargo build --release)
#   2. differential tests against rustc, plus the macro corpus (rustc mode)
#   3. differential tests under ASan/UBSan (SANITIZE=1)
#   4. fuzzers, when present (scripts/fuzz-*.sh; other work adds them)
#   5. benchmark smoke: compare.sh at a tiny size + run.py --quick
#
# Env knobs: CI_SANITIZE=0 skips step 3, CI_BENCH=0 skips step 5,
# FUZZ_SECONDS (default 20) is passed to each fuzzer script.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

step() { echo; echo "== ci: $1 =="; }

step "toolchain"
rustc --version
cargo --version
python3 --version

step "build"
if [ -n "${LUST_TCC_HOME:-}" ]; then
  echo "tcc: LUST_TCC_HOME=$LUST_TCC_HOME"
elif [ -f third_party/tcc/lib/libtcc.a ]; then
  export LUST_TCC_HOME=$root/third_party/tcc
  echo "tcc: $LUST_TCC_HOME (already built)"
else
  scripts/build-tcc.sh
  export LUST_TCC_HOME=$root/third_party/tcc
fi
cargo build --release

step "diagnostic code registry"
scripts/check-codes.sh

step "lust-port tool tests"
scripts/test-lust-port.sh

step "difftest"
scripts/difftest.sh

step "macro corpus"
scripts/test-macros.sh
scripts/test-macro-deps.sh

step "ts-rust kernels and compat fixtures"
scripts/test-kernels.sh

if [ -e scripts/test-client.sh ]; then
  step "client tests"
  scripts/test-client.sh
fi

step "crate fixture tests"
scripts/test-crates.sh

step "daemon edit scenarios (warm vs cold vs rustc)"
python3 scripts/test-daemon-scenarios.py

if [ -e scripts/test-slots.py ]; then
  step "slot identity tests"
  python3 scripts/test-slots.py
fi

if [ "${CI_SANITIZE:-1}" = 1 ]; then
  step "difftest (sanitize)"
  SANITIZE=1 scripts/difftest.sh
else
  step "difftest (sanitize) SKIPPED (CI_SANITIZE=0)"
fi

step "fuzz"
found_fuzz=""
for f in scripts/fuzz-parse.sh scripts/fuzz-edit.sh scripts/fuzz.sh scripts/fuzz-*.sh fuzz/run.sh; do
  [ -e "$f" ] || continue
  case " $found_fuzz " in *" $f "*) continue ;; esac
  found_fuzz="$found_fuzz $f"
  echo "running $f (FUZZ_SECONDS=${FUZZ_SECONDS:-20})"
  FUZZ_SECONDS=${FUZZ_SECONDS:-20} bash "$f"
done
if [ -z "$found_fuzz" ]; then
  echo "no fuzz scripts present yet (scripts/fuzz-*.sh); skipping"
fi

if [ "${CI_BENCH:-1}" = 1 ]; then
  step "benchmark smoke"
  # Row-shape regression check: compare.sh must pair debuginfo=0 rows with a
  # debuginfo=2 counterpart and confirm identical program output.
  smoke=$(mktemp)
  REPS=1 bench/compare.sh plain 40 | tee "$smoke"
  grep -q "debuginfo=0" "$smoke"
  grep -q "debuginfo=2" "$smoke"
  grep -q "same output: yes" "$smoke"
  rm -f "$smoke"
  python3 bench/run.py --quick --skip-edit-loop --json "$(mktemp -u).json"
  # The checked-in ts-rust scoreboard must stay valid JSON with rows.
  python3 -c "import json; d = json.load(open('bench/tsrust/baseline.json')); assert d['rows'], 'no rows'"
else
  step "benchmark smoke SKIPPED (CI_BENCH=0)"
fi

step "done"
echo "ci: all steps passed"
