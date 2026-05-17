import json
import os
import sys
import time
from pathlib import Path

import pytest

from skills._runtime import tmux_runtime
from skills._runtime.constants import HEARTBEAT_INTERVAL_S
from skills._runtime.types import EventEnvelope, EventKind


def _collect(*, run_id: str, max_wait_s: float = 15.0) -> list[EventEnvelope]:
    """Drain events() until an exit event arrives or the timeout fires."""
    out: list[EventEnvelope] = []
    for ev in tmux_runtime.events(
        run_id=run_id, follow=True, timeout=max_wait_s, poll_interval=0.02
    ):
        out.append(ev)
        if ev["kind"] == EventKind.EXIT.value:
            break
    return out


def _run_dir_for(*, run_id: str) -> Path:
    return Path(os.environ["CA_RUN_ROOT"]) / run_id


@pytest.mark.usefixtures("fake_backend_config", "tmp_run_root")
def test_v3_all_six_hooks_in_order(tmp_worktree: Path) -> None:
    """V3 acceptance: all 6 hooks land in events.jsonl in the spec'd order."""
    run_id = tmux_runtime.spawn(
        session="claude-admin",
        window="coder-test",
        repo=tmp_worktree,
        worktree=tmp_worktree,
        backend="fake",
        prompt="placeholder prompt",
    )
    events = _collect(run_id=run_id)
    kinds = [e["kind"] for e in events]
    expected_order = [
        EventKind.START.value,
        EventKind.QUESTION.value,
        EventKind.PAUSE.value,
        EventKind.STUCK.value,
        EventKind.PERMISSION_BLOCKED.value,
        EventKind.EXIT.value,
    ]
    assert kinds == expected_order, f"actual order: {kinds}"

    seqs = [e["seq"] for e in events]
    assert seqs == sorted(seqs) and seqs[0] == 1, f"seqs broken: {seqs}"

    start = events[0]
    assert start["payload"]["backend"] == "fake"
    assert isinstance(start["payload"]["pid"], int)

    perm = events[expected_order.index(EventKind.PERMISSION_BLOCKED.value)]
    assert "rm -rf" in perm["payload"]["line"]

    exit_ev = events[-1]
    assert exit_ev["payload"]["code"] == 0


@pytest.mark.usefixtures("fake_backend_config", "tmp_run_root")
def test_heartbeat_mtime_within_30s(tmp_worktree: Path) -> None:
    """V3 heartbeat half: mtime updates while the run is active."""
    run_id = tmux_runtime.spawn(
        session="claude-admin",
        window="coder-hb",
        repo=tmp_worktree,
        worktree=tmp_worktree,
        backend="fake",
        prompt="hb",
    )
    hb_path = _run_dir_for(run_id=run_id) / "heartbeat"
    deadline = time.monotonic() + max(HEARTBEAT_INTERVAL_S * 3, 30.0)
    first = hb_path.stat().st_mtime
    updated = False
    while time.monotonic() < deadline:
        if hb_path.stat().st_mtime > first:
            updated = True
            break
        time.sleep(0.05)
    assert updated, "heartbeat mtime did not advance within the SLA window"
    assert (time.time() - hb_path.stat().st_mtime) <= 30.0
    _collect(run_id=run_id)


@pytest.mark.usefixtures("fake_backend_config", "tmp_run_root")
def test_meta_json_records_tuple_and_pgid(tmp_worktree: Path) -> None:
    """spawn() persists session/window/repo/worktree/backend/pid/pgid so consumers can recover state."""
    run_id = tmux_runtime.spawn(
        session="claude-admin",
        window="coder-meta",
        repo=tmp_worktree,
        worktree=tmp_worktree,
        backend="fake",
        prompt="meta",
    )
    meta = json.loads((_run_dir_for(run_id=run_id) / "meta.json").read_text())
    assert meta["session"] == "claude-admin"
    assert meta["window"] == "coder-meta"
    assert meta["backend"] == "fake"
    assert meta["worktree"] == str(tmp_worktree)
    assert meta["v0_headless"] is True
    assert isinstance(meta["pgid"], int) and meta["pgid"] > 0
    _collect(run_id=run_id)


