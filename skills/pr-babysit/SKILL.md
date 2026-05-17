---
name: pr-babysit
description: "AFK polling skill that babysits an open PR through CI + bot review feedback. Adapted from a1f/agent-templates/skills/pr-babysit with S10-specific dispatch: tier-1 fixes inline (fmt/lint/typo/<10 LOC mechanical), tier-2 prints /coder tmux dispatch (broken tests, semantic bugs, multi-file refactors), tier-3 escalates to architector (5 failed rounds, wrong-direction critic verdict, any human comment, any CRITICAL). On CI red, invokes /diagnose as an analysis-only subagent before triaging the fix. Routes /critic summaries to the parent slice issue only on bad verdicts. Use when the user invokes /pr-babysit from a worktree on a feature branch with an open PR, or asks to babysit a PR through the review/CI cycle. Examples: '/pr-babysit', '/pr-babysit --interval=2m --max-rounds=8'."
argument-hint: "[--interval=DURATION] [--max-rounds=N] [--pr=NUM]"
---

# /pr-babysit (S10) — AFK PR polling loop

Automate the post-PR feedback loop in **AFK mode**: poll `gh` for new bot review
comments + CI failures + merge conflicts, triage each signal into one of three
dispatch tiers, fix or hand off, and repeat until the PR is `[READY TO MERGE]`,
`[ESCALATED]` (to architector), or `[MAX ROUNDS EXHAUSTED]`.

This is the S10 polling sibling of `/pr-decide` (the human terminal step).
**Never** opens an `AskUserQuestion` gate — every "is this a judgment call?"
signal goes to tier 3 (architect). For interactive review-summary + merge/drop
decisions, use `/pr-decide` instead.

```
/pr-babysit [--interval=3m] [--max-rounds=5] [--pr=NUM]

Loop:
  1. Fetch PR data (state, checks, mergeable, three comment endpoints) — parallel
  2. Fast-exit if already green + no comments
  3. Categorize new signals: bot vs human; actionable / question / informational / stale
  4. /critic verdict routing: post bad verdicts to slice issue (dedup)
  5. Human comments → tier-3 (defer to architector via slice-issue summary)
  6. Bot actionable comments → triage:
       - tier 1 (mechanical, <10 LOC) → inline Agent fix → commit/push
       - tier 2 (semantic, broken tests, multi-file) → print /coder tmux command,
         expect new commits on next poll
  7. On CI red:
       - run /diagnose subagent (analysis only) on logs + diff
       - feed diagnosis into the triage rule above
  8. On conflicts → rebase / merge base; resolve small conflicts inline; bail to tier 2 for large
  9. Evaluate readiness (2× consecutive ready required; respects post-push cooldown)
 10. Tier-3 escalation when: 5 consecutive failed rounds OR critic wrong-direction
       OR any human comment OR any CRITICAL critic concern
```

## Prerequisites

- `gh` CLI authenticated with `repo` scope.
- `jq` available.
- `cwd` is a worktree on a feature branch with an open PR.
- The PR body contains a `Parent slice: #<N>` header line. /coder PRs from S8
  onward include this; older PRs need the line added manually before /pr-babysit
  can route to the slice issue. Bail loudly if missing.

## Arguments

Parse from the user's invocation line. All optional.

| Arg              | Default | Example          |
|------------------|---------|------------------|
| `--interval`     | `3m`    | `--interval=2m`  |
| `--max-rounds`   | `5`     | `--max-rounds=8` |
| `--pr`           | auto    | `--pr=42`        |

`--pr=NUM` overrides the auto-detect-from-branch step in Phase 0.

## Operating context

- This is an **AFK skill**. No `AskUserQuestion` calls anywhere in the loop.
- Push permission is required. The user has approved `git push` for /pr-babysit
  (it pushes tier-1 fixup commits). Use `git push --force-with-lease` only after
  a rebase in 1i (never `--force`).
- All tier-2 escalations are **printed**, not executed. The human reads the
  terminal, runs `tmux new-window -c <worktree> claude /coder ...` themselves.
- All state lives at `.claude/pr-babysit/<PR_NUMBER>/` — survives Ctrl-C and
  re-invocations.

