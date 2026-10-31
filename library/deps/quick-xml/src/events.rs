//! Event types the reader yields and the writer accepts.
//!
//! `BytesStart` stores the tag the way 0.42.0 does: the name, then
//! ` key="value"` for each attribute, with the value already escaped.
//! `BytesText::new` runs `escape`. `BytesDecl::new` builds
//! `xml version="…" encoding="…" standalone="…"`.

use crate::escape::{escape, escape_attribute};

/// A borrowed name. The field is public, as upstream's tuple field is.
pub struct QName<'a>(pub &'a str);

/// An attribute. `value` is the raw (escaped) text between the quotes.
pub struct Attribute<'a> {
    /// The attribute name.
    pub key: QName<'a>,
    /// The raw attribute value.
    pub value: String,
}

/// Failure while splitting attributes. The reader yields `Ok` for the
/// well-formed attributes it accepts.
#[derive(Debug)]
pub struct AttrError;

/// A start tag or an empty tag: name plus the attribute text.
pub struct BytesStart {
    buf: String,
    name_len: usize,
}

impl BytesStart {
    /// A start tag with this name and no attributes.
    pub fn new(name: &str) -> BytesStart {
        BytesStart { buf: name.to_string(), name_len: name.len() }
    }

    /// The element name.
    pub fn name(&self) -> QName<'_> {
        QName(&self.buf[..self.name_len])
    }

    /// The name and attributes, as the writer emits them inside the brackets.
    pub fn as_ref(&self) -> &str {
        &self.buf
    }

    /// Appends ` key="escaped-value"`. The value is escaped with the
    /// attribute rules (`\n` and `\t` included).
    pub fn push_attribute(&mut self, attr: (&str, &str)) {
        self.buf.push(' ');
        self.buf.push_str(attr.0);
        self.buf.push_str("=\"");
        self.buf.push_str(&escape_attribute(attr.1));
        self.buf.push('"');
    }

    pub(crate) fn set_buf(&mut self, buf: String) {
        let bytes = buf.as_bytes();
        let mut n = 0;
        while n < bytes.len() && !is_ws(bytes[n]) && bytes[n] != b'/' {
            n += 1;
        }
        self.buf = buf;
        self.name_len = n;
    }

    /// Attributes in order. Each item is `Ok`, matching the `Result` the
    /// real iterator yields for a well-formed tag.
    pub fn attributes(&self) -> Attributes<'_> {
        Attributes { src: &self.buf, pairs: split_attrs(&self.buf, self.name_len), i: 0 }
    }
}

/// Iterator of attributes. Item is a `Result`, as upstream's is.
pub struct Attributes<'a> {
    src: &'a str,
    pairs: Vec<(usize, usize, String)>,
    i: usize,
}

impl<'a> Iterator for Attributes<'a> {
    type Item = Result<Attribute<'a>, AttrError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i >= self.pairs.len() {
            return None;
        }
        let i = self.i;
        let start = self.pairs[i].0;
        let end = self.pairs[i].1;
        let value = self.pairs[i].2.clone();
        self.i += 1;
        Some(Ok(Attribute { key: QName(&self.src[start..end]), value }))
    }
}

fn split_attrs(buf: &str, name_len: usize) -> Vec<(usize, usize, String)> {
    let bytes = buf.as_bytes();
    let mut i = name_len;
    let mut pairs = Vec::new();
    while i < bytes.len() {
        while i < bytes.len() && is_ws(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' && !is_ws(bytes[i]) {
            i += 1;
        }
        let key_end = i;
        while i < bytes.len() && is_ws(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            break;
        }
        i += 1;
        while i < bytes.len() && is_ws(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            break;
        }
        i += 1;
        let val_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        let value = buf[val_start..i].to_string();
        if i < bytes.len() {
            i += 1;
        }
        pairs.push((key_start, key_end, value));
    }
    pairs
}

pub(crate) fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

/// An end tag's name.
pub struct BytesEnd {
    name: String,
}

impl BytesEnd {
    /// An end tag for `name`.
    pub fn new(name: &str) -> BytesEnd {
        BytesEnd { name: name.to_string() }
    }

    /// The name, without the brackets.
    pub fn as_ref(&self) -> &str {
        &self.name
    }
}

/// Text, a comment, or a doctype body.
pub struct BytesText {
    content: String,
}

impl BytesText {
    /// Text, escaped with `escape` (`<`, `>`, `&`, quotes, and `\r`).
    pub fn new(content: &str) -> BytesText {
        BytesText { content: escape(content) }
    }

    /// Text that is already escaped. The reader uses this for the bytes it saw.
    pub fn from_escaped(content: &str) -> BytesText {
        BytesText { content: content.to_string() }
    }

    /// The text.
    pub fn as_ref(&self) -> &str {
        &self.content
    }
}

/// A CDATA body. `new` does not escape; `]]>` in the body is the caller's problem.
pub struct BytesCData {
    content: String,
}

impl BytesCData {
    /// CDATA contents, stored as given.
    pub fn new(content: &str) -> BytesCData {
        BytesCData { content: content.to_string() }
    }

    /// The contents.
    pub fn as_ref(&self) -> &str {
        &self.content
    }
}

/// `<?xml …?>` contents, without the brackets.
pub struct BytesDecl {
    content: String,
}

impl BytesDecl {
    /// `xml version="…"`, plus encoding and standalone when `Some`.
    /// Values are not escaped; the caller passes plain tokens.
    pub fn new(version: &str, encoding: Option<&str>, standalone: Option<&str>) -> BytesDecl {
        let mut buf = String::new();
        buf.push_str("xml version=\"");
        buf.push_str(version);
        if let Some(encoding) = encoding {
            buf.push_str("\" encoding=\"");
            buf.push_str(encoding);
        }
        if let Some(standalone) = standalone {
            buf.push_str("\" standalone=\"");
            buf.push_str(standalone);
        }
        buf.push('"');
        BytesDecl { content: buf }
    }

    /// The declaration body, starting with `xml`.
    pub fn as_ref(&self) -> &str {
        &self.content
    }

    pub(crate) fn from_content(content: String) -> BytesDecl {
        BytesDecl { content }
    }
}

/// A general entity or character reference, without `&` and `;`.
pub struct BytesRef {
    content: String,
}

impl BytesRef {
    /// The name between `&` and `;`.
    pub fn as_ref(&self) -> &str {
        &self.content
    }

    pub(crate) fn from_name(name: &str) -> BytesRef {
        BytesRef { content: name.to_string() }
    }
}

/// A processing instruction body, without `<?` and `?>`.
pub struct BytesPI {
    content: String,
}

impl BytesPI {
    /// The instruction body.
    pub fn as_ref(&self) -> &str {
        &self.content
    }

    pub(crate) fn from_content(content: &str) -> BytesPI {
        BytesPI { content: content.to_string() }
    }
}

/// One XML event.
pub enum Event {
    /// `<tag …>`.
    Start(BytesStart),
    /// `</tag>`.
    End(BytesEnd),
    /// `<tag …/>`.
    Empty(BytesStart),
    /// Character data.
    Text(BytesText),
    /// `<![CDATA[…]]>`.
    CData(BytesCData),
    /// `<!-- … -->`.
    Comment(BytesText),
    /// `<?xml …?>`.
    Decl(BytesDecl),
    /// `<?…?>` other than the XML declaration.
    PI(BytesPI),
    /// `<!DOCTYPE …>`.
    DocType(BytesText),
    /// `&name;`.
    GeneralRef(BytesRef),
    /// No more input.
    Eof,
}
