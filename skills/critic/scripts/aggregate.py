#!/usr/bin/env python3
"""Aggregate critic JSONL outputs into per-engine medians, consensus, and a markdown comment body."""

import argparse
import hashlib
import json
import statistics
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Final

AXES: Final[tuple[str, ...]] = (
    "achieves_goal",
    "test_coverage",
    "no_scope_creep",
    "reuses_existing",
    "validation_evidence",
)
VERDICTS: Final[tuple[str, ...]] = ("reject", "weak", "acceptable", "strong")
VERDICT_EMOJI: Final[dict[str, str]] = {
    "strong": "✅",
    "acceptable": "✅",
    "weak": "⚠️",
    "reject": "🛑",
}
ENGINE_LABEL: Final[dict[str, str]] = {"claude": "Claude", "codex": "Codex"}


@dataclass(frozen=True, slots=True)
class CriticRun:
    score: int
    verdict: str
    axes: dict[str, int]
    rationale_md: str
    concerns: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class EngineResult:
    runs: tuple[CriticRun, ...]
    errored: int
    score: int
    verdict: str
    axes: dict[str, int]
    rationale: str

    @property
    def runs_used(self) -> int:
        return len(self.runs)


@dataclass(frozen=True, slots=True)
class Consensus:
    score: int
    verdict: str
    axes: dict[str, int]
    rationale: str
    concerns: tuple[str, ...]


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--bundle", required=True, type=Path)
    p.add_argument("--pr", required=True)
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


def load_run(*, log_path: Path) -> CriticRun | None:
    if not log_path.exists() or log_path.stat().st_size == 0:
        return None
    obj = extract_json(text=log_path.read_text(errors="replace"))
    if obj is None or obj.get("_error"):
        return None
    try:
        score = int(obj.get("score", 0))
    except (TypeError, ValueError):
        score = 0
    verdict = obj.get("verdict")
    if verdict not in VERDICTS:
        verdict = "reject"
    raw_axes = obj.get("axes") if isinstance(obj.get("axes"), dict) else {}
    raw_concerns = obj.get("concerns") if isinstance(obj.get("concerns"), list) else []
    return CriticRun(
        score=score,
        verdict=verdict,
        axes={a: _safe_int(raw_axes.get(a, 0)) for a in AXES},
        rationale_md=str(obj.get("rationale_md", "")).strip(),
        concerns=tuple(str(c) for c in raw_concerns),
    )


def _safe_int(value: object) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return 0


def verdict_from_score(*, score: int) -> str:
    """Map a score to the skill's own verdict bucket."""
    if score >= 85:
        return "strong"
    if score >= 70:
        return "acceptable"
    if score >= 50:
        return "weak"
    return "reject"


def median_int(*, values: list[int]) -> int:
    if not values:
        return 0
    return int(round(statistics.median(values)))


def dedup_concerns(*, runs: list[CriticRun]) -> tuple[str, ...]:
    """First 120 chars (case-insensitive) define the dedup key — preserves the original casing of the first occurrence."""
    seen: dict[str, str] = {}
    for run in runs:
        for raw in run.concerns:
            text = raw.strip()
            if not text:
                continue
            key = hashlib.sha1(text[:120].lower().encode()).hexdigest()[:10]
            seen.setdefault(key, text)
    return tuple(seen.values())


def representative_rationale(*, runs: list[CriticRun], median_score: int) -> str:
    """Pick the rationale whose score is closest to the median (ties broken by higher score)."""
    if not runs:
        return ""
    chosen = min(runs, key=lambda r: (abs(r.score - median_score), -r.score))
    return chosen.rationale_md


def aggregate_engine(*, bundle: Path, engine: str, runs_planned: int) -> EngineResult:
    runs: list[CriticRun] = []
    errored = 0
    for r in range(1, runs_planned + 1):
        loaded = load_run(log_path=bundle / "logs" / f"{engine}-critic-{r}.jsonl")
        if loaded is None:
            errored += 1
        else:
            runs.append(loaded)

    if not runs:
        return EngineResult(
            runs=(),
            errored=errored,
            score=0,
            verdict="reject",
            axes={a: 0 for a in AXES},
            rationale="All runs failed.",
        )

    score = median_int(values=[r.score for r in runs])
    axes = {a: median_int(values=[r.axes[a] for r in runs]) for a in AXES}
    return EngineResult(
        runs=tuple(runs),
        errored=errored,
        score=score,
        verdict=verdict_from_score(score=score),
        axes=axes,
        rationale=representative_rationale(runs=runs, median_score=score),
    )


def aggregate_consensus(*, per_engine: dict[str, EngineResult]) -> Consensus:
    all_runs: list[CriticRun] = []
    for er in per_engine.values():
        all_runs.extend(er.runs)

    if not all_runs:
        return Consensus(
            score=0,
            verdict="reject",
            axes={a: 0 for a in AXES},
            rationale="All critic runs failed.",
            concerns=(),
        )

    score = median_int(values=[r.score for r in all_runs])
    axes = {a: median_int(values=[r.axes[a] for r in all_runs]) for a in AXES}
    return Consensus(
        score=score,
        verdict=verdict_from_score(score=score),
        axes=axes,
        rationale=representative_rationale(runs=all_runs, median_score=score),
        concerns=dedup_concerns(runs=all_runs),
    )


