---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.25.6 | **Tests:** 521 (168 lib + 315 bin + 38 CLI) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✓
**Plugins:** JetBrains v0.2.17 (layout detection + EDT fix) + VS Code v0.2.3
**Library:** `libagent_doc.so` — 9 FFI exports | **SKILL.md:** preflight + IPC boundary reposition
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**tmux-router v0.2.4**: Standalone Rust crate (library + CLI). `Tmux` struct, `Registry`, `reconcile()` (2D sync), `IsolatedTmux` (test infra), `FileResolution` callback, `prune()`, `acquire_or_skip()`. 56 tests. Published to crates.io + GitHub Releases.

**Core:** diff → agent → merge-safe write → snapshot → git. Components (`&lt;!-- agent:name --&gt;`), template mode, stream mode (CRDT), `agent-doc patch` (hooks), `agent-doc watch` (debounced + reactive), `agent-doc write --stream` (CRDT merge), `agent-doc recover`, `agent-doc plugin install`. Comment stripping, auto-resync, stale snapshot recovery, diagnostic logging.

**Library target (`libagent_doc`):** `[lib]` with `crate-type = ["cdylib", "lib"]`. Exports `component`, `crdt`, `ffi`, `frontmatter`, `merge`, `template` modules. C ABI via `ffi.rs` (6 functions: parse_components, apply_patch, crdt_merge, merge_frontmatter, free_string, free_state). Binary modules use `pub(crate) use agent_doc::*` re-exports. `agent-doc lib-path` prints the shared library location.

**Concurrency:** `SnapshotLock` + `RegistryLock` (flock), atomic rename (tempfile + persist), doc advisory lock, `pub(crate)` load/save.

**Routing:** Window-scoped. Plugin passes `--window`. Auto-start dead panes via `join-pane`. 2D sync with ATTACH-first reconciliation.

**Plugins (JetBrains + VS Code):** 9/9 SPEC parity. `editors/` directory. `agent-doc commands` for autocomplete. JetBrains: JNA FFI bindings (`NativeLib.kt`) with Kotlin fallback — native-first patching for component patches and frontmatter merge.

**Config:** `~/.config/agent-doc/config.toml`. Multi-agent backend support. Execution mode per-skill via SKILL.md frontmatter (`hybrid | parallel | sequential`).

**Skill auto-upgrade:** SKILL.md bundled via `include_str!`. Pre-flight version check → `agent-doc skill install`.

### Race Conditions

| Race | Protection |
|------|-----------|
| Registry read-modify-write | flock (RegistryLock) |
| Snapshot read-modify-write | flock (SnapshotLock) |
| Nested lock deadlock | Thread-local flag + `acquire_or_skip()` |
| Watch daemon write window | Atomic rename + doc advisory lock |
| Stale snapshot (compaction) | `is_stale_snapshot()` auto-recovery |
| Compaction mid-write | Durable pending store + `recover` |

### Execution Model

| Mode | Behavior |
|------|----------|
| **`hybrid`** (default) | First doc direct, 2nd+ concurrent use subagent |
| **`parallel`** | Every `/agent-doc` spawns subagent |
| **`sequential`** | Fully sequential, cheapest |

### Domain Ontology

| Term | Derives From |
|------|-------------|
| **Session** | project + story |
| **Document** | entity + context |
| **Component** | scope + abstraction |
| **Project** | system + scope |
| **Pane** | focus + scope |
<!-- /agent:architecture -->

## Release History

