//! `ZipArchive` over the central directory our writer emits, and over the
//! same bytes from zip 9.2.0. Payload length and metadata come from the
//! central directory. The local header is skipped using its own name and
//! extra lengths. Deflate is `flate2::read::DeflateDecoder`.

use std::io::{self, Read, Seek, SeekFrom};

use crate::format::CompressionMethod;
use crate::result::{ZipError, ZipResult};

struct Entry {
    name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    external: u32,
    version_needed: u16,
    data: Vec<u8>,
}

/// A zip archive read from `R`.
pub struct ZipArchive<R> {
    _reader: R,
    entries: Vec<Entry>,
}

impl<R: Read + Seek> ZipArchive<R> {
    /// Reads the central directory. The reader may sit at the end, where
    /// `ZipWriter::finish` leaves it.
    pub fn new(mut reader: R) -> ZipResult<ZipArchive<R>> {
        reader.seek(SeekFrom::Start(0))?;
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        let entries = parse(&data)?;
        Ok(ZipArchive { _reader: reader, entries })
    }

    /// The number of entries, directories included.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The entry named `name`, or `FileNotFound`.
    pub fn by_name(&mut self, name: &str) -> ZipResult<ZipFile> {
        let index = self.entries.iter().position(|e| e.name == name).ok_or(ZipError::FileNotFound)?;
        self.by_index(index)
    }

    /// The entry at `index` in central-directory order, or `FileNotFound`.
    pub fn by_index(&mut self, index: usize) -> ZipResult<ZipFile> {
        let entry = self.entries.get(index).ok_or(ZipError::FileNotFound)?;
        let bytes = match entry.method {
            0 => entry.data.clone(),
            8 => inflate(&entry.data)?,
            other => return Err(ZipError::CompressionMethodNotSupported(other)),
        };
        Ok(ZipFile {
            name: entry.name.clone(),
            method: entry.method,
            flags: entry.flags,
            crc: entry.crc,
            compressed_size: entry.compressed_size as u64,
            uncompressed_size: entry.uncompressed_size as u64,
            external: entry.external,
            version_needed: entry.version_needed,
            data: bytes,
            pos: 0,
        })
    }
}

/// One entry's bytes and the central-directory fields a caller prints.
pub struct ZipFile {
    name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    external: u32,
    version_needed: u16,
    data: Vec<u8>,
    pos: usize,
}

impl ZipFile {
    /// The entry name, including a directory's trailing slash.
    pub fn name(&self) -> ZipResult<String> {
        Ok(self.name.clone())
    }

    /// Stored or Deflated.
    pub fn compression(&self) -> CompressionMethod {
        if self.method == 8 {
            CompressionMethod::Deflated
        } else {
            CompressionMethod::Stored
        }
    }

    /// The CRC-32 of the uncompressed bytes.
    pub fn crc32(&self) -> u32 {
        self.crc
    }

    /// Uncompressed size.
    pub fn size(&self) -> u64 {
        self.uncompressed_size
    }

    /// Compressed size.
    pub fn compressed_size(&self) -> u64 {
        self.compressed_size
    }

    /// General-purpose bit flag.
    pub fn flags(&self) -> u16 {
        self.flags
    }

    /// Unix mode from the high half of the external attributes, when set.
    pub fn unix_mode(&self) -> Option<u32> {
        if self.external == 0 { None } else { Some(self.external >> 16) }
    }

    /// External attributes, including the DOS low half.
    pub fn external_attributes(&self) -> u32 {
        self.external
    }

    /// Version needed to extract.
    pub fn version_needed(&self) -> u16 {
        self.version_needed
    }

    /// A directory name ends in `/` or `\`.
    pub fn is_dir(&self) -> bool {
        let bytes = self.name.as_bytes();
        if bytes.is_empty() {
            false
        } else {
            let last = bytes[bytes.len() - 1];
            last == b'/' || last == b'\\'
        }
    }

    /// Not a directory.
    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }
}

impl Read for ZipFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - self.pos);
        for i in 0..n {
            buf[i] = self.data[self.pos + i];
        }
        self.pos += n;
        Ok(n)
    }
}

fn inflate(input: &[u8]) -> ZipResult<Vec<u8>> {
    let mut dec = flate2::read::DeflateDecoder::new(io_slice(input));
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    Ok(out)
}

