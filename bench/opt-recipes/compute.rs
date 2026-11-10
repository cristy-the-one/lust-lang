// Compute bench for `--opt` (docs/opt.md).
//
// Everything from the inner docs through the crate tests is lime-recipe's
// `src/lib.rs` from cristy-the-one/lime-slice at main, fetched 2026-11-09
// (the crate the plan inlines; milestone 2 has not committed the original
// harness). The functions from `fn sc` to the end are this repo's harness:
// nine involute outlines through that crate's own `sin_cos_deg` (the owner's
// gear ingredient is not in lime-slice), and the plan's 2D shape (80 hexes
// clipped to a disc, unioned, cut out, inset). `scripts/opt-bench.sh` builds
// it with `--sandbox` and with `--sandbox --opt` and compares the checksums.
//! The recipe side of Lime Slice's recipe protocol, version 1.
//!
//! A recipe is a program `recipe_run` builds with `lust build --sandbox` and
//! runs. The host (the MCP server) writes one start line on stdin; the recipe
//! writes one call per line on stdout and reads one reply per line; it ends
//! with a `done` line. Every call goes to the function the matching MCP tool
//! uses, so a recipe builds what the window builds.
//!
//! No dependencies, and nothing lust does not accept: this crate is built by
//! lust for the run and by rustc in CI, and must send the same bytes under both.

use std::fmt;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};

static NATIVE_GEOMETRY: AtomicBool = AtomicBool::new(false);

// Geometry can be used without a Session. Only a host which advertises the protocol enables this path.
fn native_geometry(args: Json) -> Option<Json> {
    if !NATIVE_GEOMETRY.load(Ordering::Relaxed) { return None; }
    // The host limits protocol lines to 1 MiB; large inputs stay in the local implementation.
    if args.to_string().len() > 512 * 1024 { return None; }
    let msg = Json::obj().set("id", 0u32).set("call", "native_geometry").set("args", args);
    tell(msg);
    let reply = Json::parse(&read_line()).ok()?;
    if reply.get("ok").and_then(Json::as_bool) == Some(true) { reply.get("result").cloned() } else { None }
}


/// A JSON value. Objects keep their keys in insertion order.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// An empty object, to fill with `set`.
    pub fn obj() -> Json {
        Json::Obj(Vec::new())
    }

    /// An array of anything that converts: `Json::arr(vec![1.0, 2.0])`, `Json::arr(points)`.
    pub fn arr<T: Into<Json>>(items: Vec<T>) -> Json {
        Json::Arr(items.into_iter().map(|v| v.into()).collect())
    }

    /// This array with `value` added at the end (an empty array when it was not one).
    pub fn push(self, value: impl Into<Json>) -> Json {
        let mut items = match self {
            Json::Arr(items) => items,
            _ => Vec::new(),
        };
        items.push(value.into());
        Json::Arr(items)
    }

    /// This object with `key` set to `value` (replacing an earlier value).
    pub fn set(self, key: &str, value: impl Into<Json>) -> Json {
        let mut fields = match self {
            Json::Obj(fields) => fields,
            _ => Vec::new(),
        };
        let value = value.into();
        match fields.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => fields.push((key.to_string(), value)),
        }
        Json::Obj(fields)
    }

    /// The value at `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value at a dotted path: `"metrics.volumeMm3"`, `"faces.0.index"`.
    pub fn at(&self, path: &str) -> Option<&Json> {
        let mut node = self;
        for part in path.split('.') {
            node = match node {
                Json::Arr(items) => items.get(part.parse::<usize>().ok()?)?,
                _ => node.get(part)?,
            };
        }
        Some(node)
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// An array of numbers, or of one-number rows (a one-column CSV), as `f64`s.
    pub fn as_f64s(&self) -> Option<Vec<f64>> {
        let mut out = Vec::new();
        for item in self.as_array()?.iter() {
            let v = match item {
                Json::Num(n) => *n,
                Json::Arr(row) if row.len() == 1 => row[0].as_f64()?,
                _ => return None,
            };
            out.push(v);
        }
        Some(out)
    }

    /// An array of `[x, y]` rows (more columns are ignored), or of `{x, y}` objects, as points.
    pub fn as_points2(&self) -> Option<Vec<[f64; 2]>> {
        let mut out = Vec::new();
        for item in self.as_array()?.iter() {
            let p = match item {
                Json::Arr(row) if row.len() >= 2 => [row[0].as_f64()?, row[1].as_f64()?],
                Json::Obj(_) => [item.get("x")?.as_f64()?, item.get("y")?.as_f64()?],
                _ => return None,
            };
            out.push(p);
        }
        Some(out)
    }

    /// Parses one JSON text.
    pub fn parse(text: &str) -> Result<Json, String> {
        let chars: Vec<char> = text.chars().collect();
        let mut at = 0usize;
        let value = parse_value(&chars, &mut at)?;
        skip_ws(&chars, &mut at);
        if at != chars.len() {
            return Err(format!("unexpected text at {at}"));
        }
        Ok(value)
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            // `{}` of an f64 is the shortest text that reads back the same
            // number, under rustc and lust alike. NaN and infinity are not JSON.
            Json::Num(n) => {
                if n.is_finite() {
                    write!(f, "{n}")
                } else {
                    f.write_str("null")
                }
            }
            Json::Str(s) => write_str(f, s),
            Json::Arr(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Json::Obj(fields) => {
                f.write_str("{")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write_str(f, k)?;
                    write!(f, ":{v}")?;
                }
                f.write_str("}")
            }
        }
    }
}

fn write_str(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("\"")
}

fn skip_ws(chars: &[char], at: &mut usize) {
    while *at < chars.len() && chars[*at].is_whitespace() {
        *at += 1;
    }
}

fn expect_word(chars: &[char], at: &mut usize, word: &str, value: Json) -> Result<Json, String> {
    for w in word.chars() {
        if chars.get(*at) != Some(&w) {
            return Err(format!("expected `{word}` at {at}"));
        }
        *at += 1;
    }
    Ok(value)
}

fn parse_value(chars: &[char], at: &mut usize) -> Result<Json, String> {
    skip_ws(chars, at);
    match chars.get(*at) {
        None => Err("unexpected end".to_string()),
        Some('n') => expect_word(chars, at, "null", Json::Null),
        Some('t') => expect_word(chars, at, "true", Json::Bool(true)),
        Some('f') => expect_word(chars, at, "false", Json::Bool(false)),
        Some('"') => parse_string(chars, at).map(Json::Str),
        Some('[') => {
            *at += 1;
            let mut items = Vec::new();
            skip_ws(chars, at);
            if chars.get(*at) == Some(&']') {
                *at += 1;
                return Ok(Json::Arr(items));
            }
            loop {
                items.push(parse_value(chars, at)?);
                skip_ws(chars, at);
                match chars.get(*at) {
                    Some(',') => *at += 1,
                    Some(']') => {
                        *at += 1;
                        return Ok(Json::Arr(items));
                    }
                    _ => return Err(format!("expected `,` or `]` at {at}")),
                }
            }
        }
        Some('{') => {
            *at += 1;
            let mut fields = Vec::new();
            skip_ws(chars, at);
            if chars.get(*at) == Some(&'}') {
                *at += 1;
                return Ok(Json::Obj(fields));
            }
            loop {
                skip_ws(chars, at);
                if chars.get(*at) != Some(&'"') {
                    return Err(format!("expected a key at {at}"));
                }
                let key = parse_string(chars, at)?;
                skip_ws(chars, at);
                if chars.get(*at) != Some(&':') {
                    return Err(format!("expected `:` at {at}"));
                }
                *at += 1;
                let value = parse_value(chars, at)?;
                fields.push((key, value));
                skip_ws(chars, at);
                match chars.get(*at) {
                    Some(',') => *at += 1,
                    Some('}') => {
                        *at += 1;
                        return Ok(Json::Obj(fields));
                    }
                    _ => return Err(format!("expected `,` or `}}` at {at}")),
                }
            }
        }
        Some(_) => {
            let start = *at;
            while *at < chars.len() && (chars[*at].is_ascii_digit() || "+-.eE".contains(chars[*at])) {
                *at += 1;
            }
            let text: String = chars[start..*at].iter().collect();
            text.parse::<f64>().map(Json::Num).map_err(|_| format!("bad number `{text}` at {start}"))
        }
    }
}

fn parse_string(chars: &[char], at: &mut usize) -> Result<String, String> {
    *at += 1;
    let mut out = String::new();
    while let Some(&c) = chars.get(*at) {
        *at += 1;
        match c {
            '"' => return Ok(out),
            '\\' => {
                let e = *chars.get(*at).ok_or("unterminated escape")?;
                *at += 1;
                match e {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let hex: String = chars.get(*at..*at + 4).ok_or("short \\u escape")?.iter().collect();
                        *at += 4;
                        let code = u32::from_str_radix(&hex, 16).map_err(|_| format!("bad \\u escape `{hex}`"))?;
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
    }
    Err("unterminated string".to_string())
}

impl From<f64> for Json {
    fn from(v: f64) -> Json {
        Json::Num(v)
    }
}
impl From<f32> for Json {
    fn from(v: f32) -> Json {
        Json::Num(v as f64)
    }
}
impl From<i32> for Json {
    fn from(v: i32) -> Json {
        Json::Num(v as f64)
    }
}
impl From<i64> for Json {
    fn from(v: i64) -> Json {
        Json::Num(v as f64)
    }
}
impl From<u32> for Json {
    fn from(v: u32) -> Json {
        Json::Num(v as f64)
    }
}
impl From<u64> for Json {
    fn from(v: u64) -> Json {
        Json::Num(v as f64)
    }
}
impl From<usize> for Json {
    fn from(v: usize) -> Json {
        Json::Num(v as f64)
    }
}
impl From<bool> for Json {
    fn from(v: bool) -> Json {
        Json::Bool(v)
    }
}
impl From<&str> for Json {
    fn from(v: &str) -> Json {
        Json::Str(v.to_string())
    }
}
impl From<String> for Json {
    fn from(v: String) -> Json {
        Json::Str(v)
    }
}
impl From<[f64; 2]> for Json {
    fn from(v: [f64; 2]) -> Json {
        Json::Arr(vec![Json::Num(v[0]), Json::Num(v[1])])
    }
}
impl From<[f64; 3]> for Json {
    fn from(v: [f64; 3]) -> Json {
        Json::Arr(vec![Json::Num(v[0]), Json::Num(v[1]), Json::Num(v[2])])
    }
}
impl From<Vec<Json>> for Json {
    fn from(v: Vec<Json>) -> Json {
        Json::Arr(v)
    }
}
impl From<Vec<f64>> for Json {
    fn from(v: Vec<f64>) -> Json {
        Json::Arr(v.into_iter().map(Json::Num).collect())
    }
}
/// An outline: `vec![[x, y], ...]` goes straight into an op's `points`.
impl From<Vec<[f64; 2]>> for Json {
    fn from(v: Vec<[f64; 2]>) -> Json {
        Json::Arr(v.into_iter().map(Json::from).collect())
    }
}
impl From<Vec<[f64; 3]>> for Json {
    fn from(v: Vec<[f64; 3]>) -> Json {
        Json::Arr(v.into_iter().map(Json::from).collect())
    }
}

/// A call the host refused: the engine's sentence. `line` is where the
/// recipe made the call.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub reason: String,
    pub line: u32,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

/// A solid the host holds, by its object handle.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub object: String,
}

/// This crate's recipe API level, `(major, minor)` (docs/plan-recipe-plugins.md D12). A plugin records it when it
/// is saved. A minor level only adds; a plugin of a higher level than the host's crate is not built.
/// 1.0: the calls, `param_*`, `geom2d`, `lattice`, and the `section` and `apply_many` wrappers. 1.1: typed
/// parameters (`number`, `count`, `toggle`, `choice`, `text`) and describe mode. 1.2: `face` and `outline` parameters
/// a person picks (`pick_face`, `pick_outline`), `refuse`, and `bounds`. 1.3: `face(..).curved()` parameters and what a
/// picked face is (`kind`, `axis`, `radius`), `lettering` (text on a face, flat or curved) and `text_outline` (the letters
/// as a `Section`). 1.4: a picked sphere's `centre` and a round face's `axis_point`; reads answer in the frame `apply`
/// takes (model space for a moved or turned part), as they always should have. 1.5: progress and host context.
/// 1.6: batched Region unions and differences. 1.7: resumable checkpoints. 1.8: pose animations.
/// 1.9: `Animation::background`.
pub const API: (u32, u32) = (1, 9);

/// One recipe run: the start line's parameters and context, and the call counter.
pub struct Session {
    params: Json,
    context: Json,
    next_id: u64,
    /// Describe mode: the host asked only for the parameters (D4). No call reaches the engine.
    describe: bool,
}

fn read_line() -> String {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap_or(0);
    line.trim_end().to_string()
}

/// A line to the host that is not a call: it takes no reply.
fn tell(msg: Json) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{msg}").unwrap();
    out.flush().unwrap();
}

/// A declared parameter: what the host's form and `plugin list` show.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSpec {
    pub name: String,
    pub kind: String,
    pub default: Json,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: Option<String>,
    pub label: Option<String>,
    pub help: Option<String>,
    pub options: Vec<String>,
    pub max_len: Option<u32>,
    /// A face parameter that takes a curved face too (API 1.3).
    pub curved: bool,
}

impl ParamSpec {
    fn new(name: &str, kind: &str, default: Json) -> ParamSpec {
        ParamSpec { name: name.to_string(), kind: kind.to_string(), default, min: None, max: None, step: None, unit: None, label: None, help: None, options: Vec::new(), max_len: None, curved: false }
    }

    /// The JSON the host keeps in a plugin's header: only the settings that were given.
    pub fn to_json(&self) -> Json {
        let mut j = Json::obj().set("name", self.name.as_str()).set("kind", self.kind.as_str()).set("default", self.default.clone());
        if let Some(v) = self.min {
            j = j.set("min", v);
        }
        if let Some(v) = self.max {
            j = j.set("max", v);
        }
        if let Some(v) = self.step {
            j = j.set("step", v);
        }
        if let Some(v) = &self.unit {
            j = j.set("unit", v.as_str());
        }
        if let Some(v) = &self.label {
            j = j.set("label", v.as_str());
        }
        if let Some(v) = &self.help {
            j = j.set("help", v.as_str());
        }
        if !self.options.is_empty() {
            j = j.set("options", Json::Arr(self.options.iter().map(|o| Json::from(o.as_str())).collect()));
        }
        if let Some(v) = self.max_len {
            j = j.set("maxLen", v);
        }
        if self.curved {
            j = j.set("curved", true);
        }
        j
    }
}

/// A parameter builder's view of the run, taken when the builder is made.
struct Reader {
    given: Option<Json>,
    describe: bool,
    late: bool,
}

impl Reader {
    /// Tells the host about a parameter as it is read, and gives the value the host sent for it. In describe mode
    /// that is the declaration and nothing is sent back; after the first call it is a late declaration, which the
    /// host notes, since a form drawn before the run could not show it.
    fn declare(&self, spec: &ParamSpec, line: u32) -> Option<Json> {
        if self.describe {
            tell(Json::obj().set("describe", Json::obj().set("param", spec.to_json()).set("line", line)));
            return None;
        }
        if self.late {
            tell(Json::obj().set("late_param", Json::obj().set("name", spec.name.as_str()).set("line", line)));
        }
        self.given.clone()
    }

    /// A line for the run's notes that is not a call.
    fn note(&self, text: &str) {
        if !self.describe {
            tell(Json::obj().set("note", text));
        }
    }

    fn note_held(&self, name: &str, given: f64, held: f64) {
        let side = if given < held { "below its minimum" } else { "above its maximum" };
        self.note(&format!("{name} {given} is {side}; the run used {held}."));
    }
}

/// `s.number(name, default)`: a length, angle or ratio. Then `range`, `step`, `unit`, `label`, `help`, and `get()`.
pub struct Number {
    r: Reader,
    spec: ParamSpec,
}

/// `s.count(name, default)`: a whole number. Then `range`, `label`, `help`, and `get()`.
pub struct Count {
    r: Reader,
    spec: ParamSpec,
}

/// `s.toggle(name, default)`: on or off. Then `label`, `help`, and `get()`.
pub struct Toggle {
    r: Reader,
    spec: ParamSpec,
}

/// `s.choice(name, options, default)`: one of a few words. Then `label`, `help`, and `get()`.
pub struct Choice {
    r: Reader,
    spec: ParamSpec,
}

/// `s.text(name, default)`: a line of text (embossed lettering, say). Then `max_len`, `label`, `help`, and `get()`.
pub struct Text {
    r: Reader,
    spec: ParamSpec,
}

impl Number {
    pub fn range(mut self, min: f64, max: f64) -> Number {
        self.spec.min = Some(min);
        self.spec.max = Some(max);
        self
    }
    pub fn step(mut self, step: f64) -> Number {
        self.spec.step = Some(step);
        self
    }
    /// `mm`, `deg`, `%`, or none.
    pub fn unit(mut self, unit: &str) -> Number {
        self.spec.unit = Some(unit.to_string());
        self
    }
    pub fn label(mut self, label: &str) -> Number {
        self.spec.label = Some(label.to_string());
        self
    }
    pub fn help(mut self, help: &str) -> Number {
        self.spec.help = Some(help.to_string());
        self
    }
    /// The value the host passed, held inside the range, or the default.
    #[track_caller]
    pub fn get(self) -> f64 {
        let line = std::panic::Location::caller().line();
        let default = self.spec.default.as_f64().unwrap_or(0.0);
        let given = self.r.declare(&self.spec, line).and_then(|j| j.as_f64());
        let v = given.unwrap_or(default);
        let held = hold(v, self.spec.min, self.spec.max);
        if held != v {
            self.r.note_held(&self.spec.name, v, held);
        }
        held
    }
}

impl Count {
    pub fn range(mut self, min: u32, max: u32) -> Count {
        self.spec.min = Some(min as f64);
        self.spec.max = Some(max as f64);
        self
    }
    pub fn label(mut self, label: &str) -> Count {
        self.spec.label = Some(label.to_string());
        self
    }
    pub fn help(mut self, help: &str) -> Count {
        self.spec.help = Some(help.to_string());
        self
    }
    /// The value the host passed (rounded down), held inside the range, or the default.
    #[track_caller]
    pub fn get(self) -> u32 {
        let line = std::panic::Location::caller().line();
        let default = self.spec.default.as_f64().unwrap_or(0.0);
        let v = self.r.declare(&self.spec, line).and_then(|j| j.as_f64()).map_or(default, |n| n.floor());
        let held = hold(v.max(0.0), self.spec.min, self.spec.max);
        if held != v {
            self.r.note_held(&self.spec.name, v, held);
        }
        held as u32
    }
}

impl Toggle {
    pub fn label(mut self, label: &str) -> Toggle {
        self.spec.label = Some(label.to_string());
        self
    }
    pub fn help(mut self, help: &str) -> Toggle {
        self.spec.help = Some(help.to_string());
        self
    }
    #[track_caller]
    pub fn get(self) -> bool {
        let line = std::panic::Location::caller().line();
        let default = self.spec.default.as_bool().unwrap_or(false);
        self.r.declare(&self.spec, line).and_then(|j| j.as_bool()).unwrap_or(default)
    }
}

impl Choice {
    pub fn label(mut self, label: &str) -> Choice {
        self.spec.label = Some(label.to_string());
        self
    }
    pub fn help(mut self, help: &str) -> Choice {
        self.spec.help = Some(help.to_string());
        self
    }
    /// The option the host passed, or the default when it passed none or one not in the list.
    #[track_caller]
    pub fn get(self) -> String {
        let line = std::panic::Location::caller().line();
        let default = self.spec.default.as_str().unwrap_or("").to_string();
        match self.r.declare(&self.spec, line).and_then(|j| j.as_str().map(|t| t.to_string())) {
            Some(v) if self.spec.options.contains(&v) => v,
            Some(v) => {
                self.r.note(&format!("{} has no option {}; the run used {}.", self.spec.name, v, default));
                default
            }
            None => default,
        }
    }
}

impl Text {
    pub fn max_len(mut self, n: u32) -> Text {
        self.spec.max_len = Some(n);
        self
    }
    pub fn label(mut self, label: &str) -> Text {
        self.spec.label = Some(label.to_string());
        self
    }
    pub fn help(mut self, help: &str) -> Text {
        self.spec.help = Some(help.to_string());
        self
    }
    /// The text the host passed, cut to `max_len` characters, or the default.
    #[track_caller]
    pub fn get(self) -> String {
        let line = std::panic::Location::caller().line();
        let default = self.spec.default.as_str().unwrap_or("").to_string();
        let v = self.r.declare(&self.spec, line).and_then(|j| j.as_str().map(|t| t.to_string())).unwrap_or(default);
        match self.spec.max_len {
            Some(n) if v.chars().count() > n as usize => {
                self.r.note(&format!("{} is longer than {} characters; the run used the first {}.", self.spec.name, n, n));
                v.chars().take(n as usize).collect()
            }
            _ => v,
        }
    }
}

/// `s.face(name)`: a flat face of the part, picked by the person (API 1.2). Then `rule` or `facing`, `label`, `help`,
/// and `get()`; `s.pick_face(&part, &value)` resolves it on the part as it stands.
pub struct FaceBuilder {
    r: Reader,
    spec: ParamSpec,
}

/// `s.outline(name)`: a closed region the person supplies: a sketch, a face's outline, or loops (API 1.2). Then a
/// default (`default_poly`, `default_loops`, `default_face`), `label`, `help`, and `get()`; `s.pick_outline(&value)`
/// delivers it as a `Section`.
pub struct OutlineBuilder {
    r: Reader,
    spec: ParamSpec,
}

/// A face or outline parameter's value as the run was given it (or its default): what the host resolves at a pick.
#[derive(Clone, Debug, PartialEq)]
pub struct PickParam {
    pub name: String,
    pub kind: String,
    pub value: Json,
    /// A face parameter declared `.curved()` (API 1.3).
    pub curved: bool,
}

