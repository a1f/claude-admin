"""Inline PRD context, module-impact-matrix rows, and neighbouring LESSONS.md
into a vertical-slice issue body before publishing.

Used by `skills/to-issues/SKILL.md` between the slice-draft and publish steps.
Validation: V6 (`slice_enrichment_inlines_context`) in PRD a1f/claude-admin#16.
"""

from __future__ import annotations

import re
from collections.abc import Iterator
from dataclasses import dataclass
from functools import lru_cache
from typing import Literal, TypedDict


ValidationKind = Literal["e2e", "module", "unit", "manual"]


class SliceDraft(TypedDict, total=False):
    title: str
    type: str
    deliverable: str
    acceptance: list[str]
    validations: list[str]
    blocked_by: list[str]
    modules: list[str]
    notes: str | None
    parent: str | None


@dataclass(frozen=True)
class _Deliverable:
    id: str
    title: str
    body: str


@dataclass(frozen=True)
class _Validation:
    id: str
    kind: ValidationKind | str
    name: str
    covers: tuple[str, ...]
    body: str


@dataclass(frozen=True)
class _ModuleRow:
    name: str
    path: str
    responsibility: str
    interface: str
    tests: str
    section: str


@dataclass(frozen=True)
class _ParsedPRD:
    deliverables: dict[str, _Deliverable]
    validations: dict[str, _Validation]
    modules: tuple[_ModuleRow, ...]


def enrich(
    slice_draft: SliceDraft,
    prd: str,
    modules_md: str = "",
    *,
    lessons: dict[str, str] | None = None,
) -> str:
    """Render the enriched slice-issue body as markdown.

    Args:
      slice_draft: draft slice fields (title, type, deliverable, acceptance,
        validations, blocked_by, modules, notes, parent).
      prd: full PRD markdown body. Parsed for deliverables, validations, and
        module-impact-matrix tables (`## modules to CREATE` / `## modules to UPDATE`).
      modules_md: optional extra module-impact-matrix markdown (treated as a
        standalone CREATE table) appended to whatever the PRD already supplies.
      lessons: optional mapping `{module_name: LESSONS.md body}` to inline as
        neighbouring lessons; callers load these from `modules/<name>/LESSONS.md`.

    Returns the slice issue body as one markdown string.
    """
    parsed = _parse_prd(prd, modules_md)
    cited_vs = [
        parsed.validations[vid]
        for vid in slice_draft.get("validations", [])
        if vid in parsed.validations
    ]
    cited_gs = _collect_deliverables(parsed, cited_vs)
    matched_rows = _match_modules(parsed.modules, slice_draft.get("modules", []))

    sections = [
        _render_parent(slice_draft),
        _render_deliverable(slice_draft),
        _render_e2e_covered(cited_vs),
        _render_module_test(cited_vs),
        _render_definition_of_done(slice_draft, cited_vs),
        _render_context(cited_gs, matched_rows, lessons or {}),
        _render_blocked_by(slice_draft),
    ]
    return "\n\n".join(s for s in sections if s)


# ---------- PRD parsing ----------


@lru_cache(maxsize=4)
def _parse_prd(prd: str, extra_modules_md: str = "") -> _ParsedPRD:
    """Parse PRD once per (prd, extra_modules_md) pair — `enrich()` calls this N times per
    `/to-issues` run (one per slice) with identical inputs; the cache makes that O(1)."""
    deliverables = _parse_deliverables(_section(prd, "deliverables"))
    validations = _parse_validations(_section(prd, "validations"))
    create_rows = _parse_module_table(_section(prd, "modules to CREATE"), "CREATE")
    update_rows = _parse_module_table(_section(prd, "modules to UPDATE"), "UPDATE")
    extra_rows = (
        _parse_module_table(extra_modules_md, "CREATE") if extra_modules_md.strip() else []
    )
    return _ParsedPRD(
        deliverables=deliverables,
        validations=validations,
        modules=tuple(create_rows + update_rows + extra_rows),
    )


def _section(text: str, header: str) -> str:
    """Return the body of a `## <header>` section up to the next `## ` (or EOF)."""
    m = re.search(rf"(?m)^##\s+{re.escape(header)}\b.*?$", text)
    if not m:
        return ""
    rest = text[m.end():]
    nxt = re.search(r"(?m)^##\s", rest)
    return rest[: nxt.start()] if nxt else rest


_DELIVERABLE_HEADER = re.compile(r"(?m)^- \[[ x]\] \*\*(G\d+)\*\* · (.+?)$")
_VALIDATION_HEADER = re.compile(
    r"(?m)^- \[[ x]\] \*\*(V\d+)\*\* · _(\w+)_ — `([^`]+)` — covers ([^\n]+?)$"
)


def _iter_header_blocks(
    header_re: re.Pattern[str], body: str
) -> Iterator[tuple[re.Match[str], str]]:
    """Yield (header_match, full_block_text) for each header, where block spans up to
    the next header or end-of-body."""
    if not body.strip():
        return
    matches = list(header_re.finditer(body))
    for i, m in enumerate(matches):
        end = matches[i + 1].start() if i + 1 < len(matches) else len(body)
        yield m, body[m.start():end].rstrip()


def _parse_deliverables(section_body: str) -> dict[str, _Deliverable]:
    return {
        m.group(1): _Deliverable(id=m.group(1), title=m.group(2).strip(), body=block)
        for m, block in _iter_header_blocks(_DELIVERABLE_HEADER, section_body)
    }


