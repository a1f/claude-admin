#!/usr/bin/env bash
# /distill-lessons skill orchestrator.
#
# Usage:
#   run.sh <PR#> [--no-write] [--modules a,b,...] [--bundle DIR] [--repo OWNER/NAME]
#   run.sh --bundle DIR --no-write
#
# Side effects:
#   - If <PR#> mode: builds PR bundle via scripts/build-pr-bundle.sh
#   - Fetches PR comments → bundle/pr-comments.md (filtered to /cc-review +
#     /critic verdict bodies when detectable; otherwise full comment stream)
#   - Derives touched modules from the PR's file list → bundle/modules.txt
#   - Fans out one claude subprocess per module to revise that module's
#     modules/<name>/LESSONS.md (read existing, fold in evidence, write back)
#   - Writes revised content to modules/<name>/LESSONS.md unless --no-write
#
# --bundle implies --no-write (no live repo guarantee).

set -eo pipefail
# Not -u: bash 3.2 on macOS chokes on "${array[@]}" expansion of empty arrays.

die()  { echo "distill-lessons: $*" >&2; exit 1; }
warn() { echo "distill-lessons: $*" >&2; }
log()  { echo "distill-lessons: $*" >&2; }

PR_NUM=""
NO_WRITE=0
ONLY_MODULES=""
BUNDLE=""
REPO_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-write)   NO_WRITE=1;                  shift ;;
    --modules=*)  ONLY_MODULES="${1#--modules=}"; shift ;;
    --modules)    ONLY_MODULES="$2";           shift 2 ;;
    --bundle)     BUNDLE="$2";                 shift 2 ;;
    --repo)       REPO_ARGS=(--repo "$2");     shift 2 ;;
    -h|--help)    sed -n '2,17p' "$0"; exit 0 ;;
    -*)           die "unknown flag: $1" ;;
    *)            [[ -z "$PR_NUM" ]] && PR_NUM="$1" || die "unexpected arg: $1"; shift ;;
  esac
done

command -v jq      >/dev/null || die "jq required"
command -v claude  >/dev/null || die "claude CLI required"

# --bundle implies --no-write
[[ -n "$BUNDLE" ]] && NO_WRITE=1

SKILL_DIR="$(cd "$(dirname "$0")/.." && pwd -P)"
SKILL_REPO_ROOT="$(cd "${SKILL_DIR}/../.." && pwd -P)"
DISTILL_PROMPT="${SKILL_DIR}/prompts/distill.md"
[[ -f "$DISTILL_PROMPT" ]] || die "missing distill prompt at $DISTILL_PROMPT"

# ---------- 1. Build / resolve bundle ----------
if [[ -n "$BUNDLE" ]]; then
  [[ -d "$BUNDLE" ]] || die "--bundle dir not found: $BUNDLE"
  [[ -f "$BUNDLE/pr-diff.patch" ]] || die "bundle missing pr-diff.patch"
  [[ -f "$BUNDLE/pr-context.md" ]] || die "bundle missing pr-context.md"
  PR_NUM="${PR_NUM:-offline}"
  log "using provided bundle = $BUNDLE"
else
  [[ -n "$PR_NUM" ]] || die "PR number required (or use --bundle DIR)"
  command -v gh >/dev/null || die "gh CLI required for PR mode"
  BUNDLE_BUILDER="${SKILL_REPO_ROOT}/scripts/build-pr-bundle.sh"
  [[ -x "$BUNDLE_BUILDER" ]] || die "expected bundle builder at $BUNDLE_BUILDER"
  log "building context bundle for PR #${PR_NUM}..."
  BUNDLE="$("$BUNDLE_BUILDER" "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"})"
  [[ -d "$BUNDLE" ]] || die "bundle builder did not produce a directory"
  log "bundle = $BUNDLE"
fi
mkdir -p "$BUNDLE/logs" "$BUNDLE/proposed"