/// A face the host resolved: its index on the part now, its outward normal, a point on it, its area, and its outline in
/// its own frame (`outline.frame`, `outline.region`, `outline.loops`), as `face_outline` gives it.
///
/// API 1.3: `kind` is `plane`, `cylinder`, `cone`, `sphere` or `other` (a torus or a freeform face; the part's face list does
/// not tell those apart), and for a cylinder or a cone `axis` and `radius`. A curved face has no one normal (`normal` is
/// zero) and no outline (`outline` is empty); `point` is a point on it, which `lettering` anchors the text at.
///
/// API 1.4: a sphere's `centre`, and a cylinder's or a cone's `axis_point` (a point on its axis; a sphere's is its centre).
/// Every point and direction here, like every read (`bounds`, `faces_find`, `surface_at`), is in the frame `apply` takes
/// its numbers in: model space, where a part `model_place` moved or turned stands. Measure, then cut at that point.
#[derive(Clone, Debug, PartialEq)]
pub struct PickedFace {
    pub index: u32,
    pub normal: [f64; 3],
    pub point: [f64; 3],
    pub area: f64,
    pub outline: Section,
    pub kind: String,
    pub axis: Option<[f64; 3]>,
    pub radius: Option<f64>,
    /// A sphere's centre (API 1.4).
    pub centre: Option<[f64; 3]>,
    /// A point on a cylinder's or a cone's axis, or a sphere's centre (API 1.4).
    pub axis_point: Option<[f64; 3]>,
}

/// The rules a face parameter may fall back on when the person picked none.
pub const FACE_RULES: [&str; 5] = ["largest", "largest_up", "largest_down", "top", "bottom"];

impl FaceBuilder {
    /// The face taken when the person picked none: `largest`, `largest_up`, `largest_down`, `top` or `bottom`.
    pub fn rule(mut self, rule: &str) -> FaceBuilder {
        let facing = self.spec.default.get("facing").cloned();
        let mut d = Json::obj().set("rule", rule);
        if let Some(f) = facing {
            d = d.set("facing", f);
        }
        self.spec.default = d;
        self
    }
    /// With `largest`: the largest flat face facing `dir` (within 5°). `rule("largest").facing([0,0,1])` is `largest_up`.
    pub fn facing(mut self, dir: [f64; 3]) -> FaceBuilder {
        let rule = self.spec.default.get("rule").and_then(Json::as_str).unwrap_or("largest").to_string();
        self.spec.default = Json::obj().set("rule", rule.as_str()).set("facing", dir);
        self
    }
    pub fn label(mut self, label: &str) -> FaceBuilder {
        self.spec.label = Some(label.to_string());
        self
    }
    pub fn help(mut self, help: &str) -> FaceBuilder {
        self.spec.help = Some(help.to_string());
        self
    }
    /// The parameter takes a curved face too (API 1.3): a cylinder, a cone, a sphere, a torus or a freeform face. `pick_face`
    /// then says which (`kind`) and, for a cylinder or a cone, its `axis` and `radius`. Without it only a flat face is taken.
    pub fn curved(mut self) -> FaceBuilder {
        self.spec.curved = true;
        self
    }
    /// The face the host was given (an index, or `{normal, nearPoint}`), else the default rule. Nothing is resolved
    /// yet: `s.pick_face` does that.
    #[track_caller]
    pub fn get(self) -> PickParam {
        let line = std::panic::Location::caller().line();
        let given = self.r.declare(&self.spec, line);
        let value = match given {
            Some(Json::Null) | None => self.spec.default.clone(),
            Some(v) => v,
        };
        PickParam { name: self.spec.name.clone(), kind: "face".to_string(), value, curved: self.spec.curved }
    }
}

impl OutlineBuilder {
    /// A default outline: this polygon, drawn on the part's workplane (or the bed for a new part).
    pub fn default_poly(mut self, poly: &geom2d::Poly) -> OutlineBuilder {
        self.spec.default = Json::obj().set("loops", Json::Arr(vec![poly.to_loop()]));
        self
    }
    /// A default outline: `extrude` loops (`Profile::to_loops()`, `Poly::to_loop()`), an outline then its holes.
    pub fn default_loops(mut self, loops: Vec<Json>) -> OutlineBuilder {
        self.spec.default = Json::obj().set("loops", Json::Arr(loops));
        self
    }
    /// A default outline: the outline of the part's face the rule names (`largest_up`, say).
    pub fn default_face(mut self, rule: &str) -> OutlineBuilder {
        self.spec.default = Json::obj().set("face", Json::obj().set("rule", rule));
        self
    }
    pub fn label(mut self, label: &str) -> OutlineBuilder {
        self.spec.label = Some(label.to_string());
        self
    }
    pub fn help(mut self, help: &str) -> OutlineBuilder {
        self.spec.help = Some(help.to_string());
        self
    }
    /// The outline the host was given (`{sketch}`, `{face}` or `{loops}`), else the default.
    #[track_caller]
    pub fn get(self) -> PickParam {
        let line = std::panic::Location::caller().line();
        let given = self.r.declare(&self.spec, line);
        let value = match given {
            Some(Json::Null) | None => self.spec.default.clone(),
            Some(v) => v,
        };
        PickParam { name: self.spec.name.clone(), kind: "outline".to_string(), value, curved: false }
    }
}

/// Text on a face (API 1.3), the `text` op as a typed call: `s.lettering(&part, &face, "LOT 42", &Lettering::new(6.0, 0.8).font("mono").around())`.
/// `height` is the capitals' and `depth` how far the letters stand off the surface (raised) or go into it (`.engrave()`), square to
/// it, both in mm. `font` is a bundled id (`sans`, `mono`, `serif`, `stencil`, `stroke`, `line`) or a font already in the project;
/// no font is the single-stroke grid font on a flat face. On a cylinder or a cone `.around()` (the default) reads round the part and
/// `.axis()` up it; on a sphere, a torus or a freeform face `.u()` or `.v()` is the face's direction the baseline starts in, and
/// `.project()` pushes the letters on along the face's normal instead of wrapping them at true size.
#[derive(Clone, Debug, PartialEq)]
pub struct Lettering {
    height: f64,
    depth: f64,
    font: Option<String>,
    style: Option<String>,
    weight: Option<f64>,
    spacing: Option<f64>,
    line_spacing: Option<f64>,
    align: Option<String>,
    angle: Option<f64>,
    along: Option<String>,
    wrap: Option<String>,
    flip: bool,
    cut: bool,
    stroke_width: Option<f64>,
}

impl Lettering {
    /// Capitals `height` mm tall, standing `depth` mm off the surface.
    pub fn new(height: f64, depth: f64) -> Lettering {
        Lettering { height, depth, font: None, style: None, weight: None, spacing: None, line_spacing: None, align: None, angle: None, along: None, wrap: None, flip: false, cut: false, stroke_width: None }
    }
    pub fn font(mut self, font: &str) -> Lettering {
        self.font = Some(font.to_string());
        self
    }
    /// `regular`, `bold`, `italic` or `bold-italic`.
    pub fn style(mut self, style: &str) -> Lettering {
        self.style = Some(style.to_string());
        self
    }
    /// A variable font's weight, 100 to 900.
    pub fn weight(mut self, weight: f64) -> Lettering {
        self.weight = Some(weight);
        self
    }
    /// Added after every letter, mm.
    pub fn spacing(mut self, mm: f64) -> Lettering {
        self.spacing = Some(mm);
        self
    }
    /// Capital heights between lines (1.3 by default).
    pub fn line_spacing(mut self, heights: f64) -> Lettering {
        self.line_spacing = Some(heights);
        self
    }
    /// `left`, `centre` or `right`, for each line.
    pub fn align(mut self, align: &str) -> Lettering {
        self.align = Some(align.to_string());
        self
    }
    /// The layout turned this much, degrees.
    pub fn angle(mut self, deg: f64) -> Lettering {
        self.angle = Some(deg);
        self
    }
    /// A pen font's line width, mm.
    pub fn stroke_width(mut self, mm: f64) -> Lettering {
        self.stroke_width = Some(mm);
        self
    }
    /// Round a cylinder or a cone (the default there).
    pub fn around(mut self) -> Lettering {
        self.along = Some("around".to_string());
        self
    }
    /// Up a cylinder's or a cone's axis.
    pub fn axis(mut self) -> Lettering {
        self.along = Some("axis".to_string());
        self
    }
    /// Along the face's U (a sphere, a torus or a freeform face).
    pub fn u(mut self) -> Lettering {
        self.along = Some("u".to_string());
        self
    }
    /// Along the face's V.
    pub fn v(mut self) -> Lettering {
        self.along = Some("v".to_string());
        self
    }
    /// Push the letters onto a sphere, a torus or a freeform face along its normal, like a slide, instead of wrapping them.
    pub fn project(mut self) -> Lettering {
        self.wrap = Some("project".to_string());
        self
    }
    /// Read the other way round, upside down.
    pub fn flip(mut self) -> Lettering {
        self.flip = true;
        self
    }
    /// Cut the letters into the part instead of raising them.
    pub fn engrave(mut self) -> Lettering {
        self.cut = true;
        self
    }

    /// The fields both the op and the outline call share.
    fn common(&self, text: &str, mut j: Json) -> Json {
        j = j.set("text", text).set("height", self.height);
        if let Some(v) = &self.font {
            j = j.set("font", v.as_str());
        }
        if let Some(v) = &self.style {
            j = j.set("style", v.as_str());
        }
        if let Some(v) = self.weight {
            j = j.set("weight", v);
        }
        if let Some(v) = self.spacing {
            j = j.set("spacing", v);
        }
        if let Some(v) = self.line_spacing {
            j = j.set("lineSpacing", v);
        }
        if let Some(v) = &self.align {
            j = j.set("align", v.as_str());
        }
        if let Some(v) = self.stroke_width {
            j = j.set("strokeWidth", v);
        }
        j
    }

    /// The `text` op for `face`: `faceIndex`, and for a curved face the `point` the text is centred on.
    pub fn op(&self, text: &str, face: &PickedFace) -> Json {
        let mut j = self.common(text, Json::obj().set("kind", "text").set("faceIndex", face.index)).set("depth", self.depth);
        if face.kind != "plane" {
            j = j.set("point", face.point);
        }
        if self.cut {
            j = j.set("op", "cut");
        }
        if let Some(v) = self.angle {
            j = j.set("angleDeg", v);
        }
        if let Some(v) = &self.along {
            j = j.set("along", v.as_str());
        }
        if let Some(v) = &self.wrap {
            j = j.set("wrap", v.as_str());
        }
        if self.flip {
            j = j.set("flip", true);
        }
        j
    }

    /// The arguments of the host's `text_outline` call.
    fn outline_args(&self, text: &str) -> Json {
        let mut j = self.common(text, Json::obj());
        if let Some(v) = self.angle {
            j = j.set("angleDeg", v);
        }
        j
    }
}

fn hold(v: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let mut v = v;
    if let Some(lo) = min {
        if v < lo {
            v = lo;
        }
    }
    if let Some(hi) = max {
        if v > hi {
            v = hi;
        }
    }
    v
}

impl Session {
    /// Reads the start line: `{"v":1,"params":{...},"context":{...}}`, with `"describe":true` for describe mode.
    pub fn start() -> Session {
        let start = Json::parse(&read_line()).unwrap_or(Json::Null);
        let enabled = start.at("context.nativeGeometry").and_then(Json::as_bool) == Some(true)
            && start.get("describe").and_then(Json::as_bool) != Some(true);
        NATIVE_GEOMETRY.store(enabled, Ordering::Relaxed);
        Session::from_start(&start)
    }

    /// A session from a start message already read (tests, other transports).
    pub fn from_start(start: &Json) -> Session {
        Session {
            params: start.get("params").cloned().unwrap_or(Json::obj()),
            context: start.get("context").cloned().unwrap_or(Json::obj()),
            next_id: 1,
            describe: start.get("describe").and_then(Json::as_bool) == Some(true),
        }
    }

    /// True in describe mode: every parameter gives its default and the run ends at its first call.
    pub fn describing(&self) -> bool {
        self.describe
    }

    /// What a parameter builder needs of the run: the value the host sent, describe mode, and whether a call came first.
    fn reader(&self, name: &str) -> Reader {
        Reader { given: self.params.get(name).cloned(), describe: self.describe, late: self.next_id > 1 }
    }

    /// A number parameter: `s.number("diameter", 80.0).range(20.0, 250.0).unit("mm").label("Outer diameter").get()`.
    /// Declare every parameter before the first call, so a form can show it.
    pub fn number(&self, name: &str, default: f64) -> Number {
        Number { r: self.reader(name), spec: ParamSpec::new(name, "number", Json::from(default)) }
    }

    /// A whole-number parameter: `s.count("holes", 6).range(3, 24).get()`.
    pub fn count(&self, name: &str, default: u32) -> Count {
        Count { r: self.reader(name), spec: ParamSpec::new(name, "count", Json::from(default)) }
    }

    /// An on-or-off parameter: `s.toggle("bore", true).get()`.
    pub fn toggle(&self, name: &str, default: bool) -> Toggle {
        Toggle { r: self.reader(name), spec: ParamSpec::new(name, "toggle", Json::from(default)) }
    }

    /// One of `options`: `s.choice("edge", &["fillet", "chamfer", "sharp"], "fillet").get()`.
    pub fn choice(&self, name: &str, options: &[&str], default: &str) -> Choice {
        let mut spec = ParamSpec::new(name, "choice", Json::from(default));
        spec.options = options.iter().map(|o| o.to_string()).collect();
        Choice { r: self.reader(name), spec }
    }

    /// A text parameter: `s.text("label", "LIME").max_len(20).get()`.
    pub fn text(&self, name: &str, default: &str) -> Text {
        Text { r: self.reader(name), spec: ParamSpec::new(name, "text", Json::from(default)) }
    }

    /// A face of the part the person picks (API 1.2): `s.face("face").rule("largest_up").label("Face to cut").get()`.
    pub fn face(&self, name: &str) -> FaceBuilder {
        FaceBuilder { r: self.reader(name), spec: ParamSpec::new(name, "face", Json::Null) }
    }

    /// A closed outline the person supplies (API 1.2): `s.outline("shape").default_poly(&geom2d::rect(80.0, 60.0)).get()`.
    pub fn outline(&self, name: &str) -> OutlineBuilder {
        OutlineBuilder { r: self.reader(name), spec: ParamSpec::new(name, "outline", Json::Null) }
    }

    /// The face a `face` parameter names, resolved on `part` as it stands now. A face the person picked is found again
    /// by its normal and a point on it, so it survives a change to the steps before it; when it is gone the host
    /// refuses with a sentence naming the parameter.
    #[track_caller]
    pub fn pick_face(&mut self, part: &Part, face: &PickParam) -> Result<PickedFace, Refusal> {
        let line = std::panic::Location::caller().line();
        let mut args = Json::obj().set("param", face.name.as_str()).set("kind", "face").set("object", part.object.as_str()).set("value", face.value.clone());
        if face.curved {
            args = args.set("curved", true);
        }
        let r = self.call("pick", args)?;
        let outline = Section::all(&r).into_iter().next().ok_or(Refusal { reason: "pick gave no face".to_string(), line })?;
        Ok(PickedFace {
            index: r.get("face").and_then(Json::as_f64).unwrap_or(0.0) as u32,
            normal: vec3(r.get("normal")),
            point: vec3(r.get("nearPoint")),
            area: r.get("areaMm2").and_then(Json::as_f64).unwrap_or(0.0),
            outline,
            kind: r.get("surface").and_then(Json::as_str).unwrap_or("plane").to_string(),
            axis: r.get("axis").map(|a| vec3(Some(a))),
            radius: r.get("radiusMm").and_then(Json::as_f64),
            centre: r.get("centre").map(|c| vec3(Some(c))),
            axis_point: r.get("axisPoint").map(|a| vec3(Some(a))),
        })
    }

    /// The region an `outline` parameter names, with the frame it lies in. A face's outline is read on the run's part
    /// unless the value names another; a sketch is read from the step it names.
    #[track_caller]
    pub fn pick_outline(&mut self, outline: &PickParam) -> Result<Section, Refusal> {
        let line = std::panic::Location::caller().line();
        let mut args = Json::obj().set("param", outline.name.as_str()).set("kind", "outline").set("value", outline.value.clone());
        if let Some(o) = self.context.get("object").and_then(Json::as_str) {
            args = args.set("object", o);
        }
        let r = self.call("pick", args)?;
        Section::all(&r).into_iter().next().ok_or(Refusal { reason: "pick gave no outline".to_string(), line })
    }

    /// Text on `face` (API 1.3), one history step: the `text` op built from `how`. `face` is what `pick_face` gave, flat or curved.
    #[track_caller]
    pub fn lettering(&mut self, part: &Part, face: &PickedFace, text: &str, how: &Lettering) -> Result<Json, Refusal> {
        self.apply(part, how.op(text, face))
    }

    /// The letters of `text` in `font` at capitals `height` mm as a flat outline (API 1.3): `.region` for `geom2d`, `.loops` for
    /// `extrude`. The host lays it out, since the recipe crate reads no fonts. It is centred on the origin of a frame with U right
    /// and V up, so `.extrude("fuse", depth, section.loops.clone())` raises it, and a stencil or a fit test can use the region.
    #[track_caller]
    pub fn text_outline(&mut self, text: &str, font: &str, height: f64) -> Result<Section, Refusal> {
        self.text_outline_with(text, &Lettering::new(height, 0.0).font(font))
    }

    /// `text_outline` with the style, weight, spacing, line spacing, alignment, angle and pen width of `how` (its depth is not used).
    #[track_caller]
    pub fn text_outline_with(&mut self, text: &str, how: &Lettering) -> Result<Section, Refusal> {
        let line = std::panic::Location::caller().line();
        let r = self.call("text_outline", how.outline_args(text))?;
        Section::all(&r).into_iter().next().ok_or(Refusal { reason: "text_outline gave no outline".to_string(), line })
    }

    /// The part's bounding box, `(min, max)` in mm (API 1.2), in the frame `apply` takes: model space, where a moved part stands.
    #[track_caller]
    pub fn bounds(&mut self, part: &Part) -> Result<([f64; 3], [f64; 3]), Refusal> {
        let line = std::panic::Location::caller().line();
        let r = self.call("model_describe", Json::obj().set("object", part.object.as_str()).set("limit", 1u32))?;
        match r.get("bounds") {
            Some(b) => Ok((vec3(b.get("min")), vec3(b.get("max")))),
            None => Err(Refusal { reason: "model_describe gave no bounds".to_string(), line }),
        }
    }

    /// Ends the run with `sentence` as its plain refusal (API 1.2): nothing the run did is kept, and the person or the
    /// agent reads the sentence as the reason, with no panic line around it.
    #[track_caller]
    pub fn refuse(&self, sentence: &str) -> ! {
        let line = std::panic::Location::caller().line();
        tell(Json::obj().set("refuse", Json::obj().set("reason", sentence).set("line", line)));
        while !read_line().is_empty() {}
        panic!("{}", sentence);
    }

    /// A number parameter, or `default` when the host did not send it: `number(name, default).get()`.
    #[track_caller]
    pub fn param_f64(&self, name: &str, default: f64) -> f64 {
        self.number(name, default).get()
    }

    /// A count parameter (a number the host sent, rounded down), or `default`.
    #[track_caller]
    pub fn param_u32(&self, name: &str, default: u32) -> u32 {
        self.count(name, default).get()
    }

    #[track_caller]
    pub fn param_bool(&self, name: &str, default: bool) -> bool {
        self.toggle(name, default).get()
    }

    #[track_caller]
    pub fn param_str(&self, name: &str, default: &str) -> String {
        self.text(name, default).get()
    }

    /// The context the host sent: printer, material, units, a starting object.
    pub fn context(&self) -> &Json {
        &self.context
    }

    /// Report progress from 0.0 to 1.0 and a short description to the host.
    pub fn progress(&self, fraction: f64, message: &str) {
        if self.describe { return; }
        tell(Json::obj().set("progress", Json::obj().set("fraction", fraction).set("message", message)));
    }

    /// The object the run starts from, when the host named one.
    pub fn object(&self) -> Option<Part> {
        if self.describe {
            tell(Json::obj().set("describe", Json::obj().set("object", true)));
        }
        self.context.get("object").and_then(Json::as_str).map(|o| Part { object: o.to_string() })
    }

    /// Read the named members and joints of the assembly given to an animation.
    #[track_caller]
    pub fn rig(&mut self) -> Result<anim::Rig, Refusal> {
        let line = std::panic::Location::caller().line();
        let result = self.call("rig", Json::obj())?;
        anim::Rig::from_json(&result).ok_or(Refusal { reason: "The host gave no animation rig.".to_string(), line })
    }

    /// Send the whole animation once. On success the animation run is complete.
    #[track_caller]
    pub fn animate(&mut self, animation: &anim::Animation) -> Result<Json, Refusal> {
        let result = self.call("animate", animation.to_json())?;
        tell(Json::obj().set("done", Json::obj().set("object", "").set("metrics", Json::obj())));
        Ok(result)
    }

    /// One call: `tool` is an MCP tool name from the host's allowlist, `args`
    /// as that tool takes them. The answer is the tool's result, or the
    /// host's refusal with the line that made the call.
    #[track_caller]
    pub fn call(&mut self, tool: &str, args: Json) -> Result<Json, Refusal> {
        let at = std::panic::Location::caller();
        if self.describe {
            self.end_describe(at.line());
        }
        let id = self.next_id;
        self.next_id += 1;
        let msg = Json::obj()
            .set("id", id)
            .set("call", tool)
            .set("args", args)
            .set("at", Json::obj().set("line", at.line()).set("col", at.column()));
        let mut out = std::io::stdout().lock();
        writeln!(out, "{msg}").unwrap();
        out.flush().unwrap();
        drop(out);
        let reply = Json::parse(&read_line()).unwrap_or(Json::Null);
        if reply.get("ok").and_then(Json::as_bool) == Some(true) {
            Ok(reply.get("result").cloned().unwrap_or(Json::Null))
        } else {
            let reason = reply.get("error").and_then(Json::as_str).unwrap_or("the host gave no answer").to_string();
            Err(Refusal { reason, line: at.line() })
        }
    }

