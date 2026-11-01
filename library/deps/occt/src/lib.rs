//! First slice of the Open CASCADE shim (docs/occt-shim.md).
//!
//! Handles are ids in the shim. `0` means the call failed; the status
//! and `last_error` say why. Mesh bytes are the shim's native `f64`
//! vertices (`n * 24` bytes) and `u32` triangle indexes (`n * 12`
//! bytes). `last_error` copies the message, because the shim reuses
//! its buffer on the next call.

pub fn session_new() -> (i32, u64) {
    let h = core::intrinsics::occt_session_new();
    (core::intrinsics::occt_last_status(), h)
}

pub fn session_free(session: u64) -> i32 {
    core::intrinsics::occt_session_free(session);
    core::intrinsics::occt_last_status()
}

pub fn make_box(session: u64, dx: f64, dy: f64, dz: f64) -> (i32, u64) {
    let h = core::intrinsics::occt_make_box(session, dx, dy, dz);
    (core::intrinsics::occt_last_status(), h)
}

pub fn make_cylinder(session: u64, radius: f64, height: f64) -> (i32, u64) {
    let h = core::intrinsics::occt_make_cylinder(session, radius, height);
    (core::intrinsics::occt_last_status(), h)
}

pub fn shape_free(session: u64, shape: u64) -> i32 {
    core::intrinsics::occt_shape_free(session, shape);
    core::intrinsics::occt_last_status()
}

pub fn tessellate(session: u64, shape: u64, linear_deflection: f64) -> (i32, u64) {
    let h = core::intrinsics::occt_tessellate(session, shape, linear_deflection);
    (core::intrinsics::occt_last_status(), h)
}

pub fn mesh_counts(mesh: u64) -> (i32, u64, u64) {
    let nv = core::intrinsics::occt_mesh_nverts(mesh);
    let st = core::intrinsics::occt_last_status();
    if st != 0 {
        return (st, 0, 0);
    }
    let nt = core::intrinsics::occt_mesh_ntris(mesh);
    (core::intrinsics::occt_last_status(), nv, nt)
}

pub fn mesh_copy(mesh: u64, verts: &mut Vec<u8>, tris: &mut Vec<u8>) -> i32 {
    core::intrinsics::occt_mesh_copy(mesh, verts, tris)
}

pub fn mesh_free(mesh: u64) -> i32 {
    core::intrinsics::occt_mesh_free(mesh);
    core::intrinsics::occt_last_status()
}

pub fn last_error() -> Vec<u8> {
    let s = core::intrinsics::occt_last_error();
    let mut v = Vec::new();
    let mut i = 0;
    while i < s.len() {
        v.push(s[i]);
        i += 1;
    }
    v
}
