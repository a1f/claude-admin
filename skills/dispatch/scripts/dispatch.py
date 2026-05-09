#!/usr/bin/env python3
"""
dispatch.py — spawn a claude coder for one task in a fresh worktree, inside a tmux pane.

Usage:
    python3 dispatch.py <plan-codename> <task-id> [--force]

Side effects:
    - git fetch origin <default_base>
    - git worktree add -b <task-id> ~/dev/claude-admin-worktrees/<task-id>/ origin/<default_base>
    - mkdir ~/.work/dispatches/<plan>/<task-id>/
    - ensures shared tmux session 'claude-admin' exists (creates detached if missing)
    - opens window 'claude-admin:<task-id>' running 'claude -p ... | tee log.jsonl'
      (pane auto-closes on exit; operator can `tmux attach -t claude-admin` to watch live)
    - background subprocess: watcher.py wait <plan> <task-id>
    - updates milestones.json (status=dispatched + dispatch metadata)

Exits 0 on success.
Exits 2 on argument/config errors. Exits 3 on blocker check failure (use --force).
Exits 4 if state already exists (use --force).
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

# ----------------------------------------------------------------------------
# Paths and constants
# ----------------------------------------------------------------------------

REGISTRY = Path.home() / ".claude" / "plans" / "registry.json"
WORK_ROOT = Path.home() / ".work" / "dispatches"
WORKTREES_ROOT = Path.home() / "dev" / "claude-admin-worktrees"
WATCHER_PATH = Path(__file__).parent / "watcher.py"
CODER_SKILL_PATH = Path.home() / ".claude" / "skills" / "coder" / "SKILL.md"
TMUX_SESSION = "claude-admin"

# Whitelist for the coder's tool use. Anything outside this triggers
# permission_denied events visible in log.jsonl.
ALLOWED_TOOLS = " ".join([
    "Read", "Edit", "Write", "Glob", "Grep", "TodoWrite", "WebFetch",
    "Bash(cargo *)",
    "Bash(git *)",
    "Bash(gh *)",
    "Bash(rustfmt *)",
    "Bash(make *)",
    "Bash(ls *)",
    "Bash(cat *)",
    "Bash(echo *)",
    "Bash(grep *)",
    "Bash(rg *)",
    "Bash(mkdir *)",
    "Bash(touch *)",
    "Bash(pwd)",
    "Bash(which *)",
])

CODER_RULES_INLINE = """\
You are the CODER for an autonomous workflow.

Hard rules:
- Implement ONLY the task you are given. Do not start adjacent work.
- Open a draft PR titled `[<task-id>] <task-title>` against the default base branch.
- All tests for the code you write ship in the same PR.
- Write a complete PR body: what you did, what you didn't do, open questions, validation evidence.
- Do NOT ask the user questions. If you genuinely cannot proceed, document the question in
  the PR body and stop.