    /// `solid_new`: a new solid on the plate (`ops::cuboid`, `ops::cylinder`, or any primitive as JSON).
    #[track_caller]
    pub fn solid_new(&mut self, primitive: Json) -> Result<Part, Refusal> {
        let line = std::panic::Location::caller().line();
        let made = self.call("solid_new", primitive)?;
        match made.get("object").and_then(Json::as_str) {
            Some(o) => Ok(Part { object: o.to_string() }),
            None => Err(Refusal { reason: "solid_new returned no object".to_string(), line }),
        }
    }

    /// `model_apply` with `probe: true`: the edit is checked first and lands only if it fits.
    #[track_caller]
    pub fn apply(&mut self, part: &Part, op: Json) -> Result<Json, Refusal> {
        self.call("model_apply", Json::obj().set("object", part.object.as_str()).set("op", op).set("probe", true))
    }

    /// `model_probe`: the edit on a copy. `metrics` holds the volume and bounds before and after.
    #[track_caller]
    pub fn probe(&mut self, part: &Part, op: Json) -> Result<Json, Refusal> {
        self.call("model_probe", Json::obj().set("object", part.object.as_str()).set("op", op))
    }

    /// The solid's volume in mm³, from a probe of an edit that changes nothing.
    #[track_caller]
    pub fn volume(&mut self, part: &Part) -> Result<f64, Refusal> {
        let line = std::panic::Location::caller().line();
        let r = self.call("model_describe", Json::obj().set("object", part.object.as_str()))?;
        r.at("volumeMm3").and_then(Json::as_f64).ok_or(Refusal { reason: "model_describe gave no volume".to_string(), line })
    }

    /// `model_check`: overhangs, thin walls, floating pieces, bed contact.
    #[track_caller]
    pub fn check(&mut self, part: &Part) -> Result<Json, Refusal> {
        self.call("model_check", Json::obj().set("object", part.object.as_str()))
    }

    /// `data {name}`: the workspace's `<name>.json` as it is, or `<name>.csv` as rows of numbers.
    #[track_caller]
    pub fn data(&mut self, name: &str) -> Result<Json, Refusal> {
        self.call("data", Json::obj().set("name", name))
    }

    /// `data {name}` as a list of numbers (a JSON array, or a one-column CSV).
    #[track_caller]
    pub fn data_f64(&mut self, name: &str) -> Result<Vec<f64>, Refusal> {
        let line = std::panic::Location::caller().line();
        let d = self.data(name)?;
        d.as_f64s().ok_or(Refusal { reason: format!("The data {name} is not a list of numbers."), line })
    }

    /// `data {name}` as points: `[x, y]` rows (a two-column CSV) or `{x, y}` objects, ready for `geom2d::Poly::new`.
    #[track_caller]
    pub fn data_points2(&mut self, name: &str) -> Result<Vec<[f64; 2]>, Refusal> {
        let line = std::panic::Location::caller().line();
        let d = self.data(name)?;
        d.as_points2().ok_or(Refusal { reason: format!("The data {name} is not a list of [x, y] points."), line })
    }

    /// `section {object, face, insetMm}`: a flat face's outline, inset by `inset` mm (0 for the edge itself).
    /// `face` is an index (`3.0`) or `Json::obj().set("normal", [0.0, 0.0, 1.0]).set("nearPoint", [x, y, z])`.
    /// An inset that leaves nothing gives an empty region.
    #[track_caller]
    pub fn face_outline(&mut self, part: &Part, face: impl Into<Json>, inset: f64) -> Result<Section, Refusal> {
        let line = std::panic::Location::caller().line();
        let r = self.call("section", Json::obj().set("object", part.object.as_str()).set("face", face).set("insetMm", inset))?;
        Section::all(&r).into_iter().next().ok_or(Refusal { reason: "section gave no outline".to_string(), line })
    }

    /// `section {object, planes}`: the part cut by the plane through `origin` square to `normal`.
    #[track_caller]
    pub fn section(&mut self, part: &Part, origin: [f64; 3], normal: [f64; 3]) -> Result<Section, Refusal> {
        let line = std::panic::Location::caller().line();
        let plane = Json::obj().set("origin", origin).set("normal", normal);
        let r = self.call("section", Json::obj().set("object", part.object.as_str()).set("planes", Json::Arr(vec![plane])))?;
        Section::all(&r).into_iter().next().ok_or(Refusal { reason: "section gave no cut".to_string(), line })
    }

    /// `section` with any of its arguments (`planes`, `fan`, `levels`; the object is set here): one `Section` a plane.
    #[track_caller]
    pub fn sections(&mut self, part: &Part, args: Json) -> Result<Vec<Section>, Refusal> {
        let r = self.call("section", args.set("object", part.object.as_str()))?;
        Ok(Section::all(&r))
    }

    /// `model_apply_batch {combine}`: every op (`op` is "cut" or "fuse", and each op's own `op` must match) meets the
    /// part in one boolean, one history step. An op that does not fit is listed in `failures`; the rest apply.
    #[track_caller]
    pub fn apply_many(&mut self, part: &Part, op: &str, ops: Vec<Json>) -> Result<Combined, Refusal> {
        let r = self.call("model_apply_batch", Json::obj().set("object", part.object.as_str()).set("ops", Json::Arr(ops)).set("combine", op))?;
        Ok(Combined::from_json(&r))
    }

    /// Describe mode ends at the first call: the host has every declared parameter and stops the program here.
    fn end_describe(&self, line: u32) {
        tell(Json::obj().set("describe", Json::obj().set("end", true).set("line", line)));
        while !read_line().is_empty() {}
        panic!("describe mode ends before the first call");
    }

    /// A note the host shows with the result.
    #[track_caller]
    pub fn log(&mut self, text: &str) {
        let _ = self.call("log", Json::obj().set("text", text));
    }

    /// Commit work so far and let the host resume this run after an interruption.
    #[track_caller]
    pub fn checkpoint(&mut self) -> Result<(), Refusal> {
        self.call("checkpoint", Json::obj()).map(|_| ())
    }

    /// Ends the run: the host commits the part to the plate.
    pub fn done(self, part: &Part) {
        self.done_with(part, Json::obj())
    }

    /// Ends the run with numbers for the caller (a search ranks by them).
    pub fn done_with(self, part: &Part, metrics: Json) {
        if self.describe {
            tell(Json::obj().set("describe", Json::obj().set("end", true)));
            return;
        }
        let msg = Json::obj().set("done", Json::obj().set("object", part.object.as_str()).set("metrics", metrics));
        let mut out = std::io::stdout().lock();
        writeln!(out, "{msg}").unwrap();
        out.flush().unwrap();
    }
}

fn vec3(v: Option<&Json>) -> [f64; 3] {
    let n = |i: usize| v.and_then(|a| a.as_array()).and_then(|a| a.get(i)).and_then(Json::as_f64).unwrap_or(0.0);
    [n(0), n(1), n(2)]
}

/// A plane's frame in the part frame: `origin`, and the unit axes `u`, `v` (in the plane) and `w` (its normal).
/// A point `[a, b]` of a section is `origin + a·u + b·v`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Frame {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub w: [f64; 3],
}

impl Frame {
    /// The part-frame point at `[a, b]` in this plane.
    pub fn point(&self, p: geom2d::Pt) -> [f64; 3] {
        let mut out = [0.0; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.origin[i] + p[0] * self.u[i] + p[1] * self.v[i];
        }
        out
    }
}

/// One answer of `section`: a face's outline or a plane's cut, in the plane's frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub frame: Frame,
    /// The `origin` and `turn` an `extrude` takes to draw in this frame.
    pub origin: [f64; 3],
    pub turn: [f64; 3],
    /// The area as points (curves sampled to the call's chord, 0.05 mm by default), for `geom2d`.
    pub region: geom2d::Region,
    /// The kernel's `loops`, arcs kept, ready for `extrude.loops`.
    pub loops: Json,
    /// Net area, mm².
    pub area: f64,
}

impl Section {
    /// Every section in a `section` reply, or in recipe data saved with `saveAs` (the same shape).
    pub fn all(reply: &Json) -> Vec<Section> {
        match reply.get("sections").and_then(Json::as_array) {
            Some(rows) => rows.iter().map(Section::from_json).collect(),
            None => Vec::new(),
        }
    }

    /// One section from its JSON row.
    pub fn from_json(row: &Json) -> Section {
        let frame = row.get("frame");
        let mut loops: Vec<geom2d::Poly> = Vec::new();
        if let Some(regions) = row.get("regions").and_then(Json::as_array) {
            for r in regions.iter() {
                if let Some(outer) = r.get("outer").and_then(Json::as_points2) {
                    loops.push(geom2d::Poly::new(outer));
                }
                if let Some(holes) = r.get("holes").and_then(Json::as_array) {
                    for h in holes.iter() {
                        if let Some(pts) = h.as_points2() {
                            loops.push(geom2d::Poly::new(pts));
                        }
                    }
                }
            }
        }
        Section {
            frame: Frame {
                origin: vec3(frame.and_then(|f| f.get("origin"))),
                u: vec3(frame.and_then(|f| f.get("u"))),
                v: vec3(frame.and_then(|f| f.get("v"))),
                w: vec3(frame.and_then(|f| f.get("w"))),
            },
            origin: vec3(row.get("origin")),
            turn: vec3(row.get("turn")),
            region: if loops.is_empty() { geom2d::Region::empty() } else { geom2d::Region::from_loops(&loops) },
            loops: row.get("loops").cloned().unwrap_or(Json::Arr(Vec::new())),
            area: row.get("areaMm2").and_then(Json::as_f64).unwrap_or(0.0),
        }
    }

    /// An `extrude` op that draws `loops` (one piece: an outline, then its holes) in this frame: `op` is "cut" or
    /// "fuse", `depth` along `w` (negative goes into the part).
    pub fn extrude(&self, op: &str, depth: f64, loops: Json) -> Json {
        Json::obj().set("kind", "extrude").set("op", op).set("origin", self.origin).set("turn", self.turn).set("depth", depth).set("loops", loops)
    }
}

/// What `apply_many` did: how many ops applied, and each op left out with its index in the list and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Combined {
    pub applied: u32,
    pub failures: Vec<(usize, String)>,
    /// The whole reply (node, fingerprint, bounds).
    pub reply: Json,
}

impl Combined {
    pub fn from_json(r: &Json) -> Combined {
        let mut failures: Vec<(usize, String)> = Vec::new();
        if let Some(rows) = r.get("failures").and_then(Json::as_array) {
            for f in rows.iter() {
                let index = f.get("index").and_then(Json::as_f64).unwrap_or(0.0) as usize;
                failures.push((index, f.get("reason").and_then(Json::as_str).unwrap_or("").to_string()));
            }
        }
        Combined { applied: r.get("applied").and_then(Json::as_f64).unwrap_or(0.0) as u32, failures, reply: r.clone() }
    }
}

/// Builders for common primitives and ops. Anything else is a `Json` object
/// as the MCP tool takes it.
pub mod ops {
    use super::Json;

    /// A box `dx` × `dy` × `dz`, centred on X and Y, standing on Z = 0.
    pub fn cuboid(dx: f64, dy: f64, dz: f64) -> Json {
        Json::obj().set("primitive", "box").set("dx", dx).set("dy", dy).set("dz", dz)
    }

    /// A cylinder standing on Z = 0.
    pub fn cylinder(radius: f64, height: f64) -> Json {
        Json::obj().set("primitive", "cylinder").set("radius", radius).set("height", height)
    }

    /// A through hole of `diameter` mm from `origin` along `axis` (part frame).
    pub fn hole_through(origin: [f64; 3], axis: [f64; 3], diameter: f64) -> Json {
        Json::obj().set("kind", "hole").set("origin", origin).set("axis", axis).set("diameterMm", diameter).set("through", true)
    }

    /// A blind hole `depth` mm deep.
    pub fn hole(origin: [f64; 3], axis: [f64; 3], diameter: f64, depth: f64) -> Json {
        Json::obj().set("kind", "hole").set("origin", origin).set("axis", axis).set("diameterMm", diameter).set("depth", depth)
    }

    /// A boolean with a primitive placed at `origin`: `op` is "fuse", "cut" or "common".
    pub fn boolean(op: &str, primitive: Json, origin: [f64; 3]) -> Json {
        let mut fields = match primitive {
            Json::Obj(fields) => fields,
            _ => Vec::new(),
        };
        fields.retain(|(k, _)| k != "origin");
        Json::Obj(fields).set("kind", "boolean").set("op", op).set("origin", origin)
    }

    /// A fillet of `size` mm on the edges `edges` (as the MCP tool spells an edge list).
    pub fn fillet(edges: Json, size: f64) -> Json {
        Json::obj().set("kind", "fillet").set("edges", edges).set("size", size)
    }
}

/// 2D geometry for sketches and cell patterns: polygons, regions with holes,
/// booleans, offsets, rounded and chamfered corners, cleanup, and the `loops`
/// JSON `extrude` takes. Pure functions, no calls to the host.
///
/// A `Region` keeps every outline counter-clockwise and every hole clockwise,
/// so material is always on the left of a loop. Booleans and offsets accept
/// concave outlines, holes, touching edges and overlapping edges.
pub mod geom2d {
    use super::Json;
    use std::collections::HashMap;

    const PI: f64 = std::f64::consts::PI;

    /// A point or a vector, `[x, y]` in mm.
    pub type Pt = [f64; 2];

    /// Points closer than this (mm) are one point in a boolean or an offset.
    pub const SNAP: f64 = 1e-7;

