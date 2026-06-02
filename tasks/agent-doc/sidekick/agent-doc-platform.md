---
agent_doc_session: daemon-extensions-plan
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_model: opus
---

## Status

<!-- agent:status patch=replace -->
Planning: agent-doc daemon extensions — virtual documents, event channels, snapshot-only mode, and plugin API.
<!-- /agent:status -->

## Context

Agent-doc's supervisor and CRDT infrastructure can be extended to support real-time event-driven workflows beyond the current turn-based cycle. These extensions are vendor-independent capabilities that any integration (monitoring tools, analytics platforms, IDE extensions) can build on.

## Requirements

### R1: Snapshot-only mode (no git commit pollution)
Consumers of virtual documents should not generate git commits. State tracking via `.agent-doc/` snapshots only.
- `commit: false` frontmatter option or `--no-commit` CLI flag
- `finalize` writes document + updates snapshot, skips git commit
- Session-check verifies snapshot consistency instead of HEAD match
- Virtual document directory: `.agent-doc/virtual/` — gitignored by default

### R2: Real-time event ingestion
Supervisor receives external events via IPC without requiring a preflight/respond/finalize cycle.
- New IPC message type: `Event { kind, payload, timestamp }`
- Hook adapter: harness hooks pipe events to supervisor socket
- State accumulator: running counters configurable per-document
- Rule engine: configurable thresholds that trigger component updates

### R3: Virtual documents
Documents managed primarily by daemons/plugins, while remaining locally inspectable and intentionally editable when needed.
- Created and updated programmatically via supervisor IPC or CLI
- Stored as ordinary local markdown files so practitioners can open them in their editor, review them, and make direct corrections when useful
- Structured components serve as typed data surfaces for both human review and machine updates
- CRDT merge + snapshots preserve traceability across daemon writes and manual edits
- Consumers (extensions, CLI tools) read via IPC or file watch
- Default flow is non-interactive (no preflight prompt loop), but optional direct edits are a feature rather than a protocol violation

### R4: Plugin API
A stable interface for external tools to:
- Register event listeners on the supervisor
- Read/write virtual document components
- Subscribe to component change notifications
- Query document state without file I/O (via IPC)

### R5: Multi-tool export interface
Compile virtual document state into format-specific output files:
- AGENTS.md (Claude Code)
- .github/copilot-instructions.md (GitHub Copilot)
- .cursor/rules (Cursor)
- Custom formats via export plugins

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                 agent-doc Supervisor                      │
│  (extended with event channel + plugin registry)         │
├──────────────┬──────────────┬───────────────────────────┤
│ Event Ingest │ State Engine │ Component Materializer     │
│ (hooks→IPC)  │ (rules eval) │ (CRDT → virtual doc)      │
├──────────────┴──────────────┴───────────────────────────┤
│           Persistence Layer (snapshot-only)               │
│  .agent-doc/virtual/{doc-id}.md                          │
├─────────────────────────────────────────────────────────┤
│              Plugin Interface                             │
│  ┌────────────────────────────────────────────────────┐ │
│  │ register_listener() | read_component()             │ │
│  │ write_component()   | subscribe_changes()          │ │
│  │ query_state()       | export_format()              │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

## Why the document surface matters

The virtual document is not just an internal storage detail. Making it a local markdown artifact creates practical product advantages:

- **Recoverability and trust:** users can inspect the live state without proprietary tooling, correct it locally, and see exactly what the daemon believes
- **Traceability:** snapshots provide a machine-owned audit trail, and teams that want human review can opt into git-backed history for selected documents or exports
- **Interoperability:** other tools can consume the same surface through file watch, IPC, or export without learning a private database schema
- **Marketing clarity:** "the system writes a real document you can open" is easier to explain and demo than "state lives inside a daemon"
- **Productivity:** the same artifact works as an operational surface for plugins, a debugging surface for developers, and a review surface for managers

## Virtual document vs daemon

These are complementary parts of the same system, not competing concepts.

- **The daemon/supervisor is the runtime:** it stays alive, receives events, runs rules, coordinates plugins, manages lifecycle, and decides when state should change
- **The virtual document is the materialized state surface:** it is the durable markdown artifact the daemon updates so humans and tools can inspect, diff, export, and sometimes edit the resulting state
- **The daemon is active; the document is inspectable:** the daemon performs work, while the document exposes the result of that work in a portable form
- **The daemon is infrastructure logic; the document is infrastructure interface:** one is the engine, the other is the legible surface the rest of the ecosystem can consume
- **Without the daemon, the document goes stale; without the document, the daemon becomes opaque:** the value comes from pairing a live coordinator with a visible, local artifact

## Implementation Phases

### Phase 0: Snapshot-only mode
- [ ] Add `commit: false` frontmatter option (or `--no-commit` CLI flag)
- [ ] When enabled, `finalize` writes document + updates snapshot but skips git commit
- [ ] Preflight diffing works against snapshots (already does this)
- [ ] Session-check guard adapted: in no-commit mode, verify snapshot consistency instead of HEAD match
- [ ] Virtual document directory: `.agent-doc/virtual/` — outside git tracking (added to .gitignore)
- [ ] CLI: `agent-doc virtual create <name>` — scaffold a virtual document

### Phase 1: Event channel on supervisor
- [ ] New IPC message type: `Event { kind, payload, timestamp }`
- [ ] Hook adapter: harness hooks pipe events to supervisor socket
- [ ] State accumulator: running counters (configurable per document)
- [ ] Rule engine: configurable thresholds trigger component updates
- [ ] Component push: when rule fires, update virtual document component via CRDT

### Phase 2: Plugin API
- [ ] Define stable IPC protocol for plugin registration
- [ ] `register_listener(event_kind, callback)` — subscribe to events
- [ ] `read_component(doc_id, component_name)` — query component state
- [ ] `write_component(doc_id, component_name, content)` — update component
- [ ] `subscribe_changes(doc_id, component_name)` — get notified on mutations
- [ ] CLI: `agent-doc plugin list`, `agent-doc plugin register <path>`

### Phase 3: Multi-tool export
- [ ] Export interface: plugin provides `format_id` + `compile(components) → output`
- [ ] Built-in exporters: AGENTS.md, copilot-instructions.md, .cursor/rules
- [ ] Trigger: on component change, on demand, or on schedule
- [ ] CLI: `agent-doc export <format> [--doc <id>]`

### Phase 4: tsift integration (knowledge plugin)
- [ ] Index session transcripts alongside code (new tsift content type)
- [ ] Session search API: query past sessions semantically
- [ ] Community overlap: code communities correlated with access patterns
- [ ] Feed graph data into virtual document knowledge components

## Open Questions

1. Should virtual documents live in `.agent-doc/virtual/` (per-project) or in `~/.agent-doc/` (global)?
2. Event throughput: debounce at 1s intervals or process every event?
3. Plugin protocol: JSON-RPC over Unix socket, or simpler line-delimited messages?
4. Should the export interface support incremental updates or always full recompile?

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