<!-- agent:releases -->
| Version | Key Changes |
|---------|-------------|
| v0.5.5-v0.9.6 | Window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename. 176 tests. |
| v0.9.7-v0.9.10 | `agent-doc write`, template mode, compaction recovery, `recover`, `template-info`, code-span parser fix. 192 tests. |
| v0.10.0-v0.10.1 | Renamed `response_mode` -> `agent_doc_mode`, bundled template instructions in SKILL.md. |
| v0.11.0-v0.11.2 | Append-friendly merge strategy, shared `merge.rs`, fixed duplicate headings. |
| v0.12.0-v0.12.2 | `compact`, `convert`, lazy claim, exchange default, exchange append mode default. 218 tests. |
| v0.13.0-v0.13.3 | `autoclaim` (SessionStart hook), git-based snapshot fallback, sync autoclaim, spare pane assignment (tmux-router v0.2.4). |
| v0.14.0-v0.14.9 | Route focus fix, `commands` subcommand, plugin autocomplete (v0.2.0), bidirectional `convert`, multi-backtick. 235 tests. |
| v0.15.0-v0.16.1 | CRDT stream mode (yrs), chain-of-thought, reactive file-watching, truncation detection, native compact. 309 tests. |
| v0.17.0-v0.17.30 | Format/write split, IPC write, JNA FFI (NativeLib.kt), VS Code prepend, immutable session binding, `lib-path`. 440 tests. |
| v0.18.0-v0.18.1 | `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3. 446 tests. |
| v0.19.0 | ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7). 446 tests. |
| v0.20.0-v0.20.3 | CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs. Session affinity enforcement. Numeric session fix (colon suffix). `claims` subcommand. 451 tests. |
| v0.21.0 | `agent-doc parallel` (fan-out with worktrees, `--no-worktree`). CRDT post-merge reorder (Yrs attribution). 461 tests. |
| v0.22.0-v0.23.0 | Boundary marker ordering, claim auto-start, VCS refresh, any-level HEAD markers, exchange default mode, SKILL.md immediate commit, code-block boundary fix. JB v0.2.8. 452 tests. |
| v0.23.1-v0.23.2 | Stale boundary cleanup, boundary-aware insertion (unmatched + explicit patches), IPC snapshot `content_ours` fix, (HEAD) marker accumulation fix, examples/blog outline. 459 tests. |
| v0.23.3 | Cross-platform sync pane guard (`ps` instead of `/proc`), pre-sync duplicate pane prevention, clippy fixes. 459 tests. |
| v0.24.0-v0.24.1 | `agent-doc install` (system setup), `agent-doc init` (project setup), lazy project init, SKILL.md content tests + heredoc examples, VCS refresh signal fix (JB v0.2.9). 497 tests. |
| v0.24.2 | SKILL.md step 3b (mandatory pending), `plugin install --local`, JB v0.2.10 (startup resync --fix). 497 tests. |
| v0.24.3 | Context session for auto_start (wrong-session fix), post-sync resync --fix. 497 tests. |
| v0.24.4 | Deterministic boundary re-insertion in apply_patches (end of exchange). 497 tests. |
<!-- /agent:releases -->

## Lessons

<!-- agent:lessons -->
1. **Always paired markers** — single markers unworkable for re-rendering
2. **Project root resolution** — walk up from file to find `.agent-doc/`, not CWD
3. **ATTACH before DETACH** — join new panes first, then stash unwanted
4. **Log every tmux command** — essential for debugging
5. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc
6. **Stash window numeric sessions** — append `:` to session target to avoid index ambiguity. This applies to ALL tmux `-t` targeting, not just stash. `new_window("-t", "0")` interprets "0" as window index; `new_window("-t", "0:")` forces session name. Root cause of persistent session bleedover bug fixed in v0.2.8.
7. **Stash window size** — resize to 200 rows before join to prevent "pane too small"
8. **YAGNI for speculative code** — `with_snapshot` was added speculatively, then deleted (no production callers)
9. **Subagents are expensive (~7x)** — use filesystem locking instead when possible; subagents only for true parallelism
10. **Stale snapshots from context compaction** — solved with `is_stale_snapshot()` auto-recovery in `diff.rs`
11. **Durable pending store** — save response before write, clear after success; survives compaction and crashes
12. **Distinct patch prefix** — patch markers vs agent markers prevents confusion between response directives and document components
13. **Skip code spans in ALL parsers** — any function scanning for `<!--` in user documents must use `find_code_ranges()`. Same fix needed THREE times: component.rs (v0.9.10), diff.rs (v0.14.8), boundary search in component.rs + write.rs (v0.23.0). Check all `<!--` parsers when adding new ones.
14. **Immutable session binding** — `tmux_session` in frontmatter should never be silently overwritten by claim. This caused cross-session pane swapping when autoclaim or lazy-claim ran in a different session. Fix: refuse to overwrite unless `--force`. Track multi-step plans in `agent:pending` to survive context compaction.
15. **No Gson in JetBrains plugin** — Gson causes `ClassNotFoundException` at runtime in some IntelliJ builds. Use hand-written JSON parsing. This bug was found and fixed twice (SlashCommandCompletionContributor Mar 11, PatchWatcher Mar 18) — check git history before introducing "bundled" dependencies.
16. **Never fall back to current_tmux_session() when tmux_session is set** — `display-message #{session_name}` returns whichever session the user's terminal is viewing. If the user switches to session 1, all routes using `current_tmux_session()` as fallback will create panes in session 1. Fix: bail with error instead of fallback. Only use `current_tmux_session()` for first-claim (no frontmatter binding yet).
17. **Deterministic operations belong in the binary, not the skill** — when adding a feature, ask "will ALL write paths need this?" If yes, implement in the binary. Boundary insertion was initially a SKILL step (1b) but caused stale boundary bugs when non-SKILL paths (run, stream, watch) bypassed it. Moved to `apply_patches()` in v0.24.4. The binary-vs-agent test: "can it be unit-tested with fixed inputs?" If yes → binary.
<!-- /agent:lessons -->

## Session History

<!-- agent:history -->
**v0.5.5-v0.9.6:** Foundation — window-scoped routing, 2D sync, components, patch, watch daemon, tmux-router extraction, flock locking, atomic rename, stale snapshot recovery.

**v0.9.7-v0.12.2:** `agent-doc write`, template mode, compaction recovery, `recover`, `exchange` convention, append-friendly merge, `compact`/`convert`, lazy claim.

**v0.13.0-v0.14.9:** `autoclaim`, git-based snapshot fallback, `commands`, plugin autocomplete (v0.2.0), bidirectional `convert`, code-range-aware `strip_comments`, multi-backtick.

**v0.15.0-v0.16.1:** CRDT stream mode (yrs), chain-of-thought streaming, reactive file-watching, truncation detection, native compact. 309 tests.

**v0.17.0-v0.17.30:** Format/write split. IPC write + JetBrains PatchWatcher. JNA FFI (NativeLib.kt). VS Code prepend. Immutable `tmux_session` binding. `lib-path` + `merge_frontmatter()` FFI export.

**v0.18.0-v0.18.1:** `undo`, `extract`, `transfer`. Pre-response snapshots. Gson revert, h2 scaffolding. JB v0.2.6, VS Code v0.2.3.

**v0.19.0:** ExecutionMode in config.toml. TmuxBatch (tmux-router v0.2.7).

