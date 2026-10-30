#!/usr/bin/env bash
# Differential tests against rustc (the oracle). Three modes (ARCH 9.3
# runner table):
#   rustc runs with its default lint levels (only -A warnings), so a deny-by-default lint
#   rejects a case; a case that needs an allow carries it in its source.
#   Default: each compiler with its own defaults (`rustc --edition 2024 -A warnings`, no -C
#   flags: a debug build, panics unwind, overflow checks on; lust build with no -C flags),
#   so this mode compares what a user gets from plain `rustc file.rs` and `lust build file.rs`.
#   PANIC=abort: the same cases with -C panic=abort -C overflow-checks=on on both sides (a
#   profile that asks for abort; a panic exits by SIGABRT, not 101).
#   Both of these modes:
#     tests/pass/*.rs: rustc and lust must both build. Stdout, exit status, and stderr
#                      must match (stdout with the case path as @CASE@ and the checkout
#                      root as @ROOT@, case-pool.sh mask_stdout; stderr with the thread
#                      id masked and the case path as @CASE@: panic text and location
#                      byte for byte, W-PANIC).
#                      A `// panic: <text>` comment also requires <text> in lust's stderr.
#     tests/fail/*.rs: rustc must reject the program, and `lust check` must report the code
#                      named on the first line (`// expect: L0301`); `lust build` must reject it too.
#     tests/reject/*.rs: valid Rust that lust rejects on purpose (lifetimes, user macros).
#                      rustc must accept it, `lust check` must report the expected code, and
#                      `lust build` must reject it.
#   PANIC=unwind (rustc -C panic=unwind, overflow checks on; lust -C panic=unwind):
#     tests/unwind/*.rs plus every tests/pass case. Abort mode cannot test
#     recovered panics, poison, or unwind drops, so those live in
#     tests/unwind. Stdout, exit status, and stderr must match as in abort
#     mode (a backtrace's frame lines are dropped on both sides: lust
#     prints only Rust's "stack backtrace:" header). A tests/unwind case
#     that lust rejects fails; a tests/pass case that lust rejects in
#     unwind mode only (a form unwinding does not cover yet, such as
#     thread::spawn) is UNSUPPORTED, and LUST_STRICT=1 makes it fail.
#     The filter `unwind-` runs only the tests/unwind cases (their names get that prefix).
#   EH=1 (with PANIC=unwind or RELEASE=1): lust's C with unwind tables (LUST_UNWIND_TABLES=1),
#     built by clang -fexceptions (eh_build), in place of the tcc build.
#   CC_DRIVER=1 (with RELEASE=1): the lust release driver builds each case (cc_build:
#     lust build --release --target $CC_TARGET, default the host triple, the libc rustc
#     builds for, with the compiler LUST_CC names), the shipped release path: unwind
#     tables, release C and its inline definitions with LUST_RELEASE_C=1.
#     CC_TARGET=x86_64-unknown-linux-musl adds the musl allocator; its OS error and
#     resolver texts and cfg(target_env) then differ from a glibc rustc's.
#   LL=1 (with PANIC=unwind or RELEASE=1): the LLVM backend's release C and LLVM IR
#     (LUST_RELEASE_C=1 LUST_RELEASE_BACKEND=llvm, unwind tables), built by scripts/llbuild.sh
#     (ll_build, LL_OPT clang level, default -O1), in place of the tcc build.
#   RELEASE=1 (rustc and lust both -C overflow-checks=off -C debug-assertions=off, a Cargo
#     release profile's semantics; panics unwind unless PANIC=abort): tests/release/*.rs plus
#     every tests/pass case (and tests/unwind when panics unwind), compared as in
#     PANIC=unwind mode. A tests/release case that lust rejects fails. The filter `release-`
#     runs only the tests/release cases.
# Usage: scripts/difftest.sh [filter]
#   SANITIZE=1 also builds the generated C with gcc -fsanitize=address,undefined and runs it.
#              Its stdout and exit status must match the plain lust build's.
#              The gcc build uses -Werror=incompatible-pointer-types, so a
#              miscompile that passes a pointer of the wrong type (which
#              tcc accepts silently) fails loudly here. A unit whose sanitized
#              build passes SAN_CC_LIMIT seconds (default 20) is built at -O0.
#   CASE_TIMEOUT seconds per compile (default 30), RUN_TIMEOUT per run
#   (default 10). A case whose lust compile times out reruns once, alone, at
#   RETRY_CASE_TIMEOUT (default 6x); only a second timeout is a HANG failure
#   (scripts/case-pool.sh). A missing or empty fixture dir fails the run, as does
#   a filter that matches no case (ARCH 9.3: no silent skips).
#   DIFFTEST_JOBS cases run at the same time (default nproc), each in its own dir.
#   DIFFTEST_SHARD=<k>/<n> runs shard k of n (details in scripts/case-pool.sh). The run then
#   prints "difftest: shard k/n ran <seen> of <matched> cases" before its summary line.
#   DIFFTEST_RUSTC_CACHE caches rustc's side of each case, never lust's. Details in
#   scripts/case-pool.sh.
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Always compile cold: a warm daemon in this directory (or one autostarted by the first
# test) would serve later tests, and a warm `--emit-c` writes only the changed units, so
# the SANITIZE build would miss the runtime unit. Cache and daemon behaviour have their
# own gate (scripts/test-client.sh). run_all gives each case its own socket path and
# cache dir. The tcc runtime object in $TMPDIR is shared: it is keyed by its content and
# written by rename, so parallel builds are safe (src/tcc.rs cached_runtime_object).
export LUST_NO_DAEMON=1
if ! command -v timeout >/dev/null 2>&1; then
  echo "FAIL: the timeout command is required for per-case timeouts"
  exit 1
fi
filter=${1:-}
oracle_args=()
source "$root/scripts/case-pool.sh"

# shard_line: under DIFFTEST_SHARD, how many cases this shard ran of all that matched.
shard_line() { [ -z "${DIFFTEST_SHARD:-}" ] || echo "difftest: shard $DIFFTEST_SHARD ran $seen of $matched cases"; }

# A fixture dir this run needs must exist and hold .rs files.
require_dir() {
  if [ ! -d "$root/$1" ]; then
    echo "FAIL fixture dir missing: $1"
    exit 1
  fi
  if ! compgen -G "$root/$1/*.rs" >/dev/null; then
    echo "FAIL fixture dir holds no .rs files: $1"
    exit 1
  fi
}

# Extra lust build flags of this mode (the sanitized build uses them too).
lust_flags=()
case "${PANIC:-}" in
  ""|abort|unwind) ;;
  *) echo "FAIL: PANIC must be unset, abort or unwind, not '$PANIC'"; exit 1 ;;
