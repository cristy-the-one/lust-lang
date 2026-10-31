# Adapter: flate2 1.1.10

Lust adapter crate (docs/ARCH.md section 3.6) for the parts
ts-rust uses: the read side (diagnostics_loc.rs decodes locale
data with MultiGzDecoder), `write::GzEncoder` (pprof.rs writes
the CPU profile), `write::ZlibEncoder` (layergraph.rs), and
`write::DeflateEncoder` (raw deflate, used by the zip adapter). Gate: `scripts/test-adapters.sh flate2`.

## Implemented, same results as upstream

- `read::GzDecoder`, `read::MultiGzDecoder`,
  `read::ZlibDecoder`, and `read::DeflateDecoder` over a
  pure-Rust inflate in the adapter, including gzip header
  forms (extra field, name, comment, header CRC), the gzip
  CRC and size trailer checks, and the zlib Adler check.
  Error kinds and messages match upstream for bad headers,
  corrupt deflate streams, bad checksums, and truncation.
- `write::GzEncoder`: `new`, `get_ref`, `get_mut`, `try_finish`,
  `finish`, `Write` (`write`, `flush`), `Read` (to the inner
  reader), and `Drop` (finishes, errors ignored). It is a line for
  line port of upstream's gz/write.rs, zio.rs `Writer`, and the
  miniz backend, over a copy of miniz_oxide 0.9.1's deflate source
  (`src/miniz/`, upstream's text with the edits listed in
  `src/miniz/mod.rs`). So the gzip bytes, and the order and size of
  every write and flush call on the inner writer, are upstream's for
  every input, level, and write/flush pattern, also when the inner
  writer fails or takes short writes. A failed `try_finish` followed
  by `flush` returns upstream's `CompressError` io::Error (same
  Display and Debug, through a std hook only adapters can call).
  Checked against cargo builds of flate2 1.1.10 with generated
  probes (tf-flate2 REPORT).
- `write::ZlibEncoder`: `new`, `get_ref`, `get_mut`, `try_finish`,
  `finish`, `flush_finish`, `total_in`, `total_out`, `Write`, `Read`,
  and the Writer's `Drop`, a port of upstream's zlib/write.rs over the
  same Writer and a zlib-wrapped miniz compressor (header and Adler-32
  trailer from the miniz copy). Not here: `reset`, `new_with_compress`,
  `Debug`. ts-rust's layergraph.rs encodes mermaid URLs with it.
- `write::DeflateEncoder`: the raw-deflate twin of `ZlibEncoder`
  (`new`, `get_ref`, `get_mut`, `try_finish`, `finish`, `total_in`,
  `total_out`, `Write`, `Read`). Zip's `deflate-flate2` entries are
  this stream. Not here: `reset`.
- `Compression`.
- A unit without a backend feature (`rust_backend` or
  `miniz_oxide`) fails to build, as upstream does.
- A decode error returns the bytes decoded before it first,
  as upstream's streaming decoder does at `read_to_end`.

## Known differences

- The first read pulls the whole input through the decoder,
  where upstream streams. A reader that never ends (a live
  pipe fed slowly) blocks here where upstream would return
  the bytes decoded so far per read.

## Rejected (lust error, never a different answer)

- The read and bufread encoders, the write-side decoders, `GzBuilder`,
  `Debug` of the encoders (upstream prints its internal state), the
  `bufread` module, `Compress`/`Decompress`, and the low-level types.
- `bufread` decoders, `Decompress` and the low-level types,
  `Crc` and `GzBuilder` helpers, and layout queries.
