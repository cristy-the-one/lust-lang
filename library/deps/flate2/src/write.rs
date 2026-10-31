// flate2 1.1.10's `write::GzEncoder`, ported line for line from
// upstream gz/write.rs, zio.rs (`Writer`), mem.rs (`Compress`), crc.rs,
// and ffi/miniz_oxide.rs (`Deflate`), over the miniz_oxide deflate
// source in `crate::miniz`. The order and size of every write to the
// inner writer, the 32 KiB output buffer, the flush and finish loops,
// and both Drop impls (the encoder's and the inner Writer's) are
// upstream's, so the bytes and the write pattern the inner writer sees
// are the same, also when it fails or returns short writes.
//
// `write::ZlibEncoder` is upstream's zlib/write.rs over the same Writer
// with a zlib-wrapped `Compress`. `write::DeflateEncoder` is the same
// Writer with a raw `Compress` (no zlib header, no Adler trailer), which
// is the encoder `zip` writes Deflated entries with. No `Debug` (upstream
// derives it over its internal state), no `GzBuilder`, no `reset` or
// `new_with_compress` (no public `Compress`). The read and bufread
// encoders are not here. Code that names them rejects.

use std::io::{self, Read, Write};

use crate::Compression;
use crate::miniz::deflate::core::CompressorOxide;
use crate::miniz::{DataFormat, MZError, MZFlush, MZStatus};

/// Upstream's flush modes (mem.rs `FlushCompress`) that the Writer uses.
#[derive(Copy, Clone, PartialEq, Eq)]
enum FlushCompress {
    None,
    Sync,
    Finish,
}

impl From<FlushCompress> for MZFlush {
    fn from(value: FlushCompress) -> Self {
        match value {
            FlushCompress::None => Self::None,
            FlushCompress::Sync => Self::Sync,
            FlushCompress::Finish => Self::Finish,
        }
    }
}

/// Upstream's mem.rs `Status`.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Status {
    Ok,
    BufError,
    StreamEnd,
}

/// The miniz backend's error message: always empty (ffi/miniz_oxide.rs).
#[derive(Clone, Default)]
struct ErrorMessage;

impl ErrorMessage {
    fn get(&self) -> Option<&str> {
        None
    }
}

/// Upstream ffi/mod.rs: prints `get()`, so `None`.
impl std::fmt::Debug for ErrorMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

/// Upstream's mem.rs `CompressError`, private here; an io::Error made from
/// it prints as upstream's does (Display and Debug: `CompressError { msg: None }`).
/// `Default` is for the
/// std hook below; upstream has none (the type is not public here).
#[derive(Clone, Debug, Default)]
struct CompressError {
    msg: ErrorMessage,
}

impl std::fmt::Display for CompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.msg.get() {
            Some(msg) => write!(f, "deflate compression error: {msg}"),
            None => write!(f, "deflate compression error"),
        }
    }
}

impl std::error::Error for CompressError {}

/// Prints a `CompressError` (it has no data) for the io::Error hook.
fn compress_error_display(f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    std::fmt::Display::fmt(&CompressError::default(), f)
}

fn compress_error_debug(f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    std::fmt::Debug::fmt(&CompressError::default(), f)
}

impl From<CompressError> for io::Error {
    // Upstream: `io::Error::new(io::ErrorKind::Other, data)`. Lust's
    // io::Error::new takes text payloads only; this std hook keeps the
    // payload's own Display and Debug (`CompressError { msg: None }`).
    fn from(data: CompressError) -> io::Error {
        let _ = data;
        io::Error::__lust_adapter_error(io::ErrorKind::Other, compress_error_display, compress_error_debug)
    }
}

/// Upstream's `Compress` over its miniz `Deflate` backend.
struct Compress {
    inner: Box<CompressorOxide>,
    total_in: u64,
    total_out: u64,
}

impl Compress {
    /// A deflate stream at `level`: raw (gzip wraps it itself), or with
    /// the zlib header and Adler-32 trailer.
    fn new(level: Compression, zlib_header: bool) -> Compress {
        debug_assert!(level.level() <= 10);
        let mut inner: Box<CompressorOxide> = Box::default();
        let format = if zlib_header { DataFormat::Zlib } else { DataFormat::Raw };
        inner.set_format_and_level(format, level.level().try_into().unwrap_or(1));
        Compress { inner, total_in: 0, total_out: 0 }
    }

