# Controller startup layout authority

Invariant: remembered layout columns never authorize tmux placement or focus after controller startup; a live editor publication or explicit layout command must establish current intent.

Policy owner: ControllerRuntime initialization and its ProcessScope ControllerPaneLayoutGraph.

Transition table:
- Cold startup with absent or saved columns: no desired generation and no structural/focus effect.
- Live layout publication before sink installation: retain intent, then execute when sink readiness arrives.
- Live publication after sink installation: execute the current generation.
- Repeated background publication: coalesce; explicit identical foreground intent: allocate a fresh generation.
- Actor updates without a live desired layout: no layout effect.
- Controller restart: require live intent again; retain saved columns for explicit layout recall.

Evidence inputs: live logs show the nested project controller repeatedly applying a singleton saved layout at generation 1 with focus_not_requested, displacing the workspace's two-document layout even while the queue is paused.

Reactive topology: live editor/command ingress → desired Source; actor Sources and effect receipts → existing applicability Computeds → retained layout Effect → generation-fenced receipt Source. Sink readiness remains a Source. Cold storage remains layout recall data, not an action Source.

Imperative extraction audit: remove the startup SQLite-to-desired conversion that manufactured authority. Existing applicability, actor bindings, and receipt matching already belong to Computeds. No polling or alternate ownership mechanism is added.

Allowed edit surfaces: controller runtime/graph constructor and regression tests; routing specification; generated release versions and release notes.

Verification: reproduce saved-layout startup mutation before the fix; prove startup and actor changes emit no effect, fresh identical live layout still runs, and late sink installation still executes live intent. Run full make check and tmux-ci; build/install and inspect live controller handoff/layout evidence.

Out of scope: queue pause semantics, editor restart, credential changes, and deleting layout history.

## Implementation and verification

Removed the saved-columns constructor input and startup storage load. The existing desired Source now starts absent, with no published generation. Live publication owns the first generation; sink readiness still activates intent that arrived before the sink.

The regression failed before the change with `saved columns must not move or focus panes at startup`. It passes afterward, including actor hydration, live publication, persistence retention, and restart. The late-sink test now supplies actual live intent before binding the sink.

Source commit: `7fca2ff8f` (v0.35.356). `make check tmux-ci` exited 0: 9,280 workspace tests passed, 258 skipped, plus editor/native, parity, simulator, formal, and 21 isolated tmux tests. The controller package passed all 410 tests under nextest.

`make install-full` exited 0 and installed the CLI, native library, and both editor packages. Both the workspace and affected nested-project controllers completed stable handoffs to 0.35.356 with running inodes matching the installed executable. During the nested-project recycle, window 0 retained panes `%177` and `%6` with `%6` active. No new pane-layout effect appeared in that project's log; its old singleton `%189` layout was not replayed. This verifies the reported startup/recycle mechanism without moving the operator's focus for the test.

Lazily state remains in ProcessScope: desired and observation Sources, actor/receipt/applicability Computeds, and the retained tmux Effect with generation-fenced receipt Sources. No independent cache, caller-owned recomputation, or new polling loop was introduced.
