// The std loader (docs/ARCH.md sections 3.1 and 3.8, serial refactor S5).
//
// Parses and checks the shipped `library/` source files as their own
// crates, resolves the lang item table, and produces the splice text
// the driver appends to a user compile that can mention a source std
// fn. The driver calls `load_std` at most once per session and caches
// the result (std is immutable inside a session, section 3.8); the
// common compile that mentions no source std fn never calls it at all.
//
// Splice model, until multi-crate collect lands (W-RESOLVE): a spliced
// std file rides in the user crate under mangled fn names, in a byte
// range after the user file. The standalone check here runs under the
// shipped crate's own stable hash, so def identity and intrinsic
// privilege are proven in the shipped crate first; the splice range
// then carries the same provenance into the user compile. Only
// non-generic fns route this way (section 9.2 S5). The std methods of
// Option and Result (core's option.rs and result.rs) are impls, not
// routed fns: they splice whole, unrenamed, and check resolves them in
// their own scope (`StdLoad::methods`, `Ast::lang_methods`). A compile
// only reads their text; the tests check them.
//
// Every failure here is a compiler installation error (a broken
// `library/`, a registry row with no source fn, a missing required
// lang item), returned as one message string; the driver reports it
// with exit code 3. It is never a user diagnostic.
use crate::ast::{Ast, ItemKind};
use crate::check::collect;
use crate::def::{CrateIds, LOCAL_CRATE, LocalDefId};
use crate::diag::{Diag, FileId};
use crate::intern::{Interner, Known};
use crate::ty::TyIntern;
use crate::ty::lang::{self, LangItems, ShippedCrate, StdFnDef};
use crate::{borrowck, check, lexer, lower, parser};

/// One shipped source file the loader knows: its crate, its module path
/// inside the crate, and its path under `library/`. The built-in text
/// comes from `builtin_text`; `LUST_STD_SRC` overrides the whole tree.
struct SourceFile {
    krate: ShippedCrate,
    module: &'static [&'static str],
    rel: &'static str,
}

const SOURCE_FILES: &[SourceFile] = &[
    SourceFile { krate: ShippedCrate::Core, module: &[], rel: "core/src/lib.rs" },
    SourceFile { krate: ShippedCrate::Core, module: &["intrinsics"], rel: "core/src/intrinsics.rs" },
    SourceFile { krate: ShippedCrate::Core, module: &["option"], rel: "core/src/option.rs" },
    SourceFile { krate: ShippedCrate::Core, module: &["result"], rel: "core/src/result.rs" },
    SourceFile { krate: ShippedCrate::Alloc, module: &[], rel: "alloc/src/lib.rs" },
    SourceFile { krate: ShippedCrate::Std, module: &[], rel: "std/src/lib.rs" },
    SourceFile { krate: ShippedCrate::Std, module: &["prelude"], rel: "std/src/prelude.rs" },
    SourceFile { krate: ShippedCrate::Std, module: &["process"], rel: "std/src/process.rs" },
];

/// The `library/` text built into the compiler, by relative path.
fn builtin_text(rel: &str) -> Option<&'static str> {
    Some(match rel {
        "core/src/lib.rs" => include_str!("../library/core/src/lib.rs"),
        "core/src/intrinsics.rs" => include_str!("../library/core/src/intrinsics.rs"),
        "core/src/option.rs" => include_str!("../library/core/src/option.rs"),
        "core/src/result.rs" => include_str!("../library/core/src/result.rs"),
        "alloc/src/lib.rs" => include_str!("../library/alloc/src/lib.rs"),
        "std/src/lib.rs" => include_str!("../library/std/src/lib.rs"),
        "std/src/prelude.rs" => include_str!("../library/std/src/prelude.rs"),
        "std/src/process.rs" => include_str!("../library/std/src/process.rs"),
        "core/src/time.rs" => include_str!("../library/core/src/time.rs"),
        "std/src/time.rs" => include_str!("../library/std/src/time.rs"),
        "core/src/marker.rs" => include_str!("../library/core/src/marker.rs"),
        "core/src/cmp.rs" => include_str!("../library/core/src/cmp.rs"),
        "core/src/ops.rs" => include_str!("../library/core/src/ops.rs"),
        "core/src/iter.rs" => include_str!("../library/core/src/iter.rs"),
        "core/src/cell.rs" => include_str!("../library/core/src/cell.rs"),
        "core/src/char.rs" => include_str!("../library/core/src/char.rs"),
        "core/src/mem.rs" => include_str!("../library/core/src/mem.rs"),
        "std/src/sync.rs" => include_str!("../library/std/src/sync.rs"),
        "std/src/panic.rs" => include_str!("../library/std/src/panic.rs"),
        "alloc/src/collections.rs" => include_str!("../library/alloc/src/collections.rs"),
        "core/src/error.rs" => include_str!("../library/core/src/error.rs"),
        "std/src/net.rs" => include_str!("../library/std/src/net.rs"),
        "std/src/alloc.rs" => include_str!("../library/std/src/alloc.rs"),
        "std/src/unix_net.rs" => include_str!("../library/std/src/unix_net.rs"),
        "std/src/fs_time.rs" => include_str!("../library/std/src/fs_time.rs"),
        "std/src/backtrace.rs" => include_str!("../library/std/src/backtrace.rs"),
        _ => return None,
    })
}

/// The text of one shipped file: from `$LUST_STD_SRC/<rel>` when the
/// override is set (section 3.8), else the built-in copy.
pub(crate) fn source_text(rel: &str) -> Result<String, String> {
    match std::env::var_os("LUST_STD_SRC") {
        Some(dir) if !dir.is_empty() => {
            let path = std::path::Path::new(&dir).join(rel);
            std::fs::read_to_string(&path)
                .map_err(|e| format!("lust std load failed: cannot read {} from LUST_STD_SRC: {e}", path.display()))
        }
        _ => builtin_text(rel).map(str::to_string).ok_or_else(|| format!("lust std load failed: no built-in text for library/{rel}")),
    }
}

