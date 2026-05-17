---
name: analyze
description: Cross-artifact consistency check. Reads roadmap / PRD / slice list / PR table and reports missing-coverage, drift, and numbering gaps between them. Use when the user invokes /analyze, or call from /to-prd, /to-issues, /architector critique loops to catch drift between artifacts (e.g. a PRD goal with no slice covering it).
argument-hint: "--prd <ref> [--slices <ref>] [--pr-table <ref>] [--roadmap <ref>]"
---

# Analyze skill

Cross-artifact consistency gate for the M1 pipeline. Catches drift between roadmap, PRD, slice breakdown, and PR table that a single-doc fidelity critique would miss.

## When to invoke

- Standalone: `/analyze --prd <ref> --slices <ref> [--pr-table <ref>] [--roadmap <ref>]`
- Inside `/to-prd` critique loop: after PRD draft is structured, before publish.
- Inside `/to-issues` critique loop: after slice draft, before publish.
- Inside `/architector` re-planning: after slice issue body or PR table edits.

## Inputs

Each `<ref>` is one of:

- a local file path (e.g. `./drafts/prd.md`)
- a `gh:OWNER/REPO#N` shorthand (e.g. `gh:a1f/claude-admin#16`)
- a full GitHub issue URL

For GitHub refs, fetch with `gh issue view N --repo OWNER/REPO`. At least one artifact must be supplied.

## What to check

Read each supplied artifact and extract its ids:

- **Roadmap** — milestone ids (M0, M1, ...)
- **PRD** — deliverable ids (G1, G2, ...) and validation ids (V1, V2, ...), plus which Gn each Vn covers
- **Slice list** — slice ids (S1, S2, ...) and what each declares it covers (a `**Covers:** Gn` line, or a `**Validations referenced:** Vm` line where the PRD says `Vm covers Gn`)
- **PR table** — PR ids (PR1, PR2, ...) and which Sn each references

Then report findings in three categories:

| Category | Example |
|---|---|
| **missing-coverage** | `no slice covers G3 (referenced by V9)` |
| **drift** | `slice S2 references V99 not in PRD` · `V9 covers G99 but G99 not in PRD` · `PR2 references S99 which is not in the slice list` |
| **inconsistencies** | `G numbering gap: G2 missing (have G1, G3)` |

Group findings under those three headings. Within each group, sort by id so the same inputs produce the same report.

If everything checks out, say so explicitly: `analyze: clean (N goals, M validations, K slices)`.

## How to report

Output a short markdown report. Don't editorialize — just the findings. When invoked from a critique loop, the caller branches on whether the report is clean:

- clean → ready to publish/dispatch
- any findings → fix and re-run; the report names exactly which id is missing or dangling

## Notes

- **Coverage chain:** a slice covers `Gn` iff (a) its body has `**Covers:** Gn`, or (b) its `**Validations referenced:**` line cites a `Vm` whose PRD entry says `covers Gn`. This matches the templates used by `/to-prd` and `/to-issues`.
- If the artifacts don't follow those templates, say so in the report rather than guessing — the upstream skill needs to fix the format.

## References

- Spec: [a1f/claude-admin#17 §S6](https://github.com/a1f/claude-admin/issues/17)
- PRD: [a1f/claude-admin#16 §G6, §V9](https://github.com/a1f/claude-admin/issues/16)
