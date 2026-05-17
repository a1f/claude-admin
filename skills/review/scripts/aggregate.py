#!/usr/bin/env python3
"""Aggregate reviewer JSONL outputs into a summary table + per-kind detail comments.

Inputs:
  --bundle DIR  bundle directory containing logs/claude-<kind>-<run>.jsonl
  --pr N        PR number (used in headers)
  --kinds CSV   comma-separated kinds to aggregate
  --runs N      runs per kind (informational, used in header)

Outputs (written into the bundle dir):
  summary.md      markdown summary table (gets posted as the top PR comment)
  summary.json    machine-readable totals (run.sh reads .totals.blocker)
  detail-<kind>.md  one per kind that produced any findings
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

SEVERITIES = ["blocker", "major", "minor", "nit"]
SEVERITY_EMOJI = {"blocker": "🛑", "major": "⚠️", "minor": "ℹ️", "nit": "·"}


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--bundle", required=True, type=Path)
    p.add_argument("--pr", required=True)
    p.add_argument("--kinds", required=True)
    p.add_argument("--runs", required=True, type=int)
    return p.parse_args()


def extract_json(text: str) -> dict | None:
    """Find the first balanced top-level JSON object in `text`.

    Claude sometimes wraps JSON in prose or markdown fences despite instructions.
    We scan for the first '{' and walk to its matching '}' respecting strings.
    """
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
                blob = text[start : i + 1]
                try:
                    return json.loads(blob)
                except json.JSONDecodeError:
                    return None
    return None


def load_run(log_path: Path) -> dict | None:
    if not log_path.exists() or log_path.stat().st_size == 0:
        return None
    text = log_path.read_text(errors="replace")
    obj = extract_json(text)
    if obj is None:
        return None
    if not isinstance(obj.get("findings"), list):
        obj["findings"] = []
    return obj


def finding_key(f: dict) -> str:
    """Dedup key: file + first-line + hash(desc[:80])."""
    file_ = str(f.get("file", "?"))
    lines = f.get("lines") or [0]
    first_line = lines[0] if isinstance(lines, list) and lines else 0
    desc_hash = hashlib.sha1(str(f.get("desc", ""))[:80].encode()).hexdigest()[:8]
    return f"{file_}:{first_line}:{desc_hash}"


def severity_rank(sev: str) -> int:
    return SEVERITIES.index(sev) if sev in SEVERITIES else len(SEVERITIES)


def merge_findings(runs: list[dict]) -> list[dict]:
    """Union of findings across runs, deduped. On dup, keep highest severity."""
    by_key: dict[str, dict] = {}
    for run in runs:
        for f in run.get("findings", []):
            sev = f.get("severity", "nit")
            if sev not in SEVERITIES:
                sev = "nit"
                f["severity"] = sev
            key = finding_key(f)
            existing = by_key.get(key)
            if existing is None or severity_rank(sev) < severity_rank(existing["severity"]):
                by_key[key] = f
    out = list(by_key.values())
    out.sort(key=lambda f: (severity_rank(f["severity"]), str(f.get("file", "")), (f.get("lines") or [0])[0]))
    return out


def aggregate(bundle: Path, kinds: list[str], runs: int) -> dict:
    result: dict[str, dict] = {}
    for kind in kinds:
        run_objs: list[dict] = []
        errored_runs = 0
        empty_runs = 0
        for r in range(1, runs + 1):
            log = bundle / "logs" / f"claude-{kind}-{r}.jsonl"
            obj = load_run(log)
            if obj is None:
                empty_runs += 1
                continue
            if obj.get("_error"):
                errored_runs += 1
                continue
            run_objs.append(obj)
        merged = merge_findings(run_objs)
        counts = defaultdict(int)
        for f in merged:
            counts[f["severity"]] += 1
        summaries = [str(r.get("summary", "")).strip() for r in run_objs if r.get("summary")]
        result[kind] = {
            "findings": merged,
            "counts": dict(counts),
            "runs_used": len(run_objs),
            "runs_errored": errored_runs,
            "runs_empty": empty_runs,
            "summaries": summaries,
        }
    return result


def render_summary_md(pr_num: str, agg: dict, kinds: list[str], runs: int) -> tuple[str, dict]:
    lines: list[str] = []
    lines.append(f"## 🔍 Multi-agent review — PR #{pr_num}")
    lines.append("")
    lines.append(f"_Claude reviewers, {runs} independent runs per kind, deduped union of findings._")
    lines.append("")
    lines.append("| Kind | runs used | blockers | major | minor | nit |")
    lines.append("|------|-----------|----------|-------|-------|-----|")
    totals = defaultdict(int)
    for kind in kinds:
        info = agg.get(kind, {"counts": {}, "runs_used": 0})
        c = info["counts"]
        b = c.get("blocker", 0)
        ma = c.get("major", 0)
        mi = c.get("minor", 0)
        ni = c.get("nit", 0)
        totals["blocker"] += b
        totals["major"] += ma
        totals["minor"] += mi
        totals["nit"] += ni
        ru = info["runs_used"]
        marker = f"{ru}/{runs}"
        if ru == 0:
            marker += " ⚠"
        lines.append(f"| `{kind}` | {marker} | **{b}** | {ma} | {mi} | {ni} |")
    lines.append("|---|---|---|---|---|---|")
    lines.append(
        f"| **total** | | **{totals['blocker']}** | {totals['major']} | {totals['minor']} | {totals['nit']} |"
    )
    lines.append("")
    if totals["blocker"] > 0:
        lines.append(f"**🛑 {totals['blocker']} blocker(s) found — `CRITICAL` label applied.**")
    else:
        lines.append("**✅ No blockers.**")
    lines.append("")
    lines.append("Detail comments per kind follow below for any kind with findings.")
    return "\n".join(lines) + "\n", dict(totals)


def render_detail_md(pr_num: str, kind: str, info: dict) -> str:
    if not info["findings"]:
        return ""
    by_file: dict[str, list[dict]] = defaultdict(list)
    for f in info["findings"]:
        by_file[str(f.get("file", "?"))].append(f)

    lines: list[str] = []
    lines.append(f"### `{kind}` reviewer findings — PR #{pr_num}")
    lines.append("")
    c = info["counts"]
    lines.append(
        f"_{info['runs_used']} run(s) succeeded. Counts: blockers {c.get('blocker', 0)}, "
        f"major {c.get('major', 0)}, minor {c.get('minor', 0)}, nit {c.get('nit', 0)}._"
    )
    lines.append("")
    if info["summaries"]:
        lines.append("**Reviewer summaries (one per run):**")
        for s in info["summaries"]:
            lines.append(f"- {s}")
        lines.append("")
    for file_, fs in sorted(by_file.items()):
        lines.append(f"#### `{file_}`")
        lines.append("")
        for f in fs:
            sev = f.get("severity", "nit")
            emoji = SEVERITY_EMOJI.get(sev, "·")
            lines_range = f.get("lines") or []
            if isinstance(lines_range, list) and len(lines_range) == 2:
                anchor = f"L{lines_range[0]}–L{lines_range[1]}"
            elif isinstance(lines_range, list) and lines_range:
                anchor = f"L{lines_range[0]}"
            else:
                anchor = ""
            desc = str(f.get("desc", "")).strip()
            fix = str(f.get("suggested_fix", "")).strip()
            header = f"- {emoji} **{sev}** {anchor}".rstrip()
            lines.append(header)
            if desc:
                lines.append(f"  - {desc}")
            if fix:
                lines.append(f"  - _fix:_ {fix}")
        lines.append("")
    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    bundle: Path = args.bundle
    if not bundle.is_dir():
        print(f"aggregate: bundle dir not found: {bundle}", file=sys.stderr)
        return 1
    kinds = [k.strip() for k in args.kinds.split(",") if k.strip()]
    if not kinds:
        print("aggregate: --kinds is empty", file=sys.stderr)
        return 1

    agg = aggregate(bundle, kinds, args.runs)
    summary_md, totals = render_summary_md(args.pr, agg, kinds, args.runs)
    (bundle / "summary.md").write_text(summary_md)
    (bundle / "summary.json").write_text(
        json.dumps(
            {
                "pr": args.pr,
                "totals": totals,
                "per_kind": {
                    k: {
                        "counts": v["counts"],
                        "runs_used": v["runs_used"],
                        "runs_errored": v["runs_errored"],
                        "runs_empty": v["runs_empty"],
                    }
                    for k, v in agg.items()
                },
            },
            indent=2,
        )
    )
    for kind in kinds:
        info = agg.get(kind)
        if not info or not info["findings"]:
            continue
        detail = render_detail_md(args.pr, kind, info)
        (bundle / f"detail-{kind}.md").write_text(detail)

    print(f"aggregate: wrote summary.md + summary.json + detail-*.md in {bundle}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
