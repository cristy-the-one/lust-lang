# `--sandbox`

`lust check|build|run --sandbox file.rs` builds a program that may read
stdin and write stdout and stderr, and use no other OS service. It is for
code a tool or a model wrote and a host runs: a recipe in a CAD app, a
candidate in a search loop. The host talks to the program over stdin and
stdout; the program cannot reach the host's files, network, environment
or processes on its own.

It is lust's own flag, not rustc's. `--release` and `--rustc` refuse it.

## What a sandboxed program keeps

- `std::io::stdin`, `stdout`, `stderr` and everything built on them.
- Memory, collections, formatting, math, `std::time` (the clock and
  `thread::sleep`), `std::path` as text (`Path::new`, `extension`,
  `is_absolute`), `std::process::exit` and `abort`.
- `std::env::args` is refused at check; the host passes data on stdin.

## How it is enforced

Three layers, each enough for what it covers.

| Layer | Where | What it refuses |
|---|---|---|
| Check | `src/check/sandbox.rs` | A user body that names `std::fs`, `std::net`, `std::env`, `std::os`, or `std::process` other than `exit`, `abort`, `ExitCode`, `Termination`, `id`, however a `use` spelled it (the resolver has already turned the path into one std name). A thread spawn in any form (`thread::spawn`, `thread::scope`, `Builder::spawn`, a scope's `spawn`), found by its resolved builtin. Error L0532 at the expression. |
| Expand | `src/expand/builtin.rs` | `include!`, `include_str!`, `include_bytes!` (L0532): a build copies no host file in. `env!` and `option_env!` see no variable. `cargo metadata` and the C compiler still run with the caller's environment (they need its `PATH`). |
| Codegen | `src/cgen.rs` `intrinsic` | Every OS intrinsic of a refused family that a reachable std or adapter body calls gets `lust_sandbox_deny(family)` in front of it. The program prints `error[L0532]: this program was built with --sandbox and tried to use <family>` on stderr and exits with status 101 before the call runs. This covers routes check cannot see, such as `Path::exists` reaching `fs::metadata`. |

The families are in `sandbox::intrinsic_family`. A unit test fails when a
new `sys_` intrinsic is neither kept nor placed in a family.

A sandboxed build is its own daemon session (`ProfileFlags::sandbox` is
part of the session key), so its warm state and cached objects never mix
with a normal build of the same file.

## Reading a failure

A host reads a failed run from the exit status and stderr; nothing in
the program's stdout is lust's.

| Exit status | stderr | Meaning |
|---|---|---|
| 101 | `thread 'main' (<tid>) panicked at <file>:<line>:<col>:` then the message | A panic. The line is rustc's, byte for byte (rule zero), so a host parses one format for both builds. `<file>` is the path as the build command named it. |
| 101 | `error[L0532]: this program was built with --sandbox and tried to use <family>` | The codegen stop (above). Nothing ran past it. |
| any other | whatever the program wrote | `std::process::exit(n)`, or a signal (killed by the host on a time limit). |

Check and build errors come before any run, as JSON with `--json`, each
with its `edits`.

## What it does not do

- It does not limit time or memory. The host kills a program that runs
  too long; a memory cap is planned (`--max-memory`).
- It does not change what an allowed program prints. A sandboxed program
  that only uses allowed services behaves as the rustc build of the same
  source (`scripts/test-sandbox.sh` checks one).
- A host that starts the program with extra open file descriptors still
  owns them; std has no safe way to reach a descriptor it did not open,
  but the host should pass only stdin, stdout and stderr.

Tests: `scripts/test-sandbox.sh` (in the CI gate).
