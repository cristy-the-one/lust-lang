//! `Reader::from_str` / `read_event`.
//!
//! Text stops at `<` and at `&`. `&name;` is a `GeneralRef` whose name
//! does not include the ampersand or the semicolon. Empty elements stay
//! `Empty`. Trailing whitespace in an end tag is stripped
//! (`trim_markup_names_in_closing_tags` defaults to true). A mismatch is
//! `ill-formed document: expected `</a>`, but `</b>` was found`.
//! `trim_text(true)` skips leading whitespace of a text chunk and trims
//! trailing whitespace. A whitespace-only chunk is consumed by the leading
//! trim, so it is not emitted.

use crate::events::{self, BytesCData, BytesDecl, BytesEnd, BytesPI, BytesRef, BytesStart, BytesText, Event};
use crate::Error;

/// Reader configuration. Only `trim_text` is wired.
pub struct Config {
    trim_start: bool,
    trim_end: bool,
}

impl Config {
    /// Sets both trim ends, as `Config::trim_text` does.
    pub fn trim_text(&mut self, trim: bool) {
        self.trim_start = trim;
        self.trim_end = trim;
    }
}

/// A pull reader over one string.
pub struct Reader {
    input: String,
    pos: usize,
    stack: Vec<String>,
    config: Config,
}

impl Reader {
    /// A reader at the start of `input`.
    pub fn from_str(input: &str) -> Reader {
        Reader {
            input: input.to_string(),
            pos: 0,
            stack: Vec::new(),
            config: Config { trim_start: false, trim_end: false },
        }
    }

    /// The configuration.
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    /// The next event, or `Eof`. An unclosed start tag does not error; a
    /// mismatched end tag does.
    pub fn read_event(&mut self) -> Result<Event, Error> {
        loop {
            if self.pos >= self.input.len() {
                return Ok(Event::Eof);
            }
            let b = self.input.as_bytes()[self.pos];
            if b == b'<' {
                return self.read_markup();
            }
            if b == b'&' {
                return self.read_ref();
            }
            if let Some(text) = self.read_text()? {
                return Ok(Event::Text(BytesText::from_escaped(&text)));
            }
        }
    }

    fn read_text(&mut self) -> Result<Option<String>, Error> {
        let bytes = self.input.as_bytes();
        if self.config.trim_start {
            while self.pos < bytes.len() && events::is_ws(bytes[self.pos]) {
                self.pos += 1;
            }
        }
        if self.pos >= self.input.len() {
            return Ok(None);
        }
        let b = self.input.as_bytes()[self.pos];
        if b == b'<' || b == b'&' {
            return Ok(None);
        }
        let start = self.pos;
        while self.pos < bytes.len() && bytes[self.pos] != b'<' && bytes[self.pos] != b'&' {
            self.pos += 1;
        }
        let mut end = self.pos;
        if self.config.trim_end {
            while end > start && events::is_ws(bytes[end - 1]) {
                end -= 1;
            }
        }
        Ok(Some(self.input[start..end].to_string()))
    }

