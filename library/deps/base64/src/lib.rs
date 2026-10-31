// Lust adapter for base64 0.22.1 (docs/ARCH.md section 3.6): the standard
// and URL-safe engines lime-slice uses to encode and decode. The bytes
// match the real crate, including decode errors (symbol, offset, padding,
// trailing bits). `Engine` is the trait a `use base64::Engine` names;
// `encode` and `decode` are inherent on the engines. Not here (they
// reject at the use): the deprecated free functions, custom alphabets,
// `read` and `write` wrappers, and slice encode/decode.

pub mod engine;
pub mod prelude;

mod error;

pub use crate::engine::Engine;
pub use crate::error::DecodeError;
