#!/usr/bin/env bash
# /review skill orchestrator.
#
# Usage:
#   run.sh <PR#> [--kinds=k1,k2,...] [--runs=N] [--engine=claude|codex|both]
#               [--tmux] [--repo OWNER/NAME] [--bundle DIR] [--no-post]
#
# Defaults: kinds = bugs,quality,architecture,tests,bulletproof
#           runs  = 3   ;  engine = claude
#
# Side effects:
#   - If <PR#> mode: builds context bundle via scripts/build-pr-bundle.sh
#   - Fans out (engine × kind × run) reviewer subprocesses in parallel
#   - Logs to bundle/logs/<engine>-<kind>-<run>.jsonl
#   - Aggregates per-engine findings (deduped across engines too)
#   - Posts summary + per-kind detail comments to the PR
#   - Applies CRITICAL label if any blocker found

set -eo pipefail

die()  { echo "review: $*" >&2; exit 1; }
warn() { echo "review: $*" >&2; }

PR_NUM=""
KINDS="bugs,quality,architecture,tests,bulletproof"
RUNS=3
ENGINES="claude"
BUNDLE=""
NO_POST=0
TMUX=0
REPO_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --kinds=*)  KINDS="${1#--kinds=}";      shift ;;
    --kinds)    KINDS="$2";                 shift 2 ;;
    --runs=*)   RUNS="${1#--runs=}";        shift ;;
    --runs)     RUNS="$2";                  shift 2 ;;
    --engine=*) ENGINES="${1#--engine=}";   shift ;;
    --engine)   ENGINES="$2";               shift 2 ;;
    --bundle)   BUNDLE="$2";                shift 2 ;;
    --no-post)  NO_POST=1;                  shift ;;
    --tmux)     TMUX=1;                     shift ;;
    --repo)     REPO_ARGS=(--repo "$2");    shift 2 ;;
    -h|--help)  sed -n '2,15p' "$0"; exit 0 ;;
    -*)         die "unknown flag: $1" ;;
    *)          [[ -z "$PR_NUM" ]] && PR_NUM="$1" || die "unexpected arg: $1"; shift ;;
  esac
done

[[ "$RUNS" =~ ^[0-9]+$ && "$RUNS" -ge 1 ]] || die "--runs must be a positive integer"
case "$ENGINES" in
  claude)    ENGINE_LIST=(claude) ;;
  codex)     ENGINE_LIST=(codex) ;;
  both|all)  ENGINE_LIST=(claude codex) ;;
  *)         die "--engine must be claude, codex, or both (got: $ENGINES)" ;;
esac
for e in "${ENGINE_LIST[@]}"; do
  command -v "$e" >/dev/null || die "$e CLI not installed (required for --engine=$ENGINES)"
done
command -v jq      >/dev/null || die "jq required"
command -v python3 >/dev/null || die "python3 required"

[[ -n "$BUNDLE" ]] && NO_POST=1
[[ "$TMUX" -eq 1 ]] && warn "tmux mode not yet implemented; continuing without it"

SKILL_DIR="$(cd "$(dirname "$0")/.." && pwd -P)"
SKILL_REPO_ROOT="$(cd "${SKILL_DIR}/../.." && pwd -P)"

# ---------- 1. Build / resolve bundle ----------
if [[ -n "$BUNDLE" ]]; then
  [[ -d "$BUNDLE" ]] || die "--bundle dir not found: $BUNDLE"
  [[ -f "$BUNDLE/pr-diff.patch" ]] || die "bundle missing pr-diff.patch"
  [[ -f "$BUNDLE/pr-context.md" ]] || die "bundle missing pr-context.md"
  PR_NUM="${PR_NUM:-offline}"
  echo "review: using provided bundle = $BUNDLE" >&2
else
  [[ -n "$PR_NUM" ]] || die "PR number required (or use --bundle DIR)"
  command -v gh >/dev/null || die "gh CLI required for PR mode"
  BUNDLE_BUILDER="${SKILL_REPO_ROOT}/scripts/build-pr-bundle.sh"
  [[ -x "$BUNDLE_BUILDER" ]] || die "expected bundle builder at $BUNDLE_BUILDER"
  echo "review: building context bundle for PR #${PR_NUM}..." >&2
  BUNDLE="$("$BUNDLE_BUILDER" "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"})"
  [[ -d "$BUNDLE" ]] || die "bundle builder did not produce a directory"
  echo "review: bundle = $BUNDLE" >&2
fi
mkdir -p "$BUNDLE/logs"

# ---------- 2. Resolve prompt files per kind ----------
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

# ---------- 3. Spawner helpers ----------
spawn_claude() {
  local prompt_file="$1" user_prompt="$2" log="$3" err="$4" kind_for_err="$5"
  (
    claude -p "$user_prompt" \
           --append-system-prompt "$(cat "$prompt_file")" \
           --output-format text \
           < /dev/null \
           > "$log" 2> "$err" \
      || echo "{\"kind\":\"${kind_for_err}\",\"summary\":\"claude reviewer subprocess errored\",\"findings\":[],\"_error\":true}" > "$log"
  ) &
}

