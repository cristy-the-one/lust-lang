// quick-xml 0.42.0 reader events and writer bytes. Entities stay split.
// Attribute values are the raw text between the quotes.
use quick_xml::events::{BytesCData, BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

fn show(ev: Event) {
    match ev {
        Event::Start(e) => show_tag("start", &e),
        Event::Empty(e) => show_tag("empty", &e),
        Event::End(e) => println!("end {}", e.as_ref()),
        Event::Text(e) => println!("text {:?}", e.as_ref()),
        Event::CData(e) => println!("cdata {:?}", e.as_ref()),
        Event::Comment(e) => println!("comment {:?}", e.as_ref()),
        Event::Decl(e) => println!("decl {:?}", e.as_ref()),
        Event::PI(e) => println!("pi {:?}", e.as_ref()),
        Event::DocType(e) => println!("doctype {:?}", e.as_ref()),
        Event::GeneralRef(e) => println!("ref {}", e.as_ref()),
        Event::Eof => println!("eof"),
    }
}

fn show_tag(kind: &str, e: &BytesStart) {
    print!("{kind} {}", e.name().0);
    for a in e.attributes() {
        let a = a.unwrap();
        print!(" {}={}", a.key.0, a.value);
    }
    println!();
}

fn dump(label: &str, xml: &str) {
    println!("-- {label}");
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => {
                println!("eof");
                break;
            }
            Ok(ev) => show(ev),
            Err(e) => {
                println!("err {e}");
                break;
            }
        }
    }
}

fn dump_trim(xml: &str) {
    println!("-- trim");
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => {
                println!("eof");
                break;
            }
            Ok(ev) => show(ev),
            Err(e) => {
                println!("err {e}");
                break;
            }
        }
    }
}

fn main() {
    dump("model", "<?xml version=\"1.0\" encoding=\"UTF-8\"?><model unit=\"mm\" note=\"a&lt;b&amp;&quot;&apos;\">hi &lt;&amp;&gt;</model>");
    dump("empty", "<a/>");
    dump("empty-sp", "<a />");
    dump("empty-attr", "<a b=\"c\"/>");
    dump("sq", "<a b='c'/>");
    dump("comment", "<r><!-- hi --><t>x</t></r>");
    dump("cdata", "<r><![CDATA[a<b>&]]></r>");
    dump("pi", "<r><?target data?></r>");
    dump("doctype", "<!DOCTYPE html><r/>");
    dump("doctype-sub", "<!DOCTYPE note [<!ELEMENT note ANY>]><r/>");
    dump("ws", "<r>\n  <t> x </t>\n</r>");
    dump("ref", "<r>&#60; &#x41;</r>");
    dump("uni", "<r>héllo 文件</r>");
    dump("end-sp", "<a></a >");
    dump("mismatch", "<a></b>");
    dump("extra-end", "</a>");
    dump_trim("<r>\n  <t> x </t>\n</r>");

    let mut buf: Vec<u8> = Vec::new();
    let mut w = Writer::new(&mut buf);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None))).unwrap();
    let mut start = BytesStart::new("model");
    start.push_attribute(("unit", "mm"));
    start.push_attribute(("note", "a<b&\"'"));
    w.write_event(Event::Start(start)).unwrap();
    w.write_event(Event::Text(BytesText::new("hi <&>"))).unwrap();
    w.write_event(Event::Empty(BytesStart::new("br"))).unwrap();
    w.write_event(Event::Comment(BytesText::from_escaped(" c "))).unwrap();
    w.write_event(Event::CData(BytesCData::new("a<b>"))).unwrap();
    w.write_event(Event::End(BytesEnd::new("model"))).unwrap();
    let text = String::from_utf8(buf).unwrap();
    println!("-- writer");
    println!("{text}");
    dump("writer-parse", &text);
}