esac
san_flags=(-O1 -w -Werror=incompatible-pointer-types -fno-sanitize-recover=undefined -fsanitize=address,undefined)
# san_obj: this run's gcc objects of the generated C units, named by a hash of the unit's
# bytes. The runtime unit is the same in most cases and is the slow one to build, so each
# distinct unit compiles once per run. Not kept across runs.
san_obj=$work/san-obj
mkdir -p "$san_obj"
# san_cc <unit.c> <obj> <errfile>: the sanitized gcc build of one unit. ASAN's use-after-scope
# instrumentation at -O1 is superlinear in a long function: one 5166-line main (iter_flatten)
# took 185 s, and 0.5 s without that check. A unit that takes longer than SAN_CC_LIMIT seconds
# (default 20) is built again at -O0, which keeps every check (8 s for that unit).
san_cc() {
  timeout "${SAN_CC_LIMIT:-20}" gcc "${san_flags[@]}" -c -o "$2" "$1" 2>>"$3"
  local st=$?
  [ $st -eq 124 ] || return $st
  gcc "${san_flags[@]}" -O0 -c -o "$2" "$1" 2>>"$3"
}

sanitize_case() {
  # $1 fixture, $2 name, $3 expected-stdout file, $4 expected exit status (the unsanitized
  # lust build's, already checked against rustc). Returns 0 on match. Runs in the case dir.
  # A timeout or another exit status (a death without sanitizer text) is a failure.
  local f=$1 name=$2 expected=$3 want=$4 c o st objs=()
  run_lust "$lust" build "${lust_flags[@]}" "$f" -o "$name.tmp" --emit-c "$name" 2>/dev/null || { echo "lust build --emit-c failed"; return 1; }
  : >"$name.san.err"
  for c in "$name".*.c; do
    o=$san_obj/$(sha256sum <"$c" | cut -c1-40).o
    objs+=("$o")
    (
      flock 9
      [ -f "$o" ] || { san_cc "$c" "$o.tmp" "$name.san.err" && mv "$o.tmp" "$o"; }
    ) 9>"$o.lock"
  done
  # -lm: an -O0 unit (san_cc) calls libm functions that -O1 builds inline.
  if ! gcc "${san_flags[@]}" -o "$name.san" "${objs[@]}" -lm 2>>"$name.san.err"; then
    echo "gcc could not build the generated C: $(head -3 "$name.san.err")"
    return 1
  fi
  # Under panic=unwind a panicking statement's C temporaries (a message
  # or format buffer) are not freed (known gap, W-UNWIND): PANIC=unwind mode
  # has no leak check, and in the default mode (which unwinds too) a leak
  # report after a panic is judged again without it, as abort mode never
  # reaches the leak check after a panic. A run with no panic keeps it.
  # A huge malloc returns NULL, as glibc's does, so an allocation failure
  # case runs its own abort path (ASan warns once; that line is ignored).
  local leaks=1
  [ "${PANIC:-}" = unwind ] && leaks=0
  ASAN_OPTIONS=detect_leaks=$leaks:allocator_may_return_null=1 run_to "./$name.san" >"$name.san.out" 2>"$name.san.err"; st=$?
  if [ $leaks = 1 ] && grep -q "LeakSanitizer" "$name.san.err" && grep -q "panicked at" "$name.san.err"; then
    ASAN_OPTIONS=detect_leaks=0:allocator_may_return_null=1 run_to "./$name.san" >"$name.san.out" 2>"$name.san.err"; st=$?
  fi
  # UBSan prints "file:line:col: runtime error"; Rust's own "fatal runtime
  # error: stack overflow" (panic_stack_overflow) is program output.
  if grep -vE "WARNING: AddressSanitizer failed to allocate" "$name.san.err" | grep -qE "[0-9]: runtime error|Sanitizer"; then
    echo "sanitizer: $(grep -m2 -E "[0-9]: runtime error|Sanitizer" "$name.san.err")"
    return 1
  fi
  if [ "$st" -ne "$want" ]; then
    if [ "$st" -eq 124 ] && [ "$want" -ne 124 ]; then echo "sanitized build timed out after ${run_timeout}s"
    else echo "sanitized build exit status $st, want $want: $(head -2 "$name.san.err" | tr '\n' ' ')"; fi
    return 1
  fi
  if ! cmp -s "$expected" <(mask_stdout "$f" "$name.san.out"); then
    echo "sanitized build output differs"
    return 1
  fi
}

