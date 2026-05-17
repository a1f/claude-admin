import os
import time
from pathlib import Path

from grill_docs import build_freshness, inspect_context


def _write(repo: Path, rel: str, content: str = "") -> Path:
    target = repo / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")
    return target


class TestInspectContext:
    def test_missing_context_reported_as_not_existing(
        self, tmp_path: Path
    ) -> None:
        report = inspect_context(
            context_path=tmp_path / "CONTEXT.md", repo_root=tmp_path
        )
        assert report.exists is False

    def test_detects_missing_file_references(self, tmp_path: Path) -> None:
        _write(
            tmp_path,
            "CONTEXT.md",
            "See [planner](src/planner.py) and `legacy/main.py`",
        )
        report = inspect_context(
            context_path=tmp_path / "CONTEXT.md", repo_root=tmp_path
        )
        assert report.exists is True
        assert "src/planner.py" in report.missing_files
        assert "legacy/main.py" in report.missing_files

    def test_present_file_references_not_flagged_missing(
        self, tmp_path: Path
    ) -> None:
        _write(
            tmp_path,
            "CONTEXT.md",
            "See [planner](src/planner.py) and `legacy/main.py`",
        )
        _write(tmp_path, "src/planner.py", "# real\n")
        _write(tmp_path, "legacy/main.py", "# real\n")
        report = inspect_context(
            context_path=tmp_path / "CONTEXT.md", repo_root=tmp_path
        )
        assert report.missing_files == ()

    def test_age_days_reported(self, tmp_path: Path) -> None:
        ctx = _write(tmp_path, "CONTEXT.md", "## Language\n\n**Foo**:\nx\n")
        old = time.time() - 90 * 86400
        os.utime(ctx, (old, old))
        report = inspect_context(context_path=ctx, repo_root=tmp_path)
        assert report.age_days is not None
        assert report.age_days > 60


class TestBuildFreshness:
    def test_no_context_yields_empty_contexts_not_stale(
        self, tmp_path: Path
    ) -> None:
        report = build_freshness(repo_root=tmp_path)
        assert report.contexts == ()
        assert report.stale is False
        assert report.has_context_map is False

    def test_stale_when_orphans_present(self, tmp_path: Path) -> None:
        _write(tmp_path, "CONTEXT.md", "## Language\n\n**Ghost**:\ngone.\n")
        _write(tmp_path, "src/lib.rs", "fn other() {}\n")
        report = build_freshness(repo_root=tmp_path)
        assert report.stale is True

    def test_stale_when_missing_file_refs(self, tmp_path: Path) -> None:
        _write(tmp_path, "CONTEXT.md", "See `src/gone.py`")
        report = build_freshness(repo_root=tmp_path)
        assert report.stale is True

    def test_stale_when_age_over_threshold(self, tmp_path: Path) -> None:
        # term present in code so orphan check passes; only staleness is age
        ctx = _write(
            tmp_path, "CONTEXT.md", "## Language\n\n**Order**:\nx\n"
        )
        _write(tmp_path, "src/lib.rs", "struct Order;\n")
        old = time.time() - 90 * 86400
        os.utime(ctx, (old, old))
        report = build_freshness(repo_root=tmp_path)
        assert report.stale is True

    def test_context_map_discovers_multiple_contexts(
        self, tmp_path: Path
    ) -> None:
        _write(
            tmp_path,
            "CONTEXT-MAP.md",
            "- [Ordering](./src/ordering/CONTEXT.md)\n"
            "- [Billing](./src/billing/CONTEXT.md)\n",
        )
        _write(
            tmp_path,
            "src/ordering/CONTEXT.md",
            "## Language\n\n**Order**:\nx\n",
        )
        _write(
            tmp_path,
            "src/billing/CONTEXT.md",
            "## Language\n\n**Invoice**:\nx\n",
        )
        report = build_freshness(repo_root=tmp_path)
        assert report.has_context_map is True
        assert len(report.contexts) == 2
        paths = {c.path for c in report.contexts}
        assert paths == {
            "src/ordering/CONTEXT.md",
            "src/billing/CONTEXT.md",
        }
