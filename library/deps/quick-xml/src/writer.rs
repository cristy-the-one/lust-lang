//! `Writer::new` / `write_event` / `into_inner`.
//!
//! Empty elements end with `/>` (no space). Text and attributes are written
//! as already escaped by `BytesText::new` and `BytesStart::push_attribute`.

use std::io::{self, Write};

use crate::events::Event;

/// Writes events to `W`.
pub struct Writer<W: Write> {
    inner: W,
}

impl<W: Write> Writer<W> {
    /// A writer over `inner`.
    pub fn new(inner: W) -> Writer<W> {
        Writer { inner }
    }

    /// Writes one event. `Eof` writes nothing.
    pub fn write_event(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::Start(e) => self.wrapped("<", e.as_ref(), ">"),
            Event::End(e) => self.wrapped("</", e.as_ref(), ">"),
            Event::Empty(e) => self.wrapped("<", e.as_ref(), "/>"),
            Event::Text(e) => self.inner.write_all(e.as_ref().as_bytes()),
            Event::Comment(e) => self.wrapped("<!--", e.as_ref(), "-->"),
            Event::CData(e) => self.wrapped("<![CDATA[", e.as_ref(), "]]>"),
            Event::Decl(e) => self.wrapped("<?", e.as_ref(), "?>"),
            Event::PI(e) => self.wrapped("<?", e.as_ref(), "?>"),
            Event::DocType(e) => self.wrapped("<!DOCTYPE ", e.as_ref(), ">"),
            Event::GeneralRef(e) => self.wrapped("&", e.as_ref(), ";"),
            Event::Eof => Ok(()),
        }
    }

    /// The underlying writer.
    pub fn into_inner(self) -> W {
        self.inner
    }

    fn wrapped(&mut self, before: &str, value: &str, after: &str) -> io::Result<()> {
        self.inner.write_all(before.as_bytes())?;
        self.inner.write_all(value.as_bytes())?;
        self.inner.write_all(after.as_bytes())
    }
}
