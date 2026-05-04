---
name: suggest
description: "Scan a plan's milestone breakdown(s) and report which tasks are dispatchable (all blockers satisfied) and which are blocked. Outputs ready-to-run dispatch commands. Use when the user asks 'what can I dispatch?' / 'what tasks are ready?' / 'what's unblocked?' / 'show me dispatchable work' / invokes /suggest. Examples: '/suggest v2_design', '/suggest v2_design M0a'."
argument-hint: "<plan-codename> [<milestone-id>]"
---

# Suggest skill

Run the deterministic dispatchable-task scanner.

## Inputs

`/suggest <plan-codename> [<milestone-id>]`

Examples: `/suggest v2_design` · `/suggest v2_design M0a`

If `<plan-codename>` is missing, ask via AskUserQuestion (read keys of `~/.claude/plans/registry.json` → `plans` for options). Milestone id is optional — if omitted, scan all broken-down milestones.

## Steps

### 1. Run the scanner

```bash
python3 /Users/alf/.claude/skills/suggest/scripts/suggest.py <plan-codename> [<milestone-id>]
```

The script:

- Reads `~/.claude/plans/registry.json` to resolve the codename
- Reads `<plan_dir>/breakdowns/<milestone-id>.md` for the task list
- Parses each task's `Blockers:` line
- Queries `gh` to check each blocker's condition
- Prints dispatchable tasks (with their copy-pasteable dispatch commands) + blocked tasks (with which blocker(s) are unmet)
- Exits 0 if at least one task is dispatchable, 1 otherwise

### 2. Show the output

Pass through the script's stdout to the user **verbatim**. It's already formatted. Don't editorialize, don't reformat.

If exit code != 0, briefly state at the end: "Nothing dispatchable right now."

### 3. (Optional) Offer to dispatch

If exactly one task is dispatchable, ask via AskUserQuestion if they want to run the dispatch command immediately. Defer actually running dispatch to the `/dispatch` skill — this skill only suggests.

## Notes

- The script is the source of truth. Don't try to reproduce its logic in chat.
- Blockers are parsed from the local breakdown markdown (`<plan_dir>/breakdowns/<id>.md`). If the local file is suspected stale relative to GH, surface a warning.
- This skill never modifies state. Pure read.

## Blocker syntax

Format expected on the `**Blockers:** ...` line in each task:

- `none` — task has no blockers, dispatchable immediately
- `<task-id> <state>` — predecessor task in given state. States: `merged`, `drafted`, `ready`
- `label:<label-name>` — parent breakdown issue must carry the label
- Multiple blockers separated by `; ` (semicolon + space)

Examples:
- `Blockers: none`
- `Blockers: M0a-T1 merged`
- `Blockers: M0a-T1 merged; M0a-T2 drafted; label:design-approved`
