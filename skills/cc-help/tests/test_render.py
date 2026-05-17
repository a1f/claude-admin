"""Pytest suite for the /cc-help renderer.

Locks down V1 (output mentions 'architector') and V10 (all 9 steps in order,
each with a one-line purpose) so future edits to render.py can't silently
break the visible /cc-help contract.
"""

import re
import sys
from pathlib import Path

import pytest

_SCRIPTS: Path = Path(__file__).resolve().parent.parent / "scripts"
sys.path.insert(0, str(_SCRIPTS))

import render  # noqa: E402  (sys.path tweak above)
from render import (  # noqa: E402
    FALLBACK_DESCRIPTIONS,
    PIPELINE,
    one_line,
    parse_frontmatter,
    render as render_pipeline,
)

# A realistic subset of what's actually installed at the M1-S1 baseline.
# Kept here (not in render.py) so prod constants don't carry test fixtures.
_INSTALLED_TODAY: dict[str, str] = {
    "coder": "Internal skill loaded by /dispatch. Defines the coder agent.",
    "critic": "Internal skill loaded by the watcher when fanning out critiques.",
    "pr-babysit": "User-facing skill for the post-review decision.",
    "to-issues": "Break a plan or PRD into independently-grabbable issues.",
}


@pytest.fixture
def fake_skills(tmp_path: Path) -> Path:
    """Mirrors today's installed-vs-planned mix so render() exercises both paths."""
    for name, desc in _INSTALLED_TODAY.items():
        skill_dir: Path = tmp_path / name
        skill_dir.mkdir(parents=True)
        (skill_dir / "SKILL.md").write_text(
            f'---\nname: {name}\ndescription: "{desc}"\n---\n\nbody\n',
            encoding="utf-8",
        )
    return tmp_path


# ----- parse_frontmatter -------------------------------------------------


def test_parse_frontmatter_extracts_quoted_description() -> None:
    text: str = '---\nname: foo\ndescription: "hello world"\n---\n\nbody'
    assert parse_frontmatter(text=text)["description"] == "hello world"


def test_parse_frontmatter_extracts_unquoted_description() -> None:
    text: str = "---\nname: foo\ndescription: hello world\n---\n"
    assert parse_frontmatter(text=text)["description"] == "hello world"


def test_parse_frontmatter_extracts_single_quoted() -> None:
    text: str = "---\nname: foo\ndescription: 'hi there'\n---\n"
    assert parse_frontmatter(text=text)["description"] == "hi there"


def test_parse_frontmatter_no_frontmatter_returns_empty() -> None:
    assert parse_frontmatter(text="just body text\n") == {}


def test_parse_frontmatter_unterminated_returns_empty() -> None:
    assert parse_frontmatter(text="---\nname: foo\nbody\n") == {}


# ----- one_line ----------------------------------------------------------


def test_one_line_takes_first_sentence() -> None:
    assert one_line(description="Do this thing. Then another.") == "Do this thing"


def test_one_line_no_period_returns_whole() -> None:
    assert one_line(description="just one line") == "just one line"


def test_one_line_period_at_end_only() -> None:
    assert one_line(description="just one sentence.") == "just one sentence"


def test_one_line_empty_input() -> None:
    assert one_line(description="") == ""


# ----- V1 ---------------------------------------------------------------


def test_v1_output_mentions_architector(fake_skills: Path) -> None:
    """V1 proxy: the e2e gate `claude -p '/cc-help' | grep architector` must pass."""
    out: str = render_pipeline(skills_dir=fake_skills)
    assert "architector" in out


# ----- V10 --------------------------------------------------------------


def test_v10_pipeline_has_exactly_nine_steps() -> None:
    """V10 starts with the PRD-declared count: 9 pipeline steps, not 8 or 10."""
    assert len(PIPELINE) == 9


def test_v10_all_nine_steps_appear_in_declared_order(fake_skills: Path) -> None:
    """V10: each step appears exactly once, in PIPELINE order."""
    out: str = render_pipeline(skills_dir=fake_skills)
    positions: list[int] = [out.find(f"/{name}") for name in PIPELINE]
    for name, pos in zip(PIPELINE, positions, strict=True):
        assert pos > -1, f"missing /{name} in output"
    assert positions == sorted(positions), f"steps out of order: {positions}"


def test_v10_each_step_has_one_line_purpose(fake_skills: Path) -> None:
    """V10: every step row has a non-empty purpose after the ` - ` separator."""
    out: str = render_pipeline(skills_dir=fake_skills)
    for i, name in enumerate(PIPELINE, start=1):
        pattern: str = rf"^\s*{i}\.\s+/{re.escape(name)}\s+-\s+(.+)$"
        match: re.Match[str] | None = re.search(pattern, out, re.MULTILINE)
        assert match is not None, f"no row for step {i} /{name}"
        # Drop optional `(planned)` suffix before checking the purpose is non-empty.
        purpose_clean: str = match.group(1).strip().removesuffix("(planned)").strip()
        assert purpose_clean, f"empty purpose for step {i} /{name}"


# ----- behavioural ------------------------------------------------------


def test_installed_skill_uses_frontmatter_description(fake_skills: Path) -> None:
    out: str = render_pipeline(skills_dir=fake_skills)
    assert "Internal skill loaded by /dispatch" in out


def test_missing_skill_uses_fallback_with_planned_marker(fake_skills: Path) -> None:
    out: str = render_pipeline(skills_dir=fake_skills)
    review_lines: list[str] = [ln for ln in out.splitlines() if "/review " in ln + " "]
    assert len(review_lines) == 1, f"expected one /review line, got: {review_lines}"
    line: str = review_lines[0]
    assert "(planned)" in line
    assert FALLBACK_DESCRIPTIONS["review"] in line


def test_fallback_descriptions_cover_every_pipeline_step() -> None:
    """Render would KeyError if any pipeline name lacked a fallback — fail fast here."""
    for name in PIPELINE:
        assert name in FALLBACK_DESCRIPTIONS


def test_render_against_real_repo_skills_dir_does_not_crash() -> None:
    """Catches regressions where a real SKILL.md format change breaks the parser."""
    repo_skills: Path = Path(render.__file__).resolve().parent.parent.parent
    out: str = render_pipeline(skills_dir=repo_skills)
    assert "architector" in out
    assert "/coder" in out
