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
// Most of these are not `auto`: they change something a program can
// observe (the text of a panic message, how many `write` calls a writer
// sees) or add an item of their own, which is why lust rejects the original
// rather than rewrite it silently. `lust fix --all` applies them. Taking
// out a lifetime argument is `auto`: it changes nothing a program does.

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
        // The site as written: one inside a user macro's body or arguments
        // has no text of its own to edit.
        let written = d.span.ctxt == CtxtId::ROOT;
        let hi = d.span.hi as usize;
        let edits: Option<Vec<Edit>> = match (d.code, d.message.as_str()) {
            ("L0809", ASSERT_LONG) if written => site(map, root_src, d.span).and_then(|(text, lo)| assert_message(text, lo)).map(|e| vec![e]),
            ("L0641", WRITE_USER) => site(map, root_src, d.span).and_then(|(text, lo)| write_all(text, lo)).map(|e| vec![e]),
            ("L0731", LIFETIME_ARG) if written => site(map, root_src, d.span).and_then(|(text, lo)| drop_lifetime_arg(text, lo, hi)).map(|e| vec![e]),
            ("L0522", NO_PRELUDE) if written => site(map, root_src, d.span).and_then(|(text, lo)| drop_attr(text, lo, hi)).map(|e| vec![e]),
            ("L0630", CLOSURE_CONST) if written => site(map, root_src, d.span).and_then(|(text, lo)| closure_to_fn(text, lo, hi)),
            _ => None,
        };
        d.edits.extend(edits.into_iter().flatten());
    }
}

/// The message of the lifetime turbofish reject (check/mod.rs).
pub const LIFETIME_ARG: &str = "explicit lifetime arguments on a fn are not supported";
/// The message of the `#[no_implicit_prelude]` reject (resolve/worklist.rs).
pub const NO_PRELUDE: &str = "`#[no_implicit_prelude]` is not supported yet";
/// The message of a closure in a const or static initializer (ty/consteval.rs).
pub const CLOSURE_CONST: &str = "a closure is outside the bounded const evaluator";

/// `f::<'a, T>` loses its lifetime argument, and `f::<'a>` its whole
/// turbofish: Rust infers the lifetime. This one is `auto`: a lifetime
/// argument changes nothing a program does.
fn drop_lifetime_arg(text: &str, lo: usize, hi: usize) -> Option<Edit> {
    let s = text.as_bytes();
    if s.get(lo) != Some(&b'\'') || hi > s.len() {
        return None;
    }
    let mut a = lo;
    while a > 0 && s[a - 1].is_ascii_whitespace() {
        a -= 1;
    }
    let mut b = hi;
    while s.get(b).is_some_and(u8::is_ascii_whitespace) {
        b += 1;
    }
    let (prev, next) = (*s.get(a.checked_sub(1)?)?, *s.get(b)?);
    let (start, end) = match (prev, next) {
        // The only argument: the turbofish goes with it.
        (b'<', b'>') => (text[..a - 1].strip_suffix("::")?.len(), b + 1),
        (_, b',') => {
            let mut e = b + 1;
            while s.get(e) == Some(&b' ') {
                e += 1;
            }
            (lo, e)
        }
        (b',', b'>') => (a - 1, hi),
        _ => return None,
    };
    Some(Edit { start: start as u32, end: end as u32, replacement: String::new(), auto: true })
}

/// An attribute goes, with its line when it has the line to itself.
fn drop_attr(text: &str, lo: usize, hi: usize) -> Option<Edit> {
    let s = text.as_bytes();
    if s.get(lo) != Some(&b'#') || hi > s.len() {
        return None;
    }
    // The attribute's span can stop before its closing bracket.
    let hi = if s.get(hi) == Some(&b']') && !text[lo..hi].ends_with(']') { hi + 1 } else { hi };
    let line_start = text[..lo].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[hi..].find('\n').map_or(s.len(), |i| hi + i + 1);
    let alone = text[line_start..lo].trim().is_empty() && text[hi..line_end].trim().is_empty();
    let (start, end) = if alone { (line_start, line_end) } else { (lo, hi) };
    Some(Edit { start: start as u32, end: end as u32, replacement: String::new(), auto: false })
}