- When you have pushed the draft PR, exit cleanly. Do not loiter.
- You are running non-interactively (no TTY). There is no human at the keyboard.
"""

# ----------------------------------------------------------------------------
# Args
# ----------------------------------------------------------------------------

def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("plan_codename")
    ap.add_argument("task_id")
    ap.add_argument("--force", action="store_true",
                    help="Tear down existing worktree/branch/state and start fresh")
    return ap.parse_args()


# ----------------------------------------------------------------------------
# Registry / breakdown / blocker logic (duplicated minimally from suggest.py
# until we have a shared module)
# ----------------------------------------------------------------------------

TASK_HEADER_RE = re.compile(r"^### ([A-Z][A-Za-z0-9]*-T\d+)\s*·\s*(.+?)\s*$")
BLOCKERS_RE = re.compile(r"^\*\*Blockers:\*\*\s*(.+?)\s*$", re.MULTILINE)
TASK_STATE_BLOCKER_RE = re.compile(r"^([A-Z][A-Za-z0-9]*-T\d+)\s+(merged|drafted|ready)$")
LABEL_BLOCKER_RE = re.compile(r"^label:(\S+)$")


def die(msg: str, code: int = 2) -> None:
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(code)


def load_registry() -> dict:
    if not REGISTRY.exists():
        die(f"registry not found at {REGISTRY}")
    return json.loads(REGISTRY.read_text())


def find_milestone_for_task(task_id: str, milestones: list[dict]) -> dict | None:
    """Task ID 'M0a-T1' belongs to milestone 'M0a'."""
    m_id = task_id.rsplit("-T", 1)[0]
    for m in milestones:
        if m["id"] == m_id:
            return m
    return None


def parse_task_block(md_path: Path, task_id: str) -> dict | None:
    """Return {id, title, blockers_raw, blockers, body} for the task, or None if not found."""
    text = md_path.read_text()
    blocks = re.split(r"^(?=### [A-Z][A-Za-z0-9]*-T\d+\s*·)", text, flags=re.MULTILINE)
    for block in blocks:
        m = TASK_HEADER_RE.match(block.split("\n", 1)[0])
        if not m or m.group(1).strip() != task_id:
            continue
        title = m.group(2).strip()
        bm = BLOCKERS_RE.search(block)
        blockers_raw = bm.group(1).strip() if bm else "none"
        blockers = parse_blocker_line(blockers_raw)
        return {
            "id": task_id,
            "title": title,
            "blockers_raw": blockers_raw,
            "blockers": blockers,
            "body": block.strip(),
        }
    return None


def parse_blocker_line(raw: str) -> list[str]:
    if not raw or raw.strip().lower() == "none":
        return []
    return [b.strip() for b in raw.split(";") if b.strip()]


def strip_frontmatter(text: str) -> str:
    """Remove a leading YAML frontmatter block (--- ... ---) from a markdown string."""
    if not text.startswith("---\n"):
        return text
    end = text.find("\n---\n", 4)
    if end == -1:
        return text
    return text[end + 5:].lstrip()


def check_blocker(blocker: str, gh_repo: str, breakdown_issue: int) -> tuple[bool, str]:
    m = TASK_STATE_BLOCKER_RE.match(blocker)
    if m:
        return check_task_state(m.group(1), m.group(2), gh_repo)
    m = LABEL_BLOCKER_RE.match(blocker)
    if m:
        return check_label(m.group(1), breakdown_issue, gh_repo)
    return False, f"unknown syntax: {blocker!r}"


def check_task_state(task_id: str, state: str, gh_repo: str) -> tuple[bool, str]:
    gh_state = {"merged": "merged", "drafted": "open", "ready": "open"}[state]
    cmd = [
        "gh", "pr", "list",
        "--repo", gh_repo,
        "--search", f'"[{task_id}]" in:title',
        "--state", gh_state,
        "--json", "number,title,isDraft,state",
        "--limit", "5",
    ]
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=20, check=True)
    except subprocess.CalledProcessError as e:
        return False, f"gh failed: {e.stderr.strip() or e}"
    except subprocess.TimeoutExpired:
        return False, "gh timed out"
    prs = json.loads(res.stdout)
    if not prs:
        return False, f"no PR matching [{task_id}] in {gh_state}"
    if state == "merged":
        return True, f"PR #{prs[0]['number']} merged"
    if state == "drafted":
        drafts = [p for p in prs if p.get("isDraft")]
        if drafts:
            return True, f"draft PR #{drafts[0]['number']}"
        return False, "PR exists but not a draft"
    if state == "ready":
        ready = [p for p in prs if not p.get("isDraft") and p.get("state") == "OPEN"]
        if ready:
            return True, f"ready PR #{ready[0]['number']}"
        return False, "PR exists but draft or closed"
    return False, "unhandled state"


def check_label(label: str, breakdown_issue: int, gh_repo: str) -> tuple[bool, str]:
    cmd = ["gh", "issue", "view", str(breakdown_issue), "--repo", gh_repo, "--json", "labels"]
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=15, check=True)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
        return False, f"gh failed: {e}"
    data = json.loads(res.stdout)
    labels = {lbl["name"] for lbl in data.get("labels", [])}
    if label in labels:
        return True, f"label '{label}' present"
    return False, f"label '{label}' missing"


# ----------------------------------------------------------------------------
# tmux helpers
# ----------------------------------------------------------------------------

def tmux_available() -> bool:
    return shutil.which("tmux") is not None


def ensure_tmux_session() -> None:
    """Create the shared 'claude-admin' tmux session detached if it doesn't exist."""
    if not tmux_available():
        die("tmux not on PATH; install tmux or run dispatch on a machine with it")
    has = subprocess.run(
        ["tmux", "has-session", "-t", TMUX_SESSION],
        capture_output=True, text=True,
    )
    if has.returncode == 0:
        return
    create = subprocess.run(
        ["tmux", "new-session", "-d", "-s", TMUX_SESSION, "-n", "_idle"],
        capture_output=True, text=True,
    )
    if create.returncode != 0:
        die(f"tmux new-session failed: {create.stderr.strip()}")


