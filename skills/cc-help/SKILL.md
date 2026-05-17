---
name: cc-help
description: "Print the claude-admin M1 pipeline reference: 9 ordered steps with a one-line purpose each. Use when the user runs /cc-help, asks 'what skills are in the pipeline', wants the M1 pipeline reference, or needs a quick map of available pipeline skills."
---

# /cc-help

Entry-point reference for the claude-admin M1 pipeline. Renders the 9 ordered
pipeline steps with a one-line purpose for each, pulled from every step's
own SKILL.md frontmatter.

## How to invoke

Run the renderer and print its output verbatim. Do not paraphrase, reorder, or
omit steps -- this command exists so a newcomer can clone the repo and see the
exact pipeline in 30 seconds.

```bash
python3 "$(dirname "$(readlink -f "${BASH_SOURCE[0]:-$0}")")/../scripts/render.py"
```

Equivalent form when invoked from outside the skill directory:

```bash
python3 <repo>/skills/cc-help/scripts/render.py
```

## What the renderer does

1. Walks `<repo>/skills/*/SKILL.md`.
2. Filters to the 9 ordered pipeline skills (see `PIPELINE` in `scripts/render.py`).
3. For each step, extracts the first sentence of the `description:` frontmatter
   field and prints `<n>. /<name>  -  <one-line purpose>`.
4. For pipeline skills not yet installed (the slice that builds them hasn't
   shipped), prints the fallback purpose with a `(planned)` marker.

## Tests

```bash
python3 -m unittest discover -s skills/cc-help/tests
```

Covers V1 (`output contains 'architector'`) and V10 (`all 9 steps in order,
each with a one-line purpose`).
