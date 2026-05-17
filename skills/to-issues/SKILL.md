---
name: to-issues
description: Break a plan, spec, or PRD into independently-grabbable issues on the project issue tracker using tracer-bullet vertical slices. Use when user wants to convert a plan into issues, create implementation tickets, or break down work into issues.
---

# To Issues

Break a plan into independently-grabbable issues using vertical slices (tracer bullets).

The issue tracker and triage label vocabulary should have been provided to you — run `/setup-matt-pocock-skills` if not.

## Process

### 1. Gather context

Work from whatever is already in the conversation context. If the user passes an issue reference (issue number, URL, or path) as an argument, fetch it from the issue tracker and read its full body and comments.

### 2. Explore the codebase (optional)

If you have not already explored the codebase, do so to understand the current state of the code. Issue titles and descriptions should use the project's domain glossary vocabulary, and respect ADRs in the area you're touching.

### 3. Draft vertical slices

Break the plan into **tracer bullet** issues. Each issue is a thin vertical slice that cuts through ALL integration layers end-to-end, NOT a horizontal slice of one layer.

Slices may be 'HITL' or 'AFK'. HITL slices require human interaction, such as an architectural decision or a design review. AFK slices can be implemented and merged without human interaction. Prefer AFK over HITL where possible.

<vertical-slice-rules>
- Each slice delivers a narrow but COMPLETE path through every layer (schema, API, UI, tests)
- A completed slice is demoable or verifiable on its own
- Prefer many thin slices over few thick ones
</vertical-slice-rules>

For each slice, draft the following fields (these are the same fields rendered into the published issue body in step 7):

<slice-draft-template>
- **Title** — short descriptive name, prefixed with the slice id (e.g. `S5 · /to-issues + enrichment`)
- **Type** — `AFK` or `HITL`
- **Deliverable** — one or two sentences. What observably exists after this slice merges? Describe end-to-end behaviour, not layer-by-layer implementation.
- **E2E covered** — which end-to-end validations (Vn in the PRD, kind `_e2e_`) this slice exercises. May be empty.
- **Module-test** — which module-level validations (Vn in the PRD, kind `_module_`) this slice exercises. May be empty.
- **Definition of done** — checkbox list combining (a) slice-specific acceptance criteria and (b) the named validations from the two fields above. Each item must be independently verifiable.
- **Modules touched** — module names this slice creates or updates (used by the enrichment phase to look up matrix rows and `modules/<name>/LESSONS.md`).
- **Blocked by** — other slice ids that must merge first, or "None".
- **User stories covered** — if the source material has them, which user stories this addresses.
</slice-draft-template>

### 4. Quiz the user

Present the proposed breakdown as a numbered list. For each slice, show: Title, Type, Blocked by, User stories covered (if applicable), and a one-line summary of the Deliverable.

Ask the user:

- Does the granularity feel right? (too coarse / too fine)
- Are the dependency relationships correct?
- Should any slices be merged or split further?
- Are the correct slices marked as HITL and AFK?

Iterate until the user approves the breakdown.

### 5. Enrich each slice (before publish)

