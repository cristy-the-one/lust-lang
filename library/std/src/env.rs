// `std::env` (W-SHIM): environment variables, program arguments as
// `OsString`s, and the current directory, as on Linux.
//
// `args()` and `args().count()` keep the compiler's own std handling;
// this module adds the items that need the OS strings. Setting and
// removing variables are `unsafe fn` in edition 2024 and stay out.

use crate::ffi::{OsStr, OsString};
use crate::io;
use crate::path::{Path, PathBuf};
use core::fmt;

/// Why `var` failed.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum VarError {
    /// The variable is not set.
    NotPresent,
    /// The variable is set, but its value is not valid Unicode.
    NotUnicode(OsString),
}

impl fmt::Display for VarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VarError::NotPresent => f.write_str("environment variable not found"),
            VarError::NotUnicode(s) => write!(f, "environment variable was not valid unicode: {:?}", s),
        }
    }
}

/// The value of the variable `key` as an `OsString`, None when unset.
pub fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
    let key: &OsStr = key.as_ref();
    let mut value: Vec<u8> = Vec::new();
    if core::intrinsics::sys_getenv(key.as_encoded_bytes(), &mut value) { Some(OsString::__from_vec(value)) } else { None }
}

/// The value of the variable `key` as a `String`.
pub fn var<K: AsRef<OsStr>>(key: K) -> Result<String, VarError> {
    match var_os(key) {
        Some(s) => match s.into_string() {
            Ok(text) => Ok(text),
            Err(s) => Err(VarError::NotUnicode(s)),
        },
        None => Err(VarError::NotPresent),
    }
}

/// The NUL-terminated entries of a runtime list, as byte vectors.
fn split_nul(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut start = 0;
    for i in 0..bytes.len() {
        if bytes[i] == 0 {
            out.push(bytes[start..i].to_vec());
            start = i + 1;
        }
    }
    out
}

/// The environment as (name, value) pairs, as std's Unix parser reads
/// `environ`: the name ends at the first `=` after its first byte, and
/// entries without one are skipped.
fn env_pairs() -> Vec<(OsString, OsString)> {
    let mut raw: Vec<u8> = Vec::new();
    core::intrinsics::sys_environ(&mut raw);
    let mut out: Vec<(OsString, OsString)> = Vec::new();
    for entry in split_nul(&raw) {
        if entry.is_empty() {
            continue;
        }
        let mut pos = 1;
        while pos < entry.len() && entry[pos] != b'=' {
            pos += 1;
        }
        if pos < entry.len() {
            out.push((OsString::__from_vec(entry[..pos].to_vec()), OsString::__from_vec(entry[pos + 1..].to_vec())));
        }
    }
    out
}

/// An iterator over the environment as `OsString` pairs (`vars_os`).
pub struct VarsOs {
    pairs: Vec<(OsString, OsString)>,
    next: usize,
}

impl Iterator for VarsOs {
    type Item = (OsString, OsString);

    fn next(&mut self) -> Option<(OsString, OsString)> {
        if self.next < self.pairs.len() {
            self.next += 1;
            Some(self.pairs[self.next - 1].clone())
        } else {
            None
        }
    }
}

/// An iterator over the environment as `String` pairs (`vars`). It
/// panics on a name or value that is not valid Unicode, as std does.
pub struct Vars {
    inner: VarsOs,
}

impl Iterator for Vars {
    type Item = (String, String);

    fn next(&mut self) -> Option<(String, String)> {
        match self.inner.next() {
            Some((k, v)) => {
                let k = k.into_string().unwrap(); //@ std/src/env.rs:168:57
                let v = v.into_string().unwrap(); //@ std/src/env.rs:168:83
                Some((k, v))
            }
            None => None,
        }
    }
}


/// The environment when called, as `OsString` pairs.
pub fn vars_os() -> VarsOs {
    VarsOs { pairs: env_pairs(), next: 0 }
}

/// The environment when called, as `String` pairs.
pub fn vars() -> Vars {
    Vars { inner: vars_os() }
}

/// An iterator over the program arguments as `OsString`s (`args_os`).
pub struct ArgsOs {
    args: Vec<Vec<u8>>,
    next: usize,
}

impl Iterator for ArgsOs {
    type Item = OsString;

