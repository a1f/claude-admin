# Domain docs layout

Single-context repo. Domain language and architectural decisions live at the repo root:

- `CONTEXT.md` — domain glossary (terms, what they mean here). Created lazily by `/grill-with-docs` when the first term is resolved.
- `docs/adr/` — Architecture Decision Records, numbered (`0001-*.md`, `0002-*.md`, ...). Created lazily by `/grill-with-docs` when an ADR is needed.

Per-module learning:

- `modules/<name>/LESSONS.md` — terse rules **revisited and distilled** by `/distill-lessons` after PR merges (read existing file, fold in new evidence from `/cc-review` + `/critic` verdict comments, dedup, sharpen, write back the full revised file — not blind append). Loaded by `/coder` on plan-pr for any PR touching that module.
