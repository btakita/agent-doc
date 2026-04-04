---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.10 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-04 15:52) (HEAD)

**Releases: agent-doc v0.31.6→v0.31.10 + tmux-router v0.3.9**

**Code changes this session (shipped in v0.31.10):**
1. `ensure_initialized()` — shared auto-init for new documents (UUID + snapshot + git)
2. `ensure_initialized()` in sync's `resolve_file` — assigns UUID to template files, enables auto_start
3. Cross-process typing detection (FFI), diff stability fix, IPC error propagation
4. Granola: enrich-from-transcript runbook, import templates include session UUID

**Design work (not yet shipped):**
- **Binding invariant** formalized: document drives pane resolution (find existing OR provision new, NEVER commandeer another document's pane)
- **Domain ontology** added to README.md: Document Lifecycle (Session, Document, Snapshot, Component, Boundary, Exchange), Pane Lifecycle (Binding, Reconciliation, Provisioning, Initialization), Integration Layer (Route, Sync, Claim)
- **Module docs** updated with ontology: sync.rs (Reconciliation), claim.rs (Binding), snapshot.rs (Initialization), route.rs (Provisioning)
- **Naming audit**: claim binds existing panes, auto_start creates new ones, ensure_initialized does 3 things

**Decisions:**
- Spec-first workflow: write SPEC.md before module-harness (avoid codifying bugs as spec)
- Module-harness redesign: default=audit, seed=bootstrap from code, apply=spec-first
- Pane lifecycle spec goes in SPEC.md (user-facing) + sync.rs module doc (developer), not CLAUDE.md
- Refactoring (ensure_initialized split, auto_start rename) deferred to separate session
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] ensure_initialized: assign session UUID + sync integration
- [x] Binding invariant: memory + ontology captured
- [x] Domain ontology + module docs updated
- [ ] **SPEC.md: write pane lifecycle spec** (next)
- [ ] Release v0.31.11 with sync init + ontology
- [ ] Module-harness redesign: audit/seed/apply + runbooks (separate session)
- [ ] Refactor ensure_initialized into 3 focused functions (separate session)
- [ ] Rename auto_start_no_wait (separate session)
<!-- /agent:pending -->