For each approved slice, **enrich** the draft body with inlined context so the downstream coder starts pre-loaded (BMAD SM-compiler pattern, PRD #16 G5). Enrichment is mechanical, not creative: it copies excerpts from upstream artifacts into the slice body so the coder does not need to re-fetch them.

The canonical implementation is `skills/_lib/slice_enricher.py`:

```python
from slice_enricher import enrich

body = enrich(
    slice_draft,       # dict with the fields from the <slice-draft-template>
    prd,               # full PRD markdown body (string)
    modules_md="",     # optional extra module-impact-matrix markdown
    lessons=lessons,   # optional dict {module_name: LESSONS.md body}
)
```

For each slice, the enricher produces a `## Context (enriched)` block containing three sub-sections:

1. **PRD excerpts** — the verbatim Gn block(s) the slice's validations cover, lifted from the PRD's `## deliverables` section. Wrapped in `<details>` so the issue stays scannable.
2. **Module-impact matrix** — rows from the PRD's `## modules to CREATE` / `## modules to UPDATE` tables whose `name` or path segment matches an entry in the slice's **Modules touched** field. The rendered table tags each row with its source section (CREATE / UPDATE).
3. **Neighbouring lessons** — the body of `modules/<name>/LESSONS.md` for each module in **Modules touched**, when the file exists. Caller responsibility: read each file from disk into the `lessons` dict before calling `enrich()`; skip silently if the file is missing.

If the script is unavailable (e.g. running in pure-prompt mode), construct the same three sub-sections by hand using the rules above. The published slice body must contain the `## Context (enriched)` heading either way.

### 6. Fidelity critique (3 rounds)

Run an automated fidelity critique **between enrichment and publish**. The critique converges fast and lets you publish without re-asking the user; if it cannot converge in 3 rounds, escalate.

Each round:

1. If the `/analyze` skill is available, invoke it on `(PRD, enriched slice bodies)` and read its structured drift report.
2. Otherwise, run the inline structural self-check below.
3. Apply any fixes the round produces. Re-enrich slices whose Modules touched / validations changed.
4. Stop early if the round produces **zero findings**.

After round 3, if findings remain, surface them to the user and wait for direction. Do not publish until either (a) a round produced zero findings, or (b) the user explicitly acknowledged the residual findings.

<inline-structural-self-check>
- **PRD coverage**: every `Gn` in the PRD's `## deliverables` section appears in at least one slice's PRD excerpts. Missing Gs → either add a slice or surface the gap.
- **Validation coverage**: every `Vn` in the PRD's `## validations` section appears in at least one slice's **E2E covered** or **Module-test** field. Missing Vs → assign to a slice or surface the gap.
- **Backreference integrity**: every `Vn` a slice claims to cover exists in the PRD. Every `Gn` referenced via a cited V also exists in the PRD.
- **Definition-of-done completeness**: each slice's Definition of done includes one checkbox per cited V (the enricher does this automatically; verify it).
- **Module-matrix integrity**: every module name in a slice's **Modules touched** resolves to at least one matrix row (in the PRD or the supplied `modules_md`), OR the slice notes in **Notes** why no matrix row exists yet.
- **No empty enrichments**: no slice has all three Context sub-sections rendered as `_None._` / `_No matching ..._`. If it does, the slice is mis-tagged — fix its Modules touched / validations.
- **Dependency sanity**: every slice listed in any other slice's **Blocked by** exists in the breakdown and is published before its dependents.
</inline-structural-self-check>

### 7. Publish the issues to the issue tracker

For each approved-and-critiqued slice, publish a new issue to the issue tracker. Use the issue body template below. These issues are considered ready for AFK agents, so publish them with the correct triage label unless instructed otherwise.

Publish issues in dependency order (blockers first) so you can reference real issue identifiers in the **Blocked by** field.

<issue-template>
## Parent

A reference to the parent issue on the issue tracker (if the source was an existing issue, otherwise omit this section).

## Deliverable

One or two sentences describing the end-to-end behaviour this slice delivers.

## E2E covered

- **Vn** `validation_name` — covers Gn

Or `_None._` if this slice has no e2e validations.

## Module-test

- **Vn** `validation_name` — covers Gn

Or `_None._` if this slice has no module-level validations.

## Definition of done

- [ ] Slice-specific criterion 1
- [ ] Slice-specific criterion 2
- [ ] **Vn** (kind) `validation_name` passes

## Context (enriched)

### PRD excerpts

<details><summary>Gn · deliverable title</summary>

verbatim Gn block from PRD

</details>

### Module-impact matrix

| section | name | path | responsibility | interface | tests |
|---|---|---|---|---|---|
| CREATE | module-name | `skills/path/file.py` | one-line responsibility | `signature()` | Vn |

### Neighbouring lessons

<details><summary>modules/&lt;name&gt;/LESSONS.md</summary>

verbatim lessons file body

</details>

## Blocked by

- A reference to the blocking ticket (if any)

Or "None — can start immediately." if no blockers.
</issue-template>

Do NOT close or modify any parent issue.