def render_md(
    *,
    pr: str,
    consensus: Consensus,
    per_engine: dict[str, EngineResult],
    engines: list[str],
    runs_planned: int,
) -> str:
    eng_label = " + ".join(ENGINE_LABEL.get(e, e) for e in engines)
    emoji = VERDICT_EMOJI.get(consensus.verdict, "·")
    multi_engine = len(engines) > 1
    max_runs = max((per_engine[e].runs_used for e in engines), default=0)
    total_errored = sum(per_engine[e].errored for e in engines)

    lines: list[str] = [
        f"## 🎯 Critic verdict — PR #{pr}",
        "",
        f"_{eng_label} critic, {runs_planned} run(s) per engine. "
        "Goal-fit only — code quality is /cc-review's job._",
        "",
        f"### {emoji} **{consensus.verdict.upper()}** — consensus score {consensus.score}/100",
        "",
    ]

    lines.append("### Scores per engine")
    lines.append("")
    run_headers = " | ".join(f"r{i + 1}" for i in range(max_runs)) if max_runs else "—"
    lines.append(f"| Engine | runs | {run_headers} | median | verdict |")
    lines.append("|--------|------|" + "|".join("------" for _ in range(max_runs)) + "|--------|---------|")
    for e in engines:
        er = per_engine[e]
        label = ENGINE_LABEL.get(e, e)
        run_scores = [str(r.score) for r in er.runs] + ["—"] * (max_runs - er.runs_used)
        lines.append(
            f"| {label} | {er.runs_used}/{runs_planned} | "
            f"{' | '.join(run_scores)} | **{er.score}** | {er.verdict} |"
        )
    if multi_engine:
        empty_cells = " | ".join("" for _ in range(max_runs))
        lines.append(
            f"| **Consensus** | | {empty_cells} | **{consensus.score}** | **{consensus.verdict}** |"
        )
    lines.append("")

    lines.append("### Axes (consensus median 0–100)")
    lines.append("")
    lines.append("| Axis | Score |")
    lines.append("|------|-------|")
    lines.extend(f"| `{a}` | {consensus.axes[a]} |" for a in AXES)
    lines.append("")

    if consensus.rationale:
        lines.extend(("### Rationale (representative run)", "", consensus.rationale, ""))

    if consensus.concerns:
        lines.append("### Concerns (union across runs and engines)")
        lines.append("")
        lines.extend(f"- {c}" for c in consensus.concerns)
        lines.append("")

    if total_errored > 0:
        lines.append(f"_⚠ {total_errored} critic run(s) failed or produced unparseable output._")
        lines.append("")

    return "\n".join(lines) + "\n"


def serialize_run(*, run: CriticRun) -> dict:
    """JSON shape must stay backward-compatible with downstream readers (run.sh, golden tests)."""
    return {
        "score": run.score,
        "verdict": run.verdict,
        "axes": dict(run.axes),
        "rationale_md": run.rationale_md,
        "concerns": list(run.concerns),
    }


def serialize_engine(*, engine_result: EngineResult, runs_planned: int) -> dict:
    return {
        "runs_used": engine_result.runs_used,
        "runs_errored": engine_result.errored,
        "score": engine_result.score,
        "verdict": engine_result.verdict,
        "axes": dict(engine_result.axes),
        "per_run": [serialize_run(run=r) for r in engine_result.runs],
    }


def main() -> int:
    args = parse_args()
    bundle: Path = args.bundle
    if not bundle.is_dir():
        print(f"aggregate: bundle dir not found: {bundle}", file=sys.stderr)
        return 1

    engines = [e.strip() for e in args.engines.split(",") if e.strip()]
    if not engines:
        print("aggregate: --engines is empty", file=sys.stderr)
        return 1

    per_engine = {e: aggregate_engine(bundle=bundle, engine=e, runs_planned=args.runs) for e in engines}
    consensus = aggregate_consensus(per_engine=per_engine)
    total_runs_used = sum(per_engine[e].runs_used for e in engines)

    (bundle / "summary.md").write_text(
        render_md(
            pr=args.pr,
            consensus=consensus,
            per_engine=per_engine,
            engines=engines,
            runs_planned=args.runs,
        )
    )
    (bundle / "summary.json").write_text(
        json.dumps(
            {
                "pr": args.pr,
                "engines": engines,
                "runs_per_engine": args.runs,
                "runs_used": total_runs_used,
                "score": consensus.score,
                "verdict": consensus.verdict,
                "axes": dict(consensus.axes),
                "rationale": consensus.rationale,
                "concerns": list(consensus.concerns),
                "per_engine": {
                    e: serialize_engine(engine_result=per_engine[e], runs_planned=args.runs)
                    for e in engines
                },
            },
            indent=2,
        )
    )
    print(
        f"aggregate: engines={','.join(engines)} score={consensus.score} verdict={consensus.verdict}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
