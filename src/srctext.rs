// Rust source text as rustc reads it: a UTF-8 BOM is dropped and every CRLF
// becomes LF before spans are assigned. A bare CR stays, and the lexer reports
// it. Doing this at the read means a Windows checkout (CRLF) and a Unix
// checkout (LF) of the same program produce the same tokens, diagnostics, and
// `line!()` values as rustc on that host.
use std::path::Path;

/// `text` with a leading UTF-8 BOM removed and CRLF turned into LF.
pub fn normalize(text: String) -> String {
    let text = text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text);
    if text.as_bytes().contains(&b'\r') { text.replace("\r\n", "\n") } else { text }
}

/// Reads a Rust source file and normalizes its newlines.
pub fn read(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path).map(normalize)
}
