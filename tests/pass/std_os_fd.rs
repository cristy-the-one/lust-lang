// std::os::fd and std::os::unix::ffi (tf-adapters F1): fds print
// only booleans, never raw numbers (the next free fd differs by
// harness). FromRawFd needs an unsafe block in valid Rust, so it
// is not here (lust rejects unsafe); adapters use it internally.
use std::ffi::{CStr, OsStr, OsString};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, IntoRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

fn main() {
    let dir = std::env::temp_dir().join(format!("lust-fd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("f.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"abc").unwrap();
    let raw = f.as_raw_fd();
    println!("{}", raw >= 0 && f.as_fd().as_raw_fd() == raw);
    let owned: OwnedFd = f.into();
    println!("{}", format!("{:?}", owned).starts_with("OwnedFd { fd: "));
    println!("{}", format!("{:?}", owned.as_fd()).starts_with("BorrowedFd { fd: "));
    let dup = owned.try_clone().unwrap();
    println!("{}", dup.as_fd().as_raw_fd() >= 0 && dup.as_raw_fd() != owned.as_raw_fd());
    let back = std::fs::File::from(owned);
    println!("{}", back.as_raw_fd() == raw);
    let raw2 = back.into_raw_fd();
    println!("{}", raw2 == raw && std::path::Path::new(&format!("/proc/self/fd/{raw2}")).exists());
    drop(dup);
    println!("{}", std::io::stdout().as_raw_fd() == 1 && std::io::stderr().as_fd().as_raw_fd() == 2);
    println!("{}", std::fs::read_to_string(&path).unwrap());

    let os = OsStr::from_bytes(b"a\xffb");
    println!("{:?} {:?}", os.as_bytes(), os);
    let owned_os = OsString::from_vec(vec![b'x', 0xff, b'y']);
    println!("{:?}", owned_os.clone().into_vec());
    println!("{}", owned_os.to_string_lossy());

    let c = CStr::from_bytes_with_nul(b"a\xffb\n\"\\\0").unwrap();
    println!("{:?} {:?} {}", c.to_bytes(), c, c.is_empty());
    println!("{:?}", CStr::from_bytes_with_nul(b"").unwrap_err());
    println!("{}", CStr::from_bytes_with_nul(b"abc").unwrap_err());
    println!("{:?}", CStr::from_bytes_with_nul(b"a\0b\0").unwrap_err());
    println!("{:?}", CStr::from_bytes_with_nul(b"a\0b").unwrap_err());
    let empty = CStr::from_bytes_with_nul(b"\0").unwrap();
    println!("{} {:?} {:?}", empty.is_empty(), empty.to_bytes_with_nul(), empty.to_string_lossy());
    std::fs::remove_dir_all(&dir).unwrap();
}
