#!/usr/bin/env bash
# Builds lust with the fast gate profile (Cargo.toml [profile.gate]) and copies `lust` and
# `lust-core` to target/release/, where the test scripts look for them. Use it for test runs;
# use `cargo build --release` to time lust. Extra args go to cargo (e.g. --all-targets).
# Note: after this, target/release/ may hold gate binaries until the next release build that
# has a source change to rebuild.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
cargo build --profile gate "$@"
mkdir -p target/release
# New files (rm first): cargo hardlinks target/release/lust* to target/release/deps/.
rm -f target/release/lust target/release/lust-core
cp target/gate/lust target/gate/lust-core target/release/
# The watcher is a separate package so notify is not linked into lust.
# Its release binary sits next to the client; `lust watch` execs it.
cargo build --release --manifest-path tools/lust-watch/Cargo.toml --bin lust-watch
if [ -f tools/lust-watch/target/release/lust-watch.exe ]; then
  # Git bash treats `name` and `name.exe` as the same file, so a second
  # copy onto the bare name fails. The client looks for the `.exe` first.
  cp tools/lust-watch/target/release/lust-watch.exe target/release/lust-watch.exe
else
  cp tools/lust-watch/target/release/lust-watch target/release/lust-watch
fi