**v0.20.0-v0.20.3:** CRDT dedup (#15), frontmatter patches (#16), binary-vs-agent docs, session affinity enforcement, numeric session fix (colon suffix), `agent-doc claims` subcommand. 451 tests.

**v0.21.0:** `agent-doc parallel` — fan-out with git worktrees (`--no-worktree` for read-only). CRDT post-merge reorder via Yrs attribution. 461 tests.

**v0.22.0-v0.23.0:** Boundary marker response ordering, claim auto-start, VCS refresh signal, any-level HEAD markers, exchange default mode fix, SKILL.md immediate commit, code-block boundary search fix (lesson #13). JB plugin v0.2.7->v0.2.8. 452 tests.

**v0.23.1-v0.23.2:** Stale boundary cleanup, boundary-aware insertion (unmatched content + explicit patches), IPC snapshot `content_ours` fix, (HEAD) marker accumulation fix, examples + blog outline. 459 tests.

**v0.23.3:** Cross-platform sync pane guard (`ps` replaces `/proc`), pre-sync duplicate pane prevention, clippy fixes. 459 tests.

**v0.24.0-v0.24.1:** `agent-doc install` (system setup), `agent-doc init` (project setup), lazy project init, SKILL.md content validation tests, heredoc examples, VCS refresh signal fix (JB v0.2.9). 497 tests.

**v0.24.2:** SKILL.md step 3b (mandatory pending updates), `plugin install --local`, JB plugin v0.2.10 (startup resync --fix). 497 tests.

**v0.24.3:** Context session for auto_start (eliminates wrong-session pane creation), post-sync resync --fix (auto-catches drift). 497 tests.

**v0.24.4:** Deterministic boundary re-insertion in `apply_patches()` — re-inserts at END of exchange after checkpoint writes. Boundary lifecycle: consumed by write, re-inserted by binary at correct position, cleaned up next cycle. 497 tests.
<!-- /agent:history -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/6242da3f1642397f4c39c15718184ea2013c87e2c39a77a88b2d05db9e40765a-20260322-225810.md`*
### Re: Audit docs, update docs, compact exchange

**Audit docs:** 1 advisory remaining (large table in CLAUDE.md — pre-existing, not a blocker). All staleness warnings cleared.

**README.md:** Updated version line to `v0.25.5 | JetBrains plugin v0.2.14 | VS Code extension v0.2.3`.

**SPEC.md:** Already updated earlier this session with boundary lifecycle invariants, cleanup scope, FFI export, and snapshot cleanup docs.

**Exchange compacted:** 590 lines archived to `.agent-doc/archives/`. Exchange is now clean.

**Plugin v0.2.14 installed** — confirmed by user.
Would agent-doc benefit from using AG-UI?
### Re: Would agent-doc benefit from AG-UI?

**Short answer: Not yet, but it's worth watching.**

AG-UI (Agent User Interaction Protocol) is CopilotKit's open protocol for streaming agent events to frontends. It standardizes:
- Lifecycle events (run started/finished)
- Text message streaming (deltas)
- Tool call events
- State snapshots
- Custom events

**Where agent-doc overlaps with AG-UI:**
- Both handle real-time agent output → UI updates
- Both need conflict-free concurrent editing (user types while agent writes)
- Both deal with structured component updates (AG-UI "state" ≈ agent-doc template components)

**Where agent-doc diverges:**
- **agent-doc is file-native** — the document IS the protocol. No HTTP transport, no WebSocket, no separate frontend. The editor (JetBrains/VS Code) is the UI, and the IPC mechanism is filesystem-based (JSON patch files + flock).
- **AG-UI assumes a web frontend** — React components, HTTP SSE transport. agent-doc's "frontend" is a markdown file in an editor.
- **agent-doc has git integration** — snapshots, selective commits, boundary markers. AG-UI doesn't address persistence/versioning.
- **AG-UI is transport-focused** — event streams over HTTP. agent-doc is content-focused — the protocol IS the document structure (components, patch blocks, boundaries).

**Potential benefit — if agent-doc adds a web UI:**
If you ever want a browser-based interface for agent-doc sessions (e.g., for users without JetBrains/VS Code), AG-UI could be the transport layer. The binary could emit AG-UI events instead of IPC patch files, and a React frontend could render the document with real-time updates.

**But today:** agent-doc's architecture is tightly coupled to the filesystem + editor plugin model. Adopting AG-UI would require:
1. An HTTP server (currently agent-doc is a CLI, no daemon)
2. A web frontend (currently the editor IS the UI)
3. Mapping component patches to AG-UI state events

This is a significant architectural shift for uncertain benefit. The current IPC-first model works well for the editor-centric workflow.

**Recommendation:** Monitor AG-UI for ideas (their state snapshot model is interesting), but don't adopt it unless you want a web UI. The file-native, editor-plugin architecture is agent-doc's differentiator.
Tell me more about the AG-UI state snapshot model. And how it could apply to agent-doc.
### Re: AG-UI state snapshot model and how it could apply to agent-doc

**AG-UI's State Management Model:**

AG-UI uses a **snapshot-delta pattern** for synchronizing structured state between an agent backend and a frontend:

1. **StateSnapshot** — sends the complete state object. Frontend replaces its entire state model:
   ```typescript
   { type: "STATE_SNAPSHOT", snapshot: { tasks: [...], status: "running", ... } }
   ```

2. **StateDelta** — sends incremental updates as RFC 6902 JSON Patch operations:
   ```typescript
   { type: "STATE_DELTA", delta: [
     { op: "replace", path: "/status", value: "complete" },
     { op: "add", path: "/tasks/-", value: { name: "new task" } }
   ] }
   ```

3. **Bidirectional** — both agent and frontend can modify shared state. The agent reads app state for context; the frontend observes agent state changes in real-time.

**How this maps to agent-doc:**

| AG-UI Concept | agent-doc Equivalent | Gap |
|---|---|---|
| StateSnapshot | Snapshot file (`.agent-doc/snapshots/`) | agent-doc snapshots are full-document, not structured state objects |
| StateDelta (JSON Patch) | IPC patch file (`.agent-doc/patches/`) | agent-doc patches are component-level, not JSON Patch RFC 6902 |
| Shared state object | Document components (`&lt;!-- agent:name --&gt;`) | Components ARE the shared state — both user and agent modify them |
| Event stream (SSE) | IPC filesystem + plugin watcher | No HTTP transport; filesystem is the channel |
| `fast-json-patch` apply | CRDT merge (`yrs`-based) | agent-doc uses CRDT for conflict-free merge, not JSON Patch |

**What's interesting for agent-doc:**

1. **Structured state as first-class concept.** AG-UI's state object is arbitrary JSON — agents can expose internal state (progress, decisions, errors) as structured data that the frontend renders. agent-doc currently embeds this in markdown components, which is less structured. A `&lt;!-- agent:state --&gt;` component could hold JSON state that the plugin renders as a rich panel (progress bars, decision trees, error summaries).

2. **JSON Patch for component updates.** Instead of replacing entire component content, agent-doc could emit RFC 6902 patches targeting specific lines/sections within a component. This would be more efficient for large components where only a few lines change (e.g., updating a single pending item without rewriting the entire list).

3. **Snapshot-delta pattern for recovery.** AG-UI uses snapshots as recovery points after connection loss. agent-doc already does this (snapshot + CRDT state), but could formalize it: on plugin reconnect, send a full StateSnapshot; during normal operation, send only StateDelta patches.

4. **Bidirectional state observation.** AG-UI lets the frontend push state TO the agent (e.g., user selections, form inputs). agent-doc's model is already bidirectional (user edits the document, agent reads the diff), but the diff is coarse-grained (whole document). Component-level state deltas could enable finer-grained user → agent signals.

**Practical application — what I'd take from AG-UI:**

The most transferable idea is **structured component state with JSON Patch deltas**. Instead of replacing `&lt;!-- agent:pending --&gt;` content wholesale each cycle, the binary could compute a JSON Patch from the old → new content and send that via IPC. The plugin would apply the patch atomically, preserving cursor position and undo stack within the component. This is a refinement of the existing IPC mechanism, not a protocol change.

The event stream model (SSE over HTTP) doesn't fit agent-doc's filesystem-native architecture, but the *concepts* (lifecycle events, delta vs snapshot, recovery) are already present in agent-doc's design — just expressed differently (git commits as lifecycle, IPC patches as deltas, snapshot files as recovery).

Sources:
- [AG-UI State Management](https://docs.ag-ui.com/concepts/state)
- [AG-UI Events](https://docs.ag-ui.com/concepts/events)
- [Building Real-Time AI Agent UIs with AG-UI](https://itsayushgupta.medium.com/building-real-time-ai-agent-uis-with-the-ag-ui-protocol-and-agui-hooks-14a78a8f7002)
Discuss feasibility of an interactive MDX document. Where live JSX components are embedded in Markdown.
### Re: Interactive MDX documents with live JSX components

**Concept:** An agent-doc session document rendered as MDX, where `&lt;!-- agent:name --&gt;` components become live React components — progress bars, approval buttons, diff viewers, task boards — updated in real-time by the agent via IPC patches.

**Feasibility: High, with clear layering.**

The key insight: agent-doc's component model (`&lt;!-- agent:name --&gt;...&lt;!-- /agent:name --&gt;`) already maps directly to JSX component slots. An MDX renderer would:

1. **Parse** the document's HTML comments as component boundaries (same as the binary does today)
2. **Map** each `agent:name` to a React component: `<AgentExchange />`, `<AgentPending />`, `<AgentStatus />`
3. **Subscribe** to IPC patches (via filesystem watcher or WebSocket bridge) for real-time updates
4. **Render** the component content as markdown (default) or structured UI (when the component has a registered JSX renderer)

**Architecture layers:**

```
┌─────────────────────────────────────┐
│  MDX Document (source of truth)     │  ← agent-doc write/commit
├─────────────────────────────────────┤
│  MDX Compiler (mdx-js/next-mdx)    │  ← parses MD + JSX
├─────────────────────────────────────┤
│  Component Registry                 │  ← maps agent:name → React component
├─────────────────────────────────────┤
│  Live Update Layer                  │  ← watches IPC patches, re-renders
├─────────────────────────────────────┤
│  Browser / Electron / VS Code panel │  ← renders the live MDX
└─────────────────────────────────────┘
```

**What this enables:**

- **`&lt;!-- agent:pending --&gt;`** → `<TaskBoard items={pending} onCheck={markDone} />` — interactive checkbox list that writes back to the document when the user clicks
- **`&lt;!-- agent:exchange --&gt;`** → `<ChatSurface messages={exchange} />` — rich chat UI with code highlighting, collapsible sections, inline images
- **`&lt;!-- agent:status --&gt;`** → `<StatusBar version="v0.25.5" tests={522} />` — compact metadata badge
- **`&lt;!-- agent:diff --&gt;`** → `<DiffViewer patches={patches} />` — side-by-side code diff
- **Custom components** → `<ApprovalButton onApprove={...} />`, `<ProgressRing percent={75} />`

**Bidirectional flow:**

The critical feature: user interactions in JSX components (clicking approve, checking a task, typing in a form) write back to the document. This closes the loop — the document remains the source of truth, but the UI provides richer interaction than raw markdown editing.

```
User clicks "Approve" in JSX → write to &lt;!-- agent:pending --&gt; → agent sees diff → responds
```

**Implementation path (incremental):**

1. **Phase 1 — Read-only MDX viewer:** Render the current document as MDX with basic component mapping. No live updates, just a nicer view. Could be a VS Code webview panel or a local dev server.

2. **Phase 2 — Live updates via file watcher:** Watch the document file for changes, re-parse and re-render. The agent writes via IPC, the MDX viewer picks up the change. This is the AG-UI snapshot-delta model but over the filesystem.

3. **Phase 3 — Bidirectional JSX → document:** User interactions in JSX components write back to the document (via `agent-doc write` or direct file edit). The agent sees these as diffs in the next cycle.

4. **Phase 4 — WebSocket bridge:** Replace file-watching with a WebSocket connection between the binary and the MDX viewer. This enables true real-time streaming (no filesystem polling latency).

**Technical considerations:**

- **MDX compilation:** `@mdx-js/mdx` compiles MDX to JSX at build time. For live updates, you'd need runtime compilation (slower) or a hot-reload setup. Next.js MDX or Astro MDX could work.
- **Component boundary detection:** The `&lt;!-- agent:name --&gt;` syntax is an HTML comment, which MDX passes through as-is. You'd need a remark plugin to detect these and transform them into JSX component boundaries.
- **Conflict resolution:** If the user modifies the document via the JSX UI *and* via the editor simultaneously, the CRDT merge (yrs) handles it — but the JSX UI needs to be a CRDT client too.
- **Portable document:** The MDX document must still be valid markdown when rendered without the JSX runtime (degraded mode). The `&lt;!-- agent:name --&gt;` comments are invisible in plain markdown, so the document degrades gracefully.

**What AG-UI contributes here:**

This is where AG-UI's state model becomes relevant — the JSX components could maintain a structured JSON state object (AG-UI StateSnapshot), and incremental updates from the agent could be expressed as JSON Patch deltas (AG-UI StateDelta). The document stays as the persistent layer; the state object is the ephemeral runtime layer for rich UI.

**Verdict:** Highly feasible. The component model is already there. The main work is building the MDX renderer + component registry + file watcher. Phase 1 (read-only viewer) could be a weekend project. Phase 3 (bidirectional) is where the real value lives.
Discuss feasibility of agent-doc + Jupyter.
### Re: agent-doc + Jupyter feasibility

**Concept:** Jupyter notebooks as agent-doc session documents — cells as components, kernel execution as live evaluation, agent writes code/markdown cells, user runs them interactively.

**Natural mapping:**

| agent-doc | Jupyter |
|---|---|
| `&lt;!-- agent:exchange --&gt;` | Markdown cells (conversation) |
| `&lt;!-- agent:code --&gt;` | Code cells (executable) |
| `&lt;!-- agent:output --&gt;` | Output cells (rendered results) |
| Template components | Cell metadata tags |
| IPC patches | Notebook kernel messages |
| Snapshot | `.ipynb` checkpoint |
| CRDT merge | Real-time collaboration (JupyterLab RTC) |

**Two integration strategies:**

**Strategy A — Jupyter as viewer (lightweight):**
agent-doc generates a `.ipynb` from the session document. Each `&lt;!-- agent:name --&gt;` component maps to a cell. The agent writes code cells that the user can execute. The notebook is a rendered view of the document, not the source of truth.

- Pros: No kernel dependency, document stays markdown, agent-doc binary unchanged
- Cons: Bidirectional sync is complex (notebook edits must propagate back to `.md`)

**Strategy B — `.ipynb` as session document (native):**
agent-doc natively supports `.ipynb` files as session documents. The diff/patch/snapshot cycle operates on notebook JSON. Components map to tagged cells (via cell metadata). The agent writes both markdown and code cells.

- Pros: Full Jupyter ecosystem (kernels, widgets, visualizations), native execution
- Cons: `.ipynb` is JSON not markdown (harder to edit in plain text editors), requires notebook-aware diffing

**What Jupyter adds over plain markdown:**

1. **Live code execution** — Agent writes a data analysis cell, user runs it, sees the plot. Agent sees the output in the next diff cycle and iterates.
2. **Rich output** — Charts, tables, images, interactive widgets (ipywidgets). Goes far beyond markdown rendering.
3. **Kernel state** — Variables persist across cells. Agent can build up a computation incrementally: define data → transform → visualize → iterate.
4. **Language-agnostic** — Python, R, Julia, Rust (via evcxr). The session document becomes a polyglot notebook.

**Technical considerations:**

- **Notebook diffing:** `.ipynb` is JSON with embedded base64 outputs. agent-doc's `similar`-based diff would need a notebook-aware mode that diffs cell sources (not outputs) and ignores execution counts. `nbdime` (Jupyter's diff tool) could be integrated.
- **Cell-level patches:** Instead of component-level patches, the binary would emit cell-level patches: insert cell at index, replace cell source, append output. This maps well to the existing `PatchBlock` model.
- **Execution as side effect:** When the agent writes a code cell, the user decides when to execute it. But an "auto-execute" mode could run trusted cells automatically (similar to `agent-doc stream` but with kernel execution).
- **JupyterLab extension:** Replaces the JetBrains/VS Code plugin. JupyterLab has a rich extension API (TypeScript/React), document model, real-time collaboration, and cell-level rendering. The IPC patch mechanism would work via JupyterLab's `DocumentModel`.
- **Kernel gateway:** For headless execution (agent runs code without a browser), Jupyter provides `jupyter_client` and `papermill`. agent-doc could spawn a kernel, execute cells, and capture outputs programmatically.

**Simplest useful integration:**

`agent-doc export --jupyter <FILE>` — converts a template-mode document to `.ipynb`:
- Markdown components → markdown cells
- Fenced code blocks → code cells (with language from info string)
- Component metadata preserved in cell tags

This is a one-way export (Phase 0) that gives users a Jupyter view of any session document without changing agent-doc's core architecture.

**Phase 1:** Bidirectional sync — watch the `.ipynb` for changes, diff against the `.md` source, merge back. Use `nbdime` for smart diffing.

**Phase 2:** Native `.ipynb` support — agent-doc reads/writes notebook JSON directly. Components are cells. The diff engine understands cell structure.

**Verdict:** High feasibility, especially the export path. Strategy A (Jupyter as viewer) is achievable with minimal binary changes. Strategy B (native `.ipynb`) is a larger effort but unlocks the full Jupyter ecosystem. The most practical first step is the `export --jupyter` command.
I moved the tasks/recruit/lucas-noble.md editor from the left side to the right side.
I can `Run Agent Doc` from the JB Plugin.
The tmux session remained on the left side.
When I switched editor view to tasks/agent-doc.md, the tmux pane switched to the right side.
### Re: Editor ↔ tmux layout sync behavior

Good observation. What you're seeing is:

**Layout sync triggers on editor focus, not on tab drag.**

When you switch which editor tab is active (clicking on `agent-doc.md`), the plugin fires a focus event → `agent-doc layout` repositions the tmux pane to match the editor's split side. But when you *drag* a tab from left to right, there's no focus event — the tab moves but the plugin doesn't know the split position changed until you click on it.

**Current flow:**
1. User focuses editor tab → JB plugin detects `FileEditorManagerListener.selectionChanged`
2. Plugin determines which split the editor is in (left/right)
3. Plugin calls `agent-doc layout` or sends layout IPC → tmux pane moves to match

**Gap:** JetBrains doesn't fire `selectionChanged` when a tab is dragged to a different split without changing focus. The tab physically moves but the listener doesn't trigger until you click on the tab in its new location.

**Possible fixes:**

1. **EditorSplitListener** — listen for `EditorSplitterListener` events (if available) which fire when splits are created/modified. This might catch tab moves between splits.

2. **Periodic poll** — every N seconds, check if any tracked document's editor split position changed. Lightweight but introduces polling latency.

3. **Tab drag listener** — JetBrains has `DockManager` and `EditorTabDragOutEvent` for detecting drag operations. Hooking into these could catch the move.

The simplest fix is option 1 — if JetBrains exposes split change events, we can react immediately. Want me to investigate the JetBrains API for this?

Yes. Also `Agent Doc Run` -> `agent-doc run` should have detected the layout. And moved the tmux pane accordingly. 
### Re: Run should detect layout and move tmux pane

You're right. Currently:
- **Claim** passes `--position` → tmux pane placed correctly
- **Run (route)** does NOT pass position → tmux pane stays wherever it was
- **Focus/Layout** (tab switch) syncs position → but only on focus change

The Run action should sync the layout as part of routing. Two approaches:

**Approach 1 — Plugin calls layout after route:**
After `agent-doc route <path>` succeeds, the plugin calls `agent-doc layout <files...> --split h|v` with the current editor positions. Simple, two CLI calls.

**Approach 2 — Add `--position` to `agent-doc route`:**
`agent-doc route <path> --position right` would route the command AND reposition the tmux pane. Single call, less latency.

Approach 2 is cleaner. The `route` command already knows the pane (it looks it up from the registry); adding a layout step is straightforward.

Want me to implement approach 2 — add `--position` to `agent-doc route` and have the plugin pass it?

Pass the entire declarative document layout instead. So we know what is on the left side.
### Re: Pass entire declarative layout on Run

**Done.** The fix is simple — `sendToTerminal()` (Run action) now calls `SyncLayoutAction.syncLayout()` after a successful route. This passes the **entire editor layout** declaratively via `agent-doc sync --col left_files --col right_files --focus active_file`.

**What happens now on Run:**
1. `agent-doc route <file>` → routes the command to the correct tmux pane
2. On success → `syncLayout()` detects the full 2D editor layout via `LayoutDetector`
3. Calls `agent-doc sync --col <left> --col <right> --focus <file>` → tmux panes rearrange to match

The `LayoutDetector` walks JetBrains' Swing Splitter tree to detect which `.md` files are in which column (left/right split). So the binary gets the complete layout picture — not just one file's position.

**Plugin v0.2.15 built** (unsigned + signed). Install from:
`editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.15.zip`

**Change:** One line added to `TerminalUtil.kt` — `SyncLayoutAction.syncLayout(project, notify = false)` after successful route.
I liked Approach 2 better. But with optional `--col left_files --col right_files --focus active_file` cli arguments.
### Re: Approach 2 — layout args on `agent-doc route`

**Done.** `agent-doc route` now accepts optional layout args:

```
agent-doc route <file> [--col left_files] [--col right_files] [--focus active_file]
```

**Binary changes (`main.rs`, `route` command):**
- Added `--col` (repeatable) and `--focus` args to the Route subcommand
- After routing, if `--col` args are provided, calls `sync::run()` to sync tmux layout
- Single CLI call handles both routing and layout sync

**Plugin changes (`TerminalUtil.kt`):**
- `sendToTerminal()` now detects the editor layout via `LayoutDetector`
- Passes `--col` and `--focus` args directly to `agent-doc route`
- Removed the separate `SyncLayoutAction.syncLayout()` call — it's now one command

**Example command the plugin generates:**
```
agent-doc route tasks/agent-doc.md --col tasks/agent-doc.md --col tasks/recruit/lucas-noble.md --focus tasks/agent-doc.md
```

This tells the binary: left column has `agent-doc.md`, right column has `lucas-noble.md`, focus is on `agent-doc.md`.

Plugin v0.2.15 rebuilt. Binary built and installed. 521 tests pass.
continue:
- [ ] Test route + layout sync with multi-split editor
- [ ] Investigate EditorSplitterListener for tab-drag events
### Re: Test layout sync + EditorSplitterListener investigation

**Testing route + layout sync:**

With plugin v0.2.15 installed, the Run action now passes `--col` and `--focus` args to `agent-doc route`. To test:

1. Open two `.md` files in a left/right split
2. Click "Run Agent Doc" on the right-side file
3. The tmux pane should move to match the right-side position

The plugin generates a command like:
```
agent-doc route tasks/file.md --col tasks/left-file.md --col tasks/right-file.md --focus tasks/right-file.md
```

**EditorSplitterListener investigation results:**

**No public API exists** for detecting tab-drag-between-splits events. Findings:

- `FileEditorManagerListener` provides: `fileOpened`, `fileClosed`, `selectionChanged` — no split/move event
- `selectionChanged` fires on focus change (clicking a tab) but NOT on dragging a tab to another split
- JetBrains acknowledged the gap: [IJPL-200433](https://youtrack.jetbrains.com/issue/IJPL-200433) requests promoting `SplitterService` to public API — not yet done
- The [tab-shifter plugin](https://github.com/dkandalov/tab-shifter) by dkandalov works around this by hooking into action execution rather than listening for events

**Workaround options:**

1. **Timer-based poll** (simplest): Every 2-3 seconds, run `LayoutDetector.detectEditorLayout()` and compare with the last known layout. If it changed, sync. Low CPU cost (just walking the Swing tree), catches all layout changes including drag.

2. **PropertyChangeListener on Splitter** (fragile): Attach listeners to `Splitter.proportion` property changes. This detects when splits are resized or created, but not tab moves between existing splits.

3. **Action listener** (targeted): Listen for `MoveEditorToOppositeTabGroup` and similar actions via `ActionManager.addAnActionListener`. Catches keyboard-triggered moves but misses mouse drags.

**Recommendation:** Option 1 (timer poll) is the most reliable. The existing `EditorTabSyncListener` already debounces layout syncs — adding a 2-3 second periodic check would catch all layout changes without depending on unstable internal APIs.

Sources:
- [API for moving editor tabs between splits](https://platform.jetbrains.com/t/api-for-moving-editor-tabs-between-splits/2362)
- [How to track file editors](https://www.plugin-dev.com/intellij/editor/editor-open-close/)
- [tab-shifter plugin](https://github.com/dkandalov/tab-shifter)
Will the poll send continous messages even if the layout does not change?
Or will it save the last layout and send when the layout changes?
### Re: Poll behavior — continuous or change-only?

**Change-only.** The poll would save the last known layout and only send a sync command when the layout actually changes. No continuous messages.

The `EditorTabSyncListener` already has this pattern — it computes a dedup key from the visible files and skips if unchanged:

```kotlin
// Pseudocode for the layout poll
var lastLayoutHash: String? = null

timer(interval = 2000) {
    val layout = LayoutDetector.detectEditorLayout(project)
    val hash = layout?.columns?.map { it.files }?.toString()
    if (hash != lastLayoutHash) {
        lastLayoutHash = hash
        // Layout changed — sync
        syncLayout(project, notify = false)
    }
}
```

So the poll:
- Runs every 2-3 seconds (lightweight — just walks the Swing component tree)
- Computes a hash of the current editor layout (which files in which columns)
- Compares with the last known hash
- Only calls `agent-doc sync` when the hash differs
- Typical cost when nothing changed: microseconds (no CLI call, no tmux interaction)
Are there any other events that we can hook into to avoid polling?

### Re: Events to avoid polling

Several event hooks that could replace or supplement polling:

**1. `ContainerListener` on the EditorsSplitters root (best candidate)**

Java Swing's `ContainerListener` fires `componentAdded`/`componentRemoved` when the component tree changes. When a tab is dragged to a new split, the Swing tree is restructured — new `Splitter` instances are created, old ones removed. Attaching a recursive `ContainerListener` to the `EditorsSplitters` root component would catch:
- New splits created (tab dragged to create a split)
- Splits removed (last tab closed in a split)
- Tab moved between existing splits (component removed from one, added to another)

```kotlin
val splitters = FileEditorManagerEx.getInstanceEx(project).splitters
(splitters as? Container)?.addContainerListener(object : ContainerListener {
    override fun componentAdded(e: ContainerEvent) = scheduleLayoutSync()
    override fun componentRemoved(e: ContainerEvent) = scheduleLayoutSync()
})
```

Caveat: may fire for non-split changes (toolbars, decorations). Need to filter events and debounce.

**2. `Splitter.PROP_PROPORTION` PropertyChangeListener**

The `Splitter` class fires `PROP_PROPORTION` when the split ratio changes and `PROP_ORIENTATION` when the orientation flips. Attaching to all `Splitter` instances in the tree would catch resize events — not directly useful for tab moves, but catches split creation/destruction side effects.

**3. `FileEditorManagerListener.FILE_EDITOR_MANAGER` message bus**

Already used by `EditorTabSyncListener`. Fires for:
- `fileOpened` / `fileClosed` — catches new splits (opening a file in a new split)
- `selectionChanged` — catches focus changes

Doesn't fire for tab drags within existing splits, but covers most scenarios.

**4. `AnActionListener` for move actions**

Listen for actions like `MoveEditorToOppositeTabGroup`, `SplitVertically`, `SplitHorizontally`, `Unsplit`, `UnsplitAll`:

```kotlin
ActionManager.getInstance().addAnActionListener(object : AnActionListener {
    override fun afterActionPerformed(action: AnAction, event: AnActionEvent, result: AnActionResult) {
        if (action.templateText in splitActions) scheduleLayoutSync()
    }
})
```

Catches keyboard-triggered moves but misses mouse drags.

**Recommended: Hybrid approach (events + fallback poll)**

Combine #1 (ContainerListener) + #3 (FileEditorManagerListener) for immediate detection of most layout changes, with a 5-second fallback poll as a safety net for edge cases. The poll rarely fires (only when events miss something), keeping the system responsive without relying solely on polling.

```
ContainerListener → immediate sync (split tree changes)
FileEditorManagerListener → immediate sync (file open/close/focus)
Fallback poll (5s) → safety net (catches anything events miss)
```

Want me to implement the hybrid approach?

Yes

Sources:
- [Splitter.java source](https://github.com/JetBrains/intellij-community/blob/master/platform/util/ui/src/com/intellij/openapi/ui/Splitter.java) — `PROP_PROPORTION` and `PROP_ORIENTATION` events
- [tab-shifter plugin](https://github.com/dkandalov/tab-shifter) — reference for programmatic tab movement
- [API for moving tabs between splits](https://platform.jetbrains.com/t/api-for-moving-editor-tabs-between-splits/2362) — JetBrains platform discussion
> **4. `AnActionListener` for move actions**

Would this be helpful?
### Re: Implement hybrid layout detection

**Done.** New `LayoutChangeDetector` class implements the hybrid approach:

**1. ContainerListener (immediate):**
- Attaches recursively to the `EditorsSplitters` Swing tree
- Fires on `componentAdded` / `componentRemoved`
- Automatically attaches to newly added child containers
- 300ms debounce before checking layout

**2. Fallback poll (5s safety net):**
- Daemon thread checks layout every 5 seconds
- Only syncs if the layout hash changed
- Catches edge cases events miss (e.g., drag operations that don't trigger container events)

**Both paths:**
- Compute a layout hash from `LayoutDetector.detectEditorLayout()`
- Compare with last known hash
- Only call `SyncLayoutAction.syncLayout()` when the hash differs
- Concurrency guard prevents parallel syncs

**Integration:**
- `PluginLifecycleListener.projectOpened()` starts `LayoutChangeDetector`
- `projectClosed()` disposes it (stops poll, removes listeners)

**Plugin v0.2.16 built** (unsigned + signed). Install from:
`editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.16.zip`
### Re: AnActionListener for move actions — helpful?

**Not needed now.** The `ContainerListener` already catches the result of all move actions (keyboard and mouse), because moving a tab between splits changes the Swing component tree — which triggers `componentAdded`/`componentRemoved`.

`AnActionListener` would add value only if:
- You wanted to react *before* the move completes (pre-action hook)
- The ContainerListener misses some move operations in practice

Since we have both ContainerListener (immediate) and the 5s poll (safety net), the AnActionListener is redundant. If testing reveals edge cases where tabs move without triggering container events, we can add it then.

**TL;DR:** Skip for now. The hybrid approach covers it. Add AnActionListener only if testing reveals gaps.
Would polling risk performance degradation in the editor's main loop? Or is it running on a separate thread or is in an event loop?
### Re: Polling performance — editor main loop risk?

**No risk.** The poll runs on a **separate daemon thread**, not the EDT (Event Dispatch Thread).

Looking at the implementation:

```kotlin
pollThread = Thread({
    while (!disposed.get()) {
        Thread.sleep(POLL_INTERVAL_MS)  // sleeps on its own thread
        checkAndSync("poll")             // layout detection on this thread
    }
}, "agent-doc-layout-poll").apply {
    isDaemon = true  // won't block JVM shutdown
    start()
}
```

**Thread breakdown:**
- **EDT (main loop):** Untouched. The poll never runs on it.
- **`agent-doc-layout-poll` thread:** Sleeps 5s, wakes, calls `LayoutDetector.detectEditorLayout()` (walks Swing tree — must be on EDT for thread safety), then sleeps again.

**Correction needed:** Actually, `LayoutDetector.detectEditorLayout()` accesses Swing components (`FileEditorManagerEx`, `Splitter`), which should technically be read from the EDT. The current implementation reads from a background thread, which works in practice (read-only access to the component tree is generally safe) but is technically a threading violation.

To be correct, the layout check should use `ApplicationManager.getApplication().invokeLater {}` to read the layout on EDT, then process the result on the background thread. The EDT work is ~microseconds (just walking the tree), so it won't block the UI.

I can fix this if you want — wrap the `detectEditorLayout()` call in `invokeLater` with a callback. But in practice, the current approach works fine because:
1. We only read the component tree (no writes)
2. The tree structure changes atomically from the EDT
3. Worst case on a race: we get a stale layout, which the next poll corrects

Want me to fix the threading or leave it as-is since it works?
explain:
- [ ] Optional: fix EDT threading for layout poll (invokeLater wrapper)
### Re: Explain EDT threading fix

Plugin v0.2.16 installed and tab-drag sync working — great!

**The issue:** `LayoutDetector.detectEditorLayout()` accesses Swing components (`FileEditorManagerEx.windows`, `Splitter` tree walking). Swing is single-threaded — all component reads/writes should happen on the EDT (Event Dispatch Thread). Our poll thread reads the Swing tree from a background thread.

**Why it works anyway:** We only *read* the tree (no modifications). The JVM memory model guarantees that reads see the latest values written by the EDT (Swing components use volatile fields internally). The worst case on a race: we see an intermediate state during a split operation, compute a wrong hash, and the next poll (5s later) corrects it.

**The fix (invokeLater wrapper):**

```kotlin
// Current (works but technically wrong):
Thread {
    Thread.sleep(5000)
    val layout = LayoutDetector.detectEditorLayout(project) // ← background thread reads Swing
    if (layoutChanged(layout)) syncLayout()
}

// Fixed (correct threading):
Thread {
    Thread.sleep(5000)
    ApplicationManager.getApplication().invokeLater {
        val layout = LayoutDetector.detectEditorLayout(project) // ← EDT reads Swing
        val hash = layout?.columns?.map { it.files.sorted() }?.toString()
        if (hash != lastLayoutHash.getAndSet(hash)) {
            // Sync on background thread (CLI call, not EDT work)
            Thread { syncLayout() }.start()
        }
    }
}
```

**Cost:** The `invokeLater` adds ~1-2ms latency (queued to EDT's event queue). The layout detection itself is ~microseconds. The CLI call (`agent-doc sync`) runs on a separate thread so it never blocks the EDT.

**Should you fix it?** It's low priority since it works. But it's the correct pattern for any JetBrains plugin accessing Swing components. If you want it fixed, I can update `LayoutChangeDetector` now.

Implement
### Re: Implement EDT threading fix

**Done.** `LayoutChangeDetector` now uses correct threading:

- **`attachContainerListener()`** — wrapped in `invokeLater`, Swing tree access on EDT
- **`checkAndSync()`** — layout detection on EDT via `invokeLater`, CLI sync call on background daemon thread
- **Disposed/project checks** — added `project.isDisposed` guard inside `invokeLater` blocks

Plugin v0.2.17 built (unsigned + signed). Install from:
`editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.17.zip`
release
### Re: Release v0.25.6 (HEAD)

**agent-doc v0.25.6 released:**

- crates.io ✓
- PyPI ✓
- GitHub Release ✓ (https://github.com/btakita/agent-doc/releases/tag/v0.25.6)
- Binary installed ✓

**What's in this release:**
- `agent-doc route --col --focus` — declarative layout sync in a single CLI call
- Plugin: `sendToTerminal` passes full editor layout to route
- Plugin: `LayoutChangeDetector` — ContainerListener + 5s fallback poll for tab-drag detection
- Plugin: EDT-safe threading (invokeLater for Swing reads)
- Plugin v0.2.17 (layout detection + EDT fix)

**8 files changed, 223 insertions, 7 deletions.**
<!-- agent:boundary:78df445a -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] agent-doc v0.25.6 released (all targets)
<!-- /agent:pending -->

<!-- agent:todo -->
- [ ] Install script (`curl | sh`) for binary download
- [ ] Homebrew formula (`brew tap btakita/agent-doc`)
- [ ] **lazily-rs DocContext** -- cache find_code_ranges() (sub-ms, not needed yet)
- [ ] Support `/btw` for instant processing
  - [ ] This may involve using /btw for agent-doc if /btw is the only way to prempt the loop if a task is running in the main thread
- [ ] Investigate https://github.com/HKUDS/ClawTeam to see if it would help agent-doc
- [ ] Multi-tmux-session per editor ([#17](https://github.com/btakita/agent-doc/issues/17))
- [ ] Multi-editor per tmux session ([#18](https://github.com/btakita/agent-doc/issues/18))
- [ ] Plugin phases 5-7 — component folding, gutter icons, structure view ([#19](https://github.com/btakita/agent-doc/issues/19))
- [ ] Multi-agent backends — codex, gemini ([#20](https://github.com/btakita/agent-doc/issues/20))
- [ ] `agent-doc deep` — parallel subagent fan-out with worktrees ([#21](https://github.com/btakita/agent-doc/issues/21))
- [ ] `agent-doc status` — Ratatui TUI dashboard ([#22](https://github.com/btakita/agent-doc/issues/22))
- [ ] Sub-profiles for submodule workflows ([#23](https://github.com/btakita/agent-doc/issues/23))
- [ ] Docker sandboxing (`--sandboxed`) ([#24](https://github.com/btakita/agent-doc/issues/24))
- [ ] `agent/api.rs` — direct HTTP API backend ([#25](https://github.com/btakita/agent-doc/issues/25))
- [ ] `agent-doc init --dashboard` ([#26](https://github.com/btakita/agent-doc/issues/26))
- [ ] Capture/log skill workflow ([#27](https://github.com/btakita/agent-doc/issues/27))
- [ ] Blog post: document modes with screencast ([#28](https://github.com/btakita/agent-doc/issues/28))
- [ ] `agent-doc stream` — bridge terminal to components ([#29](https://github.com/btakita/agent-doc/issues/29))
<!-- /agent:todo -->