# eh_build <fixture> <name>: EH=1 (unwind tables, runtime/panic.c mode 2): lust emits the C with
# LUST_UNWIND_TABLES=1, clang builds it with -fexceptions and the C++ unit (<name>.eh.cc, the
# throw and the catch) with clang++, and clang++ links <name>.lust. Objects are cached per run
# by the unit's hash, as the sanitized build's are.
eh_obj=$work/eh-obj
mkdir -p "$eh_obj"
eh_build() {
  local f=$1 name=$2 c o objs=()
  (export LUST_UNWIND_TABLES=1; run_lust "$lust" build "${lust_flags[@]}" "$f" -o "$name.tmp" --emit-c "$name") || return 1
  for c in "$name".*.c "$name".eh.cc; do
    o=$eh_obj/$(sha256sum <"$c" | cut -c1-40).o
    objs+=("$o")
    (
      flock 9
      local cc=clang
      [[ "$c" == *.cc ]] && cc=clang++
      [ -f "$o" ] || { $cc -O1 -fexceptions -fno-strict-aliasing -fwrapv -w -Wno-error=implicit-function-declaration -c -o "$o.tmp" "$c" && mv "$o.tmp" "$o"; }
    ) 9>"$o.lock" || return 1
  done
  clang++ -o "$name.lust" "${objs[@]}" -lm -lpthread
}

