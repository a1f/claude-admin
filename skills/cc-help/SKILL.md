---
name: cc-help
description: "Print the claude-admin M1 pipeline reference: 9 ordered steps with a one-line purpose each, sourced live from each step's own SKILL.md. Use when the user runs /cc-help, asks 'what skills are in the pipeline', wants the M1 pipeline reference, or needs a quick map of available pipeline skills."
---

# /cc-help

Entry-point reference for the claude-admin M1 pipeline. Prints the 9 ordered
pipeline steps with a one-line purpose each. A newcomer can clone the repo,
run `./install.sh`, then `/cc-help` and see the pipeline in 30 seconds.

## Pipeline (canonical order)

These 9 skill names, in this exact order, are the M1 pipeline:

1. `roadmap-plan`
2. `milestone`
3. `to-issues`
4. `architector`
5. `coder`
6. `review`
7. `critic`
8. `pr-babysit`
9. `distill-lessons`

This list is the source of truth for the pipeline itself. The one-line
*purpose* of each step is owned by that step's own `skills/<name>/SKILL.md`.

## How to respond

When `/cc-help` is invoked:

1. Locate the repo root (the directory containing this `skills/cc-help/`
   skill). Use `git rev-parse --show-toplevel` if unsure.
2. For each of the 9 names above, in order:
   - If `<repo>/skills/<name>/SKILL.md` exists, read its YAML frontmatter,
     take the `description:` value, and use the **first sentence** (text up
     to the first `. ` or end of string) as the one-line purpose.
   - If the file does not exist, use the fallback purpose from the table
     below and append the marker `(planned)`.
3. Print the result as a numbered list, one per line, in this shape:

   ```
   claude-admin M1 pipeline (9 steps)
   ==================================

    1. /<name>         - <one-line purpose>[  (planned)]
    ...
    9. /<name>         - <one-line purpose>[  (planned)]
   ```

   Pad the `/<name>` column so the ` - ` separators line up.

Do **not** invent steps, reorder, paraphrase the live descriptions, or omit
the `(planned)` marker.

## Fallback one-liners (used only when the skill is not yet installed)

| # | Name | Fallback purpose |
|---|------|------------------|
| 1 | roadmap-plan    | Plan the roadmap: high-level milestones for a multi-month effort |
| 2 | milestone       | Turn one milestone into a PRD with deliverables + validations |
| 3 | to-issues       | Break a PRD into vertical-slice issues with enriched context |
| 4 | architector     | Per-slice runner: PR breakdown + plan-integrity owner |
| 5 | coder           | Implement one PR with plan-pr + write-pr + self-review |
| 6 | review          | Post-publish code-quality + bugs review on a PR |
| 7 | critic          | Post-publish "addresses task?" verdict (NOT quality) |
| 8 | pr-babysit      | Watch PR lifecycle, route verdicts, diagnose on CI red |
| 9 | distill-lessons | Post-merge: append distilled rules to module LESSONS.md |

## Maintenance — read this before changing the pipeline

This file is the **only** place the pipeline shape lives. If the pipeline
changes, this file must change in the same PR. Specifically:

- **Adding a step** → add the name to the canonical ordered list above, add
  a fallback one-liner to the table, update the count in the header (`9
  steps` → `N steps`) and in this skill's frontmatter `description:`.
- **Removing a step** → delete from the ordered list, delete from the
  fallback table, update the count.
- **Reordering** → reorder the list (the table is alphabetical-free; order
  there just mirrors the list for readability — keep them in sync).
- **Renaming a step** → rename in the ordered list and the fallback table.
  The live description will follow automatically once the renamed skill's
  SKILL.md ships.

A pipeline change that doesn't update this file is a bug in that PR.
