// `std::fs::DirBuilder` with `os::unix::fs::DirBuilderExt::mode` (ts-rust
// os_mkdir_all): plain and recursive creates, the mode, and the errors.
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
fn main() {
    let base = std::env::temp_dir().join(format!("dbt-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let mut b = fs::DirBuilder::new();
    println!("{:?}", b.create(&base).is_ok());
    println!("{:?}", b.create(&base).map_err(|e| e.kind()));
    let deep = base.join("a/b/c");
    println!("{:?}", b.create(&deep).map_err(|e| e.kind()));
    b.recursive(true);
    println!("{:?}", b.create(&deep).is_ok());
    println!("{:?}", b.create(&deep).is_ok());
    let m = base.join("m");
    let mut b2 = fs::DirBuilder::new();
    DirBuilderExt::mode(&mut b2, 0o700);
    b2.create(&m).unwrap();
    println!("{:o}", fs::metadata(&m).unwrap().mode() & 0o777);
    let f = base.join("file");
    fs::write(&f, b"x").unwrap();
    println!("{:?}", fs::DirBuilder::new().recursive(true).create(&f).map_err(|e| e.kind()));
    println!("{:?}", fs::DirBuilder::new().recursive(true).create(f.join("sub")).map_err(|e| e.kind()));
    println!("{:?}", fs::DirBuilder::new().recursive(true).create("").is_ok());
    fs::remove_dir_all(&base).unwrap();
}
