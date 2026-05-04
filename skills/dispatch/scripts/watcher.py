#!/usr/bin/env python3
"""
watcher.py — monitor a dispatched coder, fan out reviews when done, surface state.

Subcommands:
    wait    <plan> <task-id>      blocking; runs as background process from dispatch
    status  <plan> <task-id>      one-shot snapshot to stdout
    done    <plan> <task-id>      explicit done sentinel (called by coder)
    abort   <plan> <task-id>      SIGTERM coder + watcher, mark aborted
    review  <plan> <task-id>      manually (re-)run reviewers + critics on an existing PR

State file at ~/.work/dispatches/<plan>/<task-id>/state.json is the single source of truth.
log.jsonl is append-only, written by claude itself; watcher tails it for events.
reviews/ directory holds per-agent output + aggregated summary.

Phases:
    spawning -> coding -> reviewing -> reviewed     (success)
                       -> stuck                     (no events for STUCK_AFTER_S)
                       -> permission_blocked        (claude logged a permission-denied)
                       -> errored                   (coder process exited non-zero)
                       -> aborted                   (user invoked abort)
    reviewing  -> review_failed                     (all review subprocesses errored)
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path

# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------

WORK_ROOT = Path.home() / ".work" / "dispatches"

STATE_TICK_S = 10
GH_POLL_S = 30
STUCK_AFTER_S = 600           # 10 min idle in coding phase
GRACE_AFTER_TERMINAL_S = 60   # keep watcher alive briefly after final state
REVIEW_POLL_S = 5             # poll review subprocesses this often
REVIEW_TIMEOUT_S = 600        # max wall-clock for the whole review fan-out

REVIEWER_KINDS = ["security", "bugs", "quality"]
CRITIC_COUNT = 5
CRITIC_ACCEPT_THRESHOLD = 80
CRITIC_REJECT_THRESHOLD = 60

PERMISSION_DENY_HINTS = ("permission_denied", "tool_use_error", "blocked by permission")

REVIEWER_SKILL = Path.home() / ".claude" / "skills" / "reviewer" / "SKILL.md"
CRITIC_SKILL = Path.home() / ".claude" / "skills" / "critic" / "SKILL.md"


# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------

def now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")


def state_dir(plan: str, task_id: str) -> Path:
    return WORK_ROOT / plan / task_id


def state_path(plan: str, task_id: str) -> Path:
    return state_dir(plan, task_id) / "state.json"


def log_path(plan: str, task_id: str) -> Path:
    return state_dir(plan, task_id) / "log.jsonl"


def review_dir(plan: str, task_id: str) -> Path:
    return state_dir(plan, task_id) / "reviews"


def read_state(plan: str, task_id: str) -> dict:
    sp = state_path(plan, task_id)
    if not sp.exists():
        return {}
    return json.loads(sp.read_text())


def write_state(plan: str, task_id: str, state: dict) -> None:
    sp = state_path(plan, task_id)
    tmp = sp.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(state, indent=2))
    tmp.replace(sp)


def proc_alive(pid: int | None) -> bool:
    if not pid:
        return False
    try:
        os.kill(pid, 0)
        return True
    except (ProcessLookupError, PermissionError):
        return False


def is_terminal(phase: str) -> bool:
    return phase in {"reviewed", "review_failed", "errored", "stuck", "permission_blocked", "aborted"}


def strip_frontmatter(text: str) -> str:
    if not text.startswith("---\n"):
        return text
    end = text.find("\n---\n", 4)
    if end == -1:
        return text
    return text[end + 5:].lstrip()


def parse_log_line(line: str) -> dict | None:
    line = line.strip()
    if not line:
        return None
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return None


def event_kind(ev: dict) -> str:
    return ev.get("type") or ev.get("event_type") or ev.get("kind") or "unknown"


def event_tool_name(ev: dict) -> str | None:
    if "tool_name" in ev:
        return ev["tool_name"]
    msg = ev.get("message") or {}
    if isinstance(msg, dict):
        for c in msg.get("content", []) or []:
            if isinstance(c, dict) and c.get("type") == "tool_use":
                return c.get("name")
    return None


def looks_like_permission_block(ev: dict) -> bool:
    blob = json.dumps(ev).lower()
    return any(h in blob for h in PERMISSION_DENY_HINTS)


def poll_for_draft_pr(gh_repo: str, task_id: str) -> tuple[str | None, int | None]:
    cmd = [
        "gh", "pr", "list",
        "--repo", gh_repo,
        "--search", f'"[{task_id}]" in:title',
        "--state", "open",
        "--json", "url,number,isDraft",
        "--limit", "5",
    ]
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=15, check=True)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return None, None
    prs = json.loads(res.stdout or "[]")
    for p in prs:
        if p.get("isDraft"):
            return p.get("url"), p.get("number")
    return None, None


# ----------------------------------------------------------------------------
# Review fan-out
# ----------------------------------------------------------------------------

def fetch_pr_diff(pr_num: int, gh_repo: str, dest: Path) -> bool:
    res = subprocess.run(
        ["gh", "pr", "diff", str(pr_num), "--repo", gh_repo],
        capture_output=True, text=True, timeout=60,
    )
    if res.returncode != 0:
        return False
    dest.write_text(res.stdout)
    return True


def fetch_pr_body(pr_num: int, gh_repo: str) -> dict | None:
    res = subprocess.run(
        ["gh", "pr", "view", str(pr_num), "--repo", gh_repo, "--json", "title,body,headRefName"],
        capture_output=True, text=True, timeout=15,
    )
    if res.returncode != 0:
        return None
    return json.loads(res.stdout)


def find_task_block_in_breakdown(plan: str, task_id: str) -> str | None:
    registry = json.loads((Path.home() / ".claude" / "plans" / "registry.json").read_text())
    plan_entry = registry.get("plans", {}).get(plan)
    if not plan_entry:
        return None
    src_path = Path(plan_entry["milestones_source"])
    if not src_path.exists():
        return None
    data = json.loads(src_path.read_text())
    m_id = task_id.rsplit("-T", 1)[0]
    for m in data.get("milestones", []):
        if m["id"] != m_id:
            continue
        bd = m.get("breakdown", {})
        local = Path(bd.get("local_file", ""))
        if not local.exists():
            return None
        text = local.read_text()
        blocks = re.split(r"^(?=### [A-Z][A-Za-z0-9]*-T\d+\s*·)", text, flags=re.MULTILINE)
        for b in blocks:
            if b.lstrip().startswith(f"### {task_id}"):
                return b.strip()
    return None


def write_review_context(rdir: Path, task_id: str, plan: str, pr_data: dict) -> None:
    task_block = find_task_block_in_breakdown(plan, task_id) or "(task spec not found)"
    pr_body = pr_data.get("body") or "(no PR body)"
    pr_title = pr_data.get("title") or ""
    md = f"""# PR review context · {task_id}

