"""Tests for slice_enricher.enrich (V6 in PRD a1f/claude-admin#16).

Run from repo root with:
    python3 -m unittest discover -s skills/_lib -p 'test_*.py' -v
or directly:
    python3 skills/_lib/test_slice_enricher.py
"""

from __future__ import annotations

import unittest

from slice_enricher import enrich


PRD_FIXTURE = """\
# Test PRD

## summary

irrelevant prose.

## deliverables

- [ ] **G1** · first deliverable
  - observable: builds the first thing.
  - why: foundational.

- [ ] **G2** · second deliverable
  - observable: builds the second thing.
  - why: depends on G1.

## validations

- [ ] **V1** · _e2e_ — `e2e_first` — covers G1
  - what: drives the whole thing.
  - how: shell script.

- [ ] **V2** · _unit_ — `unit_second` — covers G2
  - what: pure unit check.
  - how: pytest.

- [ ] **V3** · _module_ — `module_second` — covers G2
  - what: integration on module boundary.
  - how: pytest with fake subprocess.

## modules to CREATE

| name | path | responsibility | interface | tests |
|---|---|---|---|---|
| foo | `skills/_lib/foo.py` | does foo things | `foo() -> int` | V2 |
| bar | `skills/_lib/bar.py` | does bar things | `bar() -> str` | V3 |

## modules to UPDATE

| name | path | responsibility | interface | tests |
|---|---|---|---|---|
| baz | `skills/baz/SKILL.md` | adds baz section | (prompt) | manual |
"""


class EnrichInlinesContextTests(unittest.TestCase):
    """V6: enrichment phase pulls correct PRD sections + module rows into each slice body."""

    def test_pulls_prd_excerpt_for_cited_g_only(self):
        slice_draft = {
            "title": "S2 · second",
            "type": "AFK",
            "deliverable": "Build the second thing end-to-end.",
            "acceptance": [
                "- [ ] foo() returns int",
                "- [ ] bar() returns str",
            ],
            "validations": ["V2", "V3"],
            "blocked_by": [],
            "modules": ["foo", "bar"],
        }
        body = enrich(slice_draft, PRD_FIXTURE)
        self.assertIn("**G2** · second deliverable", body)
        self.assertIn("builds the second thing.", body)
        self.assertNotIn("**G1** · first deliverable", body)

    def test_classifies_validations_by_kind(self):
        slice_draft = {
            "deliverable": "x",
            "validations": ["V1", "V2", "V3"],
            "modules": [],
        }
        body = enrich(slice_draft, PRD_FIXTURE)
        e2e_block = _section_block(body, "E2E covered")
        module_block = _section_block(body, "Module-test")
        self.assertIn("`e2e_first`", e2e_block)
        self.assertNotIn("`unit_second`", e2e_block)
        self.assertIn("`module_second`", module_block)
        self.assertNotIn("`e2e_first`", module_block)

    def test_definition_of_done_combines_acceptance_and_validations(self):
        slice_draft = {
            "deliverable": "x",
            "acceptance": ["- [ ] foo() returns int", "bar() works"],
            "validations": ["V2"],
            "modules": [],
        }
        body = enrich(slice_draft, PRD_FIXTURE)
        dod = _section_block(body, "Definition of done")
        self.assertIn("- [ ] foo() returns int", dod)
        self.assertIn("- [ ] bar() works", dod)  # auto-wrapped
        self.assertIn("**V2**", dod)
        self.assertIn("`unit_second` passes", dod)

    def test_matches_module_rows_from_prd_matrix(self):
        slice_draft = {
            "deliverable": "Wire foo.",
            "validations": ["V2"],
            "modules": ["foo"],
        }
        body = enrich(slice_draft, PRD_FIXTURE)
        ctx = _section_block(body, "Context (enriched)")
        self.assertIn("| CREATE | foo |", ctx)
        self.assertIn("skills/_lib/foo.py", ctx)
        self.assertNotIn("| CREATE | bar |", ctx)

    def test_module_match_does_not_substring_match_names(self):
        """Regression: `"foo"` must not match a row named `"foobar"`."""
        prd_with_lookalike = PRD_FIXTURE + """
## modules to CREATE

| name | path | responsibility | interface | tests |
|---|---|---|---|---|
| foobar | `skills/_lib/foobar.py` | unrelated | `foobar()` | V99 |
"""
        slice_draft = {"deliverable": "x", "validations": [], "modules": ["foo"]}
        body = enrich(slice_draft, prd_with_lookalike)
        self.assertIn("| CREATE | foo |", body)
        self.assertNotIn("| CREATE | foobar |", body)

    def test_matches_module_rows_from_update_section(self):
        slice_draft = {
            "deliverable": "Touch baz.",
            "validations": [],
            "modules": ["baz"],
        }
        body = enrich(slice_draft, PRD_FIXTURE)
        ctx = _section_block(body, "Context (enriched)")
        self.assertIn("| UPDATE | baz |", ctx)

    def test_inlines_lessons_when_supplied(self):
        slice_draft = {
            "deliverable": "Touch foo.",
            "validations": [],
            "modules": ["foo"],
        }
        lessons = {"foo": "- prefer pure functions\n- no global state"}
        body = enrich(slice_draft, PRD_FIXTURE, lessons=lessons)
        ctx = _section_block(body, "Context (enriched)")
        self.assertIn("modules/foo/LESSONS.md", ctx)
        self.assertIn("prefer pure functions", ctx)

    def test_extra_modules_md_merges_with_prd_matrix(self):
        extra = """\
## modules to CREATE

| name | path | responsibility | interface | tests |
|---|---|---|---|---|
| zoo | `skills/_lib/zoo.py` | does zoo | `zoo()` | V99 |
"""
        slice_draft = {
            "deliverable": "x",
            "validations": [],
            "modules": ["zoo"],
        }
        body = enrich(slice_draft, PRD_FIXTURE, extra)
        self.assertIn("zoo", body)
        self.assertIn("skills/_lib/zoo.py", body)