# cc_build <fixture> <name>: CC_DRIVER=1, the release driver build of one case (see the header).
cc_build() {
  run_lust "$lust" build "$1" --release --target "${CC_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}" -C "panic=$panic" -o "$2.lust"
}

# ll_build <fixture> <name>: LL=1 (with EH=1's modes): the LLVM backend (src/cgen/ll.rs):
# lust emits release C with LLVM IR fn bodies (LUST_RELEASE_C=1 LUST_RELEASE_BACKEND=llvm, unwind
# tables), and scripts/llbuild.sh compiles each unit pair as one module and links <name>.lust.
ll_obj=$work/ll-obj
mkdir -p "$ll_obj"
ll_build() {
  local f=$1 name=$2
  (export LUST_UNWIND_TABLES=1 LUST_RELEASE_C=1 LUST_RELEASE_BACKEND=llvm; run_lust "$lust" build "${lust_flags[@]}" "$f" -o "$name.tmp" --emit-c "$name") || return 1
  LLOBJ=$ll_obj JOBS=2 OPT=${LL_OPT:--O1} "$root/scripts/llbuild.sh" "$name" "$name.lust"
}

# lust_stderr <case file> <lust stderr>: lust's stderr as difftest compares it to rustc's
# (mask_stderr, the case path as @CASE@). A panic inside lust's shipped std source reports
# std's own library/ path and line (src/stdloc.rs), so the whole text compares exactly.
lust_stderr() { mask_stderr "$2" | sed "s|$1|@CASE@|g"; }

