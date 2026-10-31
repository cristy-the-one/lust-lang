//! The `ZipError` strings the reader and writer return.

use std::fmt;
use std::io;

/// `Result` with [`ZipError`] as the error.
pub type ZipResult<T> = Result<T, ZipError>;

/// An error from reading or writing a zip archive.
pub enum ZipError {
    /// An I/O error, printed as `i/o error: …`.
    Io(io::Error),
    /// A malformed archive or a rejected write, printed as `invalid Zip archive: …`.
    InvalidArchive(String),
    /// A compression level or method this adapter does not write.
    UnsupportedArchive(&'static str),
    /// `by_name` / `by_index` did not find the entry.
    FileNotFound,
    /// A compression method id this reader does not decode.
    CompressionMethodNotSupported(u16),
}

impl fmt::Debug for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "i/o error: {e}"),
            Self::InvalidArchive(e) => write!(f, "invalid Zip archive: {e}"),
            Self::UnsupportedArchive(e) => write!(f, "unsupported Zip archive: {e}"),
            Self::FileNotFound => f.write_str("specified file not found in archive"),
            Self::CompressionMethodNotSupported(id) => {
                write!(f, "compression method not supported: {id}")
            }
        }
    }
}

impl From<io::Error> for ZipError {
    fn from(e: io::Error) -> ZipError {
        ZipError::Io(e)
    }
}

pub(crate) fn invalid(message: String) -> ZipError {
    ZipError::InvalidArchive(message)
}
