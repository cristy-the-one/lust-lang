# Open CASCADE shim (phase 1 plan)

This is a plan, not an implementation. Nothing here is built, and CI does
not fetch or compile Open CASCADE.

Marius chose a fixed C shim over general `extern` blocks. Lust keeps
rejecting `extern` blocks (L0810), `unsafe` (L0404), and `#[link]`
(L0522). The shim is called the way the `memchr` adapter calls
`lust_memchr`: a lust source crate invokes a named intrinsic, and
codegen emits a C call to a fixed symbol. There is no general foreign-fn
item and no new `#[link]` or `-L` machinery.

`lime-slice-core` is not in this repo and was not readable from here.
The function list below assumes Lime Slice is the Tauri slicer whose
Rust side already wants `clipper2`, `truck-stepio`, `zip`, `quick-xml`,
`sha2`, `base64`, `libm`, and `serde_json`, and that Open CASCADE would
supply the B-rep kernel `truck-stepio` does not: a solid, a boolean, a
triangle mesh, and STEP. If the real crate's first screen is different,
the first-slice rows change and the link story does not.

## 1. Shim shape

One shared library, built once with the system C++ compiler, not with
tcc and not by `scripts/build.sh`.

| | Linux | Windows |
|---|---|---|
| Compiler | g++ or clang++ | the compiler that built OCCT, usually MSVC `cl`, or `clang-cl` against that OCCT |
| Product | `liblust_occt.so` | `lust_occt.dll` (and its import lib, used only by the rustc oracle) |
| OCCT | dynamic libs, found via rpath recorded in the shim | the OCCT DLLs next to `lust_occt.dll`, or on `PATH` |

The public header is C. Every exported function is `extern "C"`,
`LUST_OCCT_API`, and wrapped in `try { ... } catch (...)`. A C++
exception never leaves the shim. The status returns are `int`: `0` ok,
nonzero a stable code. The message sits in a thread-local buffer read
by `lust_occt_last_error`.

Handles are `uint64_t` ids into tables owned by the shim. The lust heap
and the OCCT heap do not free each other's memory. Inputs are borrowed
for the call (pointer plus length). Mesh bytes are copied into a buffer
the caller allocated. That is the boundary in ARCH §3.7, narrowed to
this one library.

Suggested surface, all scalars, pointers, and explicit sizes:

```c
/* 0 ok, nonzero status. Message from lust_occt_last_error. */
int lust_occt_session_new(uint64_t *out_session);
void lust_occt_session_free(uint64_t session);

int lust_occt_make_box(uint64_t session, double dx, double dy, double dz, uint64_t *out_shape);
int lust_occt_make_cylinder(uint64_t session, double radius, double height, uint64_t *out_shape);
void lust_occt_shape_free(uint64_t session, uint64_t shape);

/* Later than the first slice. */
int lust_occt_fuse(uint64_t session, uint64_t a, uint64_t b, uint64_t *out_shape);
int lust_occt_cut(uint64_t session, uint64_t a, uint64_t b, uint64_t *out_shape);
int lust_occt_common(uint64_t session, uint64_t a, uint64_t b, uint64_t *out_shape);

int lust_occt_read_step(uint64_t session, const uint8_t *path, uint64_t path_len, uint64_t *out_shape);
int lust_occt_write_step(uint64_t session, uint64_t shape, const uint8_t *path, uint64_t path_len);

/* linear_deflection is OCCT's BRepMesh deflection. */
int lust_occt_tessellate(uint64_t session, uint64_t shape, double linear_deflection, uint64_t *out_mesh);
int lust_occt_mesh_counts(uint64_t mesh, uint64_t *n_verts, uint64_t *n_tris);
/* verts: n_verts * 3 f64, xyz. tris: n_tris * 3 u32, zero-based. */
int lust_occt_mesh_copy(uint64_t mesh, double *verts, uint32_t *tris);
void lust_occt_mesh_free(uint64_t mesh);

const char *lust_occt_last_error(void);
```

