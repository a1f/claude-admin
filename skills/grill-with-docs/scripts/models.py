"""Value objects exchanged between the grill-with-docs CLI subcommands."""

from dataclasses import dataclass, field


@dataclass(frozen=True)
class ContextReport:
    path: str
    exists: bool
    mtime_unix: float | None = None
    age_days: float | None = None
    terms: tuple[str, ...] = ()
    orphaned_terms: tuple[str, ...] = ()
    file_refs: tuple[str, ...] = ()
    missing_files: tuple[str, ...] = ()


@dataclass(frozen=True)
class FreshnessReport:
    repo_root: str
    has_context_map: bool
    stale: bool
    contexts: tuple[ContextReport, ...] = ()


@dataclass(frozen=True)
class ContextSnapshot:
    path: str
    exists: bool
    sha256: str | None = None
    terms: tuple[str, ...] = ()


@dataclass(frozen=True)
class Snapshot:
    repo_root: str
    contexts: tuple[ContextSnapshot, ...] = ()
    adr_files: tuple[str, ...] = ()


@dataclass(frozen=True)
class TermMismatch:
    name: str
    context: str
    # one of: "context-missing" | "term-not-written"
    reason: str


@dataclass(frozen=True)
class AdrMismatch:
    slug: str
    number: int | None
    # one of: "no-matching-file" | "already-existed"
    reason: str


@dataclass
class AuditReport:
    """Mutable during construction; serialised once complete."""

    clean: bool = True
    terms_ok: list[str] = field(default_factory=list)
    adrs_ok: list[str] = field(default_factory=list)
    term_mismatches: list[TermMismatch] = field(default_factory=list)
    adr_mismatches: list[AdrMismatch] = field(default_factory=list)
