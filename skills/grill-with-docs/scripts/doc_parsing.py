"""Shared parsers for CONTEXT.md / ADR files and code-orphan detection."""

import re
import subprocess
from pathlib import Path

from constants import (
    BACKTICK_PATH_RE,
    BOLD_TERM_RE,
    CODE_EXCLUDE_DIRS,
    CONTEXT_MAP_LINK_RE,
    DOC_EXTS,
    LANGUAGE_HEADING_RE,
    MD_LINK_RE,
    NEXT_HEADING_RE,
)


def parse_terms(*, text: str) -> tuple[str, ...]:
    """Bold-term definitions are the canonical glossary entries — extract them."""
    heading = LANGUAGE_HEADING_RE.search(text)
    if not heading:
        return ()
    rest = text[heading.end():]
    nxt = NEXT_HEADING_RE.search(rest)
    section = rest[: nxt.start()] if nxt else rest
    seen: list[str] = []
    seen_set: set[str] = set()
    for match in BOLD_TERM_RE.finditer(section):
        term = match.group(1).strip()
        if term and term not in seen_set:
            seen.append(term)
            seen_set.add(term)
    return tuple(seen)


def parse_file_refs(
    *, text: str, context_dir: Path, repo_root: Path
) -> tuple[str, ...]:
    """Pull out paths the doc points at so we can detect dangling references."""
    refs: list[str] = []
    seen: set[str] = set()
    resolved_root = repo_root.resolve()

    def _record(raw: str) -> None:
        raw = raw.strip()
        if not raw or raw.startswith("#"):
            return
        if "://" in raw or raw.startswith("mailto:"):
            return
        path_part = raw.split("#", 1)[0].split("?", 1)[0]
        if not path_part:
            return
        candidate = (context_dir / path_part).resolve()
        try:
            rel = candidate.relative_to(resolved_root)
        except ValueError:
            return
        rel_str = rel.as_posix()
        if rel_str not in seen:
            refs.append(rel_str)
            seen.add(rel_str)

    for match in MD_LINK_RE.finditer(text):
        _record(match.group(1))
    for match in BACKTICK_PATH_RE.finditer(text):
        candidate = match.group(1)
        if " " in candidate:
            continue
        _record(candidate)
    return tuple(refs)


def discover_contexts(*, repo_root: Path) -> tuple[bool, tuple[Path, ...]]:
    """Resolve CONTEXT-MAP.md if present, else fall back to root CONTEXT.md."""
    cmap = repo_root / "CONTEXT-MAP.md"
    if cmap.exists():
        text = cmap.read_text(encoding="utf-8")
        paths: list[Path] = []
        seen: set[Path] = set()
        for match in CONTEXT_MAP_LINK_RE.finditer(text):
            target = (cmap.parent / match.group(1)).resolve()
            if target not in seen:
                paths.append(target)
                seen.add(target)
        return True, tuple(paths)
    root_ctx = repo_root / "CONTEXT.md"
    return False, ((root_ctx,) if root_ctx.exists() else ())


def _ripgrep_available() -> bool:
    return subprocess.run(
        ["which", "rg"], capture_output=True, text=True
    ).returncode == 0


def term_orphaned(*, term: str, repo_root: Path) -> bool:
    """A glossary term with zero hits in non-doc files is dead weight to flag."""
    pattern = rf"\b{re.escape(term)}\b"
    if _ripgrep_available():
        cmd = [
            "rg", "--quiet", "--no-messages",
            "--type-not", "md",
            "-e", pattern,
        ]
        for excluded in CODE_EXCLUDE_DIRS:
            cmd += ["--glob", f"!{excluded}"]
        cmd.append(str(repo_root))
        return subprocess.run(cmd, capture_output=True).returncode != 0
    return _python_fallback_orphan(term=term, repo_root=repo_root)


def _python_fallback_orphan(*, term: str, repo_root: Path) -> bool:
    needle = re.compile(rf"\b{re.escape(term)}\b")
    for path in repo_root.rglob("*"):
        if not path.is_file():
            continue
        if any(part in CODE_EXCLUDE_DIRS for part in path.parts):
            continue
        if path.suffix.lower() in DOC_EXTS:
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        if needle.search(text):
            return False
    return True
