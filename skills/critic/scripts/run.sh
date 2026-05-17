#!/usr/bin/env bash
# /critic skill orchestrator.
#
# Usage:
#   run.sh <PR#> [--runs=N] [--no-post] [--repo OWNER/NAME]
#   run.sh --bundle DIR [--runs=N] [--no-post]
#
# Defaults: runs = 3.
#
# Side effects:
#   - If <PR#> mode: builds bundle via scripts/build-pr-bundle.sh
#   - Fans out N claude -p critic subprocesses in parallel; logs to bundle/logs/
#   - Aggregates JSON outputs into summary.md + summary.json
#   - Posts summary as a single PR comment unless --no-post / --bundle

set -eo pipefail

die()  { echo "critic: $*" >&2; exit 1; }
warn() { echo "critic: $*" >&2; }

PR_NUM=""
RUNS=3
BUNDLE=""
NO_POST=0
REPO_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs=*)   RUNS="${1#--runs=}";        shift ;;
    --runs)     RUNS="$2";                  shift 2 ;;
    --bundle)   BUNDLE="$2";                shift 2 ;;
    --no-post)  NO_POST=1;                  shift ;;
    --repo)     REPO_ARGS=(--repo "$2");    shift 2 ;;
    -h|--help)  sed -n '2,15p' "$0"; exit 0 ;;
    -*)         die "unknown flag: $1" ;;
    *)          [[ -z "$PR_NUM" ]] && PR_NUM="$1" || die "unexpected arg: $1"; shift ;;
  esac
done

[[ "$RUNS" =~ ^[0-9]+$ && "$RUNS" -ge 1 ]] || die "--runs must be a positive integer"
command -v claude  >/dev/null || die "claude CLI not installed"
command -v jq      >/dev/null || die "jq required"
command -v python3 >/dev/null || die "python3 required"

# --bundle implies --no-post (no PR to post to)
if [[ -n "$BUNDLE" ]]; then
  NO_POST=1
fi

# Resolve skill paths.
SKILL_DIR="$(cd "$(dirname "$0")/.." && pwd -P)"
AGENT_PROMPT="${SKILL_DIR}/prompts/agent.md"
[[ -f "$AGENT_PROMPT" ]] || die "missing agent prompt at $AGENT_PROMPT"

# ---------- 1. Resolve / build bundle ----------
if [[ -n "$BUNDLE" ]]; then
  [[ -d "$BUNDLE" ]] || die "--bundle dir not found: $BUNDLE"
  [[ -f "$BUNDLE/pr-diff.patch"  ]] || die "bundle missing pr-diff.patch"
  [[ -f "$BUNDLE/pr-context.md"  ]] || die "bundle missing pr-context.md"
  PR_NUM="${PR_NUM:-offline}"
  echo "critic: using provided bundle = $BUNDLE" >&2
else
  [[ -n "$PR_NUM" ]] || die "PR number required (or use --bundle DIR)"
  command -v gh >/dev/null || die "gh CLI required for PR mode"

  SKILL_REPO_ROOT="$(cd "${SKILL_DIR}/../.." && pwd -P)"
  BUNDLE_BUILDER="${SKILL_REPO_ROOT}/scripts/build-pr-bundle.sh"
  [[ -x "$BUNDLE_BUILDER" ]] || die "bundle builder missing at $BUNDLE_BUILDER"

  echo "critic: building context bundle for PR #${PR_NUM}..." >&2
  BUNDLE="$("$BUNDLE_BUILDER" "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"})"
  [[ -d "$BUNDLE" ]] || die "bundle builder did not produce a directory"
  echo "critic: bundle = $BUNDLE" >&2
fi
mkdir -p "$BUNDLE/logs"

# ---------- 2. Fan out claude critic subprocesses ----------
PIDS=()
LABELS=()

for run in $(seq 1 "$RUNS"); do
  log="${BUNDLE}/logs/claude-critic-${run}.jsonl"
  err="${BUNDLE}/logs/claude-critic-${run}.err"

  user_prompt="You are critiquing PR #${PR_NUM} for GOAL-FIT.

Bundle directory: ${BUNDLE}
Files available there:
  - pr-diff.patch    (full diff)
  - pr-context.md    (PR body + linked issue body + task spec if /dispatch PR)
  - repo-map.md      (cached repo layout — may be a minimal fixture)
  - pr-stats.txt     (files changed, +/- LOC, languages)

Read those files. Then output the JSON object exactly per the schema in your
appended system prompt. No markdown fences. No prose around it. Only JSON.

This is run ${run} of ${RUNS} — run independently. Score what YOU see."

  (
    claude -p "$user_prompt" \
           --append-system-prompt "$(cat "$AGENT_PROMPT")" \
           --output-format text \
           < /dev/null \
           > "$log" 2> "$err" \
      || echo "{\"score\":0,\"verdict\":\"reject\",\"axes\":{},\"rationale_md\":\"critic subprocess errored\",\"concerns\":[],\"_error\":true}" > "$log"
  ) &

  PIDS+=("$!")
  LABELS+=("critic/${run}")
  echo "critic: spawned critic run ${run} → pid $!  log=$log" >&2
done

# ---------- 3. Wait ----------
echo "critic: waiting for ${#PIDS[@]} critic subprocess(es)..." >&2
FAIL_COUNT=0
for i in "${!PIDS[@]}"; do
  pid="${PIDS[$i]}"
  label="${LABELS[$i]}"
  if wait "$pid"; then
    echo "critic:   ${label} done" >&2
  else
    echo "critic:   ${label} FAILED (see .err)" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
done
[[ "$FAIL_COUNT" -gt 0 ]] && warn "${FAIL_COUNT} subprocess(es) failed"

# ---------- 4. Aggregate ----------
echo "critic: aggregating..." >&2
python3 "${SKILL_DIR}/scripts/aggregate.py" \
  --bundle "$BUNDLE" \
  --pr "$PR_NUM" \
  --runs "$RUNS"

SUMMARY="${BUNDLE}/summary.md"
[[ -f "$SUMMARY" ]] || die "aggregator did not produce summary.md"

# Print headline to stderr for the user.
SCORE=$(jq -r '.score   // "?"' "${BUNDLE}/summary.json")
VERDICT=$(jq -r '.verdict // "?"' "${BUNDLE}/summary.json")
echo "critic: score=${SCORE}  verdict=${VERDICT}" >&2

# ---------- 5. Post (unless --no-post) ----------
if [[ "$NO_POST" -eq 1 ]]; then
  echo "critic: --no-post set; skipping PR comment" >&2
  echo "critic: done. Bundle preserved at $BUNDLE" >&2
  echo "$SUMMARY"
  exit 0
fi

URL=$(gh pr comment "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} --body-file "$SUMMARY")
echo "critic:   summary comment: $URL" >&2
echo "critic: done. Bundle preserved at $BUNDLE" >&2
echo "$URL"
