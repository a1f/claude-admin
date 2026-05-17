#!/usr/bin/env bash
# build-pr-bundle.sh — gather everything an AI reviewer/critic needs for a PR.
#
# Usage:
#   build-pr-bundle.sh <PR#> [--repo OWNER/NAME] [--out DIR]
#
# Output (prints bundle dir on stdout):
#   <out>/pr-diff.patch     full unified diff (gh pr diff)
#   <out>/pr-context.md     PR body + linked issue body + dispatch task-id hint
#   <out>/repo-map.md       cached lightweight repo structure summary
#   <out>/pr-stats.txt      files changed, +/- LOC, primary languages
#
# Idempotent: rebuilds diff/context/stats; reuses repo-map cache if HEAD unchanged
# and cache < 24h old.

set -eo pipefail
# Note: not using -u (nounset) — bash 3.2 on macOS chokes on "${array[@]}" for empty arrays.

die()  { echo "build-pr-bundle: $*" >&2; exit 1; }
warn() { echo "build-pr-bundle: $*" >&2; }

PR_NUM=""
REPO_FLAG=()
OUT_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) REPO_FLAG=(--repo "$2"); shift 2 ;;
    --out)  OUT_DIR="$2"; shift 2 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    -*)     die "unknown flag: $1" ;;
    *)      [[ -z "$PR_NUM" ]] && PR_NUM="$1" || die "unexpected arg: $1"; shift ;;
  esac
done

[[ -n "$PR_NUM" ]]    || die "PR number required"
command -v gh >/dev/null || die "gh CLI not installed"
command -v jq >/dev/null || die "jq required"

if [[ -z "$OUT_DIR" ]]; then
  OUT_DIR="$(mktemp -d -t "pr-bundle-${PR_NUM}-XXXX")"
fi
mkdir -p "$OUT_DIR"

# ---------- 1. PR diff ----------
gh pr diff "$PR_NUM" "${REPO_FLAG[@]}" > "$OUT_DIR/pr-diff.patch" \
  || die "failed to fetch PR diff"

# ---------- 2. PR metadata + linked issue body ----------
PR_JSON="$(gh pr view "$PR_NUM" "${REPO_FLAG[@]}" \
  --json number,title,url,body,headRefName,author,closingIssuesReferences)"

PR_TITLE=$(jq -r '.title'        <<<"$PR_JSON")
PR_URL=$(jq -r   '.url'          <<<"$PR_JSON")
PR_BODY=$(jq -r  '.body // ""'   <<<"$PR_JSON")
PR_BRANCH=$(jq -r '.headRefName' <<<"$PR_JSON")
PR_AUTHOR=$(jq -r '.author.login // "unknown"' <<<"$PR_JSON")

# Detect /dispatch PR by branch-name pattern (e.g. M0a-T1, M1-T12).
DISPATCH_TASK_ID=""
if [[ "$PR_BRANCH" =~ ^([A-Z][0-9]+[a-z]*-T[0-9]+)$ ]]; then
  DISPATCH_TASK_ID="${BASH_REMATCH[1]}"
fi

{
  echo "# PR #${PR_NUM}: ${PR_TITLE}"
  echo
  echo "- URL: ${PR_URL}"
  echo "- Author: @${PR_AUTHOR}"
  echo "- Branch: \`${PR_BRANCH}\`"
  [[ -n "$DISPATCH_TASK_ID" ]] && echo "- Dispatch task id: \`${DISPATCH_TASK_ID}\` (this PR was opened by /dispatch — the task spec governs what was promised)"
  echo
  echo "## PR body"
  echo
  if [[ -n "$PR_BODY" ]]; then echo "$PR_BODY"; else echo "_(empty)_"; fi
  echo
} > "$OUT_DIR/pr-context.md"

