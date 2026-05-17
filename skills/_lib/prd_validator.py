"""Validate that PRDs produced by /to-prd follow the structured template."""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Final


REQUIRED_SECTIONS: Final[tuple[str, ...]] = (
    "Summary",
    "Deliverables",
    "Validations",
    "Modules to CREATE",
    "Modules to UPDATE",
    "Test plan",
    "Q&A",
)

HTML_TAGS: Final[frozenset[str]] = frozenset({
    "details", "summary", "br", "hr", "kbd", "code", "em", "strong",
    "sub", "sup", "blockquote", "a", "img", "p", "div", "span",
    "ul", "ol", "li", "table", "thead", "tbody", "tr", "td", "th",
    "h1", "h2", "h3", "h4", "h5", "h6", "pre", "b", "i", "u", "small",
    "mark", "del", "ins", "abbr", "cite", "q", "s", "var", "time",
})

PLACEHOLDER_KEYWORDS: Final[tuple[str, ...]] = ("TODO", "TBD", "FIXME", "XXX")

NONE_MARKER_RE: Final[re.Pattern[str]] = re.compile(r"^_?none_?$", re.IGNORECASE)
SECTION_HEADER_RE: Final[re.Pattern[str]] = re.compile(r"^##\s+(.+?)\s*$")
GOAL_MARKER_RE: Final[re.Pattern[str]] = re.compile(r"\*\*G(\d+)\*\*")
VALIDATION_SPLIT_RE: Final[re.Pattern[str]] = re.compile(r"\*\*V(\d+)\*\*")
G_CITATION_RE: Final[re.Pattern[str]] = re.compile(r"\bG(\d+)\b")
ANGLE_BRACKET_RE: Final[re.Pattern[str]] = re.compile(r"<([^>\n]+)>")
URL_PREFIX_RE: Final[re.Pattern[str]] = re.compile(r"^[a-z][a-z0-9+.-]*://")
SEPARATOR_CELL_RE: Final[re.Pattern[str]] = re.compile(r":?-+:?")
FENCED_CODE_RE: Final[re.Pattern[str]] = re.compile(r"```.*?```", flags=re.DOTALL)
INLINE_CODE_RE: Final[re.Pattern[str]] = re.compile(r"`[^`\n]*`")


@dataclass(frozen=True, kw_only=True)
class Error:
    """One validation failure with a stable code for programmatic handling."""

    code: str
    message: str

    def __str__(self) -> str:
        return f"[{self.code}] {self.message}"


def validate(*, body: str) -> list[Error]:
    """Run all PRD structure checks and return collected errors (empty = valid)."""
    errors: list[Error] = []
    sections = _parse_sections(text=body)
    errors.extend(_check_required_sections(sections=sections))

    goals = _parse_goals(text=sections.get("deliverables", ""))
    errors.extend(_check_sequential(nums=goals, kind="G"))

    validations = _parse_validations(text=sections.get("validations", ""))
    errors.extend(_check_sequential(nums=[n for n, _ in validations], kind="V"))
    errors.extend(_check_v_cites_g(validations=validations, known_gs=set(goals)))

    errors.extend(_check_placeholders(body=body))

    errors.extend(_check_module_table(
        section_body=sections.get("modules to create", ""),
        label="CREATE",
    ))
    errors.extend(_check_module_table(
        section_body=sections.get("modules to update", ""),
        label="UPDATE",
    ))

    return errors


def _parse_sections(*, text: str) -> dict[str, str]:
    """Split markdown by H2 headers; keys are lowercased section names."""
    sections: dict[str, str] = {}
    current: str | None = None
    buf: list[str] = []
    for line in text.splitlines():
        m = SECTION_HEADER_RE.match(line)
        if m:
            if current is not None:
                sections[current] = "\n".join(buf).strip()
            current = m.group(1).strip().lower()
            buf = []
        elif current is not None:
            buf.append(line)
    if current is not None:
        sections[current] = "\n".join(buf).strip()
    return sections


def _check_required_sections(*, sections: dict[str, str]) -> list[Error]:
    return [
        Error(code="MISSING_SECTION", message=f"Required section '## {name}' not found")
        for name in REQUIRED_SECTIONS
        if name.lower() not in sections
    ]


def _parse_goals(*, text: str) -> list[int]:
    """Extract G numbers from Deliverables section in document order."""
    return [int(n) for n in GOAL_MARKER_RE.findall(text)]


def _parse_validations(*, text: str) -> list[tuple[int, str]]:
    """Extract (V-number, V-block-text) pairs from Validations section."""
    parts = VALIDATION_SPLIT_RE.split(text)
    return [(int(parts[i]), parts[i + 1]) for i in range(1, len(parts), 2)]


