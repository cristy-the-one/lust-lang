// Front-image persistence (rel-front). The implementation maps a fixed address
// range and re-executes with ASLR off, which is a Linux process feature
// (`personality(ADDR_NO_RANDOMIZE)`, `mmap(MAP_FIXED)`). On Windows the same
// requests compile without an image: `state_dir` is `None`, and the daemon's
// in-process sessions are the incremental path.
#[cfg(not(windows))]
#[path = "image_unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "image_win.rs"]
mod imp;

pub use imp::*;
