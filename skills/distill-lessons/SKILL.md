---
name: distill-lessons
description: "Post-merge per-module lesson distiller. Reads a merged PR's /cc-review + /critic verdict comments plus its diff, identifies touched modules, and for each module revisits modules/<name>/LESSONS.md (read existing, merge new evidence, dedup, refine, write back). Compound learning: future /coder runs read the relevant module's LESSONS.md during plan-pr. Use when a PR has just merged. Examples: '/distill-lessons 29', '/distill-lessons 29 --no-write', '/distill-lessons --bundle DIR --no-write'."
argument-hint: "<PR#> [--no-write] [--modules a,b] [--bundle DIR] [--repo OWNER/NAME]"
---

# /distill-lessons skill

Post-merge skill. For one merged PR, fan out one Claude subprocess per touched
module to **revisit** that module's `modules/<name>/LESSONS.md`: read what's
already there, fold in the new evidence from this PR's `/cc-review` and
`/critic` comments + diff, dedup near-duplicate rules, drop rules now
contradicted, sharpen wording, write back the full revised file.

Revisit, not append. The skill is a learning loop, not an audit log — each
LESSONS.md is the *current best understanding* of how to work in that module,
not a chronological pile.

Future `/coder` runs touching that module load `modules/<name>/LESSONS.md`
during plan-pr / self-review. That is how compound learning kicks in.

## Usage

```
/distill-lessons <PR#> [--no-write] [--modules a,b,...] [--bundle DIR] [--repo OWNER/NAME]
/distill-lessons --bundle DIR --no-write
```

- `<PR#>` — merged PR number (skill warns if not merged but proceeds).
- `--no-write` — print proposed revised LESSONS to stdout / bundle dir; do not
  modify `modules/<name>/LESSONS.md`. Always implied by `--bundle`.
- `--modules a,b` — restrict to these module names (comma-separated). Default:
  every module the PR touched.
- `--bundle DIR` — use a pre-built PR bundle (must contain `pr-diff.patch`,
  `pr-context.md`; will fetch comments if `pr-comments.md` missing). Used by
  offline tests. Implies `--no-write`.
- `--repo OWNER/NAME` — forwarded to `gh` (otherwise auto-detected).

## How to invoke

Hand the run off to the orchestration script — it does everything end-to-end:

```bash
bash "${CLAUDE_PLUGIN_ROOT:-$HOME/.claude/skills/distill-lessons}/scripts/run.sh" <PR#> [flags]
```

Show the script's output to the user verbatim. It prints:

- Bundle directory path
- Detected module list (with per-module changed-file count)
- Per-subprocess pid + log path
- Per-module: old LESSONS.md size → new LESSONS.md size, and the diff path
- Final summary: modules updated / unchanged / skipped

## Module derivation

A "module" is what gets one `LESSONS.md` file. Run.sh maps each touched file
to its module name using these rules (first match wins):

| Path pattern                  | Module name             |
|-------------------------------|--------------------------|
| `skills/<x>/...`              | `skills/<x>`             |
| `crates/<x>/...`              | `crates/<x>`             |
| `docs/agents/...`             | `docs/agents`            |
| `scripts/...`                 | `scripts`                |
| `v1_orchestrator/...`         | `v1_orchestrator`        |
| `v2_design/...`               | `v2_design`              |
| `tests/...`                   | `tests`                  |
| `.github/...`                 | `.github`                |
| (anything else at repo root)  | `root`                   |

LESSONS files land at `modules/<name>/LESSONS.md`. Slashes in `<name>` become
directories (e.g. `skills/distill-lessons` → `modules/skills/distill-lessons/LESSONS.md`).

## What it produces

On disk (always):

- Bundle dir from `scripts/build-pr-bundle.sh` (`pr-diff.patch`, `pr-context.md`, etc.)
- `<bundle>/pr-comments.md` — concatenated `/cc-review` + `/critic` verdict bodies.
- `<bundle>/modules.txt` — one module name per line, deduped, sorted.
- `<bundle>/logs/distill-<module-safe>.{out,err}` — per-module subprocess logs.
- `<bundle>/proposed/<name>/LESSONS.md` — new content claude produced per module.

With write (default): `modules/<name>/LESSONS.md` is overwritten with the
proposed content (the old version remains in git history).

With `--no-write`: only the bundle dir is touched.

## Distill prompt

See `prompts/distill.md`. It is appended via `--append-system-prompt` for each
per-module subprocess. The prompt fully governs:

- Read existing `modules/<name>/LESSONS.md` (or treat as empty if missing).
- Read PR diff for files in this module.
- Read `/cc-review` + `/critic` verdict text (concerns, findings, rationale).
- Produce one revised LESSONS.md as terse, declarative rules. Dedup. Drop
  rules contradicted by new evidence. Keep rules still valid.
- Output the full file contents (plain markdown) — no fences, no preamble.

## Out of scope (this iteration)

- Cross-module learning (a rule that applies broadly across modules). Each
  invocation writes per-module files only.
- Auto-PR of the LESSONS update back to main. The skill writes locally; the
  user (or a downstream wrapper) commits + pushes.
- Inferring "module" from anything richer than path prefix. A repo with real
  module manifests would override this with a config later.