def _parse_validations(section_body: str) -> dict[str, _Validation]:
    out: dict[str, _Validation] = {}
    for m, block in _iter_header_blocks(_VALIDATION_HEADER, section_body):
        # G-id range syntax (e.g. `G1–G13`) is intentionally NOT expanded — the
        # slice author should cite individual Gs they cover.
        covers = tuple(
            c.strip() for c in re.split(r"[,\s]+", m.group(4).strip()) if c.strip()
        )
        out[m.group(1)] = _Validation(
            id=m.group(1),
            kind=m.group(2).strip(),
            name=m.group(3).strip(),
            covers=covers,
            body=block,
        )
    return out


def _parse_module_table(section_body: str, section_label: str) -> list[_ModuleRow]:
    if not section_body.strip():
        return []
    pipe_lines = [ln for ln in section_body.splitlines() if ln.strip().startswith("|")]
    if len(pipe_lines) < 3:
        return []
    rows: list[_ModuleRow] = []
    for line in pipe_lines[2:]:
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 5:
            continue
        name, path, resp, iface, tests = cells[0], cells[1], cells[2], cells[3], cells[4]
        rows.append(
            _ModuleRow(
                name=name,
                path=path.strip("`"),
                responsibility=resp,
                interface=iface,
                tests=tests,
                section=section_label,
            )
        )
    return rows


# ---------- selection ----------


def _collect_deliverables(
    parsed: _ParsedPRD, validations: list[_Validation]
) -> list[_Deliverable]:
    seen: set[str] = set()
    out: list[_Deliverable] = []
    for v in validations:
        for gid in v.covers:
            if gid in parsed.deliverables and gid not in seen:
                seen.add(gid)
                out.append(parsed.deliverables[gid])
    return out


def _match_modules(
    rows: tuple[_ModuleRow, ...], wanted: list[str]
) -> list[_ModuleRow]:
    """Match by exact `name` or exact path segment — substring matching would silently
    pair `"foo"` with `foobar.py` or `name="foobar"`."""
    matched: list[_ModuleRow] = []
    seen: set[tuple[str, str]] = set()
    for w in wanted:
        if not w:
            continue
        for r in rows:
            if r.name == w or w in r.path.split("/"):
                key = (r.name, r.path)
                if key not in seen:
                    seen.add(key)
                    matched.append(r)
    return matched


# ---------- rendering ----------


def _render_parent(slice_draft: SliceDraft) -> str:
    parent = (slice_draft.get("parent") or "").strip()
    return f"## Parent\n\n{parent}" if parent else ""


def _render_deliverable(slice_draft: SliceDraft) -> str:
    deliverable = (slice_draft.get("deliverable") or "").strip()
    return f"## Deliverable\n\n{deliverable}" if deliverable else ""


def _render_e2e_covered(validations: list[_Validation]) -> str:
    return _render_validation_section("E2E covered", "e2e", validations)


def _render_module_test(validations: list[_Validation]) -> str:
    return _render_validation_section("Module-test", "module", validations)


def _render_validation_section(
    header: str, kind: ValidationKind, validations: list[_Validation]
) -> str:
    items = [v for v in validations if v.kind == kind]
    if not items:
        return f"## {header}\n\n_None._"
    lines = [
        f"- **{v.id}** `{v.name}` — covers {', '.join(v.covers)}" for v in items
    ]
    return f"## {header}\n\n" + "\n".join(lines)


def _render_definition_of_done(
    slice_draft: SliceDraft, validations: list[_Validation]
) -> str:
    crits: list[str] = []
    for raw in slice_draft.get("acceptance", []):
        a = raw.strip()
        if not a:
            continue
        crits.append(a if a.startswith(("- [ ]", "- [x]")) else f"- [ ] {a}")
    for v in validations:
        crits.append(f"- [ ] **{v.id}** ({v.kind}) `{v.name}` passes")
    return "## Definition of done\n\n" + "\n".join(crits) if crits else ""


def _render_context(
    deliverables: list[_Deliverable],
    modules: list[_ModuleRow],
    lessons: dict[str, str],
) -> str:
    parts: list[str] = ["## Context (enriched)"]

    parts.append("### PRD excerpts")
    if deliverables:
        for d in deliverables:
            parts.append(
                f"<details><summary>{d.id} · {d.title}</summary>\n\n{d.body}\n\n</details>"
            )
    else:
        parts.append("_No matching PRD deliverables found._")

    parts.append("### Module-impact matrix")
    if modules:
        header = "| section | name | path | responsibility | interface | tests |"
        sep = "|---|---|---|---|---|---|"
        body = [
            f"| {r.section} | {r.name} | `{r.path}` | {r.responsibility} | {r.interface} | {r.tests} |"
            for r in modules
        ]
        parts.append("\n".join([header, sep, *body]))
    else:
        parts.append("_No matching module rows._")

    parts.append("### Neighbouring lessons")
    if lessons:
        for mod, body in lessons.items():
            parts.append(
                f"<details><summary>modules/{mod}/LESSONS.md</summary>\n\n{body.strip()}\n\n</details>"
            )
    else:
        parts.append("_No neighbouring `LESSONS.md` provided._")

    return "\n\n".join(parts)


def _render_blocked_by(slice_draft: SliceDraft) -> str:
    bb = [b.strip() for b in slice_draft.get("blocked_by", []) if b and b.strip()]
    if not bb:
        return "## Blocked by\n\nNone — can start immediately."
    return "## Blocked by\n\n" + "\n".join(f"- {b}" for b in bb)


__all__ = ["enrich", "SliceDraft"]