    fn compress(&mut self, input: &[u8], output: &mut [u8], flush: FlushCompress) -> Result<Status, CompressError> {
        let res = crate::miniz::deflate::stream::deflate(&mut self.inner, input, output, flush.into());
        self.total_in += res.bytes_consumed as u64;
        self.total_out += res.bytes_written as u64;
        match res.status {
            Ok(status) => match status {
                MZStatus::Ok => Ok(Status::Ok),
                MZStatus::StreamEnd => Ok(Status::StreamEnd),
                MZStatus::NeedDict => Err(CompressError { msg: ErrorMessage }),
            },
            Err(status) => match status {
                MZError::Buf => Ok(Status::BufError),
                _ => Err(CompressError { msg: ErrorMessage }),
            },
        }
    }
}

/// The capacity of upstream's Writer buffer (`Vec::with_capacity(32 * 1024)`).
/// Upstream compresses into the buffer's spare capacity and never grows it.
const BUF_CAP: usize = 32 * 1024;

/// Upstream's zio.rs `Writer<W, Compress>`.
struct Writer<W: Write> {
    obj: Option<W>,
    data: Compress,
    buf: Vec<u8>,
    /// Stands in for `buf`'s spare capacity; zeroed before each use, as
    /// upstream's `initialize_buffer` does.
    spare: Vec<u8>,
}

impl<W: Write> Writer<W> {
    fn new(w: W, d: Compress) -> Writer<W> {
        Writer { obj: Some(w), data: d, buf: Vec::with_capacity(BUF_CAP), spare: vec![0; BUF_CAP] }
    }

    /// Upstream's `Compress::compress_vec` into `buf`'s spare capacity.
    fn run_vec(&mut self, input: &[u8], flush: FlushCompress) -> Result<Status, CompressError> {
        let len = self.buf.len();
        let out = &mut self.spare[..BUF_CAP - len];
        out.fill(0);
        let before = self.data.total_out;
        let ret = self.data.compress(input, out, flush);
        let bytes_written = (self.data.total_out - before) as usize;
        let n = (BUF_CAP - len).min(bytes_written);
        self.buf.extend_from_slice(&self.spare[..n]);
        ret
    }

    fn finish(&mut self) -> io::Result<()> {
        loop {
            self.dump()?;
            let before = self.data.total_out;
            self.run_vec(&[], FlushCompress::Finish)?;
            if before == self.data.total_out {
                return Ok(());
            }
        }
    }

    fn get_ref(&self) -> &W {
        self.obj.as_ref().unwrap()
    }

    fn get_mut(&mut self) -> &mut W {
        self.obj.as_mut().unwrap()
    }

    fn take_inner(&mut self) -> W {
        self.obj.take().unwrap()
    }

    fn is_present(&self) -> bool {
        self.obj.is_some()
    }

    fn write_with_status(&mut self, buf: &[u8]) -> io::Result<(usize, Status)> {
        loop {
            self.dump()?;
            let before_in = self.data.total_in;
            let ret = self.run_vec(buf, FlushCompress::None);
            let written = (self.data.total_in - before_in) as usize;
            let is_stream_end = matches!(ret, Ok(Status::StreamEnd));
            if !buf.is_empty() && written == 0 && ret.is_ok() && !is_stream_end {
                continue;
            }
            return match ret {
                Ok(st) => match st {
                    Status::Ok | Status::BufError | Status::StreamEnd => Ok((written, st)),
                },
                Err(_) => Err(io::Error::new(io::ErrorKind::InvalidInput, "corrupt deflate stream")),
            };
        }
    }

    fn dump(&mut self) -> io::Result<()> {
        while !self.buf.is_empty() {
            let n = self.obj.as_mut().unwrap().write(&self.buf)?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            self.buf.drain(..n);
        }
        Ok(())
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_with_status(buf).map(|res| res.0)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.run_vec(&[], FlushCompress::Sync)?;
        loop {
            self.dump()?;
            let before = self.data.total_out;
            self.run_vec(&[], FlushCompress::None)?;
            if before == self.data.total_out {
                break;
            }
        }
        self.obj.as_mut().unwrap().flush()
    }
}

