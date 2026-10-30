#!/usr/bin/env bash
# After a failed gate: print ci-failures.txt and, on a pull request, comment
# with it. The FAIL lines are also ::error annotations from the runners.
# Needs pull-requests: write and GH_TOKEN. A missing log is not a second failure.
set -euo pipefail
log=ci-failures.txt
if [ ! -s "$log" ]; then
  echo "no $log (the failing step wrote nothing)"
  exit 0
fi
echo "----- gate failures -----"
head -c 20000 "$log"
echo
if [ "${GITHUB_EVENT_NAME:-}" != pull_request ]; then
  exit 0
fi
if [ -z "${GH_TOKEN:-}" ] || [ -z "${GITHUB_REPOSITORY:-}" ] || [ -z "${PR_NUMBER:-}" ]; then
  echo "not commenting: missing token, repository, or PR number"
  exit 0
fi
body=$(python3 - "$log" << 'PY'
import os, sys
text = open(sys.argv[1], encoding="utf-8", errors="replace").read()[:50000]
job = os.environ.get("GITHUB_JOB", "ci")
sha = os.environ.get("GITHUB_SHA", "")[:7]
print(f"CI gate failures (`{job}`, `{sha}`)\n\n```\n{text}\n```\n")
PY
)
gh api "repos/${GITHUB_REPOSITORY}/issues/${PR_NUMBER}/comments" -f body="$body" >/dev/null
echo "commented on PR ${PR_NUMBER}"
