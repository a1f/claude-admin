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

set -eo pipefail

die() { echo "pr-babysit/run.sh: $*" >&2; exit 1; }

command -v gh >/dev/null || die "gh CLI not on PATH"
command -v jq >/dev/null || die "jq not on PATH"

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
  PR_NUM=$(gh pr view --json number -q '.number' 2>/dev/null) \
    || die "no open PR on current branch; pass --pr=NUM or run /make-pr first"
fi
[[ -n "$PR_NUM" ]] || die "--pr=NUM required for $MODE"

REPO_JSON=$(gh repo view --json owner,name)
OWNER=$(jq -r '.owner.login' <<<"$REPO_JSON")
REPO=$(jq -r '.name'         <<<"$REPO_JSON")

if [[ "$MODE" == "setup" ]]; then
  PR_JSON=$(gh pr view "$PR_NUM" \
    --json number,url,baseRefName,mergeable,mergeStateStatus,createdAt,body)
  CHECKS_JSON=$(gh pr checks "$PR_NUM" --json name,state,conclusion,detailsUrl 2>/dev/null || echo '[]')

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

INLINE=$(gh api --paginate "repos/$OWNER/$REPO/pulls/$PR_NUM/comments" \
  -q "[.[] | select(.created_at > \"$SINCE\")
       | {id, path, line, body, commit_id,
          user: .user.login, user_type: .user.type, created_at, html_url}]")

REVIEWS=$(gh api --paginate "repos/$OWNER/$REPO/pulls/$PR_NUM/reviews" \
  -q "[.[] | select((.submitted_at // \"\") > \"$SINCE\" and (.body // \"\") != \"\")
       | {id, body, state, user: .user.login, user_type: .user.type,
          submitted_at, html_url}]")

ISSUE_COMMENTS=$(gh api --paginate "repos/$OWNER/$REPO/issues/$PR_NUM/comments" \
  -q "[.[] | select(.created_at > \"$SINCE\")
       | {id, body, user: .user.login, user_type: .user.type, created_at, html_url}]")

MERGE=$(gh pr view "$PR_NUM" --json mergeable,mergeStateStatus,headRefOid)
CHECKS=$(gh pr checks "$PR_NUM" --json name,state,conclusion,detailsUrl 2>/dev/null || echo '[]')
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
