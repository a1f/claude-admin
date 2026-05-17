#!/usr/bin/env python3
"""Aggregate critic JSONL outputs into a summary verdict + comment body.

Inputs:
  --bundle DIR  bundle directory containing logs/claude-critic-<run>.jsonl
  --pr ID       PR identifier (used in headers; can be "offline" for fixtures)
  --runs N      runs to expect (informational)

Outputs (written into the bundle dir):
  summary.md    markdown comment body (gets posted to the PR)
  summary.json  machine-readable verdict:
    {
      "pr": ..., "runs_used": N, "runs_errored": N,
      "score": <int>,             # median across runs
      "verdict": "strong|acceptable|weak|reject",
      "axes": {<axis>: <median int>, ...},
      "rationale": "<representative rationale>",
      "concerns": ["<unioned, deduped>"],
      "per_run": [ {score, verdict, axes, rationale_md, concerns}, ... ]
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
    return p.parse_args()


def extract_json(text: str) -> dict | None:
    """First balanced top-level JSON object in `text`. Tolerates fences/prose."""
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
    # Defensive defaults.
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
    """Map median score to verdict per the skill's own rubric."""
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
    # statistics.median returns a float for even-length lists; round to nearest int.
    return int(round(statistics.median(values)))


def dedup_concerns(per_run: list[dict]) -> list[str]:
    """Union of concerns across runs, deduped by hash of normalized text."""
    seen: dict[str, str] = {}
    for run in per_run:
        for c in run.get("concerns", []):
            text = str(c).strip()
            if not text:
                continue
            key = hashlib.sha1(text[:120].lower().encode()).hexdigest()[:10]
            if key not in seen:
                seen[key] = text
    return list(seen.values())


def representative_rationale(per_run: list[dict], median_score: int) -> str:
    """Pick the rationale from the run whose score is closest to the median."""
    if not per_run:
        return ""
    chosen = min(per_run, key=lambda r: (abs(r["score"] - median_score), -r["score"]))
    return str(chosen.get("rationale_md", "")).strip()


def aggregate(bundle: Path, runs: int) -> tuple[dict, list[dict], int]:
    per_run: list[dict] = []
    errored = 0
    for r in range(1, runs + 1):
        log = bundle / "logs" / f"claude-critic-{r}.jsonl"
        obj = load_run(log)
        if obj is None:
            errored += 1
            continue
        if obj.get("_error"):
            errored += 1
            continue
        per_run.append(obj)

    if not per_run:
        return (
            {
                "score": 0,
                "verdict": "reject",
                "axes": {a: 0 for a in AXES},
                "rationale": "All critic runs failed or produced unparseable output.",
                "concerns": [],
            },
            per_run,
            errored,
        )

    median_score = median_int([r["score"] for r in per_run])
    median_axes = {
        a: median_int([int(r["axes"].get(a, 0) or 0) for r in per_run]) for a in AXES
    }
    verdict = verdict_from_score(median_score)
    rationale = representative_rationale(per_run, median_score)
    concerns = dedup_concerns(per_run)
    return (
        {
            "score": median_score,
            "verdict": verdict,
            "axes": median_axes,
            "rationale": rationale,
            "concerns": concerns,
        },
        per_run,
        errored,
    )


VERDICT_EMOJI = {"strong": "✅", "acceptable": "✅", "weak": "⚠️", "reject": "🛑"}


def render_md(pr: str, summary: dict, per_run: list[dict], runs_planned: int, errored: int) -> str:
    lines: list[str] = []
    runs_used = len(per_run)
    lines.append(f"## 🎯 Critic verdict — PR #{pr}")
    lines.append("")
    lines.append(
        f"_Claude critic, {runs_used}/{runs_planned} run(s) succeeded. Goal-fit only — "
        "code quality is /review's job._"
    )
    lines.append("")
    emoji = VERDICT_EMOJI.get(summary["verdict"], "·")
    lines.append(f"### {emoji} **{summary['verdict'].upper()}** — score {summary['score']}/100")
    lines.append("")

    if per_run:
        all_scores = [r["score"] for r in per_run]
        lines.append(
            f"_per-run scores: {', '.join(str(s) for s in all_scores)} "
            f"(median {summary['score']}, min {min(all_scores)}, max {max(all_scores)})_"
        )
        lines.append("")

    lines.append("### Axes (median 0–100)")
    lines.append("")
    lines.append("| Axis | Score |")
    lines.append("|------|-------|")
    for a in AXES:
        v = summary["axes"].get(a, 0)
        lines.append(f"| `{a}` | {v} |")
    lines.append("")

    if summary["rationale"]:
        lines.append("### Rationale (representative run)")
        lines.append("")
        lines.append(summary["rationale"])
        lines.append("")

    if summary["concerns"]:
        lines.append("### Concerns (union across runs)")
        lines.append("")
        for c in summary["concerns"]:
            lines.append(f"- {c}")
        lines.append("")

    if errored > 0:
        lines.append(f"_⚠ {errored} critic run(s) failed or produced unparseable output._")
        lines.append("")

    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    bundle: Path = args.bundle
    if not bundle.is_dir():
        print(f"aggregate: bundle dir not found: {bundle}", file=sys.stderr)
        return 1

    summary, per_run, errored = aggregate(bundle, args.runs)
    (bundle / "summary.md").write_text(
        render_md(args.pr, summary, per_run, args.runs, errored)
    )
    (bundle / "summary.json").write_text(
        json.dumps(
            {
                "pr": args.pr,
                "runs_used": len(per_run),
                "runs_errored": errored,
                "score": summary["score"],
                "verdict": summary["verdict"],
                "axes": summary["axes"],
                "rationale": summary["rationale"],
                "concerns": summary["concerns"],
                "per_run": per_run,
            },
            indent=2,
        )
    )
    print(
        f"aggregate: score={summary['score']} verdict={summary['verdict']} "
        f"runs_used={len(per_run)}/{args.runs}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
