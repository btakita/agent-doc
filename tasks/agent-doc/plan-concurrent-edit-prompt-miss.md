# Plan — concurrent user edits during response cycle bypass prompt detection

**Pending:** `#concprompt` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

When a user adds a new prompt to the document while the agent is composing its response (between preflight and write-back), the new prompt is preserved in the committed document but is never flagged for response. The `session-check` guard also passes without detecting the unresolved prompt.

Reproduction from May 7, 2026 on `tasks/velvet-thunder/dev.md`:

1. Preflight detects one prompt target: `Please rename the buildparty-investor-demo directory to velvet-thunder`
2. While agent processes the rename, user adds: `❯ On platform.openai.md, if an api key is out of credits, who and how will anyone in the organization be notified?`
3. Agent composes response addressing only the rename
4. `agent-doc write --commit` preserves both user lines in the document
5. Response only addresses the rename; the OpenAI question has no `### Re:` response
6. `session-check` reports `ok` — does not detect the unresolved prompt

The prompt was not in the preflight diff because it was added after diff stability was confirmed (the truncation recheck shows only the rename line was present). The `content_ours` write preserved it verbatim, but no mechanism flagged it for the current or next cycle.

## Root Cause

1. Preflight's diff is a point-in-time snapshot — changes made after stability confirmation are invisible to the current cycle
2. The `write --commit` path (especially `content_ours`) merges concurrent user edits into the committed document without flagging new prompt-bearing content
3. `session-check` validates the cycle state (committed, boundary present) but does not re-diff the committed document against the response to find unaddressed prompts

## Fix

### Write-time detection
During `write --commit` or `finalize`, compare the current working-tree content against the preflight snapshot. If new `❯ `-prefixed lines appear that were not in the preflight diff's `prompt_bearing_changes`, flag them as `unresolved_concurrent_prompts` in the cycle state.

### Session-check enforcement
`session-check` should scan the committed document for `❯ ` prefixed lines after the last `### Re:` response heading (or after the boundary marker) that have no corresponding response. If found, report `INTERRUPTED` with a message listing the unresolved prompts.

### Skill instruction update
The agent-doc skill instructions should note that when `session-check` detects concurrent prompts, the agent should either:
- Compose and finalize an additional response in the same cycle
- Or acknowledge the prompts in console and note they'll be addressed next cycle

## Tests

- Simulate concurrent edit (add `❯ ` line to document after preflight) and verify `session-check` detects it
- Verify `write --commit` flags concurrent prompt additions
- Verify the cycle doesn't report success when unaddressed prompts exist
