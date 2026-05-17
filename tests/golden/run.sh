#!/usr/bin/env bash
# Golden acceptance tests for /critic.
#
# Verifies the S9 axis: the critic distinguishes goal-fit from code quality.
#
#   fixture-quality-bad-goal-met       → verdict in {strong, acceptable}
#   fixture-quality-good-goal-missed   → verdict in {weak, reject}
#
# Usage:
#   tests/golden/run.sh                  (default: --runs=3)
#   tests/golden/run.sh --runs=1         (cheap mode, 1 critic per fixture)
#   tests/golden/run.sh --runs=3 --keep  (preserve work dirs for inspection)
#
# Exit 0 if all fixtures pass; nonzero otherwise.

set -eo pipefail

die()  { echo "golden: $*" >&2; exit 1; }
warn() { echo "golden: $*" >&2; }

RUNS=3
KEEP=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs=*) RUNS="${1#--runs=}"; shift ;;
    --runs)   RUNS="$2"; shift 2 ;;
    --keep)   KEEP=1; shift ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) die "unknown arg: $1" ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd -P)"
CRITIC_RUN="${REPO_ROOT}/skills/critic/scripts/run.sh"

[[ -x "$CRITIC_RUN" ]] || die "critic runner not found at $CRITIC_RUN — build /critic first"
command -v jq >/dev/null || die "jq required"

# Each entry: "<fixture-name>:<expected-verdict-set-csv>"
FIXTURES=(
  "fixture-quality-bad-goal-met:strong,acceptable"
  "fixture-quality-good-goal-missed:weak,reject"
)

declare -i passed=0 failed=0
FAILURES=()

for entry in "${FIXTURES[@]}"; do
  fixture="${entry%%:*}"
  expected_csv="${entry#*:}"
  src_dir="${SCRIPT_DIR}/${fixture}"
  [[ -d "$src_dir" ]] || die "fixture missing: $src_dir"

  # Copy fixture into a writable work dir so the runner can append logs/ + summary.json.
  work_dir="$(mktemp -d -t "golden-${fixture}-XXXX")"
  cp "$src_dir"/* "$work_dir"/

  echo
  echo "=== fixture: $fixture (expecting verdict ∈ {${expected_csv}}, --runs=${RUNS}) ==="

  if ! "$CRITIC_RUN" --bundle "$work_dir" --runs "$RUNS" --no-post; then
    warn "  runner exited nonzero for $fixture"
    failed+=1
    FAILURES+=("$fixture (runner error)")
    [[ "$KEEP" -eq 1 ]] && echo "  preserved: $work_dir" >&2 || rm -rf "$work_dir"
    continue
  fi

  summary_json="${work_dir}/summary.json"
  if [[ ! -f "$summary_json" ]]; then
    warn "  no summary.json produced"
    failed+=1
    FAILURES+=("$fixture (no summary.json)")
    [[ "$KEEP" -eq 1 ]] && echo "  preserved: $work_dir" >&2 || rm -rf "$work_dir"
    continue
  fi

  verdict=$(jq -r '.verdict // "missing"' "$summary_json")
  score=$(jq   -r '.score   // "?"'       "$summary_json")
  runs_used=$(jq -r '.runs_used // "?"'   "$summary_json")
  echo "  → verdict=${verdict}  score=${score}  runs_used=${runs_used}"

  # Membership check: verdict must be in the expected CSV set.
  ok=0
  IFS=',' read -ra allowed <<<"$expected_csv"
  for v in "${allowed[@]}"; do
    [[ "$verdict" == "$v" ]] && { ok=1; break; }
  done

  if [[ "$ok" -eq 1 ]]; then
    echo "  ✅ PASS"
    passed+=1
  else
    echo "  ❌ FAIL (expected ∈ {${expected_csv}}, got '${verdict}')"
    failed+=1
    FAILURES+=("$fixture: got '${verdict}', expected ∈ {${expected_csv}}")
  fi

  if [[ "$KEEP" -eq 1 ]]; then
    echo "  preserved: $work_dir"
  else
    rm -rf "$work_dir"
  fi
done

echo
echo "=== golden tests: ${passed} passed / ${failed} failed ==="
if [[ "$failed" -gt 0 ]]; then
  echo
  echo "Failures:"
  for f in "${FAILURES[@]}"; do
    echo "  - $f"
  done
  exit 1
fi
exit 0
