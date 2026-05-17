#!/usr/bin/env python3
"""Cross-artifact consistency gate for the M1 skills pipeline.

CLI:
    python3 analyze.py [--prd REF] [--slices REF]
                       [--pr-table REF] [--roadmap REF]
                       [--format markdown|json]

REF is a local path, "gh:OWNER/REPO#N", or a full GitHub issue URL.
Exit codes: 0 clean, 1 issues reported, 2 argument/fetch error.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from re import Pattern
from typing import Final


# --- Exceptions ----------------------------------------------------------


class AnalyzeError(Exception):
    """Base for /analyze errors."""


class ArtifactFetchError(AnalyzeError):
    """Failed to fetch an artifact by ref."""


# --- Types ---------------------------------------------------------------


class Kind(str, Enum):
    ROADMAP = "roadmap"
    PRD = "prd"
    SLICES = "slices"
    PR_TABLE = "pr_table"


@dataclass(frozen=True, kw_only=True)
class Goal:
    id: str
    title: str


@dataclass(frozen=True, kw_only=True)
class Validation:
    id: str
    title: str
    covers: tuple[str, ...]


@dataclass(frozen=True, kw_only=True)
class Slice:
    id: str
    title: str
    validations: tuple[str, ...]
    covers: tuple[str, ...]
    status: str | None


@dataclass(frozen=True, kw_only=True)
class PRRow:
    id: str
    slice_id: str | None


@dataclass(kw_only=True)
class AnalyzeReport:
    missing_coverage: list[str] = field(default_factory=list)
    drift: list[str] = field(default_factory=list)
    inconsistencies: list[str] = field(default_factory=list)

    def is_clean(self) -> bool:
        return not (self.missing_coverage or self.drift or self.inconsistencies)

    def to_markdown(self) -> str:
        return (
            "# /analyze report\n\n"
            + _md_section(title="missing-coverage", items=self.missing_coverage)
            + "\n"
            + _md_section(title="drift", items=self.drift)
            + "\n"
            + _md_section(title="inconsistencies", items=self.inconsistencies)
        )


# --- Regex constants -----------------------------------------------------

GOAL_RE: Final[Pattern[str]] = re.compile(
    r"^\s*-\s*\[[ x]\]\s*\*\*G(\d+)\*\*\s*[·–\-]?\s*(.+?)\s*$",
    re.MULTILINE,
)
VALIDATION_RE: Final[Pattern[str]] = re.compile(
    r"^\s*-\s*\[[ x]\]\s*\*\*V(\d+)\*\*\s*[·–\-]?\s*(.+?)\s*$",
    re.MULTILINE,
)
COVERS_INLINE_RE: Final[Pattern[str]] = re.compile(
    r"covers\s+([G\d, –\-]+)", re.IGNORECASE
)
G_REF_RE: Final[Pattern[str]] = re.compile(r"G(\d+)")
V_REF_RE: Final[Pattern[str]] = re.compile(r"V(\d+)")
G_RANGE_RE: Final[Pattern[str]] = re.compile(r"G(\d+)\s*[–\-]\s*G?(\d+)")

SLICE_TABLE_ROW_RE: Final[Pattern[str]] = re.compile(
    r"^\|\s*S(\d+)\s*\|\s*(.+?)\s*\|[^|]*\|\s*(\S+)\s*\|",
    re.MULTILINE,
)
SLICE_HEADING_RE: Final[Pattern[str]] = re.compile(
    r"^###\s+S(\d+)\s*·\s*(.+?)\s*(?:—.+)?$",
    re.MULTILINE,
)
VALIDATIONS_REF_RE: Final[Pattern[str]] = re.compile(
    r"\*\*Validations referenced:\*\*\s*(.+?)\s*$", re.MULTILINE
)
COVERS_LINE_RE: Final[Pattern[str]] = re.compile(
    r"\*\*Covers:\*\*\s*(.+?)\s*$", re.MULTILINE
)

PR_ROW_RE: Final[Pattern[str]] = re.compile(
    r"^\|\s*([A-Za-z][\w\-]*\d+|PR\d+)\s*\|\s*(.+?)\s*\|",
    re.MULTILINE,
)

GH_REF_RE: Final[Pattern[str]] = re.compile(r"^gh:([^#]+)#(\d+)$")
GH_URL_RE: Final[Pattern[str]] = re.compile(
    r"^https?://github\.com/([^/]+/[^/]+)/issues/(\d+)/?$"
)


# --- Parsers -------------------------------------------------------------


def _section(*, body: str, name: str) -> str:
    pat = re.compile(rf"^##\s+{re.escape(name)}\b.*?$", re.IGNORECASE | re.MULTILINE)
    m = pat.search(body)
    if not m:
        return ""
    start = m.end()
    nxt = re.search(r"^##\s+\S", body[start:], re.MULTILINE)
    return body[start : start + nxt.start()] if nxt else body[start:]


def _expand_covers(*, token: str) -> list[str]:
    """Authors may write 'covers G1, G3-G5' — expand into individual goal ids."""
    out: list[str] = []
    for chunk in token.split(","):
        chunk = chunk.strip()
        m = G_RANGE_RE.match(chunk)
        if m:
            lo, hi = int(m.group(1)), int(m.group(2))
            out.extend(f"G{i}" for i in range(lo, hi + 1))
            continue
        for g in G_REF_RE.findall(chunk):
            out.append(f"G{int(g)}")
    return list(dict.fromkeys(out))


def parse_goals(*, prd_body: str) -> list[Goal]:
    section = _section(body=prd_body, name="deliverables")
    out: list[Goal] = []
    seen: set[str] = set()
    for m in GOAL_RE.finditer(section):
        gid = f"G{int(m.group(1))}"
        if gid in seen:
            continue
        seen.add(gid)
        out.append(Goal(id=gid, title=m.group(2).strip()))
    return out


def parse_validations(*, prd_body: str) -> list[Validation]:
    section = _section(body=prd_body, name="validations")
    out: list[Validation] = []
    seen: set[str] = set()
    matches = list(VALIDATION_RE.finditer(section))
    for i, m in enumerate(matches):
        vid = f"V{int(m.group(1))}"
        if vid in seen:
            continue
        seen.add(vid)
        start = m.end()
        end = matches[i + 1].start() if i + 1 < len(matches) else len(section)
        chunk = m.group(0) + "\n" + section[start:end]
        covers: list[str] = []
        for cm in COVERS_INLINE_RE.finditer(chunk):
            covers.extend(_expand_covers(token=cm.group(1)))
        out.append(
            Validation(
                id=vid,
                title=m.group(2).strip(),
                covers=tuple(dict.fromkeys(covers)),
            )
        )
    return out


def parse_slices(*, body: str) -> list[Slice]:
    discovered: dict[str, dict[str, str | None]] = {}
    for m in SLICE_TABLE_ROW_RE.finditer(body):
        sid = f"S{int(m.group(1))}"
        discovered.setdefault(
            sid, {"title": m.group(2).strip(), "status": m.group(3).strip()}
        )
    for m in SLICE_HEADING_RE.finditer(body):
        sid = f"S{int(m.group(1))}"
        discovered.setdefault(sid, {"title": m.group(2).strip(), "status": None})

    headings = list(SLICE_HEADING_RE.finditer(body))
    sections: dict[str, str] = {}
    for i, m in enumerate(headings):
        sid = f"S{int(m.group(1))}"
        start = m.end()
        end = headings[i + 1].start() if i + 1 < len(headings) else len(body)
        sections[sid] = body[start:end]

    out: list[Slice] = []
    for sid in sorted(discovered, key=lambda s: int(s[1:])):
        info = discovered[sid]
        chunk = sections.get(sid, "")
        vs: list[str] = []
        for vm in VALIDATIONS_REF_RE.finditer(chunk):
            vs.extend(f"V{int(v)}" for v in V_REF_RE.findall(vm.group(1)))
        gs: list[str] = []
        for cm in COVERS_LINE_RE.finditer(chunk):
            gs.extend(_expand_covers(token=cm.group(1)))
        out.append(
            Slice(
                id=sid,
                title=str(info["title"]),
                validations=tuple(dict.fromkeys(vs)),
                covers=tuple(dict.fromkeys(gs)),
                status=info["status"],
            )
        )
    return out


def parse_pr_rows(*, body: str) -> list[PRRow]:
    rows: list[PRRow] = []
    for m in PR_ROW_RE.finditer(body):
        pid = m.group(1)
        slice_m = re.search(r"\bS(\d+)\b", m.group(0))
        sid = f"S{int(slice_m.group(1))}" if slice_m else None
        rows.append(PRRow(id=pid, slice_id=sid))
    return rows


# --- Detectors -----------------------------------------------------------


def _numbering_gaps(*, label: str, ids: set[str]) -> list[str]:
    if not ids:
        return []
    nums = sorted(int(x[1:]) for x in ids)
    return [
        f"{label} numbering gap: {label}{n} missing (have {label}1..{label}{nums[-1]})"
        for n in range(1, nums[-1] + 1)
        if n not in nums
    ]


def _slice_covers_goal(
    *, slice_: Slice, gid: str, val_by_id: dict[str, Validation]
) -> bool:
    if gid in slice_.covers:
        return True
    return any(
        (v := val_by_id.get(vid)) and gid in v.covers for vid in slice_.validations
    )


def _detect_missing_coverage(
    *, goals: list[Goal], validations: list[Validation], slices: list[Slice]
) -> list[str]:
    if not (goals and slices):
        return []
    val_by_id: dict[str, Validation] = {v.id: v for v in validations}
    out: list[str] = []
    for g in goals:
        if any(
            _slice_covers_goal(slice_=s, gid=g.id, val_by_id=val_by_id) for s in slices
        ):
            continue
        vs_for_g = [v.id for v in validations if g.id in v.covers]
        tag = f" (referenced by {', '.join(vs_for_g)})" if vs_for_g else ""
        out.append(f"no slice covers {g.id}{tag}: {g.title}")
    return out


def _detect_drift(
    *,
    goals: list[Goal],
    validations: list[Validation],
    slices: list[Slice],
    prs: list[PRRow],
) -> list[str]:
    goal_ids: set[str] = {g.id for g in goals}
    val_ids: set[str] = {v.id for v in validations}
    slice_ids: set[str] = {s.id for s in slices}
    out: list[str] = []
    if goals:
        for v in validations:
            out.extend(
                f"{v.id} covers {gid} but {gid} not in PRD"
                for gid in v.covers
                if gid not in goal_ids
            )
    if validations:
        for s in slices:
            out.extend(
                f"slice {s.id} references {vid} not in PRD"
                for vid in s.validations
                if vid not in val_ids
            )
    if goals:
        for s in slices:
            out.extend(
                f"slice {s.id} covers {gid} not in PRD"
                for gid in s.covers
                if gid not in goal_ids
            )
    if slices:
        out.extend(
            f"PR {pr.id} references {pr.slice_id} which is not in the slice list"
            for pr in prs
            if pr.slice_id and pr.slice_id not in slice_ids
        )
    return out


# --- Top-level analyzer --------------------------------------------------


def analyze(*, artifacts: dict[Kind, str]) -> AnalyzeReport:
    prd = artifacts.get(Kind.PRD, "")
    slices_body = artifacts.get(Kind.SLICES, "")
    pr_body = artifacts.get(Kind.PR_TABLE, "")

    goals = parse_goals(prd_body=prd) if prd else []
    validations = parse_validations(prd_body=prd) if prd else []
    slices = parse_slices(body=slices_body) if slices_body else []
    prs = parse_pr_rows(body=pr_body) if pr_body else []

    report = AnalyzeReport(
        missing_coverage=_detect_missing_coverage(
            goals=goals, validations=validations, slices=slices
        ),
        drift=_detect_drift(
            goals=goals, validations=validations, slices=slices, prs=prs
        ),
        inconsistencies=(
            _numbering_gaps(label="G", ids={g.id for g in goals})
            + _numbering_gaps(label="V", ids={v.id for v in validations})
            + _numbering_gaps(label="S", ids={s.id for s in slices})
        ),
    )
    report.missing_coverage = sorted(set(report.missing_coverage))
    report.drift = sorted(set(report.drift))
    report.inconsistencies = sorted(set(report.inconsistencies))
    return report


# --- I/O -----------------------------------------------------------------


def _md_section(*, title: str, items: list[str]) -> str:
    if not items:
        return f"### {title}\n_(none)_\n"
    body = "\n".join(f"- {x}" for x in sorted(items))
    return f"### {title}\n{body}\n"


def fetch(*, ref: str) -> str:
    m = GH_REF_RE.match(ref) or GH_URL_RE.match(ref)
    if m:
        repo, num = m.group(1), m.group(2)
        proc = subprocess.run(
            [
                "gh",
                "issue",
                "view",
                num,
                "--repo",
                repo,
                "--json",
                "body",
                "-q",
                ".body",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode != 0:
            raise ArtifactFetchError(
                f"gh issue view {repo}#{num} failed: {proc.stderr.strip()}"
            )
        return proc.stdout
    p = Path(ref)
    if not p.exists():
        raise ArtifactFetchError(f"artifact not found: {ref}")
    return p.read_text()


def main(*, argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Cross-artifact consistency gate")
    ap.add_argument("--prd")
    ap.add_argument("--slices")
    ap.add_argument("--pr-table", dest="pr_table")
    ap.add_argument("--roadmap")
    ap.add_argument("--format", choices=("markdown", "json"), default="markdown")
    args = ap.parse_args(argv)

    artifacts: dict[Kind, str] = {}
    try:
        for kind, val in (
            (Kind.ROADMAP, args.roadmap),
            (Kind.PRD, args.prd),
            (Kind.SLICES, args.slices),
            (Kind.PR_TABLE, args.pr_table),
        ):
            if val:
                artifacts[kind] = fetch(ref=val)
    except AnalyzeError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2

    if not artifacts:
        print(
            "error: at least one artifact required "
            "(--prd, --slices, --pr-table, --roadmap)",
            file=sys.stderr,
        )
        return 2

    report = analyze(artifacts=artifacts)
    if args.format == "json":
        print(
            json.dumps(
                {
                    "missing_coverage": report.missing_coverage,
                    "drift": report.drift,
                    "inconsistencies": report.inconsistencies,
                    "clean": report.is_clean(),
                },
                indent=2,
                sort_keys=True,
            )
        )
    else:
        print(report.to_markdown())

    return 0 if report.is_clean() else 1


if __name__ == "__main__":
    sys.exit(main())
