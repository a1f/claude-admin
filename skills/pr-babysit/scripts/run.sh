#!/usr/bin/env bash
# /pr-babysit skill — thin gh-CLI fetcher.
#
# Usage:
#   run.sh --setup [--pr=NUM]
#   run.sh --poll  --pr=NUM --since=ISO_TIMESTAMP
#
# Modes:
#   --setup   one-shot: returns PR metadata + initial checks + body.
#             {pr_number, pr_url, base_branch, mergeable, merge_state,
#              created_at, owner, repo, body, checks}
#
#   --poll    per-iteration fetch. Returns:
#             {inline_comments, reviews, issue_comments, merge, checks, head_sha}
#
# All other loop logic (state machines, triage, dispatch, escalation) lives
# in skills/pr-babysit/SKILL.md and runs in the caller's context.

set -euo pipefail

die() { echo "pr-babysit/run.sh: $*" >&2; exit 1; }

command -v gh >/dev/null || die "gh CLI not on PATH"
command -v jq >/dev/null || die "jq not on PATH"

# Wrap a gh call with a hard timeout and capture stdout regardless of exit code.
# Distinguishes "gh succeeded with content" from "gh exited non-zero but printed
# valid JSON" (the case for `gh pr checks` when checks are pending/failing) from
# "gh truly errored" (auth, network, 5xx).
#
# Usage: gh_capture <fallback_if_empty> <gh-args...>
# Writes JSON to stdout. Dies with the captured stderr if stdout is empty AND
# exit code is non-zero (real error).
gh_capture() {
  local fallback="$1"; shift
  local out err rc
  err=$(mktemp)
  set +e
  out=$(timeout 30s gh "$@" 2>"$err")
  rc=$?
  set -e
  if [[ -z "$out" && "$rc" -ne 0 ]]; then
    local stderr_content
    stderr_content=$(<"$err")
    rm -f "$err"
    die "gh $1 failed (rc=$rc): $stderr_content"
  fi
  rm -f "$err"
  printf '%s' "${out:-$fallback}"
}

# Paginated gh api fetch that returns a single valid JSON array.
# gh --paginate runs the -q jq filter per page and concatenates outputs; if the
# filter wraps in [...] the result is [...][...] (invalid). We emit objects per
# line and slurp into one array.
#
# Usage: gh_paginate_array <endpoint> <jq-filter-yielding-individual-objects>
gh_paginate_array() {
  local endpoint="$1" filter="$2"
  local out err rc
  err=$(mktemp)
  set +e
  out=$(timeout 60s gh api --paginate "$endpoint" -q "$filter" 2>"$err" | jq -s '.')
  rc=${PIPESTATUS[0]}
  set -e
  if [[ "$rc" -ne 0 ]]; then
    local stderr_content
    stderr_content=$(<"$err")
    rm -f "$err"
    die "gh api --paginate $endpoint failed (rc=$rc): $stderr_content"
  fi
  rm -f "$err"
  printf '%s' "${out:-[]}"
}

MODE=""
PR_NUM=""
SINCE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --setup)    MODE="setup"; shift ;;
    --poll)     MODE="poll";  shift ;;
    --pr=*)     PR_NUM="${1#--pr=}"; shift ;;
    --since=*)  SINCE="${1#--since=}"; shift ;;
    *) die "unknown arg: $1" ;;
  esac
done

[[ -n "$MODE" ]] || die "must specify --setup or --poll"

# Resolve PR number for both modes (poll requires explicit --pr=NUM).
if [[ "$MODE" == "setup" && -z "$PR_NUM" ]]; then
  PR_NUM=$(timeout 30s gh pr view --json number -q '.number' 2>/dev/null) \
    || die "no open PR on current branch; pass --pr=NUM or run /make-pr first"
fi
[[ "$PR_NUM" =~ ^[0-9]+$ ]] || die "PR number missing or non-numeric for $MODE (got: '$PR_NUM')"

REPO_JSON=$(timeout 30s gh repo view --json owner,name)
OWNER=$(jq -r '.owner.login' <<<"$REPO_JSON")
REPO=$(jq -r '.name'         <<<"$REPO_JSON")

if [[ "$MODE" == "setup" ]]; then
  PR_JSON=$(timeout 30s gh pr view "$PR_NUM" \
    --json number,url,baseRefName,mergeable,mergeStateStatus,createdAt,body)
  # gh pr checks exits non-zero when any check is pending/failing — still prints
  # valid JSON to stdout. gh_capture preserves that stdout regardless of rc.
  CHECKS_JSON=$(gh_capture '[]' pr checks "$PR_NUM" --json name,state,conclusion,detailsUrl)

  jq -n \
    --argjson pr "$PR_JSON" \
    --argjson checks "$CHECKS_JSON" \
    --arg owner "$OWNER" \
    --arg repo  "$REPO" \
    '{
       pr_number:   $pr.number,
       pr_url:      $pr.url,
       base_branch: $pr.baseRefName,
       mergeable:   $pr.mergeable,
       merge_state: $pr.mergeStateStatus,
       created_at:  $pr.createdAt,
       body:        $pr.body,
       owner:       $owner,
       repo:        $repo,
       checks:      $checks
     }'
  exit 0
fi

# --poll mode
[[ -n "$SINCE" ]] || die "--since=ISO_TIMESTAMP required for --poll"
[[ "$SINCE" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?Z$ ]] \
  || die "--since must be ISO 8601 UTC (e.g. 2026-05-17T15:42:11Z); got: '$SINCE'"

# Filter by updated_at when present so edited bot comments (e.g. coderabbitai
# editing its summary) get re-triaged. Falls back to created_at for endpoints
# that don't expose updated_at.
INLINE=$(gh_paginate_array "repos/$OWNER/$REPO/pulls/$PR_NUM/comments" \
  ".[] | select(((.updated_at // .created_at) > \"$SINCE\"))
       | {id, path, line, body, commit_id,
          user: .user.login, user_type: .user.type,
          created_at, updated_at, html_url}")

REVIEWS=$(gh_paginate_array "repos/$OWNER/$REPO/pulls/$PR_NUM/reviews" \
  ".[] | select((.submitted_at // \"\") > \"$SINCE\" and (.body // \"\") != \"\")
       | {id, body, state, user: .user.login, user_type: .user.type,
          submitted_at, html_url}")

ISSUE_COMMENTS=$(gh_paginate_array "repos/$OWNER/$REPO/issues/$PR_NUM/comments" \
  ".[] | select(((.updated_at // .created_at) > \"$SINCE\"))
       | {id, body, user: .user.login, user_type: .user.type,
          created_at, updated_at, html_url}")

MERGE=$(timeout 30s gh pr view "$PR_NUM" --json mergeable,mergeStateStatus,headRefOid)
CHECKS=$(gh_capture '[]' pr checks "$PR_NUM" --json name,state,conclusion,detailsUrl)
HEAD_SHA=$(jq -r '.headRefOid' <<<"$MERGE")

jq -n \
  --argjson inline   "$INLINE" \
  --argjson reviews  "$REVIEWS" \
  --argjson issues   "$ISSUE_COMMENTS" \
  --argjson merge    "$MERGE" \
  --argjson checks   "$CHECKS" \
  --arg     head_sha "$HEAD_SHA" \
  '{
     inline_comments: $inline,
     reviews:         $reviews,
     issue_comments:  $issues,
     merge:           $merge,
     checks:          $checks,
     head_sha:        $head_sha
   }'