if [ "${PANIC:-abort}" = unwind ] || [ "${RELEASE:-0}" = 1 ]; then
  # RELEASE=1 alone unwinds, as a release profile and plain rustc do.
  panic=${PANIC:-unwind}
  lust_flags=(-C panic=$panic)
  require_dir tests/pass
  rustc_flags=(--edition 2024 -C panic=$panic -A warnings)
  unwind_cases=""
  mode="PANIC=$panic"
  if [ "$panic" = unwind ]; then
    require_dir tests/unwind
    unwind_cases=$(ls "$root"/tests/unwind/*.rs)
  fi
  if [ "${RELEASE:-0}" = 1 ]; then
    # A release profile: no overflow checks, no debug assertions, on both sides.
    lust_flags+=(-C overflow-checks=off -C debug-assertions=off)
    rustc_flags+=(-C overflow-checks=off -C debug-assertions=off)
    require_dir tests/release
    unwind_cases="$unwind_cases $(ls "$root"/tests/release/*.rs)"
    mode="$mode RELEASE=1"
  else
    rustc_flags+=(-C overflow-checks=on)
  fi
  # tests/unwind (and tests/release) cases, then every tests/pass case: the mode must not
  # change a program that does not panic, and must unwind the ones that do.
  pass_cases=$(ls "$root"/tests/pass/*.rs)
  # Rust prints a full backtrace for a panic while panicking; lust prints
  # only its header, so both sides drop the frame lines.
  no_frames() { sed -E '/^ +[0-9]+: /d; /^ +at /d'; }
  # unwind_case <name> <file>: one PANIC=unwind case (see the header).
  unwind_case() {
    local name=$1 f=$2 rs ls msg
    rustc_oracle run "$f" "${rustc_flags[@]}"
    if [ "$(cat rustc.cc)" -ne 0 ]; then
      report bad "$name" "rustc ($mode) rejected it: $(head -3 rustc.err)"; return
    fi
    rs=$(cat rustc.rs)
    # An optimizing build may turn deep recursion into a loop (as rustc -O does), so a
    # case that expects the unoptimized oracle's stack overflow says nothing in LL=1.
    if [ "${LL:-0}" = 1 ] && grep -q "^// panic: .*overflowed its stack" "$f"; then
      echo unsupported >st
      echo "UNSUPPORTED $name ($mode LL=1): stack depth depends on the optimizer"
      return
    fi
    if [ "${CC_DRIVER:-0}" = 1 ]; then
      cc_build "$f" "$name" 2>"$name.lust.err"
    elif [ "${LL:-0}" = 1 ]; then
      ll_build "$f" "$name" 2>"$name.lust.err"
    elif [ "${EH:-0}" = 1 ]; then
      eh_build "$f" "$name" 2>"$name.lust.err"
    else
      run_lust "$lust" build "${lust_flags[@]}" "$f" -o "$name.lust" 2>"$name.lust.err"
    fi
    if [ $? -ne 0 ]; then
      if [[ "$f" == */unwind/* || "$f" == */release/* ]] || [ "${LUST_STRICT:-0}" = 1 ]; then
        report bad "$name" "lust rejected it: $(head -3 "$name.lust.err")"; return
      fi
      echo unsupported >st
      echo "UNSUPPORTED $name ($mode): $(grep -m1 -E 'L[0-9]{4}|error' "$name.lust.err")"
      return
    fi
    run_to "./$name.lust" >"$name.lust.out" 2>"$name.lust.stderr"; ls=$?
    if [ "$rs" -ne "$ls" ]; then
      report bad "$name" "exit status rustc=$rs lust=$ls: $(head -3 "$name.lust.stderr")"
    elif ! cmp -s rustc.out <(mask_stdout "$f" "$name.lust.out"); then
      report bad "$name" "stdout differs:"; diff rustc.out <(mask_stdout "$f" "$name.lust.out") | head -10
    elif ! cmp -s <(mask_stderr rustc.stderr | no_frames) <(lust_stderr "$f" "$name.lust.stderr" | no_frames); then
      report bad "$name" "stderr differs:"; diff <(mask_stderr rustc.stderr | no_frames) <(lust_stderr "$f" "$name.lust.stderr" | no_frames) | head -10
    elif [ "${SANITIZE:-0}" = 1 ]; then
      if msg=$(sanitize_case "$f" "$name" rustc.out "$ls"); then
        report ok "$name"
      else
        report bad "$name" "sanitized build: $msg"
      fi
    else
      report ok "$name"
    fi
  }
  run_all unwind_case $unwind_cases $pass_cases
  if [ $seen -eq 0 ]; then
    echo "FAIL: no cases matched filter '${filter}'"
    exit 1
  fi
  shard_line
  echo "difftest ($mode): $pass passed, $fail failed, $unsupported unsupported"
  [ $fail -eq 0 ]
  exit $?
fi

require_dir tests/pass
require_dir tests/fail
require_dir tests/reject
# rustc's default lint levels decide acceptance (rule zero): a deny-by-default lint such as
# unconditional_panic rejects the case here. -A warnings lowers only warn-level lints; it
# turns no error off. A case that needs an allow says so in its own source, with a comment.
if [ "${PANIC:-}" = abort ]; then
  lust_flags=(-C panic=abort)
  rustc_flags=(--edition 2024 -C panic=abort -C overflow-checks=on -A warnings)
else
  rustc_flags=(--edition 2024 -A warnings)
fi
# A panic while panicking makes Rust print a backtrace; lust prints only its header, so
# both sides drop the frame lines (as in PANIC=unwind mode).
no_frames() { sed -E '/^ +[0-9]+: /d; /^ +at /d'; }

# pass_case <name> <file>: one tests/pass case (see the header).
pass_case() {
  local name=$1 f=$2 rs ls panic_expect msg
  rustc_oracle run "$f" "${rustc_flags[@]}"
  if [ "$(cat rustc.cc)" -ne 0 ]; then
    report bad "$name" "rustc rejected it: $(head -3 rustc.err)"; return
  fi
  if ! run_lust "$lust" build "${lust_flags[@]}" "$f" -o "$name.lust" 2>"$name.lust.err"; then
    report bad "$name" "lust rejected it: $(head -5 "$name.lust.err")"; return
  fi
  panic_expect=$(sed -n 's|.*// panic: ||p' "$f" | head -1 | sed 's/[[:space:]]*$//')
  rs=$(cat rustc.rs)
  run_to "./$name.lust" >"$name.lust.out" 2>"$name.lust.stderr"; ls=$?
  if [ $rs -ne $ls ]; then
    report bad "$name" "exit status rustc=$rs lust=$ls"
  elif ! cmp -s rustc.out <(mask_stdout "$f" "$name.lust.out"); then
    report bad "$name" "stdout differs:"; diff rustc.out <(mask_stdout "$f" "$name.lust.out") | head -10
  elif ! cmp -s <(mask_stderr rustc.stderr | no_frames) <(lust_stderr "$f" "$name.lust.stderr" | no_frames); then
    report bad "$name" "stderr differs:"; diff <(mask_stderr rustc.stderr | no_frames) <(lust_stderr "$f" "$name.lust.stderr" | no_frames) | head -10
  elif [ -n "$panic_expect" ] && ! grep -qF "$panic_expect" "$name.lust.stderr"; then
    report bad "$name" "lust stderr lacks panic text '$panic_expect': $(head -3 "$name.lust.stderr")"
  elif [ "${SANITIZE:-0}" = 1 ]; then
    if msg=$(sanitize_case "$f" "$name" rustc.out "$ls"); then
      report ok "$name"
    else
      report bad "$name" "sanitized build: $msg"
    fi
  else
    report ok "$name"
  fi
}