    fn read_ref(&mut self) -> Result<Event, Error> {
        let start = self.pos + 1;
        let bytes = self.input.as_bytes();
        let mut i = start;
        while i < bytes.len() && bytes[i] != b';' && bytes[i] != b'<' {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b';' {
            return Err(Error::ill("unclosed entity reference".into()));
        }
        let name = self.input[start..i].to_string();
        self.pos = i + 1;
        Ok(Event::GeneralRef(BytesRef::from_name(&name)))
    }

    fn read_markup(&mut self) -> Result<Event, Error> {
        let rest = &self.input[self.pos..];
        if rest.starts_with("</") {
            return self.read_end();
        }
        if rest.starts_with("<?") {
            return self.read_pi();
        }
        if rest.starts_with("<!--") {
            return self.read_wrapped("-->", 4, false);
        }
        if rest.starts_with("<![CDATA[") {
            return self.read_wrapped("]]>", 9, true);
        }
        if rest.len() >= 9 && eq_ignore(&rest.as_bytes()[..9], b"<!DOCTYPE") {
            return self.read_doctype();
        }
        self.read_element()
    }

    fn read_end(&mut self) -> Result<Event, Error> {
        let close = match find_at(&self.input, ">", self.pos) {
            Some(i) => i,
            None => return Err(Error::ill("unclosed end tag".into())),
        };
        let raw = &self.input[self.pos + 2..close];
        let name = trim_end_ws(raw).to_string();
        self.pos = close + 1;
        match self.stack.pop() {
            Some(expected) => {
                if expected == name {
                    Ok(Event::End(BytesEnd::new(&name)))
                } else {
                    Err(Error::ill(format!("expected `</{expected}>`, but `</{name}>` was found")))
                }
            }
            None => Err(Error::ill(format!("close tag `</{name}>` does not match any open tag"))),
        }
    }

    fn read_pi(&mut self) -> Result<Event, Error> {
        let end = match find_at(&self.input, "?>", self.pos + 2) {
            Some(i) => i,
            None => return Err(Error::ill("unclosed processing instruction".into())),
        };
        let content = self.input[self.pos + 2..end].to_string();
        self.pos = end + 2;
        let bytes = content.as_bytes();
        let is_decl = bytes.len() >= 3 && &bytes[..3] == b"xml" && (bytes.len() == 3 || events::is_ws(bytes[3]));
        if is_decl {
            Ok(Event::Decl(BytesDecl::from_content(content)))
        } else {
            Ok(Event::PI(BytesPI::from_content(&content)))
        }
    }

    fn read_wrapped(&mut self, end_pat: &str, skip: usize, cdata: bool) -> Result<Event, Error> {
        let end = match find_at(&self.input, end_pat, self.pos + skip) {
            Some(i) => i,
            None => return Err(Error::ill("unclosed markup".into())),
        };
        let content = self.input[self.pos + skip..end].to_string();
        self.pos = end + end_pat.len();
        if cdata {
            Ok(Event::CData(BytesCData::new(&content)))
        } else {
            Ok(Event::Comment(BytesText::from_escaped(&content)))
        }
    }

    fn read_doctype(&mut self) -> Result<Event, Error> {
        let bytes = self.input.as_bytes();
        let mut i = self.pos + 9;
        while i < bytes.len() && events::is_ws(bytes[i]) {
            i += 1;
        }
        let start = i;
        let mut depth = 0i32;
        while i < bytes.len() {
            if bytes[i] == b'[' {
                depth += 1;
            } else if bytes[i] == b']' && depth > 0 {
                depth -= 1;
            } else if bytes[i] == b'>' && depth == 0 {
                break;
            }
            i += 1;
        }
        if i >= bytes.len() {
            return Err(Error::ill("unclosed doctype".into()));
        }
        let content = self.input[start..i].to_string();
        self.pos = i + 1;
        Ok(Event::DocType(BytesText::from_escaped(&content)))
    }

    fn read_element(&mut self) -> Result<Event, Error> {
        let bytes = self.input.as_bytes();
        let mut i = self.pos + 1;
        if i >= bytes.len() {
            return Err(Error::ill("unclosed start tag".into()));
        }
        let name_start = i;
        while i < bytes.len() && !events::is_ws(bytes[i]) && bytes[i] != b'>' && bytes[i] != b'/' {
            i += 1;
        }
        let name = self.input[name_start..i].to_string();
        let mut empty = false;
        let mut buf_end = i;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'"' || b == b'\'' {
                i += 1;
                while i < bytes.len() && bytes[i] != b {
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err(Error::ill("unclosed start tag".into()));
                }
                i += 1;
                continue;
            }
            if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                empty = true;
                buf_end = i;
                i += 2;
                break;
            }
            if b == b'>' {
                buf_end = i;
                i += 1;
                break;
            }
            i += 1;
        }
        if buf_end == name_start && i >= bytes.len() {
            return Err(Error::ill("unclosed start tag".into()));
        }
        if !empty && (i == 0 || self.input.as_bytes()[i - 1] != b'>') {
            return Err(Error::ill("unclosed start tag".into()));
        }
        let buf = self.input[name_start..buf_end].to_string();
        self.pos = i;
        let mut start = BytesStart::new(&name);
        start.set_buf(buf);
        if empty {
            Ok(Event::Empty(start))
        } else {
            self.stack.push(name);
            Ok(Event::Start(start))
        }
    }
}

fn trim_end_ws(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 && events::is_ws(bytes[end - 1]) {
        end -= 1;
    }
    &s[..end]
}

fn find_at(s: &str, pat: &str, from: usize) -> Option<usize> {
    if from > s.len() {
        return None;
    }
    match s[from..].find(pat) {
        Some(i) => Some(i + from),
        None => None,
    }
}

fn eq_ignore(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if lower(a[i]) != lower(b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

fn lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' { b + 32 } else { b }
}
