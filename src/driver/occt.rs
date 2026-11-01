//! The Open CASCADE loader unit (docs/occt-shim.md).
//!
//! Emitted only when a compile has lowered an `occt_*` intrinsic.
//! The unit `dlopen`s / `LoadLibraryW`s the cached shim. It is not
//! part of `runtime_source` and not declared from `deps.h`. A program
//! that never calls the adapter does not build this unit and does not
//! stat the cache.

use std::path::PathBuf;

/// Where `scripts/build-occt-shim.sh` writes `current`.
pub fn cache_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("LUST_OCCT_CACHE")
        && !d.is_empty()
    {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    PathBuf::from(home).join(".cache").join("lust").join("occt")
}

/// The shim library named by `LUST_OCCT_SHIM` or `$LUST_OCCT_CACHE/current`.
/// Called only when a program actually uses the adapter.
pub fn find_shim() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("LUST_OCCT_SHIM")
        && !p.is_empty()
    {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "LUST_OCCT_SHIM={} is not a file. Build the shim with scripts/build-occt-shim.sh (docs/occt-shim.md)",
            path.display()
        ));
    }
    let current = cache_dir().join("current");
    if let Ok(text) = std::fs::read_to_string(&current) {
        let path = PathBuf::from(text.trim());
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "Open CASCADE shim not found. Build it with scripts/build-occt-shim.sh (docs/occt-shim.md), then set LUST_OCCT_SHIM or write the library path to {}",
        current.display()
    ))
}

/// C source of the loader. `windows` selects `LoadLibraryW`.
pub fn loader_source(windows: bool) -> Result<String, String> {
    let shim = find_shim()?;
    let path = shim.to_str().ok_or_else(|| format!("shim path {} is not UTF-8", shim.display()))?;
    if path.contains('"') || path.contains('\n') || path.contains('\0') {
        return Err(format!("shim path {} cannot be baked into the loader", shim.display()));
    }
    let escaped = path.replace('\\', "\\\\");
    let body = if windows { LOADER_WIN } else { LOADER_POSIX };
    Ok(body.replace("@SHIM@", &escaped))
}

const LOADER_POSIX: &str = r#"
/* Open CASCADE loader. Linked only into a program that calls the adapter. */
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { void *ptr; uint64_t len; uint64_t cap; } lust_Vec;
typedef struct { uint8_t *ptr; uint64_t len; } lust_str;
void lust_vec_grow_to(lust_Vec *v, uint64_t new_cap, uint64_t elem, uint64_t align);

static void *lib;
static int ready;
static int32_t g_status;
static char g_load_err[512];
typedef int (*fn_inew)(uint64_t *);
typedef void (*fn_v1)(uint64_t);
typedef int (*fn_box)(uint64_t, double, double, double, uint64_t *);
typedef int (*fn_cyl)(uint64_t, double, double, uint64_t *);
typedef void (*fn_sfree)(uint64_t, uint64_t);
typedef int (*fn_tess)(uint64_t, uint64_t, double, uint64_t *);
typedef int (*fn_counts)(uint64_t, uint64_t *, uint64_t *);
typedef int (*fn_copy)(uint64_t, double *, uint32_t *);
typedef const char *(*fn_err)(void);
static fn_inew p_session_new;
static fn_v1 p_session_free, p_mesh_free;
static fn_box p_box;
static fn_cyl p_cyl;
static fn_sfree p_shape_free;
static fn_tess p_tess;
static fn_counts p_counts;
static fn_copy p_copy;
static fn_err p_err;

static int ensure(void) {
    if (ready) return ready > 0;
    ready = -1;
    lib = dlopen("@SHIM@", RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        snprintf(g_load_err, sizeof g_load_err, "dlopen failed: %s", dlerror());
        g_status = 6;
        return 0;
    }
#define BIND(slot, name, ty) slot = (ty)dlsym(lib, name); if (!slot) { snprintf(g_load_err, sizeof g_load_err, "dlsym %s failed", name); g_status = 6; return 0; }
    BIND(p_session_new, "lust_occt_session_new", fn_inew)
    BIND(p_session_free, "lust_occt_session_free", fn_v1)
    BIND(p_box, "lust_occt_make_box", fn_box)
    BIND(p_cyl, "lust_occt_make_cylinder", fn_cyl)
    BIND(p_shape_free, "lust_occt_shape_free", fn_sfree)
    BIND(p_tess, "lust_occt_tessellate", fn_tess)
    BIND(p_counts, "lust_occt_mesh_counts", fn_counts)
    BIND(p_copy, "lust_occt_mesh_copy", fn_copy)
    BIND(p_mesh_free, "lust_occt_mesh_free", fn_v1)
    BIND(p_err, "lust_occt_last_error", fn_err)
#undef BIND
    ready = 1;
    return 1;
}

