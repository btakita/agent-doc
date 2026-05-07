# Plan: Relocate Out-of-Exchange Prompts During Preflight

## Problem

When a user types a prompt **after** `<!-- /agent:exchange -->` (outside the exchange component), the preflight diff correctly detects the edit and the agent processes it — but the original prompt text is left stranded outside the exchange. The response gets inserted inside the exchange correctly, but the orphaned prompt remains in a no-man's-land between `<!-- /agent:exchange -->` and the next component (e.g., `###`, `## Queue`).

**Observed behavior:**
```
Re-ran full pipeline...
<!-- /agent:exchange -->       ← exchange closes here
do [#bpsqlschema]. spec-test  ← user prompt stranded outside
###
```

**Expected behavior:** Preflight should detect the out-of-band prompt, relocate it inside the exchange (before the closing tag), and proceed normally — OR flag it as a `layout_issue` so the agent knows to handle it.

## Root Cause

1. `diff.rs` detects the content change but doesn't classify *where* relative to component boundaries the edit landed.
2. `preflight.rs` approves the diff without checking if user edits are inside recognized components.
3. `template.rs` has `escaped_conversation_range_outside_exchange()` and `repair_conversation_tail_outside_exchange()` but these only run at **write time** (after response generation), not at preflight time.
4. No pre-response relocation logic exists in the pipeline.

## Proposed Fix

### Option A: Preflight-time relocation (preferred)

In `preflight.rs`, after computing the diff but before returning the JSON contract:

1. Parse the document's template structure to find the exchange component boundaries (`<!-- agent:exchange -->` ... `<!-- /agent:exchange -->`).
2. Scan the diff's added lines for any user content that falls **outside** the exchange close tag but **before** the next component heading.
3. If found, relocate: move the content to just before `<!-- /agent:exchange -->`, inserting a `❯` prompt marker if not already present.
4. Re-save the document and re-compute the diff against the updated content.

### Option B: Diff-time classification (simpler)

In `diff.rs`, when classifying the diff:

1. After computing the unified diff, check if any `[user+]` lines in the annotated diff fall outside the exchange component range.
2. If so, set a new field `out_of_band_prompt: true` in the diff result.
3. In `preflight.rs`, when `out_of_band_prompt` is true, emit a `layout_issue` warning and/or auto-relocate before returning.

### Files to modify

- `src/agent-doc/src/preflight.rs` — add post-diff boundary check + relocation
- `src/agent-doc/src/diff.rs` — optionally add out-of-band classification
- `src/agent-doc/src/template.rs` — reuse `escaped_conversation_range_outside_exchange()` logic at preflight time, not just write time
- `src/agent-doc/src/component.rs` — may need to expose exchange boundary line ranges for the check

### Test cases

1. User prompt placed after `<!-- /agent:exchange -->` before `###` → relocated inside exchange
2. User prompt placed after `<!-- /agent:exchange -->` before `## Queue` → relocated
3. User prompt correctly inside exchange → no change (regression test)
4. Multiple lines of user content outside exchange → all relocated together
5. Content outside exchange that is NOT a prompt (e.g., a comment block) → left alone
