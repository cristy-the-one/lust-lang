// base64 0.22.1 decode errors: empty is ok, and invalid padding, symbols,
// lengths, and trailing bits name the same variant, offset, and byte.
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE};
use base64::Engine as _;

fn show(engine: &str, input: &str, result: Result<Vec<u8>, base64::DecodeError>) {
    match result {
        Ok(v) => println!("{engine} {input:?} ok {v:?}"),
        Err(e) => println!("{engine} {input:?} {e} | {e:?}"),
    }
}

fn main() {
    let cases = [
        "", "A", "AA", "AAA", "AAAA", "AA=", "A===", "====", "Zm9v", "Zm9vYmFy", "@@@", "Zg==", "Zg=", "Zg", "Zm8", "Zm8=", "/w==", "/w=", "/w", "YQ", "YQI",
        "YQI=", "*A==", "AA=A", "AAA==", "A A", "Zg===", "////", "////AA==", "/x==", "/y==", "YR==", "YQJ=", "///=", "AAA/", "ABC=", "ABD=", "Zh==", "Zm9=", "Zm9w",
        "AAA@AAAA", "AAAAAAA@", "AAAA\nAAAA", "++++", "----", "____",
    ];
    for s in cases {
        show("std", s, STANDARD.decode(s));
        show("np", s, STANDARD_NO_PAD.decode(s));
        show("url", s, URL_SAFE.decode(s));
    }
    let long = format!("{}@{}", "A".repeat(64), "AAAA");
    show("std", &long, STANDARD.decode(&long));
}
