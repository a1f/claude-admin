"""Unit tests for the /analyze cross-artifact gate.

V9 (covers G6 in the M1 PRD): given a PRD with N deliverables but slices
covering only N-1, the report must name the missing one.
"""

from __future__ import annotations

import textwrap

from analyze import (
    AnalyzeReport,
    Goal,
    Kind,
    PRRow,
    Slice,
    Validation,
    analyze,
    parse_goals,
    parse_pr_rows,
    parse_slices,
    parse_validations,
)


# --- Fixtures -----------------------------------------------------------


PRD_3G = textwrap.dedent(
    """
    # toy PRD

    ## summary

    A small fixture.

    ## deliverables

    - [ ] **G1** · install + cc-help (entry point)
    - [ ] **G2** · grill-me (vendor mattpocock)
    - [ ] **G3** · grill-with-docs (doc-freshness check)

    ## validations

    - [ ] **V1** · _e2e_ — `install_smoke` — covers G1
    - [ ] **V2** · _module_ — `grill_repo_study` — covers G2
    - [ ] **V3** · _module_ — `doc_freshness_check` — covers G3
    """
).strip()


SLICES_2_OF_3 = textwrap.dedent(
    """
    # toy slices

    | # | Title | Type | Status | Blocked by | Slice issue |
    |---|---|---|---|---|---|
    | S1 | install + /cc-help skill | AFK | pending | — | _not yet_ |
    | S2 | /grill-me + study phase | AFK | pending | — | _not yet_ |

    ### S1 · install + /cc-help skill — AFK
    **Validations referenced:** V1.

    ### S2 · /grill-me + study phase — AFK
    **Validations referenced:** V2.
    """
).strip()


SLICES_3_OF_3 = SLICES_2_OF_3 + textwrap.dedent(
    """

    ### S3 · /grill-with-docs — AFK
    **Validations referenced:** V3.
    """
)


# --- V9: missing-coverage --------------------------------------------------


def test_v9_drift_detector_names_missing_goal() -> None:
    """V9: PRD has 3 deliverables, slices cover only 2 -> report names G3."""
    report = analyze(artifacts={Kind.PRD: PRD_3G, Kind.SLICES: SLICES_2_OF_3})

    assert len(report.missing_coverage) == 1, report.missing_coverage
    msg = report.missing_coverage[0]
    assert "G3" in msg
    assert "V3" in msg, "should cite the validation that flagged the goal"
    assert not report.drift
    assert not report.inconsistencies
    assert not report.is_clean()


def test_full_coverage_is_clean() -> None:
    report = analyze(artifacts={Kind.PRD: PRD_3G, Kind.SLICES: SLICES_3_OF_3})

    assert report.is_clean()
    assert report.to_markdown().count("_(none)_") == 3


def test_direct_covers_line_satisfies_coverage() -> None:
    slices = textwrap.dedent(
        """
        ### S1 · install — AFK
        **Covers:** G1, G2, G3.
        """
    ).strip()
    report = analyze(artifacts={Kind.PRD: PRD_3G, Kind.SLICES: slices})
    assert report.is_clean()


# --- Drift -----------------------------------------------------------------


def test_drift_slice_references_unknown_validation() -> None:
    slices = textwrap.dedent(
        """
        ### S1 · install — AFK
        **Validations referenced:** V1, V99.
        ### S2 · grill — AFK
        **Validations referenced:** V2.
        ### S3 · docs — AFK
        **Validations referenced:** V3.
        """
    ).strip()
    report = analyze(artifacts={Kind.PRD: PRD_3G, Kind.SLICES: slices})

    assert any("V99" in d and "S1" in d for d in report.drift), report.drift
    assert not report.missing_coverage


def test_drift_validation_covers_unknown_goal() -> None:
    prd = PRD_3G + "\n- [ ] **V9** · _unit_ — `dangling` — covers G99\n"
    report = analyze(artifacts={Kind.PRD: prd, Kind.SLICES: SLICES_3_OF_3})

    assert any("V9" in d and "G99" in d for d in report.drift), report.drift