impl<W: Write> Drop for Writer<W> {
    fn drop(&mut self) {
        if self.obj.is_some() {
            let _ = self.finish();
        }
    }
}

/// IEEE CRC-32 table (the value crc32fast computes).
const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

/// Upstream's crc.rs `Crc`: the running CRC-32 and the byte count mod 2^32.
struct Crc {
    amt: u32,
    sum: u32,
}

impl Crc {
    fn update(&mut self, data: &[u8]) {
        self.amt = self.amt.wrapping_add(data.len() as u32);
        let mut crc = !self.sum;
        for &b in data {
            crc = CRC_TABLE[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
        }
        self.sum = !crc;
    }
}

// Non-gzip writer paths flush through zio::Writer::dump, which converts
// Ok(0) on a non-empty buffer into WriteZero. Gzip writes its header and footer
// directly, so keep the same progress rule here.
fn write_nonzero<W: Write>(writer: &mut W, buf: &[u8]) -> io::Result<usize> {
    let n = writer.write(buf)?;
    if n == 0 && !buf.is_empty() { Err(io::ErrorKind::WriteZero.into()) } else { Ok(n) }
}

/// A gzip encoder, or compressor.
///
/// This structure exposes a [`Write`] interface that will emit compressed data
/// to the underlying writer `W`.
pub struct GzEncoder<W: Write> {
    inner: Writer<W>,
    crc: Crc,
    crc_bytes_written: usize,
    header: Vec<u8>,
}

/// The header `GzBuilder::new().into_header(lvl)` builds: no extra field,
/// name, or comment; mtime 0; OS 255.
fn default_header(lvl: Compression) -> Vec<u8> {
    let mut header = vec![0u8; 10];
    header[0] = 0x1f;
    header[1] = 0x8b;
    header[2] = 8;
    header[8] = if lvl.0 >= Compression::best().0 {
        2
    } else if lvl.0 <= Compression::fast().0 {
        4
    } else {
        0
    };
    header[9] = 255;
    header
}

impl<W: Write> GzEncoder<W> {
    /// Creates a new encoder which will use the given compression level.
    ///
    /// The encoder is not configured specially for the emitted header. For
    /// header configuration, see the `GzBuilder` type.
    pub fn new(w: W, level: Compression) -> GzEncoder<W> {
        GzEncoder {
            inner: Writer::new(w, Compress::new(level, false)),
            crc: Crc { amt: 0, sum: 0 },
            header: default_header(level),
            crc_bytes_written: 0,
        }
    }

    /// Acquires a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        self.inner.get_ref()
    }

    /// Acquires a mutable reference to the underlying writer.
    ///
    /// Note that mutation of the writer may result in surprising results if
    /// this encoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut W {
        self.inner.get_mut()
    }

    /// Attempt to finish this output stream, writing out final chunks of data.
    pub fn try_finish(&mut self) -> io::Result<()> {
        self.write_header()?;
        self.inner.finish()?;

        while self.crc_bytes_written < 8 {
            let (sum, amt) = (self.crc.sum, self.crc.amt);
            let buf = [
                sum as u8,
                (sum >> 8) as u8,
                (sum >> 16) as u8,
                (sum >> 24) as u8,
                amt as u8,
                (amt >> 8) as u8,
                (amt >> 16) as u8,
                (amt >> 24) as u8,
            ];
            let inner = self.inner.get_mut();
            let n = write_nonzero(inner, &buf[self.crc_bytes_written..])?;
            self.crc_bytes_written += n;
        }
        Ok(())
    }

    /// Finish encoding this stream, returning the underlying writer once the
    /// encoding is done.
    pub fn finish(mut self) -> io::Result<W> {
        self.try_finish()?;
        Ok(self.inner.take_inner())
    }

    fn write_header(&mut self) -> io::Result<()> {
        while !self.header.is_empty() {
            let n = write_nonzero(self.inner.get_mut(), &self.header)?;
            self.header.drain(..n);
        }
        Ok(())
    }
}

impl<W: Write> Write for GzEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        assert_eq!(self.crc_bytes_written, 0);
        self.write_header()?;
        let n = self.inner.write(buf)?;
        self.crc.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        assert_eq!(self.crc_bytes_written, 0);
        self.write_header()?;
        self.inner.flush()
    }
}

