#!/usr/bin/env python3
"""
pr_babysit.py — post-review decision interface for a dispatched task.

Subcommands:
    show     <plan> <task-id>                  print review summary, no side effects
    ready    <plan> <task-id>                  promote draft → ready (gh pr ready)
    merge    <plan> <task-id>                  gh pr merge --squash --delete-branch
    drop     <plan> <task-id> [--reason TEXT]  gh pr close + closing comment
    iterate  <plan> <task-id>                  v1 placeholder: post feedback comment, manual re-dispatch

State at ~/.work/dispatches/<plan>/<task-id>/state.json. Reviews summary at reviews/summary.json.

Exits 0 on success. Non-zero on errors / state mismatch.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import subprocess
import sys
from pathlib import Path

WORK_ROOT = Path.home() / ".work" / "dispatches"
REGISTRY = Path.home() / ".claude" / "plans" / "registry.json"


def now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")


def state_dir(plan: str, task_id: str) -> Path:
    return WORK_ROOT / plan / task_id


def state_path(plan: str, task_id: str) -> Path:
    return state_dir(plan, task_id) / "state.json"


def review_summary_path(plan: str, task_id: str) -> Path:
    return state_dir(plan, task_id) / "reviews" / "summary.json"


def read_state(plan: str, task_id: str) -> dict | None:
    sp = state_path(plan, task_id)
    if not sp.exists():
        return None
    return json.loads(sp.read_text())


def write_state(plan: str, task_id: str, state: dict) -> None:
    sp = state_path(plan, task_id)
    tmp = sp.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(state, indent=2))
    tmp.replace(sp)


def read_review_summary(plan: str, task_id: str) -> dict | None:
    sp = review_summary_path(plan, task_id)
    if not sp.exists():
        return None
    return json.loads(sp.read_text())


def die(msg: str, code: int = 2) -> None:
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(code)


def update_milestones_phase(plan: str, task_id: str, phase: str) -> None:
    """Mirror the phase change to milestones.json so /suggest sees it."""
    if not REGISTRY.exists():
        return
    registry = json.loads(REGISTRY.read_text())
    plan_entry = registry.get("plans", {}).get(plan)
    if not plan_entry:
        return
    src = Path(plan_entry["milestones_source"])
    if not src.exists():
        return
    data = json.loads(src.read_text())
    m_id = task_id.rsplit("-T", 1)[0]
    for m in data.get("milestones", []):
        if m["id"] != m_id:
            continue
        d = m.setdefault("dispatches", {}).get(task_id)
        if d:
            d["phase"] = phase
            d["last_phase_at"] = now_iso()
        break
    src.write_text(json.dumps(data, indent=2))


# ----------------------------------------------------------------------------
# show
# ----------------------------------------------------------------------------

def cmd_show(plan: str, task_id: str) -> int:
    state = read_state(plan, task_id)
    if not state:
        die(f"no dispatch state for {plan}/{task_id}. Was this PR created via /dispatch?")

    summary = read_review_summary(plan, task_id)

    print(f"== {task_id} · {plan} ==")
    print(f"   phase:        {state.get('phase')}")
    print(f"   pr:           {state.get('pr_url') or '(no PR yet)'}")
    print(f"   worktree:     {state.get('worktree')}")
    print(f"   elapsed:      {state.get('elapsed_s')}s")

    user_decision = state.get("user_decision")
    if user_decision:
        print(f"   user decision: {user_decision} at {state.get('user_decision_at')}")

    rev = state.get("review") or {}
    if rev:
        print(f"   review phase: {rev.get('phase')} · "
              f"recommendation={rev.get('recommendation')} · "
              f"critic_avg={rev.get('critic_avg')} · "
              f"findings={rev.get('findings_total')}")

    if not summary:
        print()
        print("(no review summary yet — review may still be running or failed)")
        return 0

    print()
    print(f"--- Review summary ---")
    print(f"   recommendation: {summary['recommendation']}")
    print(f"   critic avg:     {summary['critic_avg']} (range {summary['critic_min']}–{summary['critic_max']})")
    sev = summary["severity_counts"]
    print(f"   findings:       {summary['findings_total']} total · "
          f"blocker {sev['blocker']} · major {sev['major']} · minor {sev['minor']} · nit {sev['nit']}")
    print()

    print("Reviewers:")
    for kind, r in summary.get("reviewers", {}).items():
        if "error" in r:
            print(f"  [{kind}] error: {r['error']}")
            continue
        n = len(r.get("findings", []) or [])
        print(f"  [{kind}] {n} finding(s) — {r.get('summary', '')}")
        for f in (r.get("findings") or [])[:5]:
            file_ref = f.get("file") or "?"
            line_ref = ""
            if f.get("lines"):
                line_ref = ":" + "-".join(str(x) for x in f["lines"])
            print(f"    - [{f.get('severity','?')}] {file_ref}{line_ref}: {f.get('desc','?')}")
            if f.get("suggested_fix"):
                print(f"        → {f['suggested_fix']}")
    print()

    print("Critics:")
    for i, c in enumerate(summary.get("critics", []), 1):
        if "error" in c:
            print(f"  #{i}: error — {c['error']}")
            continue
        rationale = (c.get("rationale_md") or "").strip().replace("\n", " ")
        print(f"  #{i} score={c.get('score','?')} verdict={c.get('verdict','?')}")
        print(f"      {rationale}")
    print()

    return 0


# ----------------------------------------------------------------------------
# ready
# ----------------------------------------------------------------------------

def cmd_ready(plan: str, task_id: str) -> int:
    state = read_state(plan, task_id) or die_state(plan, task_id)
    pr_num = state.get("pr_number")
    if not pr_num:
        die(f"no PR number recorded for {task_id}")

    if state.get("phase") not in {"reviewed"}:
        die(f"phase is '{state.get('phase')}', expected 'reviewed' before ready", code=3)

    res = subprocess.run(
        ["gh", "pr", "ready", str(pr_num), "--repo", state["gh_repo"]],
        capture_output=True, text=True, timeout=30,
    )
    if res.returncode != 0:
        die(f"gh pr ready failed: {res.stderr.strip() or res.stdout.strip()}", code=4)

    state["phase"] = "accepted_pending_ci"
    state["user_decision"] = "accept"
    state["user_decision_at"] = now_iso()
    write_state(plan, task_id, state)
    update_milestones_phase(plan, task_id, "accepted_pending_ci")

    print(f"✓ PR {state.get('pr_url')} promoted to ready.")
    print(f"  Wait for CI to go green on GitHub, then:")
    print(f"  /pr-babysit {plan} {task_id} merge")
    return 0


# ----------------------------------------------------------------------------
# merge
# ----------------------------------------------------------------------------

def cmd_merge(plan: str, task_id: str) -> int:
    state = read_state(plan, task_id) or die_state(plan, task_id)
    pr_num = state.get("pr_number")
    if not pr_num:
        die(f"no PR number recorded for {task_id}")

    if state.get("phase") not in {"accepted_pending_ci", "reviewed"}:
        die(f"phase is '{state.get('phase')}', expected 'accepted_pending_ci' or 'reviewed' to merge", code=3)

    cmd = [
        "gh", "pr", "merge", str(pr_num),
        "--repo", state["gh_repo"],
        "--squash",
        "--delete-branch",
    ]
    res = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    if res.returncode != 0:
        err = res.stderr.strip() or res.stdout.strip()
        die(f"gh pr merge failed: {err}", code=4)

    state["phase"] = "merged"
    state["merged_at"] = now_iso()
    state["user_decision"] = state.get("user_decision") or "accept"
    write_state(plan, task_id, state)
    update_milestones_phase(plan, task_id, "merged")

    print(f"✓ Merged: {state.get('pr_url')}")
    print(f"  Branch deleted.")
    return 0


# ----------------------------------------------------------------------------
# drop
# ----------------------------------------------------------------------------

def cmd_drop(plan: str, task_id: str, reason: str | None) -> int:
    state = read_state(plan, task_id) or die_state(plan, task_id)
    pr_num = state.get("pr_number")
    gh_repo = state.get("gh_repo")

    if state.get("phase") in {"merged", "dropped"}:
        print(f"already {state['phase']}; nothing to do")
        return 0

    comment_body = None
    if reason:
        comment_body = f"**Dropped by /pr-babysit.** Reason: {reason}\n\n_Posted by claude_admin._"

    if pr_num and gh_repo:
        if comment_body:
            cm = subprocess.run(
                ["gh", "pr", "comment", str(pr_num), "--repo", gh_repo, "--body", comment_body],
                capture_output=True, text=True, timeout=30,
            )
            if cm.returncode != 0:
                print(f"warning: failed to post drop comment: {cm.stderr.strip()}", file=sys.stderr)

        cl = subprocess.run(
            ["gh", "pr", "close", str(pr_num), "--repo", gh_repo],
            capture_output=True, text=True, timeout=30,
        )
        if cl.returncode != 0:
            err = cl.stderr.strip() or cl.stdout.strip()
            # If already closed, that's fine
            if "already closed" not in err.lower():
                die(f"gh pr close failed: {err}", code=4)

    state["phase"] = "dropped"
    state["user_decision"] = "drop"
    state["user_decision_at"] = now_iso()
    if reason:
        state["drop_reason"] = reason
    write_state(plan, task_id, state)
    update_milestones_phase(plan, task_id, "dropped")

    print(f"✓ Dropped {task_id}.")
    if pr_num:
        print(f"  Closed PR: {state.get('pr_url')}")
    if reason:
        print(f"  Reason: {reason}")
    print(f"  Note: worktree at {state.get('worktree')} not removed; clean up manually or via /dispatch --force on a re-plan.")
    return 0


# ----------------------------------------------------------------------------
# iterate (v1 placeholder)
# ----------------------------------------------------------------------------

def cmd_iterate(plan: str, task_id: str) -> int:
    state = read_state(plan, task_id) or die_state(plan, task_id)
    summary = read_review_summary(plan, task_id)
    pr_num = state.get("pr_number")

    if not summary:
        die("no review summary to iterate from", code=3)
    if not pr_num:
        die("no PR number recorded", code=3)

    bundle = build_iterate_bundle(summary)

    cm = subprocess.run(
        ["gh", "pr", "comment", str(pr_num), "--repo", state["gh_repo"], "--body", bundle],
        capture_output=True, text=True, timeout=30,
    )
    if cm.returncode != 0:
        die(f"failed to post iterate comment: {cm.stderr.strip()}", code=4)

    state["user_decision"] = "iterate"
    state["user_decision_at"] = now_iso()
    state["iterate_count"] = (state.get("iterate_count") or 0) + 1
    write_state(plan, task_id, state)

    print(f"✓ Iterate feedback posted to PR {state.get('pr_url')}")
    print()
    print("v1 limitation: automated re-dispatch is not yet wired up.")
    print("To iterate manually:")
    print(f"  1. Review the feedback comment on the PR.")
    print(f"  2. Visit the worktree: cd {state.get('worktree')}")
    print(f"  3. Make fixes, commit, push.")
    print(f"  4. The existing draft PR will update.")
    print(f"  5. Re-run reviews when ready: python3 ~/.claude/skills/dispatch/scripts/watcher.py review {plan} {task_id}")
    return 0


def build_iterate_bundle(summary: dict) -> str:
    lines = ["## Iterate feedback (from claude_admin reviewers + critics)", ""]
    lines.append(f"**Recommendation:** `{summary['recommendation']}`  ·  "
                 f"**Critic avg:** {summary['critic_avg']} "
                 f"(range {summary['critic_min']}–{summary['critic_max']})")
    lines.append("")

    # Pull all blocker + major findings to the top
    priority_findings = []
    other_findings = []
    for kind, r in summary.get("reviewers", {}).items():
        for f in (r.get("findings") or []):
            sev = f.get("severity", "nit")
            tagged = {**f, "_kind": kind}
            if sev in ("blocker", "major"):
                priority_findings.append(tagged)
            else:
                other_findings.append(tagged)

    if priority_findings:
        lines.append("### Blockers / majors to fix")
        for f in priority_findings:
            file_ref = f.get("file") or "?"
            line_ref = ""
            if f.get("lines"):
                line_ref = ":" + "-".join(str(x) for x in f["lines"])
            lines.append(f"- **`{f['_kind']}` · `{f['severity']}` · `{file_ref}{line_ref}`** — {f.get('desc','')}")
            if f.get("suggested_fix"):
                lines.append(f"   → fix: {f['suggested_fix']}")
        lines.append("")

    # Top critic concerns
    concerns = []
    for c in summary.get("critics", []):
        if "error" in c:
            continue
        for cn in (c.get("concerns") or [])[:2]:
            concerns.append(cn)
    if concerns:
        # Dedup similar concerns
        seen = set()
        deduped = []
        for c in concerns:
            key = c[:60].lower()
            if key not in seen:
                seen.add(key)
                deduped.append(c)
        lines.append("### Goal-fit concerns from critics")
        for c in deduped[:8]:
            lines.append(f"- {c}")
        lines.append("")

    if other_findings:
        lines.append("### Nits / minors (optional)")
        for f in other_findings[:10]:
            file_ref = f.get("file") or "?"
            lines.append(f"- `{f['_kind']}` `{f['severity']}` `{file_ref}`: {f.get('desc','')}")
        lines.append("")

    lines.append("---")
    lines.append("_Posted by `/pr-babysit iterate`. Address the blockers + majors and concerns. "
                 "Push fixes to the same branch; the existing draft PR will update. "
                 "Re-run reviews afterwards via the watcher's `review` subcommand._")

    return "\n".join(lines)


# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------

def die_state(plan: str, task_id: str):
    die(f"no dispatch state at {state_path(plan, task_id)}. PR was not created via /dispatch.")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("action", choices=["show", "ready", "merge", "drop", "iterate"])
    ap.add_argument("plan")
    ap.add_argument("task_id")
    ap.add_argument("--reason", default=None, help="Reason for drop (used by 'drop' subcommand)")
    args = ap.parse_args()

    if args.action == "show":
        return cmd_show(args.plan, args.task_id)
    if args.action == "ready":
        return cmd_ready(args.plan, args.task_id)
    if args.action == "merge":
        return cmd_merge(args.plan, args.task_id)
    if args.action == "drop":
        return cmd_drop(args.plan, args.task_id, args.reason)
    if args.action == "iterate":
        return cmd_iterate(args.plan, args.task_id)
    return 2


if __name__ == "__main__":
    sys.exit(main())