Inside the C++ file the first slice maps to `BRepPrimAPI_MakeBox`,
`BRepPrimAPI_MakeCylinder`, `BRepMesh_IncrementalMesh`, and
`BRep_Tool::Triangulation`. Fuse, cut, and common map to
`BRepAlgoAPI_Fuse`, `BRepAlgoAPI_Cut`, and `BRepAlgoAPI_Common`. STEP
maps to `STEPControl_Reader` and `STEPControl_Writer`. Those names are
the OCCT 7.x API. The cache key below pins the OCCT version so a later
OCCT cannot silently change the mesh.

Lust side, when this is implemented, follows `memchr` and does not
follow `deps.h`. `deps.h` prototypes are pasted into every generated
unit (`src/runtime/deps.h`). An OCCT prototype there would cost every
program compile time, so it does not go there. The pieces are:

- A small lust crate, on the same pattern as `library/deps/memchr`,
  whose functions call `core::intrinsics::occt_*`.
- One `IntrinsicDef` row per function in `src/ty/lang.rs`,
  `allowed` set to that crate only, `c_symbol` the `lust_occt_*` name.
  User code that does not depend on the crate never resolves the name.
- Codegen emits the call only when the intrinsic is lowered.
- The loader unit below is emitted only in that same case.

No `ShippedCrate` arm runs unless that crate is actually compiled.
Adding the rows is a compiler change and has to be remeasured (section
3). Phase 1 does not add them.

## 2. What the first slice needs

| Function | OCCT | First slice |
|---|---|---|
| `session_new` / `session_free` | runtime init, message buffer | yes |
| `make_box` | `BRepPrimAPI_MakeBox` | yes |
| `make_cylinder` | `BRepPrimAPI_MakeCylinder` | yes |
| `tessellate` + `mesh_counts` + `mesh_copy` + `mesh_free` | `BRepMesh_IncrementalMesh`, `Poly_Triangulation` | yes |
| `last_error` | catch handler | yes |
| `fuse` / `cut` / `common` | `BRepAlgoAPI_*` | no |
| `read_step` / `write_step` | `STEPControl_Reader` / `Writer` | no |
| `shape_free` | shape table | yes, the box and cylinder allocate |

The first slice is a closed experiment: a box and a cylinder, meshed,
with vertex count, triangle count, and a hash of the vertex bytes
checked against the rustc caller of the same shim. It does not need a
STEP file, a boolean, or TKSTEP. Those three are the next experiment
once the loader and the oracle are real, and they are what a slicer
would add to stand in for `truck-stepio`. Clipper, 3MF (`quick-xml` +
`zip`), and hashing stay as they are. This shim does not replace them.

## 3. Build, cache, link, oracle, compile time

### Build

`scripts/build-occt-shim.sh` (not written in this phase) is a manual
script. It is not called from `scripts/build.sh` or from CI.

Linux: `g++ -shared -fPIC -O2` of `shim.cc`, linked to the OCCT import
set the first slice needs (`TKernel`, `TKMath`, `TKBRep`, `TKPrim`,
`TKMesh`, `TKTopAlgo`, and their dependencies). `-Wl,-rpath,$OCCT_LIB`
is recorded in the `.so` so lust does not pass `-L` or `-rpath` later.

Windows: the same `shim.cc` built as a DLL by the OCCT toolchain. tcc
never compiles the C++ and never links OCCT.

### Cache

The script writes

```text
$LUST_OCCT_CACHE/<key>/liblust_occt.so      # or lust_occt.dll
$LUST_OCCT_CACHE/<key>/build-id.txt
```

`<key>` is the hash of the shim sources, the OCCT version string, the
target triple, and the C++ compiler id. A miss does not start an OCCT
build. A program that calls the shim and finds no cache gets a lust
diagnostic that names the script. A program that does not call the shim
does not look at the cache.

### Link, without new link-line machinery

When, and only when, lowering emits a `lust_occt_*` call, codegen also
emits one extra C unit: a loader. tcc compiles that unit like any other
generated unit and links the resulting object with the program. The
loader defines the `lust_occt_*` symbols. Each one `dlopen`s (Linux) or
`LoadLibraryW`s (Windows) the cached shim on first use and
`dlsym`/`GetProcAddress` the same name. The path is the cache path
baked into that unit, or `LUST_OCCT_SHIM` when set.

The compiler's link step does not gain `tcc_add_library`, a search
path, or a `#[link]` attribute. OCCT's own DT_NEEDED / DLL dependencies
are recorded in the shim, so they load with it.

