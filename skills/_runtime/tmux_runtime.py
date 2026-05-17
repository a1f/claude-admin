"""Spawn LLM backends, capture their hook lifecycle, expose a heartbeat.

v0 — HEADLESS. The acceptance criteria allowed "headless + file-state only" if
tmux observability got stuck. We took that path: the runtime spawns a plain
subprocess inside the worktree and tails its stdout/stderr to a transcript.
The `session`/`window` args are accepted (and recorded in meta.json) so the
public surface is identical to a future v1 that does push the run into a tmux
window via `tmux new-window -d` + `tmux pipe-pane`.

The 6 hook events spec'd in PRD #16 V3 are emitted as JSON envelopes appended
to `<state_dir>/events.jsonl`:

    {"run_id": "...", "ts": "2026-05-16T12:00:00Z", "seq": 1,
     "kind": "start", "payload": {"backend": "claude", "pid": 12345}}
"""

import codecs
import json
import os
import signal
import subprocess
import sys
import threading
import time
import uuid
from collections.abc import Iterator
from datetime import UTC, datetime
from pathlib import Path
from typing import BinaryIO

from .backend import argv_for, feeds_prompt_via_stdin, load_config
from .constants import (
    ENV_HEARTBEAT_INTERVAL_S,
    ENV_RUN_ROOT,
    EVENTS_DEFAULT_TIMEOUT_S,
    FAKE_HOOK_TERMINATOR,
    HEARTBEAT_INTERVAL_S,
    PERMISSION_DENY_HINTS,
    RUN_DIR_MODE,
    RUN_FILE_MODE,
    STUCK_AFTER_S,
    WATCHER_POLL_S,
    default_run_root,
    fake_hook_markers,
)
from .types import EventEnvelope, EventKind, RunPaths

# Module-level locks/state. Keyed by run_id; cleaned up on EXIT to bound memory.
_STATE_LOCK = threading.Lock()
_SEQ_COUNTERS: dict[str, int] = {}
_STOP_EVENTS: dict[str, threading.Event] = {}

# Map fake-LLM marker tokens (prefix only, terminator added at match time) to kinds.
_FAKE_HOOK_MARKERS: dict[str, EventKind] = {
    marker: EventKind(kind) for marker, kind in fake_hook_markers().items()
}


def _run_root() -> Path:
    override = os.environ.get(ENV_RUN_ROOT)
    return Path(override) if override else default_run_root()


def _heartbeat_interval() -> float:
    override = os.environ.get(ENV_HEARTBEAT_INTERVAL_S)
    if override is None:
        return HEARTBEAT_INTERVAL_S
    try:
        value = float(override)
    except ValueError as exc:
        raise ValueError(f"{ENV_HEARTBEAT_INTERVAL_S}={override!r} is not a float") from exc
    if value <= 0:
        raise ValueError(f"{ENV_HEARTBEAT_INTERVAL_S} must be > 0; got {value}")
    return value


def _paths_for(*, run_id: str) -> RunPaths:
    state_dir = _run_root() / run_id
    return RunPaths(
        run_id=run_id,
        state_dir=state_dir,
        events_path=state_dir / "events.jsonl",
        transcript_path=state_dir / "transcript.log",
        heartbeat_path=state_dir / "heartbeat",
        meta_path=state_dir / "meta.json",
        prompt_path=state_dir / "prompt.md",
    )


def _now_iso() -> str:
    return datetime.now(UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def _create_file_0600(path: Path) -> None:
    """Create an empty file with 0600 perms, failing if it already exists (symlink guard)."""
    fd = os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, RUN_FILE_MODE)
    os.close(fd)


def _emit(*, paths: RunPaths, kind: EventKind, payload: dict | None = None) -> None:
    """Append one event envelope to events.jsonl atomically (one line == one event).

    The whole write happens under _STATE_LOCK so seq order matches file order
    even when emit is called from heartbeat / watcher threads simultaneously.
    """
    with _STATE_LOCK:
        seq = _SEQ_COUNTERS.get(paths.run_id, 0) + 1
        _SEQ_COUNTERS[paths.run_id] = seq
        event: EventEnvelope = {
            "run_id": paths.run_id,
            "ts": _now_iso(),
            "seq": seq,
            "kind": kind.value,
            "payload": payload or {},
        }
        line = json.dumps(event, separators=(",", ":")) + "\n"
        with paths.events_path.open("a", encoding="utf-8") as f:
            f.write(line)
        if kind is EventKind.EXIT:
            _SEQ_COUNTERS.pop(paths.run_id, None)


def _prompt_safe_for_argv(*, prompt: str) -> bool:
    """Reject prompts that start with '-' — they could otherwise be parsed as CLI flags by custom backends."""
    return not prompt.lstrip().startswith("-")


