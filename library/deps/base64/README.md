# Adapter: base64 0.22.1

Lust adapter crate (docs/ARCH.md section 3.6). Dev builds compile it in
place of the registry package base64 0.22.1; release builds use the real
crate through Cargo. Gate: `scripts/test-adapters.sh base64` (the real
crate is the oracle).

## Implemented, same results as upstream

- `engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD}`
  with `encode` and `decode`. Input is anything that is `AsRef<[u8]>`
  (a slice, an array, a `Vec<u8>`, a `str`, a `String`).
- `DecodeError` (`InvalidByte`, `InvalidLength`, `InvalidLastSymbol`,
  `InvalidPadding`) with upstream's `Display` and `Debug`, including the
  symbol, the offset, padding, and trailing bits.
- `Engine` and `prelude` (`BASE64_STANDARD` and the other three names),
  so `use base64::Engine` and `use base64::prelude::*` resolve.
- Features: `alloc`, `default`, `std`.

## Rejected (lust error, never a different answer)

- The deprecated free functions (`decode`, `encode`, ...), custom
  alphabets, `GeneralPurpose::new`, and the `read` / `write` wrappers
  (L0506 or L0104 at the use).
- `size_of` and other layout queries on `GeneralPurpose` (L0652).
