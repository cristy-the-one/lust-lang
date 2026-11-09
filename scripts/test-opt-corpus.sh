#!/usr/bin/env bash
# Lime Slice plugin corpus under `--opt` (the consumer's byte-identity check).
# Clones cristy-the-one/lime-slice (sparse) and runs mcp/plugin-corpus.test.ts
# with a wrapper that adds `--opt` to every `lust build`. Needs node and git.
# LUST_OPT_CORPUS=1 in CI. Skips with exit 0 when node or git is missing,
# except on a run that set LUST_OPT_CORPUS=1.
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
lust=$root/target/release/lust
if [ ! -x "$lust" ] && [ -x "$lust.exe" ]; then lust=$lust.exe; fi
if [ ! -x "$lust" ]; then
  echo "opt-corpus: no lust binary" >&2
  exit 1
fi
if ! command -v node >/dev/null 2>&1 || ! command -v git >/dev/null 2>&1; then
  echo "opt-corpus: SKIP (need node and git)" >&2
  if [ "${LUST_OPT_CORPUS:-0}" = 1 ]; then exit 1; fi
  exit 0
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git clone --depth 1 --filter=blob:none --sparse https://github.com/cristy-the-one/lime-slice.git "$work/lime" >&2
git -C "$work/lime" sparse-checkout set mcp crates/lime-recipe src >&2
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    # Node's spawnSync will not run a .cmd (EINVAL without shell:true).
    # A real exe, built with the zig the CI job put on PATH, forwards
    # every argument and inserts --opt on build.
    if ! command -v zig >/dev/null 2>&1; then
      echo "opt-corpus: SKIP (Windows wrapper needs zig cc)" >&2
      if [ "${LUST_OPT_CORPUS:-0}" = 1 ]; then exit 1; fi
      exit 0
    fi
    lust_m=$(cygpath -m "$lust")
    cat >"$work/lust-opt.c" <<EOF
#include <windows.h>
#include <stdio.h>
#include <string.h>
static const char *real = "$lust_m";
static int quote(char *out, int cap, int n, const char *s) {
  int i;
  if (n < cap) out[n++] = '"';
  for (i = 0; s[i] && n + 2 < cap; i++) {
    if (s[i] == '"') { if (n < cap) out[n++] = '\\\\'; }
    out[n++] = s[i];
  }
  if (n < cap) out[n++] = '"';
  return n;
}
int main(int argc, char **argv) {
  char cmd[32768];
  int n = 0, i = 1;
  n = quote(cmd, sizeof cmd, n, real);
  if (argc > 1 && strcmp(argv[1], "build") == 0) {
    n += snprintf(cmd + n, sizeof cmd - n, " build --opt");
    i = 2;
  }
  for (; i < argc; i++) {
    if (n + 1 < (int)sizeof cmd) cmd[n++] = ' ';
    n = quote(cmd, sizeof cmd, n, argv[i]);
  }
  cmd[n < (int)sizeof cmd ? n : (int)sizeof cmd - 1] = 0;
  STARTUPINFOA si;
  PROCESS_INFORMATION pi;
  ZeroMemory(&si, sizeof si);
  si.cb = sizeof si;
  if (!CreateProcessA(real, cmd, NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi)) {
    fprintf(stderr, "lust-opt: cannot start %s (%lu)\\n", real, GetLastError());
    return 127;
  }
  WaitForSingleObject(pi.hProcess, INFINITE);
  {
    DWORD code = 1;
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    return (int)code;
  }
}
EOF
    wrap=$work/lust-opt.exe
    if ! zig cc -O2 -o "$wrap" "$work/lust-opt.c"; then
      echo "opt-corpus: zig cc could not build the Windows wrapper" >&2
      exit 1
    fi
    ;;
  *)
    wrap=$work/lust-opt
    cat >"$wrap" <<EOF
#!/bin/bash
if [ "\${1:-}" = build ]; then
  exec "$lust" build --opt "\${@:2}"
fi
exec "$lust" "\$@"
EOF
    chmod +x "$wrap"
    ;;
esac
cd "$work/lime"
# One daemon for the corpus, in this temp tree, so it dies with the script's
# process group when the shell exits only if we kill it. Kill it ourselves.
export LUST_DAEMON_SOCKET=$work/corpus.sock
export LUST_CACHE_DIR=$work/cache
set +e
LIME_LUST="$wrap" LIME_RECIPE_REQUIRED=1 node --experimental-strip-types mcp/plugin-corpus.test.ts
st=$?
set -e
if [ -d /proc ]; then
  self=$$
  parent=${PPID:-0}
  for d in /proc/[0-9]*; do
    [ -r "$d/environ" ] || continue
    pid=$(basename "$d")
    # This shell and the timeout that waits on it carry the socket variable
    # too. Killing them turns a finished corpus into exit 143.
    [ "$pid" = "$self" ] || [ "$pid" = "$parent" ] && continue
    text=$(cat "$d/environ" 2>/dev/null | tr '\0' '\n') || continue
    printf '%s\n' "$text" | grep -qx "LUST_DAEMON_SOCKET=$work/corpus.sock" && kill "$pid" 2>/dev/null || true
  done
fi
exit "$st"
