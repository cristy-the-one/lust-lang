#!/usr/bin/env python3
"""Turn an expansion (rustc -Zunpretty=expanded, or `lust expand`)
into a canonical token sequence both compilers can be compared on.

Usage: normalize.py < expanded.rs > case.expanded

ARCH 6.5: the printed expansion is lossy, so the corpus compares
tokens, not text. This tokenizer drops comments and all whitespace
structure and writes the token stream space-joined and wrapped. It
is a lexer for already-valid output, not a Rust lexer: it only needs
to split that output the same way every time.

On top of tokenizing, `canonicalize` maps the forms the two
compilers print differently to one shape, so the compare tests the
user macro expansion, not pretty-printing:

- rustc's injected prelude header is dropped.
- `macro_rules!` definitions are dropped (rustc prints them in the
  expansion, lust consumes them).
- `#[macro_use]` markers are dropped (lust's expander acts on the
  marker and consumes it, rustc prints it).
- rustc's `println!`, `format!`, `panic!`, and list-form `vec!`
  expansions (the `_print`/`_eprint`, alloc format, `panic_fmt`, and
  alloc boxed forms) map back to the written macro calls.
- Format strings are positionalized (`{}` and `{name}` captures
  become `{0}`, `{1}` with captures appended as arguments), which is
  the form rustc prints and lust keeps as written.
- Redundant parens lust's flat token stream adds around an expanded
  fragment (doubled `( ( x ) )`, and a parenthesized assignment in
  statement position) are unwrapped; both spellings are the same
  program, and rustc prints neither.
- String and byte string literals compare by value: their escapes
  are rewritten one way (rustc re-escapes format strings and prints
  `include_bytes!` data as `\n`, lust keeps the written form).
- rustc's `option_env!` value paths (`::core::option::Option::Some`,
  `::core::option::Option::None::<&'static str>`) map to the prelude
  forms lust prints.
- rustc's empty `vec![]` expansion (`::alloc::vec::Vec::new()`)
  maps back to `vec![]`, a `>>` closing two generic lists splits
  into two `>` (lust prints the closes separately; a shift splits
  the same way on both sides), and a comma right before `}` is
  dropped (rustc's printer writes struct field lists with a
  trailing comma, lust keeps the source's form).

Derive expansions are not canonicalized: rustc prints generated
impls and lust has no token form for them yet, so a fixture whose
expansion contains derives can only differ here.
"""
import sys

MULTI = ["..=", "...", "=>", "->", "::", "..", "<<", ">>", "&&", "||",
         "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "==", "!=",
         "<=", ">=", ">>=", "<<="]


def tokens(text):
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c.isspace():
            i += 1
            continue
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
            continue
        if text.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            continue
        # Raw string r##"..."## and raw identifier r#name. A plain
        # identifier that starts with r falls through to ident below.
        if c == "r" and i + 1 < n and (text[i + 1] == "#" or text[i + 1] == '"'):
            j = i + 1
            while j < n and text[j] == "#":
                j += 1
            if j < n and text[j] == '"':
                hashes = j - i - 1
                end = '"' + "#" * hashes
                k = text.find(end, j + 1)
                k = n if k < 0 else k + len(end)
                yield text[i:k]
                i = k
                continue
            # raw identifier: r# plus an ident that is not a quote
            if j == i + 2 and j < n and (text[j].isalpha() or text[j] == "_"):
                k = j
                while k < n and (text[k].isalnum() or text[k] == "_"):
                    k += 1
                yield text[i:k]
                i = k
                continue
        # String, byte string, char, byte char, lifetime.
        if c == '"' or (c == "b" and i + 1 < n and text[i + 1] in "\"'"):
            j = i + 1 if c == '"' else i + 2
            quote = '"' if c == '"' or text[i + 1] == '"' else "'"
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == quote:
                    j += 1
                    break
                j += 1
            yield text[i:j]
            i = j
            continue
        if c == "'":
            # Char literal ('x', '\n', '\'' ...) vs lifetime ('a, 'static).
            if i + 2 < n and text[i + 2] == "'":
                yield text[i:i + 3]
                i += 3
                continue
            if i + 1 < n and text[i + 1] == "\\":
                j = text.find("'", i + 2)
                j = n if j < 0 else j + 1
                yield text[i:j]
                i = j
                continue
            j = i + 1
            while j < n and (text[j].isalnum() or text[j] == "_"):
                j += 1
            yield text[i:j]
            i = j
            continue
        if c.isalpha() or c == "_":
            j = i
            while j < n and (text[j].isalnum() or text[j] == "_"):
                j += 1
            yield text[i:j]
            i = j
            continue
        if c.isdigit():
            j = i
            while j < n and (text[j].isalnum() or text[j] in "_-."):
                # Do not eat a range or a field access after the number.
                if text[j] == "." and j + 1 < n and not text[j + 1].isdigit():
                    break
                if text[j] == "-" and not (j > i and text[j - 1] in "eE"):
                    break
                j += 1
            yield text[i:j]
            i = j
            continue
        matched = False
        for m in MULTI:
            if text.startswith(m, i):
                yield m
                i += len(m)
                matched = True
                break
        if not matched:
            yield c
            i += 1


