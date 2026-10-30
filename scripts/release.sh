#!/usr/bin/env bash
# Builds the release package for linux x86_64: dist/lust-<version>-x86_64-linux.tar.gz.
#
#   scripts/release.sh            # cargo build --release, then pack and print the sha256
#   scripts/release.sh --no-build # pack the binaries already in target/release
#
# Layout (lust finds lib/lust relative to its own executable; see `tcc::install_dir`):
#   bin/lust, bin/lust-core       the client and the compiler (Cargo.toml [profile.release])
#   lib/lust/tcc/bin/tcc          the pinned tcc, for parallel `tcc -c`
#   lib/lust/tcc/lib/tcc/         libtcc1.a and tcc's headers (libtcc itself is linked in)
#   lib/lust/library/             std and adapter sources read at run time (library/deps)
#   LICENSE, tcc-COPYING          lust's MIT license; tcc's LGPL 2.1 (from the tinycc checkout)
# The runtime C and the std sources lust splices are built into lust-core.
# tcc comes from $LUST_TCC_HOME, else third_party/tcc (scripts/build-tcc.sh), as in build.rs.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
build=1
[ "${1:-}" = "--no-build" ] && build=0
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
[ "$(uname -sm)" = "Linux x86_64" ] || { echo "release.sh: only linux x86_64 is packaged" >&2; exit 2; }
tcc=${LUST_TCC_HOME:-$root/third_party/tcc}
[ -x "$tcc/bin/tcc" ] && [ -f "$tcc/lib/tcc/libtcc1.a" ] || { echo "release.sh: no tcc in $tcc (run scripts/build-tcc.sh)" >&2; exit 2; }
tcc_license=$tcc/../tinycc/COPYING
[ -f "$tcc_license" ] || { echo "release.sh: no $tcc_license (scripts/build-tcc.sh clones tinycc there)" >&2; exit 2; }
if [ $build = 1 ]; then
  LUST_TCC_HOME=$tcc cargo build --release --locked --bin lust --bin lust-core
fi

name=lust-$version-x86_64-linux
out=$root/dist
stage=$out/$name
rm -rf "$stage" "$out/$name.tar.gz"
mkdir -p "$stage/bin" "$stage/lib/lust/tcc/bin" "$stage/lib/lust/tcc/lib"
cp target/release/lust target/release/lust-core "$stage/bin/"
cp "$tcc/bin/tcc" "$stage/lib/lust/tcc/bin/"
cp -r "$tcc/lib/tcc" "$stage/lib/lust/tcc/lib/"
cp -r library "$stage/lib/lust/library"
cp README.md LICENSE "$stage/"
cp "$tcc_license" "$stage/tcc-COPYING"

# Reproducible tar: sorted names, fixed owner and mtime (the commit time).
mtime=$(git log -1 --format=%ct 2>/dev/null || echo 0)
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$mtime" -C "$out" -cf - "$name" | gzip -n -9 > "$out/$name.tar.gz"
rm -rf "$stage"
(cd "$out" && sha256sum "$name.tar.gz" | tee "$name.tar.gz.sha256")
echo "glibc needed: $(objdump -T "$root/target/release/lust-core" "$tcc/bin/tcc" 2>/dev/null | grep -v ' w ' | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
