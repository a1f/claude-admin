# {{milestone_id}} · {{milestone_title}}

> **Goal.** {{milestone_goal}}

**Phase:** `{{milestone_phase}}` · **Kind:** `{{milestone_kind}}` · **Target PRs:** {{pr_count_target}} · **Risk:** {{milestone_risk}}

**Exit criteria.** _{{exit_criteria}}_

**Milestone depends on:** {{milestone_depends_on_or_none}}

**Plan:** [{{plan_codename}}]({{plan_doc}})

---

## Task checklist

{{#each tasks}}
- [ ] **[{{id}}](#{{id_anchor}})** — {{title}} _(~{{estimated_loc}} LOC)_
{{/each}}

**Aggregate:** {{task_count}} tasks · ~{{total_loc}} LOC

---

## Tasks

{{#each tasks}}
{{>task}}
---
{{/each}}

## Notes for the coder agent

- Stack: {{stack_summary}}
- Each task is roughly one PR. Open as **draft**, push commits, request review when the validation section is fully satisfied.
- Tests for the code in a task ship in the same PR.
- Task IDs are stable; reference them in PR titles and commit messages: e.g., `[{{milestone_id}}-T1] add /healthz handler`.
