// Lust adapter for flate2 1.1.10 (docs/ARCH.md section 3.6).
// The read side: `read::GzDecoder`, `read::MultiGzDecoder`,
// `read::ZlibDecoder`, and `read::DeflateDecoder` decode with
// a pure-Rust inflate in this crate, with flate2's error kinds
// and messages for the corrupt forms. Decoding is canonical,
// so every valid stream gives upstream's bytes.
//
// The write side: `write::GzEncoder`, `write::ZlibEncoder`, and
// `write::DeflateEncoder`, a port of upstream's encoders over a copy
// of miniz_oxide 0.9.1's deflate source (`miniz/`, the backend
// upstream's default `rust_backend` uses), so the compressed bytes
// equal upstream's. The `bufread` module and `GzBuilder` are absent
// and reject (README).
//
// The first read pulls the whole input through (the decoders
// ts-rust feeds are byte slices); a decode error still returns
// the bytes decoded before it first, as upstream's streaming
// decoder does at read_to_end.

// Upstream builds only with a backend; of the features this adapter
// takes, `miniz_oxide` (also implied by `rust_backend` and `default`)
// is the one that gives it one.
#[cfg(not(feature = "miniz_oxide"))]
compile_error!("flate2 needs a backend feature (`rust_backend` or `miniz_oxide`)");

pub mod read;
pub mod write;

mod inflate;
mod miniz;

/// Compression level, as `write::GzEncoder` takes it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Compression(u32);

/// Upstream's default level is 6.
impl Default for Compression {
    fn default() -> Compression {
        Compression(6)
    }
}

impl Compression {
    /// A level from 0 (none) to 9 (best), kept raw as upstream's
    /// own field does. The encoder passes it to miniz as upstream
    /// does: 10 is miniz's slowest level, a level above 255 falls
    /// back to 1, and a level above 10 fails upstream's
    /// `debug_assert!` in builds with debug assertions.
    pub const fn new(level: u32) -> Compression {
        Compression(level)
    }

    /// No compression (level 0).
    pub const fn none() -> Compression {
        Compression(0)
    }

    /// Fast compression (level 1).
    pub const fn fast() -> Compression {
        Compression(1)
    }

    /// Best compression (level 9).
    pub const fn best() -> Compression {
        Compression(9)
    }

    /// The level number (not a const fn upstream).
    pub fn level(&self) -> u32 {
        self.0
    }
}

/// The io error of one corrupt-input kind, with flate2's text.
fn invalid(msg: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, msg)
}

/// flate2's truncated-stream error.
fn eof_err() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "unexpected end of file")
}

/// IEEE CRC32 of `data`, table-driven.
fn crc32(data: &[u8], start: u32) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }
    let mut crc = !start;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

/// Adler-32 of `data` continuing from `start`.
fn adler32(data: &[u8], start: u32) -> u32 {
    let mut a = start & 0xffff;
    let mut b = (start >> 16) & 0xffff;
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