impl<R: Read + Write> Read for GzEncoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.get_mut().read(buf)
    }
}

impl<W: Write> Drop for GzEncoder<W> {
    fn drop(&mut self) {
        if self.inner.is_present() {
            let _ = self.try_finish();
        }
    }
}

/// A ZLIB encoder, or compressor.
///
/// This structure implements a [`Write`] interface and takes a stream of
/// uncompressed data, writing the compressed data to the wrapped writer.
pub struct ZlibEncoder<W: Write> {
    inner: Writer<W>,
}

impl<W: Write> ZlibEncoder<W> {
    /// Creates a new encoder which will write compressed data to the stream
    /// given at the given compression level.
    ///
    /// When this encoder is dropped or unwrapped the final pieces of data will
    /// be flushed.
    pub fn new(w: W, level: Compression) -> ZlibEncoder<W> {
        ZlibEncoder { inner: Writer::new(w, Compress::new(level, true)) }
    }

    /// Acquires a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        self.inner.get_ref()
    }

    /// Acquires a mutable reference to the underlying writer.
    ///
    /// Note that mutating the output/input state of the stream may corrupt this
    /// object, so care must be taken when using this method.
    pub fn get_mut(&mut self) -> &mut W {
        self.inner.get_mut()
    }

    /// Attempt to finish this output stream, writing out final chunks of data.
    pub fn try_finish(&mut self) -> io::Result<()> {
        self.inner.finish()
    }

    /// Consumes this encoder, flushing the output stream.
    pub fn finish(mut self) -> io::Result<W> {
        self.inner.finish()?;
        Ok(self.inner.take_inner())
    }

    /// Consumes this encoder, flushing the output stream (a sync flush, not
    /// the end of the stream).
    pub fn flush_finish(mut self) -> io::Result<W> {
        self.inner.flush()?;
        Ok(self.inner.take_inner())
    }

    /// Returns the number of bytes that have been written to this compressor.
    pub fn total_in(&self) -> u64 {
        self.inner.data.total_in
    }

    /// Returns the number of bytes that the compressor has produced.
    pub fn total_out(&self) -> u64 {
        self.inner.data.total_out
    }
}

impl<W: Write> Write for ZlibEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Read + Write> Read for ZlibEncoder<W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.get_mut().read(buf)
    }
}

/// A raw DEFLATE encoder, or compressor.
///
/// This structure implements a [`Write`] interface and takes a stream of
/// uncompressed data, writing the compressed data to the wrapped writer.
/// The bytes are raw deflate: no zlib header and no Adler-32 trailer.
pub struct DeflateEncoder<W: Write> {
    inner: Writer<W>,
}

impl<W: Write> DeflateEncoder<W> {
    /// Creates a new encoder which will write compressed data to the stream
    /// given at the given compression level.
    ///
    /// When this encoder is dropped or unwrapped the final pieces of data will
    /// be flushed.
    pub fn new(w: W, level: Compression) -> DeflateEncoder<W> {
        DeflateEncoder { inner: Writer::new(w, Compress::new(level, false)) }
    }

    /// Acquires a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        self.inner.get_ref()
    }

    /// Acquires a mutable reference to the underlying writer.
    ///
    /// Note that mutating the output/input state of the stream may corrupt this
    /// object, so care must be taken when using this method.
    pub fn get_mut(&mut self) -> &mut W {
        self.inner.get_mut()
    }

    /// Attempt to finish this output stream, writing out final chunks of data.
    pub fn try_finish(&mut self) -> io::Result<()> {
        self.inner.finish()
    }

    /// Consumes this encoder, flushing the output stream.
    pub fn finish(mut self) -> io::Result<W> {
        self.inner.finish()?;
        Ok(self.inner.take_inner())
    }

    /// Returns the number of bytes that have been written to this compressor.
    pub fn total_in(&self) -> u64 {
        self.inner.data.total_in
    }

    /// Returns the number of bytes that the compressor has produced.
    pub fn total_out(&self) -> u64 {
        self.inner.data.total_out
    }
}

impl<W: Write> Write for DeflateEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Read + Write> Read for DeflateEncoder<W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.get_mut().read(buf)
    }
}
