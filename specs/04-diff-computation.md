> Extracted from SPEC.md — see [index](../SPEC.md)

# Diff Computation

Line-level unified diff via `similar` crate. Returns `+`/`-`/` ` prefixed lines, or None if unchanged.

> **Skill-level behavior:** The `/agent-doc` Claude Code skill strips HTML comments (`<!-- ... -->`) and link reference comments (`[//]: # (...)`) from both the snapshot and current content before diff comparison. This ensures that comments serve as a user scratchpad without triggering agent responses. This stripping is performed by the skill workflow (SKILL.md §2), not by the CLI itself.
