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
    wrap=$work/lust-opt.cmd
    # %* does not see `shift`. Drop the leading "build ".
    lust_win=$(cygpath -w "$lust")
    cat >"$wrap" <<EOF
@echo off
setlocal EnableDelayedExpansion
if /I "%~1"=="build" (
  set "REST=%*"
  set "REST=!REST:~6!"
  "$lust_win" build --opt !REST!
  exit /b !ERRORLEVEL!
)
"$lust_win" %*
exit /b %ERRORLEVEL%
EOF
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
  for d in /proc/[0-9]*; do
    [ -r "$d/environ" ] || continue
    # cat, not a shell redirect: a refused /proc open is cat's error, and
    # the redirect on the redirect itself is what bash prints.
    text=$(cat "$d/environ" 2>/dev/null | tr '\0' '\n') || continue
    printf '%s\n' "$text" | grep -qx "LUST_DAEMON_SOCKET=$work/corpus.sock" && kill "$(basename "$d")" 2>/dev/null || true
  done
fi
exit "$st"