# neg_case <name> <file>: one tests/fail or tests/reject case (see the header).
neg_case() {
  local name=$1 f=$2 expect rustc_ok out
  expect=$(head -1 "$f" | sed -n 's|^// expect: *\(L[0-9]*\).*|\1|p')
  # Every mode uses the same default-lint flags: a fail case must be rejected and a reject
  # case accepted by rustc as it is.
  rustc_oracle build "$f" "${rustc_flags[@]}"
  rustc_ok=$(cat rustc.cc)
  if [[ "$f" == */fail/* && $rustc_ok -eq 0 ]]; then
    report bad "$name" "rustc accepted it, so it is not a valid negative test"; return
  fi
  if [[ "$f" == */reject/* && $rustc_ok -ne 0 ]]; then
    report bad "$name" "rustc rejected it, so it belongs in tests/fail"; return
  fi
  out=$(run_lust "$lust" check --json "${lust_flags[@]}" "$f" 2>&1)
  if [ $? -eq 0 ]; then
    report bad "$name" "lust accepted it (expected $expect)"
  elif ! grep -q "\"code\":\"$expect\"" <<<"$out"; then
    report bad "$name" "expected $expect, got: $(grep -o '"code":"L[0-9]*"' <<<"$out" | tr '\n' ' ')"
  elif run_lust "$lust" build "${lust_flags[@]}" "$f" -o "$name.lust" >/dev/null 2>&1; then
    # `build` checks bodies after mono, where a call can reach an
    # instance directly: it must reject what `check` rejects.
    report bad "$name" "lust check rejected it, but lust build accepted it"
  else
    report ok "$name"
  fi
}

# abort_case <name> <file>: pass_case or neg_case by the file's dir, so one job pool runs all.
abort_case() { if [[ "$2" == */pass/* ]]; then pass_case "$@"; else neg_case "$@"; fi; }
run_all abort_case "$root"/tests/pass/*.rs "$root"/tests/fail/*.rs "$root"/tests/reject/*.rs

if [ $seen -eq 0 ]; then
  echo "FAIL: no cases matched filter '${filter}'"
  exit 1
fi
shard_line
echo "difftest${PANIC:+ (PANIC=$PANIC)}: $pass passed, $fail failed"
[ $fail -eq 0 ]
