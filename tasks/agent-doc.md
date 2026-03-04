---
session: 56f0c92a-f2e9-44d2-9e7b-872ec3e14d91
resume: null
agent: null
model: null
branch: null
---

## Status Summary (compacted 2026-03-03)

### Architecture

**Tmux routing**: Window-scoped routing resolved. `SessionEntry` stores window ID. Plugin passes `--window` on all sync/claim/layout ops. `route` auto-starts dead panes. `claim` recovers from dead windows by scanning sessions.json for alive alternatives (v0.7.1).

**2D sync** (`agent-doc sync --col`): Declarative columnar layout with convergent reconciliation loop. Plugin detects editor splits via `LayoutDetector` (walks Swing Splitter tree). CLI arranges tmux panes to match: `-h` for columns, `-v` for stacking within columns. Fast path for focus-only changes (1 tmux command). Auto-creates Claude sessions for unclaimed docs.

**Sync reconciliation** (`sync.rs`): Additive algorithm — never breaks wanted panes from target window. Phases: SNAPSHOT → ANCHOR → EVICT unwanted → JOIN missing → REORDER if needed → VERIFY (convergent loop, max 3 attempts). Deferred eviction when unwanted is last pane. 14 integration tests + 3 proptest property-based tests covering horizontal, vertical, and 2D grid layouts.

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

### Pending / Not Built

- `TmuxBatch` — command batching to reduce flicker
- `agent-doc deep` — fan-out parallel subagents
- `agent-doc compact` — auto-summarize old exchanges
- `agent/api.rs` — direct HTTP API backend (replace junie bridge)

### Key Lessons from Sync Reconciliation (7 iterations → stable)

1. **Never break wanted panes** from the target window — scatter-gather destroys working state with no rollback
2. **No registry dependency for eviction** — break ANY unwanted pane, not just session-tracked ones
3. **Re-query tmux state before each operation** — cached state goes stale instantly
4. **Convergent loop** — retry reconciliation up to 3 times, checking layout after each attempt
5. **Log every tmux command** — essential for debugging multi-step operations
6. **Plugin must validate windows** — stale `sessions.json` entries cause cascading failures

## User

compact this file

## Assistant

**Compacted.** Collapsed 27 User/Assistant exchanges (lines 45-591) into the Status Summary. Preserved:

- Architecture descriptions (updated with current state: resilient claim, convergent sync, plugin behavior)
- Release history (condensed minor versions into ranges)
- Pending features list (removed "notification balloon" — now implemented)
- Key lessons from the sync reconciliation saga (6 hard-won rules)

Removed all resolved bug reports, intermediate fix attempts, test output logs, and release ceremony details. Document went from 595 lines → ~65 lines.

## User

I'm having issues syncing tmux in it's current state with the layout. Please add tests simulating what we have right now with tmux and the editors.

Also, can we have a hidden window that is a "stash" of all of the document sessions that are not currently open?

There was another tmux session that the inactive panes were going to. I wonder if the hidden single window session would fix the problem.

## Assistant

**Implementing both.** Here's the plan:

**Stash window** — 3 new methods on `Tmux` in `sessions.rs`:
- `find_stash_window()` — finds existing stash window by name in the tmux session
- `ensure_stash_window()` — creates one if missing, reuses if found
- `stash_pane()` — moves evicted panes to the stash window via `join_pane` instead of `break_pane` (which scatters into random new windows)

The EVICT phase of `reconcile()` now uses `stash_pane()`. ANCHOR and JOIN isolation steps also stash instead of scattering. REORDER keeps `break_pane` since those panes are immediately re-joined.

**6 new integration tests** simulating real-world state:
1. **Stale sessions** — 5 panes, kill 2, reconcile with 3 survivors
2. **Multi-file same pane** — multiple files claim same pane (like %41 with 5 files)
3. **Dead panes in desired** — some desired panes are dead, reconcile skips them gracefully
4. **Stash collects evicted** — evict 3 panes, verify all end up in stash window
5. **Stash reuse** — reconcile twice with different layouts, stash window reused not duplicated
6. **Large layout** — 8 panes in 3-column grid simulating real editor state

Implementation in progress in this session.

## User

Where is the spec file for agent-doc sync?

Now there are multiple windows in the tmux session.

We don't seem to be making good progress. It seems that there is something fundamentally wrong with the articture.

The PLUGIN MUST BE SIMPLE. No deduping. ONLY send agent-doc sync the layout. THAT IS ALL.

Please add tests for the current scenario. agent-doc sync should recover from this.

## Assistant

**Agreed — the plugin IS already simple.** It just sends `--col` args with file lists. The `.distinct()` is just deduping file paths (not panes). All intelligence belongs in the CLI.

**The spec** for sync is now at `agent-doc/SPEC.md` §7.18.

**Your current tmux state** (the real-world scenario):
- 4 solo windows: %62 (plugin.md), %39 (agent-doc.md), %60 (greensand-ai.md), %61 (parallel-coding)
- 1 stash window @206: %63 (seed) + %41 (5 files claimed this pane!)
- sessions.json: 34 entries, ~20 dead panes, stale window IDs

**3 new real-world scenario tests added** (all passing):

1. **`test_reconcile_scattered_panes_with_stash`** — 4 solo panes + 2 in stash, desired is 3 panes from the solo ones. Consolidates into target, stash untouched.

2. **`test_reconcile_target_has_unwanted_and_stash_exists`** — Target has [A, X1, X2], stash already has [S1, S2]. Desired: [A, B]. X1/X2 get stashed alongside S1/S2. B joins from solo window.

3. **`test_reconcile_full_real_world_scenario`** — 4 solo windows + stash with 1 pane. Previous sync left A+C together. New desired: [B,D] col1, [A] col2. Evicts C, joins B and D from solo windows. Stash untouched.

**Total: 123 tests pass, clippy clean, binary installed.**

The fundamental issue you're hitting: `break_pane` during ANCHOR/JOIN isolation creates temporary solo windows (visible flicker). The stash catches EVICT but not the pre-join isolation step. Your revert of ANCHOR/JOIN back to break_pane was correct — stash_pane fails when the pane is already in the stash window.

## User

This pane management is causing too many problems.

If a claude session is closed while running, will the running pause or will it only restart when the session is resumed?
