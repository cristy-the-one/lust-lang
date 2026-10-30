#!/usr/bin/env bash
# Macro dependency smoke test (ARCH 6.4, W-MACRO phase 2): a daemon
# session must not serve an expansion that read a since-changed file.
# The program's output comes only from `include_str!` and `include!`
# reads, so a rebuild with no source edit at all must still change
# its answer once those files change. A deleted include must turn
# the answer into the L0803 error, not a stale success.
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
if [ ! -x "$lust" ]; then
  echo "error: $lust not built. Run cargo build --release first."
  exit 2
fi
unset LUST_DAEMON_SOCKET LUST_CACHE_DIR LUST_NO_DAEMON LUST_NO_CACHE
work=$(mktemp -d)
pass=0
fail=0
ok() { pass=$((pass + 1)); echo "ok   $1"; }
bad() { fail=$((fail + 1)); echo "FAIL $1: $2"; }

sock() { local f; for f in "$1"/.lust-daemon-*.sock "$1"/.lust-daemon-*.sock.lock; do [ -e "$f" ] && { echo "${f%.lock}"; return; }; done; echo "$1/.lust-daemon-none.sock"; }
stop_daemon() {
  local pid
  pid=$(cat "$(sock "$1").lock" 2>/dev/null) || return 0
  [ -n "$pid" ] && kill "$pid" 2>/dev/null
  return 0
}
cleanup() {
  stop_daemon "$work/d"
  rm -rf "$work"
}
trap cleanup EXIT
wait_socket() {
  local i
  for i in $(seq 1 100); do [ -S "$(sock "$1")" ] && return 0; sleep 0.05; done
  return 1
}

d=$work/d
mkdir -p "$d"
cat >"$d/main.rs" <<'EOF'
include!("inc.rs");
fn main() { println!("{} {}", include_str!("data.txt"), value()); }
EOF
printf 'fn value() -> i32 { 1 }\n' >"$d/inc.rs"
printf 'one' >"$d/data.txt"

(cd "$d" && "$lust" build main.rs -o h1) >/dev/null 2>&1
if [ "$("$d/h1" 2>/dev/null)" = "one 1" ]; then ok "cold build reads the includes"; else bad "cold build" "want 'one 1', got '$("$d/h1" 2>&1 | head -2)'"; fi
if wait_socket "$d"; then ok "daemon is up"; else bad "daemon" "socket never appeared"; fi
(cd "$d" && "$lust" build main.rs -o h2) >/dev/null 2>&1
if [ "$("$d/h2" 2>/dev/null)" = "one 1" ]; then ok "daemon build repeats the answer"; else bad "daemon build" "want 'one 1', got '$("$d/h2" 2>&1 | head -2)'"; fi

printf 'fn value() -> i32 { 2 }\n' >"$d/inc.rs"
printf 'two' >"$d/data.txt"
(cd "$d" && "$lust" build main.rs -o h3) >/dev/null 2>&1
if [ "$("$d/h3" 2>/dev/null)" = "two 2" ]; then
  ok "edited includes change the daemon answer"
else
  bad "include edit" "want 'two 2', got '$("$d/h3" 2>&1 | head -2)' (stale expansion was reused)"
fi

rm "$d/data.txt"
if (cd "$d" && "$lust" build main.rs -o h4) >"$work/h4.log" 2>&1; then
  bad "deleted include" "build succeeded, want the L0803 error"
elif grep -q L0803 "$work/h4.log"; then
  ok "deleted include is the L0803 error, not a stale success"
else
  bad "deleted include" "build failed without L0803: $(head -2 "$work/h4.log")"
fi

# A chunk that baked in a line! value must not be reused when lines
# above it move it to a different start line.
mkdir -p "$work/line"
printf 'fn main() { println!("{}", line!()); }\n' >"$work/line/main.rs"
(cd "$work/line" && "$lust" run main.rs) >/dev/null 2>&1
for i in $(seq 1 100); do [ -S "$(sock "$work/line")" ] && break; sleep 0.05; done
w1=$(cd "$work/line" && "$lust" run main.rs 2>/dev/null)
printf '\n\nfn main() { println!("{}", line!()); }\n' >"$work/line/main.rs"
w2=$(cd "$work/line" && "$lust" run main.rs 2>/dev/null)
stop_daemon "$work/line"
if [ "$w1" = "1" ] && [ "$w2" = "3" ]; then
  ok "line! follows a moved chunk"
else
  bad "line! after blank lines" "want 1 then 3, got '$w1' then '$w2' (stale baked line)"
fi

# env! and file! read the client, not the daemon (daemon_env_stale,
# file_macro_absolute_path): the daemon starts under one env, and each
# client's env and typed root path must give its own cold answer.
e=$work/env
mkdir -p "$e/sub"
cat >"$e/sub/main.rs" <<'EOF'
mod m;
fn main() {
    println!("{} {:?} {}", env!("LUST_DEPS_BAR"), option_env!("LUST_DEPS_FOO"), file!());
    m::f();
}
EOF
printf 'pub fn f() { println!("{}", file!()); }\n' >"$e/sub/m.rs"
grep -v 'm::f\|mod m' "$e/sub/main.rs" >"$e/one.rs"
(cd "$e" && env -u LUST_DEPS_FOO LUST_DEPS_BAR=daemon LUST_DAEMON_SOCKET="$e/d.sock" "$lust" daemon 2>/dev/null) &
for i in $(seq 1 100); do [ -S "$e/d.sock" ] && break; sleep 0.05; done
envrun() { (cd "$e/$1" && shift && export LUST_DAEMON_SOCKET="$e/d.sock" && env "$@" 2>&1); }
for f in one.rs sub/main.rs; do
  for args in "LUST_DEPS_BAR=a LUST_DEPS_FOO=b|.|$f" "-u LUST_DEPS_FOO LUST_DEPS_BAR=c|.|./$f" "LUST_DEPS_BAR=d|$(dirname "$f")|$(basename "$f")"; do
    IFS='|' read -r vars dir path <<<"$args"
    # shellcheck disable=SC2086
    warm=$(envrun "$dir" $vars "$lust" run "$path")
    # shellcheck disable=SC2086
    cold=$(envrun "$dir" $vars LUST_NO_DAEMON=1 LUST_NO_CACHE=1 "$lust" run "$path")
    if [ "$warm" = "$cold" ] && grep -q " $path$" <<<"$warm" && ! grep -q "daemon\|$e" <<<"$warm"; then
      ok "env! and file! follow the client ($vars, $path)"
    else
      bad "client env/path ($vars, $path)" "warm '$warm' cold '$cold'"
    fi
  done
done
unset_out=$(envrun . -u LUST_DEPS_BAR "$lust" check one.rs)
if grep -q L0806 <<<"$unset_out"; then
  ok "env! of a var only the daemon has is L0806"
else
  bad "env! unset in client" "want L0806, got: $(head -2 <<<"$unset_out")"
fi
kill "$(cat "$e/d.sock.lock" 2>/dev/null)" 2>/dev/null

echo "test-macro-deps: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
