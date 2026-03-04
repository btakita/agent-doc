---
session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
tmux_session: claude
---


## Status Summary (compacted 2026-03-04)

### Architecture

**tmux-router crate**: Extracted standalone Rust library (`tmux-router` v0.1.0) from agent-doc. Contains `Tmux` struct (30+ tmux operations), `Registry` (persistent key-to-pane mappings), `reconcile()` (2D layout sync algorithm), `IsolatedTmux` (RAII test infra), stash window, `FileResolution` callback. Published to crates.io. 46 tests (unit + integration + proptest). GitHub Pages docs at https://btakita.github.io/tmux-router/.

**agent-doc as consumer**: v0.8.0 depends on tmux-router. `sessions.rs` (1312→413 lines) re-exports `Tmux`, `IsolatedTmux`, `RegistryEntry`, `Registry` from tmux-router. `sync.rs` (2518→64 lines) is a thin adapter: resolves frontmatter to `FileResolution`, calls `tmux_router::sync()`, writes `tmux_session` back to files. Free functions `capture_pane()` and `send_key()` use `tmux.cmd()` (pub in tmux-router).

**Tmux routing**: Window-scoped routing resolved. `SessionEntry` stores window ID. Plugin passes `--window` on all sync/claim/layout ops. `route` auto-starts dead panes and now joins active window via `join-pane` instead of creating separate windows (v0.8.0 fix). `claim` recovers from dead windows by scanning sessions.json for alive alternatives.

**2D sync** (`agent-doc sync --col`): Declarative columnar layout delegated to tmux-router. Plugin detects editor splits via `LayoutDetector` (walks Swing Splitter tree). CLI arranges tmux panes to match. Fast path for focus-only changes. Auto-creates Claude sessions for unclaimed docs. In-memory auto-registration for unresolved agent-doc files using same-column donor panes.

**Sync reconciliation**: ATTACH-first order (ATTACH→SELECT→DETACH). Joins desired panes before evicting unwanted ones. `-d` flag on ALL `join-pane` calls during reconcile prevents intermediate selection changes. Only final `select_pane` sets active pane. Same-column-only donor pane search (no cross-column spiral). Stash window for evicted panes (prevents orphan window creation).

**Focus resolution**: 4-tier fallback: (1) non-agent-doc file → preserve selection, (2) resolved in file_to_pane → select pane, (3) column-positional fallback → first resolved pane in same column, (4) file not in layout → preserve selection.

**Frontmatter**: Renamed `session:` → `agent_doc_session:` with serde alias for backward compatibility. Old documents auto-upgrade on next write cycle. `tmux_session:` field for multi-project tmux session affinity.

**Plugin**: Always sends `sync --col` (never bare focus). Passes `--focus <active-file>` for tmux pane selection. Inline hints at cursor for sync notifications. `projectWindowId()` validates windows via `tmux list-panes` to avoid stale references.

**Junie integration**: Fire-and-forget bridge (`junie-bridge.sh`). Plugin copies diff to clipboard + opens request.md. Direct API backend (`agent/api.rs`) designed but not built.

### Release History

| Version | Key Changes |
|---------|-------------|
| v0.5.5 | Window-scoped routing, `--pane`/`--window` flags |
| v0.5.6 | Commit-doc linking (`agent-doc(doc-name): timestamp`) |
| v0.6.0 | `agent-doc sync` — 2D columnar layout, `LayoutDetector` |
| v0.6.1–v0.6.2 | Junie agent backend, fire-and-forget bridge |
| v0.6.3–v0.6.4 | LayoutDetector fixes, layout equalization |
| v0.6.5 | Plugin always sends `sync --col`, single-file cleanup |
| v0.6.6 | `--focus` on SyncLayoutAction, inline hint at cursor |
| v0.7.0 | Snapshot-diff-apply sync rewrite, column inversion fix, dead window fallback |
| v0.7.1 | Convergent reconciliation loop, proptest, deferred eviction, resilient claim |
| v0.7.2 | Sync reconciliation fixes: same-column-only donors, focus preservation for unclaimed files, ATTACH-first order |
| v0.8.0 | tmux-router extraction (-3133 lines), route auto_start joins active window, frontmatter rename to `agent_doc_session` |

### tmux-router v0.1.0