static void fill(lust_Vec *v, uint64_t n) {
    if (n == 0) { v->len = 0; return; }
    if (v->cap < n) lust_vec_grow_to(v, n, 1, 8);
    v->len = n;
}

uint64_t lust_occt_h_session_new(void) {
    uint64_t id = 0;
    if (!ensure()) return 0;
    g_status = p_session_new(&id);
    return g_status == 0 ? id : 0;
}
void lust_occt_h_session_free(uint64_t s) { if (!ensure()) return; p_session_free(s); g_status = 0; }
uint64_t lust_occt_h_make_box(uint64_t s, double x, double y, double z) {
    uint64_t id = 0;
    if (!ensure()) return 0;
    g_status = p_box(s, x, y, z, &id);
    return g_status == 0 ? id : 0;
}
uint64_t lust_occt_h_make_cylinder(uint64_t s, double r, double h) {
    uint64_t id = 0;
    if (!ensure()) return 0;
    g_status = p_cyl(s, r, h, &id);
    return g_status == 0 ? id : 0;
}
void lust_occt_h_shape_free(uint64_t s, uint64_t sh) { if (!ensure()) return; p_shape_free(s, sh); g_status = 0; }
uint64_t lust_occt_h_tessellate(uint64_t s, uint64_t sh, double d) {
    uint64_t id = 0;
    if (!ensure()) return 0;
    g_status = p_tess(s, sh, d, &id);
    return g_status == 0 ? id : 0;
}
uint64_t lust_occt_h_mesh_nverts(uint64_t m) {
    uint64_t nv = 0, nt = 0;
    if (!ensure()) return 0;
    g_status = p_counts(m, &nv, &nt);
    return g_status == 0 ? nv : 0;
}
uint64_t lust_occt_h_mesh_ntris(uint64_t m) {
    uint64_t nv = 0, nt = 0;
    if (!ensure()) return 0;
    g_status = p_counts(m, &nv, &nt);
    return g_status == 0 ? nt : 0;
}
int32_t lust_occt_h_mesh_copy(uint64_t m, lust_Vec *verts, lust_Vec *tris) {
    uint64_t nv = 0, nt = 0;
    if (!ensure()) return g_status;
    g_status = p_counts(m, &nv, &nt);
    if (g_status != 0) return g_status;
    if (nv > (uint64_t)-1 / 24 || nt > (uint64_t)-1 / 12) { g_status = 5; return 5; }
    fill(verts, nv * 24);
    fill(tris, nt * 12);
    g_status = p_copy(m, nv ? (double *)verts->ptr : 0, nt ? (uint32_t *)tris->ptr : 0);
    return g_status;
}
void lust_occt_h_mesh_free(uint64_t m) { if (!ensure()) return; p_mesh_free(m); g_status = 0; }
int32_t lust_occt_h_last_status(void) { return g_status; }
lust_str lust_occt_h_last_error(void) {
    lust_str r;
    if (!ensure()) { r.ptr = (uint8_t *)g_load_err; r.len = strlen(g_load_err); return r; }
    const char *s = p_err();
    if (!s) s = "";
    r.ptr = (uint8_t *)s;
    r.len = strlen(s);
    return r;
}
"#;

const LOADER_WIN: &str = r#"
/* Open CASCADE loader (Windows). Linked only into a program that calls the adapter. */
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { void *ptr; uint64_t len; uint64_t cap; } lust_Vec;
typedef struct { uint8_t *ptr; uint64_t len; } lust_str;
void lust_vec_grow_to(lust_Vec *v, uint64_t new_cap, uint64_t elem, uint64_t align);

static HMODULE lib;
static int ready;
static int32_t g_status;
static char g_load_err[512];
typedef int (*fn_inew)(uint64_t *);
typedef void (*fn_v1)(uint64_t);
typedef int (*fn_box)(uint64_t, double, double, double, uint64_t *);
typedef int (*fn_cyl)(uint64_t, double, double, uint64_t *);
typedef void (*fn_sfree)(uint64_t, uint64_t);
typedef int (*fn_tess)(uint64_t, uint64_t, double, uint64_t *);
typedef int (*fn_counts)(uint64_t, uint64_t *, uint64_t *);
typedef int (*fn_copy)(uint64_t, double *, uint32_t *);
typedef const char *(*fn_err)(void);
static fn_inew p_session_new;
static fn_v1 p_session_free, p_mesh_free;
static fn_box p_box;
static fn_cyl p_cyl;
static fn_sfree p_shape_free;
static fn_tess p_tess;
static fn_counts p_counts;
static fn_copy p_copy;
static fn_err p_err;