## Phase 0: Setup

### 0a. Resolve the PR and the parent slice issue

Run the bundled fetch script once with `--setup` to get PR metadata + repo info
+ initial CHECKS in a single JSON payload:

```bash
bash "${CLAUDE_PLUGIN_ROOT:-$HOME/.claude/skills/pr-babysit}/scripts/run.sh" --setup [--pr=NUM]
```

The script:
- Auto-detects the PR for the current branch via `gh pr view` (or uses `--pr=NUM`).
- Returns JSON: `{pr_number, pr_url, base_branch, mergeable, merge_state, created_at, owner, repo, body, checks}`.

Extract these into shell variables. **If no PR exists, exit with**:
```
[NO PR] No open PR on current branch. Run /make-pr first.
```

### 0b. Extract parent slice issue number

From the PR body, look for a line matching `^Parent slice: #(\d+)` (anchored at
line start, case-insensitive). Capture `<N>`.

If not found, exit with:
```
[NO SLICE LINK] PR body is missing 'Parent slice: #<N>' header.
Fix: edit the PR body (gh pr edit <num> --body ...), add a line like:
    Parent slice: #17
Then re-run /pr-babysit.
```

Store the slice issue number in `SLICE_ISSUE_NUM`.

**Print to the user:**
```
Babysitting PR: <PR_URL>
Slice issue:   #<SLICE_ISSUE_NUM>
```

### 0c. Fast-exit check

If ALL of these are true, print `[ALREADY READY TO MERGE] <PR_URL>` and exit:

- `MERGEABLE == "MERGEABLE"` and `MERGE_STATE == "CLEAN"`
- At least one check exists OR the PR was created > 5 minutes ago
- Every check is in a terminal state (SUCCESS, NEUTRAL, FAILURE, SKIPPED-with-duration>0)
- All required checks pass
- No unresolved review comments
- No previously-escalated state file (`.claude/pr-babysit/<PR_NUMBER>/escalated.flag`)

If CHECKS is empty and PR was created < 5 minutes ago, do NOT fast-exit — checks
likely haven't registered yet.

### 0d. Initialize state

```
STATE_DIR = .claude/pr-babysit/<PR_NUMBER>
mkdir -p "$STATE_DIR"

IDLE_ROUNDS_REMAINING = max-rounds (default 5)
MAX_TOTAL_ITERATIONS  = max-rounds * 3
TOTAL_ITERATIONS      = 0
INTERVAL              = interval (default 3m)
LAST_CHECKED          = PR_CREATED_AT  # first pass processes all existing comments
TOTAL_WAIT_MINUTES    = 0
CONSECUTIVE_READY_COUNT = 0
PUSHED_AT             = None
EXTERNAL_CHECK_PATIENCE = 10

# Persistent state — load from disk, create empty if missing
POSTED_CRITICS  = load_json $STATE_DIR/posted-critics.json  # {verdict_id: posted_at}
DISPATCH_STATE  = load_json $STATE_DIR/dispatch-state.json  # {fix_spec, dispatched_at, last_head_sha, polls_since_dispatch}
ESCALATED       = exists $STATE_DIR/escalated.flag
```

If `ESCALATED` is true, print `[ALREADY ESCALATED] <PR_URL> — see slice issue
#<SLICE_ISSUE_NUM>.` and exit. Re-running after escalation requires the human
to delete `$STATE_DIR/escalated.flag` first (this is intentional — escalation
means the architector owns it now).

Three counters prevent infinite loops:

- `IDLE_ROUNDS_REMAINING` resets when progress reaches green CI. Exhaustion → escalate.
- `MAX_TOTAL_ITERATIONS` never resets. Exhaustion → escalate.
- `EXTERNAL_CHECK_PATIENCE` counts down only when external checks are the sole
  blocker. Does NOT consume `IDLE_ROUNDS_REMAINING`.

## Phase 1: The loop

Each iteration runs the steps below in order. Evaluate exit + escalation
conditions at the end.

At the start of each iteration, print one status line:
```
Round <TOTAL_ITERATIONS + 1>/<max-rounds> — <PR_URL>
```

### 1a. Check PR state

