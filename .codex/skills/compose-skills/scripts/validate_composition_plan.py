#!/usr/bin/env python3
"""Validate a compose-skills architecture plan."""

from __future__ import annotations

import re
import sys
from pathlib import Path


REQUIRED_SECTIONS = [
    "Decision Boundary",
    "Proposed Skills",
    "Resource Inventory",
    "Invocation Policy",
    "Validation Plan",
    "Recommendation",
]

SKILL_NAME_RE = re.compile(r"\bname:\s*`?([a-z0-9]+(?:-[a-z0-9]+)+)`?")


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: validate_composition_plan.py <plan.md>", file=sys.stderr)
        return 2

    path = Path(sys.argv[1])
    content = path.read_text(encoding="utf-8")
    missing = [
        section
        for section in REQUIRED_SECTIONS
        if not re.search(rf"^##+\s+{re.escape(section)}\s*$", content, re.MULTILINE)
    ]
    names = SKILL_NAME_RE.findall(content)

    if missing:
        print("missing required section(s): " + ", ".join(missing), file=sys.stderr)
    if not names:
        print("no candidate skill entries found; include at least one `name: skill-name` entry", file=sys.stderr)

    if missing or not names:
        return 1

    print(f"ok: {path} defines {len(set(names))} candidate skill(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
