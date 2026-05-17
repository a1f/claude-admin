---
name: pr-decide
description: "User-facing post-review decision skill for a dispatched task. Shows the aggregated review summary (reviewers + critics + recommendation) and lets the user act: ready (promote draft -> ready), merge (after CI green), drop (close PR), or iterate (re-dispatch with feedback). Use after /pr-babysit has finished its CI/review-routing loop and is waiting on a human verdict. Examples: '/pr-decide v2_design M0a-T1', '/pr-decide v2_design M0a-T1 ready', 'merge M0a-T1', 'drop M0a-T1'."
argument-hint: "<plan-codename> <task-id> [show|ready|merge|drop|iterate]"
---

# pr-decide skill

Pick up a reviewed PR, show the user the aggregated review summary, and execute their decision.

This is the optional terminal step a human runs after `/pr-babysit` has finished
its automated CI + comment-routing loop. `/pr-babysit` reaches one of:

- `[READY TO MERGE]` — green CI, no blocking comments → run `/pr-decide ... merge`.
- `[ESCALATED]` — stuck, posted to slice issue for architector → human inspects, may `/pr-decide ... drop` or re-dispatch via `/coder`.
- `[MAX ROUNDS EXHAUSTED]` — same as escalation.

## Inputs

`/pr-decide <plan-codename> <task-id> [<action>]`

Actions:

- `show` — print the review summary; no side effects (default if `<action>` omitted, then this skill becomes interactive)
- `ready` — promote draft -> ready (gh pr ready); state.phase = `accepted_pending_ci`
- `merge` — `gh pr merge --squash --delete-branch`; state.phase = `merged`. gh refuses if CI not green.
- `drop` — `gh pr close`; state.phase = `dropped`. Optionally adds a closing comment.
- `iterate` — _v1 placeholder_. Posts the feedback bundle as a PR comment + tells the user to manually re-dispatch with `--force`. Full automated iterate lands later.

If args missing, ask via AskUserQuestion. Resolve from registry.

## Steps when invoked without an action (interactive)

### 1. Show summary

```bash
python3 /Users/alf/.claude/skills/pr-decide/scripts/pr_decide.py show <plan> <task-id>
```

Pass through the script's stdout to the user verbatim.

### 2. Decide what's possible based on the current phase

The script's stdout includes `phase: <name>`. Based on phase:

- `coding` / `reviewing` — work in progress. Tell the user to come back later. Exit.
- `reviewed` — decisions available: `ready` / `iterate` / `drop` / `wait`
- `accepted_pending_ci` — only useful action: `merge` / `drop` / `wait`
- `merged` / `dropped` — terminal. Just print the state and exit.
- `errored` / `stuck` / `permission_blocked` / `aborted` — failure. Tell user; suggest `/dispatch <plan> <task-id> --force` to retry, or `drop` to close out.

### 3. AskUserQuestion to pick an action

Use AskUserQuestion (max 4 options). Tailor the option set to the phase as above. Always include a "wait" / "no action" option so the user can bail.

### 4. Execute

```bash
python3 /Users/alf/.claude/skills/pr-decide/scripts/pr_decide.py <action> <plan> <task-id>
```

Pass through the script's output. The script handles:

- `ready` — `gh pr ready <num>`, updates state.phase to `accepted_pending_ci`
- `merge` — `gh pr merge <num> --squash --delete-branch`, updates state.phase to `merged`. If gh refuses (CI not green / merge conflicts / branch protection), surface the error verbatim.
- `drop` — `gh pr close <num>` with an optional comment, updates state.phase to `dropped`. Asks the user for a one-line drop reason if not provided.
- `iterate` — composes feedback bundle from `~/.work/dispatches/<plan>/<task>/reviews/summary.json`, posts to PR as a comment, prints the manual re-dispatch command. Does not currently re-dispatch automatically.

### 5. After ready, what's next?

When the user picks `ready`, this skill exits (state phase = `accepted_pending_ci`). The user comes back later — once GitHub CI is green — and runs `/pr-decide <plan> <task-id> merge` to actually merge. Or `/pr-babysit` (the polling loop) will reach `[READY TO MERGE]` on its own when CI lands.

Tell the user explicitly: "PR is now ready. Wait for CI to go green on GitHub, then run `/pr-decide v2_design M0a-T1 merge` to merge."

## When invoked with an action

Skip the AskUserQuestion. Just run the action via the Python script and pass through output.

## Drop reasons

If the user picks `drop`, ask them for a one-line reason (AskUserQuestion or just plain text) — pass it as `--reason "..."` to the Python script. The reason gets posted as a closing comment + recorded in state.json.

## Notes

- This skill never modifies the worktree or branch directly. All state changes go through `gh` and `~/.work/dispatches/<plan>/<task-id>/state.json`.
- The state.json field `user_decision` is set by this skill. Future skills (CI watcher, auto-merge) read it.
- If state.json doesn't exist for `<plan>/<task-id>`, surface a clear error — the PR wasn't created via `/dispatch`, so we have no review summary.
- For the **automated** CI + comment-routing loop (poll `gh pr checks`, fix bot comments, route human feedback to architector, invoke /diagnose on CI red), see `/pr-babysit`.