```bash
PR_STATE=$(gh pr view "$PR_NUMBER" --json state -q '.state')
```

If `PR_STATE` is `MERGED` or `CLOSED`, print `[<STATE>] <PR_URL>` and exit.

### 1b. Fetch fresh data (one call, returns JSON)

```bash
bash "${CLAUDE_PLUGIN_ROOT:-$HOME/.claude/skills/pr-babysit}/scripts/run.sh" --poll \
  --pr="$PR_NUMBER" --since="$LAST_CHECKED" > "$STATE_DIR/round.json"
```

The script fetches in parallel:
- `pulls/<N>/comments` — inline review comments since `$LAST_CHECKED`
- `pulls/<N>/reviews`  — review bodies since `$LAST_CHECKED`
- `issues/<N>/comments` — top-level PR comments since `$LAST_CHECKED` (bots post here)
- `pr view --json mergeable,mergeStateStatus,headRefOid,statusCheckRollup`
- `pr checks --json name,state,conclusion,detailsUrl`

Parse `round.json` into: `INLINE`, `REVIEWS`, `ISSUE_COMMENTS`, `MERGE`, `CHECKS`,
`HEAD_SHA`.

Update `LAST_CHECKED` to the current UTC timestamp (ISO 8601, e.g.
`2026-05-17T15:42:11Z`) **after** the fetch returns.

### 1c. Categorize signals

For each new comment from any of the three sources, tag:

**Author type:**
- **Bot** — `user.type == "Bot"` OR login ends in `[bot]` OR login is in the
  allowlist: `github-actions`, `cursor-bugbot`, `codecov`, `coderabbitai`,
  `renovate-bot`, `sonarcloud`, `claude-bot`.
- **Human** — everything else.

**Category:**
1. **/critic verdict** — the comment body starts with `## Critic verdict` or
   contains a JSON block with `"verdict": "..."` posted by the `/critic` skill.
   Extract `verdict_id` (sha of body), `verdict` (`fits | mixed | wrong-direction`),
   and `concerns` list.
2. **Actionable code change** — requests a specific modification.
3. **Question** — asks something needing a reply.
4. **Informational / FYI** — coverage reports, "LGTM", status bots. Skip entirely.
5. **Stale bot comment** — bot reviewed an old commit, lines changed since. Auto-reply
   "Addressed in a later commit." via the inline-replies API, then skip.

### 1d. Route /critic verdicts (step before any fix work)

For each `/critic verdict` signal in 1c:

- If `verdict_id` is in `POSTED_CRITICS`, skip (already routed).
- If `verdict == "fits"` AND there are zero `concerns`, skip (healthy — no routing).
- Otherwise, post a summary comment to the slice issue:

  ```bash
  gh issue comment "$SLICE_ISSUE_NUM" --body "$(cat <<EOF
  ## /critic verdict from PR #${PR_NUMBER}

  Verdict: \`${VERDICT}\` (concerns: ${CONCERN_COUNT})

  ${CONCERNS_BULLETS}

  PR: ${PR_URL}
  Critic comment: ${CRITIC_COMMENT_URL}

  _Posted by /pr-babysit._
  EOF
  )"
  ```

  Record `POSTED_CRITICS[verdict_id] = now_iso()` and persist
  `$STATE_DIR/posted-critics.json`.

- If `verdict == "wrong-direction"`, **also** trigger tier-3 escalation in 1k
  (the per-iteration escalation check).

### 1e. AFK human-comment policy

Any **human** comment in this round's signals — actionable OR question — is
treated as a tier-3 signal. Do NOT fix it. Do NOT reply.

Record the comment ids in `HUMAN_FEEDBACK_THIS_ROUND` (in-memory). When 1k runs,
they get bundled into the escalation summary alongside any other tier-3 triggers.

**This deliberately differs from the agent-templates source skill**, which gated
human comments via `AskUserQuestion`. In AFK mode, the human isn't here.

### 1f. Process bot actionable comments — triage + dispatch

For each **bot** actionable comment:

1. **Auto-dismiss stale comments** (already handled in 1c).
2. **Read the referenced file + line** so the triage decision is grounded.
3. **Classify into tier 1 or tier 2** using this rule:

   **Tier 1 — inline fix** if ALL of:
   - The proposed change is mechanical (fmt, lint auto-fix, single-line typo,
     missing import, doc/comment text, rename in one file).
   - Estimated diff < 10 LOC.
   - Single file or single hunk.
   - No new test required.

   **Tier 2 — /coder dispatch** otherwise.

4. **If tier 1**: dispatch a `subagent_type: coder` (fall back to
   `general-purpose`) `Agent` call with a prompt of the form:

   ```
   Apply this mechanical fix to the current worktree:

   File: <path>
   Line: <line>
   Reviewer comment:
   > <comment body>

   The change must be mechanical and <10 LOC. Do NOT refactor, rename anything
   outside the indicated file, or expand scope. Make ONE commit with title
   "fix: <one-line>". Do NOT push (the babysit loop pushes).

   Report back: commit sha, files changed.
   ```

   After the agent returns, post a reply to the comment thread:

   ```bash
   gh api "repos/$OWNER/$REPO/pulls/$PR_NUMBER/comments/$COMMENT_ID/replies" \
     -f body="Fixed in <sha> — <one-line summary>."
   ```

5. **If tier 2**: do NOT modify code. Print to the terminal:

   ```
   [TIER-2 DISPATCH NEEDED]
   Comment: <comment_url>
   Fix-spec:
     <reviewer's request, paraphrased into 3-5 sentences>

   Run in a new tmux window:
     tmux new-window -c <worktree-abs-path> claude /coder "<fix-spec>"

   /pr-babysit will keep polling. New commits on this PR = progress.
   ```

   Record in `DISPATCH_STATE`:
   ```json
   {
     "fix_spec": "<paraphrased request>",
     "comment_id": <id>,
     "dispatched_at": "<iso>",
     "last_head_sha": "<HEAD_SHA at dispatch time>",
     "polls_since_dispatch": 0
   }
   ```
   Persist to `$STATE_DIR/dispatch-state.json`.

   Post a one-line reply on the comment thread:
   ```
   Routed to /coder. Babysit polling for follow-up commits. (pr-babysit)
   ```

6. **Subsequent rounds** — if `DISPATCH_STATE` is non-empty at the start of an
   iteration, check whether `HEAD_SHA` has advanced past `last_head_sha`:

   - **Advanced**: tier-2 dispatch produced a commit. Clear `DISPATCH_STATE`
     (write `{}` to disk). The new commits will be evaluated naturally by 1g/1h
     this round. Reset `polls_since_dispatch` implicitly.
   - **Same SHA**: increment `polls_since_dispatch`. If it reaches 3, count this
     iteration as 1 failed round (decrement `IDLE_ROUNDS_REMAINING`), reset
     `polls_since_dispatch = 0`, leave `DISPATCH_STATE` intact (human may still
     be working on it). Subsequent rounds re-trigger the same "1 failed round
     after 3 stale polls" rule.

### 1g. Apply tier-1 commits if any landed this round

If any tier-1 fixes were applied in 1f:

```bash
git status --short
```

If anything is staged or untracked-but-just-added by the agent, run the project's
format + lint auto-fix gates (from `.claude/gates.json` if present; otherwise
infer from manifest — `cargo fmt && cargo clippy --fix`, `ruff check --fix`,
`npm run lint -- --fix`).

If `git status` shows nothing new, the agent must have already committed. Skip
the gates run.

### 1h. CI: invoke /diagnose on red, then fix

Categorize each check from `CHECKS`:

- **Passing**: `conclusion in (SUCCESS, NEUTRAL)`.
- **Failing**: `conclusion == FAILURE`.
- **Pending**: `state in (PENDING, QUEUED, IN_PROGRESS)`.

If any checks are **failing**:

