#!/usr/bin/env bash
# Tests for `lust fix` and the rewrites for rejects (src/fix.rs, src/rewrite.rs).
# Run after `cargo build --release`.
#   A reject is valid Rust that lust refuses. Where the diagnostic carries an
#   edit, `lust fix --all` must turn the file into Rust that lust accepts, and
#   the fixed program must give the same stdout, stderr and exit code under
#   rustc and under lust (rule zero). Plain `lust fix` applies only the `auto`
#   edits, and `--dry-run` writes nothing. Rewrites: a long assert! (L0809),
#   write! into a user writer (L0641), a lifetime argument on a fn (L0731),
#   #![no_implicit_prelude] (L0522), a closure in a const or static (L0630).
#   `fix --sandbox` re-checks under the sandbox rules, so a fixed file that
#   reaches a refused service still fails with L0532.
# Usage: scripts/test-fix.sh
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) [ -f "$lust.exe" ] && lust=$lust.exe ;;
esac
if [ ! -x "$lust" ] && [ ! -f "$lust" ]; then
  echo "error: $lust not built. Run cargo build --release first."
  exit 2
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export LUST_NO_DAEMON=1 LUST_NO_CACHE=1
pass=0
fail=0
ok() { pass=$((pass + 1)); echo "ok   $1"; }
bad() { fail=$((fail + 1)); echo "FAIL $1: $2"; }
# is <name> <got> <want>
is() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "got: $2, want: $3"; fi; }

# same <name> <file.rs> : rustc and lust both build the file, and the two
# programs give the same stdout, stderr (thread id masked) and exit code.
same() {
  local name=$1 f=$2 r l
  # Both compilers get the path as typed here: a panic location prints it.
  if ! (cd "$(dirname "$f")" && rustc --edition 2024 -A warnings -o "$work/rustc-bin" "$(basename "$f")") 2>"$work/rustc.err"; then
    bad "$name" "rustc rejects the fixed file: $(head -5 "$work/rustc.err")"; return
  fi
  if ! (cd "$(dirname "$f")" && "$lust" build "$(basename "$f")" -o "$work/lust-bin") 2>"$work/lust.err"; then
    bad "$name" "lust rejects the fixed file: $(head -5 "$work/lust.err")"; return
  fi
  mask() { sed -E "s/thread '([^']*)' \([0-9]+\)/thread '\1' (N)/"; }
  r=$( (cd "$(dirname "$f")" && "$work/rustc-bin" 2>&1; echo "exit $?") | mask)
  l=$( (cd "$(dirname "$f")" && "$work/lust-bin" 2>&1; echo "exit $?") | mask)
  if [ "$r" = "$l" ]; then ok "$name"; else bad "$name" "rustc: $r / lust: $l"; fi
}

# ------------------------------------------------ the long assert! (L0809)
d=$work/assert; mkdir -p "$d"
cp "$root/tests/reject/assert_long_condition.rs" "$d/a.rs"
before=$(cat "$d/a.rs")
(cd "$d" && "$lust" check a.rs) >/dev/null 2>&1; is "assert: the reject fails its check" $? 1
out=$( (cd "$d" && "$lust" check a.rs --json) 2>/dev/null )
case "$out" in *'"code":"L0809"'*'"edits":[{"start":'*'"replacement":", \"assertion failed: first_value == 1 && '*'"auto":false}]'*) ok "assert: the diagnostic carries the edit";; *) bad "assert: the diagnostic carries the edit" "$out";; esac
(cd "$d" && "$lust" fix a.rs) >/dev/null 2>&1; is "assert: plain fix leaves a non-auto edit, exit 1" $? 1
is "assert: plain fix writes nothing" "$(cat "$d/a.rs")" "$before"
err=$( (cd "$d" && "$lust" fix a.rs --all --dry-run) 2>&1 ); rc=$?
is "assert: dry run exits 1" $rc 1
case "$err" in *"would fix"*"a.rs:6:5: L0809"*) ok "assert: dry run names the edit";; *) bad "assert: dry run names the edit" "$err";; esac
is "assert: dry run writes nothing" "$(cat "$d/a.rs")" "$before"
err=$( (cd "$d" && "$lust" fix a.rs --all) 2>&1 ); rc=$?
is "assert: fix --all exits 0" $rc 0
case "$err" in *"fixed"*"a.rs:6:5: L0809"*"fixed 1 diagnostic"*) ok "assert: fix --all reports the edit";; *) bad "assert: fix --all reports the edit" "$err";; esac
same "assert: fixed program matches rustc" "$d/a.rs"
(cd "$d" && "$lust" fix a.rs --all) >/dev/null 2>&1; is "assert: a second fix has nothing to do, exit 0" $? 0
# The message when the assert fails: the same text under both compilers.
cat >"$d/f.rs" <<'RS'
fn main() {
    let (first_value, second_value, third_value) = (1, 2, 4);
    assert!(
        first_value == 1 && second_value == 2 && third_value == 3 && "{}".len() == 2 && first_value + second_value == 3,
    );
    println!("unreachable");
}
RS
(cd "$d" && "$lust" fix f.rs --all) >/dev/null 2>&1; is "assert: multi-line call with a trailing comma, exit 0" $? 0
same "assert: failing fixed assert prints the same panic" "$d/f.rs"
case "$(cat "$d/f.rs")" in *'"assertion failed: first_value == 1 && second_value == 2 && third_value == 3 && \"{{}}\".len() == 2 && first_value + second_value == 3",'*) ok "assert: the message is the condition on one line";; *) bad "assert: the message is the condition on one line" "$(cat "$d/f.rs")";; esac

