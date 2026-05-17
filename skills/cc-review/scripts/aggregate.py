#!/usr/bin/env python3
"""Aggregate reviewer JSONL outputs into a summary table + per-kind detail comments.

Inputs:
  --bundle DIR  bundle directory containing logs/<engine>-<kind>-<run>.jsonl
  --pr N        PR number (used in headers)
  --kinds CSV   comma-separated kinds to aggregate
  --runs N      runs per kind per engine (used in header)
  --engines CSV one or more engines (default "claude"): "claude", "codex", "claude,codex"

Outputs (written into the bundle dir):
  summary.md        markdown summary table (gets posted as the top PR comment)
  summary.json      machine-readable totals (run.sh reads .totals.blocker for CRITICAL)
  detail-<kind>.md  one per kind that produced any findings
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import defaultdict
from pathlib import Path

SEVERITIES = ["blocker", "major", "minor", "nit"]
SEVERITY_EMOJI = {"blocker": "🛑", "major": "⚠️", "minor": "ℹ️", "nit": "·"}
ENGINE_LABEL = {"claude": "Claude", "codex": "Codex"}


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--bundle", required=True, type=Path)
    p.add_argument("--pr", required=True)
    p.add_argument("--kinds", required=True)
    p.add_argument("--runs", required=True, type=int)
    p.add_argument("--engines", default="claude")
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
    if not isinstance(obj.get("findings"), list):
        obj["findings"] = []
    return obj


def finding_key(f: dict) -> str:
    file_ = str(f.get("file", "?"))
    lines = f.get("lines") or [0]
    first_line = lines[0] if isinstance(lines, list) and lines else 0
    desc_hash = hashlib.sha1(str(f.get("desc", ""))[:80].encode()).hexdigest()[:8]
    return f"{file_}:{first_line}:{desc_hash}"


def severity_rank(sev: str) -> int:
    return SEVERITIES.index(sev) if sev in SEVERITIES else len(SEVERITIES)


def collect_per_engine(
    bundle: Path, kind: str, engine: str, runs: int
) -> tuple[list[dict], int, int]:
    """Return (run_objs, errored, empty) for one engine × kind."""
    run_objs: list[dict] = []
    errored = 0
    empty = 0
    for r in range(1, runs + 1):
        log = bundle / "logs" / f"{engine}-{kind}-{r}.jsonl"
        obj = load_run(log)
        if obj is None:
            empty += 1
            continue
        if obj.get("_error"):
            errored += 1
            continue
        run_objs.append(obj)
    return run_objs, errored, empty


def merge_findings(runs_with_engine: list[tuple[dict, str]]) -> list[dict]:
    """Union of findings across runs (engine-tagged), deduped.

    runs_with_engine: list of (run_obj, engine_name) pairs.
    On dup, keep highest severity AND record all engines that flagged it.
    """
    by_key: dict[str, dict] = {}
    by_key_engines: dict[str, set[str]] = defaultdict(set)
    for run, engine in runs_with_engine:
        for f in run.get("findings", []):
            sev = f.get("severity", "nit")
            if sev not in SEVERITIES:
                sev = "nit"
                f["severity"] = sev
            key = finding_key(f)
            by_key_engines[key].add(engine)
            existing = by_key.get(key)
            if existing is None or severity_rank(sev) < severity_rank(existing["severity"]):
                by_key[key] = dict(f)
    out = []
    for key, f in by_key.items():
        f["engines"] = sorted(by_key_engines[key])
        out.append(f)
    out.sort(
        key=lambda f: (
            severity_rank(f["severity"]),
            str(f.get("file", "")),
            (f.get("lines") or [0])[0],
        )
    )
    return out


def count_findings(findings: list[dict]) -> dict[str, int]:
    counts: dict[str, int] = defaultdict(int)
    for f in findings:
        counts[f["severity"]] += 1
    return dict(counts)


def aggregate(
    bundle: Path, kinds: list[str], runs: int, engines: list[str]
) -> dict:
    result: dict[str, dict] = {}
    for kind in kinds:
        per_engine: dict[str, dict] = {}
        all_runs_tagged: list[tuple[dict, str]] = []
        for engine in engines:
            run_objs, errored, empty = collect_per_engine(bundle, kind, engine, runs)
            findings = merge_findings([(r, engine) for r in run_objs])
            per_engine[engine] = {
                "runs_used": len(run_objs),
                "runs_errored": errored,
                "runs_empty": empty,
                "counts": count_findings(findings),
                "summaries": [
                    str(r.get("summary", "")).strip()
                    for r in run_objs
                    if r.get("summary")
                ],
                "findings": findings,
            }
            all_runs_tagged.extend((r, engine) for r in run_objs)

        union_findings = merge_findings(all_runs_tagged)
        result[kind] = {
            "per_engine": per_engine,
            "union": {
                "findings": union_findings,
                "counts": count_findings(union_findings),
            },
        }
    return result


def render_summary_md(
    pr_num: str,
    agg: dict,
    kinds: list[str],
    runs: int,
    engines: list[str],
) -> tuple[str, dict]:
    multi_engine = len(engines) > 1
    eng_label = " + ".join(ENGINE_LABEL.get(e, e) for e in engines)

    lines: list[str] = []
    lines.append(f"## 🔍 Multi-agent review — PR #{pr_num}")
    lines.append("")
    lines.append(
        f"_{eng_label} reviewers, {runs} independent run(s) per kind per engine; "
        "findings deduped (union across runs and engines)._"
    )
    lines.append("")

    # Header
    header_engines = [ENGINE_LABEL.get(e, e) for e in engines]
    header_cells = ["Kind"] + [f"{lbl} (b/M/m/n)" for lbl in header_engines]
    if multi_engine:
        header_cells.append("Union (b/M/m/n)")
    lines.append("| " + " | ".join(header_cells) + " |")
    lines.append("|" + "|".join("---" for _ in header_cells) + "|")

    # Per-kind rows
    union_totals: dict[str, int] = defaultdict(int)
    for kind in kinds:
        info = agg.get(kind)
        if info is None:
            continue
        row = [f"`{kind}`"]
        for e in engines:
            c = info["per_engine"][e]["counts"]
            ru = info["per_engine"][e]["runs_used"]
            ru_marker = "" if ru > 0 else " ⚠"
            row.append(
                f"{c.get('blocker', 0)}/{c.get('major', 0)}/{c.get('minor', 0)}/{c.get('nit', 0)} "
                f"({ru}/{runs}{ru_marker})"
            )
        if multi_engine:
            uc = info["union"]["counts"]
            row.append(
                f"**{uc.get('blocker', 0)}**/{uc.get('major', 0)}/"
                f"{uc.get('minor', 0)}/{uc.get('nit', 0)}"
            )
            for sev in SEVERITIES:
                union_totals[sev] += uc.get(sev, 0)
        else:
            e = engines[0]
            c = info["per_engine"][e]["counts"]
            for sev in SEVERITIES:
                union_totals[sev] += c.get(sev, 0)
        lines.append("| " + " | ".join(row) + " |")

    # Totals row
    totals_row = ["**Total**"] + ["" for _ in engines]
    if multi_engine:
        totals_row.append(
            f"**{union_totals['blocker']}**/{union_totals['major']}/"
            f"{union_totals['minor']}/{union_totals['nit']}"
        )
        # For single-engine, replace the per-engine cell with totals to avoid empty totals.
    else:
        totals_row[-1] = (
            f"**{union_totals['blocker']}**/{union_totals['major']}/"
            f"{union_totals['minor']}/{union_totals['nit']}"
        )
    lines.append("| " + " | ".join(totals_row) + " |")
    lines.append("")

    if union_totals["blocker"] > 0:
        lines.append(
            f"**🛑 {union_totals['blocker']} blocker(s) found across engines — "
            "`CRITICAL` label applied.**"
        )
    else:
        lines.append("**✅ No blockers.**")
    lines.append("")
    lines.append(
        "Detail comments per kind follow below for any kind with findings. "
        "Each finding is tagged with the engine(s) that flagged it — agreement across engines is a confidence signal."
    )
    return "\n".join(lines) + "\n", dict(union_totals)


def render_detail_md(pr_num: str, kind: str, info: dict, engines: list[str]) -> str:
    union_findings = info["union"]["findings"]
    if not union_findings:
        return ""
    by_file: dict[str, list[dict]] = defaultdict(list)
    for f in union_findings:
        by_file[str(f.get("file", "?"))].append(f)

    multi_engine = len(engines) > 1
    lines: list[str] = []
    lines.append(f"### `{kind}` reviewer findings — PR #{pr_num}")
    lines.append("")
    uc = info["union"]["counts"]
    lines.append(
        f"_Union across engines: blockers {uc.get('blocker', 0)}, "
        f"major {uc.get('major', 0)}, minor {uc.get('minor', 0)}, nit {uc.get('nit', 0)}._"
    )
    if multi_engine:
        per_engine_counts = "; ".join(
            f"{ENGINE_LABEL.get(e, e)}: "
            f"{info['per_engine'][e]['counts'].get('blocker', 0)} blockers, "
            f"{info['per_engine'][e]['counts'].get('major', 0)} major, "
            f"{info['per_engine'][e]['counts'].get('minor', 0)} minor, "
            f"{info['per_engine'][e]['counts'].get('nit', 0)} nit"
            for e in engines
        )
        lines.append(f"_{per_engine_counts}._")
    lines.append("")

    # Reviewer summaries per engine (1 line per run, prefixed by engine)
    for e in engines:
        summaries = info["per_engine"][e]["summaries"]
        if summaries:
            lines.append(f"**{ENGINE_LABEL.get(e, e)} summaries:**")
            for s in summaries:
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
            tagged_engines = f.get("engines", [])
            engine_tag = ""
            if multi_engine and tagged_engines:
                labels = [ENGINE_LABEL.get(e, e) for e in tagged_engines]
                engine_tag = f" _[{'+'.join(labels)}]_"
            header = f"- {emoji} **{sev}** {anchor}{engine_tag}".rstrip()
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
    engines = [e.strip() for e in args.engines.split(",") if e.strip()]
    if not kinds:
        print("aggregate: --kinds is empty", file=sys.stderr)
        return 1
    if not engines:
        print("aggregate: --engines is empty", file=sys.stderr)
        return 1

    agg = aggregate(bundle, kinds, args.runs, engines)
    summary_md, totals = render_summary_md(args.pr, agg, kinds, args.runs, engines)
    (bundle / "summary.md").write_text(summary_md)
    (bundle / "summary.json").write_text(
        json.dumps(
            {
                "pr": args.pr,
                "engines": engines,
                "totals": totals,
                "per_kind": {
                    k: {
                        "union": {"counts": agg[k]["union"]["counts"]},
                        "per_engine": {
                            e: {
                                "counts": agg[k]["per_engine"][e]["counts"],
                                "runs_used": agg[k]["per_engine"][e]["runs_used"],
                                "runs_errored": agg[k]["per_engine"][e]["runs_errored"],
                                "runs_empty": agg[k]["per_engine"][e]["runs_empty"],
                            }
                            for e in engines
                        },
                    }
                    for k in kinds
                    if k in agg
                },
            },
            indent=2,
        )
    )
    for kind in kinds:
        info = agg.get(kind)
        if not info or not info["union"]["findings"]:
            continue
        detail = render_detail_md(args.pr, kind, info, engines)
        (bundle / f"detail-{kind}.md").write_text(detail)

    print(
        f"aggregate: engines={','.join(engines)} kinds={','.join(kinds)} "
        f"total_blockers={totals.get('blocker', 0)}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