The loader is not pasted into `runtime_source` and not declared from
`deps.h`. Unused programs do not emit the unit, so they do not compile
it and do not link it.

### Oracle

A single Rust source cannot be the oracle. rustc calls the shim with
`extern "C"`, and lust rejects that form. The check is a same-shim
oracle, the same idea as the memchr adapter versus the real crate:

1. The rustc bin links `liblust_occt` (or loads it) and calls
   `lust_occt_make_box`, `lust_occt_make_cylinder`, and
   `lust_occt_tessellate` / `mesh_copy`.
2. The lust bin is the adapter crate calling the intrinsics. Codegen
   emits calls to those same symbols, satisfied by the loader.
3. Both print the status, the two counts, and the raw little-endian
   vertex and index bytes (or a hash of those bytes). The committed
   stdout is the rustc run. Lust must match it byte for byte.

Tessellation is deterministic for one OCCT build and one deflection.
The oracle pins that build through the cache key and runs it
single-threaded. It is not a promise that two OCCT versions mesh alike.

The oracle stays out of `scripts/ci.sh` until a runner image actually
has the cache. Missing OCCT skips the probe. It does not fail the gate.

### Compile and edit time for programs that do not use it

Target: no change in the compiler-reported edit time or cold `total`
of a program that does not depend on the crate.

That holds only if all of the following stay true:

- The loader unit and its declarations are emitted only after an OCCT
  intrinsic is lowered.
- The OCCT shared library is not on the default link line and is not
  compiled by `scripts/build.sh`.
- The cache directory is not statted while resolving an ordinary crate.
- No OCCT header or object is added to `runtime_source` or `deps.h`.

The `IntrinsicDef` rows and one `allowed` crate are a compiler rebuild,
the same class of change as registering `memchr`. They are not on the
per-function path of a program that never names the crate. After the
rows exist, remeasure a non-user program the way the libm and
quick-xml edits were remeasured (alternating runs, warmup dropped,
median and spread). If the median moves, the rows or the emit check
are on a hot path and the change stops.

Phase 1 adds none of those rows. This document does not change the
compiler.

## 4. Risks

**OCCT is not in CI.** A full OCCT source build is too large and too
slow for the lust gate. Phase 1 does not add it. Later, the oracle
runs only on a machine that already has a cached shim. Linux and
Windows CI stay green without it. macOS stays disabled, as now. OCCT's
license (LGPL-2.1 with the OCCT exception) is a distribution choice for
whoever ships the shim, not something this repo vendors.

**Build time of the shim.** The shim `.cc` itself is small. Linking it
pulls a large OCCT shared-library set. The first configuration of
include and library paths is the slow part, and it happens outside
lust, once per OCCT version. A miss in the cache must not fall back to
compiling OCCT inside a user compile.

**C++ exceptions.** OCCT reports many failures by throwing. If one
escapes, it crosses a tcc C frame or a rustc frame and aborts. Every
export is a C function with a catch-all, a status code, and
`lust_occt_last_error`. Constructors that throw must not leak a
half-registered id. Destructors run inside the `try`. The lust panic
runtime is not involved. A panic on the lust side must not unwind into
the shim. The first slice has no callback from OCCT into lust, so the
ARCH §3.7 callback trampoline is out of scope. Adding one is a new
plan, not a follow-up patch on this loader.

**Other ABI traps, not for the first slice.** `bool`, `c_long`,
structs by value, and C++ names are not in the header. Mesh doubles are
plain `double`. A later boolean or STEP call stays on this C ABI.
OCCT and the shim must be built by the same C++ toolchain. The tcc
loader only calls `dlopen` / `LoadLibraryW`, so it does not have to
match that CRT.

**Windows DLL search.** `LoadLibraryW` uses the DLL's directory, then
`PATH`. The cache layout keeps `lust_occt.dll` and the OCCT DLLs it
needs in one directory. A rustc oracle that links the import lib must
use the same directory.

## What this phase does not do

No shim source, no script, no intrinsic, no CI job, no ARCH table row
claiming the adapter exists. The next implementation step, if this
plan is accepted, is the loader-unit rule and the first-slice C
functions, still without OCCT in CI, and a non-user edit remeasure
before the intrinsic rows land.
