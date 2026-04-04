---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.12 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-04 16:29) (HEAD)

**Releases: agent-doc v0.31.6→v0.31.12 + tmux-router v0.3.9**

**Code shipped this session:**
- v0.31.10: ensure_initialized (UUID + snapshot + git), sync auto-init, cross-process typing (FFI), diff stability, IPC errors, granola runbook
- v0.31.11: sync resolve_file auto-init, SPEC.md 8.5 Binding invariant, domain ontology, module docs
- v0.31.12: refactored ensure_initialized into 3 functions (ensure_session_uuid, ensure_snapshot, ensure_git_tracked), renamed auto_start_no_wait → provision_pane, 8 new tests

**Design decisions:**
- Binding invariant: document drives pane resolution (find existing OR provision new, NEVER commandeer)
- Domain ontology: Binding, Reconciliation, Provisioning, Initialization (README.md + module docs)
- Spec-first workflow: SPEC.md before module-harness (avoid codifying bugs as spec)
- Module-harness redesign: default=audit, seed=bootstrap, apply=spec-first (SKILL.md updated)

**Test suite: 25 snapshot tests (17 existing + 8 new), 645+ total**
<!-- agent:boundary:e2f1c757 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] All items complete for this session
<!-- /agent:pending -->