static int ensure(void) {
    wchar_t wide[1024];
    int n;
    if (ready) return ready > 0;
    ready = -1;
    n = MultiByteToWideChar(CP_UTF8, 0, "@SHIM@", -1, wide, 1024);
    if (n <= 0) { snprintf(g_load_err, sizeof g_load_err, "shim path is not valid UTF-8"); g_status = 6; return 0; }
    lib = LoadLibraryW(wide);
    if (!lib) { snprintf(g_load_err, sizeof g_load_err, "LoadLibraryW failed (%lu)", (unsigned long)GetLastError()); g_status = 6; return 0; }
#define BIND(slot, name, ty) slot = (ty)GetProcAddress(lib, name); if (!slot) { snprintf(g_load_err, sizeof g_load_err, "GetProcAddress %s failed", name); g_status = 6; return 0; }
    BIND(p_session_new, "lust_occt_session_new", fn_inew)
    BIND(p_session_free, "lust_occt_session_free", fn_v1)
    BIND(p_box, "lust_occt_make_box", fn_box)
    BIND(p_cyl, "lust_occt_make_cylinder", fn_cyl)
    BIND(p_shape_free, "lust_occt_shape_free", fn_sfree)
    BIND(p_tess, "lust_occt_tessellate", fn_tess)
    BIND(p_counts, "lust_occt_mesh_counts", fn_counts)
    BIND(p_copy, "lust_occt_mesh_copy", fn_copy)
    BIND(p_mesh_free, "lust_occt_mesh_free", fn_v1)
    BIND(p_err, "lust_occt_last_error", fn_err)
#undef BIND
    ready = 1;
    return 1;
}

static void fill(lust_Vec *v, uint64_t n) {
    if (n == 0) { v->len = 0; return; }
    if (v->cap < n) lust_vec_grow_to(v, n, 1, 8);
    v->len = n;
}

uint64_t lust_occt_h_session_new(void) {
    uint64_t id = 0;
    if (!ensure()) return 0;
    g_status = p_session_new(&id);
    return g_status == 0 ? id : 0;
}
void lust_occt_h_session_free(uint64_t s) { if (!ensure()) return; p_session_free(s); g_status = 0; }
uint64_t lust_occt_h_make_box(uint64_t s, double x, double y, double z) {
    uint64_t id = 0;
    if (!ensure()) return 0;
    g_status = p_box(s, x, y, z, &id);
    return g_status == 0 ? id : 0;
}
uint64_t lust_occt_h_make_cylinder(uint64_t s, double r, double h) {
    uint64_t id = 0;
    if (!ensure()) return 0;
    g_status = p_cyl(s, r, h, &id);
    return g_status == 0 ? id : 0;
}
void lust_occt_h_shape_free(uint64_t s, uint64_t sh) { if (!ensure()) return; p_shape_free(s, sh); g_status = 0; }
uint64_t lust_occt_h_tessellate(uint64_t s, uint64_t sh, double d) {
    uint64_t id = 0;
    if (!ensure()) return 0;
    g_status = p_tess(s, sh, d, &id);
    return g_status == 0 ? id : 0;
}
uint64_t lust_occt_h_mesh_nverts(uint64_t m) {
    uint64_t nv = 0, nt = 0;
    if (!ensure()) return 0;
    g_status = p_counts(m, &nv, &nt);
    return g_status == 0 ? nv : 0;
}
uint64_t lust_occt_h_mesh_ntris(uint64_t m) {
    uint64_t nv = 0, nt = 0;
    if (!ensure()) return 0;
    g_status = p_counts(m, &nv, &nt);
    return g_status == 0 ? nt : 0;
}
int32_t lust_occt_h_mesh_copy(uint64_t m, lust_Vec *verts, lust_Vec *tris) {
    uint64_t nv = 0, nt = 0;
    if (!ensure()) return g_status;
    g_status = p_counts(m, &nv, &nt);
    if (g_status != 0) return g_status;
    if (nv > (uint64_t)-1 / 24 || nt > (uint64_t)-1 / 12) { g_status = 5; return 5; }
    fill(verts, nv * 24);
    fill(tris, nt * 12);
    g_status = p_copy(m, nv ? (double *)verts->ptr : 0, nt ? (uint32_t *)tris->ptr : 0);
    return g_status;
}
void lust_occt_h_mesh_free(uint64_t m) { if (!ensure()) return; p_mesh_free(m); g_status = 0; }
int32_t lust_occt_h_last_status(void) { return g_status; }
lust_str lust_occt_h_last_error(void) {
    lust_str r;
    if (!ensure()) { r.ptr = (uint8_t *)g_load_err; r.len = strlen(g_load_err); return r; }
    const char *s = p_err();
    if (!s) s = "";
    r.ptr = (uint8_t *)s;
    r.len = strlen(s);
    return r;
}
"#;
