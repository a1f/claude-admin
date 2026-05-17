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
class KindResult:
    per_engine: dict[str, EngineKindResult]
    union_findings: tuple[Finding, ...]
    union_counts: dict[str, int]


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--bundle", required=True, type=Path)
    p.add_argument("--pr", required=True)
    p.add_argument("--kinds", required=True)
    p.add_argument("--runs", required=True, type=int)
    p.add_argument("--engines", default="claude")
    return p.parse_args()


def extract_json(*, text: str) -> dict | None:
    """Claude/Codex sometimes wrap JSON in markdown fences; scan for the first balanced object."""
    start = text.find("{")
    if start == -1:
        return None
    depth = 0
    in_str = False
    escape = False
    for i in range(start, len(text)):
        ch = text[i]
        if in_str:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_str = False
            continue
        if ch == '"':
            in_str = True
        elif ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                try:
                    return json.loads(text[start : i + 1])
                except json.JSONDecodeError:
                    return None
    return None


def load_run(*, log_path: Path, kind: str) -> ReviewerRun | None:
    if not log_path.exists() or log_path.stat().st_size == 0:
        return None
    obj = extract_json(text=log_path.read_text(errors="replace"))
    if obj is None or obj.get("_error"):
        return None
    raw_findings = obj.get("findings") if isinstance(obj.get("findings"), list) else []
    return ReviewerRun(
        kind=str(obj.get("kind", kind)),
        summary=str(obj.get("summary", "")).strip(),
        findings=tuple(_parse_finding(raw=f) for f in raw_findings if isinstance(f, dict)),
    )


def _parse_finding(*, raw: dict) -> Finding:
    sev = raw.get("severity", "nit")
    if sev not in SEVERITIES:
        sev = "nit"
    raw_lines = raw.get("lines") or []
    lines = tuple(_safe_int(value=v) for v in raw_lines) if isinstance(raw_lines, list) else ()
    return Finding(
        severity=sev,
        file=str(raw.get("file", "?")),
        lines=lines,
        desc=str(raw.get("desc", "")).strip(),
        suggested_fix=str(raw.get("suggested_fix", "")).strip(),
    )


def _safe_int(*, value: object) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return 0


def finding_key(*, finding: Finding) -> str:
    """Dedup key: file + first-line + 8-char hash of the first 80 chars of desc."""
    first_line = finding.lines[0] if finding.lines else 0
    desc_hash = hashlib.sha1(finding.desc[:80].encode()).hexdigest()[:8]
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
        for f in run.findings:
            key = finding_key(finding=f)
            engines_by_key[key].add(engine)
            current = by_key.get(key)
            if current is None or severity_rank(severity=f.severity) < severity_rank(severity=current.severity):
                by_key[key] = f

    merged = [
        Finding(
            severity=f.severity,
            file=f.file,
            lines=f.lines,
            desc=f.desc,
            suggested_fix=f.suggested_fix,
            engines=tuple(sorted(engines_by_key[key])),
        )
        for key, f in by_key.items()
    ]
    merged.sort(key=lambda f: (severity_rank(severity=f.severity), f.file, f.lines[0] if f.lines else 0))
    return tuple(merged)


def count_by_severity(*, findings: tuple[Finding, ...]) -> dict[str, int]:
    counts: defaultdict[str, int] = defaultdict(int)
    for f in findings:
        counts[f.severity] += 1
    return dict(counts)


def collect_engine_kind(
    *, bundle: Path, kind: str, engine: str, runs_planned: int
) -> tuple[list[ReviewerRun], int, int]:
    """Returns (runs, errored_count, empty_count); errored = subprocess sentinel, empty = missing/unparseable."""
    runs: list[ReviewerRun] = []
    errored = 0
    empty = 0
    for r in range(1, runs_planned + 1):
        log_path = bundle / "logs" / f"{engine}-{kind}-{r}.jsonl"
        if not log_path.exists() or log_path.stat().st_size == 0:
            empty += 1
            continue
        obj = extract_json(text=log_path.read_text(errors="replace"))
        if obj is None:
            empty += 1
            continue
        if obj.get("_error"):
            errored += 1
            continue
        loaded = load_run(log_path=log_path, kind=kind)
        if loaded is None:
            empty += 1
        else:
            runs.append(loaded)
    return runs, errored, empty


