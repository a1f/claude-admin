import os
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest

# Ensure `import skills._runtime...` works when pytest is invoked from the repo root.
_REPO_ROOT = Path(__file__).resolve().parents[3]
if str(_REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(_REPO_ROOT))


@pytest.fixture
def tmp_run_root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Point CA_RUN_ROOT at a fresh tmp dir for the duration of the test."""
    root: Path = tmp_path / "runs"
    root.mkdir(mode=0o700)
    monkeypatch.setenv("CA_RUN_ROOT", str(root))
    return root


@pytest.fixture
def tmp_worktree(tmp_path: Path) -> Path:
    wt: Path = tmp_path / "worktree"
    wt.mkdir()
    return wt


@pytest.fixture
def fake_backend_config(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Write a runtime.toml that registers `fake` -> the fake_llm.py fixture.

    Inter-marker sleep is 0.5s so total run-time (~2s) outlives the 0.2s heartbeat
    tick we pin below, giving the heartbeat test room to observe mtime advance.
    """
    fake_script: Path = Path(__file__).resolve().parent / "fake_llm.py"
    config_path: Path = tmp_path / "runtime.toml"
    config_path.write_text(
        f"""
default_backend = "fake"

[skills]
coder = "fake"
reviewer = "codex"

[backends.fake]
argv = ["{sys.executable}", "{fake_script}", "--sleep", "0.5"]
""",
        encoding="utf-8",
    )
    monkeypatch.setenv("CA_RUNTIME_CONFIG", str(config_path))
    monkeypatch.delenv("CA_BACKEND", raising=False)
    monkeypatch.setenv("CA_HEARTBEAT_INTERVAL_S", "0.2")
    return config_path


@pytest.fixture(autouse=True)
def _clean_env(monkeypatch: pytest.MonkeyPatch) -> Iterator[None]:
    """Stop a developer's real CA_BACKEND from bleeding into tests."""
    if "CA_BACKEND" in os.environ:
        monkeypatch.delenv("CA_BACKEND", raising=False)
    yield
