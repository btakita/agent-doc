---
agent_doc_session: b7c9bb93-c9ea-446c-a421-4c1bf93c5ac4
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---
<!-- agent:status mode=replace -->
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
# Agent-Doc Plugin Architecture

Transferred from tasks/agent-doc-bugs.md — CRDT-via-FFI roadmap and plugin improvements.

## Phase 1: Fix IPC (Current)
- JB plugin 0.2.1: applyPatch returns Boolean, ACK only on success
- VSCode extension: IPC patch watcher added
- Root cause of false ACK: competing plugin installations (0.2.0 vs 0.2.1)
- Plugin SPEC: editors/PLUGIN-SPEC.md

## Phase 2: Shared CRDT Library (Rust FFI)
- Extract Yrs CRDT into `libagent_doc_crdt` crate (cdylib)
- C API: crdt_doc_new, crdt_doc_apply_ops, crdt_doc_merge, crdt_doc_encode, crdt_doc_free
- JNI wrapper for JetBrains, napi for VSCode

## Phase 3: Plugin CRDT Integration
- DocumentListener captures user keystrokes → CRDT ops
- IPC watcher receives agent ops → CRDT merge → editor buffer sync
- Three-state model: file (disk), editor (buffer), CRDT (authoritative)
- External edit = CRDT reset

## Phase 4: Binary Integration
- `agent-doc write --crdt` outputs CRDT state
- Plugin receives CRDT state, merges with local doc
- Wire-compatible Yrs versions

## Key Design Decisions
- Component-level patches (not byte ranges) for race condition safety
- Plugin finds markers at apply time, not at compute time
- Content hash verification for race detection
- Disk write fallback when no plugin available

## Open Questions
- Inline component attributes on tags vs frontmatter for config
- `agent_doc_format: inline` naming
- Squash command for exchange history
<!-- /agent:exchange -->