def tmux_window_exists(window: str) -> bool:
    res = subprocess.run(
        ["tmux", "list-windows", "-t", TMUX_SESSION, "-F", "#{window_name}"],
        capture_output=True, text=True,
    )
    if res.returncode != 0:
        return False
    return window in res.stdout.splitlines()


def tmux_kill_window(window: str) -> None:
    subprocess.run(
        ["tmux", "kill-window", "-t", f"{TMUX_SESSION}:{window}"],
        capture_output=True, text=True,
    )


def tmux_pane_pid(window: str) -> int | None:
    """Best-effort pid of the pane's foreground process (the bash running our pipeline)."""
    res = subprocess.run(
        ["tmux", "list-panes", "-t", f"{TMUX_SESSION}:{window}", "-F", "#{pane_pid}"],
        capture_output=True, text=True,
    )
    if res.returncode != 0 or not res.stdout.strip():
        return None
    try:
        return int(res.stdout.strip().splitlines()[0])
    except ValueError:
        return None


def write_coder_runner(state_dir: Path, worktree: Path, log_path: Path,
                       stderr_path: Path, claude_argv: list[str]) -> Path:
    """Write a small bash runner the tmux pane will exec. Avoids shell-escaping the
    multi-line system prompt by writing it to a file and reading via $(<file)."""
    sys_prompt_file = state_dir / "system_prompt.txt"
    user_prompt_file = state_dir / "user_prompt.txt"

    # claude_argv is ["claude", "-p", ..., "--append-system-prompt", "<text>", "<user_prompt>"]
    # Pull out the two big strings; pass them through files in the runner.
    argv = list(claude_argv)
    sys_idx = argv.index("--append-system-prompt")
    sys_prompt_file.write_text(argv[sys_idx + 1])
    argv[sys_idx + 1] = "$(cat \"$SYS_PROMPT\")"

    user_prompt_file.write_text(argv[-1])
    argv[-1] = "$(cat \"$USER_PROMPT\")"

    # Build a shell-quoted command line (the two placeholders intentionally NOT quoted
    # so the shell evaluates them).
    quoted_parts: list[str] = []
    for a in argv:
        if a in ('$(cat "$SYS_PROMPT")', '$(cat "$USER_PROMPT")'):
            quoted_parts.append(f'"{a}"')
        else:
            # Naive single-quote escape, sufficient for our argv values.
            quoted_parts.append("'" + a.replace("'", "'\"'\"'") + "'")
    cmd_line = " ".join(quoted_parts)

    runner = state_dir / "coder.sh"
    runner.write_text(f"""#!/usr/bin/env bash
set -o pipefail
export SYS_PROMPT={shell_quote(str(sys_prompt_file))}
export USER_PROMPT={shell_quote(str(user_prompt_file))}
cd {shell_quote(str(worktree))}
{cmd_line} 2>>{shell_quote(str(stderr_path))} | tee -a {shell_quote(str(log_path))}
""")
    runner.chmod(0o755)
    return runner


def shell_quote(s: str) -> str:
    return "'" + s.replace("'", "'\"'\"'") + "'"


# ----------------------------------------------------------------------------
# Cleanup (for --force)
# ----------------------------------------------------------------------------

