// Lust adapter for quick-xml 0.42.0 (docs/ARCH.md section 3.6).
//
// The slice a 3MF document uses: `Reader::from_str` / `read_event` for
// declarations, start and empty and end tags, attributes, text, general
// entities, comments, CDATA, processing instructions, and doctypes, plus
// `Writer::write_event` with the same escaping as 0.42.0. Serde, async,
// and encoding conversion are not here (README).

pub mod events;
pub mod reader;
pub mod writer;

mod escape;

pub use crate::reader::Reader;
pub use crate::writer::Writer;

/// A read error. Display matches quick-xml's `ill-formed document: …` text
/// for the forms this reader reports.
pub struct Error {
    msg: String,
}

impl Error {
    pub(crate) fn ill(detail: String) -> Error {
        Error { msg: format!("ill-formed document: {detail}") }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error({})", self.msg)
    }
}

/// `Result` with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
