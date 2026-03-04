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
| v0.9.0 | **Components**, `patch` (modes + hooks), `watch` daemon, comment stripping. 150 tests. |

### Documentation

- `docs/guide/components.md` — Component reference (syntax, config, hooks, nesting)
- `docs/guide/dashboard.md` — Dashboard-as-document tutorial (end-to-end flow, multi-component, loop prevention)
- `docs/guide/commands.md` — All CLI commands including `patch` and `watch`

### Pending / Not Built

- `TmuxBatch` — command batching to reduce flicker
- `agent-doc deep` — fan-out parallel subagents
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend (replace junie bridge)
- tmux-router CLI binary — `tmux-router sync`, `tmux-router register`, etc.
- `agent-doc init --dashboard` — scaffold standard template with log + status components
- Log component prompt extraction — skill-level intelligence to move user prompts from components to log before patching

### Key Lessons

1. **Always paired markers** — single markers unworkable for re-rendering (no boundary after first render)
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD. Thread-safe for parallel tests.
3. **Component terminology** — "component" (not "range template") aligns with web components/React
4. **CLI is dumb, skill is smart** — `patch` replaces content; prompt extraction lives in skill workflow
5. **ATTACH before DETACH** — join new panes first, then stash unwanted
6. **Log every tmux command** — essential for debugging multi-step operations
7. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc

## User

Let's add some docs for components and dashboard on the README.

Also, let's make the log component have an optional name or field that allows for multiple log components.

```
<!-- agent:capture:my-section -->
The Answer:
<!-- agent:status -->
42
I can write an inline prompt here.
<!-- /agent:status -->
<!-- agent:log:my-section -->
The log of inline prompt + answer goes here.
<!-- /agent:log:my-section -->
<!-- /agent:capture:my-section -->
```

If we do have a capture, we wouldn't need to explicitly define a name...but can use scoping rules.

```
<!-- agent:capture -->
The Answer:
<!-- agent:status -->
42
I can write an inline prompt here.
<!-- /agent:status -->
<!-- agent:log -->
The log of inline prompt + answer goes here.
<!-- /agent:log -->
<!-- /agent:capture -->
```

However we could have sibling captures or "forms" using the name:

```
<!-- agent:capture:my-section -->
The Answer:
<!-- agent:status -->
42
I can write an inline prompt here.
<!-- /agent:status -->
<!-- /agent:capture:my-section -->

<!-- agent:log:my-section -->
The log of inline prompt + answer goes here.
<!-- /agent:log:my-section -->
```

## Assistant

**README updated** with Components and Dashboard-as-Document sections. Added `patch` and `watch` to Quick Start and CLI Reference.

**Re: namespaced components and capture**

The colon-separated naming (`agent:type:scope`) is a clean extension. Three layers emerge:

1. **Simple names** (current): `agent:status` — a single component
2. **Namespaced names**: `agent:log:my-section` — the full string `log:my-section` is the component name. This already works conceptually — we just need to allow colons in the name validator.
3. **Capture semantics** — skill-level intelligence that understands the relationship between `capture:X`, inner components, and `log:X`

**Parser change needed:** The current `is_valid_name()` only allows `[a-zA-Z0-9][a-zA-Z0-9-]*`. To support colons: `[a-zA-Z0-9][a-zA-Z0-9:-]*` — allow colons as namespace separators. The CLI treats the full namespaced string as the component name (`agent-doc patch file.md "log:my-section" "entry"`).

**Capture design — three patterns:**

**Pattern 1: Capture wraps log (your first example)**
```markdown
<!-- agent:capture:my-section -->
<!-- agent:status -->42<!-- /agent:status -->
<!-- agent:log:my-section -->...<!-- /agent:log:my-section -->
<!-- /agent:capture:my-section -->
```
Log is inside capture. Patching `status` triggers prompt extraction to sibling `log:my-section`.

**Pattern 2: Anonymous capture (your second example)**
```markdown
<!-- agent:capture -->
<!-- agent:status -->42<!-- /agent:status -->
<!-- agent:log -->...<!-- /agent:log -->
<!-- /agent:capture -->
```
Scoping rule: unnamed capture uses unnamed log. Simple, works for single-capture documents.

**Pattern 3: Sibling capture + log (your third example)**
```markdown
<!-- agent:capture:my-section -->
<!-- agent:status -->42<!-- /agent:status -->
<!-- /agent:capture:my-section -->

<!-- agent:log:my-section -->...<!-- /agent:log:my-section -->
```
Capture and log are siblings, linked by shared scope name. This is the most flexible — log can live anywhere in the document, not nested inside capture.

**Recommendation:** Pattern 3 (siblings) is cleanest:
- Capture defines a region where inline prompts are intercepted
- Log is a separate component that receives those prompts
- They're linked by naming convention (`capture:X` → `log:X`), not nesting
- Log can be at the bottom of the document, in a sidebar section, etc.
- Multiple captures with different names coexist naturally

**Implementation layers:**
1. **v0.9.1 (parser):** Allow colons in component names. Pure parser change, no semantic meaning at CLI level.
2. **Skill-level:** Capture/log relationship is defined in the skill workflow. Before patching a component inside a `capture:X` scope, the skill extracts inline prompts and appends them to `log:X`.

The CLI stays dumb — it doesn't know what "capture" means. The skill provides the intelligence. This follows the "CLI is dumb, skill is smart" principle from our lessons.

Shall I implement the parser change (allow colons in names) now?

## User

compact this doc

commit and push