# ------------------------------- write! into a user io::Write type (L0641)
d=$work/write; mkdir -p "$d"
cp "$root/tests/reject/std_io_write_user_type.rs" "$d/w.rs"
(cd "$d" && "$lust" check w.rs) >/dev/null 2>&1; is "write: the reject fails its check" $? 1
(cd "$d" && "$lust" fix w.rs --all) >/dev/null 2>&1; is "write: fix --all exits 0" $? 0
case "$(cat "$d/w.rs")" in *'let _ = c.write_all(format!("a{}b{}", 1, 2).as_bytes());'*) ok "write: the call becomes one write_all";; *) bad "write: the call becomes one write_all" "$(cat "$d/w.rs")";; esac
same "write: fixed program matches rustc" "$d/w.rs"
cat >"$d/m.rs" <<'RS'
use std::io::{self, Write};
struct Sink {
    bytes: Vec<u8>,
    calls: usize,
}
impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.calls += 1;
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Report {
    out: Sink,
}
impl Report {
    fn line(&mut self, name: &str, n: u32) -> io::Result<()> {
        write!(self.out, "{name}=")?;
        writeln!(&mut self.out, "{n:>4} \"{}\"", n * 2)?;
        writeln!(self.out)
    }
}
fn main() {
    let mut r = Report { out: Sink { bytes: Vec::new(), calls: 0 } };
    r.line("a", 7).unwrap();
    r.line("b", 12).unwrap();
    print!("{}", String::from_utf8(r.out.bytes.clone()).unwrap());
    println!("{} calls", r.out.calls);
}
RS
err=$( (cd "$d" && "$lust" fix m.rs --all) 2>&1 ); rc=$?
is "write: write!, writeln! and an empty writeln!, exit 0" $rc 0
case "$err" in *"fixed 3 diagnostic"*) ok "write: three calls in one round";; *) bad "write: three calls in one round" "$err";; esac
same "write: fixed methods match rustc" "$d/m.rs"

# --------------------------------------------- auto edits (numeric casts)
d=$work/auto; mkdir -p "$d"
cat >"$d/n.rs" <<'RS'
fn main() {
    let small: i32 = 7;
    let wide: i64 = small;
    let ratio: f64 = 2;
    println!("{wide} {ratio}");
}
RS
err=$( (cd "$d" && "$lust" fix n.rs) 2>&1 ); rc=$?
is "auto: plain fix applies auto edits, exit 0" $rc 0
# A cast is two edits of one diagnostic (`(` before and `) as T` after).
case "$err" in *"fixed 2 diagnostic"*) ok "auto: the cast and the float literal";; *) bad "auto: the cast and the float literal" "$err";; esac
case "$(cat "$d/n.rs")" in *'let wide: i64 = (small) as i64;'*'let ratio: f64 = 2.0;'*) ok "auto: the fixed text";; *) bad "auto: the fixed text" "$(cat "$d/n.rs")";; esac
same "auto: fixed program matches rustc" "$d/n.rs"

# ------------------------------------- lifetime arguments on a fn (L0731)
d=$work/lifetime; mkdir -p "$d"
cp "$root/tests/reject/lifetime_turbofish_early.rs" "$d/a.rs"
cp "$root/tests/reject/method_turbofish_lifetime.rs" "$d/b.rs"
for f in a b; do
  (cd "$d" && "$lust" fix $f.rs) >/dev/null 2>&1; is "lifetime: plain fix applies the auto edit ($f), exit 0" $? 0
  same "lifetime: fixed program matches rustc ($f)" "$d/$f.rs"