@pytest.mark.usefixtures("fake_backend_config", "tmp_run_root")
def test_state_dir_and_files_are_0700_0600(tmp_worktree: Path) -> None:
    """Sensitive run state stays unreadable by other local users."""
    run_id = tmux_runtime.spawn(
        session="s",
        window="w",
        repo=tmp_worktree,
        worktree=tmp_worktree,
        backend="fake",
        prompt="perms",
    )
    state_dir = _run_dir_for(run_id=run_id)
    assert (state_dir.stat().st_mode & 0o777) == 0o700
    for name in ("events.jsonl", "transcript.log", "heartbeat", "meta.json", "prompt.md"):
        path = state_dir / name
        mode = path.stat().st_mode & 0o777
        assert mode == 0o600, f"{name} mode is {oct(mode)}, expected 0o600"
    _collect(run_id=run_id)


@pytest.mark.usefixtures("fake_backend_config", "tmp_run_root")
def test_spawn_rejects_dash_prefixed_prompt(tmp_worktree: Path) -> None:
    """CLI-flag injection guard: prompts beginning with '-' are refused."""
    with pytest.raises(ValueError, match="CLI-flag injection"):
        tmux_runtime.spawn(
            session="s",
            window="w",
            repo=tmp_worktree,
            worktree=tmp_worktree,
            backend="fake",
            prompt="--dangerously-skip-permissions",
        )


@pytest.mark.usefixtures("fake_backend_config", "tmp_run_root")
def test_kill_terminates_a_long_running_child(
    tmp_worktree: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """kill() must SIGTERM the captured pgid so children spawned by the backend also die."""
    long_script = tmp_worktree / "long_sleep.py"
    long_script.write_text("import time; time.sleep(30)\n", encoding="utf-8")
    config_path = Path(os.environ["CA_RUNTIME_CONFIG"])
    config_path.write_text(
        f"""
default_backend = "fake"
[backends.fake]
argv = ["{sys.executable}", "{long_script}"]
""",
        encoding="utf-8",
    )
    run_id = tmux_runtime.spawn(
        session="s",
        window="w",
        repo=tmp_worktree,
        worktree=tmp_worktree,
        backend="fake",
        prompt="x",
    )
    meta = json.loads((_run_dir_for(run_id=run_id) / "meta.json").read_text())
    pid = int(meta["pid"])
    tmux_runtime.kill(run_id=run_id, grace_s=2.0)
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline:
        if not tmux_runtime._pid_alive(pid=pid):
            break
        time.sleep(0.05)
    assert not tmux_runtime._pid_alive(pid=pid), "child still alive after kill()"


@pytest.mark.usefixtures("fake_backend_config", "tmp_run_root")
def test_events_follow_false_returns_immediately(tmp_worktree: Path) -> None:
    """follow=False just drains what's on disk and stops, even with no exit event yet."""
    run_id = tmux_runtime.spawn(
        session="s",
        window="w",
        repo=tmp_worktree,
        worktree=tmp_worktree,
        backend="fake",
        prompt="x",
    )
    time.sleep(0.1)
    snapshot = list(tmux_runtime.events(run_id=run_id, follow=False))
    assert any(e["kind"] == EventKind.START.value for e in snapshot)
    _collect(run_id=run_id)


def test_events_for_unknown_run_id_raises() -> None:
    with pytest.raises(FileNotFoundError):
        next(tmux_runtime.events(run_id="deadbeef" * 4, follow=False))


@pytest.mark.usefixtures("fake_backend_config", "tmp_run_root")
def test_seq_counters_cleared_after_exit(tmp_worktree: Path) -> None:
    """_SEQ_COUNTERS must drop the run after EXIT so long-lived hosts don't leak memory."""
    run_id = tmux_runtime.spawn(
        session="s",
        window="w",
        repo=tmp_worktree,
        worktree=tmp_worktree,
        backend="fake",
        prompt="seq",
    )
    _collect(run_id=run_id)
    # Give the watcher a beat to pop from the counter after emitting EXIT.
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline:
        if run_id not in tmux_runtime._SEQ_COUNTERS:
            break
        time.sleep(0.05)
    assert run_id not in tmux_runtime._SEQ_COUNTERS
