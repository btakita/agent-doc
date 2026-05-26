> Extracted from SPEC.md — see [index](../SPEC.md)

# Diff Computation

Line-level unified diff via `similar` crate. Returns `+`/`-`/` ` prefixed lines, or None if unchanged.

Prompt-bearing diff triage is part of the diff contract, not just a prompt-builder convenience. The diff layer must classify ordered user-authored changes oldest-first as:

- `prompt_target` — prompts that require a response
- `content_edit` — corrections that replace prior agent text as the new source of truth
- `recovery_artifact` — likely delayed/missed response material that should route through repair/session-check logic
- `boundary_artifact` — transient boundary / `(HEAD)` churn that should be normalized rather than answered

Mixed changes must preserve encounter order across those kinds. The classifier must not bubble later `prompt_target` items ahead of earlier `content_edit` or artifact lines from the same changed tail.

`flow::session_cycle` consumes the ordered prompt-bearing changes and owns the prompt-target list used by both `preflight` and `plan`. Command modules may still compute the underlying diff, but they must not derive a separate prompt-target order or pending-mutation closeout contract.

`boundary_artifact` is intentionally narrow: it applies only to actual response-heading `(HEAD)` reposition churn and `agent:boundary` marker churn. User prose that merely mentions `(HEAD)` remains ordinary prompt-bearing content and must not be collapsed to `no_changes`.

> **Skill-level behavior:** The `/agent-doc` Claude Code skill strips HTML comments (`<!-- ... -->`) and link reference comments (`[//]: # (...)`) from both the snapshot and current content before diff comparison. Escaped-tail and prompt-drift scanners also ignore ordinary comment bodies, including transiently unterminated comment tails while the user is typing. This ensures that comments serve as a user scratchpad without triggering agent responses or repair moves. Duplicate-residue cleanup may scrub prompt-like post-exchange comment lines only when the line lacks every available ownership proof: it was not present in the pre-response baseline/snapshot and, for route, preflight, or final closeout, it was not already present in the visible current document used for that mutation. Assistant response text that quotes or mentions prompt-like scratch lines is not prompt ownership proof for comment cleanup. Cleanup must preserve the ordinary HTML comment container, including empty comment shells, and must not erase unrelated scratch lines mixed into the same multiline comment. Template normalization also removes a raw prompt tail after the latest `agent:boundary` when that tail exactly duplicates a prompt block already followed by an assistant response earlier in `agent:exchange`; this must run before preflight commit can reposition the boundary and make the stale tail look like new prompt-bearing diff. This stripping is performed by the skill workflow (SKILL.md §2), not by the CLI itself.

During response closeout, a same-turn edit inside an ordinary HTML comment below `agent:exchange` is visible local drift, not part of the assistant response. Even if that scratch comment repeats the live prompt or preset line, `finalize` / `write --commit` must preserve it in the working tree, keep it out of the response commit when it was not in the baseline, avoid duplicating the response body, and allow `session-check` to pass because the remaining drift is comment-only. This ownership rule also applies after exchange compaction: a compacted `### Session Summary` plus boundary/prompt tail inside `agent:exchange` does not authorize full-document replay or scratch-comment deletion below the closing marker.
