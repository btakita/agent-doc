# Plan: Repair CRDT Live Prompt Prefix Duplicates

**Trigger:** `#agent-doc-bug` report from chat
**Status:** Implemented

## Problem

While the user was typing in `agent:exchange`, IPC/CRDT writeback left two adjacent prompt lines after the latest exchange boundary:

- an earlier partial prompt
- the later completed prompt, with the partial prompt as its prefix

The existing prompt duplicate repair only caught exact duplicate prompt text, so this prefix-variant form survived closeout.

## Desired Behavior

- Repair only the live prompt tail after the last `agent:boundary` marker.
- Prefer the longer completed prompt line over the shorter prefix line.
- Leave assistant response prose before the boundary untouched.
- Use the same repair for IPC snapshot sidecars before saving snapshots or redelivering editor repairs.

## Implementation

- Add a narrow live-tail prompt prefix-variant dedupe helper in `src/write.rs`.
- Run it from `normalize_template_structure_or_fail`.
- Run it from `dedupe_ipc_snapshot_content`.
- Add regressions for the observed OpenCode arrow-key prompt duplication shape.

## Verification

- [x] `cargo test live_prompt_prefix_variant`
- [x] `cargo test normalize_template_structure_`
- [x] `make check`
- [x] `make tmux-ci`

## Files Changed

- `src/agent-doc/src/write.rs`
- `src/agent-doc/VERSIONS.md`
