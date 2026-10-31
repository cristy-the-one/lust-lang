//! The replacements `escape` and `escape_attribute` perform. A string with
//! none of those bytes is copied unchanged, so the written bytes match.

pub(crate) fn escape(raw: &str) -> String {
    replace(raw, false)
}

pub(crate) fn escape_attribute(raw: &str) -> String {
    replace(raw, true)
}

fn replace(raw: &str, attr: bool) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let repl = match bytes[i] {
            b'<' => Some("&lt;"),
            b'>' => Some("&gt;"),
            b'&' => Some("&amp;"),
            b'\'' => Some("&apos;"),
            b'"' => Some("&quot;"),
            b'\r' => Some("&#13;"),
            b'\n' => {
                if attr {
                    Some("&#10;")
                } else {
                    None
                }
            }
            b'\t' => {
                if attr {
                    Some("&#9;")
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(repl) = repl {
            out.push_str(&raw[start..i]);
            out.push_str(repl);
            start = i + 1;
        }
        i += 1;
    }
    if start == 0 {
        raw.to_string()
    } else {
        out.push_str(&raw[start..]);
        out
    }
}
