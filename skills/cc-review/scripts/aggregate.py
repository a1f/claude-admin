#!/usr/bin/env python3
"""Aggregate reviewer JSONL outputs across runs and engines into a summary table + per-kind detail comments."""

import argparse
import hashlib
import json
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Final

SEVERITIES: Final[tuple[str, ...]] = ("blocker", "major", "minor", "nit")
SEVERITY_EMOJI: Final[dict[str, str]] = {
    "blocker": "🛑",
    "major": "⚠️",
    "minor": "ℹ️",
    "nit": "·",
}
ENGINE_LABEL: Final[dict[str, str]] = {"claude": "Claude", "codex": "Codex"}


@dataclass(frozen=True, slots=True)
class Finding:
    severity: str
    file: str
    lines: tuple[int, ...]
    desc: str
    suggested_fix: str
    engines: tuple[str, ...] = ()  # filled during merge; empty until then


@dataclass(frozen=True, slots=True)
class ReviewerRun:
    kind: str
    summary: str
    findings: tuple[Finding, ...]


@dataclass(frozen=True, slots=True)
class EngineKindResult:
    runs_used: int
    runs_errored: int
    runs_empty: int
    summaries: tuple[str, ...]
    findings: tuple[Finding, ...]
    counts: dict[str, int]


@dataclass(frozen=True, slots=True)
class EngineKindCollection:
    runs: tuple[ReviewerRun, ...]
    errored: int
    empty: int


@dataclass(frozen=True, slots=True)
class KindResult:
    per_engine: dict[str, EngineKindResult]
    union_findings: tuple[Finding, ...]
    union_counts: dict[str, int]


@dataclass(frozen=True, slots=True)
class RenderedSummary:
    markdown: str
    totals: dict[str, int]


def parse_args() -> argparse.Namespace:
    parser: argparse.ArgumentParser = argparse.ArgumentParser()
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--pr", required=True)
    parser.add_argument("--kinds", required=True)
    parser.add_argument("--runs", required=True, type=int)
    parser.add_argument("--engines", default="claude")
    return parser.parse_args()


def extract_json(*, text: str) -> dict | None:
    """Claude/Codex sometimes wrap JSON in markdown fences; scan for the first balanced object."""
    start: int = text.find("{")
    if start == -1:
        return None
    depth: int = 0
    in_string: bool = False
    escape: bool = False
    for index in range(start, len(text)):
        char: str = text[index]
        if in_string:
            if escape:
                escape = False
            elif char == "\\":
                escape = True
            elif char == '"':
                in_string = False
            continue
        if char == '"':
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                try:
                    return json.loads(text[start : index + 1])
                except json.JSONDecodeError:
                    return None
    return None


def safe_int(*, value: object) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return 0


def parse_finding(*, raw: dict) -> Finding:
    severity: str = raw.get("severity", "nit")
    if severity not in SEVERITIES:
        severity = "nit"
    raw_lines: object = raw.get("lines") or []
    lines: tuple[int, ...] = (
        tuple(safe_int(value=item) for item in raw_lines) if isinstance(raw_lines, list) else ()
    )
    return Finding(
        severity=severity,
        file=str(raw.get("file", "?")),
        lines=lines,
        desc=str(raw.get("desc", "")).strip(),
        suggested_fix=str(raw.get("suggested_fix", "")).strip(),
    )


def load_run(*, log_path: Path, kind: str) -> ReviewerRun | None:
    if not log_path.exists() or log_path.stat().st_size == 0:
        return None
    obj: dict | None = extract_json(text=log_path.read_text(errors="replace"))
    if obj is None or obj.get("_error"):
        return None
    raw_findings: list = obj.get("findings") if isinstance(obj.get("findings"), list) else []
    findings: tuple[Finding, ...] = tuple(
        parse_finding(raw=item) for item in raw_findings if isinstance(item, dict)
    )
    return ReviewerRun(
        kind=str(obj.get("kind", kind)),
        summary=str(obj.get("summary", "")).strip(),
        findings=findings,
    )


