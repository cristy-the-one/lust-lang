#!/usr/bin/env bash
# Recipes (tests/recipes/README.md): each sample recipe, built by lust with
# --sandbox and by Cargo, gives the same transcript with the scripted host
# (scripts/recipe-host.py), byte for byte. Then the warm loop a host runs:
# edit the recipe, check, build, run, timed. Prints the timings.
# Usage: scripts/test-recipes.sh   (needs cargo for the rustc side)
set -uo pipefail
# The host script runs the built recipe by its path; Windows wants an .exe
# name and its own process setup there. Linux and macOS run it.
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) echo "test-recipes: skipped on Windows"; exit 0 ;; esac
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
work=$(mktemp -d)
trap 'rm -rf "$work"; [ -n "${sock:-}" ] && rm -f "$sock"' EXIT
pass=0
fail=0
report() {
  if [ "$1" = ok ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $2: $3"; fi
}
# Outside this repo: its .cargo/config and workspace would change the shape.
cp -r "$root/tests/recipes" "$work/r"
for r in bar honeycomb; do printf '\n[workspace]\n' >> "$work/r/$r/Cargo.toml"; done
printf '\n[workspace]\n' >> "$work/r/kit/Cargo.toml"
export LUST_CACHE_DIR=$work/cache
sock=$work/d.sock
export LUST_DAEMON_SOCKET=$sock

params_bar='{"width":72,"holes":5}'
params_honeycomb='{"width":60,"height":40,"cell":8}'
for r in bar honeycomb; do
  dir=$work/r/$r
  p=params_$r
  if "$lust" build --sandbox "$dir/src/main.rs" --manifest-path "$dir/Cargo.toml" -o "$work/$r.lust" >"$work/$r.lerr" 2>&1; then
    python3 "$root/scripts/recipe-host.py" "$work/$r.lust" "${!p}" > "$work/$r.lust.log"
  else
    report bad "$r-lust-build" "$(head -3 "$work/$r.lerr")"; continue
  fi
  if command -v cargo >/dev/null 2>&1; then
    if cargo build -q --offline --manifest-path "$dir/Cargo.toml" --target-dir "$work/target" 2>"$work/$r.cerr"; then
      python3 "$root/scripts/recipe-host.py" "$work/target/debug/$r" "${!p}" > "$work/$r.cargo.log"
      if cmp -s "$work/$r.lust.log" "$work/$r.cargo.log"; then
        report ok "$r-transcript"
      else
        report bad "$r-transcript" "$(diff "$work/$r.lust.log" "$work/$r.cargo.log" | head -5)"
      fi
    else
      report bad "$r-cargo-build" "$(head -3 "$work/$r.cerr")"
    fi
  fi
  grep -q '"done"' "$work/$r.lust.log" && grep -qx 'exit: 0' "$work/$r.lust.log" && report ok "$r-done" || report bad "$r-done" "$(tail -3 "$work/$r.lust.log")"
done

# The warm loop: a host edits a recipe body and rebuilds it, again and again.
python3 - "$lust" "$work/r/bar" "$root/scripts/recipe-host.py" <<'PYEOF'
import os, statistics, subprocess, sys, time
lust, d, host = sys.argv[1:]
src = os.path.join(d, "src/main.rs")
base = open(src).read()
def ms(cmd):
    t = time.perf_counter()
    r = subprocess.run(cmd, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-400:]
    return (time.perf_counter() - t) * 1000
args = ["--sandbox", src, "--manifest-path", os.path.join(d, "Cargo.toml")]
check, build, run = [], [], []
for i in range(30):
    open(src, "w").write(base.replace("0.8 * width", f"0.{(i % 9) + 1} * width"))
    check.append(ms([lust, "check", *args]))
    build.append(ms([lust, "build", *args, "-o", os.path.join(d, "bar.bin")]))
    run.append(ms(["python3", host, os.path.join(d, "bar.bin"), '{"width":72,"holes":5}']))
# A new recipe in a warm slot: the host writes the next recipe's source over
# this one's (same crate, same session), so only the recipe recompiles.
other = open(os.path.join(os.path.dirname(d), "honeycomb/src/main.rs")).read()
swap = []
for i in range(6):
    open(src, "w").write(other if i % 2 == 0 else base)
    swap.append(ms([lust, "build", *args, "-o", os.path.join(d, "bar.bin")]))
open(src, "w").write(base)
med = lambda v: statistics.median(v[3:])
print(f"warm edit loop (bar, 27 edits): check {med(check):.1f} ms, build {med(build):.1f} ms, run with host {med(run):.1f} ms")
print(f"new recipe in a warm slot (6 swaps): build {statistics.median(swap):.1f} ms")
PYEOF
[ $? -eq 0 ] && report ok warm-loop || report bad warm-loop "see above"
echo "test-recipes: $pass passed, $fail failed"
[ $fail -eq 0 ]