OPEN = {"(": ")", "[": "]", "{": "}"}
CLOSE = set(OPEN.values())


def group_end(toks, i):
    """Index just past the group that opens at toks[i], or None."""
    if i >= len(toks) or toks[i] not in OPEN:
        return None
    stack = [OPEN[toks[i]]]
    j = i + 1
    while j < len(toks):
        tok = toks[j]
        if tok in OPEN:
            stack.append(OPEN[tok])
        elif tok in CLOSE:
            if stack.pop() != tok:
                return None
            if not stack:
                return j + 1
        j += 1
    return None


def split_commas(toks):
    """Split a token list at commas of the outermost level."""
    parts, depth, start = [], 0, 0
    for i, tok in enumerate(toks):
        if tok in OPEN:
            depth += 1
        elif tok in CLOSE:
            depth -= 1
        elif tok == "," and depth == 0:
            parts.append(toks[start:i])
            start = i + 1
    parts.append(toks[start:])
    return parts


def is_ident(tok):
    return tok is not None and (tok[0].isalpha() or tok[0] == "_") and all(c.isalnum() or c == "_" for c in tok)


def strip_prelude(toks):
    head = ["#", "!", "[", "feature", "(", "prelude_import", ")", "]",
            "extern", "crate", "std", ";", "#", "[", "prelude_import", "]",
            "use", "std", "::", "prelude", "::"]
    tail = ["::", "*", ";"]
    if toks[:len(head)] == head and toks[len(head) + 1:len(head) + 4] == tail and toks[len(head)].startswith("rust_"):
        return toks[len(head) + 4:]
    return toks


def drop_macro_rules(toks):
    out, i = [], 0
    while i < len(toks):
        if (toks[i] == "macro_rules" and i + 3 < len(toks) and toks[i + 1] == "!"
                and is_ident(toks[i + 2]) and toks[i + 3] in OPEN):
            end = group_end(toks, i + 3)
            if end is not None:
                # Attributes on the definition (`#[macro_export]` and
                # friends) belong to it and go with it: pop any
                # trailing `# [ ... ]` runs already emitted.
                j = len(out)
                while j >= 2 and out[j - 1] == "]":
                    # Find this attribute's opening `# [` by depth.
                    depth, k = 0, j - 1
                    while k >= 0:
                        if out[k] == "]":
                            depth += 1
                        elif out[k] == "[":
                            depth -= 1
                            if depth == 0:
                                break
                        k -= 1
                    if k < 1 or out[k - 1] != "#":
                        break
                    j = k - 1
                del out[j:]
                i = end + 1 if end < len(toks) and toks[end] == ";" else end
                continue
        out.append(toks[i])
        i += 1
    return out


def drop_macro_use_attrs(toks):
    """Drop each `# [ macro_use ... ]` run (see the header)."""
    out, i = [], 0
    while i < len(toks):
        if toks[i:i + 3] == ["#", "[", "macro_use"]:
            end = group_end(toks, i + 1)
            if end is not None:
                i = end
                continue
        out.append(toks[i])
        i += 1
    return out


def format_args_at(toks, i):
    """If `format_args ! ( ... )` starts at i, return (inner, end)."""
    if toks[i:i + 3] == ["format_args", "!", "("]:
        end = group_end(toks, i + 2)
        if end is not None:
            return toks[i + 3:end - 1], end
    return None


