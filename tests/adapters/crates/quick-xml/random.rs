// A few thousand random documents and writer events. Names, text, entities,
// comments, and attributes stay inside the subset both parsers accept.
// A mismatch of the event trace or the written string fails the oracle.
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn name(rng: &mut Rng) -> String {
    let alphabet = b"abcdefghij";
    let n = 1 + rng.below(5) as usize;
    let mut s = String::new();
    let mut i = 0;
    while i < n {
        s.push(alphabet[rng.below(alphabet.len() as u64) as usize] as char);
        i += 1;
    }
    s
}

fn raw_text(rng: &mut Rng) -> String {
    let alphabet = b"Hello 3mf XYZ";
    let n = rng.below(12) as usize;
    let mut s = String::new();
    let mut i = 0;
    while i < n {
        let pick = rng.below(8);
        if pick == 0 {
            s.push('<');
        } else if pick == 1 {
            s.push('&');
        } else if pick == 2 {
            s.push('"');
        } else {
            s.push(alphabet[rng.below(alphabet.len() as u64) as usize] as char);
        }
        i += 1;
    }
    s
}

fn escape_text(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c == '<' {
            o.push_str("&lt;");
        } else if c == '&' {
            o.push_str("&amp;");
        } else if c == '"' {
            o.push_str("&quot;");
        } else {
            o.push(c);
        }
    }
    o
}

fn show(ev: Event) {
    match ev {
        Event::Start(e) => show_tag("S", &e),
        Event::Empty(e) => show_tag("E", &e),
        Event::End(e) => print!("/{} ", e.as_ref()),
        Event::Text(e) => print!("T{:?} ", e.as_ref()),
        Event::GeneralRef(e) => print!("R{} ", e.as_ref()),
        Event::Comment(e) => print!("C{:?} ", e.as_ref()),
        Event::Decl(e) => print!("D{:?} ", e.as_ref()),
        Event::CData(e) => print!("A{:?} ", e.as_ref()),
        Event::PI(e) => print!("P{:?} ", e.as_ref()),
        Event::DocType(e) => print!("Y{:?} ", e.as_ref()),
        Event::Eof => print!("."),
    }
}

fn show_tag(kind: &str, e: &BytesStart) {
    print!("{kind}{}", e.name().0);
    for a in e.attributes() {
        let a = a.unwrap();
        print!(":{}={}", a.key.0, a.value);
    }
    print!(" ");
}

fn parse_print(xml: &str) {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => {
                println!();
                break;
            }
            Ok(ev) => show(ev),
            Err(e) => {
                println!("ERR {e}");
                break;
            }
        }
    }
}

fn document(rng: &mut Rng) -> String {
    let mut xml = String::new();
    if rng.below(3) == 0 {
        xml.push_str("<?xml version=\"1.0\"?>");
    }
    let root = name(rng);
    xml.push('<');
    xml.push_str(&root);
    let attrs = rng.below(3);
    let mut a = 0;
    while a < attrs {
        xml.push(' ');
        xml.push('k');
        xml.push((b'0' + a as u8) as char);
        xml.push_str("=\"");
        xml.push_str(&escape_text(&raw_text(rng)));
        xml.push('"');
        a += 1;
    }
    if rng.below(4) == 0 {
        xml.push_str("/>");
        return xml;
    }
    xml.push('>');
    let kids = rng.below(4);
    let mut k = 0;
    while k < kids {
        let kind = rng.below(5);
        if kind == 0 {
            xml.push_str(&escape_text(&raw_text(rng)));
            if rng.below(3) == 0 {
                xml.push_str("&amp;");
            }
        } else if kind == 1 {
            xml.push_str("<!--");
            xml.push_str(&name(rng));
            xml.push_str("-->");
        } else if kind == 2 {
            let child = name(rng);
            xml.push('<');
            xml.push_str(&child);
            xml.push_str("/>");
        } else if kind == 3 {
            xml.push_str("<![CDATA[");
            xml.push_str(&name(rng));
            xml.push_str("]]>");
        } else {
            let child = name(rng);
            xml.push('<');
            xml.push_str(&child);
            xml.push('>');
            xml.push_str(&escape_text(&raw_text(rng)));
            xml.push_str("</");
            xml.push_str(&child);
            xml.push('>');
        }
        k += 1;
    }
    xml.push_str("</");
    xml.push_str(&root);
    xml.push('>');
    xml
}

fn write_one(rng: &mut Rng) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut w = Writer::new(&mut buf);
    if rng.below(2) == 0 {
        w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None))).unwrap();
    }
    let root = name(rng);
    let mut start = BytesStart::new(&root);
    if rng.below(2) == 0 {
        let text = raw_text(rng);
        start.push_attribute(("k", text.as_str()));
    }
    w.write_event(Event::Start(start)).unwrap();
    w.write_event(Event::Text(BytesText::new(&raw_text(rng)))).unwrap();
    if rng.below(2) == 0 {
        w.write_event(Event::Empty(BytesStart::new(&name(rng)))).unwrap();
    }
    w.write_event(Event::End(BytesEnd::new(&root))).unwrap();
    String::from_utf8(buf).unwrap()
}

fn main() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut n = 0;
    while n < 3000 {
        let xml = document(&mut rng);
        print!("P ");
        parse_print(&xml);
        n += 1;
    }
    let mut n = 0;
    while n < 1000 {
        let xml = write_one(&mut rng);
        println!("W {xml}");
        n += 1;
    }
}
