# Adapter: sha2 0.10.9

Lust adapter crate (docs/ARCH.md section 3.6). Dev builds compile it in
place of the registry package sha2 0.10.9; release builds use the real
crate through Cargo. Gate: `scripts/test-adapters.sh sha2` (the real
crate is the oracle).

## Implemented, same results as upstream

- `Sha256::new`, `update`, `finalize`, `reset`, `digest`, `Clone`, and
  `Default`. `update` takes anything that is `AsRef<[u8]>` (a slice, an
  array, a `Vec<u8>`, a `str`, a `String`). Several `update` calls hash
  the same bytes as one call.
- The digest is 32 bytes. `as_slice`, indexing, and `Debug` (a list of
  the bytes) match upstream. Hex text is those bytes printed with
  `{:02x}` or `{:02X}`, which is the same text as `{:x}` / `{:X}` on the
  real digest.
- `Digest`, so `use sha2::Digest` resolves. The methods above are
  inherent.
- Features: `default`, `std`.

## Rejected (lust error, never a different answer)

- `Sha224`, `Sha384`, `Sha512`, `Sha512_224`, `Sha512_256`,
  `finalize_reset`, and the `digest` module (L0506 or L0104 at the use).
- `{:x}` and `{:X}` on the digest value itself (L0641): lust's formatter
  applies those specs to integers. Print the bytes.
- Features `oid`, `asm`, `asm-aarch64`, `compress`, `force-soft`,
  `force-soft-compact`, `loongarch64_asm` (L0508).
- `size_of` and other layout queries on `Sha256` and `Output` (L0652).