def spawn(
    *,
    session: str,
    window: str,
    repo: Path,
    worktree: Path,
    backend: str,
    prompt: str,
) -> str:
    """Start a backend LLM run in `worktree`; return a UUIDv4 hex run_id.

    v0 headless: the child is a normal subprocess, not a tmux window. The
    function returns immediately; the child runs in the background. Built-in
    `claude`/`codex` backends receive the prompt over stdin so its content
    can never be parsed as a CLI flag. Custom backends use `{prompt}` as a
    PATH placeholder (never inlined as a body) for the same reason.
    """
    if not worktree.exists():
        raise FileNotFoundError(f"worktree does not exist: {worktree}")
    if not _prompt_safe_for_argv(prompt=prompt):
        raise ValueError("prompt may not start with '-' (CLI-flag injection guard)")

    cfg = load_config()
    run_id = uuid.uuid4().hex
    paths = _paths_for(run_id=run_id)
    root = paths.state_dir.parent
    root.mkdir(parents=True, exist_ok=True, mode=RUN_DIR_MODE)
    # exist_ok=False rejects an attacker-pre-created symlinked dir.
    paths.state_dir.mkdir(mode=RUN_DIR_MODE, exist_ok=False)
    for p in (paths.events_path, paths.transcript_path, paths.heartbeat_path, paths.meta_path, paths.prompt_path):
        _create_file_0600(p)
    paths.prompt_path.write_text(prompt, encoding="utf-8")

    argv = argv_for(backend=backend, prompt_path=paths.prompt_path, worktree=worktree, config=cfg)
    use_stdin = feeds_prompt_via_stdin(backend=backend, config=cfg)

    transcript: BinaryIO = paths.transcript_path.open("ab", buffering=0)
    stdin_handle: BinaryIO | None = paths.prompt_path.open("rb") if use_stdin else None
    try:
        proc: subprocess.Popen[bytes] = subprocess.Popen(
            argv,
            cwd=str(worktree),
            stdout=transcript,
            stderr=subprocess.STDOUT,
            stdin=stdin_handle if stdin_handle is not None else subprocess.DEVNULL,
            start_new_session=True,
        )
        pgid = os.getpgid(proc.pid)
    except Exception:
        transcript.close()
        # No mkdir cleanup — leaving the state dir lets the user inspect what went wrong.
        raise
    finally:
        # Popen dups the stdin fd; close our copy so the parent doesn't hold it open.
        if stdin_handle is not None:
            stdin_handle.close()

    meta = {
        "run_id": run_id,
        "session": session,
        "window": window,
        "repo": str(repo),
        "worktree": str(worktree),
        "backend": backend,
        "argv": argv,
        "pid": proc.pid,
        "pgid": pgid,
        "started_at": _now_iso(),
        "v0_headless": True,
    }
    paths.meta_path.write_text(json.dumps(meta, indent=2), encoding="utf-8")

    _emit(paths=paths, kind=EventKind.START, payload={"backend": backend, "pid": proc.pid})

    stop = threading.Event()
    with _STATE_LOCK:
        _STOP_EVENTS[run_id] = stop
    threading.Thread(
        target=_heartbeat_loop,
        args=(paths, proc, stop),
        daemon=True,
        name=f"hb-{run_id[:8]}",
    ).start()
    threading.Thread(
        target=_watcher_loop,
        args=(paths, proc, transcript, stop),
        daemon=True,
        name=f"wt-{run_id[:8]}",
    ).start()
    return run_id


def _heartbeat_loop(paths: RunPaths, proc: subprocess.Popen[bytes], stop: threading.Event) -> None:
    """Touch heartbeat every HEARTBEAT_INTERVAL_S while the child is alive."""
    interval = _heartbeat_interval()
    while not stop.is_set() and proc.poll() is None:
        try:
            paths.heartbeat_path.touch()
        except OSError:
            return
        stop.wait(interval)


def _watcher_loop(
    paths: RunPaths,
    proc: subprocess.Popen[bytes],
    transcript_handle: BinaryIO,
    stop: threading.Event,
) -> None:
    """Tail transcript for hook markers + permission-denial hints + idle detection."""
    pos = 0
    pending = ""
    last_byte_ts = time.monotonic()
    stuck_emitted = False
    decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
    try:
        while not stop.is_set():
            alive = proc.poll() is None
            new = _read_new(path=paths.transcript_path, pos=pos)
            if new:
                pos += len(new)
                last_byte_ts = time.monotonic()
                stuck_emitted = False
                chunk = pending + decoder.decode(new)
                lines = chunk.split("\n")
                pending = lines[-1]
                for raw in lines[:-1]:
                    _scan_line(paths=paths, line=raw)
            elif alive and not stuck_emitted and (time.monotonic() - last_byte_ts) >= STUCK_AFTER_S:
                _emit(
                    paths=paths,
                    kind=EventKind.STUCK,
                    payload={"idle_s": round(time.monotonic() - last_byte_ts, 1)},
                )
                stuck_emitted = True
            if not alive:
                # Set stop FIRST so the heartbeat thread wakes immediately.
                stop.set()
                tail = decoder.decode(b"", final=True)
                if tail:
                    pending += tail
                if pending:
                    _scan_line(paths=paths, line=pending)
                    pending = ""
                rc = proc.returncode if proc.returncode is not None else proc.wait()
                _emit(paths=paths, kind=EventKind.EXIT, payload={"code": int(rc)})
                with _STATE_LOCK:
                    _STOP_EVENTS.pop(paths.run_id, None)
                return
            time.sleep(WATCHER_POLL_S)
    finally:
        try:
            transcript_handle.close()
        except OSError:
            pass


