---
name: critic
description: "Multi-agent goal-fit critic for a PR via Claude and/or Codex. Fans out N parallel critic subprocesses per engine, each scoring the PR 1-100 on whether it actually achieves its stated task (not whether the code is well-written — that's /cc-review). Aggregates median score per engine plus a cross-engine consensus, derives verdict from the median, unions concerns. Posts one summary comment to the PR. Use when the user asks to /critic a PR, get a goal-fit verdict on a PR, or check whether a PR matches its task. Examples: '/critic 26', '/critic 26 --engine=both', '/critic 26 --runs=5', '/critic --bundle /path/to/bundle --no-post'."
argument-hint: "<PR#> [--runs=N] [--engine=claude|codex|both] [--bundle DIR] [--no-post] [--repo OWNER/NAME]"
---

# /critic skill

Multi-agent goal-fit critic. For one PR, fan out N parallel critic subprocesses
per engine (Claude and/or Codex). Each scores the PR 1–100 on the question:
**does this PR actually achieve the task it claims to?** Aggregate to a per-engine
median, then a cross-engine consensus median; derive the verdict from the
consensus; union the concerns; and post one summary comment to the PR.

Goal-fit only. **Code quality is /cc-review's job.** A messy implementation that
meets the task gets a high critic score; a beautiful implementation that
solves the wrong problem gets a low one.

## Usage

```
/critic <PR#> [--runs=N] [--engine=claude|codex|both] [--no-post] [--repo OWNER/NAME]
/critic --bundle DIR [--runs=N] [--engine=...] [--no-post]
```

Defaults: `--runs=3`, `--engine=claude`. With `--engine=both --runs=3`, that's
6 subprocesses in parallel.

- `--engine=claude` (default) — Claude only.
- `--engine=codex` — Codex only.
- `--engine=both` — both engines; per-engine medians + a consensus row in the
  output table.
- `--bundle DIR` — use a pre-built context bundle directory (must contain
  `pr-diff.patch` and `pr-context.md` at minimum). Used by the golden test
  harness; lets the critic run offline. Implies `--no-post`.
- `--no-post` — skip posting to the PR.

## How to invoke

Hand the run off to the orchestration script:

```bash
bash "${CLAUDE_PLUGIN_ROOT:-$HOME/.claude/skills/critic}/scripts/run.sh" <PR#> [flags]
```

Show the script's output to the user verbatim. It prints:

- Bundle directory path
- Per-subprocess pids and where their JSONL logs land
- Aggregate summary path
- URL of the posted summary comment (if posting)
- Final verdict + score

## What it produces

In the PR (unless `--no-post`):

- **One summary comment** with: median score, verdict, per-axis score table,
  representative rationale, union of concerns across runs.

On disk:

- Bundle dir from `scripts/build-pr-bundle.sh` (contains diff, context, etc.).
- `<bundle>/logs/claude-critic-<run>.jsonl` — one per subprocess.
- `<bundle>/summary.md` and `<bundle>/summary.json`.

## Critic prompt

`prompts/agent.md` (was `skills/critic/SKILL.md` in earlier iterations).
Same JSON output schema: `{score, verdict, axes, rationale_md, concerns}`.

## Out of scope (this iteration)

- Routing to architector via label (`for-architector`) — planned separately.