ISSUE_NUMS=$(jq -r '.closingIssuesReferences[]?.number' <<<"$PR_JSON")
if [[ -n "$ISSUE_NUMS" ]]; then
  echo "## Linked issues" >> "$OUT_DIR/pr-context.md"
  echo                    >> "$OUT_DIR/pr-context.md"
  while IFS= read -r issue; do
    [[ -z "$issue" ]] && continue
    {
      echo "### Issue #${issue}"
      echo
      if ! gh issue view "$issue" "${REPO_FLAG[@]}" --json title,body,url \
            | jq -r '"**\(.title)** — \(.url)\n\n\(.body // "_(no body)_")"'; then
        echo "_(could not fetch issue body)_"
      fi
      echo
    } >> "$OUT_DIR/pr-context.md"
  done <<<"$ISSUE_NUMS"
fi

# ---------- 3. Repo map (cached) ----------
REPO_SLUG="$(gh repo view "${REPO_FLAG[@]}" --json nameWithOwner -q .nameWithOwner | tr '/' '_')"
CACHE_DIR="${HOME}/.cache/claude-admin/repo-maps"
CACHE_FILE="${CACHE_DIR}/${REPO_SLUG}.md"
CACHE_META="${CACHE_DIR}/${REPO_SLUG}.meta"
mkdir -p "$CACHE_DIR"

CURRENT_HEAD=$(git -C "$(git rev-parse --show-toplevel)" rev-parse HEAD 2>/dev/null || echo "no-head")
REBUILD=1
if [[ -f "$CACHE_FILE" && -f "$CACHE_META" ]]; then
  CACHED_HEAD=$(grep '^head=' "$CACHE_META" | cut -d= -f2 || echo "")
  CACHE_AGE=$(( $(date +%s) - $(stat -f %m "$CACHE_FILE" 2>/dev/null || stat -c %Y "$CACHE_FILE") ))
  if [[ "$CACHED_HEAD" == "$CURRENT_HEAD" && "$CACHE_AGE" -lt 86400 ]]; then
    REBUILD=0
  fi
fi

if [[ "$REBUILD" -eq 1 ]]; then
  REPO_ROOT="$(git rev-parse --show-toplevel)"
  {
    echo "# Repo map: ${REPO_SLUG//_//} @ ${CURRENT_HEAD:0:8}"
    echo
    echo "_Lightweight fallback map (project-mapper not integrated yet). Reviewer agents"
    echo "should treat depth as approximate._"
    echo
    echo "## Top-level"
    echo '```'
    (cd "$REPO_ROOT" && ls -1 --color=never 2>/dev/null || ls -1)
    echo '```'
    echo
    echo "## Directory tree (depth 3, skipping common noise)"
    echo '```'
    (cd "$REPO_ROOT" && find . -maxdepth 3 \
       \( -name node_modules -o -name target -o -name dist -o -name .git \
          -o -name __pycache__ -o -name .venv -o -name venv \) -prune -o \
       -type d -print 2>/dev/null | sort | head -200)
    echo '```'
    echo
    if [[ -f "$REPO_ROOT/README.md" ]]; then
      echo "## README.md (first 80 lines)"
      echo '```markdown'
      head -80 "$REPO_ROOT/README.md"
      echo '```'
    fi
  } > "$CACHE_FILE"
  echo "head=${CURRENT_HEAD}" > "$CACHE_META"
fi
cp "$CACHE_FILE" "$OUT_DIR/repo-map.md"

# ---------- 4. PR stats ----------
FILES_JSON="$(gh pr view "$PR_NUM" "${REPO_FLAG[@]}" --json files)"
TOTAL_FILES=$(jq '.files | length'                  <<<"$FILES_JSON")
TOTAL_ADD=$(jq   '[.files[].additions] | add // 0'  <<<"$FILES_JSON")
TOTAL_DEL=$(jq   '[.files[].deletions] | add // 0'  <<<"$FILES_JSON")

{
  echo "files_changed=${TOTAL_FILES}"
  echo "additions=${TOTAL_ADD}"
  echo "deletions=${TOTAL_DEL}"
  echo
  echo "# language breakdown (by file extension)"
  jq -r '.files[].path' <<<"$FILES_JSON" \
    | awk -F. 'NF>1{print $NF}' | sort | uniq -c | sort -rn
  echo
  echo "# changed files"
  jq -r '.files[] | "  \(.additions)+/\(.deletions)-  \(.path)"' <<<"$FILES_JSON"
} > "$OUT_DIR/pr-stats.txt"

# ---------- done ----------
echo "$OUT_DIR"