| Item | Status |
|------|--------|
| Crate extraction | Done — `tmux-router` crate with Tmux, Registry, sync, IsolatedTmux |
| crates.io | Published: https://crates.io/crates/tmux-router |
| GitHub | https://github.com/btakita/tmux-router |
| CI | GitHub Actions: clippy + 46 tests (installs tmux via apt) |
| Release workflow | `cargo publish` + GitHub Release on tag push |
| mdbook | Deployed to https://btakita.github.io/tmux-router/ |
| Rust edition | 2024 |
| Registry | `.tmux-router/registry.json`, file-path-as-key (no UUID dependency) |
| Config | `~/.config/tmux-router/config.toml` + CLI flags override |

### Pending / Not Built

- `TmuxBatch` — command batching to reduce flicker
- `agent-doc deep` — fan-out parallel subagents
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend (replace junie bridge)
- tmux-router CLI binary — `tmux-router sync`, `tmux-router register`, etc.
- `CARGO_REGISTRY_TOKEN` secret on tmux-router GitHub repo (for automated crates.io publish)

### Key Lessons

1. **Never break wanted panes** from the target window — scatter-gather destroys working state with no rollback
2. **No registry dependency for eviction** — break ANY unwanted pane, not just session-tracked ones
3. **Re-query tmux state before each operation** — cached state goes stale instantly
4. **ATTACH before DETACH** — join new panes first, select focus, then stash unwanted. Prevents intermediate selection changes
5. **`-d` on ALL join-pane during reconcile** — no pane/window selection changes until final explicit `select_pane`
6. **Same-column-only donors** — cross-column spiral causes wrong pane selection when focus file is unresolved
7. **Log every tmux command** — essential for debugging multi-step operations
8. **Plugin must validate windows** — stale `sessions.json` entries cause cascading failures
9. **Dual-layer test coverage** — algorithm tests in tmux-router, integration tests in agent-doc. Edge cases in both.

## User

I had an issue with agent-doc run on an old claim. I ran it on corky.md

It create a 3rd column on the right side

left1 | right1 | right2*

I then lost the session. Not sure if it's running atm. Please help.

## Assistant

**Your corky.md session is alive and idle.** It's in pane %83, window @315, waiting at the Claude prompt.

**Current tmux layout:**
- **@315 (active window)**: %39 (agent-doc.md, this session) | %83 (corky.md, idle at prompt)
- **@382**: %79 (parallel-coding-with-tmux-markdown.md)

**What happened with the 3rd column:** `agent-doc route` (or `claim`) for corky.md created a new pane and `join-pane`'d it into the active window @315 alongside the existing 2 panes, making 3 columns. The layout has since settled back to 2 panes.

**The corky.md session in %83 is not running anything** — it completed its last task and is at the idle prompt. You can send it a new command or use `/agent-doc tasks/corky.md` to interact via the document.

**sessions.json has 34 entries, most pointing to dead panes.** Run `agent-doc resync` to clean up stale entries.

## User

We should actively resync to avoid this issue. Contemplating having a daemon as well. For now, let's have resync happen where appropriate...such as before run or sync.

## Assistant

**Done.** Added `resync::prune()` — a quiet auto-resync that runs before every `route`, `sync`, and `claim` operation.

**Changes:**
- `resync.rs`: New `prune()` function — removes dead panes + deduplicates, prints 1 line if anything was pruned, silent otherwise
- `route.rs`: Calls `prune()` at the start of `run_with_tmux` (before registry lookup)
- `sync.rs`: Calls `prune()` at the start of `run_with_tmux` (before layout calculation)
- `claim.rs`: Calls `prune()` at the start of `run` (before window resolution)

Existing `agent-doc resync` command stays as-is (verbose output with full session listing).

**106 tests pass, clippy clean, binary installed.**

**Re: daemon** — good future addition. The lightweight `prune()` is ~1 `tmux list-panes` call, so the overhead per operation is negligible. A daemon would enable push-based cleanup (watch for pane exits) rather than poll-based.

## User

Is prune in tmux-router or agent-doc?

## Assistant

**Currently in agent-doc** (`agent-doc/src/resync.rs`). It calls `sessions::load()` / `sessions::save()` which use agent-doc's `.agent-doc/sessions.json`.

The logic is generic though — prune dead panes from a registry file. It **should live in tmux-router** as `tmux_router::prune(tmux, registry_path)` since tmux-router already owns the `Registry` type and load/save functions. Agent-doc would just call it with its registry path.