/// One checked shipped file, kept for lang lookup and splice building.
struct CheckedFile {
    krate: ShippedCrate,
    module: &'static [&'static str],
    rel: &'static str,
    /// The source with `#[lust_lang]` lines blanked (same byte length,
    /// so spans into this text stay exact).
    clean: String,
    interner: std::sync::Arc<Interner>,
    ast: std::sync::Arc<Ast>,
}

/// What a session needs from one std load. Most compiles only need
/// the lang enum methods, so the rest (the `STD_FNS` files, the lang
/// items, the std source modules) loads on first use (`StdLoad::rest`).
pub struct StdLoad {
    /// The std methods of the lang enums (core's option.rs and
    /// result.rs, impls only), spliced whole when a compile names one.
    /// The text never changes inside a session, so the daemon's reused
    /// chunks and instances stay put.
    methods: String,
    rest: std::sync::OnceLock<Result<StdRest, String>>,
    /// What each splice block asks of std itself (`StdLoad::splice_asks`):
    /// the lang methods, the `STD_FNS` files, then one per std module.
    block_asks: Vec<std::sync::OnceLock<StdPlan>>,
}

/// The std one resolver compile runs with: what the splice carries,
/// and whether the shipped std platform crate (W-SHIM) joins.
///
/// A compile's plan is a function of its user files only
/// (`StdLoad::close`): the OR of `StdPlan::asks` over every file but
/// the platform crate's, closed over what the std text asks itself.
/// The driver resolves with a guess (a daemon's last plan), then
/// compares the plan of the files that pass saw; it resolves again
/// only when they differ. The user files a pass sees do not depend on
/// the plan (a splice adds inline modules only, the platform crate is
/// its own job), so warm and cold reach the same final plan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StdPlan {
    pub needs: lang::StdNeeds,
    pub platform: bool,
}

impl StdPlan {
    /// What one file's text asks: its std splice needs and whether it
    /// can name a std platform module.
    pub fn asks(text: &str) -> StdPlan {
        StdPlan { needs: lang::std_needs(text), platform: crate::resolve::worklist::mentions_std_platform(text) }
    }

    /// Both plans' asks.
    pub fn or(self, o: StdPlan) -> StdPlan {
        let (a, b) = (self.needs, o.needs);
        StdPlan {
            needs: lang::StdNeeds { fns: a.fns || b.fns, methods: a.methods || b.methods, modules: a.modules | b.modules },
            platform: self.platform || o.platform,
        }
    }

    /// A new session's first guess at what its user files ask: the root
    /// file plus every file its `mod name;` declarations reach, found by
    /// a text scan (`mod_decls`), with no cfg and no macros. The driver
    /// checks the guess against the files the resolve sees, so a wrong
    /// guess only costs one more resolve pass.
    /// `root_asks` is `StdPlan::asks(root_text)`, which the caller knows.
    pub fn guess_crate(root_path: &std::path::Path, root_text: &str, root_asks: StdPlan) -> StdPlan {
        let dir = root_path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let plan = root_asks;
        let mut seen: Vec<std::path::PathBuf> = vec![root_path.to_path_buf()];
        // (file, its children's directory, its text)
        let mut queue: Vec<(std::path::PathBuf, std::path::PathBuf, String)> = Vec::new();
        let mut push_children = |text: &str, file_dir: &std::path::Path, dir: &std::path::Path, queue: &mut Vec<_>| {
            for (path, by_attr) in mod_files(text, file_dir, dir) {
                // A cap for a root in a huge tree; a short guess is still checked.
                if seen.len() < 20_000 && !seen.contains(&path) {
                    seen.push(path.clone());
                    if let Ok(t) = std::fs::read_to_string(&path) {
                        // A `#[path]` file's children load as a mod.rs file's do.
                        let dir =
                            if by_attr { path.parent().map(std::path::Path::to_path_buf).unwrap_or_default() } else { child_dir(&path) };
                        queue.push((path, dir, t));
                    }
                }
            }
        };
        push_children(root_text, &dir, &dir, &mut queue);
        let mut texts = Vec::new();
        while let Some((path, dir, text)) = queue.pop() {
            let file_dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
            push_children(&text, &file_dir, &dir, &mut queue);
            texts.push(text);
        }
        texts.iter().fold(plan, |p, t| p.or(StdPlan::asks(t)))
    }
}

/// A guess at what the std platform crate's files ask (`guess_crate`
/// over its root), read once per process. Only feeds a first guess.
pub fn guess_platform() -> StdPlan {
    static GUESS: std::sync::OnceLock<StdPlan> = std::sync::OnceLock::new();
    *GUESS.get_or_init(|| {
        let root = crate::resolve::worklist::std_platform_root();
        let text = std::fs::read_to_string(&root).unwrap_or_default();
        StdPlan::guess_crate(&root, &text, StdPlan::asks(&text))
    })
}

/// The directory a module file's own `mod name;` children load from:
/// its parent for a `mod.rs`, else `<parent>/<stem>`.
fn child_dir(path: &std::path::Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or(std::path::Path::new(""));
    match path.file_stem().and_then(|s| s.to_str()) {
        Some("mod") | None => parent.to_path_buf(),
        Some(stem) => parent.join(stem),
    }
}