def test_drift_pr_row_references_unknown_slice() -> None:
    pr_table = textwrap.dedent(
        """
        | id | title | status | link |
        |---|---|---|---|
        | PR1 | S1 / install | open | http://x |
        | PR2 | S99 / mystery | open | http://x |
        """
    ).strip()
    report = analyze(
        artifacts={
            Kind.PRD: PRD_3G,
            Kind.SLICES: SLICES_3_OF_3,
            Kind.PR_TABLE: pr_table,
        }
    )
    assert any("S99" in d and "PR2" in d for d in report.drift), report.drift


# --- Inconsistencies -------------------------------------------------------


def test_inconsistency_g_numbering_gap() -> None:
    prd = textwrap.dedent(
        """
        ## deliverables

        - [ ] **G1** · first
        - [ ] **G3** · third

        ## validations
        """
    ).strip()
    report = analyze(artifacts={Kind.PRD: prd})
    assert any("G2" in x for x in report.inconsistencies), report.inconsistencies


def test_inconsistency_s_numbering_gap() -> None:
    slices = textwrap.dedent(
        """
        | # | Title | Type | Status | Blocked by | Slice |
        |---|---|---|---|---|---|
        | S1 | a | AFK | pending | — | — |
        | S3 | c | AFK | pending | — | — |
        """
    ).strip()
    report = analyze(artifacts={Kind.PRD: PRD_3G, Kind.SLICES: slices})
    assert any("S2" in x for x in report.inconsistencies), report.inconsistencies


# --- Idempotence -----------------------------------------------------------


def test_idempotence_same_input_same_report() -> None:
    a = analyze(artifacts={Kind.PRD: PRD_3G, Kind.SLICES: SLICES_2_OF_3})
    b = analyze(artifacts={Kind.PRD: PRD_3G, Kind.SLICES: SLICES_2_OF_3})
    assert a.to_markdown() == b.to_markdown()


def test_empty_inputs_clean() -> None:
    assert analyze(artifacts={}).is_clean()


# --- Parser smoke tests ----------------------------------------------------


def test_parse_goals_extracts_id_and_title() -> None:
    goals = parse_goals(prd_body=PRD_3G)
    assert [g.id for g in goals] == ["G1", "G2", "G3"]
    assert goals[0].title.startswith("install")


def test_parse_validations_extracts_covers() -> None:
    vs = parse_validations(prd_body=PRD_3G)
    assert [v.id for v in vs] == ["V1", "V2", "V3"]
    assert vs[0].covers == ("G1",)


def test_parse_validations_covers_range() -> None:
    prd = textwrap.dedent(
        """
        ## validations

        - [ ] **V2** · _e2e_ — `big` — covers G1-G3
        """
    ).strip()
    vs = parse_validations(prd_body=prd)
    assert vs[0].covers == ("G1", "G2", "G3")


def test_parse_slices_picks_up_validations_referenced_line() -> None:
    slices = parse_slices(body=SLICES_3_OF_3)
    by_id = {s.id: s for s in slices}
    assert by_id["S1"].validations == ("V1",)
    assert by_id["S3"].validations == ("V3",)


def test_parse_pr_rows_skips_header_and_separator() -> None:
    pr_table = textwrap.dedent(
        """
        | id | title | status | link |
        |---|---|---|---|
        | PR1 | S1 / install | open | http://x |
        """
    ).strip()
    rows = parse_pr_rows(body=pr_table)
    assert len(rows) == 1
    assert rows[0].id == "PR1"
    assert rows[0].slice_id == "S1"


# --- Report formatting -----------------------------------------------------


def test_report_markdown_sorted() -> None:
    rep = AnalyzeReport(
        missing_coverage=["zebra", "apple"],
        drift=["beta", "alpha"],
        inconsistencies=[],
    )
    md = rep.to_markdown()
    apple_pos = md.index("apple")
    zebra_pos = md.index("zebra")
    assert apple_pos < zebra_pos
    assert md.count("_(none)_") == 1


# --- Dataclass sanity ------------------------------------------------------


def test_dataclasses_are_constructible() -> None:
    g = Goal(id="G1", title="x")
    v = Validation(id="V1", title="y", covers=("G1",))
    s = Slice(id="S1", title="z", validations=("V1",), covers=(), status="pending")
    pr = PRRow(id="PR1", slice_id="S1")
    assert (g.id, v.covers, s.status, pr.slice_id) == ("G1", ("G1",), "pending", "S1")
