---
name: cc-help
description: "Print the claude-admin M1 pipeline reference: 9 ordered steps with a one-line purpose each. Use when the user runs /cc-help, asks 'what skills are in the pipeline', wants the M1 pipeline reference, or needs a quick map of available pipeline skills."
---

# /cc-help

Entry-point reference for the claude-admin M1 pipeline. Prints the 9 ordered
pipeline steps with a one-line purpose each. A newcomer can clone the repo,
run `./install.sh`, then `/cc-help` to see the exact pipeline in 30 seconds.

## How to respond

Print the block below verbatim. Do not paraphrase, reorder, or omit steps.
Steps marked `(planned)` haven't shipped yet — leave the marker in place.

```
claude-admin M1 pipeline (9 steps)
==================================

 1. /roadmap-plan     - Plan the roadmap: high-level milestones for a multi-month effort  (planned)
 2. /milestone        - Turn one milestone into a PRD with deliverables + validations  (planned)
 3. /to-issues        - Break a plan, spec, or PRD into independently-grabbable issues on the project issue tracker using tracer-bullet vertical slices
 4. /architector      - Per-slice runner: PR breakdown + plan-integrity owner  (planned)
 5. /coder            - Internal skill loaded by /dispatch as --append-system-prompt
 6. /review           - Post-publish code-quality + bugs review on a PR  (planned)
 7. /critic           - Internal skill loaded by the watcher when fanning out PR critiques
 8. /pr-babysit       - User-facing skill for the post-review decision on a dispatched task
 9. /distill-lessons  - Post-merge: append distilled rules to module LESSONS.md  (planned)
```

## Maintenance

When a `(planned)` slice ships, drop the marker and update the one-liner from
the new skill's frontmatter `description:` (first sentence). When an installed
pipeline skill's description changes meaningfully, update the matching line
here. Source of truth for each step's purpose is that step's own SKILL.md.