    fn next(&mut self) -> Option<OsString> {
        if self.next < self.args.len() {
            self.next += 1;
            Some(OsString::__from_vec(self.args[self.next - 1].clone()))
        } else {
            None
        }
    }
}

/// The program arguments, the program name first, as `OsString`s.
pub fn args_os() -> ArgsOs {
    let mut raw: Vec<u8> = Vec::new();
    core::intrinsics::sys_argv(&mut raw);
    ArgsOs { args: split_nul(&raw), next: 0 }
}

/// The error of a path argument that holds a NUL byte.
fn nul_error() -> io::Error {
    io::Error::simple_message(
        io::ErrorKind::InvalidInput,
        if cfg!(windows) { "strings passed to WinAPI cannot contain NULs" } else { "file name contained an unexpected NUL byte" },
    )
}

/// The current working directory.
pub fn current_dir() -> io::Result<PathBuf> {
    let mut buf: Vec<u8> = Vec::new();
    match core::intrinsics::sys_getcwd(&mut buf) {
        0 => Ok(PathBuf::from_os_string(OsString::__from_vec(buf))),
        code => Err(io::Error::from_raw_os_error(code)),
    }
}

/// Changes the current working directory.
pub fn set_current_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path: &Path = path.as_ref();
    match core::intrinsics::sys_chdir(path.as_os_str().as_encoded_bytes()) {
        0 => Ok(()),
        -1 => Err(nul_error()),
        code => Err(io::Error::from_raw_os_error(code)),
    }
}

/// The directory for temporary files.
/// Unix: `TMPDIR`, else `/tmp`.
/// Windows: `GetTempPath` — `TMP`, then `TEMP`, then `USERPROFILE`,
/// then the Windows directory — made absolute, with a trailing separator.
pub fn temp_dir() -> PathBuf {
    if cfg!(windows) {
        let raw = var_os("TMP").or_else(|| var_os("TEMP")).or_else(|| var_os("USERPROFILE"));
        let mut path = match raw {
            Some(dir) => PathBuf::from_os_string(dir),
            None => PathBuf::from_os_string(OsString::__from_vec(b"C:\\Windows".to_vec())),
        };
        if !path.is_absolute() {
            if let Ok(cwd) = current_dir() {
                path = cwd.join(path);
            }
        }
        let mut bytes = path.into_os_string().into_encoded_bytes();
        if bytes.last() != Some(&b'\\') && bytes.last() != Some(&b'/') {
            bytes.push(b'\\');
        }
        return PathBuf::from_os_string(OsString::__from_vec(bytes));
    }
    match var_os("TMPDIR") {
        Some(dir) => PathBuf::from_os_string(dir),
        None => PathBuf::from_os_string(OsString::__from_vec(b"/tmp".to_vec())),
    }
}

/// The path of the running executable (`/proc/self/exe` on Linux,
/// `_NSGetExecutablePath` on macOS; the runtime picks).
pub fn current_exe() -> io::Result<PathBuf> {
    let mut buf: Vec<u8> = Vec::new();
    match core::intrinsics::sys_current_exe(&mut buf) {
        0 => Ok(PathBuf::from_os_string(OsString::__from_vec(buf))),
        code => {
            // std's Linux form: a /proc with no self/exe is its own message.
            if cfg!(target_os = "linux") && code == 2 {
                Err(io::Error::simple_message(io::ErrorKind::__Uncategorized, "no /proc/self/exe available. Is /proc mounted?"))
            } else {
                Err(io::Error::from_raw_os_error(code))
            }
        }
    }
}

/// Constants of the target (`std::env::consts`). Only the values lust
/// builds for are defined: another target leaves a name out, so a use
/// rejects instead of printing a wrong value.
pub mod consts {
    #[cfg(target_os = "linux")]
    pub const OS: &str = "linux";
    #[cfg(target_os = "macos")]
    pub const OS: &str = "macos";
    #[cfg(target_os = "windows")]
    pub const OS: &str = "windows";
    #[cfg(target_os = "linux")]
    pub const FAMILY: &str = "unix";
    #[cfg(target_os = "macos")]
    pub const FAMILY: &str = "unix";
    #[cfg(target_os = "windows")]
    pub const FAMILY: &str = "windows";
    #[cfg(target_arch = "x86_64")]
    pub const ARCH: &str = "x86_64";
    #[cfg(target_arch = "aarch64")]
    pub const ARCH: &str = "aarch64";
}
