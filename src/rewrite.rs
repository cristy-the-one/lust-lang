// Rewrites for rejects: concrete edits that turn valid Rust lust refuses
// into Rust it accepts.
//
// A reject is not an error in the user's program. Where the accepted form
// follows from the text of the rejected one, the diagnostic carries the
// edit (`Diag::edits`), `lust check --json` prints it, and `lust fix`
// applies it (src/fix.rs). The edited program is plain Rust, so rule zero
// holds: rustc builds it too, and both give the same output.
//
// The edits here need the source text, which the passes that find these
// rejects do not hold (lowering, the body checker). So `attach` runs where
// the driver renders diagnostics, with the session's source map, and adds
// the edit to each diagnostic it knows by code and message. A site whose
// text is not the plain written form gets no edit: a call inside a user
// macro, a comment between the arguments, a format string that is not a
// plain literal.
//
// None of these is `auto`. Each changes something a program can observe,
// which is why lust rejects the original rather than rewrite it silently:
// the text of a panic message, or how many `write` calls a writer sees.
// `lust fix --all` applies them.

use crate::diag::{CtxtId, Diag, Edit, FileId, SourceMap, Span};

/// The message of the long `assert!` reject (expand/builtin.rs).
pub const ASSERT_LONG: &str = "`assert!` with no message on a long or item-holding condition is not supported yet";
/// The message of the user-writer `write!` reject (check/platform.rs).
pub const WRITE_USER: &str = "`write!` into a user-defined `std::io::Write` type is not supported yet";

/// Adds the rewrite edit to each diagnostic that has one. `root_src` is the
/// text of the map's first file; other files' texts are in the map.
pub fn attach(diags: &mut [Diag], map: &SourceMap, root_src: &str) {
    for d in diags.iter_mut() {
        if !d.edits.is_empty() {
            continue;
        }
        let edit = match (d.code, d.message.as_str()) {
            // The macro call as written: one inside a user macro's body
            // or arguments has no text of its own to edit.
            ("L0809", ASSERT_LONG) if d.span.ctxt == CtxtId::ROOT => site(map, root_src, d.span).and_then(|(text, lo)| assert_message(text, lo)),
            ("L0641", WRITE_USER) => site(map, root_src, d.span).and_then(|(text, lo)| write_all(text, lo)),
            _ => None,
        };
        d.edits.extend(edit);
    }
}

/// The text of the file `span` reports in and the offset the reported site
/// starts at (its expansion call site).
fn site<'a>(map: &'a SourceMap, root_src: &'a str, span: Span) -> Option<(&'a str, usize)> {
    let at = map.call_site(span);
    if at.ctxt != CtxtId::ROOT {
        return None;
    }
    let text = map.text(at.file).or((at.file == FileId(0)).then_some(root_src))?;
    ((at.lo as usize) < text.len() && text.is_char_boundary(at.lo as usize)).then_some((text, at.lo as usize))
}

/// A macro call `name!(a, b, ..)` in source text, by byte offsets.
struct Call {
    /// The opening and the closing delimiter.
    open: usize,
    close: usize,
    /// The commas between the arguments (not inside a nested group).
    commas: Vec<usize>,
}

fn ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// The end of the string literal whose opening `"` is at `i`: the offset
/// of its closing `"`.
fn str_end(s: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while j < s.len() {
        match s[j] {
            b'\\' => j += 2,
            b'"' => return Some(j),
            _ => j += 1,
        }
    }
    None
}

