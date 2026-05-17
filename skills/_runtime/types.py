from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path
from types import MappingProxyType
from typing import Any, Mapping


type EventEnvelope = dict[str, Any]


class EventKind(StrEnum):
    """The 6 hook events spec'd in PRD #16 V3."""

    START = "start"
    QUESTION = "question"
    PAUSE = "pause"
    EXIT = "exit"
    STUCK = "stuck"
    PERMISSION_BLOCKED = "permission_blocked"


@dataclass(frozen=True, slots=True)
class RunPaths:
    """Filesystem layout for a single run; computed once, threaded through helpers."""

    run_id: str
    state_dir: Path
    events_path: Path
    transcript_path: Path
    heartbeat_path: Path
    meta_path: Path
    prompt_path: Path


@dataclass(frozen=True, slots=True)
class BackendConfig:
    """Resolved runtime.toml view; nested mappings are read-only to keep the value-object honest."""

    default_backend: str
    skills: Mapping[str, str] = field(default_factory=lambda: MappingProxyType({}))
    custom_argv: Mapping[str, tuple[str, ...]] = field(
        default_factory=lambda: MappingProxyType({})
    )