1. **Spawn /diagnose subagent (one Agent call, analysis only)**:

   Read the /diagnose discipline into `$DIAGNOSE_PROMPT`. The file lives at one
   of (in priority order, use the first that exists):
   - `${CLAUDE_PLUGIN_ROOT}/../diagnose/SKILL.md` when /pr-babysit is installed as a plugin
   - `$HOME/.claude/skills/diagnose/SKILL.md` for the user install
   - `<repo-root>/skills/diagnose/SKILL.md` when running from a worktree checkout
   Capture failure logs:
   ```bash
   for FAIL_RUN in $(echo "$CHECKS" | jq -r '.[] | select(.conclusion=="FAILURE") | .detailsUrl' | grep -oE '[0-9]+$'); do
     gh run view "$FAIL_RUN" --log-failed 2>/dev/null | tail -200
   done > "$STATE_DIR/round-fail-logs.txt"
   ```

   Capture the diff:
   ```bash
   git diff "origin/$BASE_BRANCH"...HEAD > "$STATE_DIR/round-diff.patch"
   ```

   Dispatch one `Agent` (subagent_type=general-purpose) with prompt:

   ```
   You are running the /diagnose discipline as analysis only. DO NOT edit any
   files. DO NOT make commits. Output a structured diagnosis the caller will
   use to dispatch a fix.

   /diagnose discipline:
   ===
   <contents of skills/diagnose/SKILL.md, full file>
   ===

   CI failure logs (last 200 lines of each failed run):
   ===
   <contents of $STATE_DIR/round-fail-logs.txt>
   ===

   PR diff (origin/<base>...HEAD):
   ===
   <contents of $STATE_DIR/round-diff.patch>
   ===

   Output a markdown report with these exact sections:
     ## Hypotheses (ranked, 3-5)
       <each with falsifiable prediction>
     ## Top hypothesis — confidence
       <which one + why>
     ## Suggested fix
       <concrete: file:line + what to change; OR a unified diff block; OR
       "needs /coder — explain why mechanical fix is insufficient">
     ## Fix tier estimate
       tier-1 (mechanical, <10 LOC, single file) | tier-2 (semantic / multi-file / new test needed)
   ```

   Save the agent's output to `$STATE_DIR/diagnosis-<round>.md`.

2. **Feed diagnosis into triage rule**:

   - If "Fix tier estimate" == tier-1: treat exactly like a bot-actionable tier-1
     in 1f — dispatch an inline coder Agent with the suggested fix.
   - Else: treat exactly like tier-2 — print the dispatch command. The fix-spec
     for /coder includes the full diagnosis report content.