/// The files the `mod name;` declarations in `text` name, for a file
/// in `file_dir` whose children load from `dir`: `dir/name.rs` or
/// `dir/name/mod.rs`, or a `#[path = "p"]` value (relative to
/// `file_dir`, or inside an inline module to its directory), with
/// inline `mod m { .. }` blocks as directories. True marks a `#[path]`
/// file. Comments, strings, and char literals are skipped; a
/// declaration a macro writes is missed.
fn mod_files(text: &str, file_dir: &std::path::Path, dir: &std::path::Path) -> Vec<(std::path::PathBuf, bool)> {
    let b = text.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    // Most files declare no file module: one substring search, no walk.
    let decl_at = |i: usize| {
        let mut j = i + 3;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let n = j;
        while j < b.len() && ident(b[j]) {
            j += 1;
        }
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        j > n && b.get(j) == Some(&b';')
    };
    if !text.match_indices("mod").any(|(i, _)| (i == 0 || !ident(b[i - 1])) && decl_at(i)) {
        return Vec::new();
    }
    let word_at = |i: usize| -> &str {
        let mut j = i;
        while j < b.len() && ident(b[j]) {
            j += 1;
        }
        &text[i..j]
    };
    let skip_ws = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    };
    let mut out = Vec::new();
    // One entry per open brace: the directory an inline module opened,
    // or None for any other brace.
    let mut braces: Vec<Option<std::path::PathBuf>> = Vec::new();
    let mut path_attr: Option<String> = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut depth = 0;
                while i < b.len() {
                    if b[i..].starts_with(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if b[i..].starts_with(b"*/") {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            b'"' => i = skip_str(b, i + 1, 0),
            b'r' if (i == 0 || !ident(b[i - 1])) && matches!(b.get(i + 1), Some(b'"' | b'#')) => {
                let hashes = b[i + 1..].iter().take_while(|&&h| h == b'#').count();
                if b.get(i + 1 + hashes) == Some(&b'"') {
                    i = skip_str(b, i + 2 + hashes, hashes);
                } else {
                    i += 1;
                }
                continue;
            }
            b'\'' => {
                // A char literal ('x', '\n', '\u{..}', a multibyte char), or a lifetime.
                if b.get(i + 1) == Some(&b'\\') {
                    i += 2;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                } else if let Some(ch) = text.get(i + 1..).and_then(|r| r.chars().next())
                    && b.get(i + 1 + ch.len_utf8()) == Some(&b'\'')
                {
                    i += 1 + ch.len_utf8();
                }
            }
            b'#' if b.get(skip_ws(i + 1)) == Some(&b'[') => {
                let j = skip_ws(skip_ws(i + 1) + 1);
                if word_at(j) == "path" {
                    let k = skip_ws(j + 4);
                    if b.get(k) == Some(&b'=') && b.get(skip_ws(k + 1)) == Some(&b'"') {
                        let s = skip_ws(k + 1) + 1;
                        let e = skip_str(b, s, 0);
                        path_attr = text.get(s..e.saturating_sub(1)).map(str::to_string);
                        i = e;
                        continue;
                    }
                }
                i = j;
                continue;
            }
            b'{' => {
                braces.push(None);
                path_attr = None;
            }
            b'}' => {
                braces.pop();
                path_attr = None;
            }
            b';' => path_attr = None,
            _ if ident(c) => {
                let w = word_at(i);
                if w == "mod" && (i == 0 || !ident(b[i - 1])) {
                    let n = skip_ws(i + 3);
                    let name = word_at(n);
                    let after = skip_ws(n + name.len());
                    let inline = braces.iter().rev().find_map(|d| d.clone());
                    let here = inline.clone().unwrap_or_else(|| dir.to_path_buf());
                    if !name.is_empty() && b.get(after) == Some(&b';') {
                        match path_attr.take() {
                            Some(p) => out.push((inline.unwrap_or_else(|| file_dir.to_path_buf()).join(p), true)),
                            None => {
                                let file = here.join(format!("{name}.rs"));
                                out.push((if file.exists() { file } else { here.join(name).join("mod.rs") }, false));
                            }
                        }
                        i = after + 1;
                        continue;
                    }
                    if !name.is_empty() && b.get(after) == Some(&b'{') {
                        braces.push(Some(here.join(path_attr.take().unwrap_or_else(|| name.to_string()))));
                        i = after + 1;
                        continue;
                    }
                }
                i += w.len();
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The index just past the closing quote of a string body starting at
/// `i` (after the opening quote), for a raw string with `hashes` hashes.
fn skip_str(b: &[u8], mut i: usize, hashes: usize) -> usize {
    while i < b.len() {
        if hashes == 0 && b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == b'"' && b[i + 1..].iter().take(hashes).filter(|&&h| h == b'#').count() == hashes {
            return i + 1 + hashes;
        }
        i += 1;
    }
    b.len()
}

/// The part of a std load past the lang enum methods.
pub struct StdRest {
    /// The resolved lang items of this load. Read by later waves; S5
    /// only proves that required items resolve.
    #[allow(dead_code)] // First consumers are W-STD and W-MONO; the load guarantee is the S5 gate.
    pub lang: LangItems,
    /// Splice text of the `STD_FNS` files, per shipped crate.
    fn_splices: Vec<(ShippedCrate, String)>,
    /// The implemented `STD_FNS` rows whose bodies ride in `fn_splices`.
    fns: Vec<&'static StdFnDef>,
    /// The std source modules (src/stdmods.rs) as hidden-module splice
    /// text. Their bodies call lang enum methods.
    modules: Vec<(ShippedCrate, String)>,
}

/// One compile's std splice (`StdLoad::splice`).
pub struct Splice {
    /// Text blocks per shipped crate, in load order. The driver appends
    /// each after the user file and records its byte range.
    pub blocks: Vec<(ShippedCrate, String)>,
    /// The index in `blocks` of the lang method chunks, if any.
    pub methods_block: Option<usize>,
    /// The `STD_FNS` rows whose bodies ride in `blocks`.
    pub routes: Vec<&'static StdFnDef>,
}

impl StdLoad {
    /// The splice for one compile: the lang enum methods when a method
    /// (or a std source module, whose bodies call them) is named, the
    /// `STD_FNS` files when one of their fns is, and the std source
    /// modules when one is.
    pub fn splice(&self, needs: lang::StdNeeds) -> Result<Splice, String> {
        let mut blocks: Vec<(ShippedCrate, String)> = Vec::new();
        let mut methods_block = None;
        if needs.methods || needs.modules != 0 {
            methods_block = Some(0);
            blocks.push((ShippedCrate::Core, self.methods.clone()));
        }
        let mut routes = Vec::new();
        if needs.fns || needs.modules != 0 {
            let rest = self.rest()?;
            if needs.fns {
                blocks.extend(rest.fn_splices.iter().cloned());
                routes = rest.fns.clone();
            }
            // Rows in `STD_MODULES` order, as `load_rest` reads them.
            let named = rest.modules.iter().enumerate().filter(|(k, _)| needs.modules & (1 << k) != 0);
            blocks.extend(named.map(|(_, b)| b.clone()));
        }
        Ok(Splice { blocks, methods_block, routes })
    }

    /// What the splice for `needs` asks of std itself (its methods call
    /// lang methods, a std module can name a platform module).
    /// It takes the blocks `splice` takes, each block's asks scanned once.
    fn splice_asks(&self, needs: lang::StdNeeds) -> Result<StdPlan, String> {
        let block = |k: usize, text: &mut dyn Iterator<Item = &String>| {
            *self.block_asks[k].get_or_init(|| text.fold(StdPlan::default(), |p, t| p.or(StdPlan::asks(t))))
        };
        let mut plan = StdPlan::default();
        if needs.methods || needs.modules != 0 {
            plan = plan.or(block(0, &mut std::iter::once(&self.methods)));
        }
        if needs.fns || needs.modules != 0 {
            let rest = self.rest()?;
            if needs.fns {
                plan = plan.or(block(1, &mut rest.fn_splices.iter().map(|b| &b.1)));
            }
            for (k, (_, text)) in rest.modules.iter().enumerate().filter(|(k, _)| needs.modules & (1 << k) != 0) {
                plan = plan.or(block(2 + k, &mut std::iter::once(text)));
            }
        }
        Ok(plan)
    }

    /// The plan of a compile whose user files ask `user` (see
    /// `StdPlan`): the least plan that holds `user`, what its splice
    /// asks, and, when it takes the platform crate, what the platform
    /// files ask (`platform`, the OR of their asks; `None` when the pass
    /// that saw `user` ran without the platform crate, so the next pass
    /// adds them).
    pub fn close(&self, user: StdPlan, platform: Option<StdPlan>) -> Result<StdPlan, String> {
        let mut plan = user;
        loop {
            let mut next = user.or(self.splice_asks(plan.needs)?);
            if next.platform
                && let Some(p) = platform
            {
                next = next.or(p);
            }
            // Each step only adds asks, so this ends within a few steps.
            if next == plan {
                return Ok(plan);
            }
            plan = next;
        }
    }

    /// The rest of the load, read and parsed on first use.
    pub fn rest(&self) -> Result<&StdRest, String> {
        self.rest.get_or_init(|| load_rest(&mut |rel| source_text(rel), false)).as_ref().map_err(String::clone)
    }
}

/// Extracts the `#[lust_lang = "<name>"]` marks from shipped source:
/// returns the source with each mark line blanked (offsets preserved)
/// and `(lang name, byte offset of the next line)` per mark. A mark
/// must sit alone on its line until the parser learns the attribute.
/// A line that starts a mark but does not parse as one is an
/// installation error, so a typo cannot silently drop a lang item;
/// plain mentions in comments are fine.
fn scan_lang_attrs(src: &str, rel: &str) -> Result<(String, Vec<(String, u32)>), String> {
    let mut clean = String::with_capacity(src.len());
    let mut marks = Vec::new();
    for line in src.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let at = clean.len() as u32 + line.len() as u32;
        if body.trim_start().starts_with("#[lust_lang") {
            let t = body.trim();
            let inner = t
                .strip_prefix("#[lust_lang")
                .and_then(|r| r.trim().strip_prefix('='))
                .map(str::trim)
                .and_then(|r| r.strip_suffix(']'))
                .map(str::trim)
                .filter(|r| r.len() >= 2 && r.starts_with('"') && r.ends_with('"'))
                .map(|r| r[1..r.len() - 1].to_string());
            let Some(name) = inner.filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')) else {
                return Err(format!(
                    "lust std load failed: library/{rel}: bad #[lust_lang] mark (write it alone on its line as #[lust_lang = \"name\"])"
                ));
            };
            marks.push((name, at));
            clean.extend(line.bytes().map(|b| if b == b'\n' { '\n' } else { ' ' }));
        } else {
            clean.push_str(line);
        }
    }
    Ok((clean, marks))
}

/// One diagnostic as one line of an installation error.
fn diag_line(rel: &str, d: &Diag) -> String {
    format!("library/{rel}: {}: {} @{}.{}", d.code, d.message, d.span.lo, d.span.hi)
}

/// Parses one shipped file and, when `validate` is set, collects,
/// checks, lowers, and borrow-checks it as its own crate. Returns the
/// file plus its lang marks.
fn check_file(file: &SourceFile, text: &str, validate: bool) -> Result<(CheckedFile, Vec<(String, u32)>), String> {
    let (clean, marks) = scan_lang_attrs(text, file.rel)?;
    let interner = Interner::default();
    let known = Known::new(&interner);
    let lexed = lexer::lex(&clean, FileId(0), 0, &interner)
        .and_then(|l| {
            let ctx = crate::expand::ExpandCtx::for_file(&interner, &known, std::path::Path::new(file.rel), &clean, false);
            crate::expand::expand_lexed(l, &ctx)
        })
        .map_err(|d| format!("lust std load failed: {}", diag_line(file.rel, &d)))?;
    let mut ast = parser::parse(&lexed, &interner, &known).map_err(|d| format!("lust std load failed: {}", diag_line(file.rel, &d)))?;
    // The whole file is shipped source: intrinsic calls in it run the
    // privilege test against this file's crate, and its impls of lang
    // types collect as std impls.
    ast.shipped = vec![(0, clean.len() as u32, file.krate)];
    if is_lang_method_file(file.krate, file.module) {
        ast.lang_methods = (0, clean.len() as u32);
    }
    let (ast, interner) = (std::sync::Arc::new(ast), std::sync::Arc::new(interner));
    if !validate {
        return Ok((CheckedFile { krate: file.krate, module: file.module, rel: file.rel, clean, interner, ast }, marks));
    }
    let ids = CrateIds { local: LOCAL_CRATE, stable: file.krate.stable_hash() };
    let mut program = collect::collect(ast.clone(), interner.clone(), std::sync::Arc::new(known), ids)
        .map_err(|ds| format!("lust std load failed: {}", ds.iter().map(|d| diag_line(file.rel, d)).collect::<Vec<_>>().join("; ")))?;
    // Collect ran mono: templates were checked (and borrow-checked)
    // against their param envs there, as in the driver, except the lang
    // enum methods, which a compile checks only when it instantiates
    // one: check them all here. Plain fns, instances, and closure
    // bodies check below; stubs never do.
    let lang_errs = crate::check::generics::check_lang_templates(&mut program);
    if !lang_errs.is_empty() {
        return Err(format!("lust std load failed: {}", lang_errs.iter().map(|d| diag_line(file.rel, d)).collect::<Vec<_>>().join("; ")));
    }
    for i in 0..program.fns.len() {
        use crate::check::generics::FnClass;
        let f = LocalDefId(i as u32);
        if !matches!(program.gx.fn_class.get(i).copied().unwrap_or_default(), FnClass::Plain | FnClass::Instance | FnClass::Closure) {
            continue;
        }
        let (checked, errs) = check::check_fn(&program, f);
        if !errs.is_empty() {
            return Err(format!("lust std load failed: {}", errs.iter().map(|d| diag_line(file.rel, d)).collect::<Vec<_>>().join("; ")));
        }
        let body = lower::lower(&program, f, checked);
        let errs = borrowck::borrowck(&program, &body);
        if !errs.is_empty() {
            return Err(format!("lust std load failed: {}", errs.iter().map(|d| diag_line(file.rel, d)).collect::<Vec<_>>().join("; ")));
        }
    }
    Ok((CheckedFile { krate: file.krate, module: file.module, rel: file.rel, clean, interner, ast }, marks))
}

/// Loads the whole shipped library. Called at most once per session
/// (the driver caches the result), and only when a user compile can
/// mention a source std fn. The built-in text is checked by this
/// module's tests (`shipped_library_loads`), so a compile only parses
/// it; a `LUST_STD_SRC` tree is checked in full at every load.
pub fn load_std() -> Result<StdLoad, String> {
    let validate = std::env::var_os("LUST_STD_SRC").is_some_and(|d| !d.is_empty());
    load_std_from(&mut |rel| source_text(rel), validate)
}

/// `load_std` over a caller-supplied text source, so tests can feed
/// fixture shipped source without touching the real `library/`.
fn load_std_from(read: &mut dyn FnMut(&str) -> Result<String, String>, validate: bool) -> Result<StdLoad, String> {
    let mut methods = String::new();
    for file in SOURCE_FILES.iter().filter(|f| is_lang_method_file(f.krate, f.module)) {
        let text = read(file.rel)?;
        // A compile only needs the text; validation checks the files as
        // a crate (their templates included).
        let clean = if validate {
            let (checked, _) = check_file(file, &text, true)?;
            if let Some(it) = checked.ast.items.iter().find(|it| !matches!(it.kind, ItemKind::Impl(_))) {
                return Err(format!(
                    "lust std load failed: library/{}: only impls can ride in a lang method file (item at {})",
                    file.rel, it.span.lo
                ));
            }
            checked.clean
        } else {
            scan_lang_attrs(&text, file.rel)?.0
        };
        methods.push_str(&clean);
    }
    let rest = std::sync::OnceLock::new();
    if validate {
        let _ = rest.set(Ok(load_rest(read, true)?));
    }
    let block_asks = (0..2 + crate::stdmods::STD_MODULES.len()).map(|_| std::sync::OnceLock::new()).collect();
    Ok(StdLoad { methods, rest, block_asks })
}

/// Loads the shipped files past the lang enum methods: the `STD_FNS`
/// files and the lang items they mark, and the std source modules.
fn load_rest(read: &mut dyn FnMut(&str) -> Result<String, String>, validate: bool) -> Result<StdRest, String> {
    let mut files: Vec<CheckedFile> = Vec::new();
    let mut marks: Vec<(usize, String, u32)> = Vec::new(); // (file index, lang name, next-line offset)
    for file in SOURCE_FILES.iter().filter(|f| !is_lang_method_file(f.krate, f.module)) {
        let text = read(file.rel)?;
        let (checked, file_marks) = check_file(file, &text, validate)?;
        let idx = files.len();
        marks.extend(file_marks.into_iter().map(|(n, at)| (idx, n, at)));
        files.push(checked);
    }
    let lang_items = resolve_lang_items(&files, &marks)?;
    let (fn_splices, fns) = build_splices(&files)?;
    let mut modules = Vec::new();
    for m in crate::stdmods::STD_MODULES {
        let text = read(m.rel)?;
        modules.push((m.krate, module_splice(m, &text, validate)?));
    }
    Ok(StdRest { lang: lang_items, fn_splices, fns, modules })
}

/// The splice text of one std source module (src/stdmods.rs). With
/// `validate`, the file must lex, expand, and parse on its own (the
/// built-in text is validated by the tests, so a compile skips it); its
/// items are checked in the user compile, inside their hidden module,
/// because they use imports and module paths the standalone flat check
/// here does not resolve.
fn module_splice(m: &crate::stdmods::StdModule, text: &str, validate: bool) -> Result<String, String> {
    let (clean, marks) = scan_lang_attrs(text, m.rel)?;
    if !marks.is_empty() {
        return Err(format!("lust std load failed: library/{}: a std source module cannot hold #[lust_lang] marks yet", m.rel));
    }
    if !validate {
        return Ok(crate::stdmods::splice_text(m, &clean));
    }
    let interner = Interner::default();
    let known = Known::new(&interner);
    let lexed = lexer::lex(&clean, FileId(0), 0, &interner)
        .and_then(|l| {
            let ctx = crate::expand::ExpandCtx::for_file(&interner, &known, std::path::Path::new(m.rel), &clean, false);
            crate::expand::expand_lexed(l, &ctx)
        })
        .map_err(|d| format!("lust std load failed: {}", diag_line(m.rel, &d)))?;
    parser::parse(&lexed, &interner, &known).map_err(|d| format!("lust std load failed: {}", diag_line(m.rel, &d)))?;
    Ok(crate::stdmods::splice_text(m, &clean))
}

/// Fills the lang table: builtin defs first, then one def per
/// `#[lust_lang]` mark (the item after the mark). Fails on an unknown
/// or duplicate mark, a mark with no item after it, or a required
/// item with no def.
fn resolve_lang_items(files: &[CheckedFile], marks: &[(usize, String, u32)]) -> Result<LangItems, String> {
    // Any checked file's program carries the same builtin defs (they are
    // stable path hashes); rebuild a throwaway store to read them.
    let builtins = *crate::ty::GlobalTys::new().builtins();
    let mut items = LangItems::from_builtins(builtins);
    for &(fi, ref name, at) in marks {
        let file = &files[fi];
        let Some(item) = lang::lang_item_by_name(name) else {
            return Err(format!("lust std load failed: library/{}: unknown lang item `{name}`", file.rel));
        };
        let want_crate = lang::LANG_ITEMS[item as usize].krate;
        if want_crate != file.krate {
            return Err(format!(
                "lust std load failed: library/{}: lang item `{name}` belongs to crate `{}`, not `{}`",
                file.rel,
                want_crate.name(),
                file.krate.name()
            ));
        }
        if items.get(item).is_some() && !matches!(item, lang::LangItem::Option | lang::LangItem::String | lang::LangItem::Vec) {
            return Err(format!("lust std load failed: lang item `{name}` is marked more than once"));
        }
        // The marked item is the first item that starts at or after the
        // mark's next-line offset.
        let found = file.ast.items.iter().filter(|it| it.span.lo >= at).min_by_key(|it| it.span.lo);
        let Some(found) = found else {
            return Err(format!("lust std load failed: library/{}: #[lust_lang = \"{name}\"] has no item after it", file.rel));
        };
        let item_name = match &found.kind {
            ItemKind::Fn(d) => Some(d.name),
            ItemKind::Struct(s) => Some(s.name),
            ItemKind::Enum(e) => Some(e.name),
            _ => None,
        };
        let Some(item_name) = item_name else {
            return Err(format!(
                "lust std load failed: library/{}: #[lust_lang = \"{name}\"] marks an item kind that cannot be a lang item yet",
                file.rel
            ));
        };
        let ids = CrateIds { local: LOCAL_CRATE, stable: file.krate.stable_hash() };
        let known = std::sync::Arc::new(Known::new(&file.interner));
        let program = collect::collect(file.ast.clone(), file.interner.clone(), known, ids)
            .map_err(|ds| format!("lust std load failed: {}", ds.iter().map(|d| diag_line(file.rel, d)).collect::<Vec<_>>().join("; ")))?;
        let def = match &found.kind {
            ItemKind::Fn(_) => program.fn_names.get(&item_name).map(|&f| program.def_id(f)),
            _ => program.type_names.get(&item_name).map(|&a| program.def_of_adt(a)),
        };
        let Some(def) = def else {
            return Err(format!("lust std load failed: library/{}: lang item `{name}` def not found after collect", file.rel));
        };
        items.set(item, def);
    }
    let missing = items.required_missing();
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().map(|i| lang::LANG_ITEMS[*i as usize].name).collect();
        return Err(format!("lust std load failed: required lang item(s) not resolved: {}", names.join(", ")));
    }
    Ok(items)
}

/// True for a shipped file of std methods on a lang type (`impl<T>
/// Option<T>` in core): it splices whole (`StdLoad::methods`).
fn is_lang_method_file(krate: ShippedCrate, module: &[&str]) -> bool {
    krate == ShippedCrate::Core && matches!(module, ["option"] | ["result"])
}

/// The std methods of the lang enums in the built-in source, by type
/// (`Option<T>`, `Result<T, E>`), for `lust capabilities`.
pub fn lang_method_names() -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for (rel, ty) in [("core/src/option.rs", "Option<T>"), ("core/src/result.rs", "Result<T, E>")] {
        for line in builtin_text(rel).unwrap_or_default().lines() {
            if let Some(rest) = line.strip_prefix("    pub fn ").or_else(|| line.strip_prefix("    pub const fn ")) {
                let end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
                out.push((ty, &rest[..end]));
            }
        }
    }
    out
}

