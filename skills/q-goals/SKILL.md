---
name: q-goals
description: "Draft + ratify the immutable goals and validation scenarios for a milestone. Manual mode (MVP): opens templates in $EDITOR, runs structure validators on save, freezes the files (chmod 444 + checksum) on ratify. Use when the user invokes /q-goals or asks to define goals for a milestone/task/issue. Examples: '/q-goals ca_v1 Phase1', 'draft goals for M2', 'set up goals + validations for milestone X'."
argument-hint: "<plan-codename> <milestone-id>"
---

# q-goals

Draft + ratify the immutable **goals** and **validations** for one milestone.

The output is a contract: once ratified, the goal set is frozen and every subsequent task is measured against it. This is how the architect (you) is held accountable to a stable definition of "done."

## Inputs

`/q-goals <plan-codename> <milestone-id>`

Examples:
- `/q-goals ca_v1 Phase1`
- `/q-goals ca_v1 M3`

If args missing, ask via AskUserQuestion. To find a plan codename, look in `~/.claude/plans/registry.json`.

## Mode

Manual mode only for MVP. No LLM agents — you are the architect. The skill:

1. Resolves `<plan-codename>` → `<plan_dir>` via the registry.
2. Creates `<plan_dir>/<milestone-id>/` if missing.
3. Writes `goals.md` and `validations.md` from templates if they don't already exist.
4. **Refuses to proceed if `.ratified.json` is present.** Goals are immutable post-ratify.
5. Opens `goals.md` in `$EDITOR` (default `vi`). On editor exit, runs the goals validator.
6. Opens `validations.md` in `$EDITOR`. On editor exit, runs the validations validator + cross-reference (every `G<N>` in validations must exist in goals).
7. Loops the editor on validator failure with errors printed.
8. On all validators clean: prints summary and asks `ratify? [y/N]` on stdin.
9. On `y`: computes sha256 of both files, writes `.ratified.json` (with `ratified_at`, hashes, plan, milestone-id), `chmod 444` both files.

LLM-driven mode (architect + writer + critique agents) is deferred. The structure is designed so the manual flow drops cleanly into agent mode later by replacing the editor step with a writer-agent + critique-agent loop.

## Files written

Under `<plan_dir>/<milestone-id>/`:

```
goals.md          ← deliverables (immutable after ratify)
validations.md    ← test scenarios (immutable after ratify)
.ratified.json    ← {ratified_at, plan, milestone, goals_sha256, validations_sha256}
```

## Structure required

### `goals.md`

```
## Deliverables

- [ ] **G1** · <short name>
  - **Observable when:** <concrete signal>
  - **Why:** <one line>
```

Validator rules:
- `## Deliverables` section is present.
- Every `- [ ] **G<N>** ·` bullet has `**Observable when:**` and `**Why:**` lines in the same block.
- `G<N>` are unique and sequential from 1.
- No template placeholders (`<short name>`, `<concrete signal …>`, `<one line>`, etc.) left in the file.

### `validations.md`

```
## Scenarios

- **V1** · _unit_ — `test_name_snake_case` — covers G1
  - **What it tests:** <one line>
  - **How:** <test path or shell command>
```

Validator rules:
- `## Scenarios` section is present.
- Every `- **V<N>** ·` bullet matches the kind/name/covers pattern.
- Every `G<N>` referenced must exist in `goals.md`.
- No template placeholders remain.
- (Warning, not blocker) every goal in `goals.md` should be covered by ≥1 scenario.

## Ratify

Once you press `y` at the prompt:

- `chmod 444` is applied to `goals.md` and `validations.md`.
- `.ratified.json` is written with sha256 hashes — later code can verify the files weren't mutated post-ratify.
- The skill refuses to re-run on this milestone (`.ratified.json` exists → "frozen on ..." message).

## Unfreezing (admin escape hatch)

Out of MVP scope for `q-goals` itself. Manually: `rm <plan_dir>/<milestone-id>/.ratified.json && chmod 644 goals.md validations.md` — but this is an audit-trail violation. Use only when the goals were demonstrably wrong, and capture the reason in a commit message.

## Running it

```bash
python3 /Users/alf/dev/claude_admin/skills/q-goals/scripts/q_goals.py <plan-codename> <milestone-id>
```

The skill body above tells you what gets created and asked. Pass through script output verbatim — it prints summary lines and prompts.

## What this skill does NOT do

- No LLM invocation. No tmux. No subprocess agents. Pure local editor + validator loop.
- No sqlite. The `.ratified.json` sentinel is enough for manual mode; `MS-0` mirrors this to sqlite later.
- No git commit. The user commits the resulting markdown when ready.
- No automatic re-open after ratify. That's a separate admin command (out of MVP).
