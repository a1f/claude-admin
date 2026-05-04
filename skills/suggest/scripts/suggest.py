#!/usr/bin/env python3
"""
suggest.py — scan a plan's milestone breakdown and report dispatchable tasks.

Usage:
    python3 suggest.py <plan-codename> [<milestone-id>]

Reads:
    - ~/.claude/plans/registry.json
    - <plan_dir>/breakdowns/<milestone-id>.md

Determinism:
    A task is "dispatchable" iff every blocker is satisfied.

Blocker syntax (on the **Blockers:** line of each task block):
    - "none"
    - "<task-id> <state>"   where state ∈ {merged, drafted, ready}
    - "label:<label-name>"  parent breakdown issue must carry the label
    - Multiple blockers separated by "; "

Exit codes:
    0  at least one task is dispatchable
    1  nothing dispatchable
    2  invalid args / config error
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

REGISTRY = Path.home() / ".claude" / "plans" / "registry.json"

TASK_HEADER_RE = re.compile(r"^### ([A-Z][A-Za-z0-9]*-T\d+)\s*·\s*(.+?)\s*$")
BLOCKERS_RE = re.compile(r"^\*\*Blockers:\*\*\s*(.+?)\s*$", re.MULTILINE)
LOC_RE = re.compile(r"^\*\*Estimated LOC:\*\*\s*~?(\d+)", re.MULTILINE)
TASK_STATE_BLOCKER_RE = re.compile(r"^([A-Z][A-Za-z0-9]*-T\d+)\s+(merged|drafted|ready)$")
LABEL_BLOCKER_RE = re.compile(r"^label:(\S+)$")


@dataclass
class Task:
    id: str
    title: str
    blockers_raw: str
    blockers: list[str]
    estimated_loc: Optional[int] = None


@dataclass
class BlockerCheck:
    blocker: str
    satisfied: bool
    reason: str


@dataclass
class TaskStatus:
    task: Task
    checks: list[BlockerCheck] = field(default_factory=list)

    @property
    def dispatchable(self) -> bool:
        return all(c.satisfied for c in self.checks)


# ---------- parsing ----------

def load_registry() -> dict:
    if not REGISTRY.exists():
        die(f"registry not found at {REGISTRY}")
    return json.loads(REGISTRY.read_text())


def parse_breakdown(md_path: Path) -> list[Task]:
    text = md_path.read_text()
    tasks: list[Task] = []
    # Split on lines starting with "### <task-id> · " (task headers)
    blocks = re.split(r"^(?=### [A-Z][A-Za-z0-9]*-T\d+\s*·)", text, flags=re.MULTILINE)
    for block in blocks:
        m = TASK_HEADER_RE.match(block.split("\n", 1)[0])
        if not m:
            continue
        task_id = m.group(1).strip()
        title = m.group(2).strip()
        bm = BLOCKERS_RE.search(block)
        blockers_raw = bm.group(1).strip() if bm else "none"
        blockers = parse_blocker_line(blockers_raw)
        loc_m = LOC_RE.search(block)
        loc = int(loc_m.group(1)) if loc_m else None
        tasks.append(Task(id=task_id, title=title, blockers_raw=blockers_raw, blockers=blockers, estimated_loc=loc))
    return tasks


def parse_blocker_line(raw: str) -> list[str]:
    if not raw or raw.strip().lower() == "none":
        return []
    return [b.strip() for b in raw.split(";") if b.strip()]


# ---------- blocker checks ----------

def check_blocker(blocker: str, gh_repo: str, breakdown_issue: int) -> BlockerCheck:
    m = TASK_STATE_BLOCKER_RE.match(blocker)
    if m:
        return check_task_state(blocker, m.group(1), m.group(2), gh_repo)
    m = LABEL_BLOCKER_RE.match(blocker)
    if m:
        return check_label(blocker, m.group(1), breakdown_issue, gh_repo)
    return BlockerCheck(blocker, False, f"unknown syntax: {blocker!r}")


def gh_pr_list(gh_repo: str, task_id: str, state: str) -> list[dict]:
    """Return PRs whose title contains [<task-id>] in the given state."""
    cmd = [
        "gh", "pr", "list",
        "--repo", gh_repo,
        "--search", f'"[{task_id}]" in:title',
        "--state", state,
        "--json", "number,title,isDraft,state",
        "--limit", "5",
    ]
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=20, check=True)
    except subprocess.CalledProcessError as e:
        raise RuntimeError(f"gh failed: {e.stderr.strip() or e}")
    except subprocess.TimeoutExpired:
        raise RuntimeError("gh timed out (20s)")
    return json.loads(res.stdout)


def check_task_state(blocker: str, task_id: str, state: str, gh_repo: str) -> BlockerCheck:
    gh_state = {"merged": "merged", "drafted": "open", "ready": "open"}[state]
    try:
        prs = gh_pr_list(gh_repo, task_id, gh_state)
    except RuntimeError as e:
        return BlockerCheck(blocker, False, str(e))
    if not prs:
        return BlockerCheck(blocker, False, f"no PR matching [{task_id}] in {gh_state}")
    if state == "merged":
        return BlockerCheck(blocker, True, f"PR #{prs[0]['number']} merged")
    if state == "drafted":
        drafts = [p for p in prs if p.get("isDraft")]
        if drafts:
            return BlockerCheck(blocker, True, f"draft PR #{drafts[0]['number']}")
        return BlockerCheck(blocker, False, "PR exists but is not a draft")
    if state == "ready":
        ready = [p for p in prs if not p.get("isDraft") and p.get("state") == "OPEN"]
        if ready:
            return BlockerCheck(blocker, True, f"ready PR #{ready[0]['number']}")
        return BlockerCheck(blocker, False, "PR exists but draft or closed")
    return BlockerCheck(blocker, False, f"unhandled state: {state}")


def check_label(blocker: str, label: str, breakdown_issue: int, gh_repo: str) -> BlockerCheck:
    cmd = ["gh", "issue", "view", str(breakdown_issue), "--repo", gh_repo, "--json", "labels"]
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=15, check=True)
    except subprocess.CalledProcessError as e:
        return BlockerCheck(blocker, False, f"gh failed: {e.stderr.strip() or e}")
    except subprocess.TimeoutExpired:
        return BlockerCheck(blocker, False, "gh timed out (15s)")
    data = json.loads(res.stdout)
    labels = {lbl["name"] for lbl in data.get("labels", [])}
    if label in labels:
        return BlockerCheck(blocker, True, f"label '{label}' present on #{breakdown_issue}")
    return BlockerCheck(blocker, False, f"label '{label}' missing on #{breakdown_issue}")


# ---------- output ----------

DISPATCH_CMD = "python3 /Users/alf/.claude/skills/dispatch/scripts/dispatch.py {codename} {task_id}"


def render_milestone(codename: str, milestone: dict, statuses: list[TaskStatus]) -> tuple[bool, str]:
    """Return (any_dispatchable, rendered_text)."""
    lines: list[str] = []
    m_id = milestone["id"]
    m_title = milestone["title"]
    issue_url = milestone.get("breakdown", {}).get("issue_url", "?")
    dispatchable = [s for s in statuses if s.dispatchable]
    blocked = [s for s in statuses if not s.dispatchable]

    lines.append("")
    lines.append(f"== {m_id} · {m_title} ==")
    lines.append(f"   issue: {issue_url}")
    lines.append(f"   {len(dispatchable)}/{len(statuses)} task(s) dispatchable")

    if dispatchable:
        lines.append("")
        lines.append("  Ready to dispatch:")
        for s in dispatchable:
            t = s.task
            loc = f"~{t.estimated_loc} LOC" if t.estimated_loc else "?"
            lines.append(f"    ✓ {t.id}  {t.title}  ({loc})")
            if not t.blockers:
                lines.append("       blockers: none")
            else:
                for c in s.checks:
                    lines.append(f"       ✓ {c.blocker}  ·  {c.reason}")
            lines.append(f"       → {DISPATCH_CMD.format(codename=codename, task_id=t.id)}")
            lines.append("")

    if blocked:
        if dispatchable:
            lines.append("")
        lines.append("  Blocked:")
        for s in blocked:
            t = s.task
            lines.append(f"    ◯ {t.id}  {t.title}")
            for c in s.checks:
                marker = "✓" if c.satisfied else "✗"
                lines.append(f"       {marker} {c.blocker}  ·  {c.reason}")
            lines.append("")

    return bool(dispatchable), "\n".join(lines)


# ---------- main ----------

def die(msg: str, code: int = 2) -> None:
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(code)


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    codename = sys.argv[1]
    milestone_filter = sys.argv[2] if len(sys.argv) > 2 else None

    registry = load_registry()
    plan = registry.get("plans", {}).get(codename)
    if not plan:
        available = ", ".join(sorted(registry.get("plans", {}).keys())) or "(none)"
        die(f"plan '{codename}' not in registry. Available: {available}")

    plan_dir = Path(plan["plan_dir"])
    gh_repo = plan["gh_repo"]
    milestones_source = json.loads(Path(plan["milestones_source"]).read_text())
    milestones = milestones_source.get("milestones", [])

    if milestone_filter:
        milestones = [m for m in milestones if m["id"] == milestone_filter]
        if not milestones:
            die(f"milestone '{milestone_filter}' not found in {codename}")

    any_dispatchable = False
    rendered_chunks: list[str] = []
    skipped_unbroken = 0

    for milestone in milestones:
        breakdown = milestone.get("breakdown")
        if not breakdown:
            skipped_unbroken += 1
            continue
        local_file = Path(breakdown["local_file"])
        if not local_file.exists():
            print(f"WARNING: {milestone['id']}: breakdown file missing at {local_file}", file=sys.stderr)
            continue
        issue_url = breakdown.get("issue_url", "")
        try:
            issue_num = int(issue_url.rsplit("/", 1)[-1])
        except ValueError:
            print(f"WARNING: {milestone['id']}: cannot parse issue number from {issue_url!r}", file=sys.stderr)
            issue_num = 0

        tasks = parse_breakdown(local_file)
        statuses: list[TaskStatus] = []
        for t in tasks:
            checks = [check_blocker(b, gh_repo, issue_num) for b in t.blockers]
            statuses.append(TaskStatus(task=t, checks=checks))
        ok, text = render_milestone(codename, milestone, statuses)
        rendered_chunks.append(text)
        any_dispatchable = any_dispatchable or ok

    if not rendered_chunks:
        if skipped_unbroken:
            print(f"No broken-down milestones in '{codename}' ({skipped_unbroken} milestone(s) not yet broken down).")
            print("Run `/breakdown {codename} <milestone-id>` first.".format(codename=codename))
        else:
            print(f"No milestones to scan in '{codename}'.")
        return 1

    print("\n".join(rendered_chunks))
    if skipped_unbroken:
        print()
        print(f"  ({skipped_unbroken} milestone(s) skipped — not yet broken down)")

    return 0 if any_dispatchable else 1


if __name__ == "__main__":
    sys.exit(main())