def finding_key(*, finding: Finding) -> str:
    """Dedup key: file + first-line + 8-char hash of the first 80 chars of desc."""
    first_line: int = finding.lines[0] if finding.lines else 0
    desc_hash: str = hashlib.sha1(finding.desc[:80].encode()).hexdigest()[:8]
    return f"{finding.file}:{first_line}:{desc_hash}"


def severity_rank(*, severity: str) -> int:
    try:
        return SEVERITIES.index(severity)
    except ValueError:
        return len(SEVERITIES)


def merge_findings(*, tagged: list[tuple[ReviewerRun, str]]) -> tuple[Finding, ...]:
    """Union across (run, engine) pairs, deduped; keep highest severity, record all flagging engines."""
    by_key: dict[str, Finding] = {}
    engines_by_key: defaultdict[str, set[str]] = defaultdict(set)
    for run, engine in tagged:
        for finding in run.findings:
            key: str = finding_key(finding=finding)
            engines_by_key[key].add(engine)
            current: Finding | None = by_key.get(key)
            if current is None or severity_rank(severity=finding.severity) < severity_rank(severity=current.severity):
                by_key[key] = finding

    merged: list[Finding] = [
        Finding(
            severity=finding.severity,
            file=finding.file,
            lines=finding.lines,
            desc=finding.desc,
            suggested_fix=finding.suggested_fix,
            engines=tuple(sorted(engines_by_key[key])),
        )
        for key, finding in by_key.items()
    ]
    merged.sort(
        key=lambda finding: (
            severity_rank(severity=finding.severity),
            finding.file,
            finding.lines[0] if finding.lines else 0,
        )
    )
    return tuple(merged)


def count_by_severity(*, findings: tuple[Finding, ...]) -> dict[str, int]:
    counts: defaultdict[str, int] = defaultdict(int)
    for finding in findings:
        counts[finding.severity] += 1
    return dict(counts)


def collect_engine_kind(*, bundle: Path, kind: str, engine: str, runs_planned: int) -> EngineKindCollection:
    """errored = subprocess sentinel; empty = log missing/unparseable."""
    runs: list[ReviewerRun] = []
    errored: int = 0
    empty: int = 0
    for run_num in range(1, runs_planned + 1):
        log_path: Path = bundle / "logs" / f"{engine}-{kind}-{run_num}.jsonl"
        if not log_path.exists() or log_path.stat().st_size == 0:
            empty += 1
            continue
        obj: dict | None = extract_json(text=log_path.read_text(errors="replace"))
        if obj is None:
            empty += 1
            continue
        if obj.get("_error"):
            errored += 1
            continue
        loaded: ReviewerRun | None = load_run(log_path=log_path, kind=kind)
        if loaded is None:
            empty += 1
        else:
            runs.append(loaded)
    return EngineKindCollection(runs=tuple(runs), errored=errored, empty=empty)


def aggregate_kind(*, bundle: Path, kind: str, engines: list[str], runs_planned: int) -> KindResult:
    per_engine: dict[str, EngineKindResult] = {}
    all_tagged: list[tuple[ReviewerRun, str]] = []
    for engine in engines:
        collection: EngineKindCollection = collect_engine_kind(
            bundle=bundle, kind=kind, engine=engine, runs_planned=runs_planned
        )
        engine_findings: tuple[Finding, ...] = merge_findings(
            tagged=[(run, engine) for run in collection.runs]
        )
        per_engine[engine] = EngineKindResult(
            runs_used=len(collection.runs),
            runs_errored=collection.errored,
            runs_empty=collection.empty,
            summaries=tuple(run.summary for run in collection.runs if run.summary),
            findings=engine_findings,
            counts=count_by_severity(findings=engine_findings),
        )
        all_tagged.extend((run, engine) for run in collection.runs)

    union_findings: tuple[Finding, ...] = merge_findings(tagged=all_tagged)
    return KindResult(
        per_engine=per_engine,
        union_findings=union_findings,
        union_counts=count_by_severity(findings=union_findings),
    )


def severity_cell(*, counts: dict[str, int]) -> str:
    return "/".join(str(counts.get(severity, 0)) for severity in SEVERITIES)


