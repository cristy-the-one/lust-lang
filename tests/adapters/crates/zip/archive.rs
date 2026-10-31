// zip 9.2.0 with deflate-flate2, no `time` feature. system(Unix) keeps the
// version-made-by byte identical on Windows. The hex is the archive; the
// read-back is what ZipArchive returns for those bytes.
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, System, ZipArchive, ZipWriter};

fn opts() -> SimpleFileOptions {
    SimpleFileOptions::default().system(System::Unix)
}

fn hex(bytes: &[u8]) -> String {
    let digits = b"0123456789abcdef";
    let mut s = String::new();
    for &b in bytes {
        s.push(digits[(b >> 4) as usize] as char);
        s.push(digits[(b & 0xf) as usize] as char);
    }
    s
}

fn show<T>(r: zip::result::ZipResult<T>) -> String {
    match r {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("{e}"),
    }
}

fn dump(label: &str, cursor: Cursor<Vec<u8>>) {
    let bytes = cursor.get_ref().clone();
    println!("{label} HEX {}", hex(&bytes));
    match ZipArchive::new(cursor) {
        Err(e) => println!("{label} read {e}"),
        Ok(mut archive) => {
            println!("{label} len {}", archive.len());
            let mut i = 0;
            while i < archive.len() {
                let mut f = archive.by_index(i).unwrap();
                let mut body = Vec::new();
                f.read_to_end(&mut body).unwrap();
                let mode = match f.unix_mode() {
                    Some(m) => m.to_string(),
                    None => "none".to_string(),
                };
                println!(
                    "{label} [{i}] name {} method {} dir {} file {} crc {} size {} comp {} mode {} ext {} ver {}",
                    f.name().unwrap(),
                    f.compression(),
                    f.is_dir(),
                    f.is_file(),
                    f.crc32(),
                    f.size(),
                    f.compressed_size(),
                    mode,
                    f.external_attributes(),
                    f.version_needed(),
                );
                println!("{label} [{i}] body {}", hex(&body));
                i += 1;
            }
            println!("{label} missing {}", show(archive.by_name("no-such")));
        }
    }
}

fn main() {
    dump("empty", ZipWriter::new(Cursor::new(Vec::new())).finish().unwrap());

    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("hello.txt", opts()).unwrap();
    z.write_all(b"hello 3mf\n").unwrap();
    z.start_file("stored.txt", opts().compression_method(CompressionMethod::Stored)).unwrap();
    z.write_all(b"raw").unwrap();
    z.add_directory("3D", opts()).unwrap();
    z.start_file("3D/a.txt", opts().compression_method(CompressionMethod::Stored).unix_permissions(0o600)).unwrap();
    z.write_all(b"hi").unwrap();
    z.start_file("notes.txt", opts().compression_level(Some(9))).unwrap();
    let mut i = 0;
    let chunked = b"one-byte-writes";
    while i < chunked.len() {
        let n = z.write(&chunked[i..i + 1]).unwrap();
        i += n;
    }
    z.start_file("café.txt", opts().compression_method(CompressionMethod::Stored)).unwrap();
    z.write_all("héllo".as_bytes()).unwrap();
    dump("mixed", z.finish().unwrap());

    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("empty-deflate", opts()).unwrap();
    z.start_file("empty-stored", opts().compression_method(CompressionMethod::Stored)).unwrap();
    dump("empties", z.finish().unwrap());

    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    println!("dup {}", show(z.start_file("a.txt", opts())));
    println!("dup2 {}", show(z.start_file("a.txt", opts())));
    println!("lvl {}", show(z.start_file("b.txt", opts().compression_level(Some(0)))));
    println!("lvl9ok {}", show(z.start_file("c.txt", opts().compression_level(Some(9)))));
    println!("stored-lvl {}", show(z.start_file("d.txt", opts().compression_method(CompressionMethod::Stored).compression_level(Some(6)))));
    let _ = z.finish();
}
