// std::os::unix::fs::MetadataExt: the raw stat fields of a
// file the program makes (identity fields compared, never
// printed raw).
use std::os::unix::fs::MetadataExt;

fn main() {
    let dir = std::env::temp_dir().join("lust_md_ext_case");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    std::fs::write(&a, "hello world").unwrap();
    std::fs::write(&b, "hello world").unwrap();
    let ma = std::fs::metadata(&a).unwrap();
    let mb = std::fs::metadata(&b).unwrap();
    let ml = std::fs::metadata(dir.join("./a.txt")).unwrap();
    println!("{}", ma.dev() == mb.dev());
    println!("{}", ma.ino() == ml.ino() && ma.ino() != mb.ino());
    println!("{} {}", ma.size(), ma.len());
    println!("{}", ma.nlink());
    println!("{}", ma.mode() & 0o170000 == 0o100000);
    println!("{}", ma.uid() == mb.uid() && ma.gid() == mb.gid());
    println!("{}", ma.rdev() == 0);
    println!("{}", ma.mtime() > 0 && ma.mtime_nsec() >= 0 && ma.atime() > 0 && ma.ctime() > 0);
    println!("{}", ma.blksize() > 0);
    let f = std::fs::File::open(&a).unwrap();
    println!("{}", f.metadata().unwrap().ino() == ma.ino());
    let _ = std::fs::remove_dir_all(&dir);
}
