---
name: revisit
description: "Post-merge (or post-drop) plan reconciliation. Re-reads the plan + downstream milestones in light of what just shipped (or didn't), and proposes amendments: tasks that need updating, milestones that need re-ordering, assumptions that turned out wrong. Saves a structured recommendation. Use when the user asks to revisit the plan after a PR, check if the plan still holds, or invokes /revisit. Examples: '/revisit v2_design M0a-T1', 'does the plan still hold after M0a-T1?'."
argument-hint: "<plan-codename> <task-id>"
---

# Revisit

You re-read the plan in light of a just-merged (or just-dropped) task, and propose amendments to downstream tasks/milestones. The merge may have revealed assumptions that turned out wrong, or already accomplished something a later task expected to do, or made a later task unnecessary.

## Inputs

`/revisit <plan-codename> <task-id>`

If args missing, ask via AskUserQuestion.

## Steps

### 1. Resolve & read state

```bash
cat ~/.work/dispatches/<plan>/<task-id>/state.json
```

Note `phase` (must be `merged` or `dropped` — revisit doesn't apply to in-flight tasks).

### 2. Read the plan + milestones

From `~/.claude/plans/registry.json`:

- `plan_doc` — the canonical plan
- `milestones_source` — the milestones JSON

Read the plan body and walk every milestone with status `planned` or `broken_down` (downstream of this merge, or potentially affected by it). For broken-down milestones, also read each task's spec from the breakdown file.

### 3. Read what just shipped (or didn't)

```bash
gh pr view <pr_number> --repo <gh_repo> --json title,body,mergedAt,state
gh pr diff <pr_number> --repo <gh_repo>
```

Pay close attention to the PR body's "What this PR does NOT do" + "Open items / questions" — those are signals about gaps the coder noticed.

### 4. Walk every downstream task and ask

For each downstream task spec, ask yourself:

- **Does this still need doing?** Maybe the merge already covered it.
- **Is the spec still accurate?** The merge may have changed file paths, function names, or interfaces the task spec references.
- **Are the test scenarios still appropriate?** If the merge changed the surface, scenarios may need updating.
- **Are dependencies still correct?** A dropped task changes the blocker chain.
- **Did this merge reveal a gap?** Maybe a NEW task needs to be added between current and a downstream one.
- **Is a milestone reorder warranted?** If the merge changed what's "easy" vs "hard", maybe X should come before Y.

### 5. For drops specifically

If the task was DROPPED (not merged), ask:

- Did the drop reveal a flaw in the plan that affects sibling/downstream tasks?
- Should the dropped task be re-broken-down differently?
- Should the milestone be re-scoped?

### 6. Output JSON

Write to `~/.work/dispatches/<plan>/<task-id>/post-merge/plan-revisit.json`.

Schema:

```json
{
  "task_id": "<task-id>",
  "plan": "<plan>",
  "trigger": "merged|dropped",
  "analyzed_at": "<ISO timestamp>",
  "summary": "<1-line: does the plan still hold? Y/N + 1-line why>",
  "amendments": [
    {
      "target_kind": "milestone|task|plan-section",
      "target_id": "<milestone-id, task-id, or section name>",
      "change_kind": "update|add|remove|reorder",
      "what": "<concrete description of the change>",
      "why": "<rationale, citing what the merge revealed>",
      "confidence": "high|medium|low"
    }
  ],
  "no_action_required_for": [
    "<milestone-id or task-id>"
  ]
}
```

`no_action_required_for` is the explicit "I checked these and they're fine" list — useful so the user knows you actually walked them.

### 7. Print summary

Show the user: total amendments by `change_kind`, top 3 high-confidence ones, path to JSON. Don't dump full JSON.

## Confidence rubric

- **high** — clear evidence in the merge that this amendment is needed (e.g., the merged code already implements task X.Y; remove it).
- **medium** — reasonable inference but the user should verify (e.g., this assumption seems shakier now).
- **low** — possibility worth noting; user judgment call.

## Focus

- Tasks the merge made unnecessary
- Tasks whose spec referenced files/symbols the merge moved or renamed
- Assumptions stated in the plan body that turned out wrong
- Gaps the coder noticed in the PR's "Open items / questions" section
- Re-ordering opportunities revealed by the merge

## What NOT to do

- Don't amend already-merged tasks (sealed)
- Don't propose speculative future work that isn't in the plan
- Don't rewrite the plan's vision / motivation — only practical amendments
- Don't propose every nit; high-signal only
- Don't apply amendments yourself — output the JSON, the user decides

## Following up

If the user agrees with high-confidence amendments, they can apply them by editing `milestones.json` directly (manually for v1) or via a future `/apply-amendment` skill. The JSON output is the audit trail of "we considered this and proposed it".

## Subtle cases

- A merge can make a downstream task's "Test scenarios" out of date if the merged code changed the testable surface. Flag those as `update` amendments.
- A dropped task might mean its breakdown needs to be regenerated entirely (`change_kind: update` or `add` for the next try).
- If the merge was incomplete (PR body lists deferred items), those become candidates for `add` amendments to the same milestone.
