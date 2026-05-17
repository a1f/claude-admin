#!/usr/bin/env python3
"""Fake backend used by V3 hook-contract tests.

Emits the 4 mid-run markers (QUESTION/PAUSE/STUCK/PERMISSION_BLOCKED) on stdout
with small sleeps between them. START + EXIT are emitted by the runtime itself
(spawn + process reap), so this fixture deliberately does NOT print them.

Usage:
    python3 fake_llm.py [--sleep SECONDS] [--permission-line TEXT]
"""

from __future__ import annotations

import argparse
import sys
import time


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--sleep", type=float, default=0.05)
    p.add_argument("--permission-line", default="HOOK_PERMISSION_BLOCKED: Bash:rm -rf /")
    args = p.parse_args()

    for line in (
        "HOOK_QUESTION: which API key should I use?",
        "HOOK_PAUSE: awaiting user input",
        "HOOK_STUCK: marker so the test path is exercised",
        args.permission_line,
    ):
        sys.stdout.write(line + "\n")
        sys.stdout.flush()
        time.sleep(args.sleep)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