/// The call of one of `names` that starts at `lo`, with which name it is.
/// None when the text there is not such a call, or holds a comment (the
/// edits move argument text, and a comment may not move with it).
fn scan_call(text: &str, lo: usize, names: &[&str]) -> Option<(usize, Call)> {
    let s = text.as_bytes();
    let which = names.iter().position(|n| text[lo..].starts_with(n) && !s.get(lo + n.len()).copied().is_some_and(ident_byte))?;
    let mut i = lo + names[which].len();
    let skip_ws = |mut i: usize| {
        while s.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        i
    };
    i = skip_ws(i);
    if s.get(i) != Some(&b'!') {
        return None;
    }
    i = skip_ws(i + 1);
    if !matches!(s.get(i), Some(b'(' | b'[' | b'{')) {
        return None;
    }
    let open = i;
    let (mut depth, mut commas) = (0usize, Vec::new());
    i += 1;
    while i < s.len() {
        let b = s[i];
        match b {
            b'"' => i = str_end(s, i)?,
            b'/' if matches!(s.get(i + 1), Some(b'/' | b'*')) => return None,
            b'\'' => {
                // A char literal ends at a `'` one char (or one escape)
                // later; anything else is a lifetime or a label.
                if s.get(i + 1) == Some(&b'\\') {
                    let mut j = i + 2;
                    while j < s.len() && s[j] != b'\'' {
                        j += 1;
                    }
                    i = j;
                } else if let Some(c) = text[i + 1..].chars().next()
                    && s.get(i + 1 + c.len_utf8()) == Some(&b'\'')
                {
                    i += 1 + c.len_utf8();
                }
            }
            _ if ident_byte(b) => {
                // A whole word at once, so an `r` inside one never looks
                // like a raw string's. `r"`, `r#"`, `br"` open one.
                let start = i;
                while i < s.len() && ident_byte(s[i]) {
                    i += 1;
                }
                let word = &text[start..i];
                let mut j = i;
                while s.get(j) == Some(&b'#') {
                    j += 1;
                }
                if matches!(word, "r" | "br" | "cr") && s.get(j) == Some(&b'"') {
                    let close: String = std::iter::once('"').chain(std::iter::repeat_n('#', j - i)).collect();
                    i = j + 1 + text[j + 1..].find(&close)? + close.len();
                }
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return Some((which, Call { open, close: i, commas }));
                }
                depth -= 1;
            }
            b',' if depth == 0 => commas.push(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// `assert!(cond)` gets its condition as the message: `assert!(cond,
/// "assertion failed: cond")`. The message is the condition's source text on
/// one line, where rustc prints its own layout of a long condition.
fn assert_message(text: &str, lo: usize) -> Option<Edit> {
    let (_, c) = scan_call(text, lo, &["assert"])?;
    let trailing = c.commas.last().copied().filter(|&k| text[k + 1..c.close].trim().is_empty());
    // Any other comma starts a message that is there already.
    if c.commas.len() > usize::from(trailing.is_some()) {
        return None;
    }
    let cond = text[c.open + 1..trailing.unwrap_or(c.close)].trim_end();
    if cond.trim().is_empty() {
        return None;
    }
    let at = c.open + 1 + cond.len();
    let mut msg = String::from(", \"assertion failed: ");
    for word in cond.split_whitespace() {
        if !msg.ends_with(": ") {
            msg.push(' ');
        }
        for ch in word.chars() {
            match ch {
                '\\' => msg.push_str("\\\\"),
                '"' => msg.push_str("\\\""),
                '{' => msg.push_str("{{"),
                '}' => msg.push_str("}}"),
                _ => msg.push(ch),
            }
        }
    }
    msg.push('"');
    Some(Edit { start: at as u32, end: at as u32, replacement: msg, auto: false })
}

/// `write!(dst, fmt, args..)` becomes `dst.write_all(format!(fmt,
/// args..).as_bytes())`, and `writeln!` the same with a newline at the end
/// of the format string. The writer then sees one `write_all` where
/// rustc's `write!` calls it once per piece of the format.
fn write_all(text: &str, lo: usize) -> Option<Edit> {
    let (which, c) = scan_call(text, lo, &["writeln", "write"])?;
    let line = which == 0;
    let first = c.commas.first().copied();
    let dst = text[c.open + 1..first.unwrap_or(c.close)].trim();
    let rest = first.map_or("", |k| text[k + 1..c.close].trim());
    let rest = rest.strip_suffix(',').map_or(rest, str::trim_end);
    if dst.is_empty() {
        return None;
    }
    // `write!` borrows its destination itself, and so does a method call.
    let recv = dst.strip_prefix("&mut").filter(|r| r.starts_with(char::is_whitespace)).map_or(dst, str::trim_start);
    let plain = recv.chars().all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '.' | ':'));
    let recv = if plain { recv.to_string() } else { format!("({recv})") };
    let call = match (line, rest.is_empty()) {
        (false, true) => return None,
        (true, true) => format!("{recv}.write_all(b\"\\n\")"),
        (false, false) => format!("{recv}.write_all(format!({rest}).as_bytes())"),
        (true, false) => {
            // The newline goes inside a plain string literal only.
            if !rest.starts_with('"') {
                return None;
            }
            let end = str_end(rest.as_bytes(), 0)?;
            format!("{recv}.write_all(format!({}\\n{}).as_bytes())", &rest[..end], &rest[end..])
        }
    };
    Some(Edit { start: lo as u32, end: c.close as u32 + 1, replacement: call, auto: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, e: &Edit) -> String {
        format!("{}{}{}", &text[..e.start as usize], e.replacement, &text[e.end as usize..])
    }

    #[test]
    fn assert_gets_its_condition_as_the_message() {
        let t = "fn f() { assert!(a == 1 &&\n    b(\"x{}\", ',') == [1, 2]); }";
        let e = assert_message(t, t.find("assert").unwrap()).unwrap();
        assert_eq!(apply(t, &e), "fn f() { assert!(a == 1 &&\n    b(\"x{}\", ',') == [1, 2], \"assertion failed: a == 1 && b(\\\"x{{}}\\\", ',') == [1, 2]\"); }");
        assert!(!e.auto);
    }

    #[test]
    fn assert_keeps_a_trailing_comma_and_skips_what_it_cannot_edit() {
        let t = "assert!(a < b,)";
        assert_eq!(apply(t, &assert_message(t, 0).unwrap()), "assert!(a < b, \"assertion failed: a < b\",)");
        // A message, a comment, another macro.
        assert!(assert_message("assert!(a, \"m\")", 0).is_none());
        assert!(assert_message("assert!(a /* why */ && b)", 0).is_none());
        assert!(assert_message("assert_eq!(a, b)", 0).is_none());
        // A lifetime and a raw string do not end the scan early.
        let t = "assert!(f::<'a>(r#\"),\"#) == 'x')";
        assert_eq!(apply(t, &assert_message(t, 0).unwrap()), "assert!(f::<'a>(r#\"),\"#) == 'x', \"assertion failed: f::<'a>(r#\\\"),\\\"#) == 'x'\")");
    }

    #[test]
    fn write_becomes_one_write_all() {
        let t = "let _ = write!(c, \"a{}b{}\", 1, 2);";
        assert_eq!(apply(t, &write_all(t, 8).unwrap()), "let _ = c.write_all(format!(\"a{}b{}\", 1, 2).as_bytes());");
        let t = "writeln!(&mut self.out, \"x = {x}\",)?;";
        assert_eq!(apply(t, &write_all(t, 0).unwrap()), "self.out.write_all(format!(\"x = {x}\\n\").as_bytes())?;");
        let t = "writeln!(out)";
        assert_eq!(apply(t, &write_all(t, 0).unwrap()), "out.write_all(b\"\\n\")");
        let t = "write!(get(w), \"{}\", 1)";
        assert_eq!(apply(t, &write_all(t, 0).unwrap()), "(get(w)).write_all(format!(\"{}\", 1).as_bytes())");
        // A format that is not a plain literal cannot take the newline.
        assert!(write_all("writeln!(out, concat!(\"a\", \"b\"))", 0).is_none());
        assert!(write_all("write!(out)", 0).is_none());
    }
}
