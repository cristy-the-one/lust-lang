#!/usr/bin/env bash
# Tests for `--sandbox` (src/check/sandbox.rs, docs/sandbox.md): what check
# refuses (L0532), what the expander refuses, the stop codegen puts before
# an OS call check cannot see, and that an allowed program behaves as the
# rustc build of the same source.
# Usage: scripts/test-sandbox.sh
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
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
  local out
  out=$("$lust" check --sandbox --json "$work/$1.rs" 2>&1)
  if [ $? -eq 0 ]; then
    report bad "$1" "check --sandbox accepted it"
  elif grep -q '"code":"L0532"' <<<"$out"; then
    report ok "$1"
  else
    report bad "$1" "want L0532, got: $(head -c 300 <<<"$out")"
  fi
  # The same source checks without the flag.
  "$lust" check "$work/$1.rs" >/dev/null 2>&1 || report bad "$1-plain" "plain check rejects it"
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
out=$("$lust" check --sandbox --json "$work/include.rs" 2>&1)
grep -q '"code":"L0532"' <<<"$out" && report ok include-str || report bad include-str "want L0532, got: $(head -c 300 <<<"$out")"

# An allowed program builds, runs, and prints what the rustc build prints
# (`option_env!` sees no variable in a sandboxed build: lust gets
# LUST_SANDBOX_PROBE and must not show it; rustc runs without it).
in=$'one two\nthree four five\n'
if LUST_SANDBOX_PROBE=set LUST_NO_DAEMON=1 "$lust" build --sandbox "$root/tests/sandbox/allowed.rs" -o "$work/allowed" >"$work/allowed.err" 2>&1; then
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
if "$lust" build --sandbox "$root/tests/sandbox/path_exists.rs" -o "$work/exists" >"$work/exists.err" 2>&1; then
  "$work/exists" >"$work/exists.out" 2>"$work/exists.stderr"; code=$?
  if [ $code -eq 101 ] && grep -qx before "$work/exists.out" && grep -q 'L0532.*the file system' "$work/exists.stderr"; then
    report ok runtime-stop
  else
    report bad runtime-stop "exit $code, stdout $(cat "$work/exists.out"), stderr $(cat "$work/exists.stderr")"
  fi
else
  report bad runtime-stop-builds "$(head -3 "$work/exists.err")"
fi
# The same source without the flag still reaches the file system.
"$lust" build "$root/tests/sandbox/path_exists.rs" -o "$work/exists.plain" >/dev/null 2>&1 && "$work/exists.plain" | grep -qx true \
  && report ok plain-unchanged || report bad plain-unchanged "the plain build should print true"

# A Cargo project the in-process manifest reader does not take (its
# dependency has a `license` line) runs `cargo metadata`: Cargo keeps the
# caller's environment (PATH), while `env!` in the program sees none.
mkdir -p "$work/cp/app/src" "$work/cp/dep/src"
printf '[package]\nname = "app"\nversion = "0.1.0"\nedition = "2024"\n\n[dependencies]\ndep = { path = "../dep" }\n\n[workspace]\n' > "$work/cp/app/Cargo.toml"
printf '[package]\nname = "dep"\nversion = "0.1.0"\nedition = "2024"\nlicense = "MIT"\n\n[workspace]\n' > "$work/cp/dep/Cargo.toml"
printf 'pub fn n() -> u32 { 7 }\n' > "$work/cp/dep/src/lib.rs"
printf 'fn main() { println!("{} {:?}", dep::n(), option_env!("PATH").is_some()); }\n' > "$work/cp/app/src/main.rs"
if "$lust" build --sandbox "$work/cp/app/src/main.rs" --manifest-path "$work/cp/app/Cargo.toml" -o "$work/cp/app.bin" >"$work/cp.err" 2>&1 && [ "$("$work/cp/app.bin")" = "7 false" ]; then
  report ok cargo-metadata-path
else
  report bad cargo-metadata-path "$(head -3 "$work/cp.err")"
fi

# The flag is a dev-build flag: release and the rustc oracle refuse it.
"$lust" build --sandbox --release "$root/tests/sandbox/allowed.rs" >/dev/null 2>&1; [ $? -ne 0 ] && report ok release-refuses || report bad release-refuses "accepted"
"$lust" check --sandbox --rustc "$root/tests/sandbox/allowed.rs" >/dev/null 2>&1; [ $? -ne 0 ] && report ok rustc-refuses || report bad rustc-refuses "accepted"

echo "test-sandbox: $pass passed, $fail failed"
[ $fail -eq 0 ]