def kill_pid(pid: int | None, name: str) -> None:
    if not pid:
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    except PermissionError:
        print(f"warning: cannot signal {name} pid {pid}", file=sys.stderr)
        return
    for _ in range(50):
        time.sleep(0.1)
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def force_cleanup(state_dir: Path, worktree: Path, branch: str, repo_root: Path,
                  task_id: str) -> None:
    state_path = state_dir / "state.json"
    state: dict = {}
    if state_path.exists():
        try:
            state = json.loads(state_path.read_text())
        except json.JSONDecodeError:
            pass

    # Kill the tmux window (this SIGKILLs the entire process tree under the pane,
    # including the bash runner and the claude -p underneath).
    window = state.get("tmux_window") or task_id
    if tmux_available() and tmux_window_exists(window):
        tmux_kill_window(window)

    kill_pid(state.get("watcher_pid"), "watcher")
    # Fall back to PID kill in case the pane was already gone but the process
    # somehow wasn't (shouldn't happen — tmux owns its panes — but cheap insurance).
    kill_pid(state.get("coder_pid"), "coder")

    if worktree.exists():
        subprocess.run(
            ["git", "-C", str(repo_root), "worktree", "remove", "--force", str(worktree)],
            capture_output=True, text=True,
        )
        if worktree.exists():
            shutil.rmtree(worktree, ignore_errors=True)

    subprocess.run(
        ["git", "-C", str(repo_root), "branch", "-D", branch],
        capture_output=True, text=True,
    )

    if state_dir.exists():
        shutil.rmtree(state_dir, ignore_errors=True)


# ----------------------------------------------------------------------------
# Main
# ----------------------------------------------------------------------------

