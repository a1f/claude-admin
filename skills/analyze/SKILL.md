---
name: analyze
description: Cross-artifact consistency gate. Takes roadmap / PRD / slice list / PR table and reports missing-coverage, drift, and inconsistencies. Idempotent, pure parser, no LLM. Use when the user invokes /analyze, or call from /to-prd, /to-issues, /architector critique loops to catch drift between artifacts (e.g. a PRD goal with no slice covering it).
argument-hint: "--prd <ref> [--slices <ref>] [--pr-table <ref>] [--roadmap <ref>]"
---

# Analyze skill

Deterministic cross-artifact consistency gate for the M1 skills pipeline. Catches drift between roadmap, PRD, slice breakdown, and PR table — the classes of inconsistency a single-doc fidelity critique misses.

## When to invoke

- Standalone: `/analyze --prd <ref> --slices <ref> [--pr-table <ref>] [--roadmap <ref>]`
- Inside `/to-prd` critique loop: after PRD draft is structured, before publish.
- Inside `/to-issues` 3-round critique: after slice draft, before publish.
- Inside `/architector` re-planning: after slice issue body or PR table edits.

## Inputs

Each `<ref>` is one of:

- a local file path (e.g. `./drafts/prd.md`)
- a `gh:OWNER/REPO#N` shorthand (e.g. `gh:a1f/claude-admin#16`)
- a full GitHub issue URL (e.g. `https://github.com/a1f/claude-admin/issues/16`)

GitHub refs are fetched via `gh issue view N --repo OWNER/REPO --json body`. At least one artifact must be supplied.

## Steps

### 1. Run the analyzer

```bash
python3 /Users/alf/.claude/skills/analyze/scripts/analyze.py \
    --prd       <ref> \
    --slices    <ref> \
    [--pr-table <ref>] \
    [--roadmap  <ref>] \
    [--format   markdown|json]
```

The script:

- Parses PRD `## deliverables` (Gn) and `## validations` (Vn, with `covers Gn` chain)
- Parses slice list (markdown table rows + `### Sn ·` headings + `**Validations referenced:**` / `**Covers:**` lines)
- Parses PR table rows (extracting referenced slice ids)
- Runs three detectors: missing-coverage, drift, inconsistencies
- Prints a sorted, deterministic markdown (or JSON) report
- Exits `0` if clean, `1` if any issues, `2` on argument/fetch error

### 2. Pass the output through verbatim

The script's stdout is already formatted. Don't editorialize or reformat. If the exit code is `1`, briefly note: "Issues found — see report above" so the caller knows to act.

### 3. (Critique-loop callers only) Branch on exit code

When invoked from `/to-prd`, `/to-issues`, or `/architector`, treat exit `0` as "ready to publish/dispatch" and exit `1` as "fix and re-run". The report names exactly which Gn / Vn / Sn is missing or dangling — pass that back to the upstream skill so the next iteration can address it.

## What it reports

| Category | Example |
|---|---|
| **missing-coverage** | `no slice covers G3 (referenced by V9): grill-with-docs` |
| **drift** | `slice S2 references V99 not in PRD` · `V9 covers G99 but G99 not in PRD` · `PR PR2 references S99 which is not in the slice list` |
| **inconsistencies** | `G numbering gap: G2 missing (have G1..G3)` |

## Notes

- **Pure parser, no LLM.** Same inputs → byte-identical report. Safe in critique loops without runaway costs.
- **Coverage chain:** a slice covers `Gn` iff (a) its body has `**Covers:** Gn`, or (b) its `**Validations referenced:**` line cites a `Vm` whose PRD entry says `covers Gn`. This matches the templates used by `/to-prd` (S4) and `/to-issues` (S5).
- **Idempotence is a hard guarantee.** If you find a case where the same inputs produce different reports, that's a bug — file it against this skill.
- **Tests:** `skills/analyze/scripts/test_analyze.py` (V9 golden + drift + inconsistency + idempotence). Run with `uvx pytest skills/analyze/scripts/test_analyze.py`.

## Invocation references

- Spec: [a1f/claude-admin#17 §S6](https://github.com/a1f/claude-admin/issues/17)
- PRD: [a1f/claude-admin#16 §G6, §V9](https://github.com/a1f/claude-admin/issues/16)
- Module name in PRD modules-to-CREATE table: `analyze-engine`
