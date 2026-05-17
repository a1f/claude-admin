#!/usr/bin/env python3
"""Aggregate critic JSONL outputs into per-engine medians, consensus, and a markdown comment body."""

import argparse
import hashlib
import json
import statistics
import sys
from dataclasses import dataclass
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
    parser: argparse.ArgumentParser = argparse.ArgumentParser()
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--pr", required=True)
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


def load_run(*, log_path: Path) -> CriticRun | None:
    if not log_path.exists() or log_path.stat().st_size == 0:
        return None
    obj: dict | None = extract_json(text=log_path.read_text(errors="replace"))
    if obj is None or obj.get("_error"):
        return None
    score: int = safe_int(value=obj.get("score", 0))
    verdict: str = obj.get("verdict") if obj.get("verdict") in VERDICTS else "reject"
    raw_axes: dict = obj.get("axes") if isinstance(obj.get("axes"), dict) else {}
    raw_concerns: list = obj.get("concerns") if isinstance(obj.get("concerns"), list) else []
    axes: dict[str, int] = {axis: safe_int(value=raw_axes.get(axis, 0)) for axis in AXES}
    concerns: tuple[str, ...] = tuple(str(concern) for concern in raw_concerns)
    rationale_md: str = str(obj.get("rationale_md", "")).strip()
    return CriticRun(
        score=score,
        verdict=verdict,
        axes=axes,
        rationale_md=rationale_md,
        concerns=concerns,
    )


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
    """First 120 chars (case-insensitive) define the dedup key — preserves the casing of the first occurrence."""
    seen: dict[str, str] = {}
    for run in runs:
        for raw in run.concerns:
            text: str = raw.strip()
            if not text:
                continue
            key: str = hashlib.sha1(text[:120].lower().encode()).hexdigest()[:10]
            seen.setdefault(key, text)
    return tuple(seen.values())


def representative_rationale(*, runs: list[CriticRun], median_score: int) -> str:
    """Pick the rationale whose score is closest to the median (ties broken by higher score)."""
    if not runs:
        return ""
    chosen: CriticRun = min(runs, key=lambda candidate: (abs(candidate.score - median_score), -candidate.score))
    return chosen.rationale_md


def aggregate_engine(*, bundle: Path, engine: str, runs_planned: int) -> EngineResult:
    runs: list[CriticRun] = []
    errored: int = 0
    for run_num in range(1, runs_planned + 1):
        log_path: Path = bundle / "logs" / f"{engine}-critic-{run_num}.jsonl"
        loaded: CriticRun | None = load_run(log_path=log_path)
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
            axes={axis: 0 for axis in AXES},
            rationale="All runs failed.",
        )

    score: int = median_int(values=[run.score for run in runs])
    axes: dict[str, int] = {axis: median_int(values=[run.axes[axis] for run in runs]) for axis in AXES}
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
    for engine_result in per_engine.values():
        all_runs.extend(engine_result.runs)

    if not all_runs:
        return Consensus(
            score=0,
            verdict="reject",
            axes={axis: 0 for axis in AXES},
            rationale="All critic runs failed.",
            concerns=(),
        )

    score: int = median_int(values=[run.score for run in all_runs])
    axes: dict[str, int] = {axis: median_int(values=[run.axes[axis] for run in all_runs]) for axis in AXES}
    return Consensus(
        score=score,
        verdict=verdict_from_score(score=score),
        axes=axes,
        rationale=representative_rationale(runs=all_runs, median_score=score),
        concerns=dedup_concerns(runs=all_runs),
    )