    fn sub(a: Pt, b: Pt) -> Pt {
        [a[0] - b[0], a[1] - b[1]]
    }
    fn add(a: Pt, b: Pt) -> Pt {
        [a[0] + b[0], a[1] + b[1]]
    }
    fn mul(a: Pt, k: f64) -> Pt {
        [a[0] * k, a[1] * k]
    }
    fn dot(a: Pt, b: Pt) -> f64 {
        a[0] * b[0] + a[1] * b[1]
    }
    fn cross(a: Pt, b: Pt) -> f64 {
        a[0] * b[1] - a[1] * b[0]
    }
    fn len(a: Pt) -> f64 {
        a[0].hypot(a[1])
    }
    fn dist(a: Pt, b: Pt) -> f64 {
        len(sub(a, b))
    }
    fn norm(a: Pt) -> Pt {
        let l = len(a);
        if l > 0.0 { [a[0] / l, a[1] / l] } else { [0.0, 0.0] }
    }
    /// `x` held between `lo` and `hi`.
    pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
        if x < lo {
            lo
        } else if x > hi {
            hi
        } else {
            x
        }
    }

    fn turn(p: Pt, deg: f64) -> Pt {
        let (s, c) = sin_cos_deg(deg);
        [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
    }

    // Trigonometry from + - * / and sqrt only (fdlibm's kernels), so a recipe gets the same bits under
    // lust and rustc: their libm sin, cos, tan, atan2 and acos differ in the last bit for some inputs.

    #[allow(clippy::excessive_precision)]
    fn k_sin(x: f64) -> f64 {
        let z = x * x;
        let r = 0.00833333333332249 + z * (-0.0001984126982985795 + z * (2.7557313707070068e-06 + z * (-2.5050760253406863e-08 + z * 1.58969099521155e-10)));
        x + z * x * (-0.16666666666666632 + z * r)
    }

    #[allow(clippy::excessive_precision)]
    fn k_cos(x: f64) -> f64 {
        let z = x * x;
        let r = z * (0.0416666666666666 + z * (-0.001388888888887411 + z * (2.480158728947673e-05 + z * (-2.7557314351390663e-07 + z * (2.087572321298175e-09 + z * -1.1359647557788195e-11)))));
        let hz = 0.5 * z;
        let w = 1.0 - hz;
        w + (((1.0 - w) - hz) + z * r)
    }

    /// `(sin, cos)` of `n` quarter turns plus `r` (within ±π/4).
    fn quadrant(n: i64, r: f64) -> (f64, f64) {
        let (s, c) = (k_sin(r), k_cos(r));
        let (s, c) = match n.rem_euclid(4) {
            0 => (s, c),
            1 => (c, -s),
            2 => (-s, -c),
            _ => (-c, s),
        };
        (s + 0.0, c + 0.0)
    }

    /// `(sin, cos)` of an angle in degrees, exact at multiples of 90°, with the same bits under lust and rustc.
    pub fn sin_cos_deg(deg: f64) -> (f64, f64) {
        let n = (deg / 90.0).round();
        let r = (deg - 90.0 * n) * (PI / 180.0);
        quadrant(n as i64, r)
    }

    fn sin_cos(x: f64) -> (f64, f64) {
        let n = (x * (2.0 / PI)).round();
        let r = (x - n * 1.5707963267341256) - n * 6.077100506506192e-11;
        quadrant(n as i64, r)
    }

    #[allow(clippy::excessive_precision, clippy::approx_constant)]
    fn atan(x: f64) -> f64 {
        const HI: [f64; 4] = [0.4636476090008061, 0.7853981633974483, 0.982793723247329, 1.5707963267948966];
        const LO: [f64; 4] = [2.2698777452961687e-17, 3.061616997868383e-17, 1.3903311031230998e-17, 6.123233995736766e-17];
        let neg = x < 0.0;
        let mut a = x.abs();
        if a >= 7.378697629483821e19 {
            return if neg { -HI[3] - LO[3] } else { HI[3] + LO[3] };
        }
        let id: i32 = if a < 0.4375 {
            -1
        } else if a < 1.1875 {
            if a < 0.6875 {
                a = (2.0 * a - 1.0) / (2.0 + a);
                0
            } else {
                a = (a - 1.0) / (a + 1.0);
                1
            }
        } else if a < 2.4375 {
            a = (a - 1.5) / (1.0 + 1.5 * a);
            2
        } else {
            a = -1.0 / a;
            3
        };
        let z = a * a;
        let w = z * z;
        let s1 = z * (0.3333333333333293 + w * (0.14285714272503466 + w * (0.09090887133436507 + w * (0.06661073137387531 + w * (0.049768779946159324 + w * 0.016285820115365782)))));
        let s2 = w * (-0.19999999999876483 + w * (-0.11111110405462356 + w * (-0.0769187620504483 + w * (-0.058335701337905735 + w * -0.036531572744216916))));
        let v = if id < 0 {
            a - a * (s1 + s2)
        } else {
            let k = id as usize;
            HI[k] - ((a * (s1 + s2) - LO[k]) - a)
        };
        if neg { -v } else { v }
    }

    fn atan2(y: f64, x: f64) -> f64 {
        const PI_LO: f64 = 1.2246467991473532e-16;
        if x == 0.0 {
            return if y > 0.0 {
                PI / 2.0
            } else if y < 0.0 {
                -PI / 2.0
            } else {
                0.0
            };
        }
        let z = atan((y / x).abs());
        if x > 0.0 {
            if y < 0.0 { -z } else { z }
        } else if y < 0.0 {
            (z - PI_LO) - PI
        } else {
            PI - (z - PI_LO)
        }
    }

    /// The parameter along a..b of the point nearest `p`, and the distance to it.
    fn seg_near(p: Pt, a: Pt, b: Pt) -> (f64, f64) {
        let d = sub(b, a);
        let l2 = dot(d, d);
        let t = if l2 > 0.0 { clamp(dot(sub(p, a), d) / l2, 0.0, 1.0) } else { 0.0 };
        (t, dist(p, add(a, mul(d, t))))
    }

    /// Winding number of a closed point list around `p` (not on it).
    fn winding(pts: &[Pt], p: Pt) -> i32 {
        let n = pts.len();
        let mut w = 0;
        for i in 0..n {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            let side = cross(sub(b, a), sub(p, a));
            if a[1] <= p[1] {
                if b[1] > p[1] && side > 0.0 {
                    w += 1;
                }
            } else if b[1] <= p[1] && side < 0.0 {
                w -= 1;
            }
        }
        w
    }

    fn on_boundary(pts: &[Pt], p: Pt) -> bool {
        let n = pts.len();
        (0..n).any(|i| seg_near(p, pts[i], pts[(i + 1) % n]).1 <= SNAP)
    }

    fn bbox_of(pts: &[Pt]) -> [Pt; 2] {
        if pts.is_empty() {
            return [[0.0, 0.0], [0.0, 0.0]];
        }
        let mut lo = pts[0];
        let mut hi = pts[0];
        for p in pts.iter() {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        [lo, hi]
    }

    fn json_pt(p: Pt) -> Json {
        Json::from(vec![p[0], p[1]])
    }

    /// A closed outline: the last point joins the first. Counter-clockwise has positive area.
    #[derive(Clone, Debug, PartialEq, Default)]
    pub struct Poly {
        /// The corners in order.
        pub pts: Vec<Pt>,
    }

    impl From<Vec<Pt>> for Poly {
        fn from(pts: Vec<Pt>) -> Poly {
            Poly::new(pts)
        }
    }

    impl Poly {
        /// The outline through `pts`; a last point equal to the first is dropped.
        pub fn new(pts: Vec<Pt>) -> Poly {
            let mut pts = pts;
            while pts.len() > 1 && dist(pts[0], pts[pts.len() - 1]) <= SNAP {
                pts.pop();
            }
            Poly { pts }
        }

        /// Signed area in mm²: positive when counter-clockwise.
        pub fn area(&self) -> f64 {
            let n = self.pts.len();
            let mut s = 0.0;
            for i in 0..n {
                s += cross(self.pts[i], self.pts[(i + 1) % n]);
            }
            s * 0.5
        }

        /// Length of the closed outline in mm.
        pub fn perimeter(&self) -> f64 {
            let n = self.pts.len();
            (0..n).map(|i| dist(self.pts[i], self.pts[(i + 1) % n])).fold(0.0, |s, d| s + d)
        }

        /// The centre of the enclosed area (the mean of the points when there is no area).
        pub fn centroid(&self) -> Pt {
            let n = self.pts.len();
            if n == 0 {
                return [0.0, 0.0];
            }
            let a = self.area();
            if a.abs() < 1e-12 {
                let s = self.pts.iter().fold([0.0, 0.0], |acc, p| add(acc, *p));
                return mul(s, 1.0 / n as f64);
            }
            let mut c = [0.0, 0.0];
            for i in 0..n {
                let p = self.pts[i];
                let q = self.pts[(i + 1) % n];
                let k = cross(p, q);
                c = add(c, mul(add(p, q), k));
            }
            mul(c, 1.0 / (6.0 * a))
        }

        /// `[min, max]` corners of the bounding box.
        pub fn bbox(&self) -> [Pt; 2] {
            bbox_of(&self.pts)
        }

        /// True when every corner turns the same way and the outline goes round once.
        pub fn is_convex(&self) -> bool {
            let n = self.pts.len();
            if n < 3 {
                return false;
            }
            let mut sign = 0.0;
            let mut total = 0.0;
            for i in 0..n {
                let d1 = sub(self.pts[i], self.pts[(i + n - 1) % n]);
                let d2 = sub(self.pts[(i + 1) % n], self.pts[i]);
                let c = cross(d1, d2);
                total += atan2(c, dot(d1, d2));
                if c.abs() > 1e-12 * len(d1) * len(d2) {
                    if sign == 0.0 {
                        sign = c.signum();
                    } else if c.signum() != sign {
                        return false;
                    }
                }
            }
            (total.abs() - 2.0 * PI).abs() < 1e-6
        }

        /// True when `p` is inside or on the outline (non-zero winding).
        pub fn contains(&self, p: Pt) -> bool {
            on_boundary(&self.pts, p) || winding(&self.pts, p) != 0
        }

        /// The same outline the other way round.
        pub fn reversed(&self) -> Poly {
            let mut pts = self.pts.clone();
            pts.reverse();
            Poly { pts }
        }

        /// Moved by `[dx, dy]`.
        pub fn translate(&self, dx: f64, dy: f64) -> Poly {
            Poly { pts: self.pts.iter().map(|p| [p[0] + dx, p[1] + dy]).collect() }
        }

        /// Turned by `deg` (counter-clockwise) about `about`.
        pub fn rotate(&self, deg: f64, about: Pt) -> Poly {
            Poly { pts: self.pts.iter().map(|p| add(about, turn(sub(*p, about), deg))).collect() }
        }

        /// Without points closer than `tol` to the one before, without points within `tol`
        /// of the line through their neighbours, and empty when the outline is a sliver
        /// (thinner than `tol` on average: 2 × area / perimeter < `tol`).
        pub fn clean(&self, tol: f64) -> Poly {
            let mut pts: Vec<Pt> = Vec::new();
            for p in self.pts.iter() {
                if let Some(q) = pts.last() {
                    if dist(*p, *q) <= tol {
                        continue;
                    }
                }
                pts.push(*p);
            }
            while pts.len() > 1 && dist(pts[0], pts[pts.len() - 1]) <= tol {
                pts.pop();
            }
            let mut changed = true;
            while changed && pts.len() >= 3 {
                changed = false;
                let mut i = 0;
                while i < pts.len() && pts.len() >= 3 {
                    let n = pts.len();
                    let a = pts[(i + n - 1) % n];
                    let b = pts[i];
                    let c = pts[(i + 1) % n];
                    let ac = len(sub(c, a));
                    let off = if ac <= tol { 0.0 } else { cross(sub(c, a), sub(b, a)).abs() / ac };
                    if off <= tol {
                        pts.remove(i);
                        changed = true;
                    } else {
                        i += 1;
                    }
                }
            }
            let out = Poly { pts };
            if out.pts.len() < 3 || 2.0 * out.area().abs() < tol * out.perimeter() {
                return Poly::default();
            }
            out
        }

        /// This outline as a region (a self-crossing outline keeps what it winds round).
        pub fn region(&self) -> Region {
            Region::from_poly(self)
        }

        /// Every corner rounded with radius `r`, shrunk where an edge is too short (see `round_corners_with`).
        pub fn round_corners(&self, r: f64) -> Profile {
            self.round_corners_with(r, 1.0, 0.0)
        }

        /// Every corner rounded with radius `r`. A corner may use at most `max_share` of each
        /// edge beside it, and two corners never use more than the whole edge between them;
        /// the radius shrinks to fit (`Corner::Reduced`). A corner whose radius would be under
        /// `min_r`, or a straight one, stays sharp.
        pub fn round_corners_with(&self, r: f64, max_share: f64, min_r: f64) -> Profile {
            let radii = vec![r; self.pts.len()];
            let (path, corners) = corner_path(self, &radii, max_share, min_r, false);
            Profile { paths: vec![path], corners: vec![corners] }
        }

        /// Every corner cut back `d` mm along both edges, shortened where an edge is too short.
        pub fn chamfer_corners(&self, d: f64) -> Profile {
            let radii = vec![d; self.pts.len()];
            let (path, corners) = corner_path(self, &radii, 1.0, 0.0, true);
            Profile { paths: vec![path], corners: vec![corners] }
        }

        /// One loop for `extrude.loops`: `{start, segments: [{kind:"line", to}, ...]}`.
        pub fn to_loop(&self) -> Json {
            let n = self.pts.len();
            if n == 0 {
                return Json::obj().set("start", json_pt([0.0, 0.0])).set("segments", Vec::<Json>::new());
            }
            let mut segs: Vec<Json> = Vec::new();
            for k in 1..=n {
                segs.push(Json::obj().set("kind", "line").set("to", json_pt(self.pts[k % n])));
            }
            Json::obj().set("start", json_pt(self.pts[0])).set("segments", segs)
        }
    }

    /// A `w` × `h` rectangle centred on the origin, counter-clockwise from the corner at (−w/2, −h/2).
    pub fn rect(w: f64, h: f64) -> Poly {
        let (x, y) = (w / 2.0, h / 2.0);
        Poly { pts: vec![[-x, -y], [x, -y], [x, y], [-x, y]] }
    }

    /// `rect(w, h)` with every corner rounded with radius `r`.
    pub fn rounded_rect(w: f64, h: f64, r: f64) -> Profile {
        rect(w, h).round_corners(r)
    }

    /// A circle of radius `r` around the origin as `segments` straight edges, the first point on +X.
    pub fn circle(r: f64, segments: u32) -> Poly {
        let n = segments.max(3);
        Poly { pts: (0..n).map(|k| { let (s, c) = sin_cos_deg(360.0 * k as f64 / n as f64); [r * c, r * s] }).collect() }
    }

    /// How big a regular polygon is.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Size {
        /// The length of one side.
        Side(f64),
        /// Twice the distance from the centre to a side (across the flats, for an even count).
        Across(f64),
        /// Twice the distance from the centre to a corner (across the corners).
        Corners(f64),
    }

    /// A regular polygon around `centre`, its first corner at `rot_deg` from +X, counter-clockwise.
    pub fn regular(sides: u32, size: Size, centre: Pt, rot_deg: f64) -> Poly {
        let n = sides.max(3);
        let (sin_half, cos_half) = sin_cos_deg(180.0 / n as f64);
        let r = match size {
            Size::Side(s) => {
                if n == 6 { s } else { s / (2.0 * sin_half) }
            }
            Size::Across(a) => a / 2.0 / cos_half,
            Size::Corners(c) => c / 2.0,
        };
        let step = 360.0 / n as f64;
        let pts = (0..n)
            .map(|k| {
                let (s, c) = sin_cos_deg(rot_deg + step * k as f64);
                [centre[0] + r * c, centre[1] + r * s]
            })
            .collect();
        Poly { pts }
    }

    /// One piece of a `Path`: a line to a point, or an arc through a point to a point.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Seg {
        /// `Line(to)`.
        Line(Pt),
        /// `Arc(through, to)`.
        Arc(Pt, Pt),
    }

    /// A closed loop of lines and arcs from `start` back to `start`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Path {
        /// Where the loop starts and ends.
        pub start: Pt,
        /// The pieces in order, the last ending at `start`.
        pub segs: Vec<Seg>,
    }

    /// Centre, radius, start angle and signed sweep of the arc from `p0` through `pm` to `p1`.
    fn arc_of(p0: Pt, pm: Pt, p1: Pt) -> Option<(Pt, f64, f64, f64)> {
        let b = sub(pm, p0);
        let c = sub(p1, p0);
        let dd = 2.0 * cross(b, c);
        if dd.abs() < 1e-15 {
            return None;
        }
        let u = [(c[1] * dot(b, b) - b[1] * dot(c, c)) / dd, (b[0] * dot(c, c) - c[0] * dot(b, b)) / dd];
        let centre = add(p0, u);
        let r = len(u);
        let a0 = atan2(p0[1] - centre[1], p0[0] - centre[0]);
        let a1 = atan2(p1[1] - centre[1], p1[0] - centre[0]);
        let tau = 2.0 * PI;
        let wrap = |x: f64| x - (x / tau).floor() * tau;
        let sweep = if dd > 0.0 { wrap(a1 - a0) } else { -wrap(a0 - a1) };
        Some((centre, r, a0, sweep))
    }

    impl Path {
        /// The loop for `extrude.loops`: `{start, segments}` with `line` and `arc` segments.
        pub fn to_loop(&self) -> Json {
            let mut segs: Vec<Json> = Vec::new();
            for s in self.segs.iter() {
                match s {
                    Seg::Line(to) => segs.push(Json::obj().set("kind", "line").set("to", json_pt(*to))),
                    Seg::Arc(through, to) => {
                        segs.push(Json::obj().set("kind", "arc").set("through", json_pt(*through)).set("to", json_pt(*to)))
                    }
                }
            }
            Json::obj().set("start", json_pt(self.start)).set("segments", segs)
        }

        /// The loop as straight edges: each arc split so no chord strays more than `tol` mm from it.
        pub fn to_poly(&self, tol: f64) -> Poly {
            let mut pts = vec![self.start];
            let mut at = self.start;
            for s in self.segs.iter() {
                match *s {
                    Seg::Line(to) => {
                        pts.push(to);
                        at = to;
                    }
                    Seg::Arc(through, to) => {
                        if let Some((c, r, a0, sweep)) = arc_of(at, through, to) {
                            let step = if tol > 0.0 && tol < r {
                                let z = (tol / (2.0 * r)).sqrt();
                                4.0 * atan2(z, (1.0 - z * z).sqrt())
                            } else {
                                PI / 2.0
                            };
                            let n = ((sweep.abs() / step).ceil() as usize).max(1);
                            for k in 1..n {
                                let a = a0 + sweep * k as f64 / n as f64;
                                let (sa, ca) = sin_cos(a);
                                pts.push([c[0] + r * ca, c[1] + r * sa]);
                            }
                        }
                        pts.push(to);
                        at = to;
                    }
                }
            }
            Poly::new(pts)
        }

        /// Signed area in mm², arcs counted exactly.
        pub fn area(&self) -> f64 {
            let mut s = 0.0;
            let mut at = self.start;
            for seg in self.segs.iter() {
                let to = match *seg {
                    Seg::Line(to) => to,
                    Seg::Arc(through, to) => {
                        if let Some((_, r, _, sweep)) = arc_of(at, through, to) {
                            s += r * r * (sweep - sin_cos(sweep).0);
                        }
                        to
                    }
                };
                s += cross(at, to);
                at = to;
            }
            s * 0.5
        }
    }

    /// What became of one corner when rounding or chamfering.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Corner {
        /// The size asked for.
        Full,
        /// A smaller size (the radius or the cut-back that fitted).
        Reduced(f64),
        /// Left sharp: not asked for, straight, or the fitted size was under the minimum.
        Sharp,
    }

    /// Loops of lines and arcs, outlines counter-clockwise and holes clockwise, each outline
    /// followed by its holes; `corners` reports each loop's corners in point order.
    #[derive(Clone, Debug, PartialEq, Default)]
    pub struct Profile {
        /// The loops.
        pub paths: Vec<Path>,
        /// What became of each corner, one list per loop.
        pub corners: Vec<Vec<Corner>>,
    }

    impl Profile {
        /// The first loop, for an `extrude` of one outline with no holes.
        pub fn to_loop(&self) -> Json {
            match self.paths.first() {
                Some(p) => p.to_loop(),
                None => Poly::default().to_loop(),
            }
        }

        /// One `loops` array per separate piece: its outline first, then its holes.
        pub fn to_loops(&self) -> Vec<Json> {
            let mut out: Vec<Json> = Vec::new();
            let mut cur: Vec<Json> = Vec::new();
            for p in self.paths.iter() {
                if p.area() > 0.0 && !cur.is_empty() {
                    out.push(Json::Arr(cur));
                    cur = Vec::new();
                }
                cur.push(p.to_loop());
            }
            if !cur.is_empty() {
                out.push(Json::Arr(cur));
            }
            out
        }

        /// The area in mm², arcs counted exactly.
        pub fn area(&self) -> f64 {
            self.paths.iter().map(|p| p.area()).fold(0.0, |s, a| s + a)
        }

        /// As a region of straight edges, arcs split to `tol` mm, for more booleans.
        pub fn to_region(&self, tol: f64) -> Region {
            let polys: Vec<Poly> = self.paths.iter().map(|p| p.to_poly(tol)).collect();
            resolve(&polys, &[], Fill::Positive)
        }

        /// How many corners came out `[full, reduced, sharp]`.
        pub fn counts(&self) -> [usize; 3] {
            let mut c = [0, 0, 0];
            for loop_corners in self.corners.iter() {
                for k in loop_corners.iter() {
                    match k {
                        Corner::Full => c[0] += 1,
                        Corner::Reduced(_) => c[1] += 1,
                        Corner::Sharp => c[2] += 1,
                    }
                }
            }
            c
        }
    }

    /// Rounds (or chamfers) each corner `i` of `poly` with `radii[i]` (0 leaves it sharp).
    fn corner_path(poly: &Poly, radii: &[f64], max_share: f64, min_r: f64, chamfer: bool) -> (Path, Vec<Corner>) {
        let n = poly.pts.len();
        if n < 3 {
            let start = poly.pts.first().copied().unwrap_or([0.0, 0.0]);
            let segs = poly.pts.iter().skip(1).map(|p| Seg::Line(*p)).collect();
            return (Path { start, segs }, vec![Corner::Sharp; n]);
        }
        let mut rr: Vec<f64> = vec![0.0; n];
        let mut tl: Vec<f64> = vec![0.0; n];
        let mut tan_half: Vec<f64> = vec![0.0; n];
        let mut bis: Vec<Pt> = vec![[0.0, 0.0]; n];
        let mut sin_half: Vec<f64> = vec![0.0; n];
        for i in 0..n {
            let v = poly.pts[i];
            let pv = poly.pts[(i + n - 1) % n];
            let nx = poly.pts[(i + 1) % n];
            let e1 = len(sub(pv, v));
            let e2 = len(sub(nx, v));
            let u1 = norm(sub(pv, v));
            let u2 = norm(sub(nx, v));
            let c = clamp(dot(u1, u2), -1.0, 1.0);
            let s = cross(u1, u2).abs();
            bis[i] = norm(add(u1, u2));
            let r = radii[i];
            if r <= 0.0 || s < 1e-9 {
                continue;
            }
            if c >= 0.0 {
                tan_half[i] = s / (1.0 + c);
                sin_half[i] = s / (2.0 * ((1.0 + c) / 2.0).sqrt());
            } else {
                tan_half[i] = (1.0 - c) / s;
                sin_half[i] = ((1.0 - c) / 2.0).sqrt();
            }
            let room = max_share * e1.min(e2);
            if chamfer {
                rr[i] = r.min(room);
                tl[i] = rr[i];
            } else {
                rr[i] = r.min(room * tan_half[i]);
                tl[i] = rr[i] / tan_half[i];
            }
        }
        let mut fit = vec![1.0; n];
        for i in 0..n {
            let j = (i + 1) % n;
            let l = len(sub(poly.pts[j], poly.pts[i]));
            let need = tl[i] + tl[j];
            if need > l && need > 0.0 {
                let f = l / need;
                fit[i] = f64::min(fit[i], f);
                fit[j] = f64::min(fit[j], f);
            }
        }
        let mut ins: Vec<Pt> = Vec::new();
        let mut outs: Vec<Pt> = Vec::new();
        let mut mids: Vec<Option<Pt>> = Vec::new();
        let mut corners: Vec<Corner> = Vec::new();
        for i in 0..n {
            let v = poly.pts[i];
            if fit[i] < 1.0 {
                rr[i] *= fit[i];
                tl[i] *= fit[i];
            }
            if tl[i] <= 0.0 || rr[i] < min_r || rr[i] < 1e-9 {
                ins.push(v);
                outs.push(v);
                mids.push(None);
                corners.push(Corner::Sharp);
                continue;
            }
            let u1 = norm(sub(poly.pts[(i + n - 1) % n], v));
            let u2 = norm(sub(poly.pts[(i + 1) % n], v));
            let d = if chamfer { rr[i] } else { rr[i] / tan_half[i] };
            ins.push(add(v, mul(u1, d)));
            outs.push(add(v, mul(u2, d)));
            if chamfer {
                mids.push(None);
            } else {
                mids.push(Some(add(v, mul(bis[i], rr[i] / sin_half[i] - rr[i]))));
            }
            corners.push(if rr[i] >= radii[i] * (1.0 - 1e-9) { Corner::Full } else { Corner::Reduced(rr[i]) });
        }
        let mut segs: Vec<Seg> = Vec::new();
        for k in 1..=n {
            let i = k % n;
            segs.push(Seg::Line(ins[i]));
            match mids[i] {
                Some(m) => segs.push(Seg::Arc(m, outs[i])),
                None => {
                    if dist(ins[i], outs[i]) > 0.0 {
                        segs.push(Seg::Line(outs[i]));
                    }
                }
            }
        }
        (Path { start: outs[0], segs }, corners)
    }

    /// A 2D area: outlines with holes. Outlines run counter-clockwise, holes clockwise, and
    /// each outline comes before its holes. Build one with `from_poly`, `with_holes`,
    /// `from_loops`, or a boolean.
    #[derive(Clone, Debug, PartialEq, Default)]
    pub struct Region {
        loops: Vec<Poly>,
    }

    impl From<Poly> for Region {
        fn from(p: Poly) -> Region {
            Region::from_poly(&p)
        }
    }

    impl Region {
        pub(crate) fn from_native_loops(loops: Vec<Poly>) -> Region { Region { loops } }
        /// No area.
        pub fn empty() -> Region {
            Region { loops: Vec::new() }
        }

        /// The area an outline winds round, in either direction.
        pub fn from_poly(p: &Poly) -> Region {
            let p = if p.area() < 0.0 { p.reversed() } else { p.clone() };
            resolve(&[p], &[], Fill::NonZero)
        }

        /// `outer` with `holes` taken out (either direction for each).
        pub fn with_holes(outer: &Poly, holes: &[Poly]) -> Region {
            let mut loops = vec![if outer.area() < 0.0 { outer.reversed() } else { outer.clone() }];
            for h in holes.iter() {
                loops.push(if h.area() > 0.0 { h.reversed() } else { h.clone() });
            }
            resolve(&loops, &[], Fill::Positive)
        }

        /// Loops nested any way, in any direction: inside an odd number of them is material.
        pub fn from_loops(loops: &[Poly]) -> Region {
            resolve(loops, &[], Fill::EvenOdd)
        }

        /// The loops: each outline (counter-clockwise) followed by its holes (clockwise).
        pub fn loops(&self) -> &Vec<Poly> {
            &self.loops
        }

        /// True when there is no area.
        pub fn is_empty(&self) -> bool {
            self.loops.is_empty()
        }

        /// The separate pieces, each one outline with its holes.
        pub fn pieces(&self) -> Vec<Region> {
            let mut out: Vec<Region> = Vec::new();
            for l in self.loops.iter() {
                if l.area() > 0.0 || out.is_empty() {
                    out.push(Region { loops: vec![l.clone()] });
                } else if let Some(last) = out.last_mut() {
                    last.loops.push(l.clone());
                }
            }
            out
        }

        /// Net area in mm² (holes taken off).
        pub fn area(&self) -> f64 {
            self.loops.iter().map(|l| l.area()).fold(0.0, |s, a| s + a)
        }

        /// Length of every outline and hole in mm.
        pub fn perimeter(&self) -> f64 {
            self.loops.iter().map(|l| l.perimeter()).fold(0.0, |s, p| s + p)
        }

        /// The centre of the area.
        pub fn centroid(&self) -> Pt {
            let a = self.area();
            if a.abs() < 1e-12 {
                return self.loops.first().map(|l| l.centroid()).unwrap_or([0.0, 0.0]);
            }
            let mut c = [0.0, 0.0];
            for l in self.loops.iter() {
                c = add(c, mul(l.centroid(), l.area()));
            }
            mul(c, 1.0 / a)
        }

        /// `[min, max]` corners of the bounding box.
        pub fn bbox(&self) -> [Pt; 2] {
            let pts: Vec<Pt> = self.loops.iter().flat_map(|l| l.pts.iter().copied()).collect();
            bbox_of(&pts)
        }

        /// True when `p` is in the material or on its edge.
        pub fn contains(&self, p: Pt) -> bool {
            if self.loops.iter().any(|l| on_boundary(&l.pts, p)) {
                return true;
            }
            self.loops.iter().map(|l| winding(&l.pts, p)).sum::<i32>() != 0
        }

        /// Material in either.
        pub fn union(&self, other: &Region) -> Region {
            resolve(&self.loops, &other.loops, Fill::Union)
        }

        /// Material in any of the regions, resolved in one boolean pass.
        pub fn union_all(regions: &[Region]) -> Region {
            let loops: Vec<Poly> = regions.iter().flat_map(|region| region.loops.iter().cloned()).collect();
            resolve(&loops, &[], Fill::Positive)
        }

        /// Material in both.
        pub fn intersect(&self, other: &Region) -> Region {
            resolve(&self.loops, &other.loops, Fill::Intersect)
        }

        /// Material in this and not in `other`.
        pub fn difference(&self, other: &Region) -> Region {
            resolve(&self.loops, &other.loops, Fill::Difference)
        }

        /// Material in this region with every cutter removed, resolved in one boolean pass.
        pub fn difference_all(&self, others: &[Region]) -> Region {
            let loops: Vec<Poly> = others.iter().flat_map(|region| region.loops.iter().cloned()).collect();
            resolve(&self.loops, &loops, Fill::Difference)
        }

        /// Material in exactly one of the two.
        pub fn xor(&self, other: &Region) -> Region {
            resolve(&self.loops, &other.loops, Fill::Xor)
        }

        /// The material inside `shape`: the same as `intersect` with `shape`'s region, and much faster
        /// when `shape` is small against this region (a cell against a whole floor).
        pub fn clip(&self, shape: &Poly) -> Region {
            let b = shape.bbox();
            let lo = [b[0][0] - 1.0, b[0][1] - 1.0];
            let hi = [b[1][0] + 1.0, b[1][1] + 1.0];
            let near: Vec<Poly> = self.loops.iter().map(|l| window(l, lo, hi)).filter(|l| l.pts.len() >= 3).collect();
            let shape = if shape.area() < 0.0 { shape.reversed() } else { shape.clone() };
            resolve(&[shape], &near, Fill::Intersect)
        }

        /// Grown by `d` mm (shrunk when negative), the way a sketch offset does it:
        /// - an outside corner (convex in the offset direction) is rounded with radius |d|,
        ///   its centre on the original corner;
        /// - an inside corner stays sharp;
        /// - a corner whose edges are too short for that radius gets a smaller one
        ///   (`Corner::Reduced`), never a refusal.
        ///
        /// Outside corners closer together than 2|d| merge before they are rounded, so a narrow
        /// gap closes straight. A shape thinner than 2|d| disappears when shrunk.
        /// `offset_miter` keeps every corner sharp and stays a `Region`.
        pub fn offset(&self, d: f64) -> Profile {
            let grown = self.offset_miter(d);
            let mut out = Profile::default();
            for l in grown.loops.iter() {
                let n = l.pts.len();
                let radii: Vec<f64> = (0..n)
                    .map(|i| {
                        let c = cross(sub(l.pts[i], l.pts[(i + n - 1) % n]), sub(l.pts[(i + 1) % n], l.pts[i]));
                        if (d > 0.0 && c > 0.0) || (d < 0.0 && c < 0.0) { d.abs() } else { 0.0 }
                    })
                    .collect();
                let (path, corners) = corner_path(l, &radii, 1.0, 0.0, false);
                out.paths.push(path);
                out.corners.push(corners);
            }
            out
        }

        /// Grown by `d` mm (shrunk when negative) with every corner sharp: an outside corner
        /// is mitred (cut square when sharper than about 2°), an inside corner stays sharp.
        pub fn offset_miter(&self, d: f64) -> Region {
            if d == 0.0 || self.loops.is_empty() {
                return self.clone();
            }
            let raw: Vec<Poly> = self.loops.iter().map(|l| offset_loop(l, d)).collect();
            resolve(&raw, &[], Fill::Positive)
        }

        /// Every loop cleaned (`Poly::clean`); loops that become slivers are dropped.
        pub fn clean(&self, tol: f64) -> Region {
            let loops: Vec<Poly> = self.loops.iter().map(|l| l.clean(tol)).filter(|l| l.pts.len() >= 3).collect();
            resolve(&loops, &[], Fill::Positive)
        }

        /// Without the pieces whose area is under `min_area` mm².
        pub fn without_small(&self, min_area: f64) -> Region {
            let mut loops: Vec<Poly> = Vec::new();
            for p in self.pieces() {
                if p.area() >= min_area {
                    loops.extend(p.loops);
                }
            }
            Region { loops }
        }

        /// Every corner rounded with radius `r` (see `Poly::round_corners_with`).
        pub fn round_corners(&self, r: f64) -> Profile {
            self.round_corners_with(r, 1.0, 0.0)
        }

        /// Every corner rounded; the radius shrinks to fit as in `Poly::round_corners_with`.
        pub fn round_corners_with(&self, r: f64, max_share: f64, min_r: f64) -> Profile {
            let mut out = Profile::default();
            for l in self.loops.iter() {
                let p = l.round_corners_with(r, max_share, min_r);
                out.paths.extend(p.paths);
                out.corners.extend(p.corners);
            }
            out
        }

        /// Every corner cut back `d` mm along both edges.
        pub fn chamfer_corners(&self, d: f64) -> Profile {
            let mut out = Profile::default();
            for l in self.loops.iter() {
                let p = l.chamfer_corners(d);
                out.paths.extend(p.paths);
                out.corners.extend(p.corners);
            }
            out
        }

        /// Moved by `[dx, dy]`.
        pub fn translate(&self, dx: f64, dy: f64) -> Region {
            Region { loops: self.loops.iter().map(|l| l.translate(dx, dy)).collect() }
        }

        /// Turned by `deg` (counter-clockwise) about `about`.
        pub fn rotate(&self, deg: f64, about: Pt) -> Region {
            Region { loops: self.loops.iter().map(|l| l.rotate(deg, about)).collect() }
        }

        /// One `loops` array per separate piece: its outline first, then its holes.
        pub fn to_loops(&self) -> Vec<Json> {
            self.pieces().iter().map(|p| Json::Arr(p.loops.iter().map(|l| l.to_loop()).collect())).collect()
        }
    }

    /// One loop cut to the box `lo`..`hi` edge by edge (Sutherland–Hodgman). The result may run along the
    /// box's sides there and back, but it winds round every point inside the box as the loop does.
    fn window(l: &Poly, lo: Pt, hi: Pt) -> Poly {
        let mut pts = l.pts.clone();
        for side in 0..4 {
            let axis = side % 2;
            let bound = if side < 2 { lo[axis] } else { hi[axis] };
            let inside = |p: Pt| if side < 2 { p[axis] >= bound } else { p[axis] <= bound };
            let n = pts.len();
            let mut out: Vec<Pt> = Vec::new();
            for i in 0..n {
                let cur = pts[i];
                let prev = pts[(i + n - 1) % n];
                if inside(cur) != inside(prev) {
                    let t = (bound - prev[axis]) / (cur[axis] - prev[axis]);
                    let mut x = add(prev, mul(sub(cur, prev), t));
                    x[axis] = bound;
                    out.push(x);
                }
                if inside(cur) {
                    out.push(cur);
                }
            }
            pts = out;
            if pts.is_empty() {
                break;
            }
        }
        Poly { pts }
    }

    /// The raw offset of one loop: mitred outside corners, inside corners through the
    /// original point (the backward bits wind the other way and drop out in `resolve`).
    fn offset_loop(p: &Poly, d: f64) -> Poly {
        let n = p.pts.len();
        let mut out: Vec<Pt> = Vec::new();
        for i in 0..n {
            let v = p.pts[i];
            let d1 = norm(sub(v, p.pts[(i + n - 1) % n]));
            let d2 = norm(sub(p.pts[(i + 1) % n], v));
            let n1 = [d1[1], -d1[0]];
            let n2 = [d2[1], -d2[0]];
            let c = cross(d1, d2);
            if c.abs() < 1e-12 && dot(d1, d2) > 0.0 {
                out.push(add(v, mul(n1, d)));
            } else if c * d > 0.0 {
                let k = 1.0 + dot(n1, n2);
                if k > 1e-3 {
                    out.push(add(v, mul(add(n1, n2), d / k)));
                } else {
                    out.push(add(add(v, mul(n1, d)), mul(d1, d.abs())));
                    out.push(sub(add(v, mul(n2, d)), mul(d2, d.abs())));
                }
            } else {
                out.push(add(v, mul(n1, d)));
                out.push(v);
                out.push(add(v, mul(n2, d)));
            }
        }
        Poly { pts: out }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Fill {
        Union,
        Intersect,
        Difference,
        Xor,
        NonZero,
        Positive,
        EvenOdd,
    }

    fn filled(f: Fill, a: i32, b: i32) -> bool {
        match f {
            Fill::Union => a != 0 || b != 0,
            Fill::Intersect => a != 0 && b != 0,
            Fill::Difference => a != 0 && b == 0,
            Fill::Xor => (a != 0) != (b != 0),
            Fill::NonZero => a != 0,
            Fill::Positive => a > 0,
            Fill::EvenOdd => a % 2 != 0,
        }
    }

    struct Edge {
        p: Pt,
        q: Pt,
        side: usize,
    }

    struct Group {
        u: usize,
        v: usize,
        net: [i32; 2],
        edges: Vec<usize>,
    }

    /// Points within `SNAP` of each other share one index.
    struct Pool {
        pts: Vec<Pt>,
        grid: HashMap<(i64, i64), Vec<usize>>,
    }

    impl Pool {
        fn id(&mut self, p: Pt) -> usize {
            let kx = (p[0] / SNAP).floor() as i64;
            let ky = (p[1] / SNAP).floor() as i64;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(list) = self.grid.get(&(kx + dx, ky + dy)) {
                        for &i in list.iter() {
                            if dist(self.pts[i], p) <= SNAP {
                                return i;
                            }
                        }
                    }
                }
            }
            let id = self.pts.len();
            self.pts.push(p);
            self.grid.entry((kx, ky)).or_default().push(id);
            id
        }
    }

    pub(crate) fn loops_json(loops: &[Poly]) -> Json {
        Json::arr(loops.iter().map(|p| Json::arr(p.pts.iter().map(|q| Json::arr(vec![q[0], q[1]])).collect::<Vec<Json>>())).collect::<Vec<Json>>())
    }

    pub(crate) fn loops_from_json(value: &Json) -> Option<Vec<Poly>> {
        let rows = value.as_array()?;
        if rows.len() > 20000 { return None; }
        let mut loops = Vec::new();
        let mut count = 0usize;
        for row in rows {
            let pts = row.as_points2()?;
            count += pts.len();
            if count > 100000 || pts.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) { return None; }
            loops.push(Poly { pts });
        }
        Some(loops)
    }

    pub(crate) fn native_resolve(args: &Json) -> Option<Json> {
        let a = loops_from_json(args.get("a")?)?;
        let b = loops_from_json(args.get("b")?)?;
        if a.iter().chain(b.iter()).map(|p| p.pts.len()).sum::<usize>() > 5000 { return None; }
        let fill = match args.get("fill")?.as_str()? {
            "union" => Fill::Union, "intersect" => Fill::Intersect, "difference" => Fill::Difference,
            "xor" => Fill::Xor, "nonzero" => Fill::NonZero, "positive" => Fill::Positive,
            "evenodd" => Fill::EvenOdd, _ => return None,
        };
        Some(loops_json(&resolve_local(&a, &b, fill).loops))
    }

    /// The boolean of two loop sets under a fill rule: every edge is split where it meets
    /// another, each piece is kept where the rule fills one side and not the other, and the
    /// kept pieces are joined into loops with material on their left.
    fn resolve(a: &[Poly], b: &[Poly], fill: Fill) -> Region {
        if !super::NATIVE_GEOMETRY.load(super::Ordering::Relaxed) || a.iter().chain(b.iter()).map(|p| p.pts.len()).sum::<usize>() < 40 {
            return resolve_local(a, b, fill);
        }
        let kind = match fill { Fill::Union => "union", Fill::Intersect => "intersect", Fill::Difference => "difference", Fill::Xor => "xor", Fill::NonZero => "nonzero", Fill::Positive => "positive", Fill::EvenOdd => "evenodd" };
        let args = Json::obj().set("op", "resolve").set("fill", kind).set("a", loops_json(a)).set("b", loops_json(b));
        if let Some(reply) = super::native_geometry(args) {
            if let Some(loops) = loops_from_json(&reply) { return Region { loops }; }
        }
        resolve_local(a, b, fill)
    }

    fn resolve_local(a: &[Poly], b: &[Poly], fill: Fill) -> Region {
        let mut edges: Vec<Edge> = Vec::new();
        for (side, set) in [a, b].iter().enumerate() {
            for l in set.iter() {
                let n = l.pts.len();
                if n < 3 {
                    continue;
                }
                for i in 0..n {
                    let p = l.pts[i];
                    let q = l.pts[(i + 1) % n];
                    if dist(p, q) > SNAP {
                        edges.push(Edge { p, q, side });
                    }
                }
            }
        }
        let n = edges.len();
        let boxes: Vec<[Pt; 2]> = edges.iter().map(|e| bbox_of(&[e.p, e.q])).collect();
        let mut cuts: Vec<Vec<(f64, Pt)>> = vec![Vec::new(); n];
        // Pairs in order of their boxes' left side, so the scan for each edge stops past its right side.
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|a, b| boxes[*a][0][0].partial_cmp(&boxes[*b][0][0]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b)));
        for oi in 0..n {
            let i = order[oi];
            for &j in order[oi + 1..].iter() {
                let (bi, bj) = (boxes[i], boxes[j]);
                if bj[0][0] > bi[1][0] + SNAP {
                    break;
                }
                if bi[0][0] > bj[1][0] + SNAP || bi[0][1] > bj[1][1] + SNAP || bj[0][1] > bi[1][1] + SNAP {
                    continue;
                }
                let (p, q, r, s) = (edges[i].p, edges[i].q, edges[j].p, edges[j].q);
                for x in [r, s] {
                    let (t, d) = seg_near(x, p, q);
                    if d <= SNAP && dist(x, p) > SNAP && dist(x, q) > SNAP {
                        cuts[i].push((t, x));
                    }
                }
                for x in [p, q] {
                    let (t, d) = seg_near(x, r, s);
                    if d <= SNAP && dist(x, r) > SNAP && dist(x, s) > SNAP {
                        cuts[j].push((t, x));
                    }
                }
                let d1 = sub(q, p);
                let d2 = sub(s, r);
                let den = cross(d1, d2);
                if den.abs() > 1e-12 * len(d1) * len(d2) {
                    let t = cross(sub(r, p), d2) / den;
                    let u = cross(sub(r, p), d1) / den;
                    if t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0 {
                        let x = add(p, mul(d1, t));
                        if dist(x, p) > SNAP && dist(x, q) > SNAP && dist(x, r) > SNAP && dist(x, s) > SNAP {
                            cuts[i].push((t, x));
                            cuts[j].push((u, x));
                        }
                    }
                }
            }
        }
        let mut pool = Pool { pts: Vec::new(), grid: HashMap::new() };
        let mut groups: Vec<Group> = Vec::new();
        let mut group_of: HashMap<(usize, usize), usize> = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            let mut c = cuts[i].clone();
            c.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut ids = vec![pool.id(e.p)];
            for (_, x) in c.iter() {
                ids.push(pool.id(*x));
            }
            ids.push(pool.id(e.q));
            for k in 0..ids.len() - 1 {
                let (u, v) = (ids[k], ids[k + 1]);
                if u == v {
                    continue;
                }
                let key = (u.min(v), u.max(v));
                let g = match group_of.get(&key) {
                    Some(g) => *g,
                    None => {
                        groups.push(Group { u: key.0, v: key.1, net: [0, 0], edges: Vec::new() });
                        group_of.insert(key, groups.len() - 1);
                        groups.len() - 1
                    }
                };
                groups[g].net[e.side] += if u < v { 1 } else { -1 };
                groups[g].edges.push(i);
            }
        }
        let mut kept: Vec<(usize, usize)> = Vec::new();
        for g in groups.iter() {
            // Winding just left of the piece: a ray from its middle along its left normal, counting the
            // edges it crosses (half-open, so a ray through a corner counts once). Right = left - net.
            let (pu, pv) = (pool.pts[g.u], pool.pts[g.v]);
            let m = mul(add(pu, pv), 0.5);
            let d = norm(sub(pv, pu));
            let n = [-d[1], d[0]];
            let mut left = [0i32; 2];
            for (k, e) in edges.iter().enumerate() {
                if g.edges.contains(&k) {
                    continue;
                }
                let sa = cross(n, sub(e.p, m));
                let sb = cross(n, sub(e.q, m));
                let up = sa <= 0.0 && sb > 0.0;
                let down = sb <= 0.0 && sa > 0.0;
                if !up && !down {
                    continue;
                }
                let ta = dot(n, sub(e.p, m));
                let tb = dot(n, sub(e.q, m));
                if ta + (tb - ta) * (-sa / (sb - sa)) > 0.0 {
                    left[e.side] += if up { 1 } else { -1 };
                }
            }
            let right = [left[0] - g.net[0], left[1] - g.net[1]];
            let l = filled(fill, left[0], left[1]);
            let r = filled(fill, right[0], right[1]);
            if l && !r {
                kept.push((g.u, g.v));
            } else if r && !l {
                kept.push((g.v, g.u));
            }
        }
        let mut from: Vec<Vec<usize>> = vec![Vec::new(); pool.pts.len()];
        for (k, e) in kept.iter().enumerate() {
            from[e.0].push(k);
        }
        let mut used = vec![false; kept.len()];
        let mut cycles: Vec<Vec<usize>> = Vec::new();
        // Where each point sits on the walk being followed, so a walk that comes back to one of its
        // points splits that cycle off: every loop comes out simple, touching others only at points.
        let mut on_walk: Vec<usize> = vec![usize::MAX; pool.pts.len()];
        for k0 in 0..kept.len() {
            if used[k0] {
                continue;
            }
            used[k0] = true;
            let start = kept[k0].0;
            let mut ids = vec![start];
            on_walk[start] = 0;
            let mut k = k0;
            loop {
                let cur = kept[k].1;
                if on_walk[cur] != usize::MAX {
                    let j = on_walk[cur];
                    let cycle: Vec<usize> = ids[j..].to_vec();
                    for &i in cycle.iter() {
                        on_walk[i] = usize::MAX;
                    }
                    cycles.push(cycle);
                    ids.truncate(j);
                    if ids.is_empty() {
                        break;
                    }
                    on_walk[cur] = usize::MAX;
                }
                on_walk[cur] = ids.len();
                ids.push(cur);
                let din = sub(pool.pts[cur], pool.pts[kept[k].0]);
                let mut best: Option<usize> = None;
                let mut best_turn = -10.0;
                for &c in from[cur].iter() {
                    if used[c] {
                        continue;
                    }
                    let dout = sub(pool.pts[kept[c].1], pool.pts[cur]);
                    let t = if kept[c].1 == kept[k].0 { -5.0 } else { atan2(cross(din, dout), dot(din, dout)) };
                    if t > best_turn {
                        best_turn = t;
                        best = Some(c);
                    }
                }
                match best {
                    Some(c) => {
                        used[c] = true;
                        k = c;
                    }
                    None => break,
                }
            }
            for &i in ids.iter() {
                on_walk[i] = usize::MAX;
            }
        }
        let mut found: Vec<Poly> = Vec::new();
        for ids in cycles.iter() {
            let mut pts: Vec<Pt> = ids.iter().map(|&i| pool.pts[i]).collect();
            let mut changed = true;
            while changed && pts.len() >= 3 {
                changed = false;
                let mut i = 0;
                while i < pts.len() && pts.len() >= 3 {
                    let m = pts.len();
                    let a = pts[(i + m - 1) % m];
                    let c = pts[(i + 1) % m];
                    let (_, d) = seg_near(pts[i], a, c);
                    if d <= SNAP {
                        pts.remove(i);
                        changed = true;
                    } else {
                        i += 1;
                    }
                }
            }
            let p = Poly { pts };
            if p.pts.len() >= 3 && p.area().abs() > SNAP * SNAP {
                found.push(p);
            }
        }
        let mut outers: Vec<Poly> = Vec::new();
        let mut holes: Vec<Poly> = Vec::new();
        for p in found {
            if p.area() > 0.0 {
                outers.push(p);
            } else {
                holes.push(p);
            }
        }
        let mut owned: Vec<Vec<Poly>> = vec![Vec::new(); outers.len()];
        for h in holes {
            let mut best: Option<usize> = None;
            for (i, o) in outers.iter().enumerate() {
                if o.area() < -h.area() || !h.pts.iter().all(|p| o.contains(*p)) {
                    continue;
                }
                let smaller = match best {
                    Some(b) => o.area() < outers[b].area(),
                    None => true,
                };
                if smaller {
                    best = Some(i);
                }
            }
            if let Some(b) = best {
                owned[b].push(h);
            }
        }
        let mut loops: Vec<Poly> = Vec::new();
        for (o, hs) in outers.into_iter().zip(owned) {
            loops.push(o);
            loops.extend(hs);
        }
        Region { loops }
    }
}

