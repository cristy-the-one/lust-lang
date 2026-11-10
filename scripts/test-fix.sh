#!/usr/bin/env bash
# Tests for `lust fix` and the rewrites for rejects (src/fix.rs, src/rewrite.rs).
# Run after `cargo build --release`.
#   A reject is valid Rust that lust refuses. Where the diagnostic carries an
#   edit, `lust fix --all` must turn the file into Rust that lust accepts, and
#   the fixed program must give the same stdout, stderr and exit code under
#   rustc and under lust (rule zero). Plain `lust fix` applies only the `auto`
#   edits, and `--dry-run` writes nothing.
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
case "$err" in *"fixed"*"a.rs:6:5: L0809"*"applied 1 edit"*) ok "assert: fix --all reports the edit";; *) bad "assert: fix --all reports the edit" "$err";; esac
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
case "$err" in *"applied 3 edit"*) ok "write: three edits in one round";; *) bad "write: three edits in one round" "$err";; esac
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
# A cast is two edits (`(` before and `) as T` after); the `.0` is one.
case "$err" in *"applied 3 edit"*) ok "auto: the cast and the float literal";; *) bad "auto: the cast and the float literal" "$err";; esac
case "$(cat "$d/n.rs")" in *'let wide: i64 = (small) as i64;'*'let ratio: f64 = 2.0;'*) ok "auto: the fixed text";; *) bad "auto: the fixed text" "$(cat "$d/n.rs")";; esac
same "auto: fixed program matches rustc" "$d/n.rs"

# ------------------------------------------------- another file of a crate
d=$work/mods; mkdir -p "$d"
printf 'mod util;\nfn main() { util::check(1, 2, 3); println!("ok"); }\n' >"$d/main.rs"
printf 'pub fn check(first_value: u32, second_value: u32, third_value: u32) {\n    assert!(first_value == 1 && second_value == 2 && third_value == 3 && first_value + second_value == 3);\n}\n' >"$d/util.rs"
main_before=$(cat "$d/main.rs")
(cd "$d" && "$lust" fix main.rs --all) >/dev/null 2>&1; is "mods: fix --all exits 0" $? 0
is "mods: the root file is untouched" "$(cat "$d/main.rs")" "$main_before"
case "$(cat "$d/util.rs")" in *', "assertion failed: first_value == 1 && '*) ok "mods: the edit lands in the module file";; *) bad "mods: the edit lands in the module file" "$(cat "$d/util.rs")";; esac

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
