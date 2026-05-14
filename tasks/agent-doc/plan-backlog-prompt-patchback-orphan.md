# Plan: Backlog-section prompts leave orphan text after patchback

## Bug

When a user writes prompts in the `<!-- agent:backlog -->` section (e.g., "commit + push uncommitted files"), `preflight` correctly detects them as `prompt_target` entries and the agent responds. `finalize` writes the response to the `<!-- agent:exchange -->` section via `<!-- patch:exchange -->`. However, the original prompt text in the backlog section is left as raw orphan text — it is neither cleaned up, promoted to the exchange with a `❯` marker, nor marked as resolved.

From the user's perspective, the backlog still shows the raw prompt text after the response is committed, making it look like the patchback was never written.

## Root cause

`finalize` only patches into `<!-- agent:exchange -->`. It has no mechanism to clean up or rewrite prompt text that originated in other document sections (backlog, queue, icebox, etc.). The `preflight` diff correctly identifies backlog-area edits as `prompt_target`, but there is no post-response cleanup step that removes or annotates the resolved prompt text from the source section.

## Proposed fix

### Option A: Post-finalize backlog cleanup (recommended)

After `finalize` writes the response to exchange, add a cleanup pass that:
1. Identifies resolved `prompt_target` lines that originated in `<!-- agent:backlog -->`
2. Removes those lines from the backlog section (they are now answered in exchange)
3. Commits the cleanup as part of the same finalize cycle

### Option B: Promote prompts to exchange before response

During `preflight` or `plan`, if a `prompt_target` originates from a non-exchange section, insert a `❯` marker into the exchange section for that prompt so the response appears adjacent to the promoted prompt. Clean up the original text from the source section.

### Option C: Annotate in-place

Leave the prompt text in the backlog but annotate it with a `[answered]` marker or strikethrough after the response is written.

## Files likely affected

- `src/agent-doc/src/template/finalize.rs` — post-write cleanup pass
- `src/agent-doc/src/preflight/diff.rs` — track source section of prompt_targets
- `src/agent-doc/src/template/write.rs` — section-aware cleanup