/// The parts of a comma list in type text, at the top level of `()`, `[]`
/// and `<>` (a `->` is not a `>`).
fn split_types(list: &str) -> Vec<&str> {
    let s = list.as_bytes();
    let (mut depth, mut from, mut out) = (0i32, 0, Vec::new());
    for i in 0..s.len() {
        match s[i] {
            b'(' | b'[' | b'<' => depth += 1,
            b'>' if i > 0 && s[i - 1] == b'-' => {}
            b')' | b']' | b'>' => depth -= 1,
            b',' if depth == 0 => {
                out.push(list[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(list[from..].trim());
    out.retain(|p| !p.is_empty());
    out
}

/// The one `fn(A, B) -> R` type in type text `ty`: its parameter types and
/// its return type. None when there is none, more than one, or a form
/// a plain fn item cannot repeat (`unsafe`, `extern`, `for<..>`, named
/// parameters).
fn one_fn_type(ty: &str) -> Option<(Vec<&str>, Option<&str>)> {
    let s = ty.as_bytes();
    let starts: Vec<usize> = ty.match_indices("fn(").map(|(i, _)| i).filter(|&i| i == 0 || !ident_byte(s[i - 1])).collect();
    let [at] = starts[..] else { return None };
    if ["unsafe", "extern", "for<"].iter().any(|w| ty.contains(w)) {
        return None;
    }
    let open = at + 2;
    let (mut depth, mut close) = (0i32, None);
    for i in open..s.len() {
        match s[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let params = split_types(&ty[open + 1..close]);
    // `x: T` names a parameter; `a::T` is a path.
    if params.iter().any(|p| p.replace("::", "").contains(':')) {
        return None;
    }
    let after = ty[close + 1..].trim_start();
    let ret = match after.strip_prefix("->") {
        None => None,
        Some(r) => {
            // The return type ends where the type around the fn type goes on.
            let b = r.as_bytes();
            let (mut depth, mut end) = (0i32, b.len());
            for i in 0..b.len() {
                match b[i] {
                    b'(' | b'[' | b'<' => depth += 1,
                    b'>' if i > 0 && b[i - 1] == b'-' => {}
                    b')' | b']' | b'>' | b',' | b';' if depth == 0 => {
                        end = i;
                        break;
                    }
                    b')' | b']' | b'>' => depth -= 1,
                    _ => {}
                }
            }
            Some(r[..end].trim()).filter(|r| !r.is_empty())
        }
    };
    Some((params, ret))
}

/// A closure in the initializer of a top-level `const` or `static` becomes
/// a named fn next to the item, and the closure its name:
///
///     static T: &[(&str, fn(u32) -> u32)] = &[("inc", |x| x + 1)];
///     static T: &[(&str, fn(u32) -> u32)] = &[("inc", t_fn)];
///
///     fn t_fn(x: u32) -> u32 { x + 1 }
///
/// The fn's parameter and return types are those of the one `fn(..)` type
/// the item's written type holds. No edit for an item that is not at the
/// start of a line (one in an `impl` or a fn body), a type with another
/// number of `fn(..)` types, or a closure with typed parameters.
fn closure_to_fn(text: &str, lo: usize, hi: usize) -> Option<Vec<Edit>> {
    let clo = text.get(lo..hi)?;
    let clo = clo.strip_prefix("move").map_or(clo, str::trim_start);
    let inner = clo.strip_prefix('|')?;
    // The parameter list ends at the next `|` outside a group.
    let (mut depth, mut bar) = (0i32, None);
    for (i, b) in inner.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'|' if depth == 0 => {
                bar = Some(i);
                break;
            }
            _ => {}
        }
    }
    let bar = bar?;
    let pats = split_types(&inner[..bar]);
    let body = inner[bar + 1..].trim();
    if pats.iter().any(|p| p.contains(':')) || body.starts_with("->") || body.is_empty() {
        return None;
    }
    // The item: the last `const` or `static` that starts a line before the closure.
    let mut item = None;
    let mut at = 0;
    for line in text[..lo].split_inclusive('\n') {
        let mut rest = line;
        if let Some(r) = rest.strip_prefix("pub") {
            let r = if r.starts_with('(') { r.split_once(')')?.1 } else { r };
            rest = r.trim_start();
        }
        if let Some(r) = rest.strip_prefix("static ").or_else(|| rest.strip_prefix("const ")) {
            let r = r.trim_start();
            let r = r.strip_prefix("mut ").map_or(r, str::trim_start);
            let name_len = r.bytes().take_while(|&b| ident_byte(b)).count();
            if name_len > 0 && r[name_len..].trim_start().starts_with(':') {
                let colon = at + (line.len() - r.len()) + name_len + r[name_len..].find(':')?;
                item = Some((&r[..name_len], colon));
            }
        }
        at += line.len();
    }
    let (item_name, colon) = item?;
    // The written type runs to the `=`, the initializer to its `;`.
    let eq = colon + 1 + text[colon + 1..].find('=')?;
    let (semi, _) = scan(text, eq + 1, Some(b';'))?;
    if lo <= eq || hi > semi {
        return None;
    }
    let (params, ret) = one_fn_type(text[colon + 1..eq].trim())?;
    if params.len() != pats.len() {
        return None;
    }
    let base = format!("{}_fn", item_name.to_lowercase());
    let name = std::iter::once(base.clone()).chain((2..100).map(|n| format!("{base}{n}"))).find(|n| !text.contains(n.as_str()))?;
    let sig: Vec<String> = pats.iter().zip(&params).map(|(p, t)| format!("{p}: {t}")).collect();
    let ret = ret.map_or(String::new(), |r| format!(" -> {r}"));
    let body = if body.starts_with('{') { body.to_string() } else { format!("{{ {body} }}") };
    Some(vec![
        Edit { start: lo as u32, end: hi as u32, replacement: name.clone(), auto: false },
        Edit { start: semi as u32 + 1, end: semi as u32 + 1, replacement: format!("\n\nfn {name}({}){ret} {body}", sig.join(", ")), auto: false },
    ])
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
    let (close, commas) = scan(text, open + 1, None)?;
    Some((which, Call { open, close, commas }))
}

/// Scans code from `from` to where its group ends: with `stop` None, the
/// delimiter that closes the group `from` is inside; else the first `stop`
/// byte outside any nested group. Gives that offset and the commas passed
/// outside nested groups. String, char and raw string literals are stepped
/// over. None for text that ends first, or holds a comment (the edits move
/// text, and a comment may not move with it).
fn scan(text: &str, from: usize, stop: Option<u8>) -> Option<(usize, Vec<usize>)> {
    let s = text.as_bytes();
    let (mut depth, mut commas) = (0usize, Vec::new());
    let mut i = from;
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
            _ if depth == 0 && stop == Some(b) => return Some((i, commas)),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return stop.is_none().then_some((i, commas));
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
    fn lifetime_arguments_go() {
        let cut = |t: &str, lt: &str| {
            let lo = t.find(lt).unwrap();
            let e = drop_lifetime_arg(t, lo, lo + lt.len()).unwrap();
            assert!(e.auto);
            apply(t, &e)
        };
        assert_eq!(cut("pick::<'static>(&5)", "'static"), "pick(&5)");
        assert_eq!(cut("pick::<'static, i32>(&5)", "'static"), "pick::<i32>(&5)");
        assert_eq!(cut("pick::< 'static , i32 >(&5)", "'static"), "pick::< i32 >(&5)");
        assert_eq!(cut("x.get::<T, 'a>()", "'a"), "x.get::<T>()");
        assert_eq!(cut("two::<'a, 'b>(x)", "'a"), "two::<'b>(x)");
        // A type position has no `::` to take away.
        assert!(drop_lifetime_arg("Vec<'a>", 4, 6).is_none());
    }

    #[test]
    fn an_attribute_goes_with_its_line() {
        let t = "#![no_implicit_prelude]
fn main() {}
";
        assert_eq!(apply(t, &drop_attr(t, 0, 23).unwrap()), "fn main() {}
");
        let t = "#[a] #[b] fn f() {}";
        assert_eq!(apply(t, &drop_attr(t, 5, 9).unwrap()), "#[a]  fn f() {}");
    }

    #[test]
    fn a_closure_in_a_static_becomes_a_named_fn() {
        let fix = |t: &str, clo: &str| {
            let lo = t.find(clo).unwrap();
            closure_to_fn(t, lo, lo + clo.len()).map(|es| {
                // Back to front, as `lust fix` applies them.
                let mut out = t.to_string();
                for e in es.iter().rev() {
                    out = apply(&out, e);
                }
                out
            })
        };
        let t = "static F: fn(u32) -> u32 = |x| x + 1;
fn main() {}
";
        assert_eq!(fix(t, "|x| x + 1").unwrap(), "static F: fn(u32) -> u32 = f_fn;

fn f_fn(x: u32) -> u32 { x + 1 }
fn main() {}
");
        let t = "pub(crate) static TABLE: &[(&str, fn(&mut Vec<u8>, (u8, u8)))] = &[
    (\"a\", |v, (a, b)| { v.push(a); v.push(b) }),
];
";
        assert_eq!(
            fix(t, "|v, (a, b)| { v.push(a); v.push(b) }").unwrap(),
            "pub(crate) static TABLE: &[(&str, fn(&mut Vec<u8>, (u8, u8)))] = &[
    (\"a\", table_fn),
];

fn table_fn(v: &mut Vec<u8>, (a, b): (u8, u8)) { v.push(a); v.push(b) }
"
        );
        // A second closure of the same item gets another name.
        let t = "const C: [fn() -> u8; 2] = [c_fn, || 2];

fn c_fn() -> u8 { 1 }
";
        assert_eq!(fix(t, "|| 2").unwrap(), "const C: [fn() -> u8; 2] = [c_fn2, || 2];

fn c_fn2() -> u8 { 2 }

fn c_fn() -> u8 { 1 }
".replace("[c_fn2, || 2]", "[c_fn, c_fn2]"));
        // Not at the start of a line, two fn types, a typed closure, another arity.
        assert!(fix("impl S {
    const F: fn(u32) -> u32 = |x| x;
}
", "|x| x").is_none());
        assert!(fix("static F: (fn(u8), fn(u8)) = (|x| (), |x| ());
", "|x| ()").is_none());
        assert!(fix("static F: fn(u32) -> u32 = |x: u32| x;
", "|x: u32| x").is_none());
        assert!(fix("static F: fn(u32, u32) -> u32 = |x| x;
", "|x| x").is_none());
        assert!(fix("static F: unsafe fn(u32) = |x| ();
", "|x| ()").is_none());
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