/// Cell patterns clipped to a region: hexagons, squares, bricks, triangles, or cells
/// along a line. Each cell comes back with its centre, its piece of the region, and
/// whether the region's edge cut it.
pub mod lattice {
    use super::geom2d::{self, Poly, Pt, Region, Size};

    /// Which way a hexagon points.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Orient {
        /// A corner at the top; rows run along X, odd rows shifted half a cell.
        Pointy,
        /// A flat side at the top: the `Pointy` grid turned 90°.
        Flat,
    }

    /// One cell of a pattern.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Cell {
        /// `[column, row]` in the grid (`[k, 0]` along a line).
        pub index: [i32; 2],
        /// The centre of the whole cell, before clipping.
        pub centre: Pt,
        /// The cell's material inside the region (a clipped cell can be more than one piece).
        pub region: Region,
        /// True when the region's edge cut the cell.
        pub clipped: bool,
    }

    impl Cell {
        /// The cell's first outline (the whole cell when it has one piece and no hole).
        pub fn poly(&self) -> Poly {
            self.region.loops().first().cloned().unwrap_or_default()
        }

        /// The cell's area in mm².
        pub fn area(&self) -> f64 {
            self.region.area()
        }
    }

    /// A pattern of cells: lattice points and the cell shape around each.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Grid {
        pitch: Pt,
        shift: f64,
        shapes: Vec<Poly>,
        centres: Vec<Pt>,
        points: Vec<Pt>,
        origin: Pt,
        turn_deg: f64,
        rib: f64,
    }

    fn lattice_grid(pitch: Pt, shift: f64, shapes: Vec<Poly>, centres: Vec<Pt>) -> Grid {
        Grid { pitch, shift, shapes, centres, points: Vec::new(), origin: [0.0, 0.0], turn_deg: 0.0, rib: 0.0 }
    }

    /// Hexagons of side `side` that tile the plane (pitch √3·side across, 1.5·side between rows).
    pub fn hex(side: f64, orient: Orient) -> Grid {
        let shape = geom2d::regular(6, Size::Side(side), [0.0, 0.0], 90.0);
        let px = 3f64.sqrt() * side;
        let g = lattice_grid([px, 1.5 * side], px / 2.0, vec![shape], vec![[0.0, 0.0]]);
        match orient {
            Orient::Pointy => g,
            Orient::Flat => g.turned(90.0),
        }
    }

    /// `pitch_u` × `pitch_v` rectangles in straight rows and columns.
    pub fn rect(pitch_u: f64, pitch_v: f64) -> Grid {
        lattice_grid([pitch_u, pitch_v], 0.0, vec![geom2d::rect(pitch_u, pitch_v)], vec![[0.0, 0.0]])
    }

    /// `pitch_u` × `pitch_v` rectangles with every odd row shifted `shift` along X (bricks: `pitch_u / 2`).
    pub fn staggered(pitch_u: f64, pitch_v: f64, shift: f64) -> Grid {
        lattice_grid([pitch_u, pitch_v], shift, vec![geom2d::rect(pitch_u, pitch_v)], vec![[0.0, 0.0]])
    }

    /// Equilateral triangles of side `side`, pointing up and down in turn.
    pub fn tri(side: f64) -> Grid {
        let h = side * 3f64.sqrt() / 2.0;
        let up = Poly { pts: vec![[-side / 2.0, -h / 2.0], [side / 2.0, -h / 2.0], [0.0, h / 2.0]] };
        let down = Poly { pts: vec![[side / 2.0, h / 2.0], [-side / 2.0, h / 2.0], [0.0, -h / 2.0]] };
        lattice_grid([side / 2.0, h], 0.0, vec![up, down], vec![[0.0, -h / 6.0], [0.0, h / 6.0]])
    }

    /// `count` copies of `cell` (drawn around the origin) spaced evenly along the open line
    /// `path`, the first at its start and the last at its end (one copy sits at the middle).
    pub fn along(path: &[Pt], count: usize, cell: Poly) -> Grid {
        let mut lengths = vec![0.0];
        for i in 1..path.len() {
            let d = (path[i][0] - path[i - 1][0]).hypot(path[i][1] - path[i - 1][1]);
            lengths.push(lengths[i - 1] + d);
        }
        let total = lengths.last().copied().unwrap_or(0.0);
        let mut points: Vec<Pt> = Vec::new();
        for k in 0..count {
            let s = if count == 1 { total / 2.0 } else { total * k as f64 / (count - 1) as f64 };
            let mut i = 1;
            while i + 1 < path.len() && lengths[i] < s {
                i += 1;
            }
            if path.len() < 2 {
                points.push(path.first().copied().unwrap_or([0.0, 0.0]));
                continue;
            }
            let seg = lengths[i] - lengths[i - 1];
            let t = if seg > 0.0 { geom2d::clamp((s - lengths[i - 1]) / seg, 0.0, 1.0) } else { 0.0 };
            let (a, b) = (path[i - 1], path[i]);
            points.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
        Grid { pitch: [0.0, 0.0], shift: 0.0, shapes: vec![cell], centres: vec![[0.0, 0.0]], points, origin: [0.0, 0.0], turn_deg: 0.0, rib: 0.0 }
    }

    pub(crate) fn native_cells(args: &super::Json) -> Option<super::Json> {
        fn point(v: &super::Json) -> Option<Pt> {
            let a = v.as_f64s()?;
            if a.len() != 2 || !a[0].is_finite() || !a[1].is_finite() { return None; }
            Some([a[0], a[1]])
        }
        let grid = Grid {
            pitch: point(args.get("pitch")?)?, shift: args.get("shift")?.as_f64()?,
            shapes: geom2d::loops_from_json(args.get("shapes")?)?,
            centres: args.get("centres")?.as_points2()?, points: args.get("points")?.as_points2()?,
            origin: point(args.get("origin")?)?, turn_deg: args.get("turn")?.as_f64()?, rib: args.get("rib")?.as_f64()?,
        };
        let region = Region::from_native_loops(geom2d::loops_from_json(args.get("region")?)?);
        let area = args.get("minArea")?.as_f64()?;
        if !grid.shift.is_finite() || !grid.turn_deg.is_finite() || !grid.rib.is_finite() || !area.is_finite()
            || grid.points.len() > 10000 || grid.centres.len() > 10000
            || grid.points.iter().chain(grid.centres.iter()).any(|p| !p[0].is_finite() || !p[1].is_finite()) { return None; }
        if grid.points.is_empty() && grid.pitch[0] > 0.0 && grid.pitch[1] > 0.0 && !region.is_empty() {
            let bounds = region.bbox();
            let reach = grid.shapes.iter().flat_map(|p| p.pts.iter()).fold(0.0f64, |m, p| m.max(p[0].hypot(p[1])));
            let extent = bounds.iter().map(|p| (p[0] - grid.origin[0]).hypot(p[1] - grid.origin[1])).fold(0.0f64, f64::max) + reach + grid.shift.abs();
            let nx = 2.0 * extent / grid.pitch[0] + 5.0;
            let ny = 2.0 * extent / grid.pitch[1] + 5.0;
            if !nx.is_finite() || !ny.is_finite() || nx * ny > 10000.0 { return None; }
        }
        let cells = grid.cells_local(&region, area);
        Some(super::Json::arr(cells.iter().map(|c| super::Json::obj()
            .set("index", super::Json::arr(vec![c.index[0] as f64, c.index[1] as f64]))
            .set("centre", super::Json::arr(vec![c.centre[0], c.centre[1]]))
            .set("region", geom2d::loops_json(c.region.loops())).set("clipped", c.clipped)).collect::<Vec<_>>()))
    }

    impl Grid {
        /// Leave walls `t` mm thick between cells: each cell shrinks by `t / 2` all round.
        pub fn rib(self, t: f64) -> Grid {
            Grid { rib: t, ..self }
        }

        /// Put lattice point `[0, 0]` at `origin`.
        pub fn at(self, origin: Pt) -> Grid {
            Grid { origin, ..self }
        }

        /// Turn the whole pattern by `deg` about its origin (adds to an earlier turn).
        pub fn turned(self, deg: f64) -> Grid {
            let turn_deg = self.turn_deg + deg;
            Grid { turn_deg, ..self }
        }

        fn place(&self, p: Pt) -> Pt {
            let (s, c) = geom2d::sin_cos_deg(self.turn_deg);
            if self.turn_deg == 0.0 {
                return [self.origin[0] + p[0], self.origin[1] + p[1]];
            }
            [self.origin[0] + p[0] * c - p[1] * s, self.origin[1] + p[0] * s + p[1] * c]
        }

        /// The cells that meet `region`, row by row from the lowest, each row by column;
        /// pieces under `min_area` mm² are dropped, and a cell with nothing left is skipped.
        pub fn cells(&self, region: &Region, min_area: f64) -> Vec<Cell> {
            if let Some(cells) = self.native_cells(region, min_area) { return cells; }
            self.cells_local(region, min_area)
        }

        fn native_cells(&self, region: &Region, min_area: f64) -> Option<Vec<Cell>> {
            if !super::NATIVE_GEOMETRY.load(super::Ordering::Relaxed) { return None; }
            let point = |p: Pt| super::Json::arr(vec![p[0], p[1]]);
            let args = super::Json::obj().set("op", "cells").set("pitch", point(self.pitch)).set("shift", self.shift)
                .set("shapes", geom2d::loops_json(&self.shapes))
                .set("centres", super::Json::arr(self.centres.iter().map(|p| point(*p)).collect::<Vec<_>>()))
                .set("points", super::Json::arr(self.points.iter().map(|p| point(*p)).collect::<Vec<_>>()))
                .set("origin", point(self.origin)).set("turn", self.turn_deg).set("rib", self.rib)
                .set("region", geom2d::loops_json(region.loops())).set("minArea", min_area);
            let reply = super::native_geometry(args)?;
            let mut cells = Vec::new();
            for row in reply.as_array()? {
                let i = row.get("index")?.as_f64s()?;
                let p = row.get("centre")?.as_f64s()?;
                if i.len() != 2 || p.len() != 2 { return None; }
                cells.push(Cell { index: [i[0] as i32, i[1] as i32], centre: [p[0], p[1]],
                    region: Region::from_native_loops(geom2d::loops_from_json(row.get("region")?)?),
                    clipped: row.get("clipped")?.as_bool()? });
            }
            Some(cells)
        }

        fn cells_local(&self, region: &Region, min_area: f64) -> Vec<Cell> {
            let mut out: Vec<Cell> = Vec::new();
            if region.is_empty() || self.shapes.is_empty() {
                return out;
            }
            let shapes: Vec<Poly> = self
                .shapes
                .iter()
                .map(|s| if self.rib > 0.0 { s.region().offset_miter(-self.rib / 2.0).loops().first().cloned().unwrap_or_default() } else { s.clone() })
                .collect();
            let reach = self.shapes.iter().flat_map(|s| s.pts.iter()).fold(0.0f64, |m, p| m.max(p[0].hypot(p[1])));
            let mut sites: Vec<([i32; 2], Pt)> = Vec::new();
            if !self.points.is_empty() || self.pitch[0] <= 0.0 || self.pitch[1] <= 0.0 {
                for (k, p) in self.points.iter().enumerate() {
                    sites.push(([k as i32, 0], *p));
                }
            } else {
                let rb = region.bbox();
                let (s, c) = geom2d::sin_cos_deg(-self.turn_deg);
                let mut lo = [f64::MAX, f64::MAX];
                let mut hi = [f64::MIN, f64::MIN];
                for q in [rb[0], [rb[1][0], rb[0][1]], rb[1], [rb[0][0], rb[1][1]]] {
                    let d = [q[0] - self.origin[0], q[1] - self.origin[1]];
                    let f = [d[0] * c - d[1] * s, d[0] * s + d[1] * c];
                    lo = [lo[0].min(f[0]), lo[1].min(f[1])];
                    hi = [hi[0].max(f[0]), hi[1].max(f[1])];
                }
                let (pu, pv) = (self.pitch[0], self.pitch[1]);
                let j0 = ((lo[1] - reach) / pv).floor() as i32 - 1;
                let j1 = ((hi[1] + reach) / pv).ceil() as i32 + 1;
                let i0 = ((lo[0] - reach - self.shift.abs()) / pu).floor() as i32 - 1;
                let i1 = ((hi[0] + reach + self.shift.abs()) / pu).ceil() as i32 + 1;
                for j in j0..=j1 {
                    let shift = if j.rem_euclid(2) == 1 { self.shift } else { 0.0 };
                    for i in i0..=i1 {
                        sites.push(([i, j], [i as f64 * pu + shift, j as f64 * pv]));
                    }
                }
            }
            let rb = region.bbox();
            let mut region_boxes: Vec<[Pt; 2]> = Vec::new();
            for l in region.loops().iter() {
                let n = l.pts.len();
                for i in 0..n {
                    let (a, b) = (l.pts[i], l.pts[(i + 1) % n]);
                    region_boxes.push([[a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]]);
                }
            }
            for (index, local) in sites {
                let k = if shapes.len() > 1 { ((index[0] + index[1]).rem_euclid(shapes.len() as i32)) as usize } else { 0 };
                let shape = &shapes[k];
                let cell = Poly { pts: shape.pts.iter().map(|p| self.place([local[0] + p[0], local[1] + p[1]])).collect() };
                let cc = self.centres.get(k).copied().unwrap_or([0.0, 0.0]);
                let centre = self.place([local[0] + cc[0], local[1] + cc[1]]);
                let cb = cell.bbox();
                if cb[0][0] > rb[1][0] || cb[1][0] < rb[0][0] || cb[0][1] > rb[1][1] || cb[1][1] < rb[0][1] {
                    continue;
                }
                let touches = region_boxes.iter().any(|b| !(b[0][0] > cb[1][0] || b[1][0] < cb[0][0] || b[0][1] > cb[1][1] || b[1][1] < cb[0][1]));
                let whole = cell.area().abs();
                let piece = if touches {
                    region.clip(&cell)
                } else if region.contains(cell.centroid()) {
                    cell.region()
                } else {
                    Region::empty()
                };
                let piece = piece.without_small(min_area);
                if piece.is_empty() {
                    continue;
                }
                let clipped = piece.loops().len() != 1 || (piece.area() - whole).abs() > 1e-9 * whole.max(1.0);
                out.push(Cell { index, centre, region: piece, clipped });
            }
            out
        }
    }
}

