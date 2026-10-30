# GNU `timeout` and `nproc` when the shell would otherwise find a different
# tool. On Windows, `timeout` is timeout.exe (a different program) and
# `nproc` is absent. Linux and macOS keep their own binaries: this file
# defines a function only when the GNU one is not already on PATH.
if ! command -v nproc >/dev/null 2>&1; then
  nproc() { getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4; }
fi
_gt_timeout=$(command -v timeout 2>/dev/null || true)
case "$_gt_timeout" in
  /usr/bin/timeout|/bin/timeout) ;;
  *)
    # A small stand-in for GNU timeout. Exit 124 when this function kills
    # the command; the command's own status otherwise (an abort is not a
    # timeout).
    timeout() {
      local secs=$1
      shift
      local flag pid watcher st
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
      # `set -e` would abort the caller when the command's own status is non-zero.
      set +e
      wait "$pid"
      st=$?
      kill "$watcher" 2>/dev/null
      wait "$watcher" 2>/dev/null
      set -e
      if [ -s "$flag" ]; then
        rm -f "$flag"
        return 124
      fi
      rm -f "$flag"
      return "$st"
    }
    ;;
esac
unset _gt_timeout
