// Windows stand-in for the Linux front image. The image is a heap snapshot at
// fixed addresses; Windows has no `fork` and no `ADDR_NO_RANDOMIZE`, so a build
// here is always cold in a fresh process. The daemon still keeps warm sessions
// in memory (src/daemon.rs). The allocator is mimalloc, as on Linux outside
// image mode.
use std::alloc::{GlobalAlloc, Layout};
use std::path::{Path, PathBuf};

pub const STATE_ENV: &str = "LUST_FRONT_STATE";
pub const SAVE_ENV: &str = "LUST_FRONT_SAVE";

pub struct Alloc;

unsafe impl GlobalAlloc for Alloc {
    #[inline]
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        unsafe { mimalloc::MiMalloc.alloc(l) }
    }
    #[inline]
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        unsafe { mimalloc::MiMalloc.alloc_zeroed(l) }
    }
    #[inline]
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { mimalloc::MiMalloc.dealloc(p, l) }
    }
    #[inline]
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        unsafe { mimalloc::MiMalloc.realloc(p, l, n) }
    }
}

/// Image mode is Linux-only. A set `LUST_FRONT_STATE` on Windows is ignored
/// and the build runs cold; the variable is not an error.
pub fn state_dir() -> Option<PathBuf> {
    None
}

pub fn exit_fast(code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}

pub fn arena_on() -> bool {
    false
}

pub fn reexec(_dir: &Path) -> String {
    "front images need Linux address-space controls (personality and a fixed mmap)".into()
}

pub fn process_start() {}

pub struct Roots {
    pub sessions: Vec<Box<crate::driver::Session>>,
    pub tables: crate::diag::GlobalTables,
    pub stamps: crate::driver::Stamps,
    pub graphs: crate::resolve::cargo::GraphCache,
}

/// Text packing uses libzstd, which the Linux image opens with `dlopen`.
/// Windows builds keep C texts plain. `pack_texts` checks `packs` first.
pub fn packs() -> bool {
    false
}

pub fn pack_bytes(_src: &[u8], _level: i32) -> Option<Box<[u8]>> {
    None
}

pub fn unpack_bytes(_frame: &[u8], raw_len: usize) -> Vec<u8> {
    let _ = raw_len;
    panic!("packed C texts are produced only when libzstd is available");
}

pub fn preload(_dir: &Path) {}

pub fn preloaded() -> Option<Result<Box<Roots>, String>> {
    None
}

pub fn load(_dir: &Path) -> Result<Box<Roots>, String> {
    Err("front images are Linux-only".into())
}

pub fn restored_freed() -> usize {
    0
}

pub fn touched_note() -> Option<String> {
    None
}

pub fn generation() -> usize {
    0
}

pub fn layers_note() -> String {
    "no image on Windows".into()
}

pub fn lock(_dir: &Path) {}

pub fn compact_mode() -> bool {
    false
}

pub fn compact_lock(_dir: &Path) -> bool {
    false
}

pub fn wait_for_build() {}

pub fn allow_compaction(_on: bool) {}

pub fn refuse_compaction(dir: &Path, why: &str) {
    let _ = std::fs::write(dir.join("nocompact"), format!("{why}\n"));
}

pub fn save_background(_dir: &Path, _roots: Box<Roots>) -> Result<String, String> {
    Err("background image saves fork, which Windows does not have".into())
}

pub fn save(_dir: &Path, _roots: Box<Roots>) -> Result<String, String> {
    Err("front images are Linux-only".into())
}

pub fn discard(dir: &Path) {
    let _ = std::fs::remove_file(dir.join("front.img"));
}
