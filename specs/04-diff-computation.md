> Extracted from SPEC.md — see [index](../SPEC.md)

# Diff Computation

Line-level unified diff via `similar` crate. Returns `+`/`-`/` ` prefixed lines, or None if unchanged.

Prompt-bearing diff triage is part of the diff contract, not just a prompt-builder convenience. The diff layer must classify ordered user-authored changes oldest-first as:

- `prompt_target` — prompts that require a response
- `content_edit` — corrections that replace prior agent text as the new source of truth
- `recovery_artifact` — likely delayed/missed response material that should route through repair/session-check logic
- `boundary_artifact` — transient boundary / `(HEAD)` churn that should be normalized rather than answered

Mixed changes must preserve encounter order across those kinds. The classifier must not bubble later `prompt_target` items ahead of earlier `content_edit` or artifact lines from the same changed tail.

`boundary_artifact` is intentionally narrow: it applies only to actual response-heading `(HEAD)` reposition churn and `agent:boundary` marker churn. User prose that merely mentions `(HEAD)` remains ordinary prompt-bearing content and must not be collapsed to `no_changes`.

> **Skill-level behavior:** The `/agent-doc` Claude Code skill strips HTML comments (`<!-- ... -->`) and link reference comments (`[//]: # (...)`) from both the snapshot and current content before diff comparison. Escaped-tail and prompt-drift scanners also ignore ordinary comment bodies, including transiently unterminated comment tails while the user is typing. This ensures that comments serve as a user scratchpad without triggering agent responses or repair moves. This stripping is performed by the skill workflow (SKILL.md §2), not by the CLI itself.
