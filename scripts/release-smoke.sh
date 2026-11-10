#!/usr/bin/env bash
# Smoke test of a release package (scripts/release.sh): unpacks it away from the checkout
# and checks, runs and builds one program with the packaged lust alone. A package whose
# tcc cannot find its headers, or whose std sources are missing, fails here and is not
# published (.github/workflows/release.yml).
#   scripts/release-smoke.sh dist/lust-<version>-<platform>.tar.gz
set -euo pipefail
pkg=${1:?usage: release-smoke.sh <package.tar.gz>}
pkg=$(cd "$(dirname "$pkg")" && pwd)/$(basename "$pkg")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tar xzf "$pkg" -C "$work"
root=$work/$(basename "$pkg" .tar.gz)
lust=$root/bin/lust
[ -x "$lust" ] || { echo "release-smoke: no bin/lust in $pkg" >&2; exit 1; }
# Nothing of the checkout or of a dev build may help the packaged binaries.
unset LUST_TCC_HOME LUST_STD_SRC
export LUST_NO_DAEMON=1 LUST_CACHE_DIR=$work/cache
mkdir "$work/try"
cd "$work/try"
cat > hello.rs <<'RS'
use std::collections::BTreeMap;
fn main() {
    let v: Vec<u32> = (1..=4).collect();
    let mut m = BTreeMap::new();
    m.insert("sum", v.iter().sum::<u32>());
    println!("hello {:?}", m);
}
RS
want='hello {"sum": 10}'
"$lust" --version
"$lust" check hello.rs
got=$("$lust" run hello.rs)
[ "$got" = "$want" ] || { echo "release-smoke: lust run printed: $got" >&2; exit 1; }
"$lust" build hello.rs -o hello
got=$(./hello)
[ "$got" = "$want" ] || { echo "release-smoke: the built program printed: $got" >&2; exit 1; }
echo "release-smoke: $(basename "$pkg") ok"
