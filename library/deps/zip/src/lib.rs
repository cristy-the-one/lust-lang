// Lust adapter for zip 9.2.0 (docs/ARCH.md section 3.6).
//
// The slice Lime Slice writes: `ZipWriter` stored and Deflated entries
// (directories included), then `ZipArchive` reading them back. Deflated
// bytes come from `flate2::write::DeflateEncoder` (the miniz port), which
// is what zip 9.2 produces with `default-features = false` and
// `features = ["deflate-flate2"]`. The `time` feature is off, so the
// timestamp is 1980-01-01. AES, zip64, data descriptors, and the other
// compression methods are not here (README).

#[cfg(not(feature = "deflate-flate2"))]
compile_error!("the lust zip adapter implements zip with the deflate-flate2 feature");

pub mod read;
pub mod result;
pub mod write;

mod format;

pub use crate::format::{CompressionMethod, System};
pub use crate::read::ZipArchive;
pub use crate::write::ZipWriter;