def render_md(
    *,
    pr_number: str,
    consensus: Consensus,
    per_engine: dict[str, EngineResult],
    engines: list[str],
    runs_planned: int,
) -> str:
    engine_label: str = " + ".join(ENGINE_LABEL.get(engine, engine) for engine in engines)
    emoji: str = VERDICT_EMOJI.get(consensus.verdict, "·")
    multi_engine: bool = len(engines) > 1
    max_runs: int = max((per_engine[engine].runs_used for engine in engines), default=0)
    total_errored: int = sum(per_engine[engine].errored for engine in engines)

    lines: list[str] = [
        f"## 🎯 Critic verdict — PR #{pr_number}",
        "",
        f"_{engine_label} critic, {runs_planned} run(s) per engine. "
        "Goal-fit only — code quality is /cc-review's job._",
        "",
        f"### {emoji} **{consensus.verdict.upper()}** — consensus score {consensus.score}/100",
        "",
        "### Scores per engine",
        "",
    ]

    run_headers: str = " | ".join(f"r{i + 1}" for i in range(max_runs)) if max_runs else "—"
    lines.append(f"| Engine | runs | {run_headers} | median | verdict |")
    separator_runs: str = "|".join("------" for _ in range(max_runs))
    lines.append(f"|--------|------|{separator_runs}|--------|---------|")
    for engine in engines:
        engine_result: EngineResult = per_engine[engine]
        label: str = ENGINE_LABEL.get(engine, engine)
        run_scores: list[str] = [str(run.score) for run in engine_result.runs] + ["—"] * (max_runs - engine_result.runs_used)
        lines.append(
            f"| {label} | {engine_result.runs_used}/{runs_planned} | "
            f"{' | '.join(run_scores)} | **{engine_result.score}** | {engine_result.verdict} |"
        )
    if multi_engine:
        empty_cells: str = " | ".join("" for _ in range(max_runs))
        lines.append(
            f"| **Consensus** | | {empty_cells} | **{consensus.score}** | **{consensus.verdict}** |"
        )
    lines.append("")

    lines.append("### Axes (consensus median 0–100)")
    lines.append("")
    lines.append("| Axis | Score |")
    lines.append("|------|-------|")
    lines.extend(f"| `{axis}` | {consensus.axes[axis]} |" for axis in AXES)
    lines.append("")

    if consensus.rationale:
        lines.extend(("### Rationale (representative run)", "", consensus.rationale, ""))

    if consensus.concerns:
        lines.append("### Concerns (union across runs and engines)")
        lines.append("")
        lines.extend(f"- {concern}" for concern in consensus.concerns)
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


def serialize_engine(*, engine_result: EngineResult) -> dict:
    return {
        "runs_used": engine_result.runs_used,
        "runs_errored": engine_result.errored,
        "score": engine_result.score,
        "verdict": engine_result.verdict,
        "axes": dict(engine_result.axes),
        "per_run": [serialize_run(run=run) for run in engine_result.runs],
    }


def main() -> int:
    args: argparse.Namespace = parse_args()
    bundle: Path = args.bundle
    if not bundle.is_dir():
        print(f"aggregate: bundle dir not found: {bundle}", file=sys.stderr)
        return 1

    engines: list[str] = [engine.strip() for engine in args.engines.split(",") if engine.strip()]
    if not engines:
        print("aggregate: --engines is empty", file=sys.stderr)
        return 1

    per_engine: dict[str, EngineResult] = {
        engine: aggregate_engine(bundle=bundle, engine=engine, runs_planned=args.runs) for engine in engines
    }
    consensus: Consensus = aggregate_consensus(per_engine=per_engine)
    total_runs_used: int = sum(per_engine[engine].runs_used for engine in engines)

    summary_md: str = render_md(
        pr_number=args.pr,
        consensus=consensus,
        per_engine=per_engine,
        engines=engines,
        runs_planned=args.runs,
    )
    (bundle / "summary.md").write_text(summary_md)

    summary_obj: dict = {
        "pr": args.pr,
        "engines": engines,
        "runs_per_engine": args.runs,
        "runs_used": total_runs_used,
        "score": consensus.score,
        "verdict": consensus.verdict,
        "axes": dict(consensus.axes),
        "rationale": consensus.rationale,
        "concerns": list(consensus.concerns),
        "per_engine": {engine: serialize_engine(engine_result=per_engine[engine]) for engine in engines},
    }
    (bundle / "summary.json").write_text(json.dumps(summary_obj, indent=2))

    print(
        f"aggregate: engines={','.join(engines)} score={consensus.score} verdict={consensus.verdict}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
