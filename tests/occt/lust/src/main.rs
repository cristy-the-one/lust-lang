//! Lust side of the OCCT oracle. Same steps as tests/occt/rustc_main.rs.
fn print_bytes(label: &str, bytes: &Vec<u8>) {
    print!("{}", label);
    for b in bytes.iter() {
        print!(" {}", *b);
    }
    println!();
}

fn main() {
    let (st, session) = occt::session_new();
    println!("session {} {}", st, session);

    let (st, box_s) = occt::make_box(session, 10.0, 20.0, 30.0);
    println!("box {} {}", st, box_s);
    let (st, box_m) = occt::tessellate(session, box_s, 0.5);
    println!("box_tess {} {}", st, box_m);
    let (st, nv, nt) = occt::mesh_counts(box_m);
    println!("box_mesh {} {} {}", st, nv, nt);
    let mut box_verts = Vec::new();
    let mut box_tris = Vec::new();
    let st = occt::mesh_copy(box_m, &mut box_verts, &mut box_tris);
    println!("box_copy {}", st);
    print_bytes("box_verts", &box_verts);
    print_bytes("box_tris", &box_tris);

    let (st, cyl) = occt::make_cylinder(session, 5.0, 12.0);
    println!("cyl {} {}", st, cyl);
    let (st, cyl_m) = occt::tessellate(session, cyl, 0.5);
    println!("cyl_tess {} {}", st, cyl_m);
    let (st, nv, nt) = occt::mesh_counts(cyl_m);
    println!("cyl_mesh {} {} {}", st, nv, nt);
    let mut cyl_verts = Vec::new();
    let mut cyl_tris = Vec::new();
    let st = occt::mesh_copy(cyl_m, &mut cyl_verts, &mut cyl_tris);
    println!("cyl_copy {}", st);
    print_bytes("cyl_verts", &cyl_verts);
    print_bytes("cyl_tris", &cyl_tris);

    let (st, junk) = occt::make_box(session, 0.0, 1.0, 1.0);
    println!("badbox {} {}", st, junk);
    print_bytes("badbox_error", &occt::last_error());
    let (st, junk) = occt::make_box(0, 1.0, 1.0, 1.0);
    println!("badhandle {} {}", st, junk);
    print_bytes("badhandle_error", &occt::last_error());

    let _ = occt::mesh_free(box_m);
    println!("free_box_mesh 0");
    let _ = occt::mesh_free(cyl_m);
    println!("free_cyl_mesh 0");
    let _ = occt::shape_free(session, box_s);
    println!("free_box 0");
    let _ = occt::shape_free(session, cyl);
    println!("free_cyl 0");
    let _ = occt::session_free(session);
    println!("free_session 0");
}
