# GNU `timeout` and `nproc` when the shell would otherwise find a different
# tool. On Windows, `timeout` is timeout.exe (a different program) and
# `nproc` is absent. A GNU timeout already on PATH is kept wherever it
# lives: Linux uses /usr/bin/timeout, and Homebrew on macOS puts it under
# gnubin. Replacing that binary with the stand-in below used to turn
# `set -e` on for the rest of the script, so test-adapters.sh died on the
# first non-zero grep inside a substitution and the gate reported only
# "exit 1".
if ! command -v nproc >/dev/null 2>&1; then
  nproc() { getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4; }
fi
# Already the stand-in from an earlier source of this file.
if [ "$(type -t timeout 2>/dev/null || true)" != function ]; then
  _gt_timeout=$(command -v timeout 2>/dev/null || true)
  _gt_keep=0
  case "$_gt_timeout" in
    *.exe|*[Ww]indows/System32/*|*[Ww]indows/system32/*) ;;
    "") ;;
    *)
      # Probe the binary, not a function. Windows timeout.exe is excluded
      # above: `timeout --version` there is not a version check.
      if "$_gt_timeout" --version 2>&1 | grep -q GNU; then
        _gt_keep=1
      fi
      ;;
  esac
  if [ "$_gt_keep" -eq 0 ]; then
    # A small stand-in for GNU timeout. Exit 124 when this function kills
    # the command; the command's own status otherwise (an abort is not a
    # timeout). Errexit is restored to whatever the caller had: forcing
    # `set -e` on the way out aborts scripts that record a non-zero status.
    timeout() {
      local secs=$1
      shift
      local flag pid watcher st errexit=0
      case $- in *e*) errexit=1 ;; esac
      flag=$(mktemp)
      "$@" &
      pid=$!
      (
        sleep "$secs"
        if kill -0 "$pid" 2>/dev/null; then
          echo 1 >"$flag"
          kill "$pid" 2>/dev/null || true
        fi
      ) &
      watcher=$!
      set +e
      wait "$pid"
      st=$?
      kill "$watcher" 2>/dev/null
      wait "$watcher" 2>/dev/null
      if [ "$errexit" -eq 1 ]; then set -e; fi
      if [ -s "$flag" ]; then
        rm -f "$flag"
        return 124
      fi
      rm -f "$flag"
      return "$st"
    }
  fi
  unset _gt_timeout _gt_keep
fi
