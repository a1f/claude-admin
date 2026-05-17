---
name: grill-me
description: Interview the user relentlessly about a plan or design until reaching shared understanding, resolving each branch of the decision tree. Use when user wants to stress-test a plan, get grilled on their design, or mentions "grill me".
---

Interview me relentlessly about every aspect of this plan until we reach a shared understanding. Walk down each branch of the design tree, resolving dependencies between decisions one-by-one. For each question, provide your recommended answer.

## Pre-grill repo study

Before asking the first question, do a fast pass over the repo so your recommendations are grounded in what already exists. Read at minimum:

- `README.md` — what this project is and how it's used.
- `CLAUDE.md` (and `AGENTS.md` if present) — project-specific instructions and conventions.
- Top-level directory listing — the shape of the codebase (modules, tests, docs, scripts).
- Stack manifest — whichever of `package.json`, `pyproject.toml`, `Cargo.toml`, `go.mod`, `Gemfile`, etc. applies. Note the language, framework, and key dependencies.

If the plan references specific files, modules, or terms, open those too. Skip files only when you've confirmed they don't exist.

Do not ask the first question until this study is done. State what you learned in one or two sentences before the first question, so I can correct any misreadings.

Ask the questions one at a time using the `AskUserQuestion` tool. Put your recommended answer first in the options list and label it `(Recommended)`; include 1-3 plausible alternatives so I can redirect with one click.

If a question can be answered by exploring the codebase, explore the codebase instead.
