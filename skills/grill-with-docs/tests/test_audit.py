import json
from dataclasses import asdict
from pathlib import Path

import pytest

from grill_docs import run_audit, snapshot_from_dict, take_snapshot
from models import Snapshot


def _write(repo: Path, rel: str, content: str = "") -> Path:
    target = repo / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")
    return target


class TestTakeSnapshot:
    def test_records_context_hash_and_terms(self, tmp_path: Path) -> None:
        _write(tmp_path, "CONTEXT.md", "## Language\n\n**Order**:\nx\n")
        snap = take_snapshot(repo_root=tmp_path)
        assert len(snap.contexts) == 1
        cs = snap.contexts[0]
        assert cs.exists is True
        assert cs.path == "CONTEXT.md"
        assert cs.terms == ("Order",)
        assert cs.sha256 is not None

    def test_records_missing_context_as_not_existing(
        self, tmp_path: Path
    ) -> None:
        snap = take_snapshot(repo_root=tmp_path)
        assert len(snap.contexts) == 1
        assert snap.contexts[0].exists is False

    def test_lists_pre_existing_adrs(self, tmp_path: Path) -> None:
        _write(tmp_path, "docs/adr/0001-foo.md", "old")
        _write(tmp_path, "docs/adr/0002-bar.md", "old")
        _write(tmp_path, "docs/adr/README.md", "ignored")
        snap = take_snapshot(repo_root=tmp_path)
        assert snap.adr_files == ("0001-foo.md", "0002-bar.md")


class TestAuditTerms:
    def test_clean_when_decided_term_present_in_context(
        self, tmp_path: Path
    ) -> None:
        _write(tmp_path, "CONTEXT.md", "## Language\n\n**Coder**:\nx\n")
        snap = Snapshot(repo_root=str(tmp_path))
        report = run_audit(
            repo_root=tmp_path,
            snap=snap,
            decided={"terms": [{"name": "Coder"}]},
        )
        assert report.clean is True
        assert report.terms_ok == ["CONTEXT.md::Coder"]

    def test_flags_term_not_written(self, tmp_path: Path) -> None:
        _write(tmp_path, "CONTEXT.md", "## Language\n\n**Other**:\nx\n")
        snap = Snapshot(repo_root=str(tmp_path))
        report = run_audit(
            repo_root=tmp_path,
            snap=snap,
            decided={"terms": [{"name": "Coder"}]},
        )
        assert report.clean is False
        assert len(report.term_mismatches) == 1
        assert report.term_mismatches[0].reason == "term-not-written"

    def test_flags_missing_context_file(self, tmp_path: Path) -> None:
        snap = Snapshot(repo_root=str(tmp_path))
        report = run_audit(
            repo_root=tmp_path,
            snap=snap,
            decided={"terms": [{"name": "Coder"}]},
        )
        assert report.clean is False
        assert report.term_mismatches[0].reason == "context-missing"

    def test_term_in_specific_context_path(self, tmp_path: Path) -> None:
        _write(
            tmp_path,
            "src/billing/CONTEXT.md",
            "## Language\n\n**Invoice**:\nx\n",
        )
        snap = Snapshot(repo_root=str(tmp_path))
        report = run_audit(
            repo_root=tmp_path,
            snap=snap,
            decided={
                "terms": [
                    {"name": "Invoice", "context": "src/billing/CONTEXT.md"}
                ]
            },
        )
        assert report.clean is True


class TestAuditAdrs:
    def test_clean_when_new_adr_written(self, tmp_path: Path) -> None:
        snap = Snapshot(repo_root=str(tmp_path), adr_files=())
        _write(tmp_path, "docs/adr/0001-tmux-runtime.md", "...")
        report = run_audit(
            repo_root=tmp_path,
            snap=snap,
            decided={"adrs": [{"slug": "tmux-runtime"}]},
        )
        assert report.clean is True
        assert report.adrs_ok == ["0001-tmux-runtime.md"]

    def test_flags_adr_not_written(self, tmp_path: Path) -> None:
        snap = Snapshot(repo_root=str(tmp_path), adr_files=())
        report = run_audit(
            repo_root=tmp_path,
            snap=snap,
            decided={"adrs": [{"slug": "tmux-runtime"}]},
        )
        assert report.clean is False
        assert report.adr_mismatches[0].reason == "no-matching-file"

    def test_flags_adr_already_existed(self, tmp_path: Path) -> None:
        _write(tmp_path, "docs/adr/0001-tmux-runtime.md", "...")
        snap = Snapshot(
            repo_root=str(tmp_path), adr_files=("0001-tmux-runtime.md",)
        )
        report = run_audit(
            repo_root=tmp_path,
            snap=snap,
            decided={"adrs": [{"slug": "tmux-runtime"}]},
        )
        assert report.clean is False
        assert report.adr_mismatches[0].reason == "already-existed"

    @pytest.mark.parametrize(
        ("filename", "decided_number", "should_match"),
        [
            ("0005-foo.md", 7, False),
            ("0005-foo.md", 5, True),
            ("0005-foo.md", None, True),
            ("0001-bar.md", None, False),
        ],
    )
    def test_number_must_match_when_given(
        self,
        tmp_path: Path,
        filename: str,
        decided_number: int | None,
        should_match: bool,
    ) -> None:
        _write(tmp_path, f"docs/adr/{filename}", "...")
        snap = Snapshot(repo_root=str(tmp_path), adr_files=())
        decided_entry: dict[str, object] = {"slug": "foo"}
        if decided_number is not None:
            decided_entry["number"] = decided_number
        report = run_audit(
            repo_root=tmp_path,
            snap=snap,
            decided={"adrs": [decided_entry]},
        )
        assert report.clean is should_match


class TestSnapshotRoundtrip:
    def test_snapshot_serialises_via_json(self, tmp_path: Path) -> None:
        _write(tmp_path, "CONTEXT.md", "## Language\n\n**X**:\ny\n")
        _write(tmp_path, "docs/adr/0001-a.md", "...")
        snap = take_snapshot(repo_root=tmp_path)
        restored = snapshot_from_dict(data=json.loads(json.dumps(asdict(snap))))
        assert restored.adr_files == snap.adr_files
        assert len(restored.contexts) == len(snap.contexts)
        assert restored.contexts[0].terms == ("X",)
