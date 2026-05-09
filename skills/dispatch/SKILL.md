---
name: dispatch
description: "Dispatch a task to a claude coder running in a fresh git worktree inside a tmux pane (window named after the task-id, in the shared 'claude-admin' session). Re-checks blockers, creates worktree + branch, spawns 'claude -p ... | tee log.jsonl' inside the pane (so operator can tmux attach and watch), starts a watcher that monitors progress and detects done/stuck/permission-blocked states. Use when the user asks to dispatch a task / start coding / kick off a coder for a task / invokes /dispatch. Examples: '/dispatch v2_design M0a-T1', 'dispatch M0a-T1', 'start the coder for M0a-T1'."
argument-hint: "<plan-codename> <task-id> [--force]"
---

# Dispatch skill

Spin up a claude coder for one task in its own tmux pane. The pane runs `claude -p ... | tee log.jsonl` so the JSON stream still hits disk for the watcher to parse, AND the operator can `tmux attach -t claude-admin` to watch it scroll live. No API costs (uses subscription via `claude -p`).

## Inputs

`/dispatch <plan-codename> <task-id> [--force]`

Examples: `/dispatch v2_design M0a-T1` · `/dispatch v2_design M0a-T2 --force`

If args missing, ask via AskUserQuestion. To pick a task, run `/suggest <plan-codename>` first and copy the dispatchable task ID.

## Steps

### 1. Run the dispatcher

```bash
python3 /Users/alf/.claude/skills/dispatch/scripts/dispatch.py <plan-codename> <task-id> [--force]
```

The script:

- Resolves the plan via `~/.claude/plans/registry.json`
- Reads the task's full block from `<plan_dir>/breakdowns/<milestone-id>.md` (milestone derived from task-id prefix)
- Re-checks blockers (refuses to dispatch if any unmet, unless `--force`)
- `git fetch origin <default_base>`
- Creates worktree at `~/dev/claude-admin-worktrees/<task-id>/` with branch `<task-id>` from `origin/<default_base>`
- Initializes state at `~/.work/dispatches/<plan>/<task-id>/state.json` (phase=spawning)
- Ensures the `claude-admin` tmux session exists (creates it detached if missing)
- Opens a tmux window named `<task-id>` in the `claude-admin` session, working dir = the worktree
- Spawns coder inside that window: `claude -p --output-format stream-json ... 2>>coder.stderr | tee log.jsonl`. Pane scrolls for the operator; JSON lands on disk for the watcher. Pane auto-closes when claude exits.
- Spawns watcher: `watcher.py wait <plan> <task-id>` background subprocess; updates `state.json`, polls GH for draft PR, detects stuck/permission-blocked/errored
- Updates `milestones.json` with dispatch metadata
- Prints a one-screen summary

### 2. Show the script's output to the user

Pass through verbatim. It includes:

- Dispatch confirmation with tmux window target (e.g. `claude-admin:M0a-T1`), worktree, branch, watcher PID
- Where to find the log + state file
- One-line `/status` invocation
- Hint: `tmux attach -t claude-admin \; select-window -t <task-id>` to watch the coder live

### 3. (Optional) Offer to immediately tail status

If the user wants to watch live, suggest:

```bash
tail -f ~/.work/dispatches/<plan>/<task-id>/log.jsonl
# or for a structured view (later skill):
python3 /Users/alf/.claude/skills/dispatch/scripts/watcher.py status <plan> <task-id>
```

## Re-dispatch (`--force`)

Refuses to dispatch if branch, worktree, or state dir already exist. With `--force`:

- Kills the tmux window (`tmux kill-window -t claude-admin:<task-id>`) — closes the pane and SIGKILLs the entire process tree under it
- Sends SIGTERM to the watcher PID (from state.json), waits 5s, SIGKILL if still alive
- `git worktree remove --force <worktree>`
- `git branch -D <task-id>`
- Removes `~/.work/dispatches/<plan>/<task-id>/`
- Then proceeds with a fresh dispatch

## Blocked-on-permission visibility

The coder runs with a curated `--allowedTools` whitelist (Read/Edit/Write + Bash(cargo *) Bash(git *) Bash(gh *) etc.) and `--permission-mode acceptEdits`. If claude attempts a tool outside the whitelist, the stream-json log records a `permission_denied` event. The watcher pattern-matches that and sets `state.phase = "permission_blocked"` with the blocked invocation in `state.stuck_reason`. You see it in `/status` and can intervene by:

- Manually doing the blocked action and dropping a `done` sentinel
- Re-dispatching with `--force` after broadening the whitelist (edit dispatch.py's `ALLOWED_TOOLS` constant)
- Aborting the dispatch entirely

## Notes

- The coder skill (`~/.claude/skills/coder/SKILL.md`) is appended to the system prompt if it exists; falls back to inlined coder rules if absent.
- The coder is told **not** to ask questions: document open items in the PR body and proceed. Mid-run question routing is a v2 enhancement (uses `--input-format stream-json --replay-user-messages`).
- This skill never modifies main-branch state. Worktrees + branches are isolated.
