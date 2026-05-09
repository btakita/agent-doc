# Plan: content_ours prompt prefix leak + per-turn commit churn

## Problem

Two related issues observed in buildparty-investor-demo/dev.md turn at 12:42 on 2026-05-07:

### 1. `❯ ` prefix applied to agent content during content_ours fallback

When `finalize --stream` writes the response via IPC and the sidecar normalization diverges, the write path falls back to `content_ours`. The `content_ours` reconstruction incorrectly applies `❯ ` prompt prefixes to previously committed agent-authored lines. This creates a committed state with `❯ ` on agent content, immediately followed by a "normalize" commit that strips them.

**Evidence:**
- Commit `c5274fa4`: "Done:" section committed cleanly (no prefix)
- Commit `c92b067a`: same "Done:" section now has `❯ ` on every line
- Commit `e5110748`: normalize strips the `❯ ` prefixes back out

The sidecar normalization divergence message from the first finalize:
```
[write] sidecar normalization diverged — falling back to content_ours (7533 bytes)
```

### 2. Excessive commits per turn

Single turn produced 5 session-document commits:
1. `dfe344e0` — preflight recovery (expected)
2. `c5274fa4` — first finalize (expected — main response)
3. `c92b067a` — second finalize (forced by session-check for minor content edit)
4. `e5110748` — normalize (fix `❯ ` prefix from bug 1)
5. `7b2a0e01` — boundary repair (fix residual snapshot drift)

Expected: 1-2 commits (preflight recovery + finalize).

## Root Causes

### content_ours prefix leak

The `content_ours` fallback reconstructs the document from the working tree when sidecar normalization diverges. During reconstruction, it applies `❯ ` prefixes based on line-level heuristics (e.g., lines not in the agent's patch blocks). After a response is committed, previously agent-authored lines in the exchange become part of the document's "existing content." When a subsequent finalize triggers `content_ours` on the same turn, those committed agent lines are treated as user content and prefixed.

**Fix direction:** `content_ours` must preserve the prefix state from the committed HEAD, not re-derive it. Lines that were committed without `❯ ` in HEAD should never gain `❯ ` in the `content_ours` reconstruction.

### Commit churn from content-edit re-resolution

Session-check's `prompt_bearing_changes` treats any `content_edit` (even a single-word change like `+ gh` on an already-answered line) as an unresolved prompt requiring a new cycle. This forces the agent to run a second preflight + finalize, which doubles the commit count and introduces the prefix leak opportunity.

**Fix direction:** If the content edit is on a line that already has a committed agent response (i.e., the edit is within the span of text that a `### Re:` section addresses), session-check should not flag it as unresolved. The agent's response already covers the intent — the edit is a clarification, not a new prompt.

Alternatively, the second finalize could be a lightweight "ack + commit" path that appends to the existing committed response without a full sidecar negotiation.

## Proposed Changes

### Phase 1: Fix content_ours prefix preservation

**File:** `src/agent-doc/src/write.rs` (or wherever `content_ours` fallback is implemented)

1. When building `content_ours`, read the committed HEAD version of the exchange
2. For each line in the exchange that exists in HEAD without `❯ `, do not add `❯ ` in the reconstruction
3. Only apply `❯ ` to lines that are newly added (not present in HEAD)
4. Test: finalize with sidecar divergence on a document that has committed agent content — verify no `❯ ` on agent lines

### Phase 2: Reduce session-check sensitivity to minor content edits

**File:** `src/agent-doc/src/session_check.rs` (or equivalent)

1. When evaluating `content_edit` prompt-bearing changes, check whether the edited line falls within the span of an existing `### Re:` response section
2. If the edit is within a responded section and the semantic delta is small (single word change, whitespace, punctuation), classify it as `resolved_edit` rather than `unresolved_prompt`
3. Test: user edits a word in a prompt that already has a `### Re:` response — session-check should not interrupt

### Phase 3: Consolidate normalize into the commit path

**File:** `src/agent-doc/src/commit.rs`

1. If a commit would be immediately followed by a normalize (prefix strip), combine them into a single commit
2. Post-commit normalization should detect prefix inconsistencies before committing and fix them in the same staging step
3. Test: finalize that triggers prefix leak — verify only one commit, not two

## Test Cases

1. Finalize with `content_ours` fallback on a document with previously committed agent content → no `❯ ` on agent lines, single commit
2. User edits a word in a prompt that already has a `### Re:` response → session-check passes without requiring a new cycle
3. Multiple successive normalizations (prefix strip + boundary reposition) → consolidated into a single commit
