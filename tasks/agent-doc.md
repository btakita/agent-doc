---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '1'
---


## Status Summary (compacted 2026-03-04)

### Architecture

**tmux-router crate**: Extracted standalone Rust library (`tmux-router` v0.1.1) from agent-doc. Contains `Tmux` struct (30+ tmux operations), `Registry` (persistent key-to-pane mappings), `reconcile()` (2D layout sync algorithm), `IsolatedTmux` (RAII test infra), stash window, `FileResolution` callback, `prune()` (dead pane removal + dedup). Published to crates.io. 46 tests (unit + integration + proptest). GitHub Pages docs at https://btakita.github.io/tmux-router/.

**agent-doc v0.9.0**: Interactive document sessions with AI agents. Depends on tmux-router. Published to crates.io, PyPI, GitHub Releases.

**Core features:**
- **Session workflow** (`agent-doc run`): diff → send to agent → append response → 3-way merge-safe write → snapshot → git commit
- **Components** (`<!-- agent:name -->...<!-- /agent:name -->`): Bounded, named, re-renderable document regions. Always paired markers (closing marker required for idempotent re-rendering). Nested supported (innermost-first). Parsed in `component.rs`.
- **`agent-doc patch`**: Replace/append/prepend component content. Config-driven modes via `.agent-doc/components.toml`. Shell hooks (`pre_patch` transforms via stdin/stdout, `post_patch` fire-and-forget). Thread-safe project-root resolution via `find_project_root()`.
- **`agent-doc watch`**: Daemon with `notify` crate, PID file, per-file debounce (500ms default), bounded cycle limiting (default 3), convergence detection via content hashing.
- **Comment stripping**: `strip_comments()` in diff.rs — strips `<!-- -->` and `[//]: # (...)` but preserves component markers. Comments are user scratchpad, never trigger responses.
- **Auto-resync**: `prune()` runs before route/sync/claim operations. Delegates to `tmux_router::prune()`.

**Tmux routing**: Window-scoped routing. Plugin passes `--window` on all sync/claim/layout ops. `route` auto-starts dead panes via `join-pane` into active window. 2D sync (`agent-doc sync --col`) with ATTACH-first reconciliation order.

**Plugin (JetBrains + VS Code)**: Lives in `agent-doc/editors/`. Plugin detects editor splits via `LayoutDetector`, sends `sync --col --focus <active-file>`. Inline hints at cursor for sync notifications.

### Component Config (`.agent-doc/components.toml`)

```toml
[log]
mode = "append"        # replace (default) | append | prepend
timestamp = true       # auto-prefix with ISO timestamp
max_entries = 100      # auto-trim in append/prepend (0 = unlimited)

[status]
mode = "replace"
pre_patch = "scripts/validate.sh"   # stdin: content, stdout: transformed
post_patch = "scripts/notify.sh"    # fire-and-forget, env: COMPONENT, FILE
```

### Namespaced Components & Capture (designed, not built)

Component names support colon-separated namespacing: `agent:type:scope` (e.g., `agent:log:my-section`). Parser change: `is_valid_name()` regex from `[a-zA-Z0-9][a-zA-Z0-9-]*` to `[a-zA-Z0-9][a-zA-Z0-9:-]*`.

**Three naming layers:**
1. **Simple**: `agent:status` — single component
2. **Namespaced**: `agent:log:my-section` — full string is the name, CLI treats it opaquely
3. **Capture semantics** — skill-level intelligence linking `capture:X` ↔ `log:X`

**Chosen pattern: Sibling capture + log** — capture and log are siblings linked by shared scope name (not nesting). Most flexible: log can live anywhere in the document.

```markdown
<!-- agent:capture:my-section -->
<!-- agent:status -->42<!-- /agent:status -->
<!-- /agent:capture:my-section -->

<!-- agent:log:my-section -->...<!-- /agent:log:my-section -->
```

**Implementation:** v0.9.1 parser allows colons in names (CLI stays dumb). Capture/log relationship lives in skill workflow only.

### Domain Ontology (overlay of base kernel)

| Term | Derives From | Description |
|------|-------------|-------------|
| **Session** | project + story | Bounded interaction with temporal arc; unit of agent-doc work |
| **Document** | entity + context | Markdown file holding conversational state |
| **Component** | scope + abstraction | Bounded, named, re-renderable region (`<!-- agent:name -->...<!-- /agent:name -->`). Configurable mode and shell hooks. |
| **Project** | system + scope | Bounded working context; identified by `.agent-doc/` at root. Contains documents, registry, snapshots, daemon. |
| **Pane** | focus + scope | Tmux viewport — finite attention applied to a single document |

### Release History

| Version | Key Changes |
|---------|-------------|
| v0.5.5–v0.5.6 | Window-scoped routing, commit-doc linking |
| v0.6.0–v0.6.6 | 2D sync, Junie backend, LayoutDetector, `--focus` |
| v0.7.0–v0.7.2 | Snapshot-diff-apply sync, convergent reconciliation, ATTACH-first order |
| v0.8.0 | tmux-router extraction (-3133 lines), frontmatter rename to `agent_doc_session` |
| v0.8.1 | Auto-resync via `tmux_router::prune()` |
| v0.9.0 | **Components**, `patch` (modes + hooks), `watch` daemon, comment stripping. README updated with component/dashboard docs. 150 tests. |

### Documentation

- `docs/guide/components.md` — Component reference (syntax, config, hooks, nesting)
- `docs/guide/dashboard.md` — Dashboard-as-document tutorial (end-to-end flow, multi-component, loop prevention)
- `docs/guide/commands.md` — All CLI commands including `patch` and `watch`
- `README.md` — Components and Dashboard-as-Document sections added

### Pending / Not Built

- **v0.9.1**: Allow colons in component names (parser change)
- `TmuxBatch` — command batching to reduce flicker
- `agent-doc deep` — fan-out parallel subagents
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend (replace junie bridge)
- tmux-router CLI binary — `tmux-router sync`, `tmux-router register`, etc.
- `agent-doc init --dashboard` — scaffold standard template with log + status components
- Capture/log skill workflow — prompt extraction from `capture:X` to `log:X`

### Key Lessons

1. **Always paired markers** — single markers unworkable for re-rendering (no boundary after first render)
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD. Thread-safe for parallel tests.
3. **Component terminology** — "component" (not "range template") aligns with web components/React
4. **CLI is dumb, skill is smart** — `patch` replaces content; prompt extraction lives in skill workflow
5. **ATTACH before DETACH** — join new panes first, then stash unwanted
6. **Log every tmux command** — essential for debugging multi-step operations
7. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc

## User

Commit the agent-doc README

## Assistant

Done. Committed and pushed the agent-doc README (`e084ff3`) with components, dashboard, and patch/watch CLI docs. Updated the parent submodule pointer (`6c8b825`) and pushed both.

## User


