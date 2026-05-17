from pathlib import Path
from types import MappingProxyType

import pytest

from skills._runtime import backend
from skills._runtime.backend import ConfigError
from skills._runtime.constants import CLAUDE_SAFETY_ARGS, CODEX_SAFETY_ARGS
from skills._runtime.types import BackendConfig


def test_resolve_defaults_to_claude_when_no_config(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setenv("CA_RUNTIME_CONFIG", str(tmp_path / "missing.toml"))
    monkeypatch.delenv("CA_BACKEND", raising=False)
    assert backend.resolve() == "claude"


def test_resolve_env_override_beats_everything(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CA_BACKEND", "codex")
    cfg = BackendConfig(
        default_backend="claude",
        skills=MappingProxyType({"coder": "claude"}),
    )
    assert backend.resolve(skill="coder", config=cfg) == "codex"


def test_resolve_env_override_must_be_known(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CA_BACKEND", "bogus")
    cfg = BackendConfig(default_backend="claude")
    with pytest.raises(ConfigError, match="bogus"):
        backend.resolve(config=cfg)


def test_resolve_env_override_accepts_custom_backend(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CA_BACKEND", "fake")
    cfg = BackendConfig(
        default_backend="claude",
        custom_argv=MappingProxyType({"fake": ("python3", "-")}),
    )
    assert backend.resolve(config=cfg) == "fake"


def test_resolve_skill_override_beats_default(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("CA_BACKEND", raising=False)
    cfg = BackendConfig(
        default_backend="claude",
        skills=MappingProxyType({"reviewer": "codex"}),
    )
    assert backend.resolve(skill="reviewer", config=cfg) == "codex"
    assert backend.resolve(skill="coder", config=cfg) == "claude"
    assert backend.resolve(config=cfg) == "claude"


def test_load_config_parses_toml(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config = tmp_path / "runtime.toml"
    config.write_text(
        """
default_backend = "codex"

[skills]
coder = "claude"

[backends.local-claude]
argv = ["claude", "--model", "opus", "{prompt}"]
""",
        encoding="utf-8",
    )
    monkeypatch.setenv("CA_RUNTIME_CONFIG", str(config))
    cfg = backend.load_config()
    assert cfg.default_backend == "codex"
    assert dict(cfg.skills) == {"coder": "claude"}
    assert dict(cfg.custom_argv) == {
        "local-claude": ("claude", "--model", "opus", "{prompt}"),
    }


def test_load_config_rejects_oversized_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config = tmp_path / "runtime.toml"
    config.write_text("# " + "x" * (300 * 1024), encoding="utf-8")
    monkeypatch.setenv("CA_RUNTIME_CONFIG", str(config))
    with pytest.raises(ConfigError, match="exceeds"):
        backend.load_config()


def test_load_config_rejects_non_table_skills(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config = tmp_path / "runtime.toml"
    config.write_text('skills = "claude"\n', encoding="utf-8")
    monkeypatch.setenv("CA_RUNTIME_CONFIG", str(config))
    with pytest.raises(ConfigError, match=r"\[skills\] must be a table"):
        backend.load_config()


def test_load_config_rejects_malformed_toml(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config = tmp_path / "runtime.toml"
    config.write_text("this is = = not = valid", encoding="utf-8")
    monkeypatch.setenv("CA_RUNTIME_CONFIG", str(config))
    with pytest.raises(ConfigError):
        backend.load_config()


def test_argv_for_claude_uses_stdin_and_safety_flags(tmp_path: Path) -> None:
    prompt = tmp_path / "p.md"
    prompt.write_text("hi there", encoding="utf-8")
    worktree = tmp_path / "wt"
    worktree.mkdir()
    cfg = BackendConfig(default_backend="claude")
    argv = backend.argv_for(backend="claude", prompt_path=prompt, worktree=worktree, config=cfg)
    assert argv[:2] == ["claude", "-p"]
    assert "hi there" not in argv, "prompt body must not appear in argv (flag-injection guard)"
    assert str(worktree) in argv
    for flag in CLAUDE_SAFETY_ARGS:
        assert flag in argv
    assert backend.feeds_prompt_via_stdin(backend="claude", config=cfg) is True


def test_argv_for_codex_uses_exec_subcommand_with_safety_flags(tmp_path: Path) -> None:
    prompt = tmp_path / "p.md"
    prompt.write_text("do thing", encoding="utf-8")
    worktree = tmp_path / "wt"
    worktree.mkdir()
    cfg = BackendConfig(default_backend="claude")
    argv = backend.argv_for(backend="codex", prompt_path=prompt, worktree=worktree, config=cfg)
    assert argv[0:2] == ["codex", "exec"]
    assert str(worktree) in argv
    assert "do thing" not in argv
    for flag in CODEX_SAFETY_ARGS:
        assert flag in argv


def test_argv_for_custom_substitutes_placeholders_with_path(tmp_path: Path) -> None:
    prompt = tmp_path / "p.md"
    prompt.write_text("x", encoding="utf-8")
    worktree = tmp_path / "wt"
    worktree.mkdir()
    cfg = BackendConfig(
        default_backend="claude",
        custom_argv=MappingProxyType({"fake": ("python3", "{prompt}", "--cwd", "{worktree}")}),
    )
    argv = backend.argv_for(backend="fake", prompt_path=prompt, worktree=worktree, config=cfg)
    assert argv == ["python3", str(prompt), "--cwd", str(worktree)]
    # Custom backends opt out of stdin feeding; their template references {prompt} as a path.
    assert backend.feeds_prompt_via_stdin(backend="fake", config=cfg) is False


def test_argv_for_unknown_backend_raises(tmp_path: Path) -> None:
    prompt = tmp_path / "p.md"
    prompt.write_text("x", encoding="utf-8")
    worktree = tmp_path / "wt"
    worktree.mkdir()
    cfg = BackendConfig(default_backend="claude")
    with pytest.raises(ConfigError, match="unknown backend"):
        backend.argv_for(backend="bogus", prompt_path=prompt, worktree=worktree, config=cfg)