/// Builds splice text for every implemented `STD_FNS` row: its file
/// with the row's fn renamed to the row's mangled name (by its
/// recorded name span, so no other text moves). A spliced file that
/// defines anything the registry does not name is rejected (a stray
/// name could collide with user code).
fn build_splices(files: &[CheckedFile]) -> Result<(Vec<(ShippedCrate, String)>, Vec<&'static StdFnDef>), String> {
    let mut splices: Vec<(ShippedCrate, String)> = Vec::new();
    let mut spliced_files: Vec<&str> = Vec::new();
    let mut routed: Vec<&'static StdFnDef> = Vec::new();
    for def in lang::STD_FNS.iter().filter(|d| d.implemented) {
        let module = &def.path[1..def.path.len() - 1];
        let fname = def.path[def.path.len() - 1];
        let Some(file) = files.iter().find(|f| f.krate == def.krate && f.module == module) else {
            return Err(format!("lust std load failed: registry std fn `{}` has no shipped source file", def.path.join("::")));
        };
        // Every item in a spliced file must be a registry fn of this file.
        let mut renames: Vec<(u32, u32, &'static str)> = Vec::new();
        for it in &file.ast.items {
            let ItemKind::Fn(fd) = &it.kind else {
                return Err(format!(
                    "lust std load failed: library/{}: only registry std fns can ride in a spliced file in S5 (found a non-fn item)",
                    file.rel
                ));
            };
            let n = file.interner.get(fd.name);
            let row = lang::STD_FNS.iter().find(|d| {
                d.implemented && d.krate == file.krate && &d.path[1..d.path.len() - 1] == file.module && d.path[d.path.len() - 1] == n
            });
            let Some(row) = row else {
                return Err(format!("lust std load failed: library/{}: fn `{n}` is not in the STD_FNS registry", file.rel));
            };
            renames.push((fd.name_span.lo, fd.name_span.hi, row.mangled));
        }
        if !renames.iter().any(|(_, _, m)| *m == def.mangled) {
            return Err(format!(
                "lust std load failed: library/{}: registry std fn `{}` (`{fname}`) not found in source",
                file.rel,
                def.path.join("::")
            ));
        }
        routed.push(def);
        if spliced_files.contains(&file.rel) {
            continue; // This file's splice text is already built (same file, several fns).
        }
        spliced_files.push(file.rel);
        let mut text = file.clean.clone();
        renames.sort_by_key(|(lo, _, _)| std::cmp::Reverse(*lo));
        for (lo, hi, mangled) in renames {
            text.replace_range(lo as usize..hi as usize, mangled);
        }
        splices.push((file.krate, text));
    }
    Ok((splices, routed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader over the real built-in library text, hermetic to env.
    fn builtin_reader() -> impl FnMut(&str) -> Result<String, String> {
        |rel| builtin_text(rel).map(str::to_string).ok_or_else(|| format!("no builtin {rel}"))
    }

    /// The shipped library loads: required lang items resolve, the exit
    /// fn is checked and renamed into the std splice, and the
    /// Option/Result method files splice whole.
    #[test]
    fn shipped_library_loads() {
        let load = load_std_from(&mut builtin_reader(), true).expect("shipped library must load");
        let rest = load.rest().unwrap();
        assert!(rest.lang.required_missing().is_empty());
        let want = lang::STD_FNS.iter().filter(|d| d.implemented).count();
        assert_eq!(rest.fns.len(), want, "every implemented registry row is routed");
        assert!(rest.fns.iter().any(|d| d.path == ["std", "process", "exit"]));
        let sp = load.splice(lang::std_needs("fn main() { std::process::exit(0); }")).unwrap();
        assert_eq!(sp.routes.len(), want);
        assert!(sp.methods_block.is_none(), "no method chunks when no method is named");
        assert_eq!(sp.blocks.len(), 1, "only the process fns when no std module is named");
        let (krate, text) = &sp.blocks[0];
        assert_eq!(*krate, ShippedCrate::Std);
        assert!(text.contains("fn __lust_std_process_exit(code: i32)"), "splice carries the mangled name:\n{text}");
        assert!(!text.contains("fn exit("), "the unmangled name must not ride in the splice");
        assert!(text.contains("core::intrinsics::exit(code)"), "the body still names the intrinsic");
    }

    /// A compile that names a lang method splices both method files
    /// whole, first; one that names none splices neither.
    #[test]
    fn splice_takes_lang_methods() {
        let load = load_std_from(&mut builtin_reader(), true).expect("shipped library must load");
        let sp = load.splice(lang::std_needs("fn f(x: Option<u8>) -> bool { x.is_none() }")).unwrap();
        assert!(sp.routes.is_empty());
        assert_eq!((sp.blocks.len(), sp.methods_block), (1, Some(0)));
        let text = &sp.blocks[0].1;
        assert!(text.contains("impl<T> Option<T> {") && text.contains("impl<T, E> Result<T, E> {"));
        assert!(load.splice(lang::std_needs("fn main() {}")).unwrap().blocks.is_empty());
    }

    /// `STD_LANG_METHODS` (the load trigger) names exactly the methods
    /// the shipped Option and Result files define.
    #[test]
    fn lang_methods_match_source() {
        let mut names: Vec<String> = Vec::new();
        for rel in ["core/src/option.rs", "core/src/result.rs"] {
            let file = SOURCE_FILES.iter().find(|f| f.rel == rel).unwrap();
            let (checked, _) = check_file(file, builtin_text(rel).unwrap(), true).expect("method file checks");
            for it in &checked.ast.items {
                if let ItemKind::Impl(imp) = &it.kind {
                    names.extend(imp.fns.iter().map(|f| checked.interner.get(f.name).to_string()));
                }
            }
        }
        names.sort();
        names.dedup();
        assert_eq!(names, lang::STD_LANG_METHODS, "STD_LANG_METHODS must list the shipped methods, sorted");
    }

    /// A `#[lust_lang]` mark in shipped source resolves to the next
    /// item's def, and blanking the mark line keeps byte offsets exact.
    #[test]
    fn lang_mark_resolves() {
        let src = "#[lust_lang = \"phantom_data\"]\npub struct PhantomData;\n";
        let (clean, marks) = scan_lang_attrs(src, "core/src/lib.rs").unwrap();
        assert_eq!(clean.len(), src.len());
        assert_eq!(marks, vec![("phantom_data".to_string(), 30)]);
        let mut read = |rel: &str| -> Result<String, String> {
            if rel == "core/src/lib.rs" {
                Ok(src.to_string())
            } else {
                builtin_text(rel).map(str::to_string).ok_or_else(|| format!("no builtin {rel}"))
            }
        };
        let load = load_std_from(&mut read, true).expect("fixture library must load");
        assert!(load.rest().unwrap().lang.get(lang::LangItem::PhantomData).is_some(), "mark resolves to a def");
    }

    /// A plan closes over what its own splice asks, and closing twice
    /// changes nothing (the driver compares closed plans).
    #[test]
    fn std_plan_closes() {
        let load = load_std_from(&mut builtin_reader(), false).unwrap();
        let none = StdPlan::default();
        assert_eq!(load.close(none, None).unwrap(), none);
        let modules = StdPlan { needs: lang::StdNeeds { modules: 1, ..Default::default() }, platform: false };
        let plan = load.close(modules, None).unwrap();
        assert!(plan.needs.modules & 1 != 0 && plan.needs.methods, "core::time's bodies call lang methods");
        assert_eq!(load.close(plan, None).unwrap(), plan);
        // The platform files' asks join only a plan that takes the platform crate.
        let methods = StdPlan { needs: lang::StdNeeds { methods: true, ..Default::default() }, platform: false };
        assert_eq!(load.close(none, Some(methods)).unwrap(), none);
        let platform = StdPlan { platform: true, ..none };
        assert!(load.close(platform, Some(methods)).unwrap().needs.methods);
    }

    /// The first-guess scan follows `mod name;`, `#[path]`, inline
    /// modules, and `mod.rs` directories, and skips comments and strings.
    #[test]
    fn guess_follows_module_files() {
        let dir = std::env::temp_dir().join(format!("lust-stdguess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["a", "c", "inner/deep"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let root = "mod a;\n#[path = \"other.rs\"]\npub mod b;\nmod c;\nmod inner { mod deep; }\n// mod gone;\nconst S: &str = \"mod gone;\";\nfn main() {}\n";
        let files = [
            ("a.rs", "mod sub;\n"),
            ("a/sub.rs", "pub fn f(x: Option<u8>) -> u8 { x.unwrap_or(0) }\n"),
            ("other.rs", "pub fn g() {}\n"),
            ("c/mod.rs", "mod leaf;\n"),
            ("c/leaf.rs", "use std::io::Write;\n"),
            ("inner/deep.rs", "fn h() { std::process::exit(0); }\n"),
            ("gone.rs", "use std::time::Duration;\n"),
        ];
        for (f, t) in files {
            std::fs::write(dir.join(f), t).unwrap();
        }
        let path = dir.join("main.rs");
        let plan = StdPlan::guess_crate(&path, root, StdPlan::asks(root));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(plan.needs.methods, "a/sub.rs (through a.rs) is reached");
        assert!(plan.platform, "c/leaf.rs (through c/mod.rs) is reached");
        assert!(plan.needs.fns, "inner/deep.rs (an inline module's child) is reached");
        assert_eq!(plan.needs.modules, 0, "a commented or quoted `mod gone;` is skipped");
    }

    /// A malformed mark is an installation error, never a silent skip.
    #[test]
    fn bad_lang_mark_fails() {
        let err = scan_lang_attrs("#[lust_lang = \"Nope\"]\nfn f() {}\n", "core/src/lib.rs").unwrap_err();
        assert!(err.contains("bad #[lust_lang]"), "{err}");
    }

    /// Privilege is per registry row, in shipped source too: `std` may
    /// not call the bumpalo-only `arena_alloc` intrinsic.
    #[test]
    fn shipped_source_privilege_is_per_row() {
        let process = "pub fn exit(code: i32) -> ! {\n    core::intrinsics::arena_alloc(0, 1, 1);\n    core::intrinsics::exit(code)\n}\n";
        let mut read = |rel: &str| -> Result<String, String> {
            if rel == "std/src/process.rs" {
                Ok(process.to_string())
            } else {
                builtin_text(rel).map(str::to_string).ok_or_else(|| format!("no builtin {rel}"))
            }
        };
        let err = load_std_from(&mut read, true).map(|_| ()).unwrap_err();
        assert!(err.contains("L0642"), "want a privilege error, got: {err}");
    }

    /// A registry fn with no source body fails the load (the registry
    /// and the library must agree).
    #[test]
    fn missing_registry_fn_fails() {
        let mut read = |rel: &str| -> Result<String, String> {
            if rel == "std/src/process.rs" {
                Ok("// empty\n".to_string())
            } else {
                builtin_text(rel).map(str::to_string).ok_or_else(|| format!("no builtin {rel}"))
            }
        };
        let err = load_std_from(&mut read, true).map(|_| ()).unwrap_err();
        assert!(err.contains("std::process::exit"), "error names the missing fn: {err}");
    }
}
