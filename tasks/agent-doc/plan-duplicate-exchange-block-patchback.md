# Plan: Fix Duplicate Exchange Block Patchback (#dupeexchangeblock)

## Problem

OpenCode agent writes responses into a second `<!-- agent:exchange patch=append -->...<!-- /agent:exchange -->` block instead of appending to the existing exchange block. This produces an invalid document with two adjacent exchange sections.

**Reproduction:** monsterrodholders.md session a9421282 — the "Production deploy" response was written into a new exchange element (lines 126-147) instead of being appended to the existing exchange (lines 39-125).

## Root Cause

`sanitize_component_tags` only sanitizes patch block content (`<!-- patch:NAME -->...<!-- /patch:NAME -->`), not unmatched text. When an agent response contains `<!-- agent:exchange -->...<!-- /agent:exchange -->` markers outside of patch blocks, the unmatched text retains those raw markers. When appended to the existing exchange component, these markers create duplicate exchange blocks.

Additionally, `normalize_template_structure_or_fail` did not call `repair_duplicate_exchange_opener` as defense-in-depth, so any duplicate exchange blocks introduced by CRDT/git merge could survive into the final written document.

## Fix (Two Layers)

### Layer 1: Sanitize unmatched text

Add `sanitize_unmatched()` function that applies `sanitize_component_tags` to unmatched response text. Update all 5 call sites (`run_template`, `run_stream`, `canonicalize_response_for_capture`, `apply_stream_from_string`, `apply_template_from_string`) to sanitize both patches and unmatched text.

### Layer 2: Defense-in-depth repair

Add `repair_duplicate_exchange_opener` call to `normalize_template_structure_or_fail` so any duplicate exchange blocks that survive the patch/merge phase are merged before the write.

## Tests

- `sanitize_unmatched_escapes_exchange_markers_in_response`: verifies unmatched text with exchange markers gets escaped
- `apply_patches_sanitize_unmatched_prevents_duplicate_exchange_block`: end-to-end test with a real document showing that sanitized unmatched text cannot create a second exchange block

## Files Changed

- `src/write.rs`: `sanitize_unmatched()`, updated call sites, `normalize_template_structure_or_fail` defense-in-depth, regression tests