def aggregate_kind(*, bundle: Path, kind: str, engines: list[str], runs_planned: int) -> KindResult:
    per_engine: dict[str, EngineKindResult] = {}
    all_tagged: list[tuple[ReviewerRun, str]] = []
    for engine in engines:
        runs, errored, empty = collect_engine_kind(
            bundle=bundle, kind=kind, engine=engine, runs_planned=runs_planned
        )
        engine_findings = merge_findings(tagged=[(r, engine) for r in runs])
        per_engine[engine] = EngineKindResult(
            runs_used=len(runs),
            runs_errored=errored,
            runs_empty=empty,
            summaries=tuple(r.summary for r in runs if r.summary),
            findings=engine_findings,
            counts=count_by_severity(findings=engine_findings),
        )
        all_tagged.extend((r, engine) for r in runs)

    union_findings = merge_findings(tagged=all_tagged)
    return KindResult(
        per_engine=per_engine,
        union_findings=union_findings,
        union_counts=count_by_severity(findings=union_findings),
    )


def _sev_cell(*, counts: dict[str, int]) -> str:
    return "/".join(str(counts.get(s, 0)) for s in SEVERITIES)


def _union_cell(*, counts: dict[str, int]) -> str:
    blocker = counts.get("blocker", 0)
    rest = "/".join(str(counts.get(s, 0)) for s in SEVERITIES[1:])
    return f"**{blocker}**/{rest}"


def render_summary_md(
    *,
    pr: str,
    by_kind: dict[str, KindResult],
    kinds: list[str],
    engines: list[str],
    runs_planned: int,
) -> tuple[str, dict[str, int]]:
    multi_engine = len(engines) > 1
    eng_label = " + ".join(ENGINE_LABEL.get(e, e) for e in engines)

    lines: list[str] = [
        f"## 🔍 Multi-agent review — PR #{pr}",
        "",
        f"_{eng_label} reviewers, {runs_planned} independent run(s) per kind per engine; "
        "findings deduped (union across runs and engines)._",
        "",
    ]

    header_cells = ["Kind"] + [f"{ENGINE_LABEL.get(e, e)} (b/M/m/n)" for e in engines]
    if multi_engine:
        header_cells.append("Union (b/M/m/n)")
    lines.append("| " + " | ".join(header_cells) + " |")
    lines.append("|" + "|".join("---" for _ in header_cells) + "|")

    union_totals: defaultdict[str, int] = defaultdict(int)
    for kind in kinds:
        info = by_kind.get(kind)
        if info is None:
            continue
        row = [f"`{kind}`"]
        for engine in engines:
            ekr = info.per_engine[engine]
            ru_marker = "" if ekr.runs_used > 0 else " ⚠"
            row.append(f"{_sev_cell(counts=ekr.counts)} ({ekr.runs_used}/{runs_planned}{ru_marker})")
        if multi_engine:
            row.append(_union_cell(counts=info.union_counts))
            for sev, n in info.union_counts.items():
                union_totals[sev] += n
        else:
            for sev, n in info.per_engine[engines[0]].counts.items():
                union_totals[sev] += n
        lines.append("| " + " | ".join(row) + " |")

    totals_row = ["**Total**"] + ["" for _ in engines]
    totals_cell = _union_cell(counts=dict(union_totals))
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
    return "\n".join(lines) + "\n", dict(union_totals)


