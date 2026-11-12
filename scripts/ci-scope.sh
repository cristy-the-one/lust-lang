#!/usr/bin/env bash
# Decides whether a CI run needs the full gate (scripts/ci.sh on every
# platform, about two hours) or can skip it. Prints `heavy=true` or
# `heavy=false`, then `reason=<one line>`. .github/workflows/ci.yml runs it
# in its first job and the gate jobs read the answer.
#
# The gate is skipped in two cases only:
#
# 1. A push that is the merge of a pull request whose branch already held
#    everything on the base branch, when the pull request's own CI run
#    passed. The merge then has exactly the files that run tested.
# 2. A change that touches nothing the gate reads: the plan and guide
#    documents listed in `is_document`, and a version change (the `version`
#    line of Cargo.toml and the same line of this package in Cargo.lock).
#
# Anything else, and anything this script cannot decide, runs the gate.
#
# Environment:
#   CI_EVENT   pull_request, push or workflow_dispatch
#   CI_BEFORE  for a push, the commit the branch pointed at before
#   CI_REPO    owner/name, for the GitHub API (case 1)
#   GH_TOKEN   a token that can read Actions runs (case 1)
# Run it in a checkout with full history (fetch-depth: 0). For a pull
# request, HEAD is GitHub's merge commit of the branch into its base.
set -u

answer() {
  echo "heavy=$1"
  echo "reason=$2"
  exit 0
}

# The documents no build step, test or package script of the gate reads.
# docs/errors/ is not here: `lust explain` compiles those pages in.
# library/ is not here: it ships and is read at run time.
is_document() {
  case "$1" in
    AGENTS.md|CLAUDE.md|DESIGN.md|README.md) return 0 ;;
    docs/errors/*) return 1 ;;
    docs/*.md|docs/*.html) return 0 ;;
  esac
  return 1
}

# The changed lines of one file between two commits, without the headers.
changed_lines() {
  git diff -U0 "$1" "$2" -- "$3" | grep -E '^[+-]' | grep -vE '^(\+\+\+|---) '
}

# True when `file` changed in one line only, a `version = "..."` line.
# Prints the new version.
only_version_changed() {
  local lines
  lines=$(changed_lines "$1" "$2" "$3")
  [ "$(printf '%s\n' "$lines" | wc -l)" -eq 2 ] || return 1
  printf '%s\n' "$lines" | grep -qE '^-version = "[^"]+"\r?$' || return 1
  printf '%s\n' "$lines" | sed -n 's/^+version = "\([^"]*\)".*$/\1/p' | grep .
}

event=${CI_EVENT:-}
head=$(git rev-parse HEAD) || answer true "no HEAD commit"

case "$event" in
  pull_request)
    base=$(git rev-parse --verify -q 'HEAD^1') || answer true "the merge commit has no parent"
    ;;
  push)
    base=${CI_BEFORE:-}
    base=$(git rev-parse --verify -q "$base^{commit}" 2>/dev/null) || answer true "the commit before this push is not in the checkout"
    # Case 1: the merge of an up-to-date pull request that passed.
    if second=$(git rev-parse --verify -q 'HEAD^2') && ! git rev-parse --verify -q 'HEAD^3' >/dev/null; then
      first=$(git rev-parse 'HEAD^1')
      if [ "$first" = "$base" ] \
        && [ "$(git rev-parse 'HEAD^{tree}')" = "$(git rev-parse "$second^{tree}")" ] \
        && git merge-base --is-ancestor "$first" "$second"; then
        passed=$(gh api "repos/${CI_REPO:-}/actions/workflows/ci.yml/runs?head_sha=$second&event=pull_request&status=success&per_page=1" --jq '.total_count' 2>/dev/null)
        case "$passed" in
          ''|0) ;;
          *) answer false "the merge has the same files as ${second:0:8}, which passed in its pull request" ;;
        esac
      fi
    fi
    ;;
  *)
    answer true "a run started by hand always runs the gate"
    ;;
esac

# Case 2: documents and a version change only.
files=$(git diff --name-only "$base" "$head") || answer true "cannot list the changed files"
[ -n "$files" ] || answer false "no file changed"
version_toml=
version_lock=
while IFS= read -r file; do
  case "$file" in
    Cargo.toml)
      version_toml=$(only_version_changed "$base" "$head" Cargo.toml) || answer true "Cargo.toml changed in more than its version"
      ;;
    Cargo.lock)
      version_lock=$(only_version_changed "$base" "$head" Cargo.lock) || answer true "Cargo.lock changed in more than one version line"
      ;;
    *)
      is_document "$file" || answer true "$file is read by the gate"
      ;;
  esac
done <<EOF
$files
EOF
[ "$version_toml" = "$version_lock" ] || answer true "Cargo.toml and Cargo.lock do not name the same new version"
if [ -n "$version_toml" ]; then
  answer false "only the version ($version_toml) and documents changed"
fi
answer false "only documents changed"
