"""Backend router: pick claude vs codex (or a custom backend) and build its argv.

Precedence for `resolve(skill=...)`:
    1. env CA_BACKEND (always wins; used by tests + manual override)
    2. config.skills[<skill>] (per-skill pin)
    3. config.default_backend
    4. constants.DEFAULT_BACKEND ('claude')

Both env override and custom argv from config are validated against the union of
KNOWN_BACKENDS + config.custom_argv so a typo or a hostile env var cannot resolve
to an unknown name.
"""

import os
import tomllib
from pathlib import Path
from types import MappingProxyType
from typing import Mapping

from .constants import (
    CLAUDE_SAFETY_ARGS,
    CODEX_SAFETY_ARGS,
    CONFIG_MAX_BYTES,
    DEFAULT_BACKEND,
    ENV_BACKEND_OVERRIDE,
    ENV_CONFIG_PATH,
    KNOWN_BACKENDS,
    default_config_path,
)
from .types import BackendConfig


class ConfigError(ValueError):
    """runtime.toml is present but malformed (wrong types, oversized, etc.)."""


def _config_path() -> Path:
    """Honor CA_RUNTIME_CONFIG so tests + CI don't touch a user's real config."""
    override = os.environ.get(ENV_CONFIG_PATH)
    if override:
        return Path(override)
    return default_config_path()


def load_config() -> BackendConfig:
    """Parse runtime.toml, returning a BackendConfig with safe defaults if missing.

    A missing file is not an error — fresh installs work with built-in defaults.
    Malformed schema or oversized file raises ConfigError so the user sees it
    instead of a deep stdlib traceback.
    """
    path = _config_path()
    if not path.exists():
        return BackendConfig(default_backend=DEFAULT_BACKEND)
    size = path.stat().st_size
    if size > CONFIG_MAX_BYTES:
        raise ConfigError(f"{path}: {size} bytes exceeds {CONFIG_MAX_BYTES} cap")
    try:
        raw = tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        raise ConfigError(f"{path}: {exc}") from exc
    default = str(raw.get("default_backend", DEFAULT_BACKEND))
    skills_raw = raw.get("skills", {})
    if not isinstance(skills_raw, dict):
        raise ConfigError(f"{path}: [skills] must be a table, got {type(skills_raw).__name__}")
    skills: dict[str, str] = {}
    for k, v in skills_raw.items():
        if not isinstance(v, str):
            raise ConfigError(f"{path}: skills.{k} must be a string, got {type(v).__name__}")
        skills[str(k)] = v
    backends_raw = raw.get("backends", {})
    if not isinstance(backends_raw, dict):
        raise ConfigError(f"{path}: [backends] must be a table, got {type(backends_raw).__name__}")
    custom: dict[str, tuple[str, ...]] = {}
    for name, body in backends_raw.items():
        if not isinstance(body, dict):
            raise ConfigError(f"{path}: [backends.{name}] must be a table")
        argv = body.get("argv")
        if not (isinstance(argv, list) and all(isinstance(x, str) for x in argv)):
            raise ConfigError(f"{path}: [backends.{name}].argv must be a list of strings")
        custom[str(name)] = tuple(argv)
    return BackendConfig(
        default_backend=default,
        skills=MappingProxyType(skills),
        custom_argv=MappingProxyType(custom),
    )


def resolve(*, skill: str | None = None, config: BackendConfig | None = None) -> str:
    """Return the backend name to use for `skill`; env override always wins, but it's allow-listed."""
    cfg = config if config is not None else load_config()
    allowed = KNOWN_BACKENDS | set(cfg.custom_argv.keys())
    env = os.environ.get(ENV_BACKEND_OVERRIDE)
    if env:
        if env not in allowed:
            raise ConfigError(
                f"{ENV_BACKEND_OVERRIDE}={env!r} is not known; allowed: {sorted(allowed)}"
            )
        return env
    if skill and skill in cfg.skills:
        return cfg.skills[skill]
    return cfg.default_backend


def argv_for(
    *,
    backend: str,
    prompt_path: Path,
    worktree: Path,
    config: BackendConfig | None = None,
) -> list[str]:
    """Return argv for `backend`; built-in backends read the prompt from stdin so a prompt cannot inject CLI flags.

    Custom entries in config[backends] win over built-ins so users can pin a
    specific claude path or override codex flags without code changes.
    The `{prompt}` placeholder in custom argv substitutes the prompt FILE PATH,
    never the body — keeps untrusted prompt text off the command line.
    """
    cfg = config if config is not None else load_config()
    if backend in cfg.custom_argv:
        return _substitute(cfg.custom_argv[backend], prompt_path=prompt_path, worktree=worktree)
    if backend == "claude":
        return ["claude", "-p", "--add-dir", str(worktree), *CLAUDE_SAFETY_ARGS]
    if backend == "codex":
        return ["codex", "exec", "--cd", str(worktree), *CODEX_SAFETY_ARGS]
    raise ConfigError(
        f"unknown backend {backend!r}; known: {sorted(KNOWN_BACKENDS)} "
        f"+ custom: {sorted(cfg.custom_argv)}"
    )


def feeds_prompt_via_stdin(*, backend: str, config: BackendConfig | None = None) -> bool:
    """Built-in backends consume the prompt over stdin; custom backends embed `{prompt}` as a path."""
    cfg = config if config is not None else load_config()
    return backend not in cfg.custom_argv


def _substitute(
    argv: tuple[str, ...] | list[str], *, prompt_path: Path, worktree: Path
) -> list[str]:
    """Replace {prompt} (=path) and {worktree} placeholders inside a custom argv template."""
    return [
        arg.replace("{prompt}", str(prompt_path)).replace("{worktree}", str(worktree))
        for arg in argv
    ]