def union_cell(*, counts: dict[str, int]) -> str:
    blocker_count: int = counts.get("blocker", 0)
    rest: str = "/".join(str(counts.get(severity, 0)) for severity in SEVERITIES[1:])
    return f"**{blocker_count}**/{rest}"


def render_summary_md(
    *,
    pr_number: str,
    by_kind: dict[str, KindResult],
    kinds: list[str],
    engines: list[str],
    runs_planned: int,
) -> RenderedSummary:
    multi_engine: bool = len(engines) > 1
    engine_label: str = " + ".join(ENGINE_LABEL.get(engine, engine) for engine in engines)

    lines: list[str] = [
        f"## 🔍 Multi-agent review — PR #{pr_number}",
        "",
        f"_{engine_label} reviewers, {runs_planned} independent run(s) per kind per engine; "
        "findings deduped (union across runs and engines)._",
        "",
    ]

    header_cells: list[str] = ["Kind"] + [f"{ENGINE_LABEL.get(engine, engine)} (b/M/m/n)" for engine in engines]
    if multi_engine:
        header_cells.append("Union (b/M/m/n)")
    lines.append("| " + " | ".join(header_cells) + " |")
    lines.append("|" + "|".join("---" for _ in header_cells) + "|")

    union_totals: defaultdict[str, int] = defaultdict(int)
    for kind in kinds:
        info: KindResult | None = by_kind.get(kind)
        if info is None:
            continue
        row: list[str] = [f"`{kind}`"]
        for engine in engines:
            engine_kind_result: EngineKindResult = info.per_engine[engine]
            runs_marker: str = "" if engine_kind_result.runs_used > 0 else " ⚠"
            row.append(
                f"{severity_cell(counts=engine_kind_result.counts)} "
                f"({engine_kind_result.runs_used}/{runs_planned}{runs_marker})"
            )
        if multi_engine:
            row.append(union_cell(counts=info.union_counts))
            for severity, count in info.union_counts.items():
                union_totals[severity] += count
        else:
            for severity, count in info.per_engine[engines[0]].counts.items():
                union_totals[severity] += count
        lines.append("| " + " | ".join(row) + " |")

    totals_row: list[str] = ["**Total**"] + ["" for _ in engines]
    totals_cell: str = union_cell(counts=dict(union_totals))
    if multi_engine:
        totals_row.append(totals_cell)
    else:
        totals_row[-1] = totals_cell
    lines.append("| " + " | ".join(totals_row) + " |")
    lines.append("")

    if union_totals.get("blocker", 0) > 0:
        lines.append(
            f"**🛑 {union_totals['blocker']} blocker(s) found across engines — `CRITICAL` label applied.**"
        )
    else:
        lines.append("**✅ No blockers.**")
    lines.append("")
    lines.append(
        "Detail comments per kind follow below for any kind with findings. "
        "Each finding is tagged with the engine(s) that flagged it — agreement across engines is a confidence signal."
    )
    return RenderedSummary(markdown="\n".join(lines) + "\n", totals=dict(union_totals))


def render_detail_md(*, pr_number: str, kind: str, info: KindResult, engines: list[str]) -> str:
    if not info.union_findings:
        return ""

    multi_engine: bool = len(engines) > 1
    by_file: defaultdict[str, list[Finding]] = defaultdict(list)
    for finding in info.union_findings:
        by_file[finding.file].append(finding)

    counts: dict[str, int] = info.union_counts
    lines: list[str] = [
        f"### `{kind}` reviewer findings — PR #{pr_number}",
        "",
        f"_Union across engines: blockers {counts.get('blocker', 0)}, "
        f"major {counts.get('major', 0)}, minor {counts.get('minor', 0)}, nit {counts.get('nit', 0)}._",
    ]
    if multi_engine:
        parts: list[str] = []
        for engine in engines:
            engine_counts: dict[str, int] = info.per_engine[engine].counts
            parts.append(
                f"{ENGINE_LABEL.get(engine, engine)}: {engine_counts.get('blocker', 0)} blockers, "
                f"{engine_counts.get('major', 0)} major, "
                f"{engine_counts.get('minor', 0)} minor, "
                f"{engine_counts.get('nit', 0)} nit"
            )
        lines.append(f"_{'; '.join(parts)}._")
    lines.append("")

    for engine in engines:
        summaries: tuple[str, ...] = info.per_engine[engine].summaries
        if summaries:
            lines.append(f"**{ENGINE_LABEL.get(engine, engine)} summaries:**")
            lines.extend(f"- {summary}" for summary in summaries)
            lines.append("")

    for file_path, findings in sorted(by_file.items()):
        lines.append(f"#### `{file_path}`")
        lines.append("")
        for finding in findings:
            lines.extend(render_finding(finding=finding, multi_engine=multi_engine))
        lines.append("")

    return "\n".join(lines) + "\n"