**Proposal:** Move to tmux-router before releasing. That makes it tmux-router v0.1.1 + agent-doc v0.8.1.

## User

Yes. Move to tmux-router.

## Assistant

**Released both.**

**tmux-router v0.1.1:**
- Added `tmux_router::prune(registry_path, tmux)` in `registry.rs` — removes dead panes + deduplicates, saves if changed, returns count removed
- Re-exported from `lib.rs`
- crates.io: https://crates.io/crates/tmux-router/0.1.1
- GitHub Release: https://github.com/btakita/tmux-router/releases/tag/v0.1.1

**agent-doc v0.8.1:**
- `resync.rs` rewritten to delegate to `tmux_router::prune()` — 145→65 lines
- `prune()` calls `tmux_router::prune(&registry_path, &tmux)` + prints message if anything removed
- `run()` (verbose) loads before/after to show what was removed
- crates.io: https://crates.io/crates/agent-doc/0.8.1
- GitHub Release: https://github.com/btakita/agent-doc/releases/tag/v0.8.1
- Binary installed: `agent-doc 0.8.1`

**All tests pass** (46 tmux-router, 106 agent-doc), clippy clean. Parent submodule pointers updated.

## User

Let's start ideating on the dashboard-as-document concept mentioned in tasks/parallel-coding-with-tmux-markdown.md

## Assistant

From the parallel-coding discussion, three features are needed for dashboard-as-document. Here's the ideation:

**1. `agent-doc watch <file>` — auto-submit on change**

The simplest feature. An inotify/polling loop that triggers `agent-doc route <file>` when the file changes. Could use the `notify` crate (Rust-native file watching, cross-platform). Closes the trigger gap — external watchers write to the file, agent-doc auto-responds.

Would I need a new process for each file? Can there be a background daemon that is explicitly started or auto-starts on the first call?

Debounce is critical (500ms–1s) to avoid firing mid-write. The JetBrains plugin already has this problem solved.

Should we move the debounce logic to agent-doc? agent-doc should fully work independently of the Jetbrains plugin.

**2. Sectional write-back — agent edits specific sections**

The core architectural change. Today agents only append `## Assistant` blocks. A dashboard needs inline updates. Two approaches:

- **HTML comment markers**: `<!-- agent:status-table -->...<!-- /agent:status-table -->` — agent replaces content between markers
We have range templates. Presumably it would be for a full replacement of the status-table?
Can we have nested templates?
Can we also have in-place templates (just the template itself rather than open template/close template)?

- **Section targeting**: `## Status` heading → agent replaces content under that heading until next heading

The marker approach is simpler and doesn't require heading parsing. It also works inside any section, not just top-level headings. The snapshot diff needs to understand "this section was modified by the watcher" vs "this section was last written by the agent."

**3. Structured field awareness (future)**

Parse template fields: `{{session.status}}` → typed changes. The diff becomes "field `ses-002.status` changed from `active` to `flagged`" rather than raw text. Skip for now — raw diff + section markers covers 80% of dashboard use cases.

**Proposed MVP:**
- `agent-doc watch` using `notify` crate with debounce
- HTML comment markers for sectional write-back
- Dashboard template is just a markdown file with marker regions
- Watcher process (external) writes to the file → `agent-doc watch` auto-submits → agent updates marked sections

**Open questions:**
- Should `watch` be a long-running daemon or a one-shot triggered by inotify? (daemon is simpler for users)
I addressed it above. Explicitly started daemon or lazy-started daemon.
- Should sectional write-back be in the skill instructions or in the CLI? (CLI is more reliable — `agent-doc write-section <file> <section-name>`)
cli
- How do we prevent infinite loops? (watcher writes → agent responds → file changes → watcher writes again)
Can we pass the upstream reason as an argument for the change?

It may be possible for the document to initiate a change that causes a self-update. But it's ok if the cycle is bounded. Need a way to detect unbounded infinite loops. Having an infinite loop may be ok? If the agent document can be in a steady state.

## Assistant

Addressing your inline annotations:

**Re: daemon model for watch**

A single daemon per project. `agent-doc watch` starts a background process that watches all claimed files. Auto-start on first `claim` or `route`, explicit start via `agent-doc watch --start`. Holds one `notify` watcher, routes file changes to the correct tmux pane. No per-file process.

