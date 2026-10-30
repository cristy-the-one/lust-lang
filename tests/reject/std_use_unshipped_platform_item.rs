// expect: L0505
// A std item the shipped platform module lacks (`std::os::unix::fs`
// has MetadataExt and the compat rows only) exists in std all the
// same: L0505, not L0506.
use std::os::unix::fs::chroot;

fn main() {
    println!("hi");
}
