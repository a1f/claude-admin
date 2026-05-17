#!/usr/bin/env python3
"""Renderer that the /cc-help skill prints verbatim.

A newcomer cloning the repo runs `./install.sh` then `/cc-help`; this script
is what the skill executes to produce that 30-second pipeline overview. Lives
in scripts/ (next to the SKILL.md) so the skill can locate it via $BASH_SOURCE.
"""

import sys
from pathlib import Path
from typing import Final

# Declared order = display order. PRD #16 G1 / slice issue #17 V10 say the
# output must list these 9 names in this exact sequence.
PIPELINE: Final[tuple[str, ...]] = (
    "roadmap-plan",
    "milestone",
    "to-issues",
    "architector",
    "coder",
    "review",
    "critic",
    "pr-babysit",
    "distill-lessons",
)

# Used when a pipeline skill's SKILL.md doesn't exist yet (slice not shipped)
# or its frontmatter description is empty. Keeping the (planned) entries
# informative lets /cc-help double as a "what's still coming" reference.
FALLBACK_DESCRIPTIONS: Final[dict[str, str]] = {
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


def parse_frontmatter(*, text: str) -> dict[str, str]:
    """Extracted standalone so /cc-help stays stdlib-only (no PyYAML dependency)."""
    if not text.startswith("---"):
        return {}
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        return {}
    end: int = -1
    for i in range(1, len(lines)):
        if lines[i].strip() == "---":
            end = i
            break
    if end == -1:
        return {}
    out: dict[str, str] = {}
    for line in lines[1:end]:
        stripped: str = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if ":" not in stripped:
            continue
        key, _, value = stripped.partition(":")
        v: str = value.strip()
        if (v.startswith('"') and v.endswith('"')) or (
            v.startswith("'") and v.endswith("'")
        ):
            v = v[1:-1]
        out[key.strip()] = v
    return out


def one_line(*, description: str) -> str:
    """Pipeline rows must fit on one terminal line — trim to first sentence."""
    if not description:
        return ""
    for sep in (". ", ".\n"):
        idx: int = description.find(sep)
        if idx != -1:
            return description[:idx].rstrip()
    return description.rstrip().rstrip(".")


def render(*, skills_dir: Path) -> str:
    """Output is printed verbatim by the skill — formatting changes are user-visible."""
    header: str = "claude-admin M1 pipeline (9 steps)"
    lines: list[str] = [header, "=" * len(header), ""]
    width: int = max(len(name) for name in PIPELINE) + 1  # +1 for leading '/'
    for i, name in enumerate(PIPELINE, start=1):
        skill_path: Path = skills_dir / name / "SKILL.md"
        marker: str = ""
        desc: str = ""
        if skill_path.is_file():
            fm: dict[str, str] = parse_frontmatter(text=skill_path.read_text(encoding="utf-8"))
            desc = one_line(description=fm.get("description", ""))
        if not desc:
            desc = FALLBACK_DESCRIPTIONS[name]
            marker = "  (planned)"
        slot: str = f"/{name}".ljust(width)
        lines.append(f"{i:>2}. {slot}  - {desc}{marker}")
    lines.append("")
    return "\n".join(lines)


def _default_skills_dir() -> Path:
    """Self-locate so the skill works regardless of where claude is invoked from."""
    return Path(__file__).resolve().parent.parent.parent


def main(*, argv: list[str] | None = None) -> int:
    """Optional argv lets tests render against a fake skills dir."""
    args: list[str] = list(sys.argv[1:] if argv is None else argv)
    skills_dir: Path = Path(args[0]) if args else _default_skills_dir()
    print(render(skills_dir=skills_dir))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
