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
