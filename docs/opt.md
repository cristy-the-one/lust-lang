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
reassociation, no reciprocal rewrite, and `-fmath-errno`. Each libm name the
float methods call (`sin`, `cos`, `atan2`, `exp`, `pow`, `sqrt`, and the
rest, with the `f` forms) is also `-fno-builtin-<name>`, so a constant
`sin(1.0)` is a call to libm at run time, as it is under tcc, and not a
compile-time fold of the compiler's own maths. On Windows those names are
the runtime's ucrt forwarders (`src/runtime/float.c`); a builtin would skip
them. `memcpy` stays a builtin. On the 80-hex bench a blanket
`-fno-builtin` was within about a millisecond of this list (opt geometry
about 25 ms either way, still about 7.5× tcc), so the speed cost there is
noise. The list is what this path passes, so other builtins stay. There is
no fast-math mode.

Calls are direct (`LUST_RELEASE_C` names, for this request only) so the
optimiser can inline across units. The function table the daemon uses for
tcc overlays is not how an `--opt` binary calls. The daemon still serves the
build: `--opt` is its own session, and a tcc session of the same file is left
alone.

## `--opt=bg`

`lust build --sandbox --opt=bg file.rs` links the fast tcc binary for this
run. If an optimised binary for this source is already cached, it is installed
over that output before the client runs it. Otherwise the daemon starts one
`--opt` build in the background. A second request while that build is still
running keeps the fast binary and does not start another. `LUST_OPT_TIMEOUT`
(seconds, default 600) kills a build that runs longer.

The cached binary is `$LUST_CACHE_DIR/opt/` when that variable is set. Lime
Slice passes its own cache folder there. When it is unset, the cache is the
user cache directory (`$XDG_CACHE_HOME/lust/opt`, `~/Library/Caches/lust/opt`
on macOS, `%LOCALAPPDATA%\lust\opt` on Windows), not a folder next to the
recipe and not `.lust-cache` in the project. The log is `opt-bg.log` in that
same directory.

The background process is a child of the daemon. On Linux it gets a
parent-death signal, and on Windows it sits in a job object that closes with
the daemon, so the compile and the C compiler it started die with the daemon.
It runs `lust build`, which invokes the configured C compiler. It does not
run the recipe. A one-shot `lust` with `LUST_NO_DAEMON=1` is the parent
instead, and the build ends when that process does.

The cache key is the lust build hash, the target, the profile (panic,
overflow checks, debug assertions, sandbox), the compiler version, and the
hash of every source file in the compile.

The background build needs a C compiler. With none on `PATH`, the fast build
is the one you get and lust says so. The default with no `--opt` flag is the
tcc build, including `lust watch`.

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
