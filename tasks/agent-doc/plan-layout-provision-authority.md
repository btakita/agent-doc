# Layout provisioning authority

Invariant: restoring editor-visible panes never requires dispatching queued work; focus-only observations cannot allocate or promote panes. Run Agent Doc dispatches its selected document after layout convergence, without dispatching sibling documents.

Policy owner: controller pane-layout graph owns structural intent and its generation; the owning project controller owns each document's pane; route-owned start admission owns queue-pause fencing.

Transition table:
- Focus-only + missing/stashed pane: observe/refuse structural change.
- Layout provisioning + live owner: return that owner without selecting/promoting it.
- Layout provisioning + missing owner: provision through the owning controller, with LayoutProvision purpose, then return the binding.
- Editor route: provision exact visible layout, await convergence, dispatch selected document once.
- Manual layout + paused queue: provision only; retain queue pause and suppress dispatch.
- Superseded layout: retain existing generation fence and reject stale completion.

Evidence inputs: actor/registry/live pane ownership, document session identity, desired layout generation, typed start purpose and queue control.

Reactive topology: existing ProcessScope desired-layout Source → layout Computed → structural Effect → generation-fenced pane assignment/observation Sources. Cross-root provisioning is an RPC boundary inside this Effect, returning a receipt to the same graph. Focus observations remain on their existing non-structural lane.

Imperative extraction audit: remove the focus-only API call from structural provisioning and remove sibling dispatch from editor-route layout. Existing pure start-admission decisions remain transaction-cut checks at the actorless launcher boundary; child startup rechecks queue pause. No new cache, polling loop, or graph is added.

Allowed edit surfaces: controller RPC/policy and tests, CLI sync adapter, route startup policy and tests, specs and version/install surfaces.

Verification: regressions for real production payload policy, provision-only early return, paused queue startup, selected-route layout semantics, full make check and tmux-ci; installed live two-column convergence and preserved queue pause.

Out of scope: changing the paused project's work instructions or queue authorization; changing focus-only semantics.

## Database maintenance steering

The operator additionally requested database cleanup. Direct read-only measurement found 41 MB, 10,744 state events, and 20,189 audit recovery markers; the controller's 962,594 count was a stale cached observation. Existing retention already bounds audit markers and superseded document facts. Add `gc --database-only` to expose that existing maintenance separately from orphan-file and actor cleanup, with before/after usage and a read-only dry run. Storage retains policy ownership. This is an explicit one-shot maintenance boundary, not live coordination; no new retention policy, cache, or lifecycle graph is needed. Verify a dry run leaves bytes unchanged, a missing DB stays missing, and the real CLI prunes seeded obsolete history while preserving current facts and files.

## Verification repair

The full suite exposed a scheduling assumption in `retained_resume_reacts_when_the_intent_arrives_after_delivery`: a native-save worker can own the current Effect drain when the test publishes its Source. Immediate map inspection then races receipt publication. Both retained-resume ordering tests now subscribe to the existing captured-finalize wake channel with a bounded timeout before asserting the exact wake. This checks the actual Effect receipt without sleeps or production changes.

## Live startup follow-up

The operator saw JSON replay followed by pane loss during installation. The resumed harness displays historical Search Console JSON tool output; direct recovery in retained diagnostic pane %184 proved a healthy idle composer, with queue pause unchanged. Ops prove three replacements (%181 → %182 → %183), not a database exhaustion error.

A missing receipt edge was identified in cross-controller provisioning: the requesting structural Effect retained thread-local tmux snapshots from before the remote controller created the pane. Its returned binding was then checked against the old missing-pane observation. `ensure_layout_document_pane` now publishes remote mutation completion (including failures after possible mutation) to the existing observation scopes: invalidate structural snapshots and refresh process-owner Sources. No extra lifecycle graph or timer. An isolated tmux regression creates a pane from another thread, proves the cached false-negative, and verifies the receipt exposes the live pane.

Initial full verification passed 9,278 workspace tests, native editor checks, tmux-ci and paused-queue regression; complete install-full succeeded for CLI, library, and both editor plugins. Repeating required checks and iteration install after the live-discovered receipt fix.

## Proven pane removal and cleanup authority

The retained diagnostic owner was later removed during handoff. A short tmux server trace proved an actual `kill-pane -t %185` at 23:24:52 UTC from a client launched in the workspace root. Automatic stash cleanup permitted a live Agent process to be killed after absent registry/supervisor observations; its comment promised only idle-shell deletion while its match also included Agent. This is a separate authority defect, not evidence of a memory crash. Cleanup now preserves live Agent processes in both stash and general orphan passes, even without ownership metadata. The general orphan pass requires retained-dead proof. Existing dead-pane cleanup stays covered.

Policy extraction: remove the derived implication “no registry/supervisor owner → safe to kill” from the existing cleanup Effect. Its scoped tmux observation supplies process kind and dead state; no new graph, persisted claim, polling loop, or independent owner election is added. Regression tests use an actual live agent-named process with no metadata in both stash and ordinary windows. Allowed surfaces expand to resync cleanup and its documented contract.

Tmux diagnostic logging was stopped; sensitive environment entries in the private trace were redacted. No trace/transcript content is staged for publication.

## Installed live proof

Final iteration install succeeded (artifact `tart-ddefaad34b`), and installed CLI/shared library match `target/local-install/release-local/` byte for byte. Root ops at 23:29:12 and 23:29:13 UTC exercised `stash_cleanup_preserved_live_agent pane=%189 reason=missing_ownership_is_not_exit_proof`. A subsequent controller `sync_tmux_layout` returned `observed_convergence` with `%177` + `%189`, `routes_created_panes=false`; the existing owner was restored rather than replaced. The private large tracing files and exited duplicate diagnostic launcher were removed after saving a concise sanitized proof.

## Restart continuation requirement

The operator requires a restarted running turn to continue. Recent ops contain no Restart Agent menu/IPC request: the 19:27 and 19:29 local events were supervisor-only exec handoffs, both adopting Codex PID 1150890. The conversation recorded task completion at 19:32 local. Existing lifecycle policy preserves a surviving child without duplicate dispatch, and re-triggers an open, unconsumed turn checkpoint if the child died during handoff; the explicit child-replacement run loop also arms its next launch trigger. Verify these existing restart regressions in addition to fixing the cleanup path that bypassed normal restart recovery. Do not force-restart a surviving turn to demonstrate the behavior.

## Final verification

Source commit: `1834ccda6` (v0.35.355). Final full `make check tmux-ci` plus 8 stash/dead-pane regressions, paused-queue layout, and remote-pane snapshot regression passed (`tart-2d24ae6571`): 9,279 workspace tests passed, 258 intentionally skipped, native editor checks passed. Restart-specific existing tests also passed (`tart-81be5e1142`: boot continuation matrix plus 4 restart IPC cases). Installed CLI/shared library match the final local build. Privacy scan and whitespace checks passed.

Database-only cleanup ran on the live project: 42,369,024 allocated bytes, 4,014,080 free bytes, 10,748 state events and 20,190 audit markers; no additional eligible rows or reclaimable pages under the existing threshold. Queue control and its reason matched the before-state exactly.
