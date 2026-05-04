#!/usr/bin/env bash
# uninstall.sh — remove the symlinks that install.sh created.
#
# Only removes symlinks that point back to <repo>/skills/<name>.
# Real directories and symlinks pointing elsewhere are left alone.
# Does NOT touch ~/.claude/plans/registry.json.

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILLS_SRC="${REPO_DIR}/skills"
SKILLS_DST="${HOME}/.claude/skills"

removed=()
skipped=()

for src_dir in "${SKILLS_SRC}"/*/; do
  [ -d "${src_dir}" ] || continue
  skill="$(basename "${src_dir%/}")"
  src_canonical="${src_dir%/}"
  dst="${SKILLS_DST}/${skill}"

  if [ -L "${dst}" ]; then
    target="$(readlink "${dst}")"
    if [ "${target}" = "${src_canonical}" ]; then
      rm "${dst}"
      removed+=("${skill}")
    else
      skipped+=("${skill} (points to ${target}, not ours)")
    fi
  elif [ -e "${dst}" ]; then
    skipped+=("${skill} (real dir at ${dst})")
  fi
done

echo "claude_admin uninstall"
if [ ${#removed[@]} -gt 0 ]; then
  echo "Removed symlinks:"
  for s in "${removed[@]}"; do echo "  - ${s}"; done
fi
if [ ${#skipped[@]} -gt 0 ]; then
  echo "Skipped:"
  for s in "${skipped[@]}"; do echo "  = ${s}"; done
fi
echo
echo "Note: ~/.claude/plans/registry.json was NOT touched."
echo "Done."
