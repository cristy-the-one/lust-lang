# timeout-lib.sh: the one timeout rule for every gate script (BRIEF: a timeout is
# never a reject). Source it after setting $case_timeout (seconds per lust compile).
#
# The rule (as in scripts/parse-fuzz.sh): a lust compile that times out is retried
# once, alone, with a much longer limit ($retry_timeout, default 6x $case_timeout;
# set RETRY_CASE_TIMEOUT to change it). Only a second timeout is a HANG, which the
# caller must count as a failure of the whole script. A retry that finishes (pass
# or clean reject) is reported by the caller as "slow", not as a failure, and its
# result is the case's result.
#
# Serial callers use lust_cc below (serial, so the retry is alone). Parallel
# runners cannot retry inside the pool: scripts/case-pool.sh and
# scripts/slice-check.sh defer the retry until after their pool drains, but use
# the same limits and the same slow/hang words.

retry_timeout=${RETRY_CASE_TIMEOUT:-$((case_timeout * 6))}

# is_timeout <status>: true for a timeout (124 from `timeout`) or a kill after
# one (137 = SIGKILL, for example from the OOM killer under load). Other signal
# deaths (a segfault is 139) are crashes, not timeouts, and fail at once.
is_timeout() { [ "$1" = 124 ] || [ "$1" = 137 ]; }

# lust_cc <cmd...>: run one lust compile under $case_timeout. On a timeout, run
# it once more under $retry_timeout and set lust_cc_slow=1 (the caller prints
# its "slow:" line with the case name). Returns the final exit status; a status
# is_timeout accepts after lust_cc is a hang. Stdout and stderr pass through, so
# callers redirect lust_cc itself as they redirected the old timeout call.
lust_cc_slow=0
lust_cc() {
  timeout "$case_timeout" "$@"
  local st=$?
  lust_cc_slow=0
  if is_timeout "$st"; then
    lust_cc_slow=1
    timeout "$retry_timeout" "$@"
    st=$?
  fi
  return "$st"
}