def _check_sequential(*, nums: list[int], kind: str) -> list[Error]:
    if not nums:
        return [Error(code="NO_ITEMS", message=f"No {kind}n entries found")]
    errors: list[Error] = []
    seen: set[int] = set()
    for n in nums:
        if n in seen:
            errors.append(Error(
                code="DUPLICATE",
                message=f"{kind}{n} appears more than once",
            ))
        seen.add(n)
    expected = set(range(1, len(seen) + 1))
    for missing in sorted(expected - seen):
        errors.append(Error(
            code="MISSING_NUMBER",
            message=f"{kind}{missing} missing — numbering must be sequential from 1",
        ))
    for extra in sorted(seen - expected):
        errors.append(Error(
            code="OUT_OF_SEQUENCE",
            message=f"{kind}{extra} out of sequence — numbering must be 1..N with no gaps",
        ))
    return errors


def _check_v_cites_g(
    *,
    validations: list[tuple[int, str]],
    known_gs: set[int],
) -> list[Error]:
    errors: list[Error] = []
    for n, text in validations:
        cited = {int(m) for m in G_CITATION_RE.findall(text)}
        if not cited:
            errors.append(Error(
                code="V_NO_G_CITE",
                message=f"V{n} does not cite any G",
            ))
            continue
        for unknown in sorted(cited - known_gs):
            errors.append(Error(
                code="V_UNKNOWN_G",
                message=f"V{n} cites G{unknown} which is not defined in Deliverables",
            ))
    return errors


def _check_placeholders(*, body: str) -> list[Error]:
    text = _strip_code(text=body)
    errors: list[Error] = []
    for m in ANGLE_BRACKET_RE.finditer(text):
        content = m.group(1)
        if _is_html_tag(content=content) or _is_url(content=content):
            continue
        errors.append(Error(
            code="PLACEHOLDER",
            message=f"Found unresolved placeholder: '<{content}>'",
        ))
    for keyword in PLACEHOLDER_KEYWORDS:
        for _ in re.finditer(rf"\b{keyword}\b", text):
            errors.append(Error(
                code="PLACEHOLDER",
                message=f"Found unresolved '{keyword}'",
            ))
    return errors


def _strip_code(*, text: str) -> str:
    """Drop fenced and inline code so placeholder scans skip example snippets."""
    text = FENCED_CODE_RE.sub("", text)
    return INLINE_CODE_RE.sub("", text)


def _is_html_tag(*, content: str) -> bool:
    stripped = content.strip()
    if not stripped:
        return False
    name = stripped.lstrip("/").split()[0]
    return name.lower() in HTML_TAGS


def _is_url(*, content: str) -> bool:
    return bool(URL_PREFIX_RE.match(content.strip()))


def _check_module_table(*, section_body: str, label: str) -> list[Error]:
    stripped = section_body.strip()
    if not stripped or NONE_MARKER_RE.fullmatch(stripped):
        return []

    table_lines = [
        line for line in stripped.splitlines() if line.lstrip().startswith("|")
    ]
    if len(table_lines) < 3:
        return [Error(
            code="MODULE_TABLE_MISSING",
            message=(
                f"Modules to {label}: expected a markdown table "
                f"(header + separator + at least one row), or '_none_'"
            ),
        )]

    header = _parse_row(line=table_lines[0])
    separator = _parse_row(line=table_lines[1])
    if not _is_separator_row(cells=separator):
        return [Error(
            code="MODULE_TABLE_NO_SEPARATOR",
            message=f"Modules to {label}: second table line must be a separator row",
        )]

    errors: list[Error] = []
    expected_cols = len(header)
    for i, row_line in enumerate(table_lines[2:], start=1):
        row = _parse_row(line=row_line)
        if len(row) != expected_cols:
            errors.append(Error(
                code="MODULE_TABLE_BAD_ROW",
                message=(
                    f"Modules to {label}: row {i} has {len(row)} cells, "
                    f"expected {expected_cols}"
                ),
            ))
            continue
        if all(not cell.strip() for cell in row):
            errors.append(Error(
                code="MODULE_TABLE_EMPTY_ROW",
                message=f"Modules to {label}: row {i} is empty",
            ))
    return errors


def _parse_row(*, line: str) -> list[str]:
    parts = line.strip().split("|")
    if parts and not parts[0].strip():
        parts = parts[1:]
    if parts and not parts[-1].strip():
        parts = parts[:-1]
    return [p.strip() for p in parts]


def _is_separator_row(*, cells: list[str]) -> bool:
    return len(cells) > 0 and all(SEPARATOR_CELL_RE.fullmatch(c) for c in cells)


def _main(*, argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: python3 prd_validator.py <path-to-prd.md>", file=sys.stderr)
        return 2
    path = Path(argv[1])
    if not path.is_file():
        print(f"error: not a file: {path}", file=sys.stderr)
        return 2
    errors = validate(body=path.read_text(encoding="utf-8"))
    if not errors:
        print(f"OK: {path} passes PRD structure validation", file=sys.stderr)
        return 0
    print(f"FAIL: {path} has {len(errors)} validation error(s):", file=sys.stderr)
    for err in errors:
        print(f"  {err}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(_main(argv=sys.argv))