# Warn if PR isn't merged (still proceed — the skill is post-merge but the
# user knows their workflow).
if [[ "$PR_NUM" != "offline" && ${#REPO_ARGS[@]} -ge 0 ]]; then
  MERGED=$(gh pr view "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} --json merged -q .merged 2>/dev/null || echo "")
  if [[ "$MERGED" == "false" ]]; then
    warn "PR #${PR_NUM} is not merged yet — proceeding anyway"
  fi
fi

# ---------- 2. Fetch /cc-review + /critic comments ----------
COMMENTS_FILE="${BUNDLE}/pr-comments.md"
if [[ ! -f "$COMMENTS_FILE" && "$PR_NUM" != "offline" ]]; then
  log "fetching PR comments..."
  COMMENTS_JSON=$(gh pr view "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} --json comments 2>/dev/null || echo '{"comments":[]}')

  # Filter to /cc-review summary + /cc-review detail + /critic verdict bodies.
  # Markers: "Multi-agent review" (cc-review summary), "reviewer findings"
  # (cc-review detail), "Critic verdict" (critic). If filter yields nothing,
  # fall back to all comments — better to over-include than miss evidence.
  FILTERED=$(jq -r '
    .comments
    | map(select(
        (.body | contains("Multi-agent review"))
        or (.body | contains("reviewer findings"))
        or (.body | contains("Critic verdict"))
      ))
    | if length == 0 then null else . end
  ' <<<"$COMMENTS_JSON")

  if [[ "$FILTERED" == "null" || -z "$FILTERED" ]]; then
    warn "no /cc-review or /critic markers detected in comments; including all comments"
    FILTERED=$(jq '.comments' <<<"$COMMENTS_JSON")
  fi

  {
    echo "# PR #${PR_NUM} — review/critic comments"
    echo
    jq -r '.[] | "## comment by @" + (.author.login // "unknown") + "\n\n" + (.body // "") + "\n"' <<<"$FILTERED"
  } > "$COMMENTS_FILE"
  log "comments → $COMMENTS_FILE ($(wc -l <"$COMMENTS_FILE" | tr -d ' ') lines)"
elif [[ ! -f "$COMMENTS_FILE" ]]; then
  warn "offline bundle missing pr-comments.md — claude will work from diff + context only"
  : > "$COMMENTS_FILE"
fi

# ---------- 3. Identify touched files ----------
FILES_TXT="${BUNDLE}/changed-files.txt"
if [[ ! -f "$FILES_TXT" ]]; then
  if [[ "$PR_NUM" != "offline" ]]; then
    gh pr view "$PR_NUM" ${REPO_ARGS[@]+"${REPO_ARGS[@]}"} --json files \
      | jq -r '.files[].path' > "$FILES_TXT"
  else
    # Parse paths from the diff patch (lines like "+++ b/path/to/file").
    grep -E '^\+\+\+ b/' "$BUNDLE/pr-diff.patch" \
      | sed 's|^\+\+\+ b/||' \
      | grep -v '^/dev/null$' \
      > "$FILES_TXT" || true
  fi
fi
TOTAL_FILES=$(wc -l <"$FILES_TXT" | tr -d ' ')
[[ "$TOTAL_FILES" -gt 0 ]] || die "no changed files detected for PR #${PR_NUM}"
log "changed files: ${TOTAL_FILES}"

# ---------- 4. Map files → modules ----------
# Rules (first match wins):
#   skills/<x>/...           -> skills/<x>
#   crates/<x>/...           -> crates/<x>
#   docs/<x>/...             -> docs/<x>
#   scripts/...              -> scripts
#   v1_orchestrator/...      -> v1_orchestrator
#   v2_design/...            -> v2_design
#   tests/...                -> tests
#   .github/...              -> .github
#   <anything-else-at-root>  -> root
file_to_module() {
  local f="$1"
  case "$f" in
    skills/*/*)          echo "$f" | awk -F/ '{print $1"/"$2}' ;;
    crates/*/*)          echo "$f" | awk -F/ '{print $1"/"$2}' ;;
    docs/*/*)            echo "$f" | awk -F/ '{print $1"/"$2}' ;;
    scripts/*)           echo "scripts" ;;
    v1_orchestrator/*)   echo "v1_orchestrator" ;;
    v2_design/*)         echo "v2_design" ;;
    tests/*)             echo "tests" ;;
    .github/*)           echo ".github" ;;
    *)                   echo "root" ;;
  esac
}

MODULES_TXT="${BUNDLE}/modules.txt"
: > "$MODULES_TXT"
while IFS= read -r f; do
  [[ -n "$f" ]] || continue
  file_to_module "$f"
done < "$FILES_TXT" | sort -u > "$MODULES_TXT"

# Restrict to --modules if given.
if [[ -n "$ONLY_MODULES" ]]; then
  REQUESTED=$(echo "$ONLY_MODULES" | tr ',' '\n' | sort -u)
  FILTERED_MODULES=$(comm -12 "$MODULES_TXT" <(echo "$REQUESTED"))
  if [[ -z "$FILTERED_MODULES" ]]; then
    die "--modules filter '$ONLY_MODULES' matched no touched modules (touched: $(tr '\n' ',' <"$MODULES_TXT"))"
  fi
  echo "$FILTERED_MODULES" > "$MODULES_TXT"
fi

MODULE_COUNT=$(wc -l <"$MODULES_TXT" | tr -d ' ')
log "modules to distill: ${MODULE_COUNT}"
while IFS= read -r m; do
  count=$(awk -v mod="$m" '
    BEGIN{ n=0 }
    {
      f=$0
      # mirror file_to_module — match by prefix
      if (f ~ "^skills/[^/]+/")        { split(f,a,"/"); mm=a[1]"/"a[2] }
      else if (f ~ "^crates/[^/]+/")   { split(f,a,"/"); mm=a[1]"/"a[2] }
      else if (f ~ "^docs/[^/]+/")     { split(f,a,"/"); mm=a[1]"/"a[2] }
      else if (f ~ "^scripts/")         mm="scripts"
      else if (f ~ "^v1_orchestrator/") mm="v1_orchestrator"
      else if (f ~ "^v2_design/")       mm="v2_design"
      else if (f ~ "^tests/")           mm="tests"
      else if (f ~ "^\\.github/")       mm=".github"
      else                              mm="root"
      if (mm == mod) n++
    }
    END { print n }
  ' "$FILES_TXT")
  log "  $m ($count file(s))"
done <"$MODULES_TXT"

# ---------- 5. Fan out one claude per module ----------
# Each subprocess gets module name + per-module file list + bundle path +
# absolute path to the existing LESSONS.md. It outputs the full revised file
# on stdout.

PIDS=()
LABELS=()
SAFE_NAMES=()

# Detect repo root for writing modules/<name>/LESSONS.md back. In bundle/
# offline mode, write to bundle/proposed/ only.
if [[ "$NO_WRITE" -eq 1 ]]; then
  REPO_ROOT=""
else
  REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
  [[ -n "$REPO_ROOT" ]] || die "could not resolve repo root (run inside a git checkout, or pass --no-write)"
fi

while IFS= read -r module; do
  [[ -n "$module" ]] || continue
  safe=$(echo "$module" | tr '/' '__')
  log_out="${BUNDLE}/logs/distill-${safe}.out"
  log_err="${BUNDLE}/logs/distill-${safe}.err"

  # Files belonging to this module (subset of changed-files.txt).
  module_files_path="${BUNDLE}/files-${safe}.txt"
  awk -v mod="$module" '
    {
      f=$0
      if (f ~ "^skills/[^/]+/")        { split(f,a,"/"); mm=a[1]"/"a[2] }
      else if (f ~ "^crates/[^/]+/")   { split(f,a,"/"); mm=a[1]"/"a[2] }
      else if (f ~ "^docs/[^/]+/")     { split(f,a,"/"); mm=a[1]"/"a[2] }
      else if (f ~ "^scripts/")         mm="scripts"
      else if (f ~ "^v1_orchestrator/") mm="v1_orchestrator"
      else if (f ~ "^v2_design/")       mm="v2_design"
      else if (f ~ "^tests/")           mm="tests"
      else if (f ~ "^\\.github/")       mm=".github"
      else                              mm="root"
      if (mm == mod) print f
    }
  ' "$FILES_TXT" > "$module_files_path"

  module_files_inline=$(awk '{ printf "  - %s\n", $0 }' "$module_files_path")

  # Existing LESSONS.md path (read by claude via Read tool).
  if [[ -n "$REPO_ROOT" ]]; then
    existing_lessons_path="${REPO_ROOT}/modules/${module}/LESSONS.md"
  else
    existing_lessons_path="(no live repo; treat as empty)"
  fi

  USER_PROMPT="You are the distiller for module: ${module}

Bundle directory: ${BUNDLE}
Files available there:
  - pr-diff.patch     (full PR diff — focus on this module's files)
  - pr-context.md     (PR body + linked issue body)
  - pr-comments.md    (/cc-review + /critic verdict comment bodies)
  - changed-files.txt (all changed files in this PR, for reference)

Files in THIS module touched by this PR:
${module_files_inline}

Existing LESSONS.md for this module:
  ${existing_lessons_path}

Read that LESSONS.md (use Read; if it does not exist, treat as empty). Then
apply the revisit algorithm in your system prompt. Output the full revised
LESSONS.md content as plain markdown. No fences. No prose around it. Only
the file body."

  (
    claude -p "$USER_PROMPT" \
           --append-system-prompt "$(cat "$DISTILL_PROMPT")" \
           --output-format text \
           < /dev/null \
           > "$log_out" 2> "$log_err" \
      || echo "_(distill subprocess errored — see logs/distill-${safe}.err)_" > "$log_out"
  ) &

  PIDS+=("$!")
  LABELS+=("${module}")
  SAFE_NAMES+=("${safe}")
  log "spawned ${module} → pid $!  log=$log_out"
done <"$MODULES_TXT"

# ---------- 6. Wait ----------
log "waiting for ${#PIDS[@]} distill subprocess(es)..."
FAIL_COUNT=0
for i in "${!PIDS[@]}"; do
  if wait "${PIDS[$i]}"; then
    log "  ${LABELS[$i]} done"
  else
    log "  ${LABELS[$i]} FAILED (see logs/distill-${SAFE_NAMES[$i]}.err)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
done
[[ "$FAIL_COUNT" -gt 0 ]] && warn "${FAIL_COUNT} subprocess(es) failed; proposing only successful modules"

# ---------- 7. Stage proposed outputs + write back ----------
UPDATED=0
UNCHANGED=0
EMPTY=0

for i in "${!LABELS[@]}"; do
  module="${LABELS[$i]}"
  safe="${SAFE_NAMES[$i]}"
  raw="${BUNDLE}/logs/distill-${safe}.out"
  proposed="${BUNDLE}/proposed/${module}/LESSONS.md"
  mkdir -p "$(dirname "$proposed")"

  # Strip leading blank lines + optional code fences claude sometimes adds.
  python3 - "$raw" "$proposed" <<'PY'
import re, sys
src, dst = sys.argv[1], sys.argv[2]
with open(src) as f:
    text = f.read()
text = text.lstrip()
m = re.match(r"^```(?:markdown|md)?\s*\n(.*?)\n```\s*$", text, flags=re.DOTALL)
if m:
    text = m.group(1).strip() + "\n"
if not text.endswith("\n"):
    text += "\n"
with open(dst, "w") as f:
    f.write(text)
PY

  if [[ ! -s "$proposed" ]]; then
    EMPTY=$((EMPTY + 1))
    log "  ${module}: empty output — skipping"
    continue
  fi

  if [[ "$NO_WRITE" -eq 1 ]]; then
    log "  ${module}: --no-write → proposed at ${proposed}"
    continue
  fi

  target="${REPO_ROOT}/modules/${module}/LESSONS.md"
  mkdir -p "$(dirname "$target")"
  if [[ -f "$target" ]] && cmp -s "$proposed" "$target"; then
    UNCHANGED=$((UNCHANGED + 1))
    log "  ${module}: unchanged"
  else
    cp "$proposed" "$target"
    UPDATED=$((UPDATED + 1))
    log "  ${module}: wrote ${target} ($(wc -c <"$target" | tr -d ' ') bytes)"
  fi
done

# ---------- 8. Summary ----------
echo "distill-lessons: done." >&2
echo "  modules processed : ${#LABELS[@]}" >&2
echo "  updated           : ${UPDATED}" >&2
echo "  unchanged         : ${UNCHANGED}" >&2
echo "  empty (skipped)   : ${EMPTY}" >&2
echo "  failed            : ${FAIL_COUNT}" >&2
echo "  bundle            : ${BUNDLE}" >&2
echo "$BUNDLE"