def map_alloc_format(toks):
    wrappers = [
        ["::", "alloc", "::", "__export", "::", "must_use", "(", "{", "::", "alloc", "::", "fmt", "::", "format", "("],
        ["::", "alloc", "::", "fmt", "::", "format", "("],
    ]
    out, i = [], 0
    while i < len(toks):
        done = False
        for w in wrappers:
            if toks[i:i + len(w)] == w:
                parsed = format_args_at(toks, i + len(w))
                if parsed is not None:
                    inner, end = parsed
                    # The wrappers close as `) } )` (must_use) or `)`.
                    close = toks[end:end + 3]
                    total = end + (3 if close == [")", "}", ")"] else 1 if close[:1] == [")"] else 0)
                    out += ["format", "!", "("] + inner + [")"]
                    i = total
                    done = True
                    break
        if not done:
            out.append(toks[i])
            i += 1
    return out


def unquote(tok):
    """The raw content of a plain string token, or None."""
    if len(tok) >= 2 and tok[0] == '"' and tok[-1] == '"':
        return tok[1:-1]
    return None


def map_print(toks):
    out, i = [], 0
    while i < len(toks):
        if toks[i] == "{" and toks[i + 1:i + 7] == ["::", "std", "::", "io", "::", "_print"] or \
                toks[i] == "{" and toks[i + 1:i + 7] == ["::", "std", "::", "io", "::", "_eprint"]:
            eprint = toks[i + 6] == "_eprint"
            if toks[i + 7:i + 8] == ["("]:
                parsed = format_args_at(toks, i + 8)
                if parsed is not None:
                    inner, end = parsed
                    if toks[end:end + 3] == [")", ";", "}"] and inner and unquote(inner[0]) is not None:
                        fmt = unquote(inner[0])
                        newline = fmt.endswith("\\n")
                        if newline:
                            inner = ['"' + fmt[:-2] + '"'] + inner[1:]
                        name = ("eprintln" if newline else "eprint") if eprint else ("println" if newline else "print")
                        out += [name, "!", "("] + inner + [")"]
                        i = end + 3
                        continue
        out.append(toks[i])
        i += 1
    return out


def map_panic(toks):
    """rustc's 2024 `panic!` expansion, `{ ::core::panicking::panic_fmt(
    format_args!(..)); }`, back to the written `panic!(..)`."""
    head = ["{", "::", "core", "::", "panicking", "::", "panic_fmt", "("]
    out, i = [], 0
    while i < len(toks):
        if toks[i:i + len(head)] == head:
            parsed = format_args_at(toks, i + len(head))
            if parsed is not None:
                inner, end = parsed
                if toks[end:end + 3] == [")", ";", "}"]:
                    arm = out[-1:] == ["=>"]
                    out += ["panic", "!", "("] + inner + [")"]
                    i = end + 3
                    # A block arm body takes no comma; the written
                    # `=> panic!(..)` arm has one before the next arm.
                    if arm and toks[i:i + 1] not in ([","], ["}"]):
                        out.append(",")
                    continue
        out.append(toks[i])
        i += 1
    return out


def positionalize(fmt, args):
    """Rewrite one format string to positional placeholders.

    args is the list of argument token-lists after the format string.
    Returns (new_fmt, extra_capture_names), or None to leave the call
    as written (a shape this does not model, such as `width$`).
    """
    named_explicit = {a[0] for a in args if len(a) >= 2 and is_ident(a[0]) and a[1] == "="}
    out, i, implicit, captures = [], 0, 0, []
    while i < len(fmt):
        c = fmt[i]
        if c == "{" and fmt[i + 1:i + 2] == "{":
            out.append("{{")
            i += 2
            continue
        if c == "}" and fmt[i + 1:i + 2] == "}":
            out.append("}}")
            i += 2
            continue
        if c != "{":
            out.append(c)
            i += 1
            continue
        close = fmt.find("}", i + 1)
        if close < 0 or "{" in fmt[i + 1:close]:
            return None
        inside = fmt[i + 1:close]
        if "$" in inside:
            return None
        head, _, spec = inside.partition(":")
        spec = (":" + spec) if spec or ":" in inside else ""
        if head == "":
            out.append("{%d%s}" % (implicit, spec))
            implicit += 1
        elif head.isdigit():
            out.append(fmt[i:close + 1])
        elif is_ident(head):
            if head in named_explicit:
                out.append(fmt[i:close + 1])
            else:
                if head not in captures:
                    captures.append(head)
                out.append("{%d%s}" % (len(args) + captures.index(head), spec))
        else:
            return None
        i = close + 1
    return "".join(out), captures


FORMAT_CALLS = {"println", "print", "eprintln", "eprint", "format", "format_args", "panic"}


