# Shared OS checks for the shell gate. Source after $root is set.
# A daemon endpoint is a Unix socket, or on Windows the address file the
# daemon writes at the same path (127.0.0.1:<port>).
is_daemon_endpoint() {
  [ -S "$1" ] && return 0
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) [ -f "$1" ] ;;
    *) return 1 ;;
  esac
}

# Process checks that work on Windows too. The daemon writes its own pid to the
# lock file; on Windows that is a Windows pid, which Git bash's `kill` (MSYS
# pids) does not know: `kill -0` reports it dead at once and `kill -9` misses.
pid_alive() { # pid_alive <pid>
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) tasklist //FI "PID eq $1" //NH 2>/dev/null | grep -q "[[:space:]]$1[[:space:]]" ;;
    *) kill -0 "$1" 2>/dev/null ;;
  esac
}
kill_hard() { # kill_hard <pid> : SIGKILL, or TerminateProcess on Windows
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) taskkill //F //PID "$1" >/dev/null 2>&1 ;;
    *) kill -9 "$1" 2>/dev/null ;;
  esac
}
self_pid() { # self_pid : this shell's pid as the daemon would see it
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) cat "/proc/$$/winpid" 2>/dev/null || echo "$$" ;;
    *) echo "$$" ;;
  esac
}
