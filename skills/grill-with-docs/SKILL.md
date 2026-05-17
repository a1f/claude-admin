---
name: grill-with-docs
description: Grilling session that challenges your plan against the existing domain model, sharpens terminology, and updates documentation (CONTEXT.md, ADRs) inline as decisions crystallise. Use when user wants to stress-test a plan against their project's language and documented decisions.
---

<what-to-do>

Interview me relentlessly about every aspect of this plan until we reach a shared understanding. Walk down each branch of the design tree, resolving dependencies between decisions one-by-one. For each question, provide your recommended answer.

Ask the questions one at a time, waiting for feedback on each question before continuing.

If a question can be answered by exploring the codebase, explore the codebase instead.

Before Q1, run the **start-of-session freshness check** (see below). At the end of the session, run the **end-of-session doc audit**. The standard inline-update behavior in the middle is unchanged — capture decisions in `CONTEXT.md` / `docs/adr/` as they happen.

</what-to-do>

<supporting-info>

## Domain awareness

During codebase exploration, also look for existing documentation:

### File structure

Most repos have a single context:

```
/
├── CONTEXT.md
├── docs/
│   └── adr/
│       ├── 0001-event-sourced-orders.md
│       └── 0002-postgres-for-write-model.md
└── src/
```

If a `CONTEXT-MAP.md` exists at the root, the repo has multiple contexts. The map points to where each one lives:

```
/
├── CONTEXT-MAP.md
├── docs/
│   └── adr/                          ← system-wide decisions
├── src/
│   ├── ordering/
│   │   ├── CONTEXT.md
│   │   └── docs/adr/                 ← context-specific decisions
│   └── billing/
│       ├── CONTEXT.md
│       └── docs/adr/
```

Create files lazily — only when you have something to write. If no `CONTEXT.md` exists, create one when the first term is resolved. If no `docs/adr/` exists, create it when the first ADR is needed.

## Start of session: freshness check

Before asking Q1, run the freshness scanner against the repo root:

```bash
python3 /Users/alf/.claude/skills/grill-with-docs/scripts/grill_docs.py freshness <repo-root>
```

The script prints a JSON report. Parse it and act:

- **No `CONTEXT.md` yet** (`contexts: []`): skip — the inline-update flow will create one when needed.
- **`stale: false`**: mention briefly ("CONTEXT.md last touched N days ago, no orphaned terms") and proceed.
- **`stale: true`**: collect the staleness signals (any of: `age_days > 60`, `orphaned_terms`, `missing_files`) and surface them via **`AskUserQuestion`** *before* starting the grill. Phrase the options as concrete next steps, e.g.:
  - "Update CONTEXT.md as we go — flag the stale entries when we hit them"
  - "Pause the grill, clean up the doc first"
  - "Acknowledged — proceed without changes"

Do not auto-edit `CONTEXT.md` or delete entries based on the report. The scanner is advisory; the human decides what to retire.

Also: **take a doc snapshot now** so the end-of-session audit has something to diff against. Store the JSON in working memory or to a tempfile:

```bash
python3 /Users/alf/.claude/skills/grill-with-docs/scripts/grill_docs.py snapshot <repo-root> > /tmp/grill-snapshot-$$.json
```

While grilling, **track every decision** that should land on disk — every term you canonicalise, every ADR you offer and the user accepts. Keep a running list in working memory (or a sibling tempfile) shaped like `decided.json` in the audit script's docstring.

## During the session

### Challenge against the glossary

When the user uses a term that conflicts with the existing language in `CONTEXT.md`, call it out immediately. "Your glossary defines 'cancellation' as X, but you seem to mean Y — which is it?"

### Sharpen fuzzy language

When the user uses vague or overloaded terms, propose a precise canonical term. "You're saying 'account' — do you mean the Customer or the User? Those are different things."

### Discuss concrete scenarios

When domain relationships are being discussed, stress-test them with specific scenarios. Invent scenarios that probe edge cases and force the user to be precise about the boundaries between concepts.

### Cross-reference with code

When the user states how something works, check whether the code agrees. If you find a contradiction, surface it: "Your code cancels entire Orders, but you just said partial cancellation is possible — which is right?"

### Update CONTEXT.md inline

When a term is resolved, update `CONTEXT.md` right there. Don't batch these up — capture them as they happen. Use the format in [CONTEXT-FORMAT.md](./CONTEXT-FORMAT.md).

`CONTEXT.md` should be totally devoid of implementation details. Do not treat `CONTEXT.md` as a spec, a scratch pad, or a repository for implementation decisions. It is a glossary and nothing else.

### Offer ADRs sparingly

Only offer to create an ADR when all three are true:

1. **Hard to reverse** — the cost of changing your mind later is meaningful
2. **Surprising without context** — a future reader will wonder "why did they do it this way?"
3. **The result of a real trade-off** — there were genuine alternatives and you picked one for specific reasons

If any of the three is missing, skip the ADR. Use the format in [ADR-FORMAT.md](./ADR-FORMAT.md).

## End of session: doc audit

When the grill is wrapping up (user signals done, or every branch resolved), verify the inline updates actually landed on disk.

Write the running decisions list to a tempfile in the shape:

```json
{
  "terms": [
    {"name": "Coder", "context": "CONTEXT.md"},
    {"name": "Invoice", "context": "src/billing/CONTEXT.md"}
  ],
  "adrs": [
    {"slug": "tmux-runtime", "number": 3}
  ]
}
```

`context` defaults to `CONTEXT.md` (single-context repo). `number` is optional — omit it if you don't care which number got assigned.

Then run:

```bash
python3 /Users/alf/.claude/skills/grill-with-docs/scripts/grill_docs.py audit <repo-root> \
  --snapshot /tmp/grill-snapshot-$$.json \
  --decided /tmp/grill-decided-$$.json
```

The script exits 0 if clean, 1 if mismatches were found, and prints a JSON report. Possible mismatch reasons:

- `term-not-written` — you decided on the term but it's not bolded under `## Language` in the named CONTEXT
- `context-missing` — you decided on a term for a CONTEXT file that doesn't exist on disk
- `no-matching-file` — you offered an ADR but no `docs/adr/NNNN-<slug>.md` was created (or the number doesn't match)
- `already-existed` — the ADR file was already there before the session started (you didn't actually add anything)

For each mismatch, write the missing entry inline now — this is the same flow as "Update CONTEXT.md inline", just compressed at the end. Re-run the audit until clean. Only report "session complete" to the user once the audit passes (or once the user has explicitly waived a mismatch).

</supporting-info>
