// PermissionsExt (mode, set_mode, from_mode) by method and by path, fs::set_permissions (chmod), and std's Mode Debug (the ls form only for a known file type).
use std::os::unix::fs::PermissionsExt;
fn main() { more();
    let d = std::env::temp_dir().join(format!("pm-{}", std::process::id()));
    std::fs::write(&d, b"1").unwrap();
    let mut p = std::fs::metadata(&d).unwrap().permissions();
    p.set_mode(0o640);
    println!("{:o}", p.mode() & 0o777);
    std::os::unix::fs::PermissionsExt::set_mode(&mut p, 0o600);
    PermissionsExt::set_mode(&mut p, 0o644);
    <std::fs::Permissions as PermissionsExt>::set_mode(&mut p, 0o604);
    println!("{:o} {:o}", p.mode() & 0o777, PermissionsExt::mode(&p) & 0o777);
    std::fs::set_permissions(&d, p).unwrap();
    println!("{:o}", std::fs::metadata(&d).unwrap().permissions().mode() & 0o777);
    std::fs::remove_file(&d).unwrap();
}
fn more() {
    use std::os::unix::fs::PermissionsExt as PE;
    let p = <std::fs::Permissions as PE>::from_mode(0o100755);
    println!("{:?} {:o} {}", p, p.mode(), p.readonly());
    let q = std::fs::Permissions::from_mode(0o444);
    println!("{:#?} {}", q, q.readonly());
    let r = std::fs::set_permissions("/nonexistent/x", q.clone());
    println!("{:?}", r.map_err(|e| e.kind()));
    let mut big = q.clone(); big.set_mode(u32::MAX); println!("{:o} {:?}", big.mode(), big);
}
