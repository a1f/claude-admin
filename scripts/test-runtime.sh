#!/usr/bin/env bash
# Run skills/_runtime/ test suite via uv (no system pytest required).
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_DIR}"

exec uv run --python 3.12 --with pytest -- \
  python -m pytest skills/_runtime/tests/ "$@"