def main() -> int:
    args = parse_args()
    plan_codename = args.plan_codename
    task_id = args.task_id

    # Resolve plan
    registry = load_registry()
    plan = registry.get("plans", {}).get(plan_codename)
    if not plan:
        die(f"plan '{plan_codename}' not in registry")

    plan_dir = Path(plan["plan_dir"])
    gh_repo = plan["gh_repo"]
    default_base = plan.get("default_base", "main")
    repo_root = Path(plan.get("repo_local_path", str(plan_dir.parent)))

    if not repo_root.exists() or not (repo_root / ".git").exists():
        die(f"repo root not a git repo: {repo_root}")

    # Find milestone + breakdown file
    milestones_source = json.loads(Path(plan["milestones_source"]).read_text())
    milestones = milestones_source["milestones"]
    milestone = find_milestone_for_task(task_id, milestones)
    if milestone is None:
        die(f"no milestone found for task '{task_id}' (expected prefix like 'M0a')")
    breakdown = milestone.get("breakdown")
    if not breakdown:
        die(f"milestone '{milestone['id']}' has no breakdown — run /breakdown first")

    breakdown_file = Path(breakdown["local_file"])
    if not breakdown_file.exists():
        die(f"breakdown file missing: {breakdown_file}")

    issue_url = breakdown["issue_url"]
    issue_num = int(issue_url.rsplit("/", 1)[-1])

    task = parse_task_block(breakdown_file, task_id)
    if task is None:
        die(f"task '{task_id}' not found in {breakdown_file}")

    # Re-check blockers
    print(f"Checking blockers for {task_id}...")
    unmet: list[tuple[str, str]] = []
    for b in task["blockers"]:
        ok, reason = check_blocker(b, gh_repo, issue_num)
        if not ok:
            unmet.append((b, reason))
    if unmet:
        if args.force:
            print(f"  --force: ignoring {len(unmet)} unmet blocker(s):")
        else:
            print(f"  {len(unmet)} blocker(s) unmet:", file=sys.stderr)
        for b, r in unmet:
            line = f"    ✗ {b}  ·  {r}"
            print(line, file=sys.stderr if not args.force else sys.stdout)
        if not args.force:
            print("Use --force to dispatch anyway, or wait for blockers to clear.", file=sys.stderr)
            return 3

    # Paths for this dispatch
    worktree = WORKTREES_ROOT / task_id
    state_dir = WORK_ROOT / plan_codename / task_id
    branch = task_id

    # Pre-flight: existing state
    branch_exists = subprocess.run(
        ["git", "-C", str(repo_root), "rev-parse", "--verify", branch],
        capture_output=True, text=True,
    ).returncode == 0

    pre_existing = worktree.exists() or state_dir.exists() or branch_exists
    if pre_existing:
        if args.force:
            print(f"--force: cleaning up existing dispatch for {task_id}...")
            force_cleanup(state_dir, worktree, branch, repo_root, task_id)
        else:
            print(f"error: existing state for {task_id}:", file=sys.stderr)
            if worktree.exists():
                print(f"  worktree: {worktree}", file=sys.stderr)
            if branch_exists:
                print(f"  branch:   {branch}", file=sys.stderr)
            if state_dir.exists():
                print(f"  state:    {state_dir}", file=sys.stderr)
            print("Use --force to clean and re-dispatch.", file=sys.stderr)
            return 4

    # git fetch
    print(f"Fetching origin {default_base}...")
    fetch = subprocess.run(
        ["git", "-C", str(repo_root), "fetch", "origin", default_base],
        capture_output=True, text=True,
    )
    if fetch.returncode != 0:
        die(f"git fetch failed: {fetch.stderr.strip()}")

    # git worktree add
    print(f"Creating worktree at {worktree}...")
    WORKTREES_ROOT.mkdir(parents=True, exist_ok=True)
    wt = subprocess.run(
        ["git", "-C", str(repo_root), "worktree", "add", "-b", branch, str(worktree), f"origin/{default_base}"],
        capture_output=True, text=True,
    )
    if wt.returncode != 0:
        die(f"git worktree add failed: {wt.stderr.strip()}")

    # Init state dir
    state_dir.mkdir(parents=True, exist_ok=True)
    log_path = state_dir / "log.jsonl"
    log_path.touch()
    state_path = state_dir / "state.json"

    started_at = dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")
    initial_state = {
        "task_id": task_id,
        "plan": plan_codename,
        "milestone": milestone["id"],
        "phase": "spawning",
        "worktree": str(worktree),
        "branch": branch,
        "gh_repo": gh_repo,
        "issue_url": issue_url,
        "started_at": started_at,
        "last_event_at": None,
        "elapsed_s": 0,
        "tool_counts": {},
        "coder_pid": None,
        "watcher_pid": None,
        "pr_url": None,
        "error_summary": None,
        "stuck_reason": None,
    }
    state_path.write_text(json.dumps(initial_state, indent=2))

    # Build coder prompt — strip frontmatter from SKILL.md so it doesn't leak into the system prompt
    coder_skill_block = ""
    if CODER_SKILL_PATH.exists():
        raw = CODER_SKILL_PATH.read_text()
        coder_skill_block = strip_frontmatter(raw)
        system_prompt_source = "from ~/.claude/skills/coder/SKILL.md"
    else:
        coder_skill_block = CODER_RULES_INLINE
        system_prompt_source = "inline (coder skill not yet built)"

    user_prompt = f"""\
You are dispatched to implement task {task_id} of plan {plan_codename}.

The full task description (deliverable, expectation, scope, motivation, validation,
test scenarios) lives in GitHub issue {issue_url}, in the section titled `### {task_id}`.

To read it:
    gh issue view {issue_num} --repo {gh_repo}

You are operating in this worktree:
    {worktree}

Branch: {branch} (already created from origin/{default_base}).

Workflow:
1. Read the task spec from the GH issue.
2. Implement only this task — stay within scope.
3. Write/update tests for code you produced (see "Test scenarios" in the task spec).
4. Push commits to branch `{branch}`.
5. Open a DRAFT PR titled `[{task_id}] <task-title>` against `{default_base}`.
6. Write a complete PR body: what you did, what you didn't do, open items.
7. Exit cleanly when the draft PR is up.

Rules:
- Don't ask questions. Document open items in the PR body.
- Stay in scope. Don't refactor adjacent code unless the task asks.
- Tests ship with the code in the same PR.
- This is non-interactive: there is no human at the terminal.
"""

    claude_argv = [
        "claude",
        "-p",
        "--output-format", "stream-json",
        "--include-partial-messages",
        "--verbose",
        "--permission-mode", "acceptEdits",
        "--allowedTools", ALLOWED_TOOLS,
        "--add-dir", str(worktree),
        "--append-system-prompt", coder_skill_block,
        user_prompt,
    ]

    # Ensure shared tmux session + write the per-pane runner script
    print("Ensuring tmux session 'claude-admin' exists...")
    ensure_tmux_session()

    if tmux_window_exists(task_id):
        # Defensive: --force path should have killed it; bail if we got here without --force.
        die(f"tmux window '{TMUX_SESSION}:{task_id}' already exists; re-dispatch with --force", code=4)

    stderr_path = state_dir / "coder.stderr"
    stderr_path.touch()
    runner = write_coder_runner(state_dir, worktree, log_path, stderr_path, claude_argv)

    print(f"Spawning coder pane in tmux window '{TMUX_SESSION}:{task_id}'...")
    new_window = subprocess.run(
        ["tmux", "new-window", "-d", "-t", f"{TMUX_SESSION}:", "-n", task_id,
         "-c", str(worktree), f"bash {shell_quote(str(runner))}"],
        capture_output=True, text=True,
    )
    if new_window.returncode != 0:
        die(f"tmux new-window failed: {new_window.stderr.strip()}")

    # Best-effort: capture the pane's bash pid for diagnostics. The source of
    # truth for liveness is `tmux list-windows`, not the pid.
    coder_pid = tmux_pane_pid(task_id)

    # Spawn watcher
    print("Spawning watcher...")
    watcher_log = state_dir / "watcher.log"
    watcher_proc = subprocess.Popen(
        ["python3", str(WATCHER_PATH), "wait", plan_codename, task_id],
        stdout=open(watcher_log, "ab"),
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )

    # Update state.json with PIDs + tmux window
    initial_state["phase"] = "coding"
    initial_state["coder_pid"] = coder_pid
    initial_state["watcher_pid"] = watcher_proc.pid
    initial_state["tmux_session"] = TMUX_SESSION
    initial_state["tmux_window"] = task_id
    initial_state["last_event_at"] = started_at
    initial_state["system_prompt_source"] = system_prompt_source
    state_path.write_text(json.dumps(initial_state, indent=2))

    # Update milestones.json
    update_milestones(plan, task, milestone, worktree, branch, started_at, coder_pid, watcher_proc.pid)

    # Print summary
    print()
    print(f"✓ Dispatched {task_id} · {task['title']}")
    print(f"  worktree:      {worktree}")
    print(f"  branch:        {branch}")
    print(f"  tmux window:   {TMUX_SESSION}:{task_id}")
    print(f"  coder pid:     {coder_pid if coder_pid is not None else '(unknown — tmux owns it)'}")
    print(f"  watcher pid:   {watcher_proc.pid}")
    print(f"  prompt source: {system_prompt_source}")
    print()
    print(f"  watch live:  tmux attach -t {TMUX_SESSION} \\; select-window -t {task_id}")
    print(f"  log:         tail -f {log_path}")
    print(f"  state:       cat {state_path}")
    print(f"  status:      python3 {WATCHER_PATH} status {plan_codename} {task_id}")
    print()
    print(f"  to abort:    python3 {WATCHER_PATH} abort {plan_codename} {task_id}")
    return 0


def update_milestones(plan: dict, task: dict, milestone: dict,
                      worktree: Path, branch: str, started_at: str,
                      coder_pid: int | None, watcher_pid: int) -> None:
    src = Path(plan["milestones_source"])
    data = json.loads(src.read_text())
    for m in data["milestones"]:
        if m["id"] != milestone["id"]:
            continue
        m.setdefault("dispatches", {})
        m["dispatches"][task["id"]] = {
            "worktree": str(worktree),
            "branch": branch,
            "started_at": started_at,
            "coder_pid": coder_pid,
            "watcher_pid": watcher_pid,
            "phase": "coding",
        }
        break
    src.write_text(json.dumps(data, indent=2))


if __name__ == "__main__":
    sys.exit(main())