def render_finding(*, finding: Finding, multi_engine: bool) -> list[str]:
    emoji: str = SEVERITY_EMOJI.get(finding.severity, "·")
    anchor: str = line_anchor(lines=finding.lines)
    engine_tag: str = ""
    if multi_engine and finding.engines:
        labels: list[str] = [ENGINE_LABEL.get(engine, engine) for engine in finding.engines]
        engine_tag = f" _[{'+'.join(labels)}]_"
    out: list[str] = [f"- {emoji} **{finding.severity}** {anchor}{engine_tag}".rstrip()]
    if finding.desc:
        out.append(f"  - {finding.desc}")
    if finding.suggested_fix:
        out.append(f"  - _fix:_ {finding.suggested_fix}")
    return out


def line_anchor(*, lines: tuple[int, ...]) -> str:
    if len(lines) >= 2:
        return f"L{lines[0]}–L{lines[1]}"
    if len(lines) == 1:
        return f"L{lines[0]}"
    return ""


def serialize_kind_for_json(*, info: KindResult, engines: list[str]) -> dict:
    return {
        "union": {"counts": dict(info.union_counts)},
        "per_engine": {
            engine: {
                "counts": dict(info.per_engine[engine].counts),
                "runs_used": info.per_engine[engine].runs_used,
                "runs_errored": info.per_engine[engine].runs_errored,
                "runs_empty": info.per_engine[engine].runs_empty,
            }
            for engine in engines
        },
    }


def main() -> int:
    args: argparse.Namespace = parse_args()
    bundle: Path = args.bundle
    if not bundle.is_dir():
        print(f"aggregate: bundle dir not found: {bundle}", file=sys.stderr)
        return 1
    kinds: list[str] = [kind.strip() for kind in args.kinds.split(",") if kind.strip()]
    engines: list[str] = [engine.strip() for engine in args.engines.split(",") if engine.strip()]
    if not kinds:
        print("aggregate: --kinds is empty", file=sys.stderr)
        return 1
    if not engines:
        print("aggregate: --engines is empty", file=sys.stderr)
        return 1

    by_kind: dict[str, KindResult] = {
        kind: aggregate_kind(bundle=bundle, kind=kind, engines=engines, runs_planned=args.runs)
        for kind in kinds
    }
    rendered: RenderedSummary = render_summary_md(
        pr_number=args.pr, by_kind=by_kind, kinds=kinds, engines=engines, runs_planned=args.runs
    )
    (bundle / "summary.md").write_text(rendered.markdown)

    summary_obj: dict = {
        "pr": args.pr,
        "engines": engines,
        "totals": rendered.totals,
        "per_kind": {
            kind: serialize_kind_for_json(info=by_kind[kind], engines=engines)
            for kind in kinds
            if kind in by_kind
        },
    }
    (bundle / "summary.json").write_text(json.dumps(summary_obj, indent=2))

    for kind in kinds:
        info: KindResult | None = by_kind.get(kind)
        if info is None or not info.union_findings:
            continue
        detail_md: str = render_detail_md(pr_number=args.pr, kind=kind, info=info, engines=engines)
        (bundle / f"detail-{kind}.md").write_text(detail_md)

    print(
        f"aggregate: engines={','.join(engines)} kinds={','.join(kinds)} "
        f"total_blockers={rendered.totals.get('blocker', 0)}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