/// `DeflateDecoder` takes `impl Read`. A small owned reader keeps the
/// compressed bytes without requiring `Read for &[u8]` to be named at the
/// call through a temporary.
struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}

fn io_slice(data: &[u8]) -> SliceReader<'_> {
    SliceReader { data, pos: 0 }
}

impl Read for SliceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - self.pos);
        for i in 0..n {
            buf[i] = self.data[self.pos + i];
        }
        self.pos += n;
        Ok(n)
    }
}

fn parse(data: &[u8]) -> ZipResult<Vec<Entry>> {
    if data.len() < 22 {
        return Err(crate::result::invalid("Invalid Central Directory End header".into()));
    }
    let eocd = find_eocd(data)?;
    let count = u16_at(data, eocd + 10) as usize;
    let central_size = u32_at(data, eocd + 12) as usize;
    let central_off = u32_at(data, eocd + 16) as usize;
    if central_off + central_size > data.len() {
        return Err(crate::result::invalid("Invalid central directory size or offset".into()));
    }
    let mut at = central_off;
    let end = central_off + central_size;
    let mut entries = Vec::new();
    while entries.len() < count && at + 46 <= end {
        if u32_at(data, at) != 0x0201_4b50 {
            return Err(crate::result::invalid("Invalid Central Directory header".into()));
        }
        let version_needed = u16_at(data, at + 6);
        let flags = u16_at(data, at + 8);
        let method = u16_at(data, at + 10);
        let crc = u32_at(data, at + 16);
        let compressed_size = u32_at(data, at + 20);
        let uncompressed_size = u32_at(data, at + 24);
        let name_len = u16_at(data, at + 28) as usize;
        let extra_len = u16_at(data, at + 30) as usize;
        let comment_len = u16_at(data, at + 32) as usize;
        let external = u32_at(data, at + 38);
        let offset = u32_at(data, at + 42);
        let name_at = at + 46;
        let next = name_at + name_len + extra_len + comment_len;
        if next > data.len() {
            return Err(crate::result::invalid("Invalid Central Directory header".into()));
        }
        let mut name_bytes = Vec::new();
        name_bytes.extend_from_slice(&data[name_at..name_at + name_len]);
        let name = match String::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(_) => return Err(crate::result::invalid("Invalid Central Directory header".into())),
        };
        let payload = local_payload(data, offset as usize, compressed_size as usize)?;
        entries.push(Entry {
            name,
            method,
            flags,
            crc,
            compressed_size,
            uncompressed_size,
            external,
            version_needed,
            data: payload,
        });
        at = next;
    }
    Ok(entries)
}

fn local_payload(data: &[u8], offset: usize, compressed_size: usize) -> ZipResult<Vec<u8>> {
    if offset + 30 > data.len() || u32_at(data, offset) != 0x0403_4b50 {
        return Err(crate::result::invalid("Invalid local file header".into()));
    }
    let name_len = u16_at(data, offset + 26) as usize;
    let extra_len = u16_at(data, offset + 28) as usize;
    let start = offset + 30 + name_len + extra_len;
    let end = start + compressed_size;
    if end > data.len() {
        return Err(crate::result::invalid("Invalid local file header".into()));
    }
    let mut payload = Vec::new();
    payload.extend_from_slice(&data[start..end]);
    Ok(payload)
}

fn find_eocd(data: &[u8]) -> ZipResult<usize> {
    // Comment length lives in the last two bytes when the record is last,
    // which is every archive this writer produces. Search back for the
    // signature so a comment still resolves.
    let mut i = data.len() - 22;
    loop {
        if u32_at(data, i) == 0x0605_4b50 {
            let comment = u16_at(data, i + 20) as usize;
            if i + 22 + comment == data.len() {
                return Ok(i);
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    Err(crate::result::invalid("Invalid Central Directory End header".into()))
}

fn u16_at(data: &[u8], i: usize) -> u16 {
    data[i] as u16 | ((data[i + 1] as u16) << 8)
}

fn u32_at(data: &[u8], i: usize) -> u32 {
    data[i] as u32 | ((data[i + 1] as u32) << 8) | ((data[i + 2] as u32) << 16) | ((data[i + 3] as u32) << 24)
}