def _read_new(*, path: Path, pos: int) -> bytes:
    try:
        with path.open("rb") as f:
            f.seek(pos)
            return f.read()
    except FileNotFoundError:
        return b""


def _scan_line(*, paths: RunPaths, line: str) -> None:
    """Translate one transcript line into a hook event if it matches.

    Markers must end with FAKE_HOOK_TERMINATOR or appear on a bare line so
    future markers (HOOK_PAUSE_RESUME) can't collide with shorter prefixes.
    """
    stripped = line.strip()
    if not stripped:
        return
    for marker, kind in _FAKE_HOOK_MARKERS.items():
        matches_terminator = stripped.startswith(marker + FAKE_HOOK_TERMINATOR)
        matches_bare = stripped == marker
        if matches_terminator or matches_bare:
            payload_str = stripped[len(marker):].lstrip(FAKE_HOOK_TERMINATOR).strip()
            key = "line" if kind is EventKind.PERMISSION_BLOCKED else "text"
            payload = {key: payload_str[:500]} if payload_str else {}
            _emit(paths=paths, kind=kind, payload=payload)
            return
    lower = stripped.lower()
    if any(h in lower for h in PERMISSION_DENY_HINTS):
        _emit(paths=paths, kind=EventKind.PERMISSION_BLOCKED, payload={"line": stripped[:500]})


def events(
    *,
    run_id: str,
    follow: bool = True,
    timeout: float | None = None,
    poll_interval: float = 0.05,
) -> Iterator[EventEnvelope]:
    """Yield event envelopes for `run_id` in order, tailing events.jsonl.

    follow=True (default): keep tailing past the file's current end until an
        `exit` event is read, `timeout` seconds elapse, or EVENTS_DEFAULT_TIMEOUT_S
        if `timeout` is None.
    follow=False: yield only what's currently on disk, then stop.
    """
    paths = _paths_for(run_id=run_id)
    if not paths.state_dir.exists():
        raise FileNotFoundError(f"unknown run_id: {run_id}")
    effective_timeout = timeout if timeout is not None else (EVENTS_DEFAULT_TIMEOUT_S if follow else 0.0)
    deadline = time.monotonic() + effective_timeout if follow else None
    pos = 0
    buf = ""
    decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
    while True:
        try:
            with paths.events_path.open("rb") as f:
                f.seek(pos)
                chunk = f.read()
        except FileNotFoundError:
            chunk = b""
        if chunk:
            pos += len(chunk)
            buf += decoder.decode(chunk)
            while "\n" in buf:
                line, buf = buf.split("\n", 1)
                line = line.strip()
                if not line:
                    continue
                try:
                    ev = json.loads(line)
                except json.JSONDecodeError:
                    continue
                yield ev
                if ev.get("kind") == EventKind.EXIT.value:
                    return
        if not follow:
            return
        if deadline is not None and time.monotonic() >= deadline:
            return
        time.sleep(poll_interval)


def kill(*, run_id: str, grace_s: float = 5.0) -> None:
    """SIGTERM the run's child process group (using the pgid captured at spawn); SIGKILL after `grace_s`."""
    paths = _paths_for(run_id=run_id)
    if not paths.meta_path.exists():
        raise FileNotFoundError(f"unknown run_id: {run_id}")
    meta = json.loads(paths.meta_path.read_text(encoding="utf-8"))
    pid = int(meta["pid"])
    pgid = int(meta.get("pgid", pid))
    if not _pid_alive(pid=pid):
        return
    try:
        os.killpg(pgid, signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        return
    deadline = time.monotonic() + grace_s
    while time.monotonic() < deadline:
        if not _pid_alive(pid=pid):
            return
        time.sleep(0.1)
    try:
        os.killpg(pgid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        return


def _pid_alive(*, pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except (ProcessLookupError, PermissionError):
        return False


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: python -m skills._runtime.tmux_runtime <run_id>", file=sys.stderr)
        sys.exit(2)
    for ev in events(run_id=sys.argv[1]):
        print(json.dumps(ev))
