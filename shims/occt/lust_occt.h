/* First-slice C ABI for the Open CASCADE shim (docs/occt-shim.md).
   Every export is extern "C". The C++ file wraps each one in
   try/catch so a C++ exception never crosses into tcc or rustc.
   Handles are uint64_t ids. 0 is never a valid id.
   Status: 0 ok, 1 OCCT exception, 2 std::exception, 3 unknown
   exception, 4 bad handle or null out-pointer, 5 the operation did
   not finish. The message is lust_occt_last_error (thread-local).
   Mesh vertices are n*3 little-endian f64 on this ABI's host
   (native double). Triangles are n*3 little-endian u32, zero-based. */
#ifndef LUST_OCCT_H
#define LUST_OCCT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(_WIN32)
#  define LUST_OCCT_API __declspec(dllexport)
#else
#  define LUST_OCCT_API __attribute__((visibility("default")))
#endif

LUST_OCCT_API int lust_occt_session_new(uint64_t *out_session);
LUST_OCCT_API void lust_occt_session_free(uint64_t session);

LUST_OCCT_API int lust_occt_make_box(uint64_t session, double dx, double dy, double dz, uint64_t *out_shape);
LUST_OCCT_API int lust_occt_make_cylinder(uint64_t session, double radius, double height, uint64_t *out_shape);
LUST_OCCT_API void lust_occt_shape_free(uint64_t session, uint64_t shape);

LUST_OCCT_API int lust_occt_tessellate(uint64_t session, uint64_t shape, double linear_deflection, uint64_t *out_mesh);
LUST_OCCT_API int lust_occt_mesh_counts(uint64_t mesh, uint64_t *n_verts, uint64_t *n_tris);
LUST_OCCT_API int lust_occt_mesh_copy(uint64_t mesh, double *verts, uint32_t *tris);
LUST_OCCT_API void lust_occt_mesh_free(uint64_t mesh);

LUST_OCCT_API const char *lust_occt_last_error(void);

#ifdef __cplusplus
}
#endif

#endif
