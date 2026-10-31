# Adapter: zip 9.2.0

Lust adapter crate (docs/ARCH.md section 3.6) for stored and Deflated
archives. Deflate bytes come from `flate2::write::DeflateEncoder` (the
miniz port), which is what zip 9.2.0 writes with
`default-features = false` and `features = ["deflate-flate2"]`. The
`time` feature is off, so the timestamp is 1980-01-01. Gate:
`scripts/test-adapters.sh zip`.

## Implemented, same bytes as upstream

- `ZipWriter::new`, `start_file`, `write` / `write_all`, `add_directory`,
  `finish`, and `Drop` (finishes).
- `SimpleFileOptions`: `default`, `compression_method`, `compression_level`,
  `system`, `unix_permissions`. The default method is Deflated, the default
  level is 6, and a level outside 1..=9 (or any level on a stored entry) is
  `unsupported Zip archive: Unsupported compression level`.
- Version-needed 10 for a stored file and 20 for deflate or a directory.
  Version-made-by is `(system << 8) | version-needed`. A non-ASCII name sets
  the language-encoding flag. Unix mode is the high half of the external
  attributes (`0o100644` for a file, `0o040755` for a directory). Pass
  `system(System::Unix)` when the bytes must be identical on Windows.
- `ZipArchive::new`, `len`, `by_name`, `by_index`. `ZipFile` reads the
  inflated bytes and reports name, method, sizes, crc, mode, and directory.
- A missing name is `specified file not found in archive`. A duplicate name
  is `invalid Zip archive: Duplicate filename: EscapeAscii { .. }`,
  which is what rustc 1.99 prints for `{:?}` of `escape_ascii()`.

## Rejected (lust error, never a different answer)

- AES, zip64, data descriptors, encryption, symlinks, and every compression
  method but stored and deflate. The `time` feature's wall-clock timestamp.
- `ZipWriter` methods this file does not define (`merge_archive`,
  `raw_copy_file`, `add_symlink`, and the parallel appenders).