spawn_codex() {
  local prompt_file="$1" user_prompt="$2" log="$3" err="$4" kind_for_err="$5"
  # Codex has no --append-system-prompt; concatenate agent prompt + user prompt
  # and pipe via stdin (passing as argv breaks when the prompt starts with `---`
  # — e.g. YAML frontmatter — because clap rejects it as an unknown flag).
  local combined
  combined="$(cat "$prompt_file"; printf '\n---\n\n'; printf '%s' "$user_prompt")"
  (
    printf '%s' "$combined" \
      | codex exec --skip-git-repo-check -C "$BUNDLE" - \
             > "$log" 2> "$err" \
      || echo "{\"kind\":\"${kind_for_err}\",\"summary\":\"codex reviewer subprocess errored\",\"findings\":[],\"_error\":true}" > "$log"
  ) &
}

# ---------- 4. Fan out engine × kind × run ----------
IFS=',' read -ra KIND_LIST <<<"$KINDS"
PIDS=()
LABELS=()

for engine in "${ENGINE_LIST[@]}"; do
  for kind in "${KIND_LIST[@]}"; do
    prompt_file="$(prompt_for_kind "$kind")" || die "unknown kind: $kind"
    [[ -f "$prompt_file" ]] || die "missing prompt file for $kind: $prompt_file"

    for run in $(seq 1 "$RUNS"); do
      log="${BUNDLE}/logs/${engine}-${kind}-${run}.jsonl"
      err="${BUNDLE}/logs/${engine}-${kind}-${run}.err"

      user_prompt="You are reviewing PR #${PR_NUM} as a reviewer of kind: ${kind}.

Bundle directory: ${BUNDLE}
Files available there:
  - pr-diff.patch    (full diff)
  - pr-context.md    (PR body + linked issue body + task spec if /dispatch PR)
  - repo-map.md      (cached repo layout)
  - pr-stats.txt     (files changed, +/- LOC, languages)

Read those files. Then output the JSON object exactly per the schema in the
agent prompt above. No markdown fences. No prose around it. Only JSON.

This is run ${run} of ${RUNS} for this kind on engine ${engine} — run
independently. Score what YOU see."

      case "$engine" in
        claude) spawn_claude "$prompt_file" "$user_prompt" "$log" "$err" "$kind" ;;
        codex)  spawn_codex  "$prompt_file" "$user_prompt" "$log" "$err" "$kind" ;;
      esac

      PIDS+=("$!")
      LABELS+=("${engine}/${kind}/${run}")
      echo "review: spawned ${engine} ${kind} run ${run} → pid $!  log=$log" >&2
    done
  done
done

# ---------- 5. Wait ----------
echo "review: waiting for ${#PIDS[@]} reviewer subprocess(es)..." >&2
FAIL_COUNT=0
for i in "${!PIDS[@]}"; do
  if wait "${PIDS[$i]}"; then
    echo "review:   ${LABELS[$i]} done" >&2
  else
    echo "review:   ${LABELS[$i]} FAILED (see .err)" >&2
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
done
[[ "$FAIL_COUNT" -gt 0 ]] && warn "${FAIL_COUNT} subprocess(es) failed; aggregator will include any partial output and skip the rest"

# ---------- 6. Aggregate ----------
echo "review: aggregating findings..." >&2
python3 "${SKILL_DIR}/scripts/aggregate.py" \
  --bundle "$BUNDLE" \
  --pr "$PR_NUM" \
  --kinds "$KINDS" \
  --runs "$RUNS" \
  --engines "$(IFS=, ; echo "${ENGINE_LIST[*]}")"

SUMMARY="${BUNDLE}/summary.md"
[[ -f "$SUMMARY" ]] || die "aggregator did not produce summary.md"

# ---------- 7. Post (unless --no-post) ----------
if [[ "$NO_POST" -eq 1 ]]; then
  echo "review: --no-post set; skipping PR comments" >&2
  echo "review: done. Bundle preserved at $BUNDLE" >&2
  echo "$SUMMARY"
  exit 0
fi

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

# ---------- 8. CRITICAL label if any blocker ----------
BLOCKER_TOTAL=$(jq -r '.totals.blocker // 0' "${BUNDLE}/summary.json")
if [[ "$BLOCKER_TOTAL" -gt 0 ]]; then
  gh label create CRITICAL --color B60205 --description "Reviewer found a blocker" \
    ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} 2>/dev/null || true
  gh pr edit "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} --add-label CRITICAL >/dev/null
  echo "review:   CRITICAL label applied (${BLOCKER_TOTAL} blocker(s))" >&2
else
  echo "review:   no blockers; CRITICAL label not applied" >&2
fi

echo "review: done. Bundle preserved at $BUNDLE" >&2
echo "$SUMMARY_URL"
