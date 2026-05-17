---
name: review
description: "Multi-agent PR review via Claude. Fans out N parallel reviewers across 5 kinds (bugs, quality, architecture, tests, bulletproof), aggregates findings, posts a summary table + per-kind detail comments to the PR, and applies a CRITICAL label if any blocker is found. Use when the user asks to /review a PR, multi-review a PR, or wants reviewer agents to look at a published PR. Examples: '/review 26', '/review 26 --kinds=bugs,tests', '/review 26 --runs=1'."
argument-hint: "<PR#> [--kinds=...] [--runs=N] [--tmux]"
---

# /review skill

Multi-agent PR review. For one PR, fan out parallel Claude reviewer subprocesses
across multiple "kinds" (bugs, quality, architecture, tests, bulletproof), N
independent runs per kind. Aggregate, dedupe, post one summary comment plus one
detail comment per kind. Apply `CRITICAL` label if any reviewer reports a blocker.

## Usage

```
/review <PR#> [--kinds=bugs,quality,architecture,tests,bulletproof] [--runs=N] [--tmux] [--repo OWNER/NAME]
```

Defaults: `--kinds=bugs,quality,architecture,tests,bulletproof`, `--runs=3`.
With defaults, that's 15 Claude subprocesses in parallel. Use `--runs=1` or a narrower `--kinds`
for a cheaper pass.

`--tmux` is currently a stub (prints a note and runs without it).

## How to invoke

Hand the run off to the orchestration script — it does everything end-to-end:

```bash
bash "${CLAUDE_PLUGIN_ROOT:-$HOME/.claude/skills/review}/scripts/run.sh" <PR#> [flags]
```

Show the script's output to the user verbatim. It prints:

- Bundle directory path
- Per-subprocess pids and where their JSONL logs land
- Aggregate summary path
- URL of the posted summary comment
- Whether the `CRITICAL` label was applied

## What it produces

In the PR:

- **One summary comment** with a markdown table: per-kind counts of blocker/major/minor/nit findings, total blockers, label status.
- **One detail comment per kind** that produced findings: grouped by file, with `file:line` anchors, severity, description, suggested fix.

On disk:

- Bundle dir from `scripts/build-pr-bundle.sh` (contains diff, context, repo-map, stats).
- `<bundle>/logs/claude-<kind>-<run>.jsonl` — one per subprocess.
- `<bundle>/summary.md` and `<bundle>/detail-<kind>.md` — what gets posted.

## Reviewer prompts

- `bugs`, `quality` — reuse `skills/reviewer/SKILL.md` (the existing kind-aware reviewer).
- `architecture` — `prompts/architecture.md` (new): module boundaries, layering, abstraction fit; relies on `repo-map.md`.
- `tests` — `prompts/tests.md` (new): coverage of new code, test quality, missing scenarios.
- `bulletproof` — `prompts/bulletproof.md` (new): adversarial e2e, prod-only states, races, failure modes.

All prompts output the same JSON schema (see `skills/reviewer/SKILL.md`).

## Out of scope

- Codex engine (planned next iteration).
- `/critic` (separate skill).
- Routing to architector (separate iteration).
- Inline file:line review comments via `gh pr review` (this iteration uses body comments only).
