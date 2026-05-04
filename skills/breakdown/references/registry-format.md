# `~/.claude/plans/registry.json` format

```json
{
  "$schema_version": 1,
  "plans": {
    "<codename>": {
      "title": "<human-readable plan title>",
      "milestones_source": "<absolute path to milestones.json>",
      "plan_doc": "<absolute path or URL to canonical plan doc>",
      "plan_dir": "<absolute path to plan directory; breakdowns saved under <plan_dir>/breakdowns/>",
      "gh_repo": "<owner/name>",
      "default_base": "main",
      "stack": {
        "backend": "...",
        "frontend": "...",
        "db": "...",
        "agent_runtime": "..."
      }
    }
  }
}
```

# `<plan_dir>/milestones.json` format

```json
{
  "$schema_version": 1,
  "plan_codename": "<codename>",
  "plan_title": "<human title>",
  "gh_repo": "<owner/name>",
  "default_base": "main",
  "milestones": [
    {
      "id": "M0a",
      "title": "...",
      "phase": "foundation|repo|observe|author|dispatch|review|gate|merge|polish|codex",
      "kind": "infra|api|ui|api+ui|api+light_ui",
      "goal": "...",
      "pr_count_target": 5,
      "raw_prs": ["seed line 1", "seed line 2", "..."],
      "exit_criteria": "...",
      "risk": "low|low-medium|medium|high",
      "depends_on": ["<other-milestone-id>"],
      "status": "planned|broken_down|in_flight|all_merged|shipped|archived",
      "breakdown": {
        "issue_url": "https://github.com/owner/repo/issues/N",
        "local_file": "<plan_dir>/breakdowns/<id>.md",
        "task_count": 5,
        "created_at": "ISO-8601"
      }
    }
  ]
}
```

The `breakdown` object is added/updated by the `/breakdown` skill when the breakdown is generated.

# Adding a new plan

1. Create the plan's directory and a `milestones.json` inside it.
2. Add an entry under `plans.<codename>` in `~/.claude/plans/registry.json`.
3. Done — `/breakdown <codename> <id>` will find it.
