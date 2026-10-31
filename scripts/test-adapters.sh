#!/usr/bin/env bash
# Adapter corpus runner (ARCH 9.3 runner table). Wave A1 part:
# the layout corpus in tests/adapters/layout/ (ARCH 4.4 gate),
# plus the const initializer corpus in tests/consts/ (ARCH
# 2.13), whose layout queries come from the layout engine.
# Wave B adds the adapter crates in tests/adapters/crates/ (ARCH
# 3.6), each against its real crate (see corpus_crate_dir below).
# The filter also matches a crate dir name: `test-adapters.sh memchr`.
# A filter that is exactly a crate directory runs only that crate.
# Otherwise `zip` would also build flate2's `gzip_*` cases, whose
# default-feature resolve needs zlib-rs.
# Usage: scripts/test-adapters.sh [filter]
# Modes and knobs: see scripts/test-corpus-lib.sh.
set -uo pipefail
# shellcheck source=scripts/test-corpus-lib.sh
source "$(dirname "$0")/test-corpus-lib.sh"

corpus_init "${1:-}"

# The committed generated corpus must be the generator's
# current output (ARCH 4.4: grammar, generator, and engine
# grow together).
if [ -f "$corpus_root/tests/adapters/layout/generate.py" ]; then
  if timeout "$case_timeout" python3 "$corpus_root/tests/adapters/layout/generate.py" >"$corpus_work/generate.out" 2>&1; then
    corpus_report ok generate-check
  else
    corpus_report bad generate-check "$(cat "$corpus_work/generate.out")"
  fi
else
  corpus_report bad generate-check "tests/adapters/layout/generate.py is missing"
fi
corpus_pass_dir tests/adapters/layout/pass
corpus_fail_dir tests/adapters/layout/fail
corpus_pass_dir tests/consts/pass
corpus_fail_dir tests/consts/fail
corpus_pass_dir tests/consts/later/pass
corpus_fail_dir tests/consts/later/fail

