# Adapter: quick-xml 0.42.0

Lust adapter crate (docs/ARCH.md section 3.6) for the reader and writer
a 3MF document uses. Default features only (no serde, tokio, or
encoding_rs). Gate: `scripts/test-adapters.sh quick-xml`.

## Implemented, same results as upstream

- `Reader::from_str`, `read_event`, `config_mut().trim_text`.
- Events: `Start`, `End`, `Empty`, `Text`, `CData`, `Comment`, `Decl`,
  `PI`, `DocType`, `GeneralRef`, `Eof`. Text stops at `<` and `&`.
  `&name;` is a `GeneralRef` of `name`. Empty elements stay `Empty`.
  A mismatched end tag is
  `ill-formed document: expected `</a>`, but `</b>` was found`.
- `BytesStart::new`, `push_attribute`, `name`, `attributes`.
  `BytesText::new` escapes `<`, `>`, `&`, quotes, and `\r`.
  Attribute values also escape `\n` and `\t`.
  `BytesDecl::new`, `BytesEnd::new`, `BytesCData::new`.
- `Writer::new`, `write_event`, `into_inner`. Empty elements end with
  `/>` (no extra space).

## Rejected (lust error, never a different answer)

- Serde (`serialize` / `deserialize`), tokio, encoding conversion, and
  namespace resolution (`NsReader`).
