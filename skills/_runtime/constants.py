from datetime import UTC, datetime  # noqa: F401  (re-exported for tests)
from pathlib import Path
from typing import Final, Mapping


# Default backend if config + env say nothing.
DEFAULT_BACKEND: Final[str] = "claude"

# Backends shipped out of the box. Other names resolve through config[backends].
KNOWN_BACKENDS: Final[frozenset[str]] = frozenset({"claude", "codex"})

# Env var whose value overrides every other backend-resolution input.
ENV_BACKEND_OVERRIDE: Final[str] = "CA_BACKEND"

# Env var pointing at an alternate runtime.toml (used by tests + CI).
ENV_CONFIG_PATH: Final[str] = "CA_RUNTIME_CONFIG"

# Env var pointing at the per-run state root (used by tests + CI).
ENV_RUN_ROOT: Final[str] = "CA_RUN_ROOT"

# Env var that overrides HEARTBEAT_INTERVAL_S (lets tests pick a sub-second tick).
ENV_HEARTBEAT_INTERVAL_S: Final[str] = "CA_HEARTBEAT_INTERVAL_S"


def default_config_path() -> Path:
    return Path("~/.config/claude-admin/runtime.toml").expanduser()


def default_run_root() -> Path:
    return Path("~/.work/runs").expanduser()


# Heartbeat cadence. Spec mandates mtime updates <= 30s; we touch every 10s
# so a missed tick still leaves slack before the SLA breaks.
HEARTBEAT_INTERVAL_S: Final[float] = 10.0

# If the transcript has produced no new bytes for this long, watcher emits
# a `stuck` event. Matches the v1 watcher.py value.
STUCK_AFTER_S: Final[float] = 600.0

# How often watcher polls the child + transcript while it's running.
WATCHER_POLL_S: Final[float] = 0.25

# `events(follow=True)` caps blocking at this many seconds to avoid orchestrator hangs
# when a child crashed without emitting EXIT or the watcher thread died.
EVENTS_DEFAULT_TIMEOUT_S: Final[float] = 24 * 60 * 60.0

# Max bytes for runtime.toml; defense against multi-GB / /dev/zero symlinks.
CONFIG_MAX_BYTES: Final[int] = 256 * 1024

# Substrings flagged as permission_blocked in transcript output.
PERMISSION_DENY_HINTS: Final[tuple[str, ...]] = (
    "permission_denied",
    "tool_use_error",
    "blocked by permission",
)

# Markers the fake-LLM fixture writes; real backends never emit these.
FAKE_HOOK_PREFIX: Final[str] = "HOOK_"

# Hook markers must terminate with this char to avoid prefix-collision
# (e.g., a future HOOK_PAUSE_RESUME being misclassified as HOOK_PAUSE).
FAKE_HOOK_TERMINATOR: Final[str] = ":"

# File modes for run state — 0700 dir + 0600 files keeps prompts/transcripts
# unreadable by other local users on a shared host.
RUN_DIR_MODE: Final[int] = 0o700
RUN_FILE_MODE: Final[int] = 0o600

# Safety flags appended to the built-in `claude` argv. Mirrors v1 dispatch.py's
# whitelist so the runtime cannot inadvertently broaden the blast radius.
CLAUDE_SAFETY_ARGS: Final[tuple[str, ...]] = (
    "--permission-mode",
    "acceptEdits",
    "--strict-mcp-config",
    "--disallowedTools",
    "mcp__*",
)

# Codex's equivalent: ask-for-approval on tool use, workspace-only sandbox.
CODEX_SAFETY_ARGS: Final[tuple[str, ...]] = (
    "--ask-for-approval",
    "on-request",
    "--sandbox",
    "workspace-write",
)


def fake_hook_markers() -> Mapping[str, str]:
    """The 4 mid-run hook markers the test fixture emits; keys match the event kinds."""
    return {
        f"{FAKE_HOOK_PREFIX}QUESTION": "question",
        f"{FAKE_HOOK_PREFIX}PAUSE": "pause",
        f"{FAKE_HOOK_PREFIX}STUCK": "stuck",
        f"{FAKE_HOOK_PREFIX}PERMISSION_BLOCKED": "permission_blocked",
    }
