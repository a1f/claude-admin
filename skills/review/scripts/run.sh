#!/usr/bin/env bash
# /review skill orchestrator.
#
# Usage:
#   run.sh <PR#> [--kinds=k1,k2,...] [--runs=N] [--tmux] [--repo OWNER/NAME]
#
# Defaults: kinds = bugs,quality,architecture,tests,bulletproof  ;  runs = 3
#
# Side effects:
#   - Builds context bundle via scripts/build-pr-bundle.sh
#   - Fans out (kind × run) claude -p subprocesses in parallel; logs to bundle/logs/
#   - Aggregates JSON outputs into summary.md + detail-<kind>.md
#   - Posts summary as a PR comment, plus one detail comment per kind with findings
#   - Applies the CRITICAL label if any blocker found

set -eo pipefail

die()  { echo "review: $*" >&2; exit 1; }
warn() { echo "review: $*" >&2; }

PR_NUM=""
KINDS="bugs,quality,architecture,tests,bulletproof"
RUNS=3
TMUX=0
REPO_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --kinds=*) KINDS="${1#--kinds=}";       shift ;;
    --kinds)   KINDS="$2";                  shift 2 ;;
    --runs=*)  RUNS="${1#--runs=}";         shift ;;
    --runs)    RUNS="$2";                   shift 2 ;;
    --tmux)    TMUX=1;                      shift ;;
    --repo)    REPO_ARGS=(--repo "$2");     shift 2 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    -*)        die "unknown flag: $1" ;;
    *)         [[ -z "$PR_NUM" ]] && PR_NUM="$1" || die "unexpected arg: $1"; shift ;;
  esac
done

[[ -n "$PR_NUM" ]]    || die "PR number required"
[[ "$RUNS" =~ ^[0-9]+$ && "$RUNS" -ge 1 ]] || die "--runs must be a positive integer"
command -v claude >/dev/null || die "claude CLI not installed"
command -v gh     >/dev/null || die "gh CLI not installed"
command -v jq     >/dev/null || die "jq required"
command -v python3 >/dev/null || die "python3 required"

if [[ "$TMUX" -eq 1 ]]; then
  warn "tmux mode not yet implemented; continuing without it"
fi

# Skill root: this script is at <SKILL>/scripts/run.sh, where <SKILL> is
# <repo>/skills/review/. Resolve via the script's own path (works whether
# invoked directly or via the ~/.claude/skills/review symlink, since `cd`
# follows the link to its target).
SKILL_DIR="$(cd "$(dirname "$0")/.." && pwd -P)"
SKILL_REPO_ROOT="$(cd "${SKILL_DIR}/../.." && pwd -P)"
BUNDLE_BUILDER="${SKILL_REPO_ROOT}/scripts/build-pr-bundle.sh"
[[ -x "$BUNDLE_BUILDER" ]] || die "expected bundle builder at $BUNDLE_BUILDER"

# ---------- 1. Build context bundle ----------
echo "review: building context bundle for PR #${PR_NUM}..." >&2
BUNDLE="$("$BUNDLE_BUILDER" "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"})"
[[ -d "$BUNDLE" ]] || die "bundle builder did not produce a directory"
mkdir -p "$BUNDLE/logs"
echo "review: bundle = $BUNDLE" >&2

# ---------- 2. Resolve prompt files per kind ----------
# bugs + quality reuse the existing skills/reviewer/SKILL.md (kind-aware).
# architecture, tests, bulletproof use our new prompt files.
REVIEWER_SKILL="${SKILL_REPO_ROOT}/skills/reviewer/SKILL.md"
[[ -f "$REVIEWER_SKILL" ]] || die "expected reviewer skill at $REVIEWER_SKILL"

prompt_for_kind() {
  local k="$1"
  case "$k" in
    bugs|quality)  echo "$REVIEWER_SKILL" ;;
    architecture)  echo "${SKILL_DIR}/prompts/architecture.md" ;;
    tests)         echo "${SKILL_DIR}/prompts/tests.md" ;;
    bulletproof)   echo "${SKILL_DIR}/prompts/bulletproof.md" ;;
    *)             return 1 ;;
  esac
}

