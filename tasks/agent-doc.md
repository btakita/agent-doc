---
session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
resume: null
agent: null
model: null
branch: null
---

## Status Summary (compacted 2026-03-03)

### Architecture

**Tmux routing**: Window-scoped routing resolved. `SessionEntry` stores window ID. Plugin passes `--window` on all sync/claim/layout ops. `route` auto-starts dead panes.

**2D sync** (`agent-doc sync --col`): Declarative columnar layout. Plugin detects editor splits via `LayoutDetector` (walks Swing Splitter tree). CLI arranges tmux panes to match: `-h` for columns, `-v` for stacking within columns. Auto-creates Claude sessions for unclaimed docs.

**Junie integration**: Fire-and-forget bridge (`junie-bridge.sh`). Plugin copies diff to clipboard + opens request.md. User pastes into Junie chat. Direct API backend (`agent/api.rs`) designed but not built.

### Release History

| Version | Key Changes |
|---------|-------------|
| v0.5.5 | Window-scoped routing, `--pane`/`--window` flags, layout safeguards |
| v0.5.6 | Commit-doc linking (`agent-doc(doc-name): timestamp`) |
| v0.6.0 | `agent-doc sync` — 2D columnar layout, `LayoutDetector`, `EditorTabSyncListener` |
| v0.6.1 | Junie agent backend, `RunWithJunieAction.kt`, `junie-bridge.sh` |
| v0.6.2 | Fire-and-forget junie bridge, plugin clipboard handoff |
| v0.6.3 | LayoutDetector fix — skip non-splitter Container children |
| v0.6.4 | Cross-window pane theft fix (Phase 1.5), layout equalization (Phase 8) |

### Pending / Not Built

- `TmuxBatch` — command batching to reduce flicker
- `agent-doc deep` — fan-out parallel subagents
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend (replace junie bridge)
- Plugin notification balloon showing detected layout

### Bugs Fixed This Session

1. **3-column layout bug**: `LayoutDetector.walkSplitterTree()` Container handler `flatMap`-ed ALL children when ANY contained a splitter. Fix: only delegate to splitter-containing children.
2. **Cross-window pane theft**: `sync` pulled panes from other tmux windows. Fix: Phase 1.5 window filtering in `sync.rs`.
3. **Unequal sizing after join**: tmux cascading 50/50 splits gave lopsided columns. Fix: Phase 8 equalization with `resize_pane`.

## User

Please also have the plugin display the layout it sent to agent-doc sync...In the prompt message.

## User

