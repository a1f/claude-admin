import re
from typing import Final

BOLD_TERM_RE: Final[re.Pattern[str]] = re.compile(r"\*\*([A-Z][\w/ -]{0,40})\*\*")
LANGUAGE_HEADING_RE: Final[re.Pattern[str]] = re.compile(
    r"^##\s+Language\s*$", re.MULTILINE
)
NEXT_HEADING_RE: Final[re.Pattern[str]] = re.compile(r"^##\s+", re.MULTILINE)
MD_LINK_RE: Final[re.Pattern[str]] = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
BACKTICK_PATH_RE: Final[re.Pattern[str]] = re.compile(r"`([^`\n]+/[^`\n]+)`")
CONTEXT_MAP_LINK_RE: Final[re.Pattern[str]] = re.compile(
    r"\[[^\]]+\]\(([^)]+CONTEXT\.md)\)"
)
ADR_FILE_RE: Final[re.Pattern[str]] = re.compile(r"^(\d{4})-([\w-]+)\.md$")

CODE_EXCLUDE_DIRS: Final[frozenset[str]] = frozenset({
    ".git", "target", "node_modules", "dist", "build",
    ".venv", "venv", "__pycache__", ".pytest_cache", ".mypy_cache",
})
DOC_EXTS: Final[frozenset[str]] = frozenset({".md", ".markdown", ".rst", ".txt"})

STALE_AGE_DAYS: Final[float] = 60.0
SECONDS_PER_DAY: Final[float] = 86400.0
