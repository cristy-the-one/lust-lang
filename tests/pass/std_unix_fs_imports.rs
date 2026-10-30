// std::os::unix::fs is a partial platform module (it defines
// only MetadataExt): its other items keep the std compat
// handling, so every import below resolves (fx-stdos; the
// goport-util slices import OpenOptionsExt, FileTypeExt, and
// a function-local PermissionsExt exactly like this). The
// compat traits and fns are imported, not called: lust does
// not model their methods yet.
use std::os::unix::fs::{chown, fchown, lchown, symlink, DirBuilderExt, DirEntryExt, FileExt, FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::fs::MetadataExt;

fn local_import() {
    use std::os::unix::fs::PermissionsExt as _;
}

fn main() {
    local_import();
    let m = std::fs::metadata(".").unwrap();
    println!("{}", m.mode() & 0o170000 == 0o040000);
    println!("{}", m.ino() > 0);
    println!("ok");
}
