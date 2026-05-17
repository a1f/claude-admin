#!/usr/bin/env python3
"""Aggregate critic JSONL outputs into a summary verdict + comment body.

Inputs:
  --bundle DIR  bundle directory containing logs/<engine>-critic-<run>.jsonl
  --pr ID       PR identifier (used in headers; can be "offline" for fixtures)
  --runs N      runs per engine (informational)
  --engines CSV one or more engines: "claude", "codex", or "claude,codex"

Outputs (written into the bundle dir):
  summary.md    markdown comment body (gets posted to the PR)
  summary.json  machine-readable verdict:
    {
      "pr": ..., "engines": ["claude", "codex"], "runs_per_engine": N,
      "score": <int>,           # consensus median across all engines × runs
      "verdict": "strong|acceptable|weak|reject",
      "axes": {<axis>: <median>, ...},
      "rationale": "<representative rationale>",
      "concerns": ["<unioned, deduped>"],
      "per_engine": {
        "<engine>": { "runs_used": N, "runs_errored": N, "score": <median>,
                      "verdict": "...", "axes": {...},
                      "per_run": [ {score, verdict, axes, ...}, ... ] }
      }
    }

Verdict derivation is by median score (deterministic), matching the
skill's own rubric thresholds.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import statistics
import sys
from pathlib import Path

AXES = ["achieves_goal", "test_coverage", "no_scope_creep", "reuses_existing", "validation_evidence"]
VERDICT_ORDER = ["reject", "weak", "acceptable", "strong"]


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--bundle", required=True, type=Path)
    p.add_argument("--pr", required=True)
    p.add_argument("--runs", required=True, type=int)
    p.add_argument("--engines", default="claude")
    return p.parse_args()


def extract_json(text: str) -> dict | None:
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


def load_run(log_path: Path) -> dict | None:
    if not log_path.exists() or log_path.stat().st_size == 0:
        return None
    obj = extract_json(log_path.read_text(errors="replace"))
    if obj is None:
        return None
    if not isinstance(obj.get("axes"), dict):
        obj["axes"] = {}
    if not isinstance(obj.get("concerns"), list):
        obj["concerns"] = []
    try:
        obj["score"] = int(obj.get("score", 0))
    except (TypeError, ValueError):
        obj["score"] = 0
    if obj.get("verdict") not in VERDICT_ORDER:
        obj["verdict"] = "reject"
    return obj


def verdict_from_score(score: int) -> str:
    if score >= 85:
        return "strong"
    if score >= 70:
        return "acceptable"
    if score >= 50:
        return "weak"
    return "reject"


def median_int(values: list[int]) -> int:
    if not values:
        return 0
    return int(round(statistics.median(values)))


def dedup_concerns(runs: list[dict]) -> list[str]:
    seen: dict[str, str] = {}
    for run in runs:
        for c in run.get("concerns", []):
            text = str(c).strip()
            if not text:
                continue
            key = hashlib.sha1(text[:120].lower().encode()).hexdigest()[:10]
            if key not in seen:
                seen[key] = text
    return list(seen.values())


def representative_rationale(runs: list[dict], median_score: int) -> str:
    if not runs:
        return ""
    chosen = min(runs, key=lambda r: (abs(r["score"] - median_score), -r["score"]))
    return str(chosen.get("rationale_md", "")).strip()


def aggregate_engine(bundle: Path, engine: str, runs_planned: int) -> dict:
    """Aggregate runs for one engine."""
    per_run: list[dict] = []
    errored = 0
    for r in range(1, runs_planned + 1):
        log = bundle / "logs" / f"{engine}-critic-{r}.jsonl"
        obj = load_run(log)
        if obj is None:
            errored += 1
            continue
        if obj.get("_error"):
            errored += 1
            continue
        per_run.append(obj)

    if not per_run:
        return {
            "runs_used": 0,
            "runs_errored": errored,
            "score": 0,
            "verdict": "reject",
            "axes": {a: 0 for a in AXES},
            "rationale": "All runs failed.",
            "per_run": per_run,
        }

    median_score = median_int([r["score"] for r in per_run])
    median_axes = {
        a: median_int([int(r["axes"].get(a, 0) or 0) for r in per_run]) for a in AXES
    }
    return {
        "runs_used": len(per_run),
        "runs_errored": errored,
        "score": median_score,
        "verdict": verdict_from_score(median_score),
        "axes": median_axes,
        "rationale": representative_rationale(per_run, median_score),
        "per_run": per_run,
    }


def aggregate_consensus(per_engine: dict[str, dict]) -> dict:
    """Combine per-engine results into a consensus across all runs."""
    all_runs: list[dict] = []
    for eng_data in per_engine.values():
        all_runs.extend(eng_data["per_run"])

    if not all_runs:
        return {
            "score": 0,
            "verdict": "reject",
            "axes": {a: 0 for a in AXES},
            "rationale": "All critic runs failed.",
            "concerns": [],
        }

    median_score = median_int([r["score"] for r in all_runs])
    median_axes = {
        a: median_int([int(r["axes"].get(a, 0) or 0) for r in all_runs]) for a in AXES
    }
    return {
        "score": median_score,
        "verdict": verdict_from_score(median_score),
        "axes": median_axes,
        "rationale": representative_rationale(all_runs, median_score),
        "concerns": dedup_concerns(all_runs),
    }


VERDICT_EMOJI = {"strong": "✅", "acceptable": "✅", "weak": "⚠️", "reject": "🛑"}
ENGINE_LABEL = {"claude": "Claude", "codex": "Codex"}


def render_md(
    pr: str,
    consensus: dict,
    per_engine: dict[str, dict],
    engines: list[str],
    runs_planned: int,
) -> str:
    lines: list[str] = []
    lines.append(f"## 🎯 Critic verdict — PR #{pr}")
    lines.append("")
    eng_label = " + ".join(ENGINE_LABEL.get(e, e) for e in engines)
    lines.append(
        f"_{eng_label} critic, {runs_planned} run(s) per engine. "
        "Goal-fit only — code quality is /review's job._"
    )
    lines.append("")
    emoji = VERDICT_EMOJI.get(consensus["verdict"], "·")
    lines.append(
        f"### {emoji} **{consensus['verdict'].upper()}** — consensus score {consensus['score']}/100"
    )
    lines.append("")

    # Per-engine table (only if more than one engine, otherwise it's redundant)
    if len(engines) >= 1:
        lines.append("### Scores per engine")
        lines.append("")
        # Build header dynamically for up to N runs
        max_runs = max((len(per_engine[e]["per_run"]) for e in engines), default=0)
        run_headers = " | ".join(f"r{i+1}" for i in range(max_runs)) if max_runs else "—"
        lines.append(f"| Engine | runs | {run_headers} | median | verdict |")
        lines.append("|--------|------|" + "|".join("------" for _ in range(max_runs)) + "|--------|---------|")
        for e in engines:
            ed = per_engine[e]
            label = ENGINE_LABEL.get(e, e)
            run_scores = [str(r["score"]) for r in ed["per_run"]]
            run_scores += ["—"] * (max_runs - len(run_scores))
            lines.append(
                f"| {label} | {ed['runs_used']}/{runs_planned} | "
                f"{' | '.join(run_scores)} | **{ed['score']}** | {ed['verdict']} |"
            )
        if len(engines) > 1:
            lines.append(
                "| **Consensus** | | "
                + " | ".join("" for _ in range(max_runs))
                + f" | **{consensus['score']}** | **{consensus['verdict']}** |"
            )
        lines.append("")

    lines.append("### Axes (consensus median 0–100)")
    lines.append("")
    lines.append("| Axis | Score |")
    lines.append("|------|-------|")
    for a in AXES:
        lines.append(f"| `{a}` | {consensus['axes'].get(a, 0)} |")
    lines.append("")

    if consensus["rationale"]:
        lines.append("### Rationale (representative run)")
        lines.append("")
        lines.append(consensus["rationale"])
        lines.append("")

    if consensus["concerns"]:
        lines.append("### Concerns (union across runs and engines)")
        lines.append("")
        for c in consensus["concerns"]:
            lines.append(f"- {c}")
        lines.append("")

    total_errored = sum(per_engine[e]["runs_errored"] for e in engines)
    if total_errored > 0:
        lines.append(f"_⚠ {total_errored} critic run(s) failed or produced unparseable output._")
        lines.append("")

    return "\n".join(lines) + "\n"


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

    per_engine = {e: aggregate_engine(bundle, e, args.runs) for e in engines}
    consensus = aggregate_consensus(per_engine)

    (bundle / "summary.md").write_text(
        render_md(args.pr, consensus, per_engine, engines, args.runs)
    )
    total_runs_used = sum(per_engine[e]["runs_used"] for e in engines)
    (bundle / "summary.json").write_text(
        json.dumps(
            {
                "pr": args.pr,
                "engines": engines,
                "runs_per_engine": args.runs,
                "runs_used": total_runs_used,
                "score": consensus["score"],
                "verdict": consensus["verdict"],
                "axes": consensus["axes"],
                "rationale": consensus["rationale"],
                "concerns": consensus["concerns"],
                "per_engine": {
                    e: {
                        "runs_used": per_engine[e]["runs_used"],
                        "runs_errored": per_engine[e]["runs_errored"],
                        "score": per_engine[e]["score"],
                        "verdict": per_engine[e]["verdict"],
                        "axes": per_engine[e]["axes"],
                        "per_run": per_engine[e]["per_run"],
                    }
                    for e in engines
                },
            },
            indent=2,
        )
    )
    print(
        f"aggregate: engines={','.join(engines)} score={consensus['score']} "
        f"verdict={consensus['verdict']}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
