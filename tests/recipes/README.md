# Recipe fixtures

A recipe is a small program a CAD app's MCP host builds with
`lust build --sandbox` and runs with a JSON-lines protocol on stdin and
stdout: the host sends one start line with the parameters, the recipe
sends one call per line and reads one reply per line, and ends with a
`done` line. This is the shape the lime-slice plan calls protocol v1.

- `kit/`: a stand-in for the host's recipe crate (`lime-recipe` in that
  plan): no dependencies, its own small JSON writer and reader.
- `bar/`, `honeycomb/`: two recipes, each a one-package Cargo project
  with a path dependency on `kit`, the shape lust reads without
  `cargo metadata`.

`scripts/test-recipes.sh` drives each recipe with a scripted host, built
by lust (`--sandbox`) and by Cargo, and requires the same call stream,
byte for byte (rule zero for recipes). It also times the warm edit loop.

## Numbers (2026-11-01, 2-vCPU Linux sandbox, gate build)

| Step | lust | Cargo (debug) |
|---|---:|---:|
| Cold build of `bar`, empty caches | 407 ms | 679 ms |
| First build of a new recipe crate in a running daemon | about 420 ms | |
| A new recipe written over a warm slot (same crate, new source) | about 105 ms | |
| Edit + check, warm | 7.5 ms | |
| Edit + build, warm | 14.8 ms | |

A host gets the third row by keeping a few recipe slots (one crate
directory each, with the kit as a path dependency) and writing each new
recipe's source into a free slot: the daemon keys sessions by crate, so
the slot's session, std and kit stay warm. No in-memory source request
is needed for this.
