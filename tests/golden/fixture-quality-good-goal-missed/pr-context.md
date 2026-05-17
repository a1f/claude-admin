# PR #9001: Add slugify utility

- URL: https://github.com/example/repo/pull/9001
- Author: @somebody
- Branch: `M0a-T1`
- Dispatch task id: `M0a-T1` (this PR was opened by /dispatch — the task spec governs what was promised)

## PR body

Adds slugify per task spec.

## Linked issues

### Issue #9000

**M0a-T1 — slugify utility** — https://github.com/example/repo/issues/9000

## What this task does

Add a `slugify(text)` function to `src/utils.py` that:

- Lowercases the input.
- Replaces every run of non-alphanumeric characters with a single `-`.
- Strips leading and trailing `-`.
- Returns the resulting string.

## Validation

- `slugify("Hello, World!") == "hello-world"`
- `slugify("") == ""`
- `slugify("  multi   spaces  ") == "multi-spaces"`
- `slugify("---weird___chars???") == "weird-chars"`

## Tests to add

- `_unit_` — `test_slugify_empty` — empty string → empty
- `_unit_` — `test_slugify_single_word` — `"hello"` → `"hello"`
- `_unit_` — `test_slugify_multi_word` — `"Hello World"` → `"hello-world"`
- `_unit_` — `test_slugify_special_chars` — punctuation collapses to single `-`, stripped at edges

## Out of scope

- Unicode normalization (NFC/NFKD) — leave for a future task.
- Performance optimization — correctness first.
