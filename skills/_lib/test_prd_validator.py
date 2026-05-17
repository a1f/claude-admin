from __future__ import annotations

from pathlib import Path
from textwrap import dedent

from prd_validator import _main, validate


def _valid_prd() -> str:
    return dedent("""
        # Sample PRD

        ## Summary

        Build a thing that does the thing for the people.

        ## Deliverables

        - [ ] **G1** · first goal
          - observable: thing happens
          - why: because

        - [ ] **G2** · second goal
          - observable: other thing happens
          - why: also because

        ## Validations

        - [ ] **V1** · _unit_ — `first_test` — covers G1
          - what: test the first thing
          - how: pytest

        - [ ] **V2** · _e2e_ — `second_test` — covers G2
          - what: test the second thing end-to-end
          - how: shell script

        ## Modules to CREATE

        | name | path | responsibility | interface | tests |
        |---|---|---|---|---|
        | thing | `path/to/thing.py` | does the thing | `do() -> None` | V1 |

        ## Modules to UPDATE

        _none_

        ## Test plan

        Test the things using pytest.

        ## Q&A

        <details>
        <summary>Grilling Q&A</summary>

        Q: Why does this exist?
        A: Because someone asked.

        </details>
        """).strip()


def _codes(body: str) -> list[str]:
    return [e.code for e in validate(body=body)]


def test_valid_prd_passes() -> None:
    assert validate(body=_valid_prd()) == []


def test_missing_section_flagged() -> None:
    body = _valid_prd().replace("## Test plan", "## Not Test Plan")
    assert "MISSING_SECTION" in _codes(body)


def test_g_numbering_gap_flagged() -> None:
    body = _valid_prd().replace("**G2**", "**G3**")
    codes = _codes(body)
    assert "MISSING_NUMBER" in codes
    assert "OUT_OF_SEQUENCE" in codes


def test_v_numbering_gap_flagged() -> None:
    body = _valid_prd().replace("**V2**", "**V3**")
    codes = _codes(body)
    assert "MISSING_NUMBER" in codes
    assert "OUT_OF_SEQUENCE" in codes


def test_duplicate_g_flagged() -> None:
    body = _valid_prd().replace("**G2**", "**G1**")
    assert "DUPLICATE" in _codes(body)


def test_v_with_no_g_cite_flagged() -> None:
    body = _valid_prd().replace(
        "**V1** · _unit_ — `first_test` — covers G1",
        "**V1** · _unit_ — `first_test`",
    )
    assert "V_NO_G_CITE" in _codes(body)


def test_v_with_unknown_g_flagged() -> None:
    body = _valid_prd().replace(
        "**V1** · _unit_ — `first_test` — covers G1",
        "**V1** · _unit_ — `first_test` — covers G99",
    )
    assert "V_UNKNOWN_G" in _codes(body)


def test_placeholder_angle_bracket_flagged() -> None:
    body = _valid_prd().replace(
        "Build a thing that does the thing for the people.",
        "Build a <thing to be filled in> for the people.",
    )
    assert "PLACEHOLDER" in _codes(body)


def test_placeholder_todo_flagged() -> None:
    body = _valid_prd() + "\n\nTODO: fill in later\n"
    assert "PLACEHOLDER" in _codes(body)


def test_placeholder_tbd_flagged() -> None:
    body = _valid_prd().replace("Test the things using pytest.", "TBD")
    assert "PLACEHOLDER" in _codes(body)


def test_html_tags_not_flagged() -> None:
    errors = validate(body=_valid_prd())
    assert [e for e in errors if e.code == "PLACEHOLDER"] == []


def test_url_in_angle_brackets_not_flagged() -> None:
    body = _valid_prd().replace(
        "Build a thing",
        "Build a thing (see <https://example.com/spec>)",
    )
    errors = validate(body=body)
    assert [e for e in errors if e.code == "PLACEHOLDER"] == []


def test_placeholder_inside_fenced_code_ignored() -> None:
    body = _valid_prd() + "\n\n```\nTODO this is in a fenced block\n```\n"
    errors = validate(body=body)
    todo_errors = [
        e for e in errors if e.code == "PLACEHOLDER" and "TODO" in e.message
    ]
    assert todo_errors == []


def test_placeholder_inside_inline_code_ignored() -> None:
    body = _valid_prd().replace(
        "Test the things using pytest.",
        "Test the things using pytest. See `<placeholder>` syntax docs.",
    )
    errors = validate(body=body)
    placeholder_errors = [
        e for e in errors if e.code == "PLACEHOLDER" and "<placeholder>" in e.message
    ]
    assert placeholder_errors == []


def test_module_table_missing_rows_flagged() -> None:
    body = _valid_prd().replace(
        "| thing | `path/to/thing.py` | does the thing | `do() -> None` | V1 |",
        "",
    )
    assert "MODULE_TABLE_MISSING" in _codes(body)


def test_module_table_row_column_mismatch_flagged() -> None:
    body = _valid_prd().replace(
        "| thing | `path/to/thing.py` | does the thing | `do() -> None` | V1 |",
        "| thing | `path/to/thing.py` | V1 |",
    )
    assert "MODULE_TABLE_BAD_ROW" in _codes(body)


def test_module_table_none_marker_accepted() -> None:
    errors = validate(body=_valid_prd())
    table_errors = [e for e in errors if e.code.startswith("MODULE_TABLE")]
    assert table_errors == []


def test_module_table_no_separator_flagged() -> None:
    body = _valid_prd().replace(
        "|---|---|---|---|---|",
        "| name | path | responsibility | interface | tests |",
    )
    assert "MODULE_TABLE_NO_SEPARATOR" in _codes(body)


def test_empty_validations_section_flagged() -> None:
    body = _valid_prd().replace(
        "- [ ] **V1** · _unit_ — `first_test` — covers G1\n  - what: test the first thing\n  - how: pytest\n\n- [ ] **V2** · _e2e_ — `second_test` — covers G2\n  - what: test the second thing end-to-end\n  - how: shell script",
        "",
    )
    assert "NO_ITEMS" in _codes(body)


def test_cli_exits_zero_on_valid(tmp_path: Path) -> None:
    prd_file = tmp_path / "valid.md"
    prd_file.write_text(_valid_prd(), encoding="utf-8")
    assert _main(argv=["prd_validator.py", str(prd_file)]) == 0


def test_cli_exits_one_on_invalid(tmp_path: Path) -> None:
    body = _valid_prd().replace("## Summary", "## Not Summary")
    prd_file = tmp_path / "bad.md"
    prd_file.write_text(body, encoding="utf-8")
    assert _main(argv=["prd_validator.py", str(prd_file)]) == 1


def test_cli_exits_two_on_bad_args() -> None:
    assert _main(argv=["prd_validator.py"]) == 2


def test_cli_exits_two_on_missing_file(tmp_path: Path) -> None:
    assert _main(argv=["prd_validator.py", str(tmp_path / "nope.md")]) == 2
