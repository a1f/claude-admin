#!/usr/bin/env python3
"""Unit tests for the /cc-help renderer.

Covers V1 (pipeline output mentions 'architector') and V10 (all 9 pipeline
steps render in order, each with a non-empty one-line purpose).
"""

from __future__ import annotations

import re
import sys
import tempfile
import unittest
from pathlib import Path

# Make `render` importable without polluting sys.path globally.
_SCRIPTS = Path(__file__).resolve().parent.parent / "scripts"
sys.path.insert(0, str(_SCRIPTS))

import render  # noqa: E402  (sys.path tweak above)
from render import (  # noqa: E402
    FALLBACK_DESCRIPTIONS,
    PIPELINE,
    one_line,
    parse_frontmatter,
)
from render import render as render_pipeline  # noqa: E402


# Skills that exist today (M1-S1 baseline). Used to build a realistic fake
# skills/ dir for the integration-ish render tests.
_INSTALLED_TODAY = {
    "coder": "Internal skill loaded by /dispatch. Defines the coder agent.",
    "critic": "Internal skill loaded by the watcher when fanning out critiques.",
    "pr-babysit": "User-facing skill for the post-review decision.",
    "to-issues": "Break a plan or PRD into independently-grabbable issues.",
}


def _build_fake_skills(root: Path, installed: dict[str, str]) -> None:
    """Create `<root>/<name>/SKILL.md` files with the given descriptions."""
    for name, desc in installed.items():
        (root / name).mkdir(parents=True, exist_ok=True)
        (root / name / "SKILL.md").write_text(
            f'---\nname: {name}\ndescription: "{desc}"\n---\n\nbody\n',
            encoding="utf-8",
        )


class TestParseFrontmatter(unittest.TestCase):
    def test_extracts_quoted_description(self) -> None:
        text = '---\nname: foo\ndescription: "hello world"\n---\n\nbody'
        self.assertEqual(parse_frontmatter(text)["description"], "hello world")

    def test_extracts_unquoted_description(self) -> None:
        text = "---\nname: foo\ndescription: hello world\n---\n"
        self.assertEqual(parse_frontmatter(text)["description"], "hello world")

    def test_extracts_single_quoted(self) -> None:
        text = "---\nname: foo\ndescription: 'hi there'\n---\n"
        self.assertEqual(parse_frontmatter(text)["description"], "hi there")

    def test_no_frontmatter_returns_empty(self) -> None:
        self.assertEqual(parse_frontmatter("just body text\n"), {})

    def test_unterminated_frontmatter_returns_empty(self) -> None:
        self.assertEqual(parse_frontmatter("---\nname: foo\nbody\n"), {})


class TestOneLine(unittest.TestCase):
    def test_first_sentence(self) -> None:
        self.assertEqual(
            one_line("Do this thing. Then do another. And another."),
            "Do this thing",
        )

    def test_no_period_returns_whole(self) -> None:
        self.assertEqual(one_line("just one line"), "just one line")

    def test_period_at_end_only(self) -> None:
        self.assertEqual(one_line("just one sentence."), "just one sentence")

    def test_empty_input(self) -> None:
        self.assertEqual(one_line(""), "")


class TestRenderOutput(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.skills = Path(self._tmp.name)
        _build_fake_skills(self.skills, _INSTALLED_TODAY)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    # --- V1 -------------------------------------------------------------
    def test_v1_output_mentions_architector(self) -> None:
        """V1 proxy: rendered pipeline contains 'architector'."""
        out = render_pipeline(self.skills)
        self.assertIn("architector", out)

    # --- V10 ------------------------------------------------------------
    def test_v10_pipeline_has_exactly_nine_steps(self) -> None:
        """V10: PRD declares 9 ordered pipeline steps."""
        self.assertEqual(len(PIPELINE), 9)

    def test_v10_all_nine_steps_appear_in_order(self) -> None:
        """V10: every pipeline step appears, in declared order."""
        out = render_pipeline(self.skills)
        positions = [out.find(f"/{name}") for name in PIPELINE]
        for name, pos in zip(PIPELINE, positions, strict=True):
            self.assertGreater(pos, -1, f"missing /{name} in output")
        self.assertEqual(
            positions,
            sorted(positions),
            f"steps out of order: {positions}",
        )

    def test_v10_each_step_has_one_line_purpose(self) -> None:
        """V10: every step renders with a non-empty one-line purpose."""
        out = render_pipeline(self.skills)
        for i, name in enumerate(PIPELINE, start=1):
            # Line shape: "<n>. /<name>...  -  <purpose>[  (planned)]"
            pattern = rf"^\s*{i}\.\s+/{re.escape(name)}\s+-\s+(.+)$"
            match = re.search(pattern, out, re.MULTILINE)
            self.assertIsNotNone(match, f"no line for step {i} /{name}")
            assert match is not None  # for type checker
            purpose = match.group(1).strip()
            # Strip the optional planned marker before asserting non-empty.
            purpose_clean = purpose.removesuffix("(planned)").strip()
            self.assertTrue(
                purpose_clean,
                f"empty purpose for step {i} /{name}",
            )

    # --- behavioural --------------------------------------------------
    def test_installed_skill_uses_frontmatter_description(self) -> None:
        out = render_pipeline(self.skills)
        # 'coder' is installed in the fake dir with our test description.
        self.assertIn("Internal skill loaded by /dispatch", out)

    def test_missing_skill_uses_fallback_with_planned_marker(self) -> None:
        out = render_pipeline(self.skills)
        # 'review' is NOT installed; must show fallback + (planned).
        review_lines = [ln for ln in out.splitlines() if "/review " in ln + " "]
        self.assertEqual(
            len(review_lines), 1, f"expected one /review line, got: {review_lines}"
        )
        line = review_lines[0]
        self.assertIn("(planned)", line)
        self.assertIn(FALLBACK_DESCRIPTIONS["review"], line)

    def test_fallback_descriptions_cover_every_pipeline_step(self) -> None:
        """No pipeline step may be missing a fallback (would crash on render)."""
        for name in PIPELINE:
            self.assertIn(name, FALLBACK_DESCRIPTIONS)

    def test_render_against_repo_skills_dir_smoke(self) -> None:
        """Smoke: rendering against the real repo skills dir doesn't crash."""
        repo_skills = Path(render.__file__).resolve().parent.parent.parent
        out = render_pipeline(repo_skills)
        self.assertIn("architector", out)
        self.assertIn("/coder", out)


if __name__ == "__main__":
    unittest.main()
