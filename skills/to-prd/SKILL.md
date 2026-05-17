---
name: to-prd
description: Turn the current conversation context into a PRD and publish it to the project issue tracker. Use when user wants to create a PRD from the current context.
---

This skill takes the current conversation context and codebase understanding and produces a PRD with structured Deliverables / Validations / Modules sections. Do NOT interview the user — just synthesize what you already know.

The issue tracker and triage label vocabulary should have been provided to you — run `/setup-matt-pocock-skills` if not.

## Process

1. Explore the repo to understand the current state of the codebase, if you haven't already. Use the project's domain glossary vocabulary throughout the PRD, and respect any ADRs in the area you're touching.

2. Sketch out the major modules you will need to build or modify. Actively look for opportunities to extract deep modules that can be tested in isolation.

A deep module (as opposed to a shallow module) is one which encapsulates a lot of functionality in a simple, testable interface which rarely changes.

Check with the user that these modules match their expectations. Check with the user which modules they want tests written for.

3. Draft the PRD using the template below. Save the draft to a local file (e.g. `/tmp/prd-draft.md`).

4. **Validate the draft before posting** — run:

   ```bash
   python3 skills/_lib/prd_validator.py /tmp/prd-draft.md
   ```

   If the validator reports errors, fix them in the draft and re-run until it passes. Do NOT post a PRD that fails validation — every check exists to prevent the downstream pipeline (architector / critic / coder) from working off an ambiguous spec.

5. **Critique the draft** — dispatch the 3 agents below in parallel (single message, 3 Agent tool calls). Each returns a score 1-100 and a short list of concrete fixes. If any score is < 80, apply the fixes and re-critique. Do NOT publish until all three score ≥ 80.

   Each agent receives: the draft PRD, the original user prompt that started this conversation, and the Q&A transcript from the grilling step (paste them verbatim into the prompt).

   - **Agent A — prompt fidelity.** Does the PRD actually deliver what the user asked for in the original prompt? Score 1-100. List anything in the prompt that is missing, watered down, or silently expanded.
   - **Agent B — Q&A fidelity.** Does the PRD reflect every decision made in the Q&A? Score 1-100. List any Q&A answer that is contradicted, ignored, or only partially honored.
   - **Agent C — structure & vertical slicing.** Are Deliverables observable, Validations tied to Gn, Modules concrete, Test plan substantive, and is each Gn a real vertical slice (not a horizontal layer)? Score 1-100. List structural weaknesses.

6. Publish the validated and critiqued PRD to the project issue tracker (see `docs/agents/issue-tracker.md`). Apply the `ready-for-agent` triage label — no need for additional triage.

## Template

The PRD MUST contain exactly these 7 H2 sections, in this order:

<prd-template>

## Summary

One paragraph: what is being built, for whom, and why now. Use the project's domain vocabulary. No bullet points here — prose.

## Deliverables

Numbered, observable goals. Use this format for each:

- [ ] **G1** · short name
  - observable: a concrete behavior an outside observer can verify (not "user can do X" — "running `foo` produces Y")
  - why: one-line motivation tying back to the Summary

- [ ] **G2** · short name
  - observable: ...
  - why: ...

Numbering MUST be sequential from G1 with no gaps. Each Gn is a unit of work that maps directly to a vertical slice in `/to-issues`.

## Validations

Numbered validations. Each Vn MUST cite at least one Gn that it covers. Format:

- [ ] **V1** · _kind_ — `slug` — covers G1
  - what: what is being validated
  - how: how it is verified (exact command, test name, or manual procedure)

- [ ] **V2** · _kind_ — `slug` — covers G1, G2
  - what: ...
  - how: ...

`kind` is one of `unit`, `module`, `e2e`, `manual`. Numbering MUST be sequential from V1 with no gaps. Every Gn defined above should be covered by at least one Vn (the validator does not enforce this, but `/analyze` will).

## Modules to CREATE

New modules introduced by this PRD. Markdown table with these columns:

| name | path | responsibility | interface (key fns) | tests |
|---|---|---|---|---|
| example-module | `path/to/example.py` | one-line of what it owns | `do_thing(*, arg: int) -> Result` | V1 |

If no new modules are needed, write `_none_` as the only content of this section.

## Modules to UPDATE

Existing modules touched by this PRD. Markdown table with these columns:

| name | path | what changes | tests |
|---|---|---|---|
| existing-module | `path/to/existing.py` | brief description of the change | V2 |

If nothing is being updated, write `_none_`.

## Test plan

Prose: what makes a good test for this work, the prior art in the codebase (similar tests to model on), and which Validations cover which modules. Tests must verify externally observable behavior, not implementation details.

## Q&A

Collapsible block with the grilling Q&A that produced this PRD. Use HTML `<details>` so it does not clutter the issue body by default:

<details>
<summary>Grilling Q&A</summary>

Q: question text
A: answer text

Q: ...
A: ...

</details>

</prd-template>

## Validator contract

`skills/_lib/prd_validator.py` enforces, and rejects PRDs that fail any of these:

- All 7 H2 sections present (case-insensitive match on names above)
- G numbering sequential from 1, no gaps, no duplicates
- V numbering sequential from 1, no gaps, no duplicates
- Every Vn block cites at least one `Gn`, and every cited `Gn` is defined in Deliverables
- No unresolved template placeholders (`<...>` prose, `TODO`, `TBD`, `FIXME`, `XXX`)
- Module tables well-formed: header + separator + ≥1 data row, consistent column counts. `_none_` is accepted in place of a table.

The validator ignores content inside fenced code blocks (```) and inline code spans (` `` `), so example snippets and command lines can contain placeholder-looking text without tripping checks.

Exit codes: `0` = valid, `1` = validation errors (printed to stderr), `2` = bad invocation (missing/wrong arguments).
