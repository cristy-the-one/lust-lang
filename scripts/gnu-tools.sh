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
    # SIGTERM does not stop a native Windows process, and it does not
    # interrupt Git bash while that bash is blocked in `wait` or in a
    # read of a pipe the child still holds. Kill by pid, then by tree.
    _gt_kill_tree() {
      local pid=$1 win
      case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*)
          win=$(cat "/proc/$pid/winpid" 2>/dev/null || true)
          [ -n "$win" ] || win=$pid
          taskkill //F //T //PID "$win" >/dev/null 2>&1 || true
          ;;
        *)
          kill "$pid" 2>/dev/null || true
          sleep 1
          kill -9 "$pid" 2>/dev/null || true
          ;;
      esac
    }
    # A small stand-in for GNU timeout. Exit 124 when this function kills
    # the command; the command's own status otherwise (an abort is not a
    # timeout). The wait is polled: a signal sent from another process
    # never has to interrupt it. Errexit is restored to whatever the
    # caller had: forcing `set -e` on the way out aborts scripts that
    # record a non-zero status.
    timeout() {
      local secs=$1
      shift
      local flag pid st start now errexit=0
      case $- in *e*) errexit=1 ;; esac
      flag=$(mktemp)
      "$@" &
      pid=$!
      start=$(date +%s)
      set +e
      while kill -0 "$pid" 2>/dev/null; do
        now=$(date +%s)
        if [ $((now - start)) -ge "$secs" ]; then
          echo 1 >"$flag"
          _gt_kill_tree "$pid"
          break
        fi
        sleep 1
      done
      # The process was killed above, or it has already exited. This wait
      # reaps it. It is not the thing that enforces the bound.
      wait "$pid"
      st=$?
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
