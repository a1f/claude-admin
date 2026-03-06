#!/usr/bin/env bash
# Forwards Claude Code hook events to the claude-admin daemon.
# Always exits 0 so Claude Code is never blocked by a missing daemon.

SOCKET="${HOME}/.claude-admin/daemon.sock"

# Read stdin in full and strip newlines to prevent IPC injection.
HOOK_JSON=$(cat | tr -d '\n\r')

# Wrap the raw hook payload in the IPC request envelope.
REQUEST='{"type":"hook_event","event":'"${HOOK_JSON}"'}'

# Attempt delivery. Discard all output; -w 1 is portable to both GNU and macOS nc.
printf '%s\n' "${REQUEST}" | nc -U -w 1 "${SOCKET}" >/dev/null 2>&1 || true

exit 0
