from pathlib import Path

import pytest

from doc_parsing import parse_file_refs, parse_terms, term_orphaned

CONTEXT_SAMPLE = """# Ordering

## Language

**Order**:
A request from a Customer.

**Customer**:
A person who places Orders.

**Multi Word Term**:
Something with spaces.

## Relationships

- An **Order** belongs to a **Customer**

## References

- See [the planner](src/planner.py)
- The legacy entrypoint is `legacy/main.py`
- External: [docs](https://example.com/docs)
- Anchor only: [self](#self)
"""


def _write(repo: Path, rel: str, content: str = "") -> Path:
    target = repo / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")
    return target


class TestParseTerms:
    def test_extracts_bold_terms_under_language_heading(self) -> None:
        assert parse_terms(text=CONTEXT_SAMPLE) == (
            "Order", "Customer", "Multi Word Term",
        )

    def test_returns_empty_when_no_language_heading(self) -> None:
        assert parse_terms(text="# T\n\nProse with **Bold** in it.\n") == ()

    def test_dedupes_terms(self) -> None:
        text = "## Language\n\n**Foo**:\nx\n\n**Foo**:\ny\n"
        assert parse_terms(text=text) == ("Foo",)

    def test_stops_at_next_heading(self) -> None:
        text = (
            "## Language\n\n**InScope**:\nin\n\n"
            "## Relationships\n\n**OutOfScope**:\nout\n"
        )
        assert parse_terms(text=text) == ("InScope",)


class TestParseFileRefs:
    def test_extracts_relative_paths_skipping_urls_and_anchors(
        self, tmp_path: Path
    ) -> None:
        ctx = _write(tmp_path, "CONTEXT.md", CONTEXT_SAMPLE)
        refs = parse_file_refs(
            text=CONTEXT_SAMPLE, context_dir=ctx.parent, repo_root=tmp_path
        )
        assert "src/planner.py" in refs
        assert "legacy/main.py" in refs
        for ref in refs:
            assert not ref.startswith("http")
            assert not ref.startswith("#")

    def test_ignores_paths_outside_repo(self, tmp_path: Path) -> None:
        ctx = _write(tmp_path, "CONTEXT.md", "[outside](../../../etc/passwd)")
        refs = parse_file_refs(
            text="[outside](../../../etc/passwd)",
            context_dir=ctx.parent,
            repo_root=tmp_path,
        )
        assert refs == ()


class TestTermOrphaned:
    def test_term_with_code_hits_not_orphaned(self, tmp_path: Path) -> None:
        _write(tmp_path, "src/lib.rs", "struct Order;\nfn make_order() {}\n")
        assert term_orphaned(term="Order", repo_root=tmp_path) is False

    def test_term_only_in_docs_is_orphaned(self, tmp_path: Path) -> None:
        _write(tmp_path, "CONTEXT.md", "**Fictional**: nothing real.")
        _write(tmp_path, "docs/notes.md", "we love Fictional things")
        assert term_orphaned(term="Fictional", repo_root=tmp_path) is True

    @pytest.mark.parametrize(
        ("source_text", "term", "expected_orphan"),
        [
            ("struct Ordering;\n", "Order", True),
            ("fn make_order() {}\n", "Order", True),
            ("let Order = 1;\n", "Order", False),
        ],
    )
    def test_whole_word_case_sensitive_match(
        self,
        tmp_path: Path,
        source_text: str,
        term: str,
        expected_orphan: bool,
    ) -> None:
        _write(tmp_path, "src/lib.rs", source_text)
        assert (
            term_orphaned(term=term, repo_root=tmp_path) is expected_orphan
        )
