# Clear proof and supervisor recycle scope

Invariant: clear success requires an observable submitted/cleared pane; a document
route waits only for that document's supervisor replacement.

Policy owner: context-clear submission projection in turn-executor-tmux, and the
controller's existing document projection graph for supervisor recycle state.

Transition table: blank/unrendered capture -> await rendering without resending;
visible clear draft -> bounded submit-key repair; observed ready transition ->
accepted; deadline/capture failure -> explicit unproven result. Recycle requested
-> dispatchable; own supervisor started -> wait; own settled -> dispatchable;
sibling started/settled -> no change to this document; stale epoch -> ignored.

Evidence inputs: fresh pane capture and prompt readiness, elapsed observation
budget, pre-delivery hash; canonical document identity and typed recycle events.

Reactive topology: existing process-owned keyed document Sources -> keyed recycle
Computed -> condition-variable subscriber. State-event ingress updates the graph
and wakes subscribers; SQLite remains the event sink. Clear observation remains
a bounded external tmux capture adapter (tmux supplies no semantic clear receipt),
with submission decisions derived from observed input state; no internal RPC poll
loop or new graph island.

Imperative extraction audit: remove project-global recycle selection from all
file-scoped clients and derive the file recycle projection from existing Sources.
Keep the actorless CLI capture loop only at the external terminal boundary; a
blank repaint must not consume the stuck-draft retry budget or prove acceptance.

Allowed edit surfaces: controller recycle RPC/projection and tests; clear proof
policy and adapter/tests; routing/session specs and release metadata.

Verification: sibling isolation and own settlement wake-up, stale epoch handling,
delayed blank repaint followed by ready evidence, persistent blank/visible draft
fail-closed cases; full make check, tmux-ci, install and live read-only state checks.

Out of scope: clearing another active conversation for testing, replaying work
into the current pane, replacing editor authority, global timeout increases.

## Implementation and evidence

- `request_supervisor_recycle_for_file` preserves the file through all five RPCs;
  ingress canonicalizes it and keys lifecycle events by document hash. The
  existing process-owned document SourceMap feeds a new recycle ComputedMap.
  Own settlement wakes the existing condition-variable waiter; a sibling or
  legacy global flag cannot change this route's readiness.
- Clear's actorless tmux observation adapter retains the short stuck-draft
  budget, allows ten seconds for a blank repaint, and reports `Unrendered`
  without a duplicate command if no frame appears. Both transition proof and
  cleared-history proof reuse the existing idle/busy pane classifier.
- Regression `file_recycle_events_isolate_siblings_and_wake_own_waiter` covers
  observation before the first event, sibling start/settlement, own waiting and
  settlement, and a legacy global in-flight latch. The isolated tmux clear
  fixture erases its screen for 1.2 seconds before printing its prompt.
- Initial full check passed all 9,267 Rust tests but caught a missing release
  parity row. After adding the row, `make check` passed, including native editor
  peers, Lean, and TLA. The tmux suite passed. Final verification also covers the
  shared idle/busy-classifier adjustment and the fixture's elapsed-time assertion.
- Final `make check tmux-ci` passed: 9,267 tests, 256 skipped, native editor peers,
  editor-parity audit, Lean/TLA gates, and all 21 selected live tmux tests. Artifact:
  `src/agent-doc/.tsift/artifacts/tart-ef9994c25f.test.log`.

The reported SDK pane is active on another task; live proof remains read-only.
No user conversation is cleared merely to validate this fix.

## Added operator request: three-pane layout

Live evidence: at 19:54:54 the layout converged to `%159` and `%66`. At
19:54:56 an older focus request resumed a missing actor into `%174`, leaving
three panes. The editor-surface `Focus` handler still selected `ResumeLatest`,
and the async focus handler discarded `no_promotion` while its final effect
could promote a stashed pane.

Invariant: editor focus is a selection-only operation; only the retained layout
graph may provision or place panes. Policy owner: shared controller focus policy
and final focus effect. Observe-only / no-promotion -> select a visible owner or
return a structural-repair receipt; explicit resume permission -> existing
operator recovery behavior. Missing, hidden, and stale targets cannot grow the
visible window through focus.

Reactive topology: retain editor Source -> Focus/Sync Computed -> existing
controller effects. Remove structural work from Focus rather than introducing
another layout owner. The surface/layout worker already owns generation-fenced
structural convergence. Verify that missing and hidden observe-only focus never
calls provisioning/promotion, then reconcile the live two-column layout without
terminating any session.

## Final verification

After adding the layout fix, `make check tmux-ci` passed: 9,269 workspace tests,
256 intentionally skipped tests, both native editor peers, editor parity,
formal checks, and 21 isolated tmux tests. The retained test artifact is
`src/agent-doc/.tsift/artifacts/tart-c73f5f0748.test.log`.

The live controller demonstrated scope isolation before final installation:
legacy project recycle epoch 3385 remained `in_flight` (`auto_install_reexec`),
while this document independently reported epoch 7877 `settled`
(`watch_loop_started`). The live `agent-doc` window returned to two panes,
`%177` and `%66`; no unrelated conversation was cleared to obtain this proof.

`make install-full` completed successfully for 0.35.353; artifact
`src/agent-doc/.tsift/artifacts/tart-a2fc8dbea1.log` records the release install,
JetBrains 0.2.376 and VS Code 0.2.70 package installs, and native reload fanout.
SHA-256 comparisons proved the installed executable and native library match
the release artifacts byte-for-byte. Controller handoffs completed for the
workspace and SDK project; both running controller inodes match the installed
binary. The workspace controller reported document recycle epoch 7890 settled.
The final live window again had exactly two panes (`%177`, `%78`) after further
editor document switches. Native reload reached one of two known editor
endpoints; installed artifacts and both relevant controllers are current.

The latest existing CI run was green. The existing v0.35.352 PyPI upload failed
with the established project-storage quota error; retain the existing quota
review for v0.35.353. Publishing proceeds via the release tag after document
closeout, with external CI observed once rather than awaited.