# ---------- 3. Fan out claude subprocesses ----------
IFS=',' read -ra KIND_LIST <<<"$KINDS"
PIDS=()
LABELS=()

for kind in "${KIND_LIST[@]}"; do
  prompt_file="$(prompt_for_kind "$kind")" || die "unknown kind: $kind"
  [[ -f "$prompt_file" ]] || die "missing prompt file for $kind: $prompt_file"

  for run in $(seq 1 "$RUNS"); do
    log="${BUNDLE}/logs/claude-${kind}-${run}.jsonl"
    err="${BUNDLE}/logs/claude-${kind}-${run}.err"

    user_prompt="You are reviewing PR #${PR_NUM} as a reviewer of kind: ${kind}.

Bundle directory: ${BUNDLE}
Files available there:
  - pr-diff.patch    (full diff)
  - pr-context.md    (PR body + linked issue body + task spec if /dispatch PR)
  - repo-map.md      (cached repo layout)
  - pr-stats.txt     (files changed, +/- LOC, languages)

Read those files. Then output the JSON object exactly per the schema in your
appended system prompt. No markdown fences. No prose around it. Only JSON.

This is run ${run} of ${RUNS} for this kind — run independently. Score what YOU see."

    (
      claude -p "$user_prompt" \
             --append-system-prompt "$(cat "$prompt_file")" \
             --output-format text \
             < /dev/null \
             > "$log" 2> "$err" \
        || echo "{\"kind\":\"${kind}\",\"summary\":\"reviewer subprocess errored\",\"findings\":[],\"_error\":true}" > "$log"
    ) &

    PIDS+=("$!")
    LABELS+=("${kind}/${run}")
    echo "review: spawned ${kind} run ${run} → pid $!  log=$log" >&2
  done
done

# ---------- 4. Wait for all subprocesses ----------
echo "review: waiting for ${#PIDS[@]} reviewer subprocess(es)..." >&2
FAIL_COUNT=0
for i in "${!PIDS[@]}"; do
  pid="${PIDS[$i]}"
  label="${LABELS[$i]}"
  if wait "$pid"; then
    echo "review:   ${label} done" >&2
  else
    echo "review:   ${label} FAILED (see .err)" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
done

[[ "$FAIL_COUNT" -gt 0 ]] && warn "${FAIL_COUNT} subprocess(es) failed; aggregator will include any partial output and skip the rest"

# ---------- 5. Aggregate ----------
echo "review: aggregating findings..." >&2
python3 "${SKILL_DIR}/scripts/aggregate.py" \
  --bundle "$BUNDLE" \
  --pr "$PR_NUM" \
  --kinds "$KINDS" \
  --runs "$RUNS"

SUMMARY="${BUNDLE}/summary.md"
[[ -f "$SUMMARY" ]] || die "aggregator did not produce summary.md"

# ---------- 6. Post to PR ----------
echo "review: posting summary comment to PR #${PR_NUM}..." >&2
SUMMARY_URL=$(gh pr comment "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} --body-file "$SUMMARY")
echo "review:   summary comment: $SUMMARY_URL" >&2

for kind in "${KIND_LIST[@]}"; do
  detail="${BUNDLE}/detail-${kind}.md"
  if [[ -s "$detail" ]]; then
    url=$(gh pr comment "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} --body-file "$detail")
    echo "review:   ${kind} detail comment: $url" >&2
  fi
done

# ---------- 7. CRITICAL label if any blocker ----------
BLOCKER_TOTAL=$(jq -r '.totals.blocker // 0' "${BUNDLE}/summary.json")
if [[ "$BLOCKER_TOTAL" -gt 0 ]]; then
  # Ensure label exists (idempotent: ignore "already exists" error).
  gh label create CRITICAL --color B60205 --description "Reviewer found a blocker" \
    ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} 2>/dev/null || true
  gh pr edit "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} --add-label CRITICAL >/dev/null
  echo "review:   CRITICAL label applied (${BLOCKER_TOTAL} blocker(s))" >&2
else
  echo "review:   no blockers; CRITICAL label not applied" >&2
fi

echo "review: done. Bundle preserved at $BUNDLE" >&2
echo "$SUMMARY_URL"
