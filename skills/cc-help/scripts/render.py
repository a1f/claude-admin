#!/usr/bin/env python3
"""Render the claude-admin M1 pipeline reference.

Enumerates the 9 pipeline skills (in order), pulls the one-line purpose from
each skill's SKILL.md frontmatter `description` field, and prints a compact
text reference. Skills not yet installed render with a `(planned)` marker so
the user can see what's still coming.
"""

from __future__ import annotations

import sys
from pathlib import Path

# The 9-step M1 pipeline (PRD #16 G1 / V10). Order matters.
PIPELINE: list[str] = [
    "roadmap-plan",
    "milestone",
    "to-issues",
    "architector",
    "coder",
    "review",
    "critic",
    "pr-babysit",
    "distill-lessons",
]

# Used when a pipeline skill is not yet installed (no SKILL.md found),
# or when its frontmatter has no usable `description`.
FALLBACK_DESCRIPTIONS: dict[str, str] = {
    "roadmap-plan": "Plan the roadmap: high-level milestones for a multi-month effort",
    "milestone": "Turn one milestone into a PRD with deliverables + validations",
    "to-issues": "Break a PRD into vertical-slice issues with enriched context",
    "architector": "Per-slice runner: PR breakdown + plan-integrity owner",
    "coder": "Implement one PR with plan-pr + write-pr + self-review",
    "review": "Post-publish code-quality + bugs review on a PR",
    "critic": "Post-publish 'addresses task?' verdict (NOT quality)",
    "pr-babysit": "Watch PR lifecycle, route verdicts, diagnose on CI red",
    "distill-lessons": "Post-merge: append distilled rules to module LESSONS.md",
}


def parse_frontmatter(text: str) -> dict[str, str]:
    """Return key→value from YAML-ish frontmatter (single-line scalars only).

    Handles `key: value` and `key: "value"` / `key: 'value'`. Multi-line
    values, lists, and nested maps are out of scope — pipeline SKILL.md
    descriptions are always single-line scalars.
    """
    if not text.startswith("---"):
        return {}
    # Find the closing `---` line.
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        return {}
    end = -1
    for i in range(1, len(lines)):
        if lines[i].strip() == "---":
            end = i
            break
    if end == -1:
        return {}
    out: dict[str, str] = {}
    for line in lines[1:end]:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if ":" not in stripped:
            continue
        key, _, value = stripped.partition(":")
        v = value.strip()
        if (v.startswith('"') and v.endswith('"')) or (
            v.startswith("'") and v.endswith("'")
        ):
            v = v[1:-1]
        out[key.strip()] = v
    return out


def one_line(description: str) -> str:
    """Take the first sentence of `description` (cuts at '. ' or '.\\n')."""
    if not description:
        return ""
    # Cut at first sentence boundary.
    for sep in (". ", ".\n"):
        idx = description.find(sep)
        if idx != -1:
            return description[:idx].rstrip()
    return description.rstrip().rstrip(".")


def render(skills_dir: Path) -> str:
    """Render the pipeline reference as a multi-line string."""
    header = "claude-admin M1 pipeline (9 steps)"
    lines: list[str] = [header, "=" * len(header), ""]
    width = max(len(name) for name in PIPELINE) + 1  # +1 for the leading '/'
    for i, name in enumerate(PIPELINE, start=1):
        skill_path = skills_dir / name / "SKILL.md"
        marker = ""
        desc = ""
        if skill_path.is_file():
            fm = parse_frontmatter(skill_path.read_text(encoding="utf-8"))
            desc = one_line(fm.get("description", ""))
        if not desc:
            desc = FALLBACK_DESCRIPTIONS[name]
            marker = "  (planned)"
        slot = f"/{name}".ljust(width)
        lines.append(f"{i:>2}. {slot}  - {desc}{marker}")
    lines.append("")
    return "\n".join(lines)


def _default_skills_dir() -> Path:
    """`<repo>/skills/` — render.py lives at `<repo>/skills/cc-help/scripts/`."""
    return Path(__file__).resolve().parent.parent.parent


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    skills_dir = Path(args[0]) if args else _default_skills_dir()
    print(render(skills_dir))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
