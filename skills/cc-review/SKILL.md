---
name: cc-review
description: "Multi-agent PR review via Claude and/or Codex. Fans out parallel reviewers across 5 kinds (bugs, quality, architecture, tests, bulletproof) and 1 or 2 engines, aggregates with cross-engine dedup, posts a side-by-side summary table + per-kind detail comments (each finding tagged with the engine(s) that flagged it), and applies a CRITICAL label if any blocker is found. Named cc-review to avoid the built-in /review slash command. Use when the user asks to /cc-review a PR, multi-review a PR, or wants reviewer agents to look at a published PR. Examples: '/cc-review 26', '/cc-review 26 --kinds=bugs,tests', '/cc-review 26 --engine=both', '/cc-review 26 --runs=1'."
argument-hint: "<PR#> [--kinds=...] [--runs=N] [--engine=claude|codex|both] [--bundle DIR] [--no-post] [--tmux]"
---

# /cc-review skill

Multi-agent PR review. For one PR, fan out parallel reviewer subprocesses across
multiple "kinds" (bugs, quality, architecture, tests, bulletproof), N independent
runs per kind per engine, using one or both of Claude and Codex. Aggregate per
engine (deduped) and unioned across engines, post one summary comment plus one
detail comment per kind. Apply `CRITICAL` label if any reviewer reports a blocker.

Named `cc-review` to avoid colliding with Claude Code's built-in `/review` slash
command.

## Usage

```
/cc-review <PR#> [--kinds=bugs,quality,architecture,tests,bulletproof]
                 [--runs=N] [--engine=claude|codex|both]
                 [--bundle DIR] [--no-post] [--tmux] [--repo OWNER/NAME]
```

Defaults: `--kinds=bugs,quality,architecture,tests,bulletproof`, `--runs=3`, `--engine=claude`.
With all defaults plus `--engine=both`, that's `5 × 3 × 2 = 30` subprocesses in parallel.
Use `--runs=1` or narrower `--kinds` for a cheaper pass.

- `--engine=claude` (default) — only Claude reviewers.
- `--engine=codex` — only Codex reviewers (faster, different lens).
- `--engine=both` — both engines; side-by-side columns + cross-engine dedup;
  agreement between engines is a confidence signal.
- `--bundle DIR` — use a pre-built context bundle directory (implies `--no-post`).
- `--no-post` — skip posting to the PR; useful for inspection / dry runs.
- `--tmux` — stub (prints a note and runs without it).

## How to invoke

Hand the run off to the orchestration script — it does everything end-to-end:

```bash
bash "${CLAUDE_PLUGIN_ROOT:-$HOME/.claude/skills/cc-review}/scripts/run.sh" <PR#> [flags]
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

## Out of scope (this iteration)

- Routing to architector (label-based handoff): planned separately.
- Inline file:line review comments via `gh pr review`: this iteration uses body
  comments only (simpler, sufficient for the multi-agent table format).
