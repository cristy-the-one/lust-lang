//! `ZipWriter` for stored and Deflated entries.
//!
//! Headers match zip 9.2.0 with `deflate-flate2` and without `time`:
//! version-needed 10 for a stored file and 20 for deflate or a directory,
//! version-made-by `(system << 8) | version-needed`, flags 0 except bit 11
//! for a non-ASCII name, MS-DOS time 1980-01-01, and Unix mode in the high
//! half of the external attributes (`0o100644` for a file, `0o040755` for
//! a directory). Deflate bytes are one `flate2::write::DeflateEncoder`
//! stream of the entry, finished when the entry closes. Level `None` is 6.
//! Levels outside 1..=9, and any level on a stored entry, are
//! `Unsupported compression level`.

use std::io::{self, Seek, Write};

use crate::format::{CompressionMethod, System};
use crate::result::{self, ZipError, ZipResult};

const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const FILE_MODE: u32 = 0o644 | S_IFREG;
const DIR_MODE: u32 = 0o755 | S_IFDIR;
/// 1980-01-01 in the MS-DOS date field (`DateTime::DEFAULT`).
const DOS_DATE: u16 = 0x0021;

/// Options for one entry. `SimpleFileOptions` upstream.
#[derive(Clone)]
pub struct FileOptions {
    method: CompressionMethod,
    level: Option<i64>,
    system: Option<System>,
    /// Permission bits from `unix_permissions` (`mode & 0o777`), or `None`.
    permissions: Option<u32>,
}

/// The copyable options a simple archive uses.
pub type SimpleFileOptions = FileOptions;

impl Default for FileOptions {
    fn default() -> Self {
        FileOptions {
            method: CompressionMethod::default(),
            level: None,
            system: None,
            permissions: None,
        }
    }
}

impl FileOptions {
    /// Stored or Deflated. The default is Deflated.
    pub fn compression_method(mut self, method: CompressionMethod) -> Self {
        self.method = method;
        self
    }

    /// `None` is the default level (6 for deflate). A stored entry rejects any level.
    pub fn compression_level(mut self, level: Option<i64>) -> Self {
        self.level = level;
        self
    }

    /// Host system written in the high byte of version-made-by.
    /// `None` follows the host: DOS on Windows, Unix elsewhere.
    pub fn system(mut self, system: System) -> Self {
        self.system = Some(system);
        self
    }

    /// Unix permission bits. The file type is added when the entry starts
    /// (`S_IFREG` for a file, `S_IFDIR` for a directory).
    pub fn unix_permissions(mut self, mode: u32) -> Self {
        self.permissions = Some(mode & 0o777);
        self
    }

    fn host(&self) -> System {
        if let Some(system) = self.system {
            system
        } else if cfg!(windows) {
            System::Dos
        } else {
            System::Unix
        }
    }
}

struct Entry {
    name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    external: u32,
    offset: u32,
    version_needed: u16,
    system: System,
}

struct Open {
    name: String,
    opts: FileOptions,
    raw: Vec<u8>,
    encoder: Option<flate2::write::DeflateEncoder<Vec<u8>>>,
}

/// Writes a zip archive into `W`.
pub struct ZipWriter<W: Write + Seek> {
    inner: Option<W>,
    out: Vec<u8>,
    entries: Vec<Entry>,
    open: Option<Open>,
    closed: bool,
}

impl<W: Write + Seek> ZipWriter<W> {
    /// An empty archive. Nothing is written until `finish` or drop.
    pub fn new(inner: W) -> ZipWriter<W> {
        ZipWriter { inner: Some(inner), out: Vec::new(), entries: Vec::new(), open: None, closed: false }
    }

