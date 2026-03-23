---
agent_doc_session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


# agent-doc

<!-- agent:status -->
**Version:** v0.25.5 | **Tests:** 522 (168 lib + 316 bin + 38 CLI) | **Dependencies:** tmux-router v0.2.8, instruction-files v0.1
**Published:** GitHub Release ✓ | crates.io ✓ | PyPI ✓
**Plugins:** JetBrains v0.2.14 (FFI reposition + boundary cleanup) + VS Code v0.2.3
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
<!-- agent:boundary:9cead441 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [ ] Interactive MDX viewer — Phase 1 (read-only) as VS Code webview or local dev server
- [ ] Remark plugin for `&lt;!-- agent:name --&gt;` → JSX component transformation
- [ ] Consider AG-UI state model for JSX component runtime state
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
