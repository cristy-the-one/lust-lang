// `std::os::unix::fs::symlink`: creates the link, and a second
// creation at the same path is std's AlreadyExists error.
use std::os::unix::fs::symlink;

fn main() {
    let dir = std::env::temp_dir().join(format!("lust-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("f.txt");
    std::fs::write(&f, "hi").unwrap();
    let l = dir.join("l.txt");
    symlink(&f, &l).unwrap();
    println!("{} {}", std::fs::symlink_metadata(&l).unwrap().is_symlink(), std::fs::read_link(&l).unwrap() == f);
    println!("{:?}", symlink(&f, &l).map_err(|e| e.kind()));
    println!("{}", std::fs::read_to_string(&l).unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}
