# `--opt`

`lust build --opt file.rs` and `lust run --opt file.rs` compile the program
with an optimising C compiler instead of tcc. The frontend, the generated C,
the sandbox and the dev profile stay as they are for a normal build: overflow
checks and debug assertions stay on, and `--sandbox` still refuses the same
calls and still plants `lust_sandbox_deny` in front of every OS intrinsic.

`--release` is a different path. It hands the file to Cargo or `rustc`, or
with `--target` to the release driver, and it refuses `--sandbox`. `--opt`
does not.

The host compiler is the one the release driver already uses (`zig cc` when
`zig` is on `PATH`, otherwise `cc`, or `LUST_CC`). Flags are `-O2 -flto` plus
a strict floating-point set: `-fno-fast-math`, `-ffp-contract=off`, no
reassociation, no reciprocal rewrite, and `-fmath-errno` so libm calls stay
libm calls. That is what keeps a result bit-for-bit with the tcc build when
both execute the same operations. There is no fast-math mode.

Calls are direct (`LUST_RELEASE_C` names, for this request only) so the
optimiser can inline across units. The function table the daemon uses for
tcc overlays is not how an `--opt` binary calls. The daemon still serves the
build: `--opt` is its own session, and a tcc session of the same file is left
alone.

## `--opt=bg`

`lust build --sandbox --opt=bg file.rs` links the fast tcc binary for this
run. If an optimised binary for this source is already cached, it is installed
over that output before the client runs it. Otherwise lust starts an `--opt`
build in the background. The cache key is the lust build hash, the target, the
profile (panic, overflow checks, debug assertions, sandbox), the compiler
version, and the hash of every source file in the compile. A later run of the
same source, once that build has published, is the optimised binary.

The background build needs a C compiler. With none on `PATH`, the fast build
is the one you get and lust says so. A failed background build is logged at
`.lust-cache/opt-bg.log` and the next `--opt=bg` tries again. The default
with no `--opt` flag is the tcc build, including `lust watch`.

## Platforms

The generated C is GNU C (the same text tcc compiles), so the opt compiler
is gcc, clang, or `zig cc`, including on Windows. MSVC `cl` cannot compile
it. On Windows, install clang, gcc, or zig, or set `LUST_CC`. Linux CI has
`cc`. macOS has clang. A host with no such compiler skips the opt tests.

Tests: `scripts/test-opt.sh` (in the CI gate). The compute comparison against
tcc is `scripts/opt-bench.sh` on `bench/opt-recipes/compute.rs`. That file
inlines lime-recipe and times nine involute outlines plus an 80-hex clip,
union, cut and inset. It is the stand-in for Lime Slice's uncommitted
compute harness (`docs/plan-native-recipes.md` section 2.7 in
cristy-the-one/lime-slice). The checksum lines must match.