class EnrichGracefulEdgesTests(unittest.TestCase):
    def test_empty_validations_renders_none_markers(self):
        body = enrich(
            {"deliverable": "stub.", "validations": [], "modules": []}, PRD_FIXTURE
        )
        self.assertIn("## E2E covered\n\n_None._", body)
        self.assertIn("## Module-test\n\n_None._", body)

    def test_no_matching_modules_renders_none_marker(self):
        body = enrich(
            {
                "deliverable": "hits no modules.",
                "validations": ["V1"],
                "modules": ["nonexistent"],
            },
            PRD_FIXTURE,
        )
        self.assertIn("_No matching module rows._", body)

    def test_no_lessons_renders_none_marker(self):
        body = enrich(
            {"deliverable": "x", "validations": [], "modules": []}, PRD_FIXTURE
        )
        self.assertIn("_No neighbouring `LESSONS.md` provided._", body)

    def test_unknown_validation_id_is_silently_dropped(self):
        body = enrich(
            {
                "deliverable": "x",
                "validations": ["V99"],
                "modules": [],
            },
            PRD_FIXTURE,
        )
        self.assertIn("## E2E covered\n\n_None._", body)
        self.assertNotIn("V99", body)

    def test_parent_section_emitted_when_present(self):
        body = enrich(
            {"deliverable": "x", "parent": "#17 — M1 breakdown"}, PRD_FIXTURE
        )
        self.assertTrue(body.startswith("## Parent"))
        self.assertIn("#17", body)

    def test_parent_section_omitted_when_absent(self):
        body = enrich({"deliverable": "x"}, PRD_FIXTURE)
        self.assertFalse(body.startswith("## Parent"))

    def test_blocked_by_lists_items(self):
        body = enrich(
            {"deliverable": "x", "blocked_by": ["S7", "S9"]}, PRD_FIXTURE
        )
        bb = _section_block(body, "Blocked by")
        self.assertIn("- S7", bb)
        self.assertIn("- S9", bb)

    def test_blocked_by_empty_says_none(self):
        body = enrich({"deliverable": "x", "blocked_by": []}, PRD_FIXTURE)
        self.assertIn("None — can start immediately.", body)


def _section_block(body: str, header: str) -> str:
    """Slice out a `## <header>` block from rendered body up to next `## `."""
    marker = f"## {header}"
    start = body.find(marker)
    if start == -1:
        return ""
    rest = body[start + len(marker):]
    nxt = rest.find("\n## ")
    return rest if nxt == -1 else rest[:nxt]


if __name__ == "__main__":
    unittest.main()