## PR title
{pr_title}

## Task spec (from breakdown)

{task_block}

---

## PR body (what the coder claims)

{pr_body}
"""
    (rdir / "pr-context.md").write_text(md)


def spawn_agent(name: str, rdir: Path, system_prompt: str, user_prompt: str) -> tuple[str, subprocess.Popen]:
    out_file = rdir / f"{name}.json"
    log_file = rdir / f"{name}.log"
    cmd = [
        "claude", "-p",
        "--output-format", "text",
        "--permission-mode", "acceptEdits",
        "--allowedTools", "Read Glob Grep",
        "--add-dir", str(rdir),
        "--append-system-prompt", system_prompt,
        user_prompt,
    ]
    proc = subprocess.Popen(
        cmd,
        stdout=open(out_file, "wb"),
        stderr=open(log_file, "wb"),
        cwd=str(rdir),
        start_new_session=True,
    )
    return name, proc


def fan_out_reviews(plan: str, task_id: str, pr_url: str, pr_num: int, gh_repo: str) -> dict:
    rdir = review_dir(plan, task_id)
    rdir.mkdir(parents=True, exist_ok=True)

    if not fetch_pr_diff(pr_num, gh_repo, rdir / "pr-diff.patch"):
        raise RuntimeError("failed to fetch PR diff")
    pr_data = fetch_pr_body(pr_num, gh_repo) or {}
    write_review_context(rdir, task_id, plan, pr_data)

    if not REVIEWER_SKILL.exists():
        raise RuntimeError(f"reviewer skill not found at {REVIEWER_SKILL}")
    if not CRITIC_SKILL.exists():
        raise RuntimeError(f"critic skill not found at {CRITIC_SKILL}")

    reviewer_sys = strip_frontmatter(REVIEWER_SKILL.read_text())
    critic_sys = strip_frontmatter(CRITIC_SKILL.read_text())

    agents: list[tuple[str, subprocess.Popen]] = []

    for kind in REVIEWER_KINDS:
        user_prompt = (
            f"You are reviewing PR for task {task_id} of plan {plan}.\n\n"
            f"Reviewer kind: {kind}\n\n"
            "Read these files in your working directory:\n"
            "- pr-diff.patch — the unified diff\n"
            "- pr-context.md — the task spec and PR body\n\n"
            "Output ONLY the JSON object as specified in your system prompt. "
            "No markdown fences, no prose."
        )
        name, proc = spawn_agent(f"reviewer-{kind}", rdir, reviewer_sys, user_prompt)
        agents.append((name, proc))

    for i in range(1, CRITIC_COUNT + 1):
        user_prompt = (
            f"You are critic #{i} of {CRITIC_COUNT} reviewing PR for task {task_id} of plan {plan}.\n\n"
            "Read these files in your working directory:\n"
            "- pr-diff.patch — the unified diff\n"
            "- pr-context.md — the task spec and PR body\n\n"
            "Output ONLY the JSON object as specified in your system prompt. "
            "No markdown fences, no prose."
        )
        name, proc = spawn_agent(f"critic-{i}", rdir, critic_sys, user_prompt)
        agents.append((name, proc))

    return {name: proc.pid for name, proc in agents}


def parse_agent_output(path: Path) -> dict | None:
    if not path.exists():
        return None
    text = path.read_text().strip()
    if not text:
        return None
    if text.startswith("```"):
        first_nl = text.find("\n")
        if first_nl != -1:
            text = text[first_nl + 1:]
        if text.endswith("```"):
            text = text[:-3].rstrip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        start = text.find("{")
        end = text.rfind("}")
        if start != -1 and end != -1 and end > start:
            try:
                return json.loads(text[start:end + 1])
            except json.JSONDecodeError:
                return None
    return None


def all_agents_done(agent_pids: dict[str, int]) -> bool:
    return all(not proc_alive(pid) for pid in agent_pids.values())


def aggregate_reviews(rdir: Path) -> dict:
    reviewer_results = {}
    critic_results = []
    for kind in REVIEWER_KINDS:
        out = parse_agent_output(rdir / f"reviewer-{kind}.json")
        reviewer_results[kind] = out if out is not None else {"error": "no parseable output"}

    for i in range(1, CRITIC_COUNT + 1):
        out = parse_agent_output(rdir / f"critic-{i}.json")
        if out is not None:
            critic_results.append(out)
        else:
            critic_results.append({"error": "no parseable output", "instance": i})

    severity_counts = {"blocker": 0, "major": 0, "minor": 0, "nit": 0}
    findings_total = 0
    for r in reviewer_results.values():
        for f in r.get("findings", []) or []:
            sev = f.get("severity", "nit")
            severity_counts[sev] = severity_counts.get(sev, 0) + 1
            findings_total += 1

    valid_scores = [c["score"] for c in critic_results if isinstance(c.get("score"), (int, float))]
    critic_avg = round(sum(valid_scores) / len(valid_scores), 1) if valid_scores else None
    critic_min = min(valid_scores) if valid_scores else None
    critic_max = max(valid_scores) if valid_scores else None

    if critic_avg is None:
        recommendation = "abstain"
    elif severity_counts["blocker"] > 0:
        recommendation = "iterate"
    elif critic_avg >= CRITIC_ACCEPT_THRESHOLD:
        recommendation = "accept"
    elif critic_avg < CRITIC_REJECT_THRESHOLD:
        recommendation = "drop"
    else:
        recommendation = "iterate"

    summary = {
        "completed_at": now_iso(),
        "reviewers": reviewer_results,
        "critics": critic_results,
        "severity_counts": severity_counts,
        "findings_total": findings_total,
        "critic_avg": critic_avg,
        "critic_min": critic_min,
        "critic_max": critic_max,
        "recommendation": recommendation,
    }
    (rdir / "summary.json").write_text(json.dumps(summary, indent=2))
    return summary


def render_pr_comment(task_id: str, summary: dict) -> str:
    sev = summary["severity_counts"]
    avg = summary["critic_avg"]
    rec = summary["recommendation"]
    lines = [f"## Automated review · `{task_id}`", ""]
    lines.append(
        f"**Recommendation:** `{rec}`  ·  **Critic avg:** `{avg}`  "
        f"(range {summary['critic_min']}–{summary['critic_max']} across {CRITIC_COUNT} critics)"
    )
    lines.append("")
    lines.append(
        f"**Reviewer findings:** {summary['findings_total']} total · "
        f"blocker {sev['blocker']} · major {sev['major']} · minor {sev['minor']} · nit {sev['nit']}"
    )
    lines.append("")
    lines.append("### Reviewers")
    for kind in REVIEWER_KINDS:
        r = summary["reviewers"].get(kind, {})
        if "error" in r:
            lines.append(f"- **{kind}**: _{r['error']}_")
            continue
        s = r.get("summary", "(no summary)")
        n = len(r.get("findings", []) or [])
        lines.append(f"- **{kind}** ({n} finding{'s' if n != 1 else ''}): {s}")
        for f in (r.get("findings", []) or [])[:6]:
            file_ref = f.get("file") or "?"
            line_ref = ""
            if f.get("lines"):
                line_ref = ":" + "-".join(str(x) for x in f["lines"])
            lines.append(f"    - `{f.get('severity','?')}` `{file_ref}{line_ref}` — {f.get('desc','?')}")
            if f.get("suggested_fix"):
                lines.append(f"      → {f['suggested_fix']}")
    lines.append("")
    lines.append("### Critics")
    for i, c in enumerate(summary["critics"], 1):
        if "error" in c:
            lines.append(f"- **#{i}**: _{c['error']}_")
            continue
        score = c.get("score", "?")
        verdict = c.get("verdict", "?")
        rationale = (c.get("rationale_md") or "").strip().replace("\n", " ")
        lines.append(f"- **#{i}** — score `{score}` · verdict `{verdict}` — {rationale}")
        for concern in (c.get("concerns") or [])[:3]:
            lines.append(f"    - {concern}")
    lines.append("")
    lines.append("---")
    lines.append("*Posted by claude_admin watcher. Architect decision pending user review.*")
    return "\n".join(lines)


def post_pr_comment(pr_num: int, gh_repo: str, body: str) -> bool:
    res = subprocess.run(
        ["gh", "pr", "comment", str(pr_num), "--repo", gh_repo, "--body", body],
        capture_output=True, text=True, timeout=30,
    )
    return res.returncode == 0


def trigger_review_fanout(plan: str, task_id: str, state: dict, now_ts: float) -> tuple[bool, dict]:
    """Helper: kick off review fan-out, return (success, updated_state).
    state must have pr_url, pr_number, gh_repo set."""
    try:
        review_pids = fan_out_reviews(plan, task_id, state["pr_url"], state["pr_number"], state["gh_repo"])
    except Exception as e:
        state["phase"] = "review_failed"
        state["error_summary"] = f"failed to fan out reviews: {e}"
        return False, state
    state["phase"] = "reviewing"
    state["review"] = {
        "phase": "running",
        "started_at": now_iso(),
        "started_at_ts": now_ts,
        "agents": [{"name": k, "pid": v} for k, v in review_pids.items()],
    }
    return True, state


# ----------------------------------------------------------------------------
# Subcommand: wait (long-running)
# ----------------------------------------------------------------------------

def cmd_wait(plan: str, task_id: str) -> int:
    sd = state_dir(plan, task_id)
    if not sd.exists():
        print(f"error: state dir missing {sd}", file=sys.stderr)
        return 2

    log = log_path(plan, task_id)
    last_gh_poll = 0.0
    last_state_write = 0.0
    log_offset = 0
    terminal_seen_at: float | None = None
    state = read_state(plan, task_id)
    tool_counts: dict[str, int] = dict(state.get("tool_counts", {}))

    while True:
        now = time.time()
        state = read_state(plan, task_id) or state

        # ---- Tail log ----
        try:
            with open(log, "rb") as f:
                f.seek(log_offset)
                new_bytes = f.read()
                log_offset = f.tell()
        except FileNotFoundError:
            new_bytes = b""

        if new_bytes:
            for raw in new_bytes.decode("utf-8", errors="replace").splitlines():
                ev = parse_log_line(raw)
                if not ev:
                    continue
                state["last_event_at"] = now_iso()
                tn = event_tool_name(ev)
                if tn:
                    tool_counts[tn] = tool_counts.get(tn, 0) + 1
                if state["phase"] == "coding" and looks_like_permission_block(ev):
                    state["phase"] = "permission_blocked"
                    state["stuck_reason"] = f"permission denied: tool={tn or '?'} kind={event_kind(ev)}"

        # ---- Coding phase: poll for PR + check coder process ----
        if state["phase"] == "coding":
            if now - last_gh_poll > GH_POLL_S:
                last_gh_poll = now
                pr_url, pr_num = poll_for_draft_pr(state["gh_repo"], task_id)
                if pr_url:
                    state["pr_url"] = pr_url
                    state["pr_number"] = pr_num
                    write_state(plan, task_id, state)
                    _, state = trigger_review_fanout(plan, task_id, state, now)

            if state["phase"] == "coding" and not proc_alive(state.get("coder_pid")):
                pr_url, pr_num = poll_for_draft_pr(state["gh_repo"], task_id)
                if pr_url:
                    state["pr_url"] = pr_url
                    state["pr_number"] = pr_num
                    write_state(plan, task_id, state)
                    _, state = trigger_review_fanout(plan, task_id, state, now)
                else:
                    state["phase"] = "errored"
                    state["error_summary"] = "coder process exited without pushing a draft PR"

            if state["phase"] == "coding":
                last = state.get("last_event_at")
                if last:
                    last_dt = dt.datetime.fromisoformat(last)
                    if (dt.datetime.now(dt.timezone.utc) - last_dt).total_seconds() > STUCK_AFTER_S:
                        state["phase"] = "stuck"
                        state["stuck_reason"] = f"no events for {STUCK_AFTER_S}s"

        # ---- Reviewing phase: wait for all subprocesses to finish ----
        elif state["phase"] == "reviewing":
            review = state.get("review") or {}
            agent_pids = {a["name"]: a["pid"] for a in review.get("agents", [])}
            review_started_ts = review.get("started_at_ts", now)

            if agent_pids and all_agents_done(agent_pids):
                rdir = review_dir(plan, task_id)
                summary = aggregate_reviews(rdir)
                pr_num = state.get("pr_number")
                comment_body = render_pr_comment(task_id, summary)
                posted = bool(pr_num) and post_pr_comment(pr_num, state["gh_repo"], comment_body)
                state["review"] = {
                    "phase": "done",
                    "started_at": review.get("started_at"),
                    "completed_at": summary["completed_at"],
                    "recommendation": summary["recommendation"],
                    "critic_avg": summary["critic_avg"],
                    "findings_total": summary["findings_total"],
                    "severity_counts": summary["severity_counts"],
                    "comment_posted": posted,
                }
                state["phase"] = "reviewed"
            elif (now - review_started_ts) > REVIEW_TIMEOUT_S:
                for name, pid in agent_pids.items():
                    if proc_alive(pid):
                        try:
                            os.kill(pid, signal.SIGTERM)
                        except ProcessLookupError:
                            pass
                rdir = review_dir(plan, task_id)
                summary = aggregate_reviews(rdir)
                state["review"] = {
                    "phase": "timeout",
                    "completed_at": summary["completed_at"],
                    "recommendation": summary.get("recommendation", "abstain"),
                }
                state["phase"] = "review_failed"
                state["error_summary"] = f"review fan-out timeout after {REVIEW_TIMEOUT_S}s"

        # ---- Update tool counts + elapsed ----
        state["tool_counts"] = tool_counts
        try:
            started_dt = dt.datetime.fromisoformat(state["started_at"])
            state["elapsed_s"] = int((dt.datetime.now(dt.timezone.utc) - started_dt).total_seconds())
        except (KeyError, ValueError):
            pass

        # ---- Persist ----
        if now - last_state_write > STATE_TICK_S or is_terminal(state.get("phase", "")):
            write_state(plan, task_id, state)
            last_state_write = now

        # ---- Terminal grace ----
        if is_terminal(state.get("phase", "")):
            if terminal_seen_at is None:
                terminal_seen_at = now
            elif now - terminal_seen_at > GRACE_AFTER_TERMINAL_S:
                write_state(plan, task_id, state)
                update_milestones_phase(plan, task_id, state["phase"], state.get("pr_url"))
                return 0

        time.sleep(2)


# ----------------------------------------------------------------------------
# Subcommand: status
# ----------------------------------------------------------------------------

def cmd_status(plan: str, task_id: str) -> int:
    state = read_state(plan, task_id)
    if not state:
        print(f"no dispatch state for {plan}/{task_id}", file=sys.stderr)
        return 1
    phase = state.get("phase", "?")
    elapsed = state.get("elapsed_s", 0)
    print(f"== {task_id} ({plan}) ==")
    print(f"   phase:        {phase}")
    print(f"   elapsed:      {elapsed}s")
    print(f"   started:      {state.get('started_at')}")
    print(f"   last event:   {state.get('last_event_at')}")
    print(f"   worktree:     {state.get('worktree')}")
    print(f"   branch:       {state.get('branch')}")
    print(f"   coder pid:    {state.get('coder_pid')} ({'alive' if proc_alive(state.get('coder_pid')) else 'gone'})")
    print(f"   watcher pid:  {state.get('watcher_pid')} ({'alive' if proc_alive(state.get('watcher_pid')) else 'gone'})")
    tc = state.get("tool_counts") or {}
    if tc:
        print(f"   tools:        " + " ".join(f"{k}:{v}" for k, v in sorted(tc.items())))
    if state.get("pr_url"):
        print(f"   pr:           {state['pr_url']}")
    if state.get("stuck_reason"):
        print(f"   stuck reason: {state['stuck_reason']}")
    if state.get("error_summary"):
        print(f"   error:        {state['error_summary']}")
    rev = state.get("review") or {}
    if rev:
        print(f"   review:       phase={rev.get('phase')} avg={rev.get('critic_avg')} "
              f"rec={rev.get('recommendation')} findings={rev.get('findings_total')}")
    return 0


# ----------------------------------------------------------------------------
# Subcommand: done
# ----------------------------------------------------------------------------

def cmd_done(plan: str, task_id: str) -> int:
    sd = state_dir(plan, task_id)
    if not sd.exists():
        print(f"no state dir for {plan}/{task_id}", file=sys.stderr)
        return 1
    sentinel = sd / "done.sentinel"
    sentinel.write_text(now_iso())
    print(f"done sentinel written: {sentinel}")
    return 0


# ----------------------------------------------------------------------------
# Subcommand: abort
# ----------------------------------------------------------------------------

def cmd_abort(plan: str, task_id: str) -> int:
    state = read_state(plan, task_id)
    if not state:
        print(f"no state for {plan}/{task_id}", file=sys.stderr)
        return 1

    coder_pid = state.get("coder_pid")
    watcher_pid = state.get("watcher_pid")

    for pid, name in [(coder_pid, "coder"), (watcher_pid, "watcher")]:
        if not pid:
            continue
        try:
            os.kill(pid, signal.SIGTERM)
            print(f"sent SIGTERM to {name} pid {pid}")
        except ProcessLookupError:
            pass

    for ag in (state.get("review") or {}).get("agents") or []:
        pid = ag.get("pid")
        if pid:
            try:
                os.kill(pid, signal.SIGTERM)
                print(f"sent SIGTERM to {ag.get('name')} pid {pid}")
            except ProcessLookupError:
                pass

    state["phase"] = "aborted"
    state["error_summary"] = state.get("error_summary") or "aborted by user"
    write_state(plan, task_id, state)
    update_milestones_phase(plan, task_id, "aborted", state.get("pr_url"))
    return 0


# ----------------------------------------------------------------------------
# Subcommand: review (manual / re-run)
# ----------------------------------------------------------------------------

def cmd_review(plan: str, task_id: str) -> int:
    state = read_state(plan, task_id)
    if not state:
        print(f"no state for {plan}/{task_id}", file=sys.stderr)
        return 1
    pr_url = state.get("pr_url")
    pr_num = state.get("pr_number")
    if not pr_url or not pr_num:
        pr_url, pr_num = poll_for_draft_pr(state.get("gh_repo", ""), task_id)
        if not pr_url:
            print(f"no draft PR found for {task_id}", file=sys.stderr)
            return 1
        state["pr_url"] = pr_url
        state["pr_number"] = pr_num

    ok, state = trigger_review_fanout(plan, task_id, state, time.time())
    write_state(plan, task_id, state)
    if not ok:
        print(f"failed to fan out reviews: {state.get('error_summary')}", file=sys.stderr)
        return 1
    n = len((state.get("review") or {}).get("agents", []))
    print(f"spawned {n} review subprocess(es). Use status to monitor.")
    return 0


# ----------------------------------------------------------------------------
# Milestone update
# ----------------------------------------------------------------------------

def update_milestones_phase(plan: str, task_id: str, phase: str, pr_url: str | None) -> None:
    registry_path = Path.home() / ".claude" / "plans" / "registry.json"
    if not registry_path.exists():
        return
    registry = json.loads(registry_path.read_text())
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
        if not d:
            d = m["dispatches"][task_id] = {}
        d["phase"] = phase
        d["last_phase_at"] = now_iso()
        if pr_url:
            d["pr_url"] = pr_url
        break
    src.write_text(json.dumps(data, indent=2))


# ----------------------------------------------------------------------------
# Main
# ----------------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("action", choices=["wait", "status", "done", "abort", "review"])
    ap.add_argument("plan")
    ap.add_argument("task_id")
    args = ap.parse_args()

    if args.action == "wait":
        return cmd_wait(args.plan, args.task_id)
    if args.action == "status":
        return cmd_status(args.plan, args.task_id)
    if args.action == "done":
        return cmd_done(args.plan, args.task_id)
    if args.action == "abort":
        return cmd_abort(args.plan, args.task_id)
    if args.action == "review":
        return cmd_review(args.plan, args.task_id)
    return 2


if __name__ == "__main__":
    sys.exit(main())
