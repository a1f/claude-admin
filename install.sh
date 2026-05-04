#!/usr/bin/env bash
# install.sh — symlink claude_admin's skills into ~/.claude/skills/
#
# Discovery-based: for every directory under <repo>/skills/, install a symlink at
# ~/.claude/skills/<name> pointing back to the repo. Re-run after adding new skills.
#
# Idempotent: existing symlinks pointing at the right target are left alone.
# Refuses to overwrite a real directory (you must move/remove it manually).
#
# Usage:
#     ./install.sh
#     ./install.sh --force   # remove existing real dirs in ~/.claude/skills/<name> and replace with symlinks

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLAUDE_DIR="${HOME}/.claude"
SKILLS_SRC="${REPO_DIR}/skills"
SKILLS_DST="${CLAUDE_DIR}/skills"
PLANS_DIR="${CLAUDE_DIR}/plans"
REGISTRY_DST="${PLANS_DIR}/registry.json"
REGISTRY_TPL="${REPO_DIR}/plans-registry.template.json"

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force|-f) FORCE=1 ;;
    -h|--help)
      sed -n '2,15p' "${BASH_SOURCE[0]}"
      exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

if [ ! -d "${SKILLS_SRC}" ]; then
  echo "error: ${SKILLS_SRC} does not exist" >&2
  exit 1
fi

mkdir -p "${SKILLS_DST}" "${PLANS_DIR}"

echo "claude_admin install"
echo "  repo:   ${REPO_DIR}"
echo "  target: ${SKILLS_DST}"
echo

installed=()
relinked=()
skipped=()
errors=()

for src_dir in "${SKILLS_SRC}"/*/; do
  [ -d "${src_dir}" ] || continue
  skill="$(basename "${src_dir%/}")"
  src_canonical="${src_dir%/}"
  dst="${SKILLS_DST}/${skill}"

  if [ -L "${dst}" ]; then
    current_target="$(readlink "${dst}")"
    if [ "${current_target}" = "${src_canonical}" ]; then
      skipped+=("${skill} (already linked)")
      continue
    fi
    rm "${dst}"
    ln -s "${src_canonical}" "${dst}"
    relinked+=("${skill}  (was -> ${current_target})")
  elif [ -e "${dst}" ]; then
    if [ "${FORCE}" -eq 1 ]; then
      rm -rf "${dst}"
      ln -s "${src_canonical}" "${dst}"
      relinked+=("${skill}  (replaced real dir)")
    else
      errors+=("${skill}: ${dst} exists and is not a symlink. Move/remove it, or rerun with --force.")
    fi
  else
    ln -s "${src_canonical}" "${dst}"
    installed+=("${skill}")
  fi
done

# Registry: only create if missing, never clobber.
if [ ! -f "${REGISTRY_DST}" ]; then
  if [ -f "${REGISTRY_TPL}" ]; then
    sed "s|__HOME__|${HOME}|g" "${REGISTRY_TPL}" > "${REGISTRY_DST}"
    registry_msg="created from template at ${REGISTRY_DST}"
  else
    registry_msg="no template found at ${REGISTRY_TPL} — registry not created"
  fi
else
  registry_msg="exists at ${REGISTRY_DST} (untouched)"
fi

# Report
if [ ${#installed[@]} -gt 0 ]; then
  echo "Installed:"
  for s in "${installed[@]}"; do echo "  + ${s}"; done
fi
if [ ${#relinked[@]} -gt 0 ]; then
  echo "Relinked:"
  for s in "${relinked[@]}"; do echo "  ~ ${s}"; done
fi
if [ ${#skipped[@]} -gt 0 ]; then
  echo "Skipped:"
  for s in "${skipped[@]}"; do echo "  = ${s}"; done
fi
echo "Registry:"
echo "  ${registry_msg}"
if [ ${#errors[@]} -gt 0 ]; then
  echo
  echo "Errors:" >&2
  for e in "${errors[@]}"; do echo "  ! ${e}" >&2; done
  exit 1
fi

echo
echo "Done."
