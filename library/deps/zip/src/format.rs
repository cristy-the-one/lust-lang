//! Compression method and host system, the two enums a writer names.

use std::fmt;

/// How an entry's bytes are stored. Only stored and deflate are written.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum CompressionMethod {
    /// Method 0.
    Stored,
    /// Method 8.
    Deflated,
}

impl CompressionMethod {
    /// The two-byte method id stored in the headers.
    pub(crate) fn id(self) -> u16 {
        match self {
            Self::Stored => 0,
            Self::Deflated => 8,
        }
    }
}

impl Default for CompressionMethod {
    fn default() -> Self {
        // `deflate-flate2` is on, so upstream's default is Deflated.
        CompressionMethod::Deflated
    }
}

impl fmt::Display for CompressionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stored => "Stored",
            Self::Deflated => "Deflated",
        })
    }
}

impl fmt::Debug for CompressionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Host system in the high byte of "version made by".
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum System {
    /// MS-DOS. Upstream's default when `cfg!(windows)`.
    Dos = 0,
    /// Unix. Upstream's default on every other host.
    Unix = 3,
}

impl System {
    pub(crate) fn from_byte(b: u8) -> System {
        if b == 3 { System::Unix } else { System::Dos }
    }
}
