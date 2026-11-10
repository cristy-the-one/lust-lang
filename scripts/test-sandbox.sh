#!/usr/bin/env bash
# Tests for `--sandbox` (src/check/sandbox.rs, docs/sandbox.md): what check
# refuses (L0532), what the expander refuses, the stop codegen puts before
# an OS call check cannot see, and that an allowed program behaves as the
# rustc build of the same source.
# Usage: scripts/test-sandbox.sh
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
# shellcheck source=scripts/gnu-tools.sh
source "$root/scripts/gnu-tools.sh"
# These checks are the sandbox rules, not the daemon. A live daemon in
# the repo (or one this script would autostart) answers `protocol::request`
# with a read that has no timeout, which is how a stuck daemon holds the step.
export LUST_NO_DAEMON=1
# One lust invocation. 180s is past these small programs; exit 124 is a hang.
lust_to() { timeout 180 "$lust" "$@"; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pass=0
fail=0
report() {
  if [ "$1" = ok ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $2: $3"; fi
}

# refused NAME BODY [ITEMS]: `lust check --sandbox` must answer L0532.
refused() {
  printf '%s\nfn main() {\n    %s\n}\n' "${3:-}" "$2" > "$work/$1.rs"
  local out st
  out=$(lust_to check --sandbox --json "$work/$1.rs" 2>&1)
  st=$?
  if [ "$st" -eq 124 ]; then
    report bad "$1" "HANG lust check --sandbox timed out at 180s"
  elif [ "$st" -eq 0 ]; then
    report bad "$1" "check --sandbox accepted it"
  elif grep -q '"code":"L0532"' <<<"$out"; then
    report ok "$1"
  else
    report bad "$1" "want L0532, got: $(head -c 300 <<<"$out")"
  fi
  # The same source checks without the flag.
  lust_to check "$work/$1.rs" >/dev/null 2>&1 || report bad "$1-plain" "plain check rejects it"
}

refused fs-path 'let _ = std::fs::read("x");'
refused fs-use 'let _ = fs::read_to_string("x");' 'use std::fs;'
refused fs-alias 'let _ = f::read("x");' 'use std::fs as f;'
refused fs-glob 'let _ = read("x");' 'use std::fs::*;'
refused fs-type 'let _ = File::open("x");' 'use std::fs::File;'
refused net 'let _ = TcpStream::connect("127.0.0.1:1");' 'use std::net::TcpStream;'
refused env-var 'let _ = std::env::var("HOME");'
refused env-args 'let _ = std::env::args().count();'
refused process-command 'let _ = std::process::Command::new("ls").status();'
refused thread-spawn 'let _ = std::thread::spawn(|| 1).join();'
refused thread-use-spawn 'let _ = thread::spawn(|| 1).join();' 'use std::thread;'
refused thread-scope 'std::thread::scope(|s| { s.spawn(|| 1); });'
refused thread-builder 'let _ = std::thread::Builder::new().spawn(|| 1);'

# The include! family reads no file in a sandboxed build.
echo hi > "$work/data.txt"
printf 'const S: &str = include_str!("data.txt");\nfn main() { println!("{S}"); }\n' > "$work/include.rs"
out=$(lust_to check --sandbox --json "$work/include.rs" 2>&1)
grep -q '"code":"L0532"' <<<"$out" && report ok include-str || report bad include-str "want L0532, got: $(head -c 300 <<<"$out")"

# An allowed program builds, runs, and prints what the rustc build prints
# (`option_env!` sees no variable in a sandboxed build: lust gets
# LUST_SANDBOX_PROBE and must not show it; rustc runs without it).
in=$'one two\nthree four five\n'
if LUST_SANDBOX_PROBE=set lust_to build --sandbox "$root/tests/sandbox/allowed.rs" -o "$work/allowed" >"$work/allowed.err" 2>&1; then
  printf '%s' "$in" | "$work/allowed" >"$work/allowed.out" 2>"$work/allowed.stderr"; code=$?
  if command -v rustc >/dev/null 2>&1 && env -u LUST_SANDBOX_PROBE rustc --edition 2024 -A warnings "$root/tests/sandbox/allowed.rs" -o "$work/allowed.r" 2>/dev/null; then
    printf '%s' "$in" | "$work/allowed.r" >"$work/allowed.rout" 2>"$work/allowed.rstderr"; rcode=$?
    if cmp -s "$work/allowed.out" "$work/allowed.rout" && cmp -s "$work/allowed.stderr" "$work/allowed.rstderr" && [ $code -eq $rcode ]; then
      report ok allowed-rustc-parity
    else
      report bad allowed-rustc-parity "lust: exit $code, out $(od -c "$work/allowed.out" | head -3 | tr '\n' ' '), err $(od -c "$work/allowed.stderr" | head -3 | tr '\n' ' '); rustc: exit $rcode, out $(od -c "$work/allowed.rout" | head -3 | tr '\n' ' '), err $(od -c "$work/allowed.rstderr" | head -3 | tr '\n' ' ')"
    fi
  fi
  [ $code -eq 3 ] && grep -qx '2 5 None true rs' "$work/allowed.out" && report ok allowed-runs || report bad allowed-runs "exit $code, out $(cat "$work/allowed.out")"
else
  report bad allowed-builds "$(head -3 "$work/allowed.err")"
fi

# A route check cannot see stops at run time, before the call.
if lust_to build --sandbox "$root/tests/sandbox/path_exists.rs" -o "$work/exists" >"$work/exists.err" 2>&1; then
  "$work/exists" >"$work/exists.out" 2>"$work/exists.stderr"; code=$?
  if [ $code -eq 101 ] && grep -qx before "$work/exists.out" && grep -q 'L0532.*the file system' "$work/exists.stderr"; then
    report ok runtime-stop
  else
    report bad runtime-stop "exit $code, stdout $(cat "$work/exists.out"), stderr $(cat "$work/exists.stderr")"
  fi
else
  report bad runtime-stop-builds "$(head -3 "$work/exists.err")"
fi
# --max-memory: the program stops with exit 101 and one L0532 line when its
# process holds more than the limit, after what it printed before. A
# program that stays under the limit runs as the plain build does.
mem_run() { # mem_run <exe> <mode> : runs it, leaves mem.out, mem.stderr; prints the exit code
  printf '%s\n' "$2" | "$1" >"$work/mem.out" 2>"$work/mem.stderr"; echo $?
}
mem_stops() { # mem_stops <name> <exe> <mode>
  local code; code=$(mem_run "$2" "$3")
  if [ "$code" -eq 101 ] && [ "$(cat "$work/mem.out")" = before ] && grep -qx 'error\[L0532\]: this program was built with --max-memory 67108864 and tried to use more memory' "$work/mem.stderr"; then
    report ok "$1"
  else
    report bad "$1" "exit $code, stdout $(cat "$work/mem.out"), stderr $(cat "$work/mem.stderr")"
  fi
}
mem_runs() { # mem_runs <name> <exe> <mode> <last stdout line>
  local code; code=$(mem_run "$2" "$3")
  if [ "$code" -eq 0 ] && [ "$(tail -1 "$work/mem.out")" = "$4" ] && [ ! -s "$work/mem.stderr" ]; then
    report ok "$1"
  else
    report bad "$1" "exit $code, stdout $(cat "$work/mem.out"), stderr $(cat "$work/mem.stderr")"
  fi
}
if lust_to build --sandbox --max-memory 64M "$root/tests/sandbox/memory.rs" -o "$work/mem64" >"$work/mem.err" 2>&1 \
  && lust_to build --sandbox --max-memory=1G "$root/tests/sandbox/memory.rs" -o "$work/mem1g" >>"$work/mem.err" 2>&1 \
  && lust_to build --sandbox "$root/tests/sandbox/memory.rs" -o "$work/memnone" >>"$work/mem.err" 2>&1; then
  mem_stops memory-held-blocks "$work/mem64" vec
  mem_stops memory-one-request "$work/mem64" big
  mem_stops memory-hash-table "$work/mem64" map
  mem_runs memory-churn-is-not-held "$work/mem64" churn "churned 2048 MiB"
  mem_runs memory-under-the-limit "$work/mem1g" vec "held 200 MiB"
  mem_runs memory-no-limit "$work/memnone" vec "held 200 MiB"
else
  report bad memory-builds "$(head -3 "$work/mem.err")"
fi
lust_to build --max-memory 64M "$root/tests/sandbox/memory.rs" -o "$work/x" >/dev/null 2>&1; [ $? -eq 2 ] && report ok memory-needs-sandbox || report bad memory-needs-sandbox "--max-memory without --sandbox should be a usage error"
lust_to check --sandbox --max-memory 64M "$root/tests/sandbox/memory.rs" >/dev/null 2>&1; [ $? -eq 2 ] && report ok memory-not-on-check || report bad memory-not-on-check "--max-memory on check should be a usage error"
lust_to build --sandbox --max-memory lots "$root/tests/sandbox/memory.rs" -o "$work/x" >/dev/null 2>&1; [ $? -eq 2 ] && report ok memory-bad-value || report bad memory-bad-value "a bad --max-memory value should be a usage error"
# --time-limit: a spin and a long sleep stop with exit 101 and one L0533
# line, soon. A program that finishes inside the limit runs as usual.
time_run() { # time_run <exe> <mode> : leaves time.out, time.stderr; prints the exit code
  set +o pipefail
  printf '%s\n' "$2" | timeout 5 "$1" >"$work/time.out" 2>"$work/time.stderr"
  local code=${PIPESTATUS[1]}
  set -o pipefail
  echo "$code"
}
time_stops() { # time_stops <name> <exe> <mode>
  local start end code
  start=$(date +%s)
  code=$(time_run "$2" "$3")
  end=$(date +%s)
  if [ "$code" -eq 101 ] && [ "$(head -1 "$work/time.out")" = before ] && ! grep -q after "$work/time.out" \
    && grep -qx 'error\[L0533\]: this program was built with --time-limit 300ms and tried to run longer' "$work/time.stderr" \
    && [ $((end - start)) -lt 3 ]; then
    report ok "$1"
  else
    report bad "$1" "exit $code after $((end - start))s, stdout $(cat "$work/time.out"), stderr $(cat "$work/time.stderr")"
  fi
}
if lust_to build --sandbox --time-limit 300ms "$root/tests/sandbox/time.rs" -o "$work/time300" >"$work/time.err" 2>&1 \
  && lust_to build --sandbox --time-limit=2s "$root/tests/sandbox/time.rs" -o "$work/time2s" >>"$work/time.err" 2>&1; then
  time_stops time-spin "$work/time300" spin
  time_stops time-sleep "$work/time300" sleep
  code=$(time_run "$work/time300" finish)
  if [ "$code" -eq 0 ] && [ "$(tail -1 "$work/time.out")" = done ] && [ ! -s "$work/time.stderr" ]; then
    report ok time-under-the-limit
  else
    report bad time-under-the-limit "exit $code, stdout $(cat "$work/time.out"), stderr $(cat "$work/time.stderr")"
  fi
  code=$(time_run "$work/time2s" finish)
  if [ "$code" -eq 0 ] && [ "$(tail -1 "$work/time.out")" = done ]; then
    report ok time-seconds-suffix
  else
    report bad time-seconds-suffix "exit $code, stdout $(cat "$work/time.out"), stderr $(cat "$work/time.stderr")"
  fi
else
  report bad time-builds "$(head -3 "$work/time.err")"
fi
lust_to build --time-limit 1s "$root/tests/sandbox/time.rs" -o "$work/x" >/dev/null 2>&1; [ $? -eq 2 ] && report ok time-needs-sandbox || report bad time-needs-sandbox "--time-limit without --sandbox should be a usage error"
lust_to check --sandbox --time-limit 1s "$root/tests/sandbox/time.rs" >/dev/null 2>&1; [ $? -eq 2 ] && report ok time-not-on-check || report bad time-not-on-check "--time-limit on check should be a usage error"
lust_to build --sandbox --time-limit fast "$root/tests/sandbox/time.rs" -o "$work/x" >/dev/null 2>&1; [ $? -eq 2 ] && report ok time-bad-value || report bad time-bad-value "a bad --time-limit value should be a usage error"
# The same source without the flag still reaches the file system.
lust_to build "$root/tests/sandbox/path_exists.rs" -o "$work/exists.plain" >/dev/null 2>&1 && "$work/exists.plain" | grep -qx true \
  && report ok plain-unchanged || report bad plain-unchanged "the plain build should print true"

# A Cargo project the in-process manifest reader does not take (its
# dependency has a `license` line) runs `cargo metadata`: Cargo keeps the
# caller's environment (PATH), while `env!` in the program sees none.
mkdir -p "$work/cp/app/src" "$work/cp/dep/src"
printf '[package]\nname = "app"\nversion = "0.1.0"\nedition = "2024"\n\n[dependencies]\ndep = { path = "../dep" }\n\n[workspace]\n' > "$work/cp/app/Cargo.toml"
printf '[package]\nname = "dep"\nversion = "0.1.0"\nedition = "2024"\nlicense = "MIT"\n\n[workspace]\n' > "$work/cp/dep/Cargo.toml"
printf 'pub fn n() -> u32 { 7 }\n' > "$work/cp/dep/src/lib.rs"
printf 'fn main() { println!("{} {:?}", dep::n(), option_env!("PATH").is_some()); }\n' > "$work/cp/app/src/main.rs"
if lust_to build --sandbox "$work/cp/app/src/main.rs" --manifest-path "$work/cp/app/Cargo.toml" -o "$work/cp/app.bin" >"$work/cp.err" 2>&1 && [ "$("$work/cp/app.bin")" = "7 false" ]; then
  report ok cargo-metadata-path
else
  report bad cargo-metadata-path "$(head -3 "$work/cp.err")"
fi

# The flag is a dev-build flag: release and the rustc oracle refuse it.
lust_to build --sandbox --release "$root/tests/sandbox/allowed.rs" >/dev/null 2>&1; st=$?
if [ "$st" -eq 124 ]; then report bad release-refuses "HANG timed out at 180s"; elif [ "$st" -ne 0 ]; then report ok release-refuses; else report bad release-refuses "accepted"; fi
lust_to check --sandbox --rustc "$root/tests/sandbox/allowed.rs" >/dev/null 2>&1; st=$?
if [ "$st" -eq 124 ]; then report bad rustc-refuses "HANG timed out at 180s"; elif [ "$st" -ne 0 ]; then report ok rustc-refuses; else report bad rustc-refuses "accepted"; fi

echo "test-sandbox: $pass passed, $fail failed"
[ $fail -eq 0 ]