3. **If a failure looks like a flaky test or infra issue** (the diagnosis report
   says so, OR the same check failed in the previous round's diagnosis and the
   fix didn't address the underlying logic): do NOT modify code. Note "flaky:
   <check-name>" in the round summary. Don't decrement `IDLE_ROUNDS_REMAINING`
   for the flake itself, but if it persists 3 rounds, treat as tier-3 escalation.

### 1i. Resolve merge conflicts

If `MERGE.mergeable == "CONFLICTING"` or `MERGE.mergeStateStatus == "DIRTY"`:

```bash
git fetch origin "$BASE_BRANCH"
git rebase "origin/$BASE_BRANCH"
```

If the rebase has conflicts:
- If > 3 files conflict, abort and bail to tier-2 (print dispatch command with
  fix-spec "resolve merge conflicts with $BASE_BRANCH").
- Otherwise, attempt to resolve inline: for each conflict, read both sides; if
  the resolution is obvious (e.g., one side untouched, other touched), pick the
  touched side. If non-obvious, abort and bail to tier-2.

If resolved: `git rebase --continue` (or `git merge --continue` if you fell back
to merge).

### 1j. Commit + push if changes were made

If `git status --short` is non-empty OR new commits exist beyond
`$DISPATCH_STATE.last_head_sha`:

1. If there are unstaged tier-1 fixes (which were supposed to be committed by
   their agent), stage and commit:
   ```bash
   git add -A
   git commit -m "fix: address review feedback (tier-1 mechanical fixes)"
   ```

2. Push:
   ```bash
   git push || git push --force-with-lease  # after a rebase only
   ```

3. **Reset `IDLE_ROUNDS_REMAINING` to `max-rounds`** — only if changes pushed
   were ours (tier-1) OR represent visible /coder progress.
4. **Reset `CONSECUTIVE_READY_COUNT` to 0.**
5. **Set `PUSHED_AT` to current time.**

**Post-push cooldown**: if `PUSHED_AT` is set and < 60s elapsed, do NOT evaluate
readiness this round. Skip 1k's READY check; still evaluate escalation conditions.

### 1k. Evaluate readiness + escalation

Increment `TOTAL_ITERATIONS`.

**Tier-3 escalation check (evaluated every round, before READY check)**:

Trigger escalation if ANY of:
- `IDLE_ROUNDS_REMAINING <= 0`
- `TOTAL_ITERATIONS >= MAX_TOTAL_ITERATIONS`
- `EXTERNAL_CHECK_PATIENCE <= 0`
- Any signal this round was a `/critic verdict` with `verdict == "wrong-direction"`
- Any signal this round was a `/critic verdict` with at least one CRITICAL concern
- `HUMAN_FEEDBACK_THIS_ROUND` is non-empty
- A flaky check persisted ≥ 3 rounds

If triggered → **escalate**:

1. Build the escalation summary (markdown):

   ```
   ## /pr-babysit escalation — PR #<num>

   PR: <PR_URL>
   Branch: <branch>
   Rounds: <TOTAL_ITERATIONS> (idle remaining: <IDLE_ROUNDS_REMAINING>)

   ### Trigger(s)
   - <bulleted list of which conditions fired>

   ### Last CI state
   <table of check name + state + conclusion>

   ### Human feedback this round
   <list of human comment urls + first line of each, or "_none_">

   ### /critic verdicts seen
   <list of verdict + concerns; or "_none_">

   ### Recent diagnoses
   <up to 3 most recent diagnosis-*.md headlines>

   ### Suggested next move
   <one paragraph from the architector's perspective — drop?, re-scope?, redispatch?>
   ```

2. Post the summary as a comment on the slice issue:
   ```bash
   gh issue comment "$SLICE_ISSUE_NUM" --body "$SUMMARY"
   ```

3. Apply the `architect-attention` label to the slice issue (create label if
   missing):
   ```bash
   gh issue edit "$SLICE_ISSUE_NUM" --add-label architect-attention 2>/dev/null \
     || (gh label create architect-attention --color B60205 --description "/pr-babysit escalated; needs architect" \
         && gh issue edit "$SLICE_ISSUE_NUM" --add-label architect-attention)
   ```

4. Post a one-line comment on the PR linking to the slice issue:
   ```bash
   gh pr comment "$PR_NUMBER" --body "[/pr-babysit] Escalated to slice #$SLICE_ISSUE_NUM (architect-attention). $PR_URL"
   ```

5. Touch `$STATE_DIR/escalated.flag` so subsequent /pr-babysit invocations refuse
   to restart until the human removes the flag (signalling "architector has
   handled it").

6. Print `[ESCALATED] <PR_URL> — slice #<SLICE_ISSUE_NUM>` and exit.

**READY check (only if escalation didn't fire)**:

Check unreplied comments first: fetch ALL review comments on the PR (not just
new ones), exclude bots' own informational chatter, and check if any actionable
comment has zero replies. An unreplied actionable comment means feedback was
not addressed → not ready (treat as if we made no progress: do not decrement
`IDLE_ROUNDS_REMAINING` for this specifically, but `CONSECUTIVE_READY_COUNT`
stays 0).

**HARD RULE — never declare ready if any check is in `PENDING`, `QUEUED`, or
`IN_PROGRESS`. No "external-check bypass."**

If post-push cooldown is OFF AND no changes were needed this round AND no
unreplied comments exist AND ≥ 1 check exists AND every check is terminal AND
all required pass AND no conflicts:

- Increment `CONSECUTIVE_READY_COUNT`.
- If `CONSECUTIVE_READY_COUNT >= 2`, print `[READY TO MERGE] <PR_URL>` and exit.
- Otherwise, immediately re-fetch (skip the wait) and re-evaluate (avoid a race
  where a comment lands between fetches).

If readiness conditions aren't met because checks are still pending — separate
the cases:

- **Only external checks pending, all code CI passing** → decrement
  `EXTERNAL_CHECK_PATIENCE`. Log "Waiting for external check: `<name>`."
- **Code CI checks also pending/failing** → reset `CONSECUTIVE_READY_COUNT = 0`
  but don't decrement `IDLE_ROUNDS_REMAINING` (we made progress earlier in the
  round; this round is just waiting).

If checks all terminal but some failed AND we couldn't fix them this round →
decrement `IDLE_ROUNDS_REMAINING`.

### 1l. Wait and loop

Wait `INTERVAL`. Increment `TOTAL_WAIT_MINUTES` by the interval duration.
Loop to 1a.

## State files

Under `.claude/pr-babysit/<PR_NUMBER>/`:

- `posted-critics.json` — `{verdict_id: posted_at_iso}`. Dedup for 1d.
- `dispatch-state.json` — `{fix_spec, comment_id, dispatched_at, last_head_sha, polls_since_dispatch}` or `{}`. Tracks one tier-2 in-flight at a time.
- `escalated.flag` — empty file. Set by 1k step 5. Blocks re-invocation until removed.
- `diagnosis-<N>.md` — diagnosis report from /diagnose, one per round that had CI red.
- `round.json`, `round-fail-logs.txt`, `round-diff.patch` — per-iteration scratch (overwritten each round).

All under `.claude/` so they're worktree-local and respected by the project's
existing `.gitignore` (add `.claude/pr-babysit/` to `.gitignore` if not already
ignored by a broader `.claude/` rule).

## Subagent contract

- **Tier-1 coder Agent**: `subagent_type: "coder"` (fallback `"general-purpose"`).
  Prompt MUST forbid scope expansion ("do NOT refactor", "do NOT touch files
  outside <list>"). Must produce ONE commit. Must NOT push.
- **/diagnose Agent**: `subagent_type: "general-purpose"`. Prompt MUST say
  "analysis only — do NOT edit files, do NOT commit". Output is a markdown
  report consumed by the babysit loop.

## Common mistakes

| Mistake | Fix |
|---------|-----|
| Auto-fixing or auto-replying to a human comment | AFK mode — human comments ALWAYS go to tier 3. No exceptions. |
| Asking AskUserQuestion in the loop | This skill never opens gates. If a decision needs human judgment, escalate to slice issue. |
| Tier-1 fix that's actually multi-file | Re-classify as tier-2 and print the dispatch command. Tier-1 must stay mechanical. |
| Calling /diagnose to fix the bug | /diagnose is analysis-only. The fix dispatch happens through tier-1/tier-2 routing afterward. |
| Posting every /critic verdict to slice issue | Only post on bad verdicts (wrong-direction OR any CRITICAL concern). Healthy verdicts stay on the PR. |
| Routing the same /critic twice | Dedup via `POSTED_CRITICS[verdict_id]`. Persist after every post. |
| Counting external check waits against IDLE_ROUNDS_REMAINING | Use `EXTERNAL_CHECK_PATIENCE` separately. |
| Declaring ready while a check is PENDING | Hard rule — terminal state required. No external-check bypass. |
| Force-pushing without lease | `--force-with-lease` only, only after a rebase. |
| Looping forever after tier-2 dispatch | 3 polls with no new commits = 1 failed round. Eventually escalates. |
| Re-running after escalation | Refuses unless the human deletes `$STATE_DIR/escalated.flag` (architector now owns the PR). |
| Sleeping during gate decisions | There are no gate decisions in AFK mode. Everything either auto-fixes or escalates. |
| Missing `Parent slice: #N` header in PR body | Bail at 0b with the exact fix instruction. /coder PRs (S8+) include this; backfill manually for older PRs. |

## Relationship to other skills

- `/coder` — produces the PR. PR body MUST include `Parent slice: #N` for
  /pr-babysit to work. /pr-babysit dispatches /coder via printed tmux commands
  for tier-2 fixes.
- `/cc-review` — runs before /pr-babysit picks up the PR; its findings live as
  PR comments that /pr-babysit then triages.
- `/critic` — same as /cc-review. Verdicts get routed by 1d.
- `/diagnose` — invoked as an analysis-only subagent on CI red.
- `/pr-decide` — the human terminal step. Run by the user after /pr-babysit
  exits with `[READY TO MERGE]` or `[ESCALATED]`.
- `/architector` (future, S12) — reads `architect-attention` issues; deals with
  escalated PRs.
