#!/usr/bin/env bash
# Tests for `--mark-unread <fn path>` (src/driver/unread.rs): the calls of a
# named fn whose result the program never reads, as `unread_result` lines of
# a `--json` answer. Run after `cargo build --release`.
#   tests/unread/shapes.rs marks each call that must be listed with a
#   `// unread` comment on the line the call reports (its caller location).
#   The list must be those lines and no others: from a cold check, a cold
#   build, and a warm daemon before and after an edit. tests/unread/dep
#   names a method of a dependency crate by its public path.
# Usage: scripts/test-unread.sh
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
win=0
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) win=1; [ -f "$lust.exe" ] && lust=$lust.exe ;;
esac
if [ ! -x "$lust" ] && [ ! -f "$lust" ]; then
  echo "error: $lust not built. Run cargo build --release first."
  exit 2
fi
unset LUST_DAEMON_SOCKET LUST_CACHE_DIR LUST_NO_DAEMON LUST_NO_CACHE
work=$(mktemp -d)
pass=0
fail=0
ok() { pass=$((pass + 1)); echo "ok   $1"; }
bad() { fail=$((fail + 1)); echo "FAIL $1: $2"; }
is() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "got: $2, want: $3"; fi; }
# The lines a `--json` answer lists, in order, as "L L L".
listed() { grep '"kind":"unread_result"' | sed -E 's/.*"span":\{"start":[0-9]+,"end":[0-9]+,"line":([0-9]+).*/\1/' | tr '\n' ' ' | sed 's/ $//'; }
# The lines a source marks.
marked() { grep -n 'apply(.*// unread' "$1" | cut -d: -f1 | tr '\n' ' ' | sed 's/ $//'; }
stop_daemon() {
  local f pid
  for f in "$1"/.lust-daemon-*.sock.lock; do
    [ -e "$f" ] || continue
    pid=$(cat "$f" 2>/dev/null)
    if [ "$win" = 1 ]; then rm -f "${f%.lock}"; else [ -n "$pid" ] && kill "$pid" 2>/dev/null; fi
  done
}
trap 'stop_daemon "$work/warm"; rm -rf "$work"' EXIT

d=$work/cold; mkdir -p "$d"; cp "$root/tests/unread/shapes.rs" "$d/"
want=$(marked "$d/shapes.rs")
export LUST_NO_DAEMON=1 LUST_NO_CACHE=1
out=$( (cd "$d" && "$lust" check shapes.rs --json --mark-unread Session::apply) 2>/dev/null ); rc=$?
is "cold check: exit 0" $rc 0
is "cold check: the marked lines, no others" "$(echo "$out" | listed)" "$want"
case "$out" in '{"schema":1,"kind":"unread_result","fn":"Session::apply","file":'*) ok "cold check: the record's shape";; *) bad "cold check: the record's shape" "$(echo "$out" | head -1)";; esac
out=$( (cd "$d" && "$lust" build shapes.rs -o "$d/shapes" --json --mark-unread=Session::apply) 2>/dev/null ); rc=$?
is "cold build: exit 0" $rc 0
is "cold build: the marked lines, no others" "$(echo "$out" | listed)" "$want"
out=$( (cd "$d" && "$lust" check shapes.rs --json) 2>/dev/null )
is "no flag: no record" "$(echo "$out" | listed)" ""
# The names sort by bytes here: a locale's order differs between systems.
out=$( (cd "$d" && "$lust" check shapes.rs --json --mark-unread Session::other --mark-unread helper) 2>/dev/null )
is "two names: each fn's unread calls" "$(echo "$out" | grep -o '"fn":"[A-Za-z:]*"' | LC_ALL=C sort | uniq -c | tr -s ' ' | tr '\n' ';')" ' 1 "fn":"Session::other"; 1 "fn":"helper";'
out=$( (cd "$d" && "$lust" check shapes.rs --json --mark-unread Nobody::apply) 2>/dev/null ); rc=$?
is "a name nothing has: exit 0, no record" "$rc $(echo "$out" | listed)" "0 "
(cd "$d" && "$lust" check shapes.rs --mark-unread Session::apply) >/dev/null 2>&1; is "usage: needs --json" $? 2
(cd "$d" && "$lust" check shapes.rs --json --mark-unread 'Session::apply(') >/dev/null 2>&1; is "usage: a path only" $? 2
# A program with an error lists nothing: its answer is the diagnostics.
sed 's|let r = s.apply("n")?; // read|let r: bool = s.apply("n")?;|' "$d/shapes.rs" >"$d/broken.rs"
out=$( (cd "$d" && "$lust" check broken.rs --json --mark-unread Session::apply) 2>/dev/null ); rc=$?
is "an error: exit 1, no record" "$rc $(echo "$out" | listed)" "1 "

# A method of a dependency crate, named by its public path.
d=$work/dep; cp -r "$root/tests/unread/dep" "$d"
out=$( (cd "$d" && "$lust" check src/main.rs --manifest-path Cargo.toml --json --mark-unread kit::Session::apply) 2>/dev/null ); rc=$?
is "dep: exit 0" $rc 0
is "dep: one record per file" "$(echo "$out" | grep -o '"file":"[^"]*"' | sed -E 's/.*[\\/]([a-z]+\.rs)"/\1/' | tr '\n' ' ')" "main.rs session.rs "
is "dep: the root crate's line" "$(echo "$out" | grep 'main.rs' | listed)" "$(marked "$d/src/main.rs")"
unset LUST_NO_DAEMON LUST_NO_CACHE

# A warm daemon: the same list, and the list of the edited text after an edit.
d=$work/warm; mkdir -p "$d"; cp "$root/tests/unread/shapes.rs" "$d/"
(cd "$d" && "$lust" check shapes.rs) >/dev/null 2>&1
for i in $(seq 1 100); do ls "$d"/.lust-daemon-*.sock >/dev/null 2>&1 && break; sleep 0.05; done
out=$( (cd "$d" && "$lust" check shapes.rs --json --mark-unread Session::apply) 2>/dev/null )
is "warm check: the marked lines" "$(echo "$out" | listed)" "$want"
out=$( (cd "$d" && "$lust" check shapes.rs --json --mark-unread Session::apply) 2>/dev/null )
is "warm check again (nothing changed): the marked lines" "$(echo "$out" | listed)" "$want"
# One call's result is now read, and every line below moves down one.
sed 's|    s.apply("a"); // unread|    let first = s.apply("a")?; // read\n    println!("{first}");|' "$d/shapes.rs" >"$d/next.rs" && mv "$d/next.rs" "$d/shapes.rs"
out=$( (cd "$d" && "$lust" check shapes.rs --json --mark-unread Session::apply) 2>/dev/null ); rc=$?
is "warm check after an edit: exit 0" $rc 0
is "warm check after an edit: the new marked lines" "$(echo "$out" | listed)" "$(marked "$d/shapes.rs")"
out=$( (cd "$d" && "$lust" build shapes.rs -o "$d/shapes" --json --mark-unread Session::apply) 2>/dev/null ); rc=$?
is "warm build: exit 0" $rc 0
is "warm build: the new marked lines" "$(echo "$out" | listed)" "$(marked "$d/shapes.rs")"

echo "test-unread: $pass passed, $fail failed"
[ $fail -eq 0 ]