def positionalize_calls(toks):
    out, i = [], 0
    while i < len(toks):
        if toks[i] in FORMAT_CALLS and toks[i + 1:i + 3] == ["!", "("]:
            end = group_end(toks, i + 2)
            if end is not None:
                pieces = split_commas(toks[i + 3:end - 1])
                if pieces and pieces[-1] == []:
                    pieces.pop()
                if pieces and len(pieces[0]) == 1 and unquote(pieces[0][0]) is not None:
                    made = positionalize(unquote(pieces[0][0]), pieces[1:])
                    if made is not None:
                        fmt, captures = made
                        fixed = ['"' + fmt + '"']
                        rest = [positionalize_calls(p) for p in pieces[1:]] + [[name] for name in captures]
                        out += [toks[i], "!", "("] + fixed
                        for piece in rest:
                            out += [","] + piece
                        out.append(")")
                        i = end
                        continue
        out.append(toks[i])
        i += 1
    return out



ASSIGN_OPS = {"=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="}


def unwrap_parens(toks):
    """Unwrap lust's precedence parens where rustc prints none."""
    changed = True
    while changed:
        changed = False
        out, i = [], 0
        while i < len(toks):
            if toks[i] == "(":
                end = group_end(toks, i)
                if end is not None:
                    inner = toks[i + 1:end - 1]
                    # Doubled parens whose whole content is one group.
                    if inner[:1] == ["("] and group_end(inner, 0) == len(inner):
                        out += inner
                        i = end
                        changed = True
                        continue
                    # A parenthesized assignment as a whole statement.
                    prev = out[-1] if out else None
                    nxt = toks[end] if end < len(toks) else None
                    if (prev is None or prev in ("{", ";")) and nxt == ";" and any(t in ASSIGN_OPS for t in inner):
                        depth = 0
                        top_assign = False
                        for t in inner:
                            if t in OPEN:
                                depth += 1
                            elif t in CLOSE:
                                depth -= 1
                            elif t in ASSIGN_OPS and depth == 0:
                                top_assign = True
                        if top_assign:
                            out += inner
                            i = end
                            changed = True
                            continue
            out.append(toks[i])
            i += 1
        toks = out
    return toks


def map_alloc_vec(toks):
    """rustc's list-form `vec!` expansion back to `vec ! [ ... ]`."""
    prefix = ["::", "alloc", "::", "boxed", "::", "box_assume_init_into_vec_unsafe", "(",
              "::", "alloc", "::", "intrinsics", "::", "write_box_via_move", "(",
              "::", "alloc", "::", "boxed", "::", "Box", "::", "new_uninit", "(", ")", ","]
    out, i = [], 0
    while i < len(toks):
        if toks[i:i + len(prefix)] == prefix and toks[i + len(prefix):i + len(prefix) + 1] == ["["]:
            end = group_end(toks, i + len(prefix))
            if end is not None and toks[end:end + 2] == [")", ")"]:
                inner = map_alloc_vec(toks[i + len(prefix) + 1:end - 1])
                out += ["vec", "!", "["] + inner + ["]"]
                i = end + 2
                continue
        out.append(toks[i])
        i += 1
    return out


def drop_arm_commas(toks):
    """Drop the optional comma after a block-bodied match arm.

    Source may write `pat => { ... }, next => ...`; rustc's pretty
    printer drops that comma while lust prints the template token.
    A comma right after `}` is dropped only when an arm head (`=>`
    at the same depth) follows before any statement end.
    """
    out = []
    for i, tok in enumerate(toks):
        if tok == "," and out and out[-1] == "}":
            depth, j, drop = 0, i + 1, False
            while j < len(toks):
                t = toks[j]
                if t in OPEN:
                    depth += 1
                elif t in CLOSE:
                    if depth == 0:
                        break
                    depth -= 1
                elif depth == 0 and t == "=>":
                    drop = True
                    break
                elif depth == 0 and t in (";", ","):
                    break
                j += 1
            if drop:
                continue
        out.append(tok)
    return out

ESCAPES = {"n": "\n", "r": "\r", "t": "\t", "\\": "\\", "0": "\0", "'": "'", '"': '"'}


