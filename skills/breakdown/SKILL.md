---
name: breakdown
description: "Break down a milestone of a plan into PR-shaped tasks (~150 LOC each), draft a parent GitHub issue, save locally. Use when the user asks to break down a milestone, create a breakdown for a milestone, or generate the task list for a milestone. Examples: '/breakdown v2_design M0a', 'break down M3a', 'create the breakdown for milestone M1'."
argument-hint: "<plan-codename> <milestone-id>"
---

# Breakdown skill

Generate PR-shaped tasks for a milestone of a plan, draft a parent GitHub issue, and save the breakdown locally.

## Inputs

`/breakdown <plan-codename> <milestone-id>`

Examples: `/breakdown v2_design M0a` · `/breakdown v2_design M3a`

If either arg is missing, use **AskUserQuestion** to pick from:
- available codenames (read keys of `~/.claude/plans/registry.json` → `plans`)
- available milestone IDs in that plan (read `milestones[*].id` from the milestones source)

## Definitions

**Task** — one PR-shaped unit of work, ~150 LOC, with one concrete goal. Tests live inside the task that produces the code under test (bundled in the validation section).

**Breakdown** — the full set of tasks for one milestone. Mirrors to one parent GitHub issue with a checklist + per-task spec.

## Steps

### 1. Resolve the plan

Read `~/.claude/plans/registry.json`. Look up `plans[<codename>]`. Fields you need:

- `milestones_source` — path to a `milestones.json` file
- `gh_repo` — `owner/name` for `gh issue create --repo`
- `default_base` — base branch (usually `main`)
- `plan_dir` — directory where local breakdowns will be saved (under `<plan_dir>/breakdowns/`)
- `plan_doc` — link back to the canonical plan doc (referenced in the issue body)
- `stack` — used to make tasks concrete (e.g., "axum endpoint" not "HTTP endpoint")

If codename unknown, list available ones from the registry and ask the user to pick or add. Do not invent paths.

### 2. Load the milestone

Open `milestones_source` (JSON). Find `milestones[*]` where `id == <milestone-id>`. Capture:

- `id`, `title`, `phase`, `kind`, `goal`
- `pr_count_target`, `raw_prs[]` (use these as a starting point — but expand each into a full task)
- `exit_criteria`, `risk`, `depends_on[]`

If the milestone is missing, list IDs in the file and ask user to pick.

### 3. Draft tasks

**Each `raw_prs[]` line is the seed for one task.** You may merge two seeds into one task or split one seed into two if granularity feels off. Default 1:1.

For each task, produce:

| Field | Required | Notes |
|---|---|---|
| `id` | yes | Format: `<milestone-id>-T<n>` (e.g., `M0a-T1`). Keep stable across revisions. |
| `title` | yes | Imperative, action-first, ≤60 chars. |
| `deliverable` | yes | What code/artifacts ship. Concrete. Mention specific files/modules where you can. |
| `expectation` | yes | Working state after this lands. Phrase as "after this PR: <observable outcome>". |
| `scope` | yes | Files/modules touched, what changes. Bullets. Include "out of scope" line if helpful. |
| `motivation` | yes | Why we need this. What it unblocks. Tie back to the milestone goal. |
| `validation` | yes | How to verify it's done. Concrete assertions a reviewer/critic can check. |
| `test_scenarios[]` | yes | At least 2. Each has `kind` ∈ {unit, integration, e2e} + `name` + `description`. UI tasks must include ≥1 e2e or playwright scenario. |
| `blockers` | yes | One machine-parseable line. Format: `Blockers: <task-id> <state>; <task-id> <state>; label:<label-name>` where state ∈ {merged, drafted, ready}. Empty = `Blockers: none`. Example: `Blockers: M0a-T1 merged; M0a-T2 drafted`. The `/suggest` skill parses this to determine dispatchability. |
| `estimated_loc` | yes | Integer estimate. Soft cap 200; flag if >200. |

Keep tasks **abstract enough** that they don't constrain the coder's implementation choices, but **concrete enough** that "done" is unambiguous. Reviewers must be able to read the task and the diff and decide if the diff matches.

Concretize against the plan's stack. For `v2_design`: backend = `axum + sqlx`, frontend = `React + Vite + Tailwind + TanStack Query`, agent runtime = `claude` CLI subprocess. Don't say "HTTP endpoint" when you can say "axum handler".

### 4. Show the draft to the user

Render the full breakdown in chat using the structure of `templates/issue.md`. Then **AskUserQuestion** with these options:

- **Approve & create GH issue** (Recommended)
- **Edit a specific task** (which one + what to change)
- **Add a task** (specify what)
- **Remove a task** (specify which)
- **Rewrite from scratch** (with feedback)

If they pick edit/add/remove/rewrite, apply the change and re-show. Loop until they Approve.

### 5. Create the GitHub issue

Build the body using `templates/issue.md` filled in. Then:

```bash
gh issue create \
  --repo <gh_repo> \
  --title "<milestone-id> · <milestone-title>" \
  --label breakdown \
  --label "milestone:<milestone-id>" \
  --body-file <tmp-file>
```

Capture the issue URL from stdout.

If labels don't exist, that's fine — `gh` returns a warning, not an error. Note it in the report.

### 6. Save locally

Write a markdown copy to `<plan_dir>/breakdowns/<milestone-id>.md`. The body is the same as the GH issue body, prefixed with:

```
> **GitHub issue:** <issue_url>
> **Generated:** <iso-date>
> **Plan:** [<plan_codename>](<plan_doc>)
```

### 7. Update milestones.json

In the milestones source file, set on this milestone:

- `status: "broken_down"`
- `breakdown.issue_url: <url>`
- `breakdown.local_file: <path>`
- `breakdown.task_count: <n>`
- `breakdown.created_at: <iso-date>`

Save the file.

### 8. Report to the user

Output a tight summary:

```
✓ Broke down <milestone-id> · <title>
  N tasks · <total_loc> LOC budget
  GH issue: <url>
  Local: <path>
  Next step: dispatch when you're ready (skill TBD)
```

## Editing existing breakdowns

If `milestones[<id>].status == "broken_down"`, ask the user before regenerating:

- **Open existing breakdown** — show the local file + issue URL
- **Replace it** — regenerate from scratch, archive the old local file with `.bak.<timestamp>` suffix, optionally close the old GH issue
- **Append a task** — quick add to existing breakdown (edit local file + comment on GH issue)

Default: open existing.

## Things to avoid

- Tasks that say "make it work" or "implement the feature" — too vague to dispatch.
- Tasks that bundle two unrelated changes — split.
- Tasks with no test scenarios — every task must have ≥2.
- Tasks above 250 LOC — split or call out the size explicitly.
- Tasks with blockers that reference task IDs not in the same breakdown (or the milestone's `depends_on`).
- Inventing details not present in the milestone goal/raw_prs without flagging them as additions for user approval.

## Implementation notes for the agent

- AskUserQuestion has a hard cap of 4 options per question. Keep "what next?" prompts ≤4.
- Verify the registry's `gh_repo` matches `git remote -v` of the actual repo before running `gh issue create`. If mismatch, fix the registry first.

## When the user has feedback mid-flow

Treat their feedback as a course correction. Update the affected tasks in place; don't re-draft everything unless they ask. Keep task IDs stable when possible — only renumber when adding/removing tasks.
