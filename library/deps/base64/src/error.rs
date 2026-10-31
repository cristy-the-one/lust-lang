// `base64::DecodeError`, with upstream's Debug and Display text.

use core::fmt;

/// Why a decode failed. The byte in [`InvalidByte`] and [`InvalidLastSymbol`]
/// is the offending input byte; the `usize` is its offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// A byte that is not in the alphabet, or a `=` that is not trailing padding.
    InvalidByte(usize, u8),
    /// Fewer than two alphabet symbols in the last group.
    InvalidLength(usize),
    /// Unused low bits of the last symbol were set.
    InvalidLastSymbol(usize, u8),
    /// Padding was missing, partial, or present where it is forbidden.
    InvalidPadding,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            DecodeError::InvalidByte(index, byte) => write!(f, "Invalid symbol {}, offset {}.", byte, index),
            DecodeError::InvalidLength(len) => write!(f, "Invalid input length: {}", len),
            DecodeError::InvalidLastSymbol(index, byte) => write!(f, "Invalid last symbol {}, offset {}.", byte, index),
            DecodeError::InvalidPadding => write!(f, "Invalid padding"),
        }
    }
}

impl std::error::Error for DecodeError {}
