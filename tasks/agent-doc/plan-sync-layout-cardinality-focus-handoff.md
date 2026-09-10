# Plan: Sync layout cardinality and focus handoff

## Goal

Make JetBrains `Sync Tmux Layout` and automatic tab-to-pane sync converge on exactly the editor-visible agent-doc pane set, while reliably focusing the selected document pane.

The concrete repro is a two-editor-pane layout where manual `Sync Tmux Layout` expanded the `agent-doc` tmux window to 3-4 panes, and automatic sync did not switch focus when the editor landed on `src/boost-client/tasks/monsterrodholders.md`.

## Working Theory

These symptoms share the same projection boundary:

- The plugin's visible editor snapshot is supposed to be the complete desired layout projection.
- `agent-doc sync --no-autostart` must not treat protected or unresolved panes as permission to grow the visible tmux window beyond that projection.
- A preserve-layout or blocked-layout result can still be a valid non-destructive outcome, but it must not be reported as focus-applied unless the selected pane is actually active.

Existing adjacent plans cover parts of this surface:

- `tasks/agent-doc/plan-jetbrains-cross-root-split-sync-validation.md` validates cross-root visible layout reporting.
- `tasks/agent-doc/plan-monsterrodholders-empty-column-sync-replay.md` validates empty-column replay for mixed markdown/non-markdown splits.
- `tasks/agent-doc/plan-passive-sync-existing-pane-handoff.md` targets preserve-layout focus handoff.

This plan ties those together around pane cardinality and plugin completion semantics.

## Steps

1. Reproduce the exact two-editor-pane setup:
   - one visible editor pane on `tasks/agent-doc/agent-doc-bugs2.md`
   - one visible editor pane on `src/boost-client/tasks/monsterrodholders.md`
   - capture the JetBrains command, working directory, `--col` list, `--focus`, and whether the action was manual `Sync Tmux Layout` or automatic tab sync
   - capture `tmux list-panes` before and after, plus `/tmp/agent-doc-sync.log`

2. Add a regression fixture for the plugin projection:
   - JetBrains layout detection must emit two columns for the two editor panes
   - cross-root paths must keep the workspace-root `.agent-doc/last_layout.json` scope
   - empty or non-markdown sibling columns must remain positional placeholders instead of being dropped before the Rust sync layer can apply column memory

3. Add a Rust sync regression for pane cardinality:
   - start with two visible registered panes in the `agent-doc` window
   - include one requested cross-root document and one focus target
   - assert manual sync and safe-passive sync do not leave more visible tmux panes than the requested/remembered column projection
   - assert extra live/protected panes stay in stash or their existing non-target window instead of being joined into `agent-doc`

4. Tighten the Rust sync implementation:
   - audit `run_with_options` before `tmux_router::sync_with_options`
   - make blocked/unresolved/protected panes contribute to an explicit "preserve current visible layout" outcome instead of widening the desired layout
   - ensure `build_layout_state` and `apply_column_memory` preserve column slots without duplicating remembered docs into multiple visible panes
   - log desired pane count, actual visible pane count, blocked files, protected panes, and selected focus pane in one structured sync line

5. Tighten plugin completion semantics:
   - automatic sync should only update `lastVisibleSignature` and `lastFocusedFile` after a result proves focus applied
   - preserve-layout output should keep the request pending unless it also reports `safe_passive_layout_preserved_reselected_focus`
   - manual `Sync Tmux Layout` should surface a warning when the post-sync pane count exceeds the editor-visible projection

6. Update specs:
   - `src/agent-doc/editors/SPEC.md`
   - `src/agent-doc/editors/PLUGIN-SPEC.md`
   - `src/agent-doc/specs/07-session-tmux-commands.md`

## Likely Files