How does agent-doc know what a project is? Related to the tmux pane? Is a project the same as tmux_session?
Does tmux-router know about the project?

**Re: debounce in agent-doc**

Yes — debounce belongs in agent-doc, not the plugin. The daemon debounces per-file (500ms default, configurable). The plugin can fire immediately — agent-doc coalesces.

Good. Let's stage for implementation.

**Re: range templates**

Three template flavors:

1. **Range template** (open/close): `<!-- agent:status -->...<!-- /agent:status -->` — full replacement of content between markers
2. **Nested templates**: Yes — `<!-- agent:outer -->` can contain `<!-- agent:inner -->`. Inner markers updated independently. Parse innermost first.
3. **In-place template**: `<!-- agent:timestamp -->` (single marker, no close) — replaced with a single inline value. Works for scalars: timestamps, status badges, counters.

Would we always need range templates? How would a single marker work for multiple renders? How would the next render replace the content from the first render using a single marker? How would we know exactly what is template and what is data?

**Re: CLI for sectional write-back**

`agent-doc write-section <file> <section-name> [content]` — reads from stdin or argument. Skill instructions just say "use `agent-doc write-section` to update dashboard fields."

Would write work instead of write-section? Is the impulse for contextualizing write-section due to supporting other types of writes?

**Re: infinite loops**

Three mechanisms:

1. **Change source tracking**: `--source watcher` vs `--source agent`. Agent sees where the change came from and decides whether to respond.
2. **Bounded cycles**: `--max-cycles 3` — after 3 consecutive agent-triggered re-submits with no external change, pause.
3. **Steady state detection**: If agent's response produces no diff from previous response (idempotent), stop re-triggering. Convergence = done.

Bounded cycles as the safe default. Infinite loops that converge are fine; divergent loops caught by counter.

**Revised design summary:**

| Component | Description |
|-----------|-------------|
| `agent-doc watch` | Single daemon per project, `notify` crate, per-file debounce, lazy or explicit start |
| Range templates | `<!-- agent:name -->...<!-- /agent:name -->` for sections, single marker for scalars |
| Nested templates | Supported — innermost-first parsing |
| `write-section` CLI | `agent-doc write-section <file> <section> [content]` |
| Loop prevention | Source tracking + bounded cycles (default 3) + steady state detection |

## Assistant

Addressing your second round of annotations:

**Re: project identity**

A "project" = the directory containing `.agent-doc/`. Same pattern as git (`.git/`), cargo (`.cargo/`). tmux-router is project-agnostic — it operates on registry paths and tmux sessions passed in by the caller. agent-doc resolves the project root and passes paths to tmux-router. Not the same as `tmux_session` — that's session affinity (multiple projects can share one tmux session, or one project can span multiple).

Let's document this concept. In making the docs, let's follow the same ontology pattern as in philosophy.
project is scoped to the context of agent-doc of course.


**Re: range templates only**

Always range templates. Single markers are unworkable for re-rendering — after the first render, there's no way to distinguish the marker from rendered data. You'd need a second marker to delimit the boundary. Range templates solve this cleanly: everything between `<!-- agent:name -->` and `<!-- /agent:name -->` gets replaced. The markers themselves are preserved. Drop in-place templates from the design.

Explain reason in documentation.

**Re: `write` vs `write-section`**

`write` is cleaner. Section name is just an argument:

How about `patch` instead of `write`? I think that is a good distinction.

- `agent-doc write <file> <section>` — replace content between range markers (reads from stdin)
- `agent-doc submit <file>` — existing append mode (diff → respond → append)

Two verbs, two modes. `write` for targeted section updates, `submit` for conversational flow.

**Updated design summary:**

| Component | Description |
|-----------|-------------|
| `agent-doc watch` | Single daemon per project (`.agent-doc/` root), `notify` crate, per-file debounce, lazy or explicit start |
| Range templates | `<!-- agent:name -->...<!-- /agent:name -->` — always paired, nested supported (innermost-first) |
| `agent-doc write` | `agent-doc write <file> <section> [content]` — replace range template content |
| Loop prevention | Source tracking (`--source`) + bounded cycles (default 3) + steady state detection |
| Project identity | Directory containing `.agent-doc/`; tmux-router is project-agnostic |

## User

Let's have a "tips" section:
- Agent documents can reference other files including other agent documents

Any other tips?

I also have exkernel installed. So we can overload project for agent-doc.