    /// Starts a file. The name is a Unicode string, so a non-ASCII name sets
    /// the language-encoding flag.
    pub fn start_file(&mut self, name: &str, mut options: FileOptions) -> ZipResult<()> {
        self.finish_open()?;
        if self.entries.iter().any(|e| e.name == name) {
            // rustc 1.99 prints `{:?}` of `[u8]::escape_ascii()` as this
            // non-exhaustive struct, for every name.
            return Err(result::invalid("Duplicate filename: EscapeAscii { .. }".to_string()));
        }
        let encoder = match options.method {
            CompressionMethod::Stored => {
                if options.level.is_some() {
                    return Err(ZipError::UnsupportedArchive("Unsupported compression level"));
                }
                None
            }
            CompressionMethod::Deflated => {
                let level = deflate_level(options.level)?;
                Some(flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level)))
            }
        };
        // `normalize`: permission bits with no file type become a regular file.
        if let Some(perms) = &mut options.permissions {
            if *perms & !0o777 == 0 {
                *perms |= S_IFREG;
            }
        }
        self.open = Some(Open { name: name.to_string(), opts: options, raw: Vec::new(), encoder });
        Ok(())
    }

    /// A directory entry. The name gains a trailing `/` when it has none.
    /// The method is stored, and the mode is a directory.
    pub fn add_directory(&mut self, name: &str, mut options: FileOptions) -> ZipResult<()> {
        self.finish_open()?;
        let name = if name.ends_with("/") || name.ends_with("\\") {
            name.to_string()
        } else {
            format!("{name}/")
        };
        if self.entries.iter().any(|e| e.name == name) {
            // rustc 1.99 prints `{:?}` of `[u8]::escape_ascii()` as this
            // non-exhaustive struct, for every name.
            return Err(result::invalid("Duplicate filename: EscapeAscii { .. }".to_string()));
        }
        let permissions = match options.permissions {
            Some(p) => p | S_IFDIR,
            None => DIR_MODE,
        };
        options.permissions = Some(permissions);
        options.method = CompressionMethod::Stored;
        options.level = None;
        self.push_entry(name, options, &[], &[])?;
        Ok(())
    }

    /// Finishes the open entry, writes the central directory and the end
    /// record, and returns the inner writer.
    pub fn finish(mut self) -> ZipResult<W> {
        self.finalize()?;
        Ok(self.inner.take().unwrap())
    }

    fn finish_open(&mut self) -> ZipResult<()> {
        let Some(open) = self.open.take() else {
            return Ok(());
        };
        let compressed = match open.encoder {
            Some(enc) => enc.finish()?,
            None => open.raw.clone(),
        };
        self.push_entry(open.name, open.opts, &open.raw, &compressed)
    }

    fn push_entry(&mut self, name: String, opts: FileOptions, raw: &[u8], compressed: &[u8]) -> ZipResult<()> {
        if compressed.len() > u32::MAX as usize || raw.len() > u32::MAX as usize {
            return Err(ZipError::Io(io::Error::other("File size exceeds maximum size for non-ZIP64 files")));
        }
        let user_perms = opts.permissions;
        let permissions = user_perms.unwrap_or(FILE_MODE);
        let system = opts.host();
        let is_dir = permissions & S_IFDIR == S_IFDIR;
        let version_needed = match opts.method {
            CompressionMethod::Deflated => 20,
            CompressionMethod::Stored => {
                if is_dir {
                    20
                } else {
                    10
                }
            }
        };
        let flags: u16 = if !name.is_ascii() { 1 << 11 } else { 0 };
        let offset = self.out.len() as u32;
        let crc = crc32(raw);
        write_local(
            &mut self.out,
            version_needed,
            flags,
            opts.method.id(),
            crc,
            compressed.len() as u32,
            raw.len() as u32,
            name.as_bytes(),
        );
        self.out.extend_from_slice(compressed);
        let external = external_attributes(system, &name, permissions, user_perms);
        self.entries.push(Entry {
            name,
            method: opts.method.id(),
            flags,
            crc,
            compressed_size: compressed.len() as u32,
            uncompressed_size: raw.len() as u32,
            external,
            offset,
            version_needed,
            system,
        });
        Ok(())
    }

    fn finalize(&mut self) -> ZipResult<()> {
        if self.closed {
            return Ok(());
        }
        self.finish_open()?;
        let central_start = self.out.len();
        for entry in &self.entries {
            write_central(&mut self.out, entry);
        }
        let central_size = (self.out.len() - central_start) as u32;
        write_eocd(&mut self.out, self.entries.len() as u16, central_size, central_start as u32);
        let w = self.inner.as_mut().unwrap();
        w.write_all(&self.out)?;
        self.closed = true;
        Ok(())
    }
}

