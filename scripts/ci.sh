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
# LUST_GATE_FILTER is a case-name substring, or a file of substrings,
# passed to difftest and the other runners that take a filter. A red
# step is recorded and the rest of the gate still runs.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

step() { echo; echo "== ci: $1 =="; }

# Windows Git Bash has no python3 on PATH (only python) and names the
# release binaries lust.exe. ASan/UBSan in scripts/difftest.sh is a Linux
# clang extra; it is off unless the caller asked for it.
case "$(uname -s)" in
  Linux) ;;
  *)
    if [ -z "${CI_SANITIZE+x}" ]; then CI_SANITIZE=0; fi
    ;;
esac
if ! command -v python3 >/dev/null 2>&1 && command -v python >/dev/null 2>&1; then
  _py_shim=$(mktemp -d)
  printf '#!/bin/sh\nexec python "$@"\n' >"$_py_shim/python3"
  chmod +x "$_py_shim/python3"
  export PATH="$_py_shim:$PATH"
fi

step "toolchain"
rustc --version
cargo --version
python3 --version

step "build"
# Git for Windows ships usr/bin/link.exe, which is not the MSVC linker.
# cargo invokes `link.exe` for the msvc target, so the VC tools directory
# has to come first or the build script fails immediately.
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
    if [ -x "$vswhere" ]; then
      vs_install=$("$vswhere" -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | tr -d '\r')
      if [ -n "$vs_install" ]; then
        vs_msys=$(cygpath -u "$vs_install")
        msvc_link=$(find "$vs_msys/VC/Tools/MSVC" -path '*/bin/Hostx64/x64/link.exe' -type f 2>/dev/null | head -1)
        if [ -n "$msvc_link" ]; then
          export PATH="$(dirname "$msvc_link"):$PATH"
          echo "msvc link: $msvc_link"
        fi
      fi
    fi
    ;;
esac
if [ -n "${LUST_TCC_HOME:-}" ]; then
  echo "tcc: LUST_TCC_HOME=$LUST_TCC_HOME"
elif [ -f third_party/tcc/lib/libtcc.a ] || [ -f third_party/tcc/lib/tcc.lib ]; then
  export LUST_TCC_HOME=$root/third_party/tcc
  echo "tcc: $LUST_TCC_HOME (already built)"
else
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) powershell.exe -NoProfile -File scripts/build-tcc.ps1 ;;
    *) scripts/build-tcc.sh ;;
  esac
  export LUST_TCC_HOME=$root/third_party/tcc
fi
cargo build --release
# Test scripts name `target/release/lust`. Cargo on Windows writes lust.exe.
# Git bash treats `lust` as `lust.exe`, so `cp lust.exe lust` is a same-file copy.
if [ -f target/release/lust.exe ]; then
  powershell.exe -NoProfile -Command "Copy-Item -LiteralPath 'target\release\lust.exe' -Destination 'target\release\lust' -Force; Copy-Item -LiteralPath 'target\release\lust-core.exe' -Destination 'target\release\lust-core' -Force"
  chmod +x target/release/lust.exe target/release/lust-core.exe || true
fi

step "diagnostic code registry"
scripts/check-codes.sh

step "lust-port tool tests"
scripts/test-lust-port.sh