def literal_value(body, is_bytes):
    """The value of a string or byte string literal body, or None."""
    out, i = [], 0
    while i < len(body):
        c = body[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        e = body[i + 1:i + 2]
        if e in ESCAPES:
            out.append(ESCAPES[e])
            i += 2
        elif e == "x":
            out.append(chr(int(body[i + 2:i + 4], 16)))
            i += 4
        elif e == "u" and not is_bytes:
            close = body.find("}", i)
            out.append(chr(int(body[i + 3:close], 16)))
            i = close + 1
        elif e == "\n":
            # Line continuation: the newline and the next line's indent go.
            i += 2
            while i < len(body) and body[i] in " \t\n\r":
                i += 1
        else:
            return None
    return "".join(out)


def canon_literal(tok):
    """One spelling per string or byte string literal value: `\\`, `"`,
    and control characters escaped, `\\xNN` for other bytes outside
    printable ASCII in byte strings, everything else as itself."""
    is_bytes = tok.startswith('b"')
    if not (tok.startswith('"') or is_bytes) or len(tok) < 2 or tok[-1] != '"':
        return tok
    value = literal_value(tok[2 if is_bytes else 1:-1], is_bytes)
    if value is None:
        return tok
    out = []
    for c in value:
        if c in "\\\"":
            out.append("\\" + c)
        elif c in "\n\r\t\0":
            out.append({"\n": "\\n", "\r": "\\r", "\t": "\\t", "\0": "\\0"}[c])
        elif is_bytes and not (" " <= c <= "~"):
            out.append("\\x%02x" % ord(c))
        elif not is_bytes and ord(c) < 0x20:
            out.append("\\u{%x}" % ord(c))
        else:
            out.append(c)
    return ('b"' if is_bytes else '"') + "".join(out) + '"'


OPTION_PATH = ["::", "core", "::", "option", "::", "Option", "::"]


def map_option_env(toks):
    """rustc's `option_env!` values back to lust's prelude forms."""
    out, i = [], 0
    while i < len(toks):
        if toks[i:i + 7] == OPTION_PATH and toks[i + 7:i + 8] == ["Some"]:
            out.append("Some")
            i += 8
            continue
        if toks[i:i + 7] == OPTION_PATH and toks[i + 7:i + 14] == ["None", "::", "<", "&", "'static", "str", ">"]:
            out += ["None", "::", "<", "&", "str", ">"]
            i += 14
            continue
        out.append(toks[i])
        i += 1
    return out


VEC_NEW = ["::", "alloc", "::", "vec", "::", "Vec", "::", "new", "(", ")"]


def map_vec_new(toks):
    """rustc's empty `vec![]` expansion back to `vec ! [ ]`."""
    out, i = [], 0
    while i < len(toks):
        if toks[i:i + len(VEC_NEW)] == VEC_NEW:
            out += ["vec", "!", "[", "]"]
            i += len(VEC_NEW)
            continue
        out.append(toks[i])
        i += 1
    return out


def split_gg(toks):
    """A `>>` token as two `>` closes (see the header)."""
    out = []
    for t in toks:
        if t == ">>":
            out += [">", ">"]
        else:
            out.append(t)
    return out


def drop_brace_commas(toks):
    """Drop a comma right before `}` (see the header)."""
    return [t for i, t in enumerate(toks) if not (t == "," and toks[i + 1:i + 2] == ["}"])]


def canonicalize(toks):
    toks = strip_prelude(toks)
    toks = drop_macro_use_attrs(toks)
    toks = drop_macro_rules(toks)
    toks = map_alloc_format(toks)
    toks = map_alloc_vec(toks)
    toks = map_vec_new(toks)
    toks = map_print(toks)
    toks = map_panic(toks)
    toks = positionalize_calls(toks)
    toks = unwrap_parens(toks)
    toks = drop_arm_commas(toks)
    toks = drop_brace_commas(toks)
    toks = split_gg(toks)
    toks = map_option_env(toks)
    return [canon_literal(t) for t in toks]


def main():
    # Space-joined tokens, wrapped at 100 columns. Wrapping is
    # cosmetic: normalize.py on its own output gives the same text.
    # The committed .expanded files are LF. Windows text mode would
    # write CR LF and every line would differ.
    out = sys.stdout
    if hasattr(out, "reconfigure"):
        try:
            out.reconfigure(newline="\n")
        except (AttributeError, OSError, ValueError):
            pass
    col = 0
    for tok in canonicalize(list(tokens(sys.stdin.read()))):
        if col and col + 1 + len(tok) > 100:
            out.write("\n")
            col = 0
        if col:
            out.write(" ")
            col += 1
        out.write(tok)
        col += len(tok)
    if col:
        out.write("\n")


if __name__ == "__main__":
    main()
