#!/usr/bin/env python3
"""CLI for the /grill-with-docs freshness check + end-of-session audit.

Subcommands:

    freshness <repo-root>
        Inspect CONTEXT.md (or each context via CONTEXT-MAP.md) and report
        staleness: file age, orphaned glossary terms, broken file references.

    snapshot <repo-root>
        Capture the pre-session state of CONTEXT.md and docs/adr/.

    audit <repo-root> --snapshot <snapshot.json> --decided <decided.json>
        Verify every decision made during the session landed on disk.

All output is JSON on stdout.

Exit codes:
    0  ran successfully (clean for audit)
    1  audit found mismatches
    2  invalid args / config error
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
from dataclasses import asdict
from pathlib import Path
from typing import Any

# script's own directory is auto-prepended to sys.path by Python; sibling
# modules are importable directly. `models` over `types` to avoid stdlib clash.
from constants import ADR_FILE_RE, SECONDS_PER_DAY, STALE_AGE_DAYS
from doc_parsing import (
    discover_contexts,
    parse_file_refs,
    parse_terms,
    term_orphaned,
)
from models import (
    AdrMismatch,
    AuditReport,
    ContextReport,
    ContextSnapshot,
    FreshnessReport,
    Snapshot,
    TermMismatch,
)


# ---------- freshness ----------

def inspect_context(*, context_path: Path, repo_root: Path) -> ContextReport:
    """Symlink-resolution and macOS /private/var quirks force us to normalise both sides."""
    resolved_root = repo_root.resolve()
    resolved_ctx = (
        context_path.resolve() if context_path.exists() else context_path
    )
    try:
        rel = resolved_ctx.relative_to(resolved_root).as_posix()
    except ValueError:
        rel = str(context_path)
    if not context_path.exists():
        return ContextReport(path=rel, exists=False)
    stat = context_path.stat()
    age_days = round((time.time() - stat.st_mtime) / SECONDS_PER_DAY, 2)
    text = context_path.read_text(encoding="utf-8")
    terms = parse_terms(text=text)
    orphans = tuple(
        t for t in terms if term_orphaned(term=t, repo_root=repo_root)
    )
    refs = parse_file_refs(
        text=text, context_dir=context_path.parent, repo_root=repo_root
    )
    missing = tuple(r for r in refs if not (repo_root / r).exists())
    return ContextReport(
        path=rel,
        exists=True,
        mtime_unix=stat.st_mtime,
        age_days=age_days,
        terms=terms,
        orphaned_terms=orphans,
        file_refs=refs,
        missing_files=missing,
    )


def build_freshness(*, repo_root: Path) -> FreshnessReport:
    has_map, paths = discover_contexts(repo_root=repo_root)
    contexts = tuple(
        inspect_context(context_path=p, repo_root=repo_root) for p in paths
    )
    stale = any(
        c.exists
        and (c.orphaned_terms or c.missing_files or (c.age_days or 0) > STALE_AGE_DAYS)
        for c in contexts
    )
    return FreshnessReport(
        repo_root=str(repo_root),
        has_context_map=has_map,
        stale=stale,
        contexts=contexts,
    )


# ---------- snapshot ----------

def _sha256(*, path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _list_adr_files(*, adr_dir: Path) -> tuple[str, ...]:
    if not adr_dir.is_dir():
        return ()
    return tuple(
        p.name
        for p in sorted(adr_dir.iterdir())
        if p.is_file() and ADR_FILE_RE.match(p.name)
    )


def _discover_context_paths_for_snapshot(*, repo_root: Path) -> tuple[Path, ...]:
    """Snapshot needs to record absent contexts too; freshness skips them."""
    has_map, paths = discover_contexts(repo_root=repo_root)
    if has_map or paths:
        return paths
    return (repo_root / "CONTEXT.md",)


def take_snapshot(*, repo_root: Path) -> Snapshot:
    contexts: list[ContextSnapshot] = []
    resolved_root = repo_root.resolve()
    for p in _discover_context_paths_for_snapshot(repo_root=repo_root):
        try:
            rel = p.resolve().relative_to(resolved_root).as_posix()
        except (ValueError, FileNotFoundError):
            try:
                rel = p.relative_to(repo_root).as_posix()
            except ValueError:
                rel = str(p)
        if p.exists():
            text = p.read_text(encoding="utf-8")
            contexts.append(
                ContextSnapshot(
                    path=rel,
                    exists=True,
                    sha256=_sha256(path=p),
                    terms=parse_terms(text=text),
                )
            )
        else:
            contexts.append(ContextSnapshot(path=rel, exists=False))
    return Snapshot(
        repo_root=str(repo_root),
        contexts=tuple(contexts),
        adr_files=_list_adr_files(adr_dir=repo_root / "docs" / "adr"),
    )


# ---------- audit ----------

def _audit_terms(
    *, repo_root: Path, decided: dict[str, Any], report: AuditReport
) -> None:
    for entry in decided.get("terms", []):
        name = entry["name"]
        ctx_rel = entry.get("context", "CONTEXT.md")
        ctx_path = repo_root / ctx_rel
        if not ctx_path.exists():
            report.term_mismatches.append(
                TermMismatch(name=name, context=ctx_rel, reason="context-missing")
            )
            continue
        text = ctx_path.read_text(encoding="utf-8")
        if name not in parse_terms(text=text):
            report.term_mismatches.append(
                TermMismatch(name=name, context=ctx_rel, reason="term-not-written")
            )
            continue
        report.terms_ok.append(f"{ctx_rel}::{name}")


def _audit_adrs(
    *,
    repo_root: Path,
    snap: Snapshot,
    decided: dict[str, Any],
    report: AuditReport,
) -> None:
    adr_dir = repo_root / "docs" / "adr"
    current = _list_adr_files(adr_dir=adr_dir)
    pre_existing = set(snap.adr_files)
    for entry in decided.get("adrs", []):
        slug = entry["slug"]
        number = entry.get("number")
        match: str | None = None
        for fname in current:
            m = ADR_FILE_RE.match(fname)
            if not m:
                continue
            file_num = int(m.group(1))
            file_slug = m.group(2)
            if file_slug != slug:
                continue
            if number is not None and file_num != number:
                continue
            match = fname
            break
        if match is None:
            report.adr_mismatches.append(
                AdrMismatch(slug=slug, number=number, reason="no-matching-file")
            )
            continue
        if match in pre_existing:
            report.adr_mismatches.append(
                AdrMismatch(slug=slug, number=number, reason="already-existed")
            )
            continue
        report.adrs_ok.append(match)


def run_audit(
    *, repo_root: Path, snap: Snapshot, decided: dict[str, Any]
) -> AuditReport:
    report = AuditReport()
    _audit_terms(repo_root=repo_root, decided=decided, report=report)
    _audit_adrs(repo_root=repo_root, snap=snap, decided=decided, report=report)
    report.clean = not report.term_mismatches and not report.adr_mismatches
    return report


# ---------- serialisation ----------

def snapshot_from_dict(*, data: dict[str, Any]) -> Snapshot:
    contexts = tuple(
        ContextSnapshot(
            path=c["path"],
            exists=c.get("exists", False),
            sha256=c.get("sha256"),
            terms=tuple(c.get("terms") or ()),
        )
        for c in data.get("contexts", [])
    )
    return Snapshot(
        repo_root=data.get("repo_root", ""),
        contexts=contexts,
        adr_files=tuple(data.get("adr_files") or ()),
    )


def _freshness_to_jsonable(*, report: FreshnessReport) -> dict[str, Any]:
    return {
        "repo_root": report.repo_root,
        "has_context_map": report.has_context_map,
        "stale": report.stale,
        "contexts": [asdict(c) for c in report.contexts],
    }


def _audit_to_jsonable(*, report: AuditReport) -> dict[str, Any]:
    return {
        "clean": report.clean,
        "terms_ok": report.terms_ok,
        "adrs_ok": report.adrs_ok,
        "term_mismatches": [asdict(m) for m in report.term_mismatches],
        "adr_mismatches": [asdict(m) for m in report.adr_mismatches],
    }


# ---------- CLI ----------

def _emit_json(*, payload: dict[str, Any]) -> None:
    json.dump(payload, sys.stdout, indent=2)
    sys.stdout.write("\n")


def _load_json(*, path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def main(*, argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    fp = sub.add_parser("freshness", help="check CONTEXT.md staleness")
    fp.add_argument("repo_root")

    sp = sub.add_parser("snapshot", help="capture pre-session doc state")
    sp.add_argument("repo_root")

    ap = sub.add_parser("audit", help="verify decisions landed on disk")
    ap.add_argument("repo_root")
    ap.add_argument("--snapshot", required=True)
    ap.add_argument("--decided", required=True)

    args = parser.parse_args(argv)
    repo_root = Path(args.repo_root).resolve()
    if not repo_root.is_dir():
        print(f"error: not a directory: {repo_root}", file=sys.stderr)
        return 2

    if args.cmd == "freshness":
        _emit_json(payload=_freshness_to_jsonable(
            report=build_freshness(repo_root=repo_root)
        ))
        return 0

    if args.cmd == "snapshot":
        _emit_json(payload=asdict(take_snapshot(repo_root=repo_root)))
        return 0

    snap = snapshot_from_dict(data=_load_json(path=Path(args.snapshot)))
    decided = _load_json(path=Path(args.decided))
    report = run_audit(repo_root=repo_root, snap=snap, decided=decided)
    _emit_json(payload=_audit_to_jsonable(report=report))
    return 0 if report.clean else 1


if __name__ == "__main__":
    sys.exit(main(argv=sys.argv[1:]))
