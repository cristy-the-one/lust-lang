//! rustc side of the OCCT oracle. Same steps as tests/occt/lust/src/main.rs.
//! Links liblust_occt directly. Lust calls the same shim through the loader.
use std::ffi::CStr;
use std::os::raw::c_char;

unsafe extern "C" {
    fn lust_occt_session_new(out_session: *mut u64) -> i32;
    fn lust_occt_session_free(session: u64);
    fn lust_occt_make_box(session: u64, dx: f64, dy: f64, dz: f64, out_shape: *mut u64) -> i32;
    fn lust_occt_make_cylinder(session: u64, radius: f64, height: f64, out_shape: *mut u64) -> i32;
    fn lust_occt_shape_free(session: u64, shape: u64);
    fn lust_occt_tessellate(session: u64, shape: u64, linear_deflection: f64, out_mesh: *mut u64) -> i32;
    fn lust_occt_mesh_counts(mesh: u64, n_verts: *mut u64, n_tris: *mut u64) -> i32;
    fn lust_occt_mesh_copy(mesh: u64, verts: *mut f64, tris: *mut u32) -> i32;
    fn lust_occt_mesh_free(mesh: u64);
    fn lust_occt_last_error() -> *const c_char;
}

fn print_bytes(label: &str, bytes: &[u8]) {
    print!("{label}");
    for b in bytes {
        print!(" {b}");
    }
    println!();
}

fn error_bytes() -> Vec<u8> {
    unsafe {
        let p = lust_occt_last_error();
        if p.is_null() {
            return Vec::new();
        }
        CStr::from_ptr(p).to_bytes().to_vec()
    }
}

fn main() {
    unsafe {
        let mut session = 0u64;
        let st = lust_occt_session_new(&mut session);
        println!("session {st} {session}");

        let mut box_s = 0u64;
        let st = lust_occt_make_box(session, 10.0, 20.0, 30.0, &mut box_s);
        println!("box {st} {box_s}");
        let mut box_m = 0u64;
        let st = lust_occt_tessellate(session, box_s, 0.5, &mut box_m);
        println!("box_tess {st} {box_m}");
        let mut nv = 0u64;
        let mut nt = 0u64;
        let st = lust_occt_mesh_counts(box_m, &mut nv, &mut nt);
        println!("box_mesh {st} {nv} {nt}");
        let mut verts = vec![0f64; nv as usize * 3];
        let mut tris = vec![0u32; nt as usize * 3];
        let st = lust_occt_mesh_copy(box_m, verts.as_mut_ptr(), tris.as_mut_ptr());
        println!("box_copy {st}");
        print_bytes("box_verts", std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 8));
        print_bytes("box_tris", std::slice::from_raw_parts(tris.as_ptr() as *const u8, tris.len() * 4));

        let mut cyl = 0u64;
        let st = lust_occt_make_cylinder(session, 5.0, 12.0, &mut cyl);
        println!("cyl {st} {cyl}");
        let mut cyl_m = 0u64;
        let st = lust_occt_tessellate(session, cyl, 0.5, &mut cyl_m);
        println!("cyl_tess {st} {cyl_m}");
        let st = lust_occt_mesh_counts(cyl_m, &mut nv, &mut nt);
        println!("cyl_mesh {st} {nv} {nt}");
        verts = vec![0f64; nv as usize * 3];
        tris = vec![0u32; nt as usize * 3];
        let st = lust_occt_mesh_copy(cyl_m, verts.as_mut_ptr(), tris.as_mut_ptr());
        println!("cyl_copy {st}");
        print_bytes("cyl_verts", std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 8));
        print_bytes("cyl_tris", std::slice::from_raw_parts(tris.as_ptr() as *const u8, tris.len() * 4));

        let mut junk = 0u64;
        let st = lust_occt_make_box(session, 0.0, 1.0, 1.0, &mut junk);
        println!("badbox {st} {junk}");
        print_bytes("badbox_error", &error_bytes());
        let st = lust_occt_make_box(0, 1.0, 1.0, 1.0, &mut junk);
        println!("badhandle {st} {junk}");
        print_bytes("badhandle_error", &error_bytes());

        lust_occt_mesh_free(box_m);
        println!("free_box_mesh 0");
        lust_occt_mesh_free(cyl_m);
        println!("free_cyl_mesh 0");
        lust_occt_shape_free(session, box_s);
        println!("free_box 0");
        lust_occt_shape_free(session, cyl);
        println!("free_cyl 0");
        lust_occt_session_free(session);
        println!("free_session 0");
    }
}
