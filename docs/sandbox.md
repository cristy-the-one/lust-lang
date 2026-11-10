# `--sandbox`

`lust check|build|run --sandbox file.rs` builds a program that may read
stdin and write stdout and stderr, and use no other OS service. It is for
code a tool or a model wrote and a host runs: a recipe in a CAD app, a
candidate in a search loop. The host talks to the program over stdin and
stdout; the program cannot reach the host's files, network, environment
or processes on its own.

It is lust's own flag, not rustc's. `--release` and `--rustc` refuse it. `--opt` and `--opt=bg` accept it: the optimised build is the same sandboxed C, compiled with an optimising C compiler. See [opt.md](opt.md).

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
| 101 | `error[L0532]: this program was built with --max-memory <bytes> and tried to use more memory` | The memory limit (below). What the program printed before it is flushed first. |
| 101 | `error[L0533]: this program was built with --time-limit <duration> and tried to run longer` | The time limit (below). What the program printed before it is flushed first. |
| any other | whatever the program wrote | `std::process::exit(n)`, or a signal (killed by the host, which keeps its own wall clock as the backstop). |

Check and build errors come before any run, as JSON with `--json`, each
with its `edits`.

## The memory limit

`lust build --sandbox --max-memory <size>` (and `lust run`) builds the
limit into the program. `<size>` is a byte count, or one with a binary
suffix: `64K`, `512M`, `1G`. The program stops, as the table above shows,
when its process has held more memory than that. The flag needs
`--sandbox`, and a limit is its own daemon session, like `--sandbox`.

What is measured is the operating system's peak for the process: the peak
resident set on Linux and macOS, the peak commit charge on Windows. That
is the number a host reads when it polls the process, so a host cap and
this one agree. The runtime reads it after an allocation, once for every
64th of the limit the program asks for, so the stop comes within about
that much of the limit. A single request above the whole limit stops
before it is made.

Limits of the limit:

- The check runs when the program allocates. On Linux and macOS, memory a
  program reserved and did not touch is not held yet; if it touches those
  pages later and allocates nothing more, the stop does not come. A host
  that must bound that case keeps its own cap as the backstop.
- Memory the program asked for in turn and gave back does not count:
  only what the process held at once.
- The stack is not part of this limit. A stack overflow is its own stop
  (`fatal runtime error: stack overflow`, then an abort).

## The time limit

`lust build --sandbox --time-limit <duration>` (and `lust run`) builds a
deadline into the program. `<duration>` is a number of seconds (`2`, `2s`),
milliseconds (`500ms`), or minutes (`1m`). The program stops, as the table
above shows, when it has run longer than that. The flag needs `--sandbox`,
and a limit is its own daemon session, like `--sandbox` and `--max-memory`.
`check` does not take it: a check does not run the program.

The clock is monotonic. `main` records the deadline before the program
runs. Three probes read it:

- A jump back to the same block or an earlier one (a loop) checks every
  iteration.
- A function entry checks every 64th call, so a recursion that never
  loops still stops.
- `thread::sleep` waits only for the time still left. A sleep that would
  run past the deadline does not.

The stop comes at the next probe after the deadline, so one long loop
body or one of those 64 calls can run past it. What the limit does not
see:

- A straight-line body that does not loop, call, or sleep. The host's
  wall clock is the backstop for that shape.
- The stack. A recursion that fills the stack first is the stack-overflow
  stop, not L0533.

The host should keep its own wall clock anyway. This limit is the stop
the program can report; the host kill is what happens when the probes
do not run.

## What it does not do

- It does not limit time unless the build asks (`--time-limit`, above).
  Memory has a limit when the build asks for one (`--max-memory`, above).
- It does not change what an allowed program prints. A sandboxed program
  that only uses allowed services behaves as the rustc build of the same
  source (`scripts/test-sandbox.sh` checks one).
- A host that starts the program with extra open file descriptors still
  owns them; std has no safe way to reach a descriptor it did not open,
  but the host should pass only stdin, stdout and stderr.

Tests: `scripts/test-sandbox.sh` (in the CI gate).