impl<W: Write + Seek> Write for ZipWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let Some(open) = self.open.as_mut() else {
            return Err(io::Error::other("No file has been started"));
        };
        open.raw.extend_from_slice(buf);
        if let Some(enc) = open.encoder.as_mut() {
            enc.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<W: Write + Seek> Drop for ZipWriter<W> {
    fn drop(&mut self) {
        if !self.closed && self.inner.is_some() {
            let _ = self.finalize();
        }
    }
}

/// DOS directory and read-only bits, only when the system is DOS.
/// The directory bit follows the name, as upstream's `is_dir` does. The
/// read-only bit follows the permission word after the file type is added;
/// `S_IFREG` / `S_IFDIR` do not overlap `0o444`.
fn external_attributes(system: System, name: &str, permissions: u32, user_perms: Option<u32>) -> u32 {
    let mut external = permissions << 16;
    if system == System::Dos {
        let bytes = name.as_bytes();
        if !bytes.is_empty() {
            let last = bytes[bytes.len() - 1];
            if last == b'/' || last == b'\\' {
                external |= 0x10;
            }
        }
        if user_perms.is_some_and(|p| p & 0o444 == 0) {
            external |= 0x01;
        }
    }
    external
}

fn deflate_level(level: Option<i64>) -> ZipResult<u32> {
    let n = level.unwrap_or(6);
    if n >= 1 && n <= 9 {
        Ok(n as u32)
    } else {
        Err(ZipError::UnsupportedArchive("Unsupported compression level"))
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        let mut k = 0;
        while k < 8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            k += 1;
        }
    }
    !crc
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.push(v as u8);
    out.push((v >> 8) as u8);
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push(v as u8);
    out.push((v >> 8) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 24) as u8);
}

fn write_local(out: &mut Vec<u8>, version: u16, flags: u16, method: u16, crc: u32, comp: u32, uncomp: u32, name: &[u8]) {
    push_u32(out, 0x0403_4b50);
    push_u16(out, version);
    push_u16(out, flags);
    push_u16(out, method);
    push_u16(out, 0);
    push_u16(out, DOS_DATE);
    push_u32(out, crc);
    push_u32(out, comp);
    push_u32(out, uncomp);
    push_u16(out, name.len() as u16);
    push_u16(out, 0);
    out.extend_from_slice(name);
}

fn write_central(out: &mut Vec<u8>, entry: &Entry) {
    let made_by = ((entry.system as u16) << 8) | entry.version_needed;
    push_u32(out, 0x0201_4b50);
    push_u16(out, made_by);
    push_u16(out, entry.version_needed);
    push_u16(out, entry.flags);
    push_u16(out, entry.method);
    push_u16(out, 0);
    push_u16(out, DOS_DATE);
    push_u32(out, entry.crc);
    push_u32(out, entry.compressed_size);
    push_u32(out, entry.uncompressed_size);
    push_u16(out, entry.name.len() as u16);
    push_u16(out, 0);
    push_u16(out, 0);
    push_u16(out, 0);
    push_u16(out, 0);
    push_u32(out, entry.external);
    push_u32(out, entry.offset);
    out.extend_from_slice(entry.name.as_bytes());
}

fn write_eocd(out: &mut Vec<u8>, count: u16, central_size: u32, central_start: u32) {
    push_u32(out, 0x0605_4b50);
    push_u16(out, 0);
    push_u16(out, 0);
    push_u16(out, count);
    push_u16(out, count);
    push_u32(out, central_size);
    push_u32(out, central_start);
    push_u16(out, 0);
}