def render_detail_md(*, pr: str, kind: str, info: KindResult, engines: list[str]) -> str:
    if not info.union_findings:
        return ""

    multi_engine = len(engines) > 1
    by_file: defaultdict[str, list[Finding]] = defaultdict(list)
    for f in info.union_findings:
        by_file[f.file].append(f)

    uc = info.union_counts
    lines: list[str] = [
        f"### `{kind}` reviewer findings — PR #{pr}",
        "",
        f"_Union across engines: blockers {uc.get('blocker', 0)}, "
        f"major {uc.get('major', 0)}, minor {uc.get('minor', 0)}, nit {uc.get('nit', 0)}._",
    ]
    if multi_engine:
        parts = []
        for e in engines:
            c = info.per_engine[e].counts
            parts.append(
                f"{ENGINE_LABEL.get(e, e)}: {c.get('blocker', 0)} blockers, "
                f"{c.get('major', 0)} major, {c.get('minor', 0)} minor, {c.get('nit', 0)} nit"
            )
        lines.append(f"_{'; '.join(parts)}._")
    lines.append("")

    for engine in engines:
        summaries = info.per_engine[engine].summaries
        if summaries:
            lines.append(f"**{ENGINE_LABEL.get(engine, engine)} summaries:**")
            lines.extend(f"- {s}" for s in summaries)
            lines.append("")

    for file_path, findings in sorted(by_file.items()):
        lines.append(f"#### `{file_path}`")
        lines.append("")
        for f in findings:
            lines.extend(_render_finding(finding=f, multi_engine=multi_engine))
        lines.append("")

    return "\n".join(lines) + "\n"


def _render_finding(*, finding: Finding, multi_engine: bool) -> list[str]:
    emoji = SEVERITY_EMOJI.get(finding.severity, "·")
    anchor = _line_anchor(lines=finding.lines)
    engine_tag = ""
    if multi_engine and finding.engines:
        labels = [ENGINE_LABEL.get(e, e) for e in finding.engines]
        engine_tag = f" _[{'+'.join(labels)}]_"
    out = [f"- {emoji} **{finding.severity}** {anchor}{engine_tag}".rstrip()]
    if finding.desc:
        out.append(f"  - {finding.desc}")
    if finding.suggested_fix:
        out.append(f"  - _fix:_ {finding.suggested_fix}")
    return out


def _line_anchor(*, lines: tuple[int, ...]) -> str:
    if len(lines) >= 2:
        return f"L{lines[0]}–L{lines[1]}"
    if len(lines) == 1:
        return f"L{lines[0]}"
    return ""


def serialize_kind_for_json(*, info: KindResult, engines: list[str]) -> dict:
    return {
        "union": {"counts": dict(info.union_counts)},
        "per_engine": {
            e: {
                "counts": dict(info.per_engine[e].counts),
                "runs_used": info.per_engine[e].runs_used,
                "runs_errored": info.per_engine[e].runs_errored,
                "runs_empty": info.per_engine[e].runs_empty,
            }
            for e in engines
        },
    }


def main() -> int:
    args = parse_args()
    bundle: Path = args.bundle
    if not bundle.is_dir():
        print(f"aggregate: bundle dir not found: {bundle}", file=sys.stderr)
        return 1
    kinds = [k.strip() for k in args.kinds.split(",") if k.strip()]
    engines = [e.strip() for e in args.engines.split(",") if e.strip()]
    if not kinds:
        print("aggregate: --kinds is empty", file=sys.stderr)
        return 1
    if not engines:
        print("aggregate: --engines is empty", file=sys.stderr)
        return 1

    by_kind = {
        k: aggregate_kind(bundle=bundle, kind=k, engines=engines, runs_planned=args.runs)
        for k in kinds
    }
    summary_md, totals = render_summary_md(
        pr=args.pr, by_kind=by_kind, kinds=kinds, engines=engines, runs_planned=args.runs
    )
    (bundle / "summary.md").write_text(summary_md)
    (bundle / "summary.json").write_text(
        json.dumps(
            {
                "pr": args.pr,
                "engines": engines,
                "totals": totals,
                "per_kind": {
                    k: serialize_kind_for_json(info=by_kind[k], engines=engines)
                    for k in kinds
                    if k in by_kind
                },
            },
            indent=2,
        )
    )
    for kind in kinds:
        info = by_kind.get(kind)
        if info is None or not info.union_findings:
            continue
        (bundle / f"detail-{kind}.md").write_text(
            render_detail_md(pr=args.pr, kind=kind, info=info, engines=engines)
        )

    print(
        f"aggregate: engines={','.join(engines)} kinds={','.join(kinds)} "
        f"total_blockers={totals.get('blocker', 0)}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