done
case "$(cat "$d/a.rs")" in *'pick::<'"'"'_>(&v) + pick(&5)'*) ok "lifetime: the turbofish goes, an inferred one stays";; *) bad "lifetime: the turbofish goes, an inferred one stays" "$(cat "$d/a.rs")";; esac
cat >"$d/c.rs" <<'RS'
fn pick<'a: 'a, T>(x: &'a T) -> &'a T {
    x
}
fn two<'a: 'a, 'b: 'b>(x: &'a i32, _y: &'b i32) -> &'a i32 {
    x
}
fn main() {
    println!("{}", pick::<'static, i32>(&5) + two::<'static, 'static>(&1, &2) + pick::< 'static , i32 >(&3));
}
RS
(cd "$d" && "$lust" fix c.rs) >/dev/null 2>&1; is "lifetime: beside type arguments and two in one list, exit 0" $? 0
case "$(cat "$d/c.rs")" in *'pick::<i32>(&5) + two(&1, &2) + pick::< i32 >(&3)'*) ok "lifetime: the fixed text";; *) bad "lifetime: the fixed text" "$(cat "$d/c.rs")";; esac
same "lifetime: fixed list forms match rustc" "$d/c.rs"

# ------------------------------------------ #![no_implicit_prelude] (L0522)
d=$work/prelude; mkdir -p "$d"
printf '#![no_implicit_prelude]\nfn main() {\n    ::std::println!("{}", ::std::vec::Vec::<u8>::new().len());\n}\n' >"$d/p.rs"
(cd "$d" && "$lust" fix p.rs) >/dev/null 2>&1; is "prelude: not an auto edit, exit 1" $? 1
(cd "$d" && "$lust" fix p.rs --all) >/dev/null 2>&1; is "prelude: fix --all exits 0" $? 0
is "prelude: the attribute's line is gone" "$(head -1 "$d/p.rs")" "fn main() {"
same "prelude: fixed program matches rustc" "$d/p.rs"

# ------------------------------ a closure in a const or static (L0630)
d=$work/closure; mkdir -p "$d"
cat >"$d/s.rs" <<'RS'
static INC: fn(u32) -> u32 = |x| x + 1;
const NAME: fn(&str) -> String = |s| {
    let mut out = String::from("<");
    out.push_str(s);
    out + ">"
};
static TABLE: &[(&str, fn(&mut Vec<u32>, (u32, u32)))] = &[
    ("sum", |v, (a, b)| v.push(a + b)),
    ("both", move |v, (a, b)| {
        v.push(a);
        v.push(b);
    }),
];
fn main() {
    let mut v = Vec::new();
    for (name, f) in TABLE {
        f(&mut v, (INC(1), 5));
        println!("{} {:?}", NAME(name), v);
    }
}
RS
err=$( (cd "$d" && "$lust" fix s.rs --all) 2>&1 ); rc=$?
is "closure: fix --all exits 0" $rc 0
# The const evaluator stops at the first closure of a check: one per round.
case "$err" in *"fixed 4 diagnostic"*) ok "closure: four closures, four rounds";; *) bad "closure: four closures, four rounds" "$err";; esac
src=$(cat "$d/s.rs")
case "$src" in *'static INC: fn(u32) -> u32 = inc_fn;'*'fn inc_fn(x: u32) -> u32 { x + 1 }'*'("sum", table_fn),'*'("both", table_fn2),'*'fn table_fn(v: &mut Vec<u32>, (a, b): (u32, u32)) { v.push(a + b) }'*) ok "closure: the fixed text";; *) bad "closure: the fixed text" "$src";; esac
same "closure: fixed program matches rustc" "$d/s.rs"
printf 'struct S;\nimpl S {\n    const F: fn(u32) -> u32 = |x| x + 1;\n}\nfn main() { println!("{}", (S::F)(1)); }\n' >"$d/i.rs"
i_before=$(cat "$d/i.rs")
(cd "$d" && "$lust" fix i.rs --all) >/dev/null 2>&1; is "closure: an associated const gets no edit, exit 1" $? 1
is "closure: nothing written for it" "$(cat "$d/i.rs")" "$i_before"