# LUST_GATE_FILTER (a substring, or a file of substrings) limits difftest
# and the other runners that take a filter. Empty runs the full gate.
# LUST_FAIL_LOG collects FAIL lines for the Actions annotation and the PR note.
gate_filter=${LUST_GATE_FILTER:-}
export LUST_FAIL_LOG=$root/ci-failures.txt
: > "$LUST_FAIL_LOG"
failed=()
# Run a step and remember a failure. set -e would stop at the first red
# step, so each later suite (macros, kernels, crates, daemon, fuzz) never
# ran on Windows. The build above still stops the gate: nothing else can
# run without the compiler.
run_step() {
  local name=$1
  shift
  step "$name"
  # The step's own output also goes to a file, so a failure can carry its
  # evidence into ci-failures.txt (the PR comment and the artifact): the raw
  # job log is not readable by every agent that works on this branch.
  local out=$root/ci-step.log
  set +e
  "$@" 2>&1 | tee "$out"
  local st=${PIPESTATUS[0]}
  set -e
  if [ "$st" -ne 0 ]; then
    failed+=("$name")
    echo "ci: step failed: $name (exit $st)"
    {
      printf 'STEP %s exit %s\n' "$name" "$st"
      echo "--- $name: lines that look like failures (first 40)"
      grep -a -E -i 'fail|error|mismatch|differ|bad request|panicked|not found|cannot|timed? ?out|expected' "$out" | grep -a -v -E '^(ok|PASS) ' | head -n 40
      echo "--- $name: last 25 lines"
      tail -n 25 "$out"
      echo
    } >> "$LUST_FAIL_LOG"
  fi
}
macro_corpus() {
  local st=0
  scripts/test-macros.sh ${gate_filter:+"$gate_filter"} || st=1
  scripts/test-macro-deps.sh || st=1
  return "$st"
}
kernel_suite() { scripts/test-kernels.sh ${gate_filter:+"$gate_filter"}; }
crate_suite() { scripts/test-crates.sh ${gate_filter:+"$gate_filter"}; }
fuzz_suite() {
  local f found="" st=0
  for f in scripts/fuzz-parse.sh scripts/fuzz-edit.sh scripts/fuzz.sh scripts/fuzz-*.sh fuzz/run.sh; do
    [ -e "$f" ] || continue
    case " $found " in *" $f "*) continue ;; esac
    found="$found $f"
    echo "running $f (FUZZ_SECONDS=${FUZZ_SECONDS:-20})"
    FUZZ_SECONDS=${FUZZ_SECONDS:-20} bash "$f" || st=1
  done
  if [ -z "$found" ]; then
    echo "no fuzz scripts present yet (scripts/fuzz-*.sh); skipping"
  fi
  return "$st"
}
bench_suite() {
  local smoke
  smoke=$(mktemp)
  REPS=1 bench/compare.sh plain 40 | tee "$smoke"
  grep -q "debuginfo=0" "$smoke"
  grep -q "debuginfo=2" "$smoke"
  grep -q "same output: yes" "$smoke"
  rm -f "$smoke"
  python3 bench/run.py --quick --skip-edit-loop --json "$(mktemp -u).json"
  python3 -c "import json; d = json.load(open('bench/tsrust/baseline.json')); assert d['rows'], 'no rows'"
}

# base64, sha2, libm, flate2, quick-xml, and zip are lust source adapters.
# The oracle is the real crate, so the registry has to hold those exact
# versions before the offline adapter run. LUST_STRICT makes a mismatched
# byte or bit pattern fail the step. flate2 is here because zip's deflate
# bytes come from its encoder.
adapter_oracles() {
  local name proj st=0
  for name in base64 sha2 libm flate2 quick-xml zip; do
    proj=$(mktemp -d)
    mkdir -p "$proj/src"
    echo 'fn main() {}' >"$proj/src/main.rs"
    {
      printf '[package]\nname = "prefetch-%s"\nversion = "0.0.0"\nedition = "2024"\n\n[dependencies]\n' "$name"
      cat "tests/adapters/crates/$name/deps.toml"
      printf '\n[workspace]\n'
    } >"$proj/Cargo.toml"
    cargo fetch --manifest-path "$proj/Cargo.toml" || return 1
    rm -rf "$proj"
    LUST="$root/target/release/lust" LUST_STRICT=1 scripts/test-adapters.sh "$name" || st=1
  done
  return "$st"
}
run_step "base64, sha2, libm, flate2, quick-xml, and zip adapters" adapter_oracles
# The OCCT oracle skips when the shim cache is absent. CI does not install OCCT.
run_step "occt shim oracle (skip without the cache)" scripts/occt-oracle.sh
run_step difftest scripts/difftest.sh ${gate_filter:+"$gate_filter"}
run_step "macro corpus" macro_corpus
run_step "ts-rust kernels and compat fixtures" kernel_suite
if [ -e scripts/test-client.sh ]; then
  run_step "client tests" scripts/test-client.sh
fi
run_step "crate fixture tests" crate_suite
run_step "daemon edit scenarios (warm vs cold vs rustc)" python3 scripts/test-daemon-scenarios.py
if [ -e scripts/test-slots.py ]; then
  run_step "slot identity tests" python3 scripts/test-slots.py
fi
if [ "${CI_SANITIZE:-1}" = 1 ]; then
  run_step "difftest (sanitize)" env SANITIZE=1 scripts/difftest.sh ${gate_filter:+"$gate_filter"}
else
  step "difftest (sanitize) SKIPPED (CI_SANITIZE=0)"
fi
run_step fuzz fuzz_suite
if [ "${CI_BENCH:-1}" = 1 ]; then
  run_step "benchmark smoke" bench_suite
else
  step "benchmark smoke SKIPPED (CI_BENCH=0)"
fi

step "done"
if [ ${#failed[@]} -ne 0 ]; then
  echo "ci: ${#failed[@]} step(s) failed"
  printf 'FAILED: %s\n' "${failed[@]}"
  exit 1
fi
echo "ci: all steps passed"
