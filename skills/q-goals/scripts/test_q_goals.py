"""Tests for q_goals.py validators + ratify gate + init.

Run:
    python3 -m pytest skills/q-goals/scripts/test_q_goals.py -v
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import q_goals  # noqa: E402


# ----------------------------------------------------------------------------
# validate_goals
# ----------------------------------------------------------------------------

GOOD_GOALS = """\
## Deliverables

- [ ] **G1** · ship the daemon
  - **Observable when:** `ca daemon` starts and `ca ping` returns
  - **Why:** load-bearing foundation for all subsequent work

- [ ] **G2** · sqlite ready
  - **Observable when:** ~/.work/claude-admin.db has all 12 tables
  - **Why:** shared state for skills
"""


def test_validate_goals_happy_path() -> None:
    errors, g_nums = q_goals.validate_goals(GOOD_GOALS)
    assert errors == []
    assert g_nums == [1, 2]


def test_validate_goals_missing_observable() -> None:
    text = """\
## Deliverables

- [ ] **G1** · ship the daemon
  - **Why:** load-bearing
"""
    errors, _ = q_goals.validate_goals(text)
    assert any("Observable when" in e for e in errors)


def test_validate_goals_missing_why() -> None:
    text = """\
## Deliverables

- [ ] **G1** · ship the daemon
  - **Observable when:** `ca ping` returns
"""
    errors, _ = q_goals.validate_goals(text)
    assert any("Why" in e for e in errors)


def test_validate_goals_placeholder_left() -> None:
    text = """\
## Deliverables

- [ ] **G1** · <short name>
  - **Observable when:** signal
  - **Why:** reason
"""
    errors, _ = q_goals.validate_goals(text)
    assert any("placeholder" in e for e in errors)


def test_validate_goals_non_sequential() -> None:
    text = """\
## Deliverables

- [ ] **G1** · first
  - **Observable when:** sig
  - **Why:** because

- [ ] **G3** · third
  - **Observable when:** sig
  - **Why:** because
"""
    errors, _ = q_goals.validate_goals(text)
    assert any("sequential" in e for e in errors)


def test_validate_goals_no_section() -> None:
    errors, g_nums = q_goals.validate_goals("# Title only\n\nNo deliverables here.\n")
    assert any("Deliverables" in e for e in errors)
    assert g_nums == []


def test_validate_goals_checked_box_accepted() -> None:
    text = """\
## Deliverables

- [x] **G1** · done already
  - **Observable when:** sig
  - **Why:** because
"""
    errors, g_nums = q_goals.validate_goals(text)
    assert errors == []
    assert g_nums == [1]


# ----------------------------------------------------------------------------
# validate_validations
# ----------------------------------------------------------------------------

GOOD_VALIDATIONS = """\
## Scenarios

- **V1** · _unit_ — `test_daemon_starts` — covers G1
  - **What it tests:** daemon bootstrap
  - **How:** pytest crates/ca-daemon/tests

- **V2** · _e2e_ — `e2e_full_pipeline` — covers G1, G2
  - **What it tests:** end-to-end smoke
  - **How:** scripts/e2e.sh
"""


def test_validate_validations_happy_path() -> None:
    errors, warnings = q_goals.validate_validations(GOOD_VALIDATIONS, defined_g=[1, 2])
    assert errors == []
    assert warnings == []


def test_validate_validations_dangling_g_ref() -> None:
    text = """\
## Scenarios

- **V1** · _unit_ — `test_one` — covers G7
  - **What it tests:** something
  - **How:** somewhere
"""
    errors, _ = q_goals.validate_validations(text, defined_g=[1, 2])
    assert any("G7" in e and "undefined" in e for e in errors)


def test_validate_validations_uncovered_goal_is_warning() -> None:
    text = """\
## Scenarios

- **V1** · _unit_ — `test_only_one` — covers G1
  - **What it tests:** thing
  - **How:** path
"""
    errors, warnings = q_goals.validate_validations(text, defined_g=[1, 2])
    assert errors == []
    assert any("G2" in w for w in warnings)


def test_validate_validations_placeholder_left() -> None:
    text = """\
## Scenarios

- **V1** · _unit_ — `test_one` — covers G1
  - **What it tests:** <one line>
  - **How:** somewhere
"""
    errors, _ = q_goals.validate_validations(text, defined_g=[1])
    assert any("placeholder" in e for e in errors)


def test_validate_validations_missing_what() -> None:
    text = """\
## Scenarios

- **V1** · _unit_ — `test_one` — covers G1
  - **How:** path
"""
    errors, _ = q_goals.validate_validations(text, defined_g=[1])
    assert any("What it tests" in e for e in errors)


def test_validate_validations_bad_kind() -> None:
    text = """\
## Scenarios

- **V1** · _bogus_ — `test_one` — covers G1
  - **What it tests:** thing
  - **How:** path
"""
    errors, _ = q_goals.validate_validations(text, defined_g=[1])
    # malformed head → no V-numbers collected, errors about head + missing structure
    assert any("malformed" in e or "no scenario items" in e for e in errors)


# ----------------------------------------------------------------------------
# is_ratified + init_files
# ----------------------------------------------------------------------------

def test_is_ratified_detects_file(tmp_path: Path) -> None:
    assert q_goals.is_ratified(tmp_path) is False
    (tmp_path / ".ratified.json").write_text('{"ratified_at":"x"}')
    assert q_goals.is_ratified(tmp_path) is True


def test_init_files_creates_from_templates(tmp_path: Path) -> None:
    q_goals.init_files(tmp_path, "M-test")
    goals = (tmp_path / "goals.md").read_text()
    validations = (tmp_path / "validations.md").read_text()
    assert "M-test" in goals
    assert "M-test" in validations
    assert "<milestone-id>" not in goals
    assert "<milestone-id>" not in validations


def test_init_files_idempotent(tmp_path: Path) -> None:
    q_goals.init_files(tmp_path, "M-test")
    custom = "## Deliverables\n\n- [ ] **G1** · custom\n  - **Observable when:** a\n  - **Why:** b\n"
    (tmp_path / "goals.md").write_text(custom)
    q_goals.init_files(tmp_path, "M-test")
    assert (tmp_path / "goals.md").read_text() == custom


# ----------------------------------------------------------------------------
# sha256_file
# ----------------------------------------------------------------------------

def test_sha256_file(tmp_path: Path) -> None:
    p = tmp_path / "x.txt"
    p.write_text("hello")
    assert q_goals.sha256_file(p) == "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
