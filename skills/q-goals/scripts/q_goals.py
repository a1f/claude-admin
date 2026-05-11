#!/usr/bin/env python3
"""
q_goals.py — draft + ratify immutable goals for a milestone (manual mode).

Usage:
    python3 q_goals.py <plan-codename> <milestone-id>

Side effects:
    - creates <plan_dir>/<milestone-id>/
    - writes goals.md + validations.md from templates if missing
    - opens each in $EDITOR; loops on validator failure
    - on ratify: chmod 444 + writes .ratified.json

Exits 0 on ratify. Exits 1 on user-abort. Exits 2 on config errors. Exits 3 if already ratified.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

# ----------------------------------------------------------------------------
# Paths and constants
# ----------------------------------------------------------------------------

REGISTRY = Path.home() / ".claude" / "plans" / "registry.json"
TEMPLATES_DIR = Path(__file__).parent / "templates"

GOALS_PLACEHOLDERS = [
    "<short name>",
    "<concrete signal — file exists / test passes / command returns X>",
    "<1 line, the load-bearing reason>",
    "<signal>",
    "<reason>",
]

VALIDATIONS_PLACEHOLDERS = [
    "<one line>",
    "<test file path or shell command>",
    "test_name_snake_case",
    "e2e_scenario_snake_case",
]

GOAL_ITEM_RE = re.compile(r"^- \[[ x]\] \*\*G(\d+)\*\* · (.+)$")
VALIDATION_ITEM_RE = re.compile(
    r"^- \*\*V(\d+)\*\* · _(unit|integration|e2e|manual)_ — `([a-z][a-z0-9_]*)` — covers (G\d+(?:, G\d+)*)$"
)
SECTION_RE = re.compile(r"^## (.+)$")


def die(msg: str, code: int = 2) -> None:
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(code)


# ----------------------------------------------------------------------------
# Registry + plan resolution
# ----------------------------------------------------------------------------

def load_plan_dir(plan_codename: str) -> Path:
    if not REGISTRY.exists():
        die(f"registry not found at {REGISTRY}")
    registry = json.loads(REGISTRY.read_text())
    plan = registry.get("plans", {}).get(plan_codename)
    if not plan:
        die(f"plan '{plan_codename}' not in registry")
    plan_dir = Path(plan["plan_dir"])
    if not plan_dir.exists():
        die(f"plan_dir does not exist: {plan_dir}")
    return plan_dir


# ----------------------------------------------------------------------------
# Template handling
# ----------------------------------------------------------------------------

def init_files(milestone_dir: Path, milestone_id: str) -> None:
    """Create milestone dir + write templates if missing. Idempotent — never overwrites."""
    milestone_dir.mkdir(parents=True, exist_ok=True)
    for name in ("goals.md", "validations.md"):
        dest = milestone_dir / name
        if dest.exists():
            continue
        body = (TEMPLATES_DIR / f"{name}.template").read_text()
        dest.write_text(body.replace("<milestone-id>", milestone_id))


def is_ratified(milestone_dir: Path) -> bool:
    return (milestone_dir / ".ratified.json").exists()


# ----------------------------------------------------------------------------
# Validators
# ----------------------------------------------------------------------------

def _section_blocks(text: str, section_name: str, head_re: re.Pattern[str]) -> list[list[str]]:
    """Return a list of bullet-blocks (each = list of lines) under `## <section_name>`.

    A new block starts at every line matching `head_re`; subsequent lines belong to the
    most recent block until the next head match or section change.
    """
    blocks: list[list[str]] = []
    current: list[str] | None = None
    in_section = False
    for line in text.splitlines():
        m_sec = SECTION_RE.match(line)
        if m_sec:
            in_section = m_sec.group(1).strip() == section_name
            if current is not None:
                blocks.append(current)
                current = None
            continue
        if not in_section:
            continue
        if head_re.match(line):
            if current is not None:
                blocks.append(current)
            current = [line]
        elif current is not None:
            current.append(line)
    if current is not None:
        blocks.append(current)
    return blocks


def _has_section(text: str, name: str) -> bool:
    for line in text.splitlines():
        m = SECTION_RE.match(line)
        if m and m.group(1).strip() == name:
            return True
    return False


def _validate_items_section(
    text: str,
    *,
    section: str,
    head_re: re.Pattern[str],
    required_fields: list[str],
    item_label: str,
    item_noun: str,
    placeholders: list[str],
) -> tuple[list[str], list[int], list[list[str]]]:
    """Shared structural validator for items-under-a-section markdown.

    Returns (errors, item_numbers_in_order, raw_blocks_for_caller_specific_checks).
    """
    errors: list[str] = []
    for ph in placeholders:
        if ph in text:
            errors.append(f"unfilled template placeholder: {ph!r}")
    if not _has_section(text, section):
        errors.append(f"missing `## {section}` section")
        return errors, [], []
    blocks = _section_blocks(text, section, head_re)
    if not blocks:
        errors.append(f"no {item_noun} items found under `## {section}`")
        return errors, [], []
    numbers: list[int] = []
    for block in blocks:
        m = head_re.match(block[0])
        if not m:
            errors.append(f"{item_noun} head line malformed: {block[0]!r}")
            continue
        n = int(m.group(1))
        numbers.append(n)
        body = "\n".join(block[1:])
        for field in required_fields:
            if field not in body:
                errors.append(f"{item_label}{n}: missing `{field}` line")
    if len(numbers) != len(set(numbers)):
        errors.append(f"duplicate {item_label}-numbers: {numbers}")
    if sorted(numbers) != list(range(1, len(numbers) + 1)):
        errors.append(f"{item_label}-numbers not sequential from 1: got {sorted(numbers)}")
    return errors, numbers, blocks


def validate_goals(text: str) -> tuple[list[str], list[int]]:
    """Validate goals.md content. Returns (errors, defined_g_numbers_in_order)."""
    errors, g_numbers, _ = _validate_items_section(
        text,
        section="Deliverables",
        head_re=GOAL_ITEM_RE,
        required_fields=["**Observable when:**", "**Why:**"],
        item_label="G",
        item_noun="goal",
        placeholders=GOALS_PLACEHOLDERS,
    )
    return errors, g_numbers


def validate_validations(text: str, defined_g: list[int]) -> tuple[list[str], list[str]]:
    """Validate validations.md content. Returns (errors, warnings)."""
    errors, _v_numbers, blocks = _validate_items_section(
        text,
        section="Scenarios",
        head_re=VALIDATION_ITEM_RE,
        required_fields=["**What it tests:**", "**How:**"],
        item_label="V",
        item_noun="scenario",
        placeholders=VALIDATIONS_PLACEHOLDERS,
    )
    defined_set = set(defined_g)
    covered_g: set[int] = set()
    for block in blocks:
        m = VALIDATION_ITEM_RE.match(block[0])
        if not m:
            continue
        v_num = int(m.group(1))
        for g_ref in m.group(4).split(","):
            g_n = int(g_ref.strip().lstrip("G"))
            if g_n not in defined_set:
                errors.append(f"V{v_num}: covers undefined goal G{g_n}")
            else:
                covered_g.add(g_n)
    warnings: list[str] = []
    uncovered = sorted(defined_set - covered_g)
    if uncovered:
        warnings.append(f"goals without any validation: {[f'G{g}' for g in uncovered]}")
    return errors, warnings


# ----------------------------------------------------------------------------
# Editor loop
# ----------------------------------------------------------------------------

def run_editor(path: Path) -> None:
    editor = os.environ.get("EDITOR", "vi")
    subprocess.run([editor, str(path)], check=False)


def prompt_yn(question: str, default: bool = False) -> bool:
    suffix = " [Y/n]" if default else " [y/N]"
    resp = input(question + suffix + " ").strip().lower()
    if not resp:
        return default
    return resp in {"y", "yes"}


def edit_and_validate_goals(goals_file: Path) -> list[int]:
    while True:
        run_editor(goals_file)
        errors, g_numbers = validate_goals(goals_file.read_text())
        if not errors:
            print(f"✓ goals.md: {len(g_numbers)} deliverable(s) — {[f'G{g}' for g in g_numbers]}")
            return g_numbers
        print(f"✗ goals.md has {len(errors)} error(s):", file=sys.stderr)
        for e in errors:
            print(f"   - {e}", file=sys.stderr)
        if not prompt_yn("re-open in editor?", default=True):
            sys.exit(1)


def edit_and_validate_validations(validations_file: Path, defined_g: list[int]) -> None:
    while True:
        run_editor(validations_file)
        errors, warnings = validate_validations(validations_file.read_text(), defined_g)
        if not errors:
            for w in warnings:
                print(f"  ⚠ {w}", file=sys.stderr)
            print(f"✓ validations.md: clean ({len(warnings)} warning(s))")
            return
        print(f"✗ validations.md has {len(errors)} error(s):", file=sys.stderr)
        for e in errors:
            print(f"   - {e}", file=sys.stderr)
        if not prompt_yn("re-open in editor?", default=True):
            sys.exit(1)


# ----------------------------------------------------------------------------
# Ratify
# ----------------------------------------------------------------------------

def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ratify(milestone_dir: Path, plan_codename: str, milestone_id: str) -> None:
    goals_file = milestone_dir / "goals.md"
    validations_file = milestone_dir / "validations.md"
    payload = {
        "ratified_at": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
        "plan": plan_codename,
        "milestone": milestone_id,
        "goals_sha256": sha256_file(goals_file),
        "validations_sha256": sha256_file(validations_file),
    }
    (milestone_dir / ".ratified.json").write_text(json.dumps(payload, indent=2))
    goals_file.chmod(0o444)
    validations_file.chmod(0o444)
    print(f"✓ ratified · {milestone_dir}")
    print(f"  goals.md       sha256={payload['goals_sha256']}")
    print(f"  validations.md sha256={payload['validations_sha256']}")
    print(f"  .ratified.json written ({payload['ratified_at']})")


# ----------------------------------------------------------------------------
# Main
# ----------------------------------------------------------------------------

def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser(description="Draft + ratify immutable goals for a milestone.")
    ap.add_argument("plan_codename")
    ap.add_argument("milestone_id")
    return ap.parse_args()


def main() -> int:
    args = parse_args()
    plan_dir = load_plan_dir(args.plan_codename)
    milestone_dir = plan_dir / args.milestone_id

    if is_ratified(milestone_dir):
        info = json.loads((milestone_dir / ".ratified.json").read_text())
        die(f"frozen on {info['ratified_at']} "
            f"(goals={info['goals_sha256'][:12]}…, "
            f"validations={info['validations_sha256'][:12]}…) — "
            f"remove .ratified.json to reopen", code=3)

    init_files(milestone_dir, args.milestone_id)
    print(f"Editing goals + validations for {args.plan_codename}/{args.milestone_id}")
    print(f"   dir: {milestone_dir}")
    print()

    g_numbers = edit_and_validate_goals(milestone_dir / "goals.md")
    edit_and_validate_validations(milestone_dir / "validations.md", g_numbers)

    print()
    print(f"Summary: {len(g_numbers)} deliverable(s); validations clean.")
    if not prompt_yn("ratify these goals (immutable after)?", default=False):
        print("not ratified — files left as drafts.")
        return 1
    ratify(milestone_dir, args.plan_codename, args.milestone_id)
    return 0


if __name__ == "__main__":
    sys.exit(main())