# Adapter crates (ARCH 3.6, package W-DEPS). One dir per adapter in
# tests/adapters/crates/<package>/: deps.toml holds the [dependencies]
# lines that pin the upstream version, and each <case>.rs is a bin with
# its <case>.stdout. The real crate is the oracle: Cargo builds every
# case against it (offline, from the local registry cache), and its
# output must equal the pinned .stdout. LUST mode builds each case with
# lust, which swaps the adapter in, and compares stdout and exit status.
# A case headed `// lust-reject: L####` is valid Rust that lust must
# reject with that code (an API or layout the adapter does not
# implement); Cargo must still build it. A case in the dir's `fail/`
# headed `// expect: L####` is invalid Rust against the real crate (an
# adapter shape upstream lacks): Cargo must fail to compile it and lust
# must reject it with that code. A dir may pin other features of the
# same package (deps.toml), e.g. `crates/signal-hook-nodefault/`.
corpus_crate_dir() {
  # $1 = dir relative to the repo root
  corpus_require_dir "$1" || return 0
  local dir=$corpus_root/$1 pkg proj f name
  pkg=$(basename "$dir")
  proj=$corpus_work/crate-$pkg
  if [ ! -f "$dir/deps.toml" ]; then
    corpus_report bad "$1" "deps.toml is missing"
    return
  fi
  mkdir -p "$proj/src/bin"
  {
    printf '[package]\nname = "adapter-%s"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n\n[dependencies]\n' "$pkg"
    cat "$dir/deps.toml"
    printf '\n[workspace]\n'
  } >"$proj/Cargo.toml"
  local cases=()
  for f in "$dir"/*.rs; do
    name=$(basename "$f" .rs)
    [[ -n "$corpus_filter" && "$name" != *"$corpus_filter"* && "$pkg" != *"$corpus_filter"* ]] && continue
    cp "$f" "$proj/src/bin/$name.rs"
    cases+=("$name")
  done
  if [ ${#cases[@]} -gt 0 ]; then
    # One Cargo build for every case, so the dependency compiles once.
    if ! (cd "$proj" && CARGO_NET_OFFLINE=true timeout 600 cargo build -q --bins --target-dir "$proj/target") >"$proj/cargo.out" 2>&1; then
      corpus_report bad "$1" "cargo build against the real crate failed: $(grep -m3 -E '^error' "$proj/cargo.out" | tr '\n' ' ')"
      return
    fi
    for name in "${cases[@]}"; do
      corpus_cases=$((corpus_cases + 1))
      corpus_crate_one "$dir" "$proj" "$name"
    done
  fi
  for f in "$dir"/fail/*.rs; do
    [ -f "$f" ] || continue
    name=$(basename "$f" .rs)
    [[ -n "$corpus_filter" && "$name" != *"$corpus_filter"* && "$pkg" != *"$corpus_filter"* ]] && continue
    corpus_cases=$((corpus_cases + 1))
    corpus_crate_fail_one "$f" "$proj" "$pkg" "$name"
  done
}

# One `fail/` case: Cargo must reject it in the case's own crate (not
# in the dependency), and lust must reject it with the expected code.
corpus_crate_fail_one() {
  local f=$1 proj=$2 pkg=$3 name=$4 code
  code=$(grep -m1 '// expect:' "$f" | sed 's|.*expect: *||; s/[[:space:]]*$//')
  if [ -z "$code" ]; then
    corpus_report bad "$pkg/$name" "fail case has no // expect: L#### line"
    return
  fi
  cp "$f" "$proj/src/bin/$name.rs"
  (cd "$proj" && CARGO_NET_OFFLINE=true timeout 600 cargo build -q --bin "$name" --target-dir "$proj/target") >"$proj/$name.cargo.out" 2>&1
  local cst=$?
  if [ $cst -eq 0 ]; then
    rm -f "$proj/src/bin/$name.rs"
    corpus_report bad "$pkg/$name" "the real crate builds this fail case"
    return
  fi
  if ! grep -q "could not compile \`adapter-$pkg\`" "$proj/$name.cargo.out"; then
    rm -f "$proj/src/bin/$name.rs"
    corpus_report bad "$pkg/$name" "cargo failed outside the case: $(grep -m2 -E '^error' "$proj/$name.cargo.out" | tr '\n' ' ')"
    return
  fi
  corpus_report ok "$pkg/$name"
  if [ -n "${LUST:-}" ]; then
    lust_cc env CARGO_NET_OFFLINE=true "$LUST" build "$proj/src/bin/$name.rs" -o "$proj/$name.lust" --manifest-path "$proj/Cargo.toml" \
      --bin "$name" >/dev/null 2>"$proj/$name.lust.err"
    local st=$?
    if is_timeout "$st"; then
      corpus_lust_note hang "$pkg/$name" "lust build timed out alone at ${retry_timeout}s (first at ${case_timeout}s)"
    elif [ "$st" -eq 0 ]; then
      corpus_slow_note "$pkg/$name" "lust build"
      corpus_lust_note wrong "$pkg/$name" "lust accepted a case the real crate rejects (want $code)"
    elif grep -q "$code" "$proj/$name.lust.err"; then
      corpus_slow_note "$pkg/$name" "lust build"
      corpus_lust_note match "$pkg/$name" ""
    else
      corpus_slow_note "$pkg/$name" "lust build"
      corpus_lust_note wrong "$pkg/$name" "lust rejected without $code: $(head -2 "$proj/$name.lust.err" | tr '\n' ' ')"
    fi
  fi
  rm -f "$proj/src/bin/$name.rs"
}

corpus_crate_one() {
  local dir=$1 proj=$2 name=$3
  # Reports name the case as <crate>/<case>: two crates can both have a `handle` case.
  local pkg=${dir##*/}
  local expected=$dir/$name.stdout reject
  reject=$(grep -m1 'lust-reject:' "$dir/$name.rs" | sed 's|.*lust-reject: *||; s/[[:space:]]*$//')
  if [ -z "$reject" ] && [ ! -f "$expected" ]; then
    corpus_report bad "$pkg/$name" "missing expected stdout file ${expected##*/}"
    return
  fi
  if [ -z "$reject" ]; then
    timeout "$run_timeout" "$proj/target/debug/$name" >"$proj/$name.out" 2>"$proj/$name.err"
    local status=$?
    if [ $status -ne 0 ]; then
      corpus_report bad "$pkg/$name" "the Cargo build exited with status $status"
      return
    fi
    if ! cmp -s "$expected" "$proj/$name.out"; then
      corpus_report bad "$pkg/$name" "stdout of the real crate differs from ${expected##*/}:"
      diff "$expected" "$proj/$name.out" | head -10
      return
    fi
  fi
  corpus_report ok "$pkg/$name"
  [ -n "${LUST:-}" ] || return
  local build=(build "$proj/src/bin/$name.rs" -o "$proj/$name.lust" --manifest-path "$proj/Cargo.toml" --bin "$name")
  if [ -n "$reject" ]; then
    lust_cc env CARGO_NET_OFFLINE=true "$LUST" "${build[@]}" >/dev/null 2>"$proj/$name.lust.err"
    local st=$?
    if is_timeout "$st"; then
      # A timeout is never the expected reject.
      corpus_lust_note hang "$pkg/$name" "lust build timed out alone at ${retry_timeout}s (first at ${case_timeout}s)"
    elif [ "$st" -eq 0 ]; then
      corpus_slow_note "$pkg/$name" "lust build"
      corpus_lust_note wrong "$pkg/$name" "lust accepted a case it must reject with $reject"
    elif grep -q "$reject" "$proj/$name.lust.err"; then
      corpus_slow_note "$pkg/$name" "lust build"
      corpus_lust_note match "$pkg/$name" ""
    else
      corpus_slow_note "$pkg/$name" "lust build"
      corpus_lust_note wrong "$pkg/$name" "lust rejected without $reject: $(head -2 "$proj/$name.lust.err" | tr '\n' ' ')"
    fi
    return
  fi
  lust_cc env CARGO_NET_OFFLINE=true "$LUST" "${build[@]}" 2>"$proj/$name.lust.err"
  local st=$?
  if is_timeout "$st"; then
    corpus_lust_note hang "$pkg/$name" "lust build timed out alone at ${retry_timeout}s (first at ${case_timeout}s)"
    return
  fi
  corpus_slow_note "$pkg/$name" "lust build"
  if [ "$st" -ne 0 ]; then
    corpus_lust_note unsupported "$pkg/$name" "lust rejected a case the real crate builds: $(head -2 "$proj/$name.lust.err" | tr '\n' ' ')"
    return
  fi
  timeout "$run_timeout" "$proj/$name.lust" >"$proj/$name.lust.out" 2>/dev/null
  status=$?
  if [ $status -ne 0 ] || ! cmp -s "$expected" "$proj/$name.lust.out"; then
    corpus_lust_note wrong "$pkg/$name" "lust output differs from the real crate (exit $status)"
    return
  fi
  if [ "${SANITIZE:-0}" = 1 ]; then
    lust_cc env CARGO_NET_OFFLINE=true "$LUST" "${build[@]}" --emit-c "$proj/$name.san" >/dev/null 2>&1
    st=$?
    if is_timeout "$st"; then
      corpus_lust_note hang "$pkg/$name" "lust build --emit-c timed out alone at ${retry_timeout}s (first at ${case_timeout}s)"
      return
    fi
    corpus_slow_note "$pkg/$name" "lust build --emit-c"
    if [ "$st" -ne 0 ]; then
      corpus_lust_note wrong "$pkg/$name" "lust build --emit-c failed on a case the plain build passed"
      return
    fi
    if ! gcc -O1 -w -fno-sanitize-recover=undefined -fsanitize=address,undefined -o "$proj/$name.sanbin" "$proj/$name.san".*.c 2>"$proj/$name.san.err"; then
      corpus_lust_note wrong "$pkg/$name" "gcc could not build the generated C"
      return
    fi
    timeout "$run_timeout" "$proj/$name.sanbin" >"$proj/$name.san.out" 2>"$proj/$name.san.run.err"
    if grep -q "runtime error\|Sanitizer" "$proj/$name.san.run.err" || ! cmp -s "$expected" "$proj/$name.san.out"; then
      corpus_lust_note wrong "$pkg/$name" "sanitized build failed or output differs"
      return
    fi
  fi
  corpus_lust_note match "$pkg/$name" ""
}

# An exact crate-directory name wins over a case-name substring, so
# `zip` does not select `gzip_write`.
exact_crate=
if [ -n "$corpus_filter" ] && [ -d "$corpus_root/tests/adapters/crates/$corpus_filter" ]; then
  exact_crate=1
fi
for d in "$corpus_root"/tests/adapters/crates/*/; do
  if [ -n "$exact_crate" ] && [ "$(basename "$d")" != "$corpus_filter" ]; then
    continue
  fi
  m0=$lust_match u0=$lust_unsupported w0=$lust_wrong h0=$lust_hang
  corpus_crate_dir "tests/adapters/crates/$(basename "$d")"
  # One line per crate in lust mode: the W-DEPS exactness table (ARCH
  # 9.3). The last line stays the total that land.sh reads.
  if [ -n "${LUST:-}" ] && [ $((lust_match + lust_unsupported + lust_wrong + lust_hang - m0 - u0 - w0 - h0)) -gt 0 ]; then
    echo "crate $(basename "$d"): $((lust_match - m0)) matched, $((lust_unsupported - u0)) unsupported, $((lust_wrong - w0)) wrong"
  fi
done
corpus_finish test-adapters
