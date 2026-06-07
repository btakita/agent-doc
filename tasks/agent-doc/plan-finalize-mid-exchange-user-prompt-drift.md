# Plan: finalize closeout leaves mid-exchange user prompts as uncommitted drift

`#agent-doc-bug` — filed 2026-06-04

## Symptom

When a user adds inline prompts between existing response blocks in the exchange (mid-exchange position), `agent-doc finalize` commits the response snapshot but does not absorb the user edits that triggered the cycle. The user prompts remain as uncommitted working-tree drift. `session-check` then correctly flags them as "unresolved prompt-bearing user changes."

## Root Cause

1. The patch point is always at end-of-exchange (appended after the boundary marker)
2. User prompts that triggered the cycle are mid-exchange (between previous `### Re:` block and the new boundary)
3. `git::commit()` commits the snapshot, which contains the new response appended at the end
4. The user edits above the new boundary are not absorbed into the committed blob
5. The snapshot matches HEAD (response only), user edits are excluded

## Flow

```
Exchange before cycle:
  [previous response]
  <!-- agent:boundary:old -->

User adds prompts mid-exchange:
  [previous response]
  USER PROMPT TEXT        <-- mid-exchange position
  <!-- agent:boundary:old -->

finalize patches response at end:
  [previous response]
  USER PROMPT TEXT        <-- NOT in committed snapshot
  [new response]          <-- committed
  <!-- agent:boundary:new -->

Working tree drift:
  USER PROMPT TEXT + (HEAD) markers + boundary change
```

## Fix Options

### Option A: Absorb user edits above the boundary into the committed blob

In `git::commit()`, after computing the snapshot (response text), also absorb any working-tree user edits that appear between the previous boundary and the new boundary into the committed blob. The snapshot should match the full working tree state (response + user prompts + boundary), not just the response text.

**Pros:** Correct by construction — the committed blob matches the full document state.
**Cons:** Requires `git::commit()` to understand the full exchange structure, not just the patch.

### Option B: Reorder — patch response immediately after the user prompts

Instead of always appending at end-of-exchange, insert the response immediately after the last user prompt that triggered the cycle. This way the response follows the prompts in document order.

**Pros:** Natural document flow — prompts immediately followed by response.
**Cons:** Changes the patch insertion logic; needs to identify the correct insertion point.

### Option C: Commit the full working-tree state, not just the snapshot

After patching, instead of committing the snapshot (which is response-only), commit the actual file content (which includes user edits). The snapshot would match the file after patching, not the idealized response-only state.

**Pros:** Simplest change — one line in the commit path.
**Cons:** May include unintended working-tree changes beyond the user prompts.

## Recommendation

Option A is the cleanest fix. The `git::commit()` function already has access to the file content; it should commit the post-patch file state (which includes user edits) rather than the pre-computed snapshot. The snapshot should be updated to match the committed blob.

## Affected Files

- `src/agent-doc/src/git.rs` — `commit()` function
- `src/agent-doc/src/template.rs` — boundary insertion logic
- `src/agent-doc/src/write.rs` — write path that calls commit
- `src/agent-doc/src/snapshot.rs` — snapshot save logic

## Verification

1. Add user prompts mid-exchange in a test document
2. Run `agent-doc finalize` with a response
3. Verify `git diff` shows no drift for the session document
4. Verify `agent-doc session-check` passes
5. Verify committed blob contains both user prompts and response