# ------------------------------------------------- another file of a crate
d=$work/mods; mkdir -p "$d"
printf 'mod util;\nfn main() { util::check(1, 2, 3); println!("ok"); }\n' >"$d/main.rs"
printf 'pub fn check(first_value: u32, second_value: u32, third_value: u32) {\n    assert!(first_value == 1 && second_value == 2 && third_value == 3 && first_value + second_value == 3);\n}\n' >"$d/util.rs"
main_before=$(cat "$d/main.rs")
(cd "$d" && "$lust" fix main.rs --all) >/dev/null 2>&1; is "mods: fix --all exits 0" $? 0
is "mods: the root file is untouched" "$(cat "$d/main.rs")" "$main_before"
case "$(cat "$d/util.rs")" in *', "assertion failed: first_value == 1 && '*) ok "mods: the edit lands in the module file";; *) bad "mods: the edit lands in the module file" "$(cat "$d/util.rs")";; esac

# ---------------------------------------------- the sandbox rules (--sandbox)
# Each round of `fix --sandbox` is a sandboxed check: a file whose edit is
# fine under a plain check still fails on what the sandbox refuses (L0532).
d=$work/sandbox; mkdir -p "$d"
cat >"$d/s.rs" <<'RS'
fn main() {
    let small: i32 = 7;
    let wide: i64 = small;
    let text = std::fs::read_to_string("in.txt").unwrap_or_default();
    println!("{wide} {}", text.len());
}
RS
cp "$d/s.rs" "$d/plain.rs"
s_before=$(cat "$d/s.rs")
(cd "$d" && "$lust" fix plain.rs) >/dev/null 2>&1; is "sandbox: plain fix of the same file exits 0" $? 0
err=$( (cd "$d" && "$lust" fix s.rs --sandbox --dry-run) 2>&1 ); rc=$?
is "sandbox: dry run exits 1" $rc 1
is "sandbox: dry run writes nothing" "$(cat "$d/s.rs")" "$s_before"
err=$( (cd "$d" && "$lust" fix s.rs --sandbox) 2>&1 ); rc=$?
is "sandbox: fix --sandbox exits 1 on the sandbox refusal" $rc 1
case "$err" in *"fixed"*"s.rs:3:21: L0110"*"error[L0532]"*"std::fs::read_to_string"*"fixed 1 diagnostic"*) ok "sandbox: the edit is applied, then the re-check reports L0532";; *) bad "sandbox: the edit is applied, then the re-check reports L0532" "$err";; esac
case "$(cat "$d/s.rs")" in *'let wide: i64 = (small) as i64;'*) ok "sandbox: the fixed text";; *) bad "sandbox: the fixed text" "$(cat "$d/s.rs")";; esac
(cd "$d" && "$lust" check s.rs --sandbox) >/dev/null 2>&1; is "sandbox: lust check --sandbox agrees, exit 1" $? 1
printf 'use std::io::Read;\nfn main() {\n    let mut s = String::new();\n    std::io::stdin().read_to_string(&mut s).unwrap();\n    println!("{}", s.len());\n}\n' >"$d/ok.rs"
ok_before=$(cat "$d/ok.rs")
(cd "$d" && "$lust" fix ok.rs --sandbox) >/dev/null 2>&1; is "sandbox: fix --sandbox of a clean sandbox-legal file exits 0" $? 0
is "sandbox: nothing written for it" "$(cat "$d/ok.rs")" "$ok_before"
# The limits are for a program lust runs; a check takes none.
(cd "$d" && "$lust" fix ok.rs --sandbox --max-memory 64M) >/dev/null 2>&1; is "sandbox: --max-memory is a usage error, exit 2" $? 2

# ------------------------------------------------------- nothing to offer
d=$work/none; mkdir -p "$d"
printf 'fn main() { let x: u32 = "s"; }\n' >"$d/e.rs"
e_before=$(cat "$d/e.rs")
err=$( (cd "$d" && "$lust" fix e.rs --all) 2>&1 ); rc=$?
is "none: an error with no edit, exit 1" $rc 1
case "$err" in *"error[L0110]"*) ok "none: the check's diagnostics are shown";; *) bad "none: the check's diagnostics are shown" "$err";; esac
is "none: nothing written" "$(cat "$d/e.rs")" "$e_before"
(cd "$d" && "$lust" fix) >/dev/null 2>&1; is "usage: no file is a usage error" $? 2

echo "test-fix: $pass passed, $fail failed"
[ $fail -eq 0 ]
