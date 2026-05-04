---
name: test-builder
description: "Post-merge analyzer. Scans a just-merged PR's code for under-tested paths and proposes specific additional test scenarios to add (with kind, name, target file, rationale). Saves a structured recommendation. Use when the user asks to find test coverage gaps, propose more tests for a task, or invokes /test-builder. Examples: '/test-builder v2_design M0a-T1', 'what tests should we add for M0a-T1?'."
argument-hint: "<plan-codename> <task-id>"
---

# Test builder

You analyze a just-merged PR and propose specific test scenarios the existing tests don't cover. You are NOT writing the tests — you're producing a list of scenarios that would justifiably ship in a follow-up PR.

## Inputs

`/test-builder <plan-codename> <task-id>`

If args missing, ask via AskUserQuestion.

## Steps

### 1. Resolve & read state

```bash
cat ~/.work/dispatches/<plan>/<task-id>/state.json
```

Pull `pr_url`, `pr_number`, `gh_repo`.

### 2. Read the diff + tests in the diff

```bash
gh pr diff <pr_number> --repo <gh_repo>
```

Identify which files are tests (test_*.py, *.test.ts, *_test.rs, etc.) and which are source under test.

### 3. Read the task spec's test scenarios

The task spec lists `_unit | integration | e2e_ — name — description`. Walk it:

- Did the coder implement each listed scenario?
- Marked ◯ skipped in the PR body? Note it.
- Are the implemented tests **actually testing** what their names claim, or are they shallow?

### 4. Investigate the source code

Use `Read`/`Grep` to look at the source files the PR changed. Spot:

- **Edge cases** the tests don't exercise: empty input, null/None, max/min, zero, negative, unicode, very large
- **Error paths**: `Err(...)` returns, panics, exception throws — are these covered?
- **Branches**: if/else, match arms, loops with edge inputs
- **Public API surface**: every public function/handler should have at least one direct test
- **Integration boundaries**: where does this code talk to DB, network, FS? Are those mocked or hit?
- **Concurrency**: if the code uses async/threads, is there a test for the concurrent path?
- **Regression potential**: subtle behavior worth pinning so it doesn't drift

### 5. Output JSON

Write the result to `~/.work/dispatches/<plan>/<task-id>/post-merge/test-suggestions.json`.

Schema:

```json
{
  "task_id": "<task-id>",
  "plan": "<plan>",
  "analyzed_at": "<ISO timestamp>",
  "summary": "<1-line read on coverage>",
  "skipped_from_task_spec": [
    {
      "name": "<scenario name from task spec>",
      "reason_recorded": "<what the coder said when marking ◯>",
      "kind": "unit|integration|e2e"
    }
  ],
  "coverage_gaps": [
    {
      "kind": "unit|integration|e2e",
      "name": "<test_function_name_to_add>",
      "target_file": "<source file the test would cover>",
      "target_function": "<function or endpoint, optional>",
      "scenario": "<plain-language description of what the test exercises>",
      "rationale": "<why this matters>",
      "priority": "must|should|nice"
    }
  ]
}
```

### 6. Print summary

Show the user: total skipped scenarios, total new gaps by priority, top 5 must-haves, path to JSON. Don't dump full JSON.

## Priority rubric

- **must** — code path will silently fail in production if not tested. Pin it now.
- **should** — important enough to add before the next milestone but not urgent.
- **nice** — defensive coverage; add if quick.

## Focus

- Edge cases the listed scenarios didn't anticipate
- Error paths (these often go untested first)
- Public API surface coverage
- Integration / e2e gaps when only unit tests exist
- The skipped scenarios from the task spec — those are immediate must-haves unless the reason was justified

## What NOT to do

- Don't propose tests that already exist (read the test files first)
- Don't write the tests — that's a coder's job (dispatched as a follow-up task)
- Don't demand 100% line coverage; focus on meaningful paths
- Don't pad — empty `coverage_gaps: []` is fine when coverage is solid

## Following up

If gaps are significant (≥3 must-haves), tell the user this could justify a follow-up task — they can then `/breakdown` it or just reference these scenarios when a related task is dispatched next.