/// Executes the recipe's geometry algorithms in the native host, without using the recipe protocol itself.
pub fn native_geometry_request(text: &str) -> Option<String> {
    let args = Json::parse(text).ok()?;
    let result = match args.get("op")?.as_str()? {
        "resolve" => geom2d::native_resolve(&args),
        "cells" => lattice::native_cells(&args),
        _ => None,
    }?;
    Some(result.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back() {
        let v = Json::obj()
            .set("a", 1.5)
            .set("b", "q\"x\n")
            .set("c", vec![Json::Null, Json::from(true), Json::from(3u32)])
            .set("d", [0.1, 2.0, -3.25]);
        let text = v.to_string();
        assert_eq!(text, r#"{"a":1.5,"b":"q\"x\n","c":[null,true,3],"d":[0.1,2,-3.25]}"#);
        assert_eq!(Json::parse(&text).unwrap(), v);
    }

    #[test]
    fn paths_and_escapes() {
        let v = Json::parse(r#" {"m": {"volumeMm3": 4800}, "faces": [{"index": 3}], "s": "é\t"} "#).unwrap();
        assert_eq!(v.at("m.volumeMm3").and_then(Json::as_f64), Some(4800.0));
        assert_eq!(v.at("faces.0.index").and_then(Json::as_f64), Some(3.0));
        assert_eq!(v.get("s").and_then(Json::as_str), Some("é\t"));
        assert!(Json::parse("{").is_err());
    }

    #[test]
    fn arrays() {
        let outline: Vec<[f64; 2]> = vec![[0.0, 0.0], [1.5, 0.0], [0.0, 2.0]];
        assert_eq!(Json::from(outline.clone()).to_string(), "[[0,0],[1.5,0],[0,2]]");
        assert_eq!(Json::arr(outline).to_string(), "[[0,0],[1.5,0],[0,2]]");
        assert_eq!(Json::arr(vec![1u32, 2]).push("x").to_string(), r#"[1,2,"x"]"#);
        assert_eq!(Json::Null.push(1.0).to_string(), "[1]");
    }

    #[test]
    fn data_shapes() {
        assert_eq!(Json::parse("[1, 2.5, -3]").unwrap().as_f64s(), Some(vec![1.0, 2.5, -3.0]));
        assert_eq!(Json::parse("[[1], [2]]").unwrap().as_f64s(), Some(vec![1.0, 2.0]));
        assert_eq!(Json::parse("[[1, 2]]").unwrap().as_f64s(), None);
        assert_eq!(Json::parse("[[1, 2, 9], [3, 4]]").unwrap().as_points2(), Some(vec![[1.0, 2.0], [3.0, 4.0]]));
        assert_eq!(Json::parse(r#"[{"x": 1, "y": 2}]"#).unwrap().as_points2(), Some(vec![[1.0, 2.0]]));
        assert_eq!(Json::parse("[1, 2]").unwrap().as_points2(), None);
        assert_eq!(Json::parse(r#"{"a": 1}"#).unwrap().as_f64s(), None);
    }

    #[test]
    fn section_reads_a_reply() {
        let reply = Json::parse(
            r#"{"ok":true,"sections":[{"frame":{"origin":[0,0,10],"u":[1,0,0],"v":[0,1,0],"w":[0,0,1]},"origin":[0,0,10],"turn":[0,0,0],
            "loops":[{"start":[-5,-5],"segments":[]}],"regions":[{"outer":[[-5,-5],[5,-5],[5,5],[-5,5]],"holes":[[[-1,-1],[-1,1],[1,1],[1,-1]]]}],"areaMm2":96}]}"#,
        )
        .unwrap();
        let all = Section::all(&reply);
        assert_eq!(all.len(), 1);
        let s = &all[0];
        assert_eq!(s.frame.w, [0.0, 0.0, 1.0]);
        assert_eq!(s.frame.point([2.0, 3.0]), [2.0, 3.0, 10.0]);
        assert!((s.region.area() - 96.0).abs() < 1e-9);
        assert_eq!(s.area, 96.0);
        assert_eq!(s.loops.as_array().map(|l| l.len()), Some(1));
        let op = s.extrude("cut", -2.0, s.loops.clone());
        assert_eq!(op.get("turn").map(|t| t.to_string()), Some("[0,0,0]".to_string()));
        assert!(Section::all(&Json::obj()).is_empty());
    }

    #[test]
    fn typed_params_json() {
        let s = Session::from_start(&Json::obj());
        let n = s.number("diameter", 80.0).range(20.0, 250.0).step(0.5).unit("mm").label("Outer diameter");
        assert_eq!(n.spec.to_json().to_string(), r#"{"name":"diameter","kind":"number","default":80,"min":20,"max":250,"step":0.5,"unit":"mm","label":"Outer diameter"}"#);
        let c = s.count("holes", 6).range(3, 24).help("Bolt holes");
        assert_eq!(c.spec.to_json().to_string(), r#"{"name":"holes","kind":"count","default":6,"min":3,"max":24,"help":"Bolt holes"}"#);
        assert_eq!(s.toggle("bore", true).spec.to_json().to_string(), r#"{"name":"bore","kind":"toggle","default":true}"#);
        assert_eq!(
            s.choice("edge", &["fillet", "chamfer"], "fillet").spec.to_json().to_string(),
            r#"{"name":"edge","kind":"choice","default":"fillet","options":["fillet","chamfer"]}"#
        );
        assert_eq!(s.text("label", "LIME").max_len(20).spec.to_json().to_string(), r#"{"name":"label","kind":"text","default":"LIME","maxLen":20}"#);
        assert_eq!(API, (1, 9));
    }

    #[test]
    fn typed_params_values() {
        let s = Session::from_start(&Json::parse(r#"{"params":{"d":300,"n":4.7,"m":1,"b":false,"e":"round","t":"abcdef","old":2.5}}"#).unwrap());
        assert_eq!(s.number("d", 80.0).range(20.0, 250.0).get(), 250.0);
        assert_eq!(s.number("missing", 80.0).range(20.0, 250.0).get(), 80.0);
        assert_eq!(s.count("n", 6).range(3, 24).get(), 4);
        assert_eq!(s.count("m", 6).range(3, 24).get(), 3);
        assert!(!s.toggle("b", true).get());
        assert_eq!(s.choice("e", &["fillet", "chamfer"], "fillet").get(), "fillet");
        assert_eq!(s.text("t", "x").max_len(3).get(), "abc");
        assert_eq!(s.param_f64("old", 1.0), 2.5);
        assert_eq!(s.param_u32("n", 1), 4);
    }

    #[test]
    fn picked_params_json() {
        let s = Session::from_start(&Json::obj());
        let f = s.face("face").rule("largest_up").label("Face to cut");
        assert_eq!(f.spec.to_json().to_string(), r#"{"name":"face","kind":"face","default":{"rule":"largest_up"},"label":"Face to cut"}"#);
        let g = s.face("side").facing([1.0, 0.0, 0.0]).rule("largest");
        assert_eq!(g.spec.to_json().to_string(), r#"{"name":"side","kind":"face","default":{"rule":"largest","facing":[1,0,0]}}"#);
        assert_eq!(s.face("any").spec.to_json().to_string(), r#"{"name":"any","kind":"face","default":null}"#);
        let o = s.outline("shape").default_poly(&geom2d::rect(4.0, 2.0)).help("A sketch or a face");
        assert_eq!(
            o.spec.to_json().to_string(),
            r#"{"name":"shape","kind":"outline","default":{"loops":[{"start":[-2,-1],"segments":[{"kind":"line","to":[2,-1]},{"kind":"line","to":[2,1]},{"kind":"line","to":[-2,1]},{"kind":"line","to":[-2,-1]}]}]},"help":"A sketch or a face"}"#
        );
        assert_eq!(s.outline("rim").default_face("top").spec.to_json().to_string(), r#"{"name":"rim","kind":"outline","default":{"face":{"rule":"top"}}}"#);
        assert_eq!(FACE_RULES.len(), 5);
    }

    #[test]
    fn a_curved_face_parameter_says_so_and_a_flat_one_does_not() {
        let s = Session::from_start(&Json::obj());
        assert_eq!(
            s.face("wall").curved().label("Wall").spec.to_json().to_string(),
            r#"{"name":"wall","kind":"face","default":null,"label":"Wall","curved":true}"#
        );
        assert_eq!(s.face("top").rule("top").spec.to_json().to_string(), r#"{"name":"top","kind":"face","default":{"rule":"top"}}"#);
        let p = s.face("wall").curved().get();
        assert!(p.curved);
        assert!(!s.face("top").get().curved);
        // A curved face is kept by its surface and a point.
        let given = Session::from_start(&Json::parse(r#"{"params":{"wall":{"surface":"cylinder","nearPoint":[-20,0,20]}}}"#).unwrap());
        assert_eq!(given.face("wall").curved().get().value.to_string(), r#"{"surface":"cylinder","nearPoint":[-20,0,20]}"#);
    }

    fn face_of(kind: &str, index: u32) -> PickedFace {
        PickedFace {
            index,
            normal: if kind == "plane" { [0.0, 0.0, 1.0] } else { [0.0; 3] },
            point: [-20.0, 0.0, 20.0],
            area: 10.0,
            outline: Section::from_json(&Json::obj()),
            kind: kind.to_string(),
            axis: None,
            radius: None,
            centre: None,
            axis_point: None,
        }
    }

    #[test]
    fn lettering_is_the_text_op_with_the_fields_given() {
        let flat = Lettering::new(6.0, 0.8).font("mono").style("bold").align("left").spacing(0.2).op("LOT 42", &face_of("plane", 3));
        assert_eq!(
            flat.to_string(),
            r#"{"kind":"text","faceIndex":3,"text":"LOT 42","height":6,"font":"mono","style":"bold","spacing":0.2,"align":"left","depth":0.8}"#
        );
        let round = Lettering::new(5.0, 0.6).font("line").around().flip().engrave().angle(10.0).op("Lime", &face_of("cylinder", 1));
        assert_eq!(
            round.to_string(),
            r#"{"kind":"text","faceIndex":1,"text":"Lime","height":5,"font":"line","depth":0.6,"point":[-20,0,20],"op":"cut","angleDeg":10,"along":"around","flip":true}"#
        );
        let sphere = Lettering::new(4.0, 0.5).u().project().stroke_width(0.5).op("A", &face_of("sphere", 2));
        assert_eq!(sphere.get("wrap").and_then(Json::as_str), Some("project"));
        assert_eq!(sphere.get("strokeWidth").and_then(Json::as_f64), Some(0.5));
        assert_eq!(sphere.get("along").and_then(Json::as_str), Some("u"));
    }

    #[test]
    fn the_outline_call_takes_the_layout_fields_and_no_depth() {
        let how = Lettering::new(10.0, 0.0).font("sans").style("bold").line_spacing(1.5);
        assert_eq!(how.outline_args("LIME").to_string(), r#"{"text":"LIME","height":10,"font":"sans","style":"bold","lineSpacing":1.5}"#);
    }

    #[test]
    fn picked_params_values() {
        let s = Session::from_start(&Json::parse(r#"{"params":{"face":{"normal":[0,0,1],"nearPoint":[1,2,3]},"n":4,"shape":{"sketch":7}}}"#).unwrap());
        let f = s.face("face").rule("largest").get();
        assert_eq!(f.kind, "face");
        assert_eq!(f.value.to_string(), r#"{"normal":[0,0,1],"nearPoint":[1,2,3]}"#);
        assert_eq!(s.face("n").get().value.to_string(), "4");
        assert_eq!(s.face("missing").rule("top").get().value.to_string(), r#"{"rule":"top"}"#);
        assert_eq!(s.outline("shape").get().value.to_string(), r#"{"sketch":7}"#);
        assert_eq!(s.outline("none").get().value, Json::Null);
    }

    #[test]
    fn combined_reads_failures() {
        let r = Json::parse(r#"{"ok":true,"applied":3,"failures":[{"index":1,"reason":"no"}]}"#).unwrap();
        let c = Combined::from_json(&r);
        assert_eq!(c.applied, 3);
        assert_eq!(c.failures, vec![(1, "no".to_string())]);
    }

    #[test]
    fn ops_shapes() {
        assert_eq!(ops::cuboid(10.0, 20.0, 4.0).to_string(), r#"{"primitive":"box","dx":10,"dy":20,"dz":4}"#);
        assert_eq!(
            ops::boolean("cut", ops::cylinder(2.0, 5.0), [1.0, 0.0, 0.0]).to_string(),
            r#"{"primitive":"cylinder","radius":2,"height":5,"kind":"boolean","op":"cut","origin":[1,0,0]}"#
        );
    }
}

#[cfg(test)]
mod geom_tests {
    use super::geom2d::*;
    use super::lattice;
    use std::f64::consts::PI;

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    fn sq(x0: f64, y0: f64, s: f64) -> Region {
        Region::from_poly(&rect(s, s).translate(x0 + s / 2.0, y0 + s / 2.0))
    }

    /// Area by counting a fine grid of sample points: an independent check of a boolean.
    fn sampled_area(r: &Region, lo: Pt, hi: Pt, n: usize) -> f64 {
        let (dx, dy) = ((hi[0] - lo[0]) / n as f64, (hi[1] - lo[1]) / n as f64);
        let mut inside = 0;
        for i in 0..n {
            for j in 0..n {
                if r.contains([lo[0] + (i as f64 + 0.5) * dx + 1.234e-5, lo[1] + (j as f64 + 0.5) * dy + 2.345e-5]) {
                    inside += 1;
                }
            }
        }
        inside as f64 * dx * dy
    }

    #[test]
    fn own_trigonometry_matches_std() {
        for k in -4000..4000 {
            let deg = k as f64 * 0.37;
            let (s, c) = sin_cos_deg(deg);
            let turned = (deg - 360.0 * (deg / 360.0).round()).to_radians();
            assert!(near(s, turned.sin(), 4e-16) && near(c, turned.cos(), 4e-16), "{deg}");
        }
        assert_eq!(sin_cos_deg(90.0), (1.0, 0.0));
        assert_eq!(sin_cos_deg(-180.0), (0.0, -1.0));
        assert_eq!(sin_cos_deg(450.0), (1.0, 0.0));
        let hex = regular(6, Size::Side(10.0), [0.0, 0.0], 90.0);
        assert_eq!(hex.pts[0], [0.0, 10.0]);
    }

    #[test]
    fn shapes_and_measures() {
        let r = rect(4.0, 2.0);
        assert_eq!(r.area(), 8.0);
        assert_eq!(r.perimeter(), 12.0);
        assert_eq!(r.bbox(), [[-2.0, -1.0], [2.0, 1.0]]);
        assert_eq!(r.translate(3.0, 1.0).centroid(), [3.0, 1.0]);
        assert!(r.is_convex());
        assert!(r.contains([0.0, 0.0]) && r.contains([2.0, 0.5]) && !r.contains([2.1, 0.0]));
        assert_eq!(r.reversed().area(), -8.0);
        let hex = regular(6, Size::Side(10.0), [0.0, 0.0], 90.0);
        assert!(near(hex.area(), 1.5 * 3f64.sqrt() * 100.0, 1e-9));
        assert!(near(hex.pts[0][1], 10.0, 1e-12) && near(hex.pts[0][0], 0.0, 1e-12));
        let flats = regular(6, Size::Across(10.0), [0.0, 0.0], 0.0);
        assert!(near(flats.bbox()[1][1] * 2.0, 10.0, 1e-9));
        let corners = regular(5, Size::Corners(10.0), [1.0, 2.0], 0.0);
        assert!(near(corners.pts[0][0], 6.0, 1e-12));
        let sq5 = regular(4, Size::Side(5.0), [0.0, 0.0], 45.0);
        assert!(near(sq5.area(), 25.0, 1e-9));
        assert!(near(circle(10.0, 720).area(), PI * 100.0, 0.01));
        let l = Poly::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0], [10.0, 20.0], [0.0, 20.0], [0.0, 0.0]]);
        assert_eq!(l.pts.len(), 6);
        assert!(!l.is_convex());
        assert!(!l.contains([15.0, 15.0]) && l.contains([5.0, 15.0]) && l.contains([10.0, 15.0]));
        let bowtie = Poly::new(vec![[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]]);
        assert!(!bowtie.is_convex());
        assert_eq!(Poly::default().area(), 0.0);
        assert_eq!(Poly::default().centroid(), [0.0, 0.0]);
    }

    #[test]
    fn batched_region_booleans_preserve_union_and_holes() {
        let a = sq(0.0, 0.0, 6.0);
        let b = sq(4.0, 1.0, 4.0);
        let c = sq(2.0, 3.0, 4.0);
        let batch = Region::union_all(&[a.clone(), b.clone(), c.clone()]);
        let folded = a.union(&b).union(&c);
        assert!(near(batch.area(), folded.area(), 1e-8));
        let cut = a.difference_all(&[b.clone(), c.clone()]);
        let folded_cut = a.difference(&b).difference(&c);
        assert!(near(cut.area(), folded_cut.area(), 1e-8));
        let ring = sq(0.0, 0.0, 10.0).difference(&sq(2.0, 2.0, 6.0));
        let fill = sq(3.0, 3.0, 2.0);
        let merged = Region::union_all(&[ring.clone(), fill.clone()]);
        assert!(near(merged.area(), ring.union(&fill).area(), 1e-8));
        assert!(merged.contains([4.0, 4.0]));
        assert!(!merged.contains([6.0, 6.0]));
        assert!(Region::union_all(&[]).is_empty());
        assert!(near(a.difference_all(&[]).area(), a.area(), 1e-8));
    }

    #[test]
    fn booleans_of_squares() {
        let a = sq(0.0, 0.0, 2.0);
        let b = sq(1.0, 1.0, 2.0);
        assert!(near(a.union(&b).area(), 7.0, 1e-9));
        assert_eq!(a.union(&b).loops().len(), 1);
        assert_eq!(a.union(&b).loops()[0].pts.len(), 8);
        assert!(near(a.intersect(&b).area(), 1.0, 1e-9));
        assert!(near(a.difference(&b).area(), 3.0, 1e-9));
        assert!(near(a.xor(&b).area(), 6.0, 1e-9));
        assert_eq!(a.xor(&b).pieces().len(), 2);
        // Sharing a whole edge: one outline, the shared edge and its split points gone.
        let c = sq(2.0, 0.0, 2.0);
        let u = a.union(&c);
        assert_eq!(u.loops().len(), 1);
        assert_eq!(u.loops()[0].pts.len(), 4);
        assert!(near(u.area(), 8.0, 1e-9));
        assert!(a.intersect(&c).is_empty());
        // Touching at one corner: two pieces.
        let d = sq(2.0, 2.0, 2.0);
        assert_eq!(a.union(&d).pieces().len(), 2);
        assert!(near(a.union(&d).area(), 8.0, 1e-9));
        // The same square twice.
        assert!(near(a.union(&a).area(), 4.0, 1e-9));
        assert!(a.difference(&a).is_empty());
        assert!(near(a.intersect(&a).area(), 4.0, 1e-9));
        // Missing entirely.
        let far = sq(10.0, 10.0, 1.0);
        assert!(a.intersect(&far).is_empty());
        assert!(near(a.difference(&far).area(), 4.0, 1e-9));
        assert!(near(Region::empty().union(&a).area(), 4.0, 1e-9));
        // Hexagons that only share an edge.
        let h1 = Region::from_poly(&regular(6, Size::Side(20.0), [0.0, 0.0], 90.0));
        let h2 = Region::from_poly(&regular(6, Size::Side(20.0), [20.0 * 3f64.sqrt(), 0.0], 90.0));
        assert!(h1.intersect(&h2).is_empty(), "{:?}", h1.intersect(&h2));
        assert_eq!(h1.union(&h2).loops().len(), 1);
        assert!(Region::empty().intersect(&a).is_empty());
    }

    #[test]
    fn booleans_with_holes_and_concave_outlines() {
        let big = sq(0.0, 0.0, 10.0);
        let small = sq(3.0, 3.0, 4.0);
        let ring = big.difference(&small);
        assert_eq!(ring.loops().len(), 2);
        assert!(ring.loops()[0].area() > 0.0 && ring.loops()[1].area() < 0.0);
        assert_eq!(ring.pieces().len(), 1);
        assert!(near(ring.area(), 84.0, 1e-9));
        assert!(!ring.contains([5.0, 5.0]) && ring.contains([1.0, 5.0]) && ring.contains([3.0, 5.0]));
        // Filling the hole back gives the square again, as one loop.
        let filled = ring.union(&small);
        assert_eq!(filled.loops().len(), 1);
        assert!(near(filled.area(), 100.0, 1e-9));
        // An island in the hole.
        let island = sq(4.0, 4.0, 2.0);
        let with_island = ring.union(&island);
        assert_eq!(with_island.pieces().len(), 2);
        assert!(near(with_island.area(), 88.0, 1e-9));
        // A bar across the ring cuts it in two.
        let bar = Region::from_poly(&rect(2.0, 20.0).translate(5.0, 5.0));
        let halves = ring.difference(&bar);
        assert_eq!(halves.pieces().len(), 2);
        assert!(near(halves.area(), 84.0 - 2.0 * 6.0, 1e-9));
        // A concave L against a square across its notch.
        let l = Region::from_poly(&Poly::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0], [10.0, 20.0], [0.0, 20.0]]));
        let s = sq(5.0, 5.0, 10.0);
        assert!(near(l.intersect(&s).area(), 75.0, 1e-9));
        assert!(near(l.union(&s).area(), 300.0 + 25.0, 1e-9));
        assert!(near(l.difference(&s).area(), 300.0 - 75.0, 1e-9));
        let lo = [-1.0, -1.0];
        let hi = [21.0, 21.0];
        for r in [l.intersect(&s), l.union(&s), l.difference(&s), l.xor(&s), ring.difference(&bar)] {
            let sampled = sampled_area(&r, lo, hi, 440);
            assert!(near(sampled, r.area(), 0.5), "{} vs {}", sampled, r.area());
        }
    }

    #[test]
    fn booleans_of_curved_and_star_shapes() {
        let c = Region::from_poly(&circle(10.0, 96));
        let star: Vec<Pt> = (0..10)
            .map(|k| {
                let r = if k % 2 == 0 { 14.0 } else { 5.0 };
                let a = (90.0 + 36.0 * k as f64).to_radians();
                [r * a.cos(), r * a.sin()]
            })
            .collect();
        let s = Region::from_poly(&Poly::new(star));
        let i = c.intersect(&s);
        let u = c.union(&s);
        let d = c.difference(&s);
        assert!(near(i.area() + u.area(), c.area() + s.area(), 1e-6));
        assert!(near(d.area(), c.area() - i.area(), 1e-6));
        assert_eq!(d.pieces().len(), 5);
        assert_eq!(s.difference(&c).pieces().len(), 5);
        let lo = [-15.0, -15.0];
        let hi = [15.0, 15.0];
        for r in [&i, &u, &d] {
            assert!(near(sampled_area(r, lo, hi, 600), r.area(), 0.6));
        }
    }

    /// Random outlines on a coarse grid, so edges overlap, cross at corners and touch: the
    /// area identities of the four booleans hold and the results' loops are valid inputs again.
    #[test]
    fn random_grid_outlines() {
        let mut seed: u64 = 12345;
        let mut next = move |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) % n) as f64
        };
        for round in 0..400 {
            let mut make = || {
                let k = 3 + next(6) as usize;
                let pts: Vec<Pt> = (0..k).map(|_| [next(7) * 2.0, next(7) * 2.0]).collect();
                Poly::new(pts)
            };
            let a = Region::from_poly(&make());
            let pb = make();
            let b = Region::from_poly(&pb);
            assert!(near(a.clip(&pb).area(), a.intersect(&b).area(), 1e-9), "round {round}: clip");
            let (u, i, d, x) = (a.union(&b), a.intersect(&b), a.difference(&b), a.xor(&b));
            assert!(near(u.area() + i.area(), a.area() + b.area(), 1e-9), "round {round}: {a:?} {b:?}");
            assert!(near(d.area(), a.area() - i.area(), 1e-9), "round {round}");
            assert!(near(x.area(), u.area() - i.area(), 1e-9), "round {round}");
            for r in [&u, &i, &d, &x] {
                for l in r.loops().iter() {
                    assert!(l.pts.len() >= 3);
                    assert!((0..l.pts.len()).all(|p| (p + 1..l.pts.len()).all(|q| l.pts[p] != l.pts[q])), "round {round}: a loop touches itself");
                }
                assert!(near(r.union(&Region::empty()).area(), r.area(), 1e-9), "round {round}: not stable");
            }
            let grown = a.offset_miter(0.7);
            let shrunk = a.offset_miter(-0.4);
            assert!(grown.area() >= a.area() - 1e-9 && shrunk.area() <= a.area() + 1e-9, "round {round}");
            assert!(shrunk.loops().iter().all(|l| l.pts.iter().all(|p| a.contains(*p))), "round {round}: shrunk outside");
            assert!(a.offset(0.7).to_region(0.001).difference(&grown).area() < 1e-6, "round {round}: rounded beyond mitred");
            if round % 30 == 0 {
                assert!(near(sampled_area(&d, [0.0, 0.0], [12.0, 12.0], 240), d.area(), 0.3), "round {round}: {} vs {}
A {:?}
B {:?}
D {:?}", sampled_area(&d, [0.0, 0.0], [12.0, 12.0], 240), d.area(), a, b, d);
            }
        }
    }

    #[test]
    fn degenerate_input() {
        let flat = Region::from_poly(&Poly::new(vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]]));
        assert!(flat.is_empty());
        let two = Region::from_poly(&Poly::new(vec![[0.0, 0.0], [1.0, 0.0]]));
        assert!(two.is_empty());
        // A loop that touches itself at one point is two pieces.
        let touch = Poly::new(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0], [1.0, 1.0], [0.0, 1.0]]);
        let r = Region::from_poly(&touch);
        assert_eq!(r.pieces().len(), 2);
        assert!(near(r.area(), 2.0, 1e-12));
        // A self-crossing bow tie keeps both lobes.
        let bowtie = Region::from_poly(&Poly::new(vec![[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]]));
        assert_eq!(bowtie.pieces().len(), 2);
        assert!(near(bowtie.area(), 2.0, 1e-12));
        // A hole drawn on the outline's edge leaves a U.
        let u = Region::with_holes(&rect(4.0, 4.0).translate(2.0, 2.0), &[rect(2.0, 2.0).translate(2.0, 1.0)]);
        assert_eq!(u.loops().len(), 1);
        assert!(near(u.area(), 12.0, 1e-12));
        // Nested any way round: even-odd.
        let nest = Region::from_loops(&[rect(10.0, 10.0), rect(6.0, 6.0), rect(2.0, 2.0)]);
        assert_eq!(nest.pieces().len(), 2);
        assert!(near(nest.area(), 100.0 - 36.0 + 4.0, 1e-12));
    }

    #[test]
    fn corners_round_and_chamfer() {
        let r = rect(20.0, 10.0).round_corners(2.0);
        assert_eq!(r.counts(), [4, 0, 0]);
        assert!(near(r.area(), 200.0 - (4.0 - PI) * 4.0, 1e-9));
        let json = r.to_loop().to_string();
        assert!(json.starts_with(r#"{"start":[-8,-5],"segments":[{"kind":"line","to":[8,-5]},{"kind":"arc","through":["#), "{json}");
        // Edges shorter than the radius: each corner shrinks to fit.
        let thin = rect(10.0, 1.0).round_corners(2.0);
        assert!(thin.corners[0].iter().all(|c| matches!(c, Corner::Reduced(r) if near(*r, 0.5, 1e-12))));
        assert!(near(thin.area(), 10.0 - (4.0 - PI) * 0.25, 1e-9));
        // A share limit and a minimum radius, as the honeycomb recipe rounds its cells.
        let capped = rect(10.0, 1.0).round_corners_with(2.0, 0.45, 0.0);
        assert_eq!(capped.counts(), [0, 4, 0]);
        assert!(capped.corners[0].iter().all(|c| matches!(c, Corner::Reduced(r) if near(*r, 0.45, 1e-12))));
        let sharp = rect(10.0, 1.0).round_corners_with(2.0, 0.45, 0.5);
        assert_eq!(sharp.counts(), [0, 0, 4]);
        assert_eq!(sharp.to_loop().to_string(), rect(10.0, 1.0).to_loop().to_string());
        // A concave corner is rounded the other way: material is added.
        let l = Poly::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0], [10.0, 20.0], [0.0, 20.0]]);
        let lr = l.round_corners(1.0);
        assert_eq!(lr.counts(), [6, 0, 0]);
        assert!(near(lr.area(), 300.0 - 5.0 * (4.0 - PI) / 4.0 + (4.0 - PI) / 4.0, 1e-9));
        let ch = rect(10.0, 10.0).chamfer_corners(1.0);
        assert_eq!(ch.counts(), [4, 0, 0]);
        assert!(near(ch.area(), 100.0 - 2.0, 1e-9));
        assert_eq!(ch.paths[0].segs.len(), 8);
        let ch_short = rect(10.0, 1.0).chamfer_corners(1.0);
        assert!(ch_short.corners[0].iter().all(|c| matches!(c, Corner::Reduced(r) if near(*r, 0.5, 1e-12))));
        let rr = rounded_rect(30.0, 20.0, 5.0);
        assert!(near(rr.area(), 600.0 - (4.0 - PI) * 25.0, 1e-9));
        let back = rr.to_region(0.001);
        assert!(near(back.area(), rr.area(), 0.03), "{} vs {}", back.area(), rr.area());
        assert!(near(rr.to_region(1e-5).area(), rr.area(), 3e-4));
        assert!(back.contains([0.0, 0.0]) && !back.contains([14.9, 9.9]));
    }

    // The three corner fixtures shared with the sketch card's Offset (docs/plan-sketch-for-people.md, 7a).
    #[test]
    fn offset_fixture_outside_corner() {
        let square = Region::from_poly(&rect(20.0, 20.0).translate(10.0, 10.0));
        let grown = square.offset(2.0);
        assert_eq!(grown.counts(), [4, 0, 0]);
        assert!(near(grown.area(), 400.0 + 4.0 * 20.0 * 2.0 + PI * 4.0, 1e-9));
        // Each arc is centred on the original corner: the corner at (20, 20) bulges to (21.414, 21.414).
        let s = 2.0 / 2f64.sqrt();
        let arcs: Vec<Pt> = grown.paths[0]
            .segs
            .iter()
            .filter_map(|g| match g {
                Seg::Arc(m, _) => Some(*m),
                Seg::Line(_) => None,
            })
            .collect();
        assert!(arcs.iter().any(|m| near(m[0], 20.0 + s, 1e-9) && near(m[1], 20.0 + s, 1e-9)), "{arcs:?}");
        let shrunk = square.offset(-2.0);
        assert_eq!(shrunk.counts(), [0, 0, 4]);
        assert!(near(shrunk.area(), 256.0, 1e-9));
    }

    #[test]
    fn offset_fixture_inside_corner() {
        let l = Region::from_poly(&Poly::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0], [10.0, 20.0], [0.0, 20.0]]));
        let grown = l.offset(1.0);
        assert_eq!(grown.counts(), [5, 0, 1]);
        let path = &grown.paths[0];
        let ends: Vec<Pt> = path
            .segs
            .iter()
            .map(|g| match g {
                Seg::Line(to) => *to,
                Seg::Arc(_, to) => *to,
            })
            .collect();
        assert!(ends.contains(&[11.0, 11.0]), "{ends:?}");
        assert!(near(grown.area(), 300.0 + 80.0 - 1.0 + 1.25 * PI, 1e-9));
        // Shrinking: the inside corner becomes the outside one and is the only one rounded.
        let shrunk = l.offset(-1.0);
        assert_eq!(shrunk.counts(), [1, 0, 5]);
    }

    #[test]
    fn offset_fixture_short_edge_corner() {
        // A 1 mm step: grown by 2, the corner above it has 1 mm of edge, so its radius is reduced to 1.
        let step = Region::from_poly(&Poly::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [11.0, 10.0], [11.0, 11.0], [0.0, 11.0]]));
        let grown = step.offset(2.0);
        assert_eq!(grown.counts(), [4, 1, 1]);
        assert!(grown.corners[0].iter().any(|c| matches!(c, Corner::Reduced(r) if near(*r, 1.0, 1e-9))), "{:?}", grown.corners);
        assert!(grown.area() > step.area());
    }

    #[test]
    fn offsets() {
        let ring = sq(0.0, 0.0, 10.0).difference(&sq(3.0, 3.0, 4.0));
        let m = ring.offset_miter(-1.0);
        assert!(near(m.area(), 64.0 - 36.0, 1e-9));
        assert_eq!(m.loops().len(), 2);
        assert!(ring.offset_miter(-2.0).area() < 1e-9);
        assert!(sq(0.0, 0.0, 2.0).offset(-1.5).paths.is_empty());
        assert!(near(sq(0.0, 0.0, 2.0).offset_miter(1.0).area(), 16.0, 1e-9));
        assert_eq!(ring.offset(0.0).paths.len(), 2);
        // Growing two squares 4 mm apart by 2.5: they join.
        let two = sq(0.0, 0.0, 10.0).union(&sq(14.0, 0.0, 10.0));
        assert_eq!(two.offset_miter(2.5).pieces().len(), 1);
        assert_eq!(two.offset_miter(1.5).pieces().len(), 2);
        // A circle grows by the distance.
        let c = Region::from_poly(&circle(10.0, 360)).offset_miter(2.0);
        assert!(near(c.area(), PI * 144.0, 0.05));
        // A dumbbell shrunk past its neck comes apart.
        let bell = sq(0.0, 0.0, 10.0).union(&sq(20.0, 0.0, 10.0)).union(&Region::from_poly(&rect(12.0, 2.0).translate(15.0, 5.0)));
        assert_eq!(bell.pieces().len(), 1);
        assert_eq!(bell.offset_miter(-1.5).pieces().len(), 2);
    }

    #[test]
    fn cleanup() {
        let messy = Poly::new(vec![[0.0, 0.0], [0.01, 0.0], [5.0, 0.0], [10.0, 0.001], [10.0, 10.0], [0.0, 10.0], [0.0, 5.0]]);
        assert_eq!(messy.clean(0.05).pts, vec![[0.0, 0.0], [10.0, 0.001], [10.0, 10.0], [0.0, 10.0]]);
        let sliver = Poly::new(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 0.02], [0.0, 0.02]]);
        assert!(sliver.clean(0.05).pts.is_empty());
        assert_eq!(sliver.clean(0.01).pts.len(), 4);
        let spike = Poly::new(vec![[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        assert_eq!(spike.clean(1e-6).pts.len(), 4);
        let r = Region::from_loops(&[rect(10.0, 10.0), sliver.translate(-20.0, 0.0)]);
        assert_eq!(r.loops().len(), 2);
        assert_eq!(r.clean(0.05).loops().len(), 1);
    }

    #[test]
    fn loops_json() {
        let p = Poly::new(vec![[0.0, 0.0], [1.5, 0.0], [0.0, 2.0]]);
        assert_eq!(
            p.to_loop().to_string(),
            r#"{"start":[0,0],"segments":[{"kind":"line","to":[1.5,0]},{"kind":"line","to":[0,2]},{"kind":"line","to":[0,0]}]}"#
        );
        let ring = sq(0.0, 0.0, 10.0).difference(&sq(3.0, 3.0, 4.0)).union(&sq(20.0, 0.0, 2.0));
        let loops = ring.to_loops();
        assert_eq!(loops.len(), 2);
        assert_eq!(loops[0].as_array().map(|a| a.len()), Some(2));
        assert_eq!(loops[1].as_array().map(|a| a.len()), Some(1));
        let rounded = ring.round_corners(0.5).to_loops();
        assert_eq!(rounded.iter().map(|l| l.as_array().map(|a| a.len()).unwrap_or(0)).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(rounded[0].at("1.segments.1.kind").and_then(|k| k.as_str()), Some("arc"));
    }

    #[test]
    fn lattices() {
        let floor = Region::from_poly(&rect(100.0, 60.0));
        let cells = lattice::hex(10.0, lattice::Orient::Pointy).cells(&floor, 0.0);
        let total: f64 = cells.iter().map(|c| c.area()).sum();
        assert!(near(total, 6000.0, 1e-6));
        assert!(cells.iter().any(|c| !c.clipped) && cells.iter().any(|c| c.clipped));
        let full = cells.iter().find(|c| c.index == [0, 0]).unwrap();
        assert!(!full.clipped && near(full.area(), 1.5 * 3f64.sqrt() * 100.0, 1e-9));
        assert_eq!(full.centre, [0.0, 0.0]);
        let odd = cells.iter().find(|c| c.index == [0, 1]).unwrap();
        assert!(near(odd.centre[0], 3f64.sqrt() * 5.0, 1e-12) && near(odd.centre[1], 15.0, 1e-12));
        // Walls: a 2.4 mm rib leaves a pocket of side 10 - 2.4/√3.
        let pockets = lattice::hex(10.0, lattice::Orient::Pointy).rib(2.4).cells(&floor, 12.0);
        let p0 = pockets.iter().find(|c| c.index == [0, 0]).unwrap();
        let a2 = 10.0 - 2.4 / 3f64.sqrt();
        assert!(near(p0.area(), 1.5 * 3f64.sqrt() * a2 * a2, 1e-9));
        assert!(pockets.iter().all(|c| c.area() >= 12.0));
        // Flat hexagons: a flat side on top.
        let flat = lattice::hex(10.0, lattice::Orient::Flat).cells(&floor, 0.0);
        let f0 = flat.iter().find(|c| c.index == [0, 0]).unwrap();
        assert!(near(f0.region.bbox()[1][1], 3f64.sqrt() * 5.0, 1e-9));
        // Squares fill exactly, bricks and triangles tile too.
        let squares = lattice::rect(10.0, 10.0).at([5.0, 5.0]).cells(&floor, 0.0);
        assert_eq!(squares.len(), 60);
        assert!(squares.iter().all(|c| !c.clipped));
        let bricks: f64 = lattice::staggered(10.0, 5.0, 5.0).cells(&floor, 0.0).iter().map(|c| c.area()).sum();
        assert!(near(bricks, 6000.0, 1e-6));
        let tris = lattice::tri(10.0).cells(&floor, 0.0);
        assert!(near(tris.iter().map(|c| c.area()).sum::<f64>(), 6000.0, 1e-6));
        let t0 = tris.iter().find(|c| c.index == [0, 0]).unwrap();
        assert!(near(t0.centre[1], -10.0 * 3f64.sqrt() / 12.0, 1e-12));
        // A ring: no cell in the hole.
        let ring = floor.difference(&Region::from_poly(&rect(40.0, 30.0)));
        let ring_cells = lattice::rect(5.0, 5.0).cells(&ring, 0.0);
        assert!(near(ring_cells.iter().map(|c| c.area()).sum::<f64>(), 6000.0 - 1200.0, 1e-6));
        // Along a line, and a turned grid.
        let pegs = lattice::along(&[[0.0, 0.0], [30.0, 0.0], [30.0, 40.0]], 8, circle(1.0, 24)).cells(&Region::from_poly(&rect(200.0, 200.0)), 0.0);
        assert_eq!(pegs.len(), 8);
        assert!(near(pegs[7].centre[0], 30.0, 1e-12) && near(pegs[7].centre[1], 40.0, 1e-12));
        assert!(near(pegs[1].centre[0], 10.0, 1e-12));
        let turned = lattice::rect(10.0, 10.0).turned(45.0).at([1.0, 1.0]).cells(&floor, 0.0);
        assert!(near(turned.iter().map(|c| c.area()).sum::<f64>(), 6000.0, 1e-6));
        // An empty region, and slivers dropped by min_area.
        assert!(lattice::hex(10.0, lattice::Orient::Pointy).cells(&Region::empty(), 0.0).is_empty());
        let few = lattice::rect(10.0, 10.0).cells(&Region::from_poly(&rect(10.5, 10.5)), 3.0);
        assert_eq!(few.len(), 1);
    }
}

/// A declarative pose animation. The host owns sampling, limits and rendering (recipe API 1.8).
pub mod anim {
    use super::Json;

    /// One joint target in the member's part frame, or a model-space Reach target.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Aim {
        Degrees(f64), Rest, Aligned, Min, Max, Fraction(f64),
        Rotation([f64; 4]), Reach { at: [f64; 3], to: [f64; 3] },
    }
    impl From<f64> for Aim { fn from(value: f64) -> Self { Aim::Degrees(value) } }
    impl Aim {
        fn json(&self) -> Json {
            match self {
                Aim::Degrees(value) => Json::from(*value),
                Aim::Rest => Json::from("rest"), Aim::Aligned => Json::from("aligned"),
                Aim::Min => Json::from("min"), Aim::Max => Json::from("max"),
                Aim::Fraction(value) => Json::obj().set("fraction", *value),
                Aim::Rotation(q) => Json::obj().set("rotation", Json::arr(q.to_vec())),
                Aim::Reach { at, to } => Json::obj().set("reach", Json::obj().set("at", *at).set("to", *to)),
            }
        }
    }

    /// The easing into a key. The default is linear.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Ease { Linear, InOut }
    impl Ease { fn name(self) -> &'static str { match self { Ease::Linear => "linear", Ease::InOut => "inOut" } } }

    /// Names matching a wildcard union. The original pattern goes to the host, which refuses an empty group.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Group { pattern: String, names: Vec<String> }
    impl Group { pub fn names(&self) -> &[String] { &self.names } }

    /// A joint and its parent, as supplied by the host. All angles are degrees.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Member {
        pub name: String, pub parent: Option<String>, pub joint: Option<String>,
        pub angle: Option<f64>, pub min: Option<f64>, pub max: Option<f64>, pub cone: Option<f64>, pub aligned: Option<f64>,
    }
    #[derive(Clone, Debug, PartialEq)]
    pub struct Rig { pub members: Vec<Member> }
    impl Rig {
        pub(crate) fn from_json(value: &Json) -> Option<Self> {
            let rows = value.get("members")?.as_array()?;
            let mut members = Vec::new();
            for row in rows {
                let joint = row.get("joint");
                members.push(Member {
                    name: row.get("name")?.as_str()?.to_string(),
                    parent: row.get("parent").and_then(Json::as_str).map(str::to_string),
                    joint: joint.and_then(|j| j.get("kind")).and_then(Json::as_str).map(str::to_string),
                    angle: joint.and_then(|j| j.get("angle")).and_then(Json::as_f64),
                    min: joint.and_then(|j| j.get("min")).and_then(Json::as_f64),
                    max: joint.and_then(|j| j.get("max")).and_then(Json::as_f64),
                    cone: joint.and_then(|j| j.get("cone")).and_then(Json::as_f64),
                    aligned: row.get("aligned").and_then(Json::as_f64),
                });
            }
            Some(Rig { members })
        }
        pub fn group(&self, pattern: &str) -> Group {
            let mut names = Vec::new();
            for member in &self.members {
                if pattern.split('|').any(|one| matches(one.trim(), &member.name)) { names.push(member.name.clone()); }
            }
            Group { pattern: pattern.to_string(), names }
        }
    }
    fn matches(pattern: &str, name: &str) -> bool {
        let pieces: Vec<&str> = pattern.split('*').collect();
        if pieces.len() == 1 { return pattern == name; }
        if !name.starts_with(pieces[0]) || !name.ends_with(pieces[pieces.len() - 1]) { return false; }
        let mut from = pieces[0].len();
        for piece in pieces.iter().skip(1) {
            match name[from..].find(piece) { Some(at) => from += at + piece.len(), None => return false }
        }
        from <= name.len()
    }

    /// A sparse pose: unmentioned joints keep the value interpolated by the host.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Pose { rest: bool, set: Vec<(String, Aim)>, colors: Vec<(String, String)> }
    impl Default for Pose { fn default() -> Self { Self::new() } }
    impl Pose {
        pub fn new() -> Self { Pose { rest: false, set: Vec::new(), colors: Vec::new() } }
        pub fn rest() -> Self { Pose { rest: true, set: Vec::new(), colors: Vec::new() } }
        pub fn set(mut self, name: &str, aim: impl Into<Aim>) -> Self { self.set.push((name.to_string(), aim.into())); self }
        pub fn set_all(mut self, group: &Group, aim: Aim) -> Self { self.set.push((group.pattern.clone(), aim)); self }
        /// A colour change at this key is a cut, not an interpolation.
        pub fn colour(mut self, name: &str, hex: &str) -> Self { self.colors.push((name.to_string(), hex.to_string())); self }
        fn json(&self) -> (Json, Json) {
            let mut set = Json::obj(); let mut colors = Json::obj();
            for (name, aim) in &self.set { set = set.set(name, aim.json()); }
            for (name, color) in &self.colors { colors = colors.set(name, color.as_str()); }
            (set, colors)
        }
    }

    /// A camera direction from the assembly toward the camera, with an optional up direction.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Camera { direction: [f64; 3], up: Option<[f64; 3]> }
    impl Camera {
        pub fn looking(x: f64, y: f64, z: f64) -> Self { Camera { direction: [x, y, z], up: None } }
        pub fn up(mut self, x: f64, y: f64, z: f64) -> Self { self.up = Some([x, y, z]); self }
        fn json(&self) -> Json { let mut out = Json::obj().set("direction", self.direction); if let Some(up) = self.up { out = out.set("up", up); } out }
    }
    #[derive(Clone, Debug, PartialEq)]
    pub struct Key { t: f64, line: u32, pose: Pose, ease: Ease, hold: f64 }
    impl Key {
        pub fn ease(&mut self, ease: Ease) -> &mut Self { self.ease = ease; self }
        pub fn hold(&mut self, seconds: f64) -> &mut Self { self.hold = seconds; self }
        fn json(&self) -> Json {
            let (set, colors) = self.pose.json();
            Json::obj().set("t", self.t).set("line", self.line).set("rest", self.pose.rest).set("set", set).set("colors", colors).set("ease", self.ease.name()).set("hold", self.hold)
        }
    }
    /// Keys and presentation for one animation; no pose is changed until the host runs it.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Animation { fps: u32, keys: Vec<Key>, camera: Option<Camera>, colors: Vec<(String, String)>, background: Option<String> }
    impl Animation {
        pub fn new(fps: u32) -> Self { Animation { fps, keys: Vec::new(), camera: None, colors: Vec::new(), background: None } }
        #[track_caller]
        pub fn key(&mut self, time: f64, pose: Pose) -> &mut Key {
            self.keys.push(Key { t: time, line: std::panic::Location::caller().line(), pose, ease: Ease::Linear, hold: 0.0 });
            self.keys.last_mut().unwrap()
        }
        pub fn camera(&mut self, camera: Camera) -> &mut Self { self.camera = Some(camera); self }
        pub fn colour(&mut self, name: &str, hex: &str) -> &mut Self { self.colors.push((name.to_string(), hex.to_string())); self }
        /// The frames' background, "#rrggbb" (recipe API 1.9). The host refuses any other form; a caller's own choice overrides it.
        pub fn background(&mut self, hex: &str) -> &mut Self { self.background = Some(hex.to_string()); self }
        pub fn to_json(&self) -> Json {
            let mut colors = Json::obj();
            for (name, color) in &self.colors { colors = colors.set(name, color.as_str()); }
            let mut out = Json::obj().set("fps", self.fps).set("keys", Json::Arr(self.keys.iter().map(Key::json).collect())).set("colors", colors);
            if let Some(camera) = &self.camera { out = out.set("camera", camera.json()); }
            if let Some(background) = &self.background { out = out.set("background", background.as_str()); }
            out
        }
    }
}

#[cfg(test)]
mod animation_tests {
    use super::{anim::{Aim, Animation, Ease, Pose, Rig}, Json};

    #[test]
    fn animation_encodes_keys_and_rig_groups() {
        let rig = Rig::from_json(&Json::obj().set("members", Json::arr(vec![
            Json::obj().set("name", "Finger 1 root").set("parent", "Palm").set("joint", Json::obj().set("kind", "hinge").set("angle", 12).set("min", -90).set("max", 90)).set("aligned", 15),
            Json::obj().set("name", "Finger 2 claw"),
        ]))).unwrap();
        let group = rig.group("Finger 1 *|Finger 2 *");
        assert_eq!(group.names().len(), 2);
        assert_eq!(rig.members[0].aligned, Some(15.0));
        let mut anim = Animation::new(12);
        anim.key(1.0, Pose::rest().set_all(&group, Aim::Aligned).colour("Finger 1 root", "#ff0000")).ease(Ease::InOut).hold(0.5);
        let data = anim.to_json();
        assert_eq!(data.at("fps").and_then(Json::as_f64), Some(12.0));
        assert_eq!(data.at("keys.0.set.Finger 1 *|Finger 2 *").and_then(Json::as_str), Some("aligned"));
        assert_eq!(data.at("keys.0.colors.Finger 1 root").and_then(Json::as_str), Some("#ff0000"));
        assert_eq!(data.at("keys.0.hold").and_then(Json::as_f64), Some(0.5));
        assert!(data.at("keys.0.line").and_then(Json::as_f64).unwrap() > 0.0);
    }

    #[test]
    fn background_is_sent_only_when_set() {
        let mut anim = Animation::new(12);
        assert!(anim.to_json().at("background").is_none(), "a 1.8 animation must send the same call as before");
        anim.background("#204060");
        assert_eq!(anim.to_json().at("background").and_then(Json::as_str), Some("#204060"));
    }
}


fn sc(rad: f64) -> (f64, f64) {
    geom2d::sin_cos_deg(rad * (180.0 / std::f64::consts::PI))
}

/// Involute gear outline. The flank samples use the crate's own sin/cos so the
/// bits do not depend on libm. `salt` makes each repetition a different input.
fn gear_outline(teeth: u32, module: f64, pressure_deg: f64, flank: u32, salt: f64) -> geom2d::Poly {
    let module = module + salt;
    let (_sp, cp) = geom2d::sin_cos_deg(pressure_deg);
    let pitch = teeth as f64 * module / 2.0;
    let base_r = pitch * cp;
    let tip = pitch + module;
    let root = pitch - 1.25 * module;
    let root = if root > base_r * 0.2 { root } else { base_r * 0.2 };
    let mut max_t = 0.05_f64;
    let mut guard = 0;
    while guard < 40 {
        let (s, c) = sc(max_t);
        let x = base_r * (c + max_t * s);
        let y = base_r * (s - max_t * c);
        let r = (x * x + y * y).sqrt();
        if r >= tip { break; }
        max_t = max_t + 0.02;
        guard = guard + 1;
    }
    let tooth = std::f64::consts::PI * 2.0 / teeth as f64;
    let mut pts: Vec<geom2d::Pt> = Vec::new();
    let mut n = 0u32;
    while n < teeth {
        let centre = tooth * n as f64;
        let half = tooth * 0.25;
        let mut k = 0u32;
        while k <= flank {
            let t = max_t * (flank - k) as f64 / flank as f64;
            let (s, c) = sc(t);
            let x = base_r * (c + t * s);
            let y = base_r * (s - t * c);
            let ang = centre - half;
            let (sa, ca) = sc(ang);
            pts.push([x * ca - y * sa, x * sa + y * ca]);
            k = k + 1;
        }
        let (sr, cr) = sc(centre - half * 0.15);
        pts.push([root * cr, root * sr]);
        let mut k2 = 0u32;
        while k2 <= flank {
            let t = max_t * k2 as f64 / flank as f64;
            let (s, c) = sc(t);
            let x = base_r * (c + t * s);
            let y = -(base_r * (s - t * c));
            let ang = centre + half;
            let (sa, ca) = sc(ang);
            pts.push([x * ca - y * sa, x * sa + y * ca]);
            k2 = k2 + 1;
        }
        n = n + 1;
    }
    geom2d::Poly::new(pts)
}

fn gear_once(salt: f64) -> u64 {
    let teeth = [12u32, 18, 24, 30, 36, 17, 23, 41, 60];
    let mut acc = 0.0_f64;
    let mut i = 0;
    while i < teeth.len() {
        let poly = gear_outline(teeth[i], 2.0, 20.0, 8, salt);
        let mut k = 0;
        while k < poly.pts.len() {
            acc = acc + poly.pts[k][0] + poly.pts[k][1];
            k = k + 1;
        }
        i = i + 1;
    }
    acc.to_bits()
}

fn fold_region(r: &geom2d::Region) -> u64 {
    let mut b = r.area().to_bits();
    let c = r.centroid();
    b = b.wrapping_add(c[0].to_bits()).wrapping_add(c[1].to_bits());
    let loops = r.loops();
    let mut i = 0;
    while i < loops.len() {
        let pts = &loops[i].pts;
        let mut k = 0;
        while k < pts.len() {
            b = b.wrapping_mul(0x100000001b3).wrapping_add(pts[k][0].to_bits()).wrapping_add(pts[k][1].to_bits());
            k = k + 1;
        }
        i = i + 1;
    }
    b
}

/// 80 hex cells clipped to a disc, unioned, cut out of the disc, then inset.
/// The shape of Lime Slice's geom2d row (plan-native-recipes.md section 2.7).
fn geom_once() -> (u64, usize) {
    let disc = geom2d::circle(40.0, 96);
    let disc_r = geom2d::Region::from_poly(&disc);
    let mut cells: Vec<geom2d::Region> = Vec::new();
    let side = 4.2_f64;
    let mut row = 0;
    while row < 10 {
        let mut col = 0;
        while col < 8 {
            let shift = if row % 2 == 1 { side * 0.8660254037844386 } else { 0.0 };
            let x = col as f64 * side * 1.7320508075688772 + shift - 30.0;
            let y = row as f64 * side * 1.5 - 28.0;
            let hex = geom2d::regular(6, geom2d::Size::Side(side * 0.9), [x, y], 30.0);
            cells.push(disc_r.clip(&hex));
            col = col + 1;
        }
        row = row + 1;
    }
    let unioned = geom2d::Region::union_all(&cells);
    let cut = disc_r.difference(&unioned);
    let inset = cut.offset_miter(-0.35);
    let n = inset.loops().len();
    (fold_region(&inset), n)
}

fn main() {
    let reps = 400u32;
    let t0 = std::time::Instant::now();
    let mut bits = 0u64;
    let mut i = 0u32;
    while i < reps {
        bits ^= gear_once(i as f64 * 1.0e-12);
        i = i + 1;
    }
    let gear_us = t0.elapsed().as_micros();
    println!("gear {gear_us} {bits:x}");
    let t1 = std::time::Instant::now();
    let (gb, n) = geom_once();
    let geom_us = t1.elapsed().as_micros();
    println!("geom {geom_us} {gb:x} {n}");
}
