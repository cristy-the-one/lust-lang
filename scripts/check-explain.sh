#!/usr/bin/env bash
# Checks every `lust explain` page in docs/errors/:
#   1. Every L#### code in src/ has a page, and every page is listed in the index.
#   2. The wrong example fails under `lust build` with exactly that code present.
#      A page marked `<!-- runtime-stop -->` is valid Rust: the wrong example
#      builds, and running it exits with the code on stderr.
#   3. The wrong example fails under rustc too, unless the page carries the marker
#      `<!-- rustc-wrong: pass -->` (valid Rust that lust rejects on purpose).
#   4. The fixed example builds under both lust and rustc. A runtime-stop page
#      also runs the fixed example.
# Usage: scripts/check-explain.sh [filter]
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
filter=${1:-}
pass=0
fail=0
report() {
  if [ "$1" = ok ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $2: $3"; fi
}
rustc_flags=(--edition 2024 -C overflow-checks=on -A warnings)

# 0. Registry: ranges, collisions, reserved codes, explain rows (no build needed).
if ! "$root/scripts/check-codes.sh" "$root"; then
  report bad registry "scripts/check-codes.sh failed (see above)"
fi

# 1. Coverage: source codes, pages, and index must agree.
grep -rhoE 'L[0-9]{4}' "$root"/src/ | sort -u > "$work/src-codes"
ls "$root"/docs/errors/L*.md | sed 's|.*/||; s|\.md||' | sort -u > "$work/doc-codes"
if ! diff -q "$work/src-codes" "$work/doc-codes" >/dev/null; then
  report bad coverage "codes in src/ and docs/errors/ differ:"
  diff "$work/src-codes" "$work/doc-codes" | head -10
else
  report ok coverage
fi
while IFS= read -r code; do
  grep -q "$code" "$root/docs/errors/README.md" || report bad "index/$code" "not listed in docs/errors/README.md"
done < "$work/doc-codes"

# Extracts one fenced block from a page: $1 = file, $2 = Wrong|Fixed, writes to stdout.
extract() {
  awk -v want="## $2" '
    $0 == want { on = 1; next }
    on && /^```rust$/ { grab = 1; next }
    grab && /^```$/ { exit }
    grab { print }
  ' "$1"
}

# Writes the `## Files` section's extra files into the work dir: each entry
# is a `### relpath` heading plus one fenced block. Module and manifest
# errors (L0502, L0530, L0531) need sibling files next to the example.
extract_files() {
  awk -v root="$work" '
    /^## Files$/ { on = 1; next }
    /^## / { if (on && $0 != "## Files") on = 0 }
    on && /^### / { path = root "/" $2; n = split(path, parts, "/"); dir = parts[1]; for (i = 2; i < n; i++) dir = dir "/" parts[i]; system("mkdir -p \"" dir "\""); next }
    on && /^```/ { grab = !grab; next }
    on && grab && path != "" { print > path }
  ' "$1"
}

for page in "$root"/docs/errors/L*.md; do
  code=$(basename "$page" .md)
  [[ -n "$filter" && "$code" != *"$filter"* ]] && continue
  extract "$page" Wrong > "$work/$code-wrong.rs"
  extract "$page" Fixed > "$work/$code-fixed.rs"
  grep -q '^## Files$' "$page" && extract_files "$page"
  # `<!-- lust-args: ... -->` adds args to the wrong example's lust build
  # only (a manifest selection, say); `{work}` expands to the work dir.
  lust_args=$(sed -n 's|.*<!-- lust-args: \(.*\) -->.*|\1|p' "$page" | head -1 | sed "s|{work}|$work|g")
  if [ ! -s "$work/$code-wrong.rs" ] || [ ! -s "$work/$code-fixed.rs" ]; then
    report bad "$code" "missing a Wrong or Fixed example"; continue
  fi
  # A runtime stop (L0533) is valid Rust. The wrong example builds, then
  # the program exits with the code on stderr. The fixed example runs.
  if grep -q 'runtime-stop' "$page"; then
    # shellcheck disable=SC2086 # lust_args is a word list by design
    if ! "$lust" build "$work/$code-wrong.rs" -o "$work/$code.bin" $lust_args >"$work/$code-rt.lerr" 2>&1; then
      report bad "$code" "lust rejected the runtime example: $(head -3 "$work/$code-rt.lerr")"; continue
    fi
    timeout 5 "$work/$code.bin" >"$work/$code-rt.out" 2>"$work/$code-rt.err"
    rc=$?
    if [ $rc -eq 0 ] || [ $rc -eq 124 ] || ! grep -q "$code" "$work/$code-rt.err"; then
      report bad "$code" "runtime stop: exit $rc, stderr $(head -3 "$work/$code-rt.err")"; continue
    fi
    if ! rustc "${rustc_flags[@]}" "$work/$code-wrong.rs" -o "$work/$code.rbin" >"$work/$code-rt.rerr" 2>&1; then
      report bad "$code" "rustc rejects the runtime example: $(head -3 "$work/$code-rt.rerr")"; continue
    fi
    # shellcheck disable=SC2086
    if ! "$lust" build "$work/$code-fixed.rs" -o "$work/$code-fixed.bin" $lust_args >"$work/$code-fixed.lerr" 2>&1; then
      report bad "$code" "lust rejects the fixed example: $(head -3 "$work/$code-fixed.lerr")"; continue
    fi
    timeout 5 "$work/$code-fixed.bin" >"$work/$code-fixed.out" 2>"$work/$code-fixed.run"
    if [ $? -ne 0 ]; then
      report bad "$code" "the fixed example did not run: $(head -3 "$work/$code-fixed.run")"; continue
    fi
    if ! rustc "${rustc_flags[@]}" "$work/$code-fixed.rs" -o "$work/$code.rbin" >"$work/$code-fixed.rerr" 2>&1; then
      report bad "$code" "rustc rejects the fixed example: $(head -3 "$work/$code-fixed.rerr")"; continue
    fi
    report ok "$code"
    continue
  fi
  # 2. Wrong under lust.
  # shellcheck disable=SC2086 # lust_args is a word list by design
  out=$("$lust" build --json "$work/$code-wrong.rs" -o "$work/$code.bin" $lust_args 2>&1)
  if [ $? -eq 0 ]; then
    report bad "$code" "lust accepted the wrong example"; continue
  elif ! grep -q "\"code\":\"$code\"" <<<"$out"; then
    report bad "$code" "wrong example gives $(grep -o '"code":"L[0-9]*"' <<<"$out" | tr '\n' ' '), want $code"; continue
  fi
  # 3. Wrong under rustc (unless marked as valid Rust).
  # (rc holds rustc's status: the grep below sets $? itself.)
  rustc "${rustc_flags[@]}" "$work/$code-wrong.rs" -o "$work/$code.rbin" >/dev/null 2>&1; rc=$?
  if grep -q 'rustc-wrong: pass' "$page"; then
    [ $rc -eq 0 ] || { report bad "$code" "page says rustc accepts the wrong example, but rustc rejects it"; continue; }
  else
    [ $rc -ne 0 ] || { report bad "$code" "rustc accepts the wrong example; add the rustc-wrong marker if lust rejects valid Rust on purpose"; continue; }
  fi
  # 4. Fixed under both.
  if ! "$lust" build "$work/$code-fixed.rs" -o "$work/$code.bin" >"$work/$code-fixed.lerr" 2>&1; then
    report bad "$code" "lust rejects the fixed example: $(head -3 "$work/$code-fixed.lerr")"; continue
  fi
  if ! rustc "${rustc_flags[@]}" "$work/$code-fixed.rs" -o "$work/$code.rbin" >"$work/$code-fixed.rerr" 2>&1; then
    report bad "$code" "rustc rejects the fixed example: $(head -3 "$work/$code-fixed.rerr")"; continue
  fi
  report ok "$code"
done

echo "check-explain: $pass passed, $fail failed"
[ $fail -eq 0 ]
