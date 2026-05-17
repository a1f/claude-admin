---
name: critic
description: "Multi-agent goal-fit critic for a PR via Claude. Fans out N parallel critic subprocesses, each scoring the PR 1-100 on whether it actually achieves its stated task (not whether the code is well-written — that's /review). Aggregates median score, derives verdict, unions concerns, posts one summary comment to the PR. Use when the user asks to /critic a PR, get a goal-fit verdict on a PR, or check whether a PR matches its task. Examples: '/critic 26', '/critic 26 --runs=5', '/critic --bundle /path/to/bundle --no-post'."
argument-hint: "<PR#> [--runs=N] [--bundle DIR] [--no-post] [--repo OWNER/NAME]"
---

# /critic skill

Multi-agent goal-fit critic. For one PR, fan out N parallel Claude critic
subprocesses. Each scores the PR 1–100 on the question: **does this PR actually
achieve the task it claims to?** Aggregate to a median score, derive the
verdict, union the concerns, and post one summary comment to the PR.

Goal-fit only. **Code quality is /review's job.** A messy implementation that
meets the task gets a high critic score; a beautiful implementation that
solves the wrong problem gets a low one.

## Usage

```
/critic <PR#> [--runs=N] [--no-post] [--repo OWNER/NAME]
/critic --bundle DIR [--runs=N] [--no-post]
```

Defaults: `--runs=3`. With defaults, that's 3 Claude subprocesses in parallel.

- `--bundle DIR` — use a pre-built context bundle directory (must contain
  `pr-diff.patch` and `pr-context.md` at minimum). Used by the golden test
  harness; lets the critic run offline.
- `--no-post` — skip posting to the PR. Implied by `--bundle`.

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

## Out of scope

- Codex engine (planned next iteration).
- Routing to architector via label (`for-architector`) — planned.
