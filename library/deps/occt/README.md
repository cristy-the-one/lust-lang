# occt

Lust adapter for the first slice of the Open CASCADE C shim
(`docs/occt-shim.md`). This is not a crates.io stand-in. A path
dependency whose package name is `occt` compiles this crate, and the
crate is the only one that may call the `occt_*` intrinsics.

The shared library is built by `scripts/build-occt-shim.sh`, not by
`scripts/build.sh` and not by CI. A program that does not depend on
`occt` does not emit the loader and does not look at the cache.
