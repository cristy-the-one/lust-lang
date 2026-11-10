# Working on lust

Lust is a subset of Rust, limited to the features that allow a fast compile, with a warm
daemon. Keep every change inside that sentence.

## Rules that do not bend

1. **Rule zero: every program lust accepts is valid Rust** (rustc 1.99, edition 2024) and
   gives the same output as the rustc build (DESIGN.md, "Rule zero"). No lust-only syntax,
   attributes, or semantics. Config lives outside the source (`[package.metadata.lust]`,
   `.lust.toml`, command flags).
2. **What lust cannot match, it rejects** with an error code (`docs/errors/L*.md`,
   `scripts/check-codes.sh`). A reject is fine. A wrong accept or a different output is a bug.
3. **A feature must pay for itself in compile time.** Say no to anything that makes the
   check or the edit loop slower for programs that do not use it: proc macros, `async`,
   user `unsafe`, `extern` blocks, a search in trait selection. Dependencies come in as
   adapter crates (`library/deps`), not as compiled upstream source.
4. **A warm answer equals a cold answer.** A daemon fast path is taken only when it can be
   proved the same as a cold compile. If not, fall to the slower path.
5. **`lust` and `lust-core` stay small to build.** The root crate is not a workspace and
   depends on mimalloc only. A tool that needs more is its own package under `tools/`
   (see `tools/lust-watch`).

## Build and test

```sh
scripts/build-tcc.sh               # once; Windows: scripts/build-tcc.ps1 (pwsh, MSVC shell)
scripts/build.sh                   # fast gate profile, for test runs
cargo build --release              # only to time lust or to package it
LUST_GATE_FILTER=<name> bash scripts/ci.sh   # the gate; no filter runs all of it
```

`build.rs` needs libtcc in `third_party/tcc`, or in `LUST_TCC_HOME`. rustc is the test
oracle: a new accepted program gets a case under `tests/pass`, a new reject under
`tests/fail` or `tests/reject`.

## Layout

- `src/` is the compiler: `client.rs` is the `lust` bin, `main.rs` is `lust-core`, `daemon.rs`
  and `driver/` are the warm sessions, `check/`, `borrowck/`, `lower*`, `cgen*` are the passes.
- `library/` is std and the adapter crates. It ships with the binaries and is read at run time.
- `docs/PLAN.md` is the current plan. DESIGN.md and `docs/ARCH.md` are plans and can be
  older than the code. README.md describes only what is in the tree.

## Who depends on this

Lime Slice bundles lust to build and run recipes (`lust build --sandbox`). It
builds lust from a pinned commit and lays it out as `scripts/release.sh` does. Treat these
as an interface, and say so in the pull request when one changes:

- the package layout: `bin/lust`, `bin/lust-core`, `lib/lust/tcc`, `lib/lust/library`;
- `lust --version`, the command flags, the exit codes, and the JSON diagnostics;
- the sandbox rejects and the error codes.

The trunk is `next`. There is no `main`.