- `src/agent-doc/src/sync.rs`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/EditorTabSyncListenerTest.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/SyncLayoutActionTest.kt`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `src/agent-doc/editors/SPEC.md`
- `src/agent-doc/editors/PLUGIN-SPEC.md`

## Verification

- Focused Rust tests for column memory, safe-passive preserve-layout, and cross-root sync.
- JetBrains plugin tests for two-column layout detection, active-file replay, and preserve-layout retry behavior.
- A live validation run in the reported JetBrains layout:
  - before manual sync: 2 editor panes and 2 intended tmux panes
  - after manual sync: no more than 2 visible `agent-doc` tmux panes
  - after landing on `monsterrodholders.md`: the monsterrodholders pane is active without requiring manual sync

## Exit Criteria

- `Sync Tmux Layout` cannot expand a two-editor-pane projection into 3-4 visible tmux panes.
- Automatic sync focuses `monsterrodholders.md` when its pane is already visible or retries when preserve-layout prevented focus.
- Logs make the outcome unambiguous: desired columns, visible pane count, preserved panes, and active focus pane are all recorded.

## Current implementation contract (2026-09-10)

Invariant: provisioning owned by a layout operation cannot change the visible
pane set or selection before the current layout graph places the returned pane.
Two visible editor columns therefore stay at two panes while an obsolete or
concurrent startup completes.

Policy owner: the existing ControllerPaneLayoutGraph owns placement and its
generation. Route startup is the process-creation effect adapter and must stage
layout-owned creations; standalone route startup keeps its existing explicit
visible-placement behavior.

Transition table:
- LayoutProvision or deferred-to-layout start with a visible target window:
  create a detached staging pane, register/start it, preserve visible geometry
  and selection, and return the normal pane binding receipt.
- Existing document owner: reuse it; no duplicate creation.
- Current layout includes the new owner: the existing reconciliation Effect
  attaches it and stashes the replaced owner, preserving both processes.
- Layout superseded before startup completes: the new owner remains off-screen;
  completion cannot append a third pane to the settled two-pane layout.
- Standalone explicit route: retain its visible anchor/placement policy.
- No target window yet: retain the existing first-window bootstrap contract.
- Creation failure: return the error without promoting or deleting another pane.

Evidence inputs: the 2026-09-09T23:11:43Z log proves reconciliation stashed %181,
but a concurrent startup created %182 before VERIFY, leaving [%177,%6,%182]
for desired [%177,%6]. The current IDEA has two columns and the controller
reports a converged [%177,%6] cut. The historical startup replay fix removes one
trigger; the process-creation boundary still directly splits the visible window
for layout provisioning and allows this race.

Reactive topology: existing desired-layout Source → applicability Computed →
structural Effect → remote/local startup pane-binding receipt Source → current
layout placement Effect. Creation supplies the binding, not visibility authority.
Existing generation fences decide whether that binding remains desired.

Imperative extraction audit: remove visible-window mutation and focus selection
from layout-owned startup; both are already owned by the layout Effect. Launch
mode/argv and the bounded tmux create command are actorless effect-boundary cuts,
not long-lived derived caches. Reuse the existing mutation observation invalidation
and cross-controller receipt publication. No polling, new graph, or SQLite arbiter.

Allowed edit surfaces: route startup adapter, deterministic transition/isolated
tmux regression tests, tmux-ci coverage selection if needed, routing specs and
release metadata. Do not modify the already-dirty tmux-router checkout.

Verification: reproduce three visible panes using the production creation helper;
prove staging leaves two panes and selection unchanged, a current replacement
can be joined with two visible panes, and the superseded staged owner stays
alive. Cover standalone placement and empty-window bootstrap compatibility.
Run full make check and tmux-ci, release/install, and read live geometry.

Out of scope: killing or interrupting user agents, changing operator focus to
manufacture a repro, changing queue permissions, and another window-policy owner.

## Completion proof (2026-09-10, v0.35.360)

Implemented the contract in `agent-doc-route-io/src/startup.rs`: anchored
layout-owned starts create a detached staging window and return the usual binding;
standalone routing retains its visible split. The existing Lazily layout graph
owns attachment and selection. No new state owner, polling loop, or SQLite
coordination was introduced; the dirty sibling tmux-router checkout was untouched.

The production-helper regression failed before the fix with three visible panes
instead of two, then passed after staging. Isolated tmux coverage proves late and
concurrent starts, physical column order, preserved selection, paused queues,
replacement placement, and surviving superseded/replaced sessions. The tmux CI
target now includes these regressions.

Verification: `make check tmux-ci` passed, including 9,302 workspace tests (259
skipped), 28 explicit tmux tests, editor parity, native cross-editor simulation,
Lean and TLA checks. Privacy audit and diff checks passed. Logs are under
`/home/brian/.cache/agent-doc-test-tmp/layout-{check,install,release-build}.log`;
the before/after repro logs are `layout-regression-{before,after}.log`.

Source commit `642da69db` and tag `v0.35.360` are pushed. `make install-full`
installed CLI/native 0.35.360 and matching existing editor packages. A normal
project controller recycle succeeded without force or child interruption;
controller PID 3297648 reports 0.35.360 and stable handoff. Live window @74 still
contains exactly [%177,%6], with %6 active. The pre-recycle reactive receipt also
proved exact desired/actual documents and pane order; no editor event had yet
published a new status frame immediately after recycle, so the post-install
claim is the directly observed tmux geometry, not an invented convergence receipt.

One post-push observation found CI, Release, and PyPI in progress:
- https://github.com/btakita/agent-doc/actions/runs/34439180076
- https://github.com/btakita/agent-doc/actions/runs/34439181625
- https://github.com/btakita/agent-doc/actions/runs/34439181602

The implementation and local installation are complete; remote publication is
not claimed complete while those jobs run.
