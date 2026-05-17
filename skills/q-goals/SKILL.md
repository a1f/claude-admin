---
name: q-goals
description: "Convert a plan into a GH issue with checkboxed goals + validations. Reads plan from conversation context, prompt text, or a GH issue URL. Confirms each step via AskUserQuestion. Posts the result as a new (or updates an existing) issue in caveman style. Ratify by adding the `goals-ratified` label. Use when the user invokes /q-goals or asks to define goals for a milestone/feature/plan."
argument-hint: "[plan-source: gh-issue-url | inline text | (omit to use conversation context)]"
---

# q-goals

Turn a plan into an immutable **goals + validations** contract on GitHub.

The agent invoking this skill IS the writer. No subagent, no Python script, no local files. Source of truth = the GitHub issue.

## Inputs

`/q-goals [plan-source]`

Plan source resolves in priority order:

1. **GH issue URL** — `/q-goals https://github.com/org/repo/issues/123` → read via `gh issue view 123 --repo org/repo`.
2. **Inline text** — `/q-goals build a daemon that ...` → treat the prompt tail as the plan.
3. **Conversation context** — if no arg, use the plan visible in the current chat.

If none of those resolve to a real plan, ask via AskUserQuestion where the plan lives. Do NOT guess.

## Workflow

1. **Read the plan.** From URL / prompt text / context per above.
2. **Decide target repo.** AskUserQuestion: which repo + does the goals issue go in this same repo or another? Default = repo the plan-source issue lives in (or current working directory's repo).
3. **Check for existing ratified issue.** Run `gh issue list --search "goals: <plan-title>" --label goals-ratified`. If a ratified goals issue for this plan already exists, **STOP** and print "frozen at <url>. unfreeze: `gh issue edit <num> --remove-label goals-ratified`".
4. **Study the target repo.** The agent MUST read the repo before drafting — goals in a vacuum produce vague checkboxes. Minimum study set:
   - `CLAUDE.md` (root + any sub-dir `CLAUDE.md` files) — project conventions
   - `README.md` (root) — what the project is
   - Stack config file: `Cargo.toml` / `pyproject.toml` / `package.json` / `go.mod` — language + dependencies + test commands
   - Top-level dir listing — what crates / packages / modules exist
   - Any plan-source-relevant code path (if plan mentions a module, read its current state)

   For larger / unfamiliar repos, dispatch the `Explore` subagent: `"Survey the repo at <path>: stack, top-level structure, test commands, what already exists relevant to <plan summary>. Report in under 200 words."`

   Goals must reference **real** file paths, **real** test commands (e.g. `cargo test --workspace`, `pytest`, `pnpm test`), and **real** conventions that exist in the repo. If the plan asks for something that contradicts the repo (e.g. proposes Python in a Rust repo), surface that conflict to the user via AskUserQuestion before drafting.

5. **Draft goals + validations.** Use the template below. Write the issue body in caveman style (terse — drop articles/filler; keep facts, paths, commands, checkboxes). Each `observable:` and each `how:` should cite a real path/command from your study.
6. **Confirm goals via AskUserQuestion.** Ask: "goals cover plan?" — options: looks right / add more / remove some / restart. Loop until user picks "looks right".
7. **Confirm validations via AskUserQuestion.** Same loop for validations.
8. **Confirm post target.** AskUserQuestion: new issue in `<repo>` / update existing issue (provide #) / cancel.
9. **Post.**
   - New: `gh issue create --repo <org/repo> --title "goals · <plan-name>" --body "<caveman body>"`.
   - Update: `gh issue edit <num> --repo <org/repo> --body "<caveman body>"` (only if issue is NOT already labeled `goals-ratified`).
10. **Tell user how to ratify** — print the literal command:
    ```
    gh issue edit <num> --repo <org/repo> --add-label goals-ratified
    ```
    **Do NOT add the label automatically.** Ratify is an explicit human act.

## Hard rules

- **No Python script. No persistent local state.** The agent does the work; the GitHub issue holds the result.
- **Study the repo before drafting.** Goals must cite real paths + real commands. The `Explore` subagent is allowed for broader scans; it's not an LLM "agent" in the q-goals sense, just a read-only repo survey.
- **Caveman style for the issue body only.** This SKILL.md stays readable English.
- **Every goal + every validation is a checkbox** (`- [ ]`).
- **Refuse to edit an issue with `goals-ratified` label.** Print the unfreeze command instead.
- **Never auto-apply the `goals-ratified` label.** User does it.
- **Confirm every step via AskUserQuestion.** No free-form questions. Each ambiguity = an AskUserQuestion.

## Caveman style — what it means for the issue body

Drop: articles (the / a / an), pleasantries, hedge words ("should", "may", "could probably"), filler ("note that", "it is worth mentioning").

Keep: nouns, verbs, file paths, command names, references (G1, V3), checkboxes.

| normal | caveman |
|---|---|
| "The daemon should start when the user runs `ca daemon`." | "daemon starts on `ca daemon`." |
| "We expect that the test will verify the end-to-end flow." | "test verifies e2e flow." |
| "It is important that the data is persisted to disk." | "data persists on disk." |

Body must stay readable. Don't drop nouns or verbs. Don't abbreviate words. Just cut filler.

## Issue body template (post this, in caveman style)

```markdown
# goals · <plan-or-milestone-name>

> immutable. ratify: add `goals-ratified` label. unfreeze: remove label.
> source plan: <issue url | "inline text in prompt" | "conversation context on YYYY-MM-DD">

## deliverables

- [ ] **G1** · <short name>
  - observable: <concrete signal — file exists / test passes / command returns X>
  - why: <one line>

- [ ] **G2** · <short name>
  - observable: <signal>
  - why: <reason>

## validations

- [ ] **V1** · _unit_ — `test_name_snake_case` — covers G1
  - what: <one line>
  - how: <path or command>

- [ ] **V2** · _e2e_ — `scenario_name_snake_case` — covers G1, G2
  - what: <one line>
  - how: <path or command>

- [ ] **V3** · _manual_ — `manual_check_name` — covers G2
  - what: <one line>
  - how: <visual check or command>
```

Validation kinds: `unit` / `integration` / `e2e` / `manual`. Pick the right one per scenario.

## Structural rules for the issue body

- `## deliverables` section present.
- `## validations` section present.
- Every G-item has `observable:` and `why:` sub-bullets.
- Every V-item has `what:` and `how:` sub-bullets.
- Every V-item's `covers G<N>` refers to a G defined above.
- G-numbers + V-numbers sequential from 1, no gaps.
- No template placeholders (`<short name>`, `<signal>`, etc.) left in the posted body.

If your draft violates any of these, fix BEFORE the confirm step — don't make the user catch structural slop.

## Companion: `/q-breakdown`

After ratify, `/q-breakdown <goals-issue-url>` reads the ratified goals + validations issue and produces a separate **breakdown issue** with PR-sized task list. Goals issue stays source of truth for "what we promised"; breakdown issue is the mutable task list.

## What this skill does NOT do

- No background subprocess / no agent fan-out. The current agent does the drafting + posting. (The `Explore` subagent is allowed in the study step — read-only repo survey, returns a summary, ends.)
- No local markdown files. No `.ratified.json` sentinel. No `chmod 444`.
- No Python. No tests. No `uv` / pytest dependency.
- No LLM-driven critique loop. AskUserQuestion is the only feedback mechanism in MVP.
- No automatic ratify. User adds the label.
- No q-breakdown work. Separate skill.
