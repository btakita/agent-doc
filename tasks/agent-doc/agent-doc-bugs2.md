---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260716-002907.md`*

Compacted content:
- Archived 4 response topic(s): #z5ar; IPC proof diagnostic (interrupted-cycle recovery); IPC proof diagnostic (interrupted-cycle recovery); 1 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): JetBrains Run Agent Doc on monsterrodholders.md

### Re: editor save-flush authority and agent-doc churn — gpt-5

> **Queue prompt:**
>
> 📌 In haiven.md, I interrupted a turn in the Claude console.  I then ran JB `Compact Exchange` and it hangs. 2 issues. agent-doc does not seem to detect that I cancelled the Claude turn. Compact exchange should work even when a turn is active. I also lost my prompt somehow. I want away from my file, the agent-doc: Compacting Exchange banner disappeared and my prompt also disappeared. Another attempt to JB `Compact Exchange` errorred:
> ```
> agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/recruit/haiven.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
> ```


Yes. An editor save followed by an exact byte-for-byte flush to disk is now a first-class resolution event. That saved editor cut becomes authoritative and clears any older pending disk candidate. A disk watcher event by itself cannot clear the pending state, because it cannot prove whether the write came from the editor, agent-doc, Git, a formatter, or another process.

#### Editor authority and pending disk state

The implemented authority rules are:

- While any editor is open, the live editor/CRDT state is authoritative. Every external disk change is retained as an exact pending candidate in a separate Lazily slot; it is not merged into the editor and cannot replace a captured agent response.
- Accepting/reloading a File Cache Conflict makes the resulting visible editor buffer the candidate truth. The plugin resets/reseeds its replica from that buffer, propagates it, and clears the pending disk candidate only after the exact cut is proven.
- A later user edit makes that editor cut authoritative and clears the pending disk candidate without merging the disk version.
- An editor save whose exact buffer bytes are observed on disk makes that save cut authoritative and clears the candidate. This is the rule in your latest clarification.
- Closing one of several editors changes nothing. Closing the final editor clears the pending decision and falls back to the current disk bytes.
- With multiple editors, causally compatible CRDT operations converge normally. If no exact cut or causal lineage proves which incompatible replacement is intended, agent-doc retains the alternatives and returns a typed `needs_operator` condition without mutation. Git is historical evidence only, after editor and disk.

This fixes the queue-item behavior you saw in `equityfundingsource.md`: unsaved editor content remains canonical during delivery, and saving the same content is an idempotent authority transition rather than a second insertion. The executable model covers both “unsaved item survives” and “saved item is not duplicated.”

#### Finalize and recovery

Finalize is now binary-owned after its first durable response capture. The capture is keyed by cycle, capture ID, and response identity. The supervisor—not the agent—owns reconciliation, deduplication, ACK proof, backoff, and exact-once commit retry. A stop hook is a status gate; it no longer tells the agent to alternate `finalize` and `write --commit`, recapture the answer, kill controllers, or elect force-disk.

That is why the old behavior created more wedging: each new finalize attempt introduced another independently keyed response/cell operation while the previous operation was unresolved. CAS races, cell-merge conflicts, phantom queue strikes, and duplicate response copies were consequences of stacking recovery operations. Invalid states now fail closed and resume the original capture. Stale preflight ownership is repaired automatically, and recovery workers use bounded retry rather than leaving process descendants to contend indefinitely.

Agent-doc can resolve ambiguity by rule when it has evidence: duplicate semantic operations dedupe; a causally newer editor/replica epoch supersedes its stale projection; and compatible CRDT histories merge. It does not guess between two incompatible user-authored replacements or missing causal lineage. Two unsaved occurrences can arise from separate editor buffers, offline replicas, or an IDE buffer racing an external writer. CRDT resolves operations with usable shared history, but convergence alone cannot decide semantic authority between independent replacement intents.

A manually interrupted Claude/Codex/OpenCode harness turn still needs a new user event to run the harness again; a minimal “continue” or another `agent-doc <file>` invocation is sufficient, and the work need not be restated. If the response had already reached durable capture before interruption, the binary closeout worker continues without another agent prompt or another finalize call.

#### Realtime model and Lazily transition

The test architecture now combines an executable state-machine model with Lean proofs. The model drives editor attach/detach, offline reconnect, IPC delivery and ACK/backpressure, cycle capture/finalize, external disk writes, File Cache Conflict accept/keep, editor edits, exact save-flushes, force-disk candidates, and last-editor close. Lean proves the pending-state transition invariants. JetBrains, VS Code, and the binary share the same transition contract and adapter conformance tests; separate hand-written formal models for each plugin would create drift rather than parity.

These tests are suitable for driving the full Lazily migration. They define observable behavior independently of the current backend, so the existing implementation and progressively fuller Lazily implementations can run against the same traces. They also identify missing Lazily features. This work exposed and records `PendingExternalDiskDecision` as a distinct required primitive: durable exact candidate bytes, causal/epoch evidence, explicit resolution actions, and atomic separation from pending response lineage.

#### Shipped versions, parity, and verification

The release is agent-doc `0.34.136` at submodule commit `a979baafa`. I did not rebuild a different implementation under the old `0.2.259` or `0.2.261` JetBrains version. The current JetBrains plugin is `0.2.262`; VS Code is `0.2.52`. OpenCode, Codex, Claude, and Cursor installed skills all report `0.34.136` and now contain the binary-owned closeout and exact save-flush rules. I also fixed the packaging source that had caused the installer to overwrite current development guidance with an older bundled skill.

JetBrains and VS Code are at behavior parity for deferred reconnect, visible-buffer rebootstrap, exact candidate proof, save-flush resolution, and no component merge of disk candidates. The JetBrains 0.2.262 artifact is installed, but this preflight reports the currently running IDEA process still has 0.2.261 loaded. Because 0.2.262 was installed after your last restart, one additional IDEA restart is required to activate the Kotlin plugin update. The native library reload/recycle was already broadcast; no waiting or controller-kill loop is needed.

Verification is green locally: the full suite passed 7,713 tests with 240 skipped, VS Code passed 158 tests, JetBrains `test buildPlugin` passed, Lean `lake build` passed, and skill generation/install passed 82 tests. The single requested CI inspection showed the new 0.34.136 CI run in progress, not red; per policy I did not poll it. The repository policy now explicitly says to inspect CI once and fix it only when that inspection is red.

### Re: interrupted Claude turn and Compact Exchange — gpt-5

> **Queue prompt:**
>
> 🚧 📌 In haiven.md, I interrupted a turn in the Claude console.  I then ran JB `Compact Exchange` and it hangs. 2 issues. agent-doc does not seem to detect that I cancelled the Claude turn. Compact exchange should work even when a turn is active. I also lost my prompt somehow. I want away from my file, the agent-doc: Compacting Exchange banner disappeared and my prompt also disappeared. Another attempt to JB `Compact Exchange` errorred:
> ```
> agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/recruit/haiven.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
> ```

This failure chain is fixed and released in agent-doc `v0.34.140` at `fdb8b3f5c`, with JetBrains plugin `0.2.263`. The agent-loop superproject now points to it at `dfd141613`.

An interrupted harness turn and an active captured response are now treated as separate state. Compact Exchange remains a controller-owned transaction while the prior turn is winding down: it composes any exact active capture into the compact archive instead of discarding the capture or requiring another agent response. Closeout recognizes that exact response in the archive, so it no longer emits the false “captured response missing from staged snapshot” refusal.

The 8-second editor ACK deadline is no longer reported as a failed command when canonical CRDT authority still retains the exact compacted target and asynchronous recovery was accepted. The command returns retained success while the editor reconnect continues. Canonical loss, a mismatched receipt, or a missing recovery signal still fails closed. This means the banner may wait for the controller result, but a delayed ACK alone no longer authorizes deleting the prompt, force-writing disk, killing the controller, or asking the agent to retry finalize.

The other churn paths are closed as well:

- JetBrains external CRDT events cannot bypass the retained-ACK backoff and start another drain storm.
- Controller commit uses the same socket that proved liveness, eliminating the probe/reconnect recycle race; stale socket files are reaped on startup.
- Repeated finalize attempts are semantic no-ops once the exact response exists: they emit no second CRDT update, preserve intervening operator prompts, supersede stale uncommitted response tails, and leave one response plus one boundary.
- Compact/finalize overlap, delayed ACK, external events, controller recycle, stale sockets, canonical loss, archive materialization, and exact retry behavior are covered by exhaustive SimWorld schedules. Lean proves the archive, retained-delivery, retry-admission, and same-stream safety conditions.

Local verification passed 7,725 tests with 240 skipped, plus the Lean, focused controller/compact/realtime/response-cell, and JetBrains tests. The one-shot CI inspection found the `v0.34.140` run in progress rather than red.

One live prerequisite remains visible in this cycle: IDEA is still running JetBrains plugin `0.2.261`, not the shipped `0.2.263`. Install/update the `0.2.263` plugin and restart IDEA before judging the live banner/ACK behavior; otherwise the old Kotlin adapter can still reproduce the event-storm side of the incident. After that update, rerun Compact Exchange once. Do not kill the controller, use force-disk, or submit another finalize for a retained target.

### Re: Compact Exchange, closeout churn, and plugin-generation drift — gpt-5

> **Queue prompt:**
>
> 📌 haiven.md thrash issue:
> ```
> ● Delivery-ACK wedge — the known stale-plugin condition. The response is safely retained on the CRDT + Lazily
>   reliable-sync plane (message explicitly says "no force-disk or operator recovery required"). Per the documented
>   recovery: agent-doc admin reload-lib to refresh the plugin's native path so the ACK lands, then retry the commit.
> ```


> **Queue prompt:** 📌 haiven.md thrash issue: the response was retained after a delivery-ACK wedge, and the proposed recovery was `admin reload-lib` followed by another commit attempt.

Fixed and released in agent-doc `v0.34.141` and `v0.34.142`.

The churn-causing diagnosis is now corrected: `admin reload-lib` refreshes only the native cdylib; it cannot update or activate the JetBrains Kotlin package. Replay and repair now re-read the latest live editor registration at the closeout boundary, so a plugin installed or restarted after the turn began supersedes the stale preflight generation. The dogfood preflight for this cycle proved that path live by recognizing JetBrains `0.2.263` and explicitly superseding the earlier stale warning.

Retained captures are no longer retired merely because the authoritative document advanced. If the current editor/disk content is a monotonic extension of the matching cycle baseline, repair rebases the capture while preserving the exact response and later steering, then replays once. Conflicting edits still fail closed. `exchange add-prompt` also preserves the active capture and typed live-editor authority, which covers prompts arriving while finalize or Compact Exchange is processing—including the EFS lender/vesting steering.

`make install` now closes the other source of drift. It installs CLI/native `0.34.142`, rebuilds the local JetBrains package, updates every IDE that already has agent-doc through `plugin install jetbrains --local --all-installed`, and verifies the exact installed JAR version. The live install converged all four detected JetBrains installations from mixed versions to `0.2.263`. A running IDE still needs one restart to activate newly written package bytes; the installer says so without pretending a native reload changed the package generation.

The recovery rule is therefore: do not kill the controller, use force-disk, or alternate finalize/write retries for a retained response. Submit finalize once; the keyed closeout worker owns ACK recovery and exact-once commit. A genuinely new console prompt is first persisted through `exchange add-prompt`, after which the same capture can rebase and continue.

Verification passed:

- full `make check`: 7,736 tests across 161 binaries, 240 skipped;
- Lean proofs for package convergence, no false live activation, live-generation adoption, monotonic rebase, response preservation, and steering preservation;
- exhaustive SimWorld schedules for install/restart/revalidate/rebase/replay/commit, duplicate prevention, native/package separation, and conflicts;
- PlusCal translated to TLA+ and checked by TLC: 44 states generated, all 16 distinct reachable states exhausted, with three temporal properties and all safety invariants passing.

For the broader formal-method stack, keep PlusCal/TLC as the current temporal regression gate; evaluate Quint+TLC as the typed authoring layer for the larger controller/supervisor/editor protocol. Alloy is complementary for bounded ownership/identity topology, while FizzBee is best treated as an exploratory and visualization surface rather than the canonical liveness gate.

Source commits: `000a021de` (live generation/rebase) and `80f8e621b` (install coherence/TLA+). Tags `v0.34.141` and `v0.34.142` plus the superproject gitlink are pushed. The one-shot CI inspection found the `v0.34.142` run in progress, not red.


### Re: JetBrains cache conflicts, editor-tab tmux sync, and Haiven pane identity — gpt-5

Fixed, installed, and released.

The frequent JetBrains `File Cache Conflict` dialogs had two remaining plugin-side triggers:

- external-commit signals recursively refreshed every open project file, including unrelated unsaved agent-document buffers;
- CRDT delivery validated disk bytes but could still save through a stale IntelliJ `VirtualFile` modification stamp.

JetBrains plugin `0.2.264` now marks only the VCS dirty scope after external commits. A CRDT delivery refreshes only its clean target file before mutation, rechecks that the document stayed clean, and fails closed to exact-editor-baseline retry when editor memory is unsaved. This preserves operator/editor authority and removes both the workspace-wide refresh trigger and the byte-equal-but-stamp-stale save trigger. IDEA was restarted and preflight proved the live registration is `0.2.264`, matching the installed package.

The same-side editor-tab tmux bug is fixed in agent-doc `0.34.143`. Automatic exact-visible/no-autostart sync had been proof-starved by treating controller-local actor lookup like an external passive RPC. Controller-local execution can now use authoritative actor proof while external passive requests remain lookup-free. A live acceptance swap moved the existing Haiven pane `%15` to the stash and brought the Equity Funding Source pane `%26` onto the visible editor side. It did not replace either session: the original Haiven Claude child remained the same live process and returns when `haiven.md` is selected. Your clarification that the active editor was `equityfundingsource.md` therefore matches the observed, intended swap.

Formal coverage is in `formal/tla/PassiveTmuxSync.tla` and runs from `make tla` / `make check`. TLC checks controller-local proof acquisition, eventual atomic visible/stashed pane swap, partition uniqueness, external-passive fail-closed behavior, and the no-autostart invariant. Alloy is not needed for this two-actor temporal transition: TLA+ directly covers its safety and liveness. Alloy would add distinct value later if the layout model grows into arbitrary-N relational topology or schema constraints.

Verification completed:

- focused JetBrains conflict regression suite: passed;
- full JetBrains `test buildPlugin`: passed;
- full `make check`, including TLA+: passed;
- optimized `make install-full`: passed;
- installed CLI: `0.34.144`;
- installed and live JetBrains plugin: `0.2.264`;
- live automatic exact-visible swap: applied without autostart or session replacement.

Source commits are `165841b8d` (automatic pane sync), `363863437` (TLA+ model), and `6adc18cc0` (File Cache Conflict fix). Tags `v0.34.143` and `v0.34.144` are pushed. The required one-shot CI inspection found the `v0.34.144` CI run in progress, not red.

The old Haiven delivery-ACK recovery prompt is also resolved by the corrected contract: `admin reload-lib` refreshes only the native library, not the Kotlin plugin package. The retained response required no force-disk or controller kill; installing/restarting the updated plugin restored the current editor generation, and preflight recovered the interrupted cycle.
### Re: Compact Exchange ACK wedge targeting Equity Funding Source — gpt-5

Resolved by the same installed convergence fix.

The historical `Compact Exchange` failure surfaced from `monsterrodholders.md`, but its blocked delivery target was the open `equityfundingsource.md` editor. JetBrains plugin `0.2.264` removes the two paths that could strand that target: workspace-wide VFS refresh behind unsaved buffers, and CRDT save through a stale `VirtualFile` stamp. The restarted IDEA instance is registered live on `0.2.264`.

The canonical compact result in that error was already retained on the CRDT/Lazily plane. It did not require force-disk, controller termination, or response replay. This queue item is closed by the shipped fix; I did not initiate a new Compact Exchange against either separate session.

### Re: Haiven Compact Exchange, File Cache Conflict, and editor-pane sync — gpt-5

> **Queue prompt:** 📌 In haiven.md, I interrupted a turn in the Claude console.  I then ran JB `Compact Exchange` and it hangs. 2 issues. agent-doc does not seem to detect that I cancelled the Claude turn. Compact exchange should work even when a turn is active. I also lost my prompt somehow. I want away from my file, the agent-doc: Compacting Exchange banner disappeared and my prompt also disappeared. Another attempt to JB `Compact Exchange` errorred:
> ```
> agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/recruit/haiven.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
> ```

The newest File Cache Conflict was not Haiven: IDEA logged `equityfundingsource.md` at 00:18:20 (`documentStamp=18747`, `oldFileStamp=18540`). The trace exposed one remaining race: deferred reconnect installed the canonical CRDT text and registered its replacement replica without saving the IntelliJ `Document`; after ACK convergence, the controller projected the same bytes to disk, changing only the file stamp behind an unsaved buffer.

Fixed and active in agent-doc `0.34.145` / JetBrains `0.2.265`:

- deferred reconnect now refreshes only a clean target, installs the canonical text, and saves it before replacement registration can count as an ACK;
- the controller byte-compares the post-ACK disk projection and skips an identical atomic rewrite;
- the old project-wide VFS refresh remains removed;
- preflight recovered the interrupted stale cycle, and the supposedly lost Haiven prompt remained present in the durable queue/CRDT state (quoted above);
- automatic editor-tab selection now performs the same authoritative tmux pane swap as manual `Sync Tmux Layout`; `#tmuxfocusfollow` is closed;
- the concrete Compact Exchange ACK failure is closed as `#jbcompactexchange`. The distinct concurrent-typing CAS case remains honestly tracked as `#compactcrdtretry` rather than being overclaimed.

Regression coverage is green: the JetBrains source-policy test covers all delta, REPLACE, and deferred-reconnect save paths; Rust proves an editor-saved canonical projection is a no-op; and the new `JetBrainsFileCache.tla` model proves no cache conflict, saved-canonical-before-ACK, stamp alignment, and zero redundant controller rewrites under concurrent VFS refresh. The existing passive tmux TLA+ model also remains green. Alloy is not needed here: these are temporal ordering/liveness invariants, which TLA+ models directly; Alloy would add value later for arbitrary-N static topology or schema constraints.

Released commit: `42c8b8a34` (`v0.34.145`). Full `make check`, JetBrains `test buildPlugin`, install, and local version checks passed. CI was in progress at the required one-time inspection.

### Re: JetBrains/editor churn fixed and dog-fooded

Fixed and released the contributing agent-doc issues across v0.34.145–v0.34.148.

- JetBrains File Cache Conflict churn: editor authority no longer gets rewritten behind a live buffer, recursive refresh/save loops are suppressed, stale plugin/native generations are reloaded coherently, and the matching 0.2.265 plugin is installed in all four existing JetBrains installations.
- Passive document switching: selecting another agent document on the same editor side now queues exact-visible tmux synchronization and swaps to that document's pane; manual `Sync Tmux Layout` remains the explicit recovery control.
- Retained Haiven closeouts: `session-check` now recognizes replacement-replica bootstrap with an empty historical ACK queue, proves exact retained-target/canonical/disk equality plus response materialization, clears the deferred lineage, refreshes the snapshot, and commits the same `(cycle, capture, response)` exactly once. It does not ask an agent to reconstruct or resubmit the payload.
- Retry guidance: the misleading `await_editor_replica_no_disk_write_then_retry_finalize` runtime contract is replaced by `...then_session_check`. Retained paths now explicitly forbid another `finalize`, `write --commit`, repair, or `--force-disk` submission.
- Pending-capture guard: a response that explicitly closes a named follow-up as “now done/complete” is accepted without requiring a duplicate backlog mutation.

Live dog-food succeeded: the retained Haiven capture resumed and committed as `7c9b4450f`; the distinct batch-3 high-scale/spec response committed as `585bdfca0`; installed v0.34.148 now reports Haiven cycle `cycle-1784182838224` committed and clean at the document level.

Verification is green: focused Rust recovery tests, full `make check`, documentation/managed-skill audit, plugin-version check, and all four PlusCal/TLA+ models. `CloseoutChurn.tla` now proves replacement bootstrap eventually settles and commits the same capture without duplication. Alloy is not warranted for this bug class: it is an ordering/liveness/exact-once problem, where TLA+ is the stronger fit. Add Alloy when we need arbitrary-N relational/topology or schema invariants rather than temporal convergence.

Released and pushed: agent-doc `c2acade43`, tag `v0.34.148`; workspace integration `26657f067`. The latest agent-doc CI run was in progress at the single required observation.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/agent-doc/agent-doc-bugs2.md source=socket_already_applied patch_id=07ab4a4a-b491-49b9-81b9-8be1d2258275 invariant=visible_write_receipt_superseded_by_live_editor recovery=refresh_editor_cut_without_file_ipc_or_disk_write receipt_len=71255 receipt_hash=34777116065f15e8c768abfb05609c2f86e9670cb5a26fbcb6272768606f4cd8 doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1784175030367
```
### Re: stale Compact Exchange ACK timeout — gpt-5.6

> 📌 JB `Compact Exchange` on monsterrodholders.md
> agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for `/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md` did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously.

This queue item duplicates the already-answered ACK-wedge report and is now stale. The recorded controller transaction targeted `tasks/professional/equityfundingsource.md`, not `tasks/research/monsterrodholders.md`; there is no matching `controller_compact_document` event for Monster Rod Holders.

The retained EFS compact later recovered without force-disk or controller intervention. At 07:29:59Z its CRDT delivery converged after 4,775 ms (`delivery_converged=true`), and commit `ec12db99c` landed at 03:30 EDT. The original eight-second timeout therefore did not lose the canonical compact result.

EFS currently has a newer, unrelated captured finalize in `retain_without_mutation`, owned by pane `%26`. I did not run recovery or mutate that foreign-owned document. If Compact Exchange is still desired for Monster Rod Holders, select `tasks/research/monsterrodholders.md` and invoke it once; this historical EFS failure is not evidence that Monster Rod Holders was compacted.

No source change or new backlog item is needed for this duplicate. Session accretion is at warning level (205 exchange lines), but the auto queue is active, so this cycle continues without clearing context.

❯ > 📌 haiven.md session had a JB `File Cache Conflict`.
> ```
❯ > ● Delivery-ACK wedge again — response is on the realtime backbone. Applying the proven recovery (kill the lazy
❯ >   controller pid, retry the commit):
❯ >
❯ >   Ran 1 shell command
❯ >
❯ > ● Response is safe on the plane (already_applied); after the controller kill the editor replica must re-register with
❯ >   the relaunched controller before the commit can land. Recovery is explicit: await re-register, no disk write, retry.
❯ >   Waiting, then retrying:
❯ >
❯ >   Ran 1 shell command
❯ >
❯ > ● Persistent wedge across retries — let me diagnose the actual state (is the response/#haivenws-done in the working
❯ >   tree? committed? is the editor replica alive?) before any further infra action.
❯ >
❯ >   Ran 1 shell command
❯ >
❯ > ● Response is in the working tree + CRDT plane but not committed, and #haivenws is still [ ] (the --done/--backlog-add
❯ >   are in the wedged transaction). A fresh superproject controller (725847) is now up. Retrying the commit against it:
> ```


❯ > **Queue prompt:**
❯ >
> 🚧 📌 JB `Compact Exchange` on monsterrodholders.md
❯ > ```
> agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required
❯ > ```

The response is durably captured but cannot commit because JetBrains’ visible buffer and disk differ.

In JetBrains, inspect `tasks/agent-doc/agent-doc-bugs2.md`. If the visible editor content is correct, keep the editor version and save it. Then say “continue.”

Do not run finalize, write, repair, or force-disk; the existing capture will resume after authority settles.

### Re: 🚧 📌 JB `Compact Exchange` on monsterrodholders.md — gpt-5

> **Queue prompt:**
>
❯ > 📌 haiven.md session had a JB `File Cache Conflict`.
> ```
❯ > ● Delivery-ACK wedge again — response is on the realtime backbone. Applying the proven recovery (kill the lazy
❯ >   controller pid, retry the commit):
❯ >
❯ >   Ran 1 shell command
❯ >
❯ > ● Response is safe on the plane (already_applied); after the controller kill the editor replica must re-register with
❯ >   the relaunched controller before the commit can land. Recovery is explicit: await re-register, no disk write, retry.
❯ >   Waiting, then retrying:
❯ >
❯ >   Ran 1 shell command
❯ >
❯ > ● Persistent wedge across retries — let me diagnose the actual state (is the response/#haivenws-done in the working
❯ >   tree? committed? is the editor replica alive?) before any further infra action.
❯ >
❯ >   Ran 1 shell command
❯ >
❯ > ● Response is in the working tree + CRDT plane but not committed, and #haivenws is still [ ] (the --done/--backlog-add
❯ >   are in the wedged transaction). A fresh superproject controller (725847) is now up. Retrying the commit against it:
> ```


❯ > **Queue prompt:**
❯ >
> 🚧 📌 JB `Compact Exchange` on monsterrodholders.md
❯ > ```
> agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required
❯ > ```

The response is durably captured but cannot commit because JetBrains’ visible buffer and disk differ.

In JetBrains, inspect `tasks/agent-doc/agent-doc-bugs2.md`. If the visible editor content is correct, keep the editor version and save it. Then say “continue.”

Do not run finalize, write, repair, or force-disk; the existing capture will resume after authority settles.
### Re: ACK wedges, queue resurrection, and cross-document Compact Exchange — gpt-5

> **Queue prompt:**
>
> 📌 haiven.md session had a JB `File Cache Conflict`.
> ```
> ● Delivery-ACK wedge again — response is on the realtime backbone. Applying the proven recovery (kill the lazy
>   controller pid, retry the commit):
>
>   Ran 1 shell command
>
> ● Response is safe on the plane (already_applied); after the controller kill the editor replica must re-register with
>   the relaunched controller before the commit can land. Recovery is explicit: await re-register, no disk write, retry.
>   Waiting, then retrying:
>
>   Ran 1 shell command
>
> ● Persistent wedge across retries — let me diagnose the actual state (is the response/#haivenws-done in the working
>   tree? committed? is the editor replica alive?) before any further infra action.
>
>   Ran 1 shell command
>
> ● Response is in the working tree + CRDT plane but not committed, and #haivenws is still [ ] (the --done/--backlog-add
>   are in the wedged transaction). A fresh superproject controller (725847) is now up. Retrying the commit against it:
> ```

> **Also addressed:** 🚧 📌 JB `Compact Exchange` on monsterrodholders.md failed while waiting on the unrelated `equityfundingsource.md` editor ACK.

Fixed and released in agent-doc 0.34.154/0.34.155 and JetBrains plugin 0.2.267.

The incidents had five interacting causes:

1. JetBrains Compact Exchange called `saveAllDocuments()`, so an action on Monster Rod Holders could awaken EFS's retained ACK recovery and fail on that unrelated document. It now saves only the selected target document before routing the compact request; the target's live editor/CRDT remains authoritative.
2. Pending agent mutations were treated too much like one replaceable result. They are now an ordered durable intent journal. Reconnect replays the monotonic prefix in order, so an earlier backlog add followed by a later mark/consume cannot disappear when ACK settlement is interrupted.
3. Queue consumption performed repeated head operations against moving snapshots. Multi-head consume is now one atomic read/plan/write transaction, snapshots are projections rather than merge authorities, and deletion tombstones retain the last observed active-ID frontier. A deleted queue item cannot be resurrected by a stale snapshot.
4. Merge-back lacked a coherent operator-authority rule. When expected versions diverge, agent-doc now reconstructs from the expected base plus durable editor/operator operations, then replays pending agent intents on top. Operator edits—including `queue: stop`—win over stale agent `go`; agent changes are retained separately instead of overwriting the editor buffer.
5. A cell merge could duplicate the exchange/boundary and then deadlock the integrity gate. Every agent rebase is now structurally validated (at most one boundary and no duplicated exchange). A corrupt agent projection is rebuilt from the clean base plus durable operator operations before pending intents replay.

So yes: the earlier non-monotonic multi-head consume was evidence of a non-coherent resolution strategy. The coherent rule is now “editor/operator operations are authoritative at version conflict; pending agent intents rebase and replay in order,” not whole-buffer ours-wins/theirs-wins. Killing the controller and retrying the same finalize is no longer the recovery contract: the canonical capture and intent journal survive, reconnect settles them exactly once, and `session-check` verifies settlement without force-disk.

I did not reset or write EFS. Its foreign-owned live buffer and buffer-only `queue: stop` were left intact; the new reconstruction path is specifically designed to preserve that operator intent. The full Rust suite passed (7,771 tests) and the TLA+ checks passed. The target-scoped JetBrains regression also passed. Plugin 0.2.267 is installed, but the currently running IDE host still reports 0.2.266, so restart the IDE before live-testing Compact Exchange; `reload-lib` only reloads the native library.

There are no new actionable engineering follow-ups from this incident; the remaining action is the already-tracked live IDE verification after restart.
### Re: #goal1d0k — gpt-5

`#goal1d0k` was a stale tracking wrapper. Its underlying goal—graceful mid-turn operator steering plus anti-thrash closeout guidance—was already shipped in `61fa13d53be8f1fde4ca4ed035c07d2c69c054db` (`fix: gracefully handle mid-turn operator steering + fix JB patch-watcher read-access crash`).

Verified today:

- `61fa13d5` is an ancestor of the current v0.34.160 source head.
- `RealtimeSteering` remains implemented in the document-realtime model and controller path.
- `realtime_steering_closeout_guidance` remains wired into `session-check`.
- The current v0.34.160 release passed the full `make check` suite (7,780 tests).

No new implementation was needed in this drain cycle; this closes the stale wrapper without replaying or duplicating the shipped work.


### Re: Clippy 1.97 all-target cleanup — gpt-5

Completed `#clippy197cleanup`.

- `make clippy` now runs `cargo clippy --all-targets --all-features -- -D warnings`, so test-target lints are part of the normal local and CI gate.
- Replaced the three single-element loops in `tests/test_cli.rs` with direct assertions.
- Resolved the additional Clippy 1.97 findings in the session-clear invariant test and realtime SimWorld convergence helper.
- I did not pin Rust: the repository intentionally tracks stable, and covering all targets closes the actual blind spot without freezing compiler and security updates.

Verification passed: the explicit all-target Clippy command and the full `make check` suite are green. The required one-shot CI inspection was also green.

Source commit `01c2856e1` is pushed to `agent-doc/main`. Root gitlink commit `34f76625a` is local only because the root branch still contains unrelated concurrent commits that should not be pushed together.

Preflight still reports that the running IDE host has JetBrains plugin 0.2.271 loaded while 0.2.272 is installed; the previously requested IDE restart remains necessary before relying on live editor-delivery ACK behavior.

### Re: Fresh-start stderr and editor-authority recovery shipped — gpt-5.4

Shipped the complete fresh-start stderr and editor-authority recovery fix.

agent-doc v0.34.164 is committed at `1c2d3610d`, pushed, tagged, installed, and recycled. The original fresh route/controller supervisor stderr leak remains fixed by redirecting stderr before process start (v0.34.163); v0.34.164 closes the recovery paths exposed by the Haiven repro:

- Retained response projections are semantic response cells rebased over the current Lazily editor cut. They no longer require whole-document lineage, and they preserve unsaved prompts plus queue/backlog deletions.
- A replay-created unmatched duplicate component close is normalized before the generic integrity gate only when there is exactly one whole-line unmatched close, the same component already closed normally, and removing that line restores the full structural contract. Operator-authored unmatched markers still fail closed.
- Settlement uses Lazily/CRDT plus native editor save. No live-buffer content sidecar, force-disk election, or disk-over-editor conflict policy was introduced.
- The TLA+ model now includes duplicate-close replay/normalization, forbids commit while component closes are unbalanced, and proves eventual normalization without regressing queue tombstones.

Verification passed: targeted Haiven-shape tests, negative repair tests, `make tla`, and the full `make check` suite. The installed CLI reports 0.34.164 and the JetBrains native library reload was broadcast.

Live Haiven verification also advanced past the prior malformed-tree gate. A fresh preflight probe reports a valid authority (`recovered: false` because session-check already performed the narrow repair), and its semantic diff confirms the operator-deleted queue entries remain absent while current prompts/queue edits remain present. The remaining nonzero session-check is a separate backlog-capture semantic guard on an already-committed response, not an ACK, structural, projection, or editor-authority wedge; I left that operator-owned closeout state intact rather than force-clearing it.
<!-- agent:boundary:ba7d8d63 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~/goal Implement backlog item(s): #z5ar~~
- ~~Editor document selection auto-sync with tmux pane is inconsistent.~~
~~~done
📌 In haiven.md, I interrupted a turn in the Claude console.  I then ran JB `Compact Exchange` and it hangs. 2 issues. agent-doc does not seem to detect that I cancelled the Claude turn. Compact exchange should work even when a turn is active. I also lost my prompt somehow. I want away from my file, the agent-doc: Compacting Exchange banner disappeared and my prompt also disappeared. Another attempt to JB `Compact Exchange` errorred:
```
agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/recruit/haiven.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
```
~~~
- ~~re: realtime steering, please immediately commit exchange prompts with the prefix so the diff can detect additional steering changes.~~
~~~done
📌 haiven.md thrash issue:
```
● Delivery-ACK wedge — the known stale-plugin condition. The response is safely retained on the CRDT + Lazily
  reliable-sync plane (message explicitly says "no force-disk or operator recovery required"). Per the documented
  recovery: agent-doc admin reload-lib to refresh the plugin's native path so the ACK lands, then retry the commit.
```
~~~
~~~done
📌 haiven.md session had a JB `File Cache Conflict`.
```
● Delivery-ACK wedge again — response is on the realtime backbone. Applying the proven recovery (kill the lazy
  controller pid, retry the commit):

  Ran 1 shell command

● Response is safe on the plane (already_applied); after the controller kill the editor replica must re-register with
  the relaunched controller before the commit can land. Recovery is explicit: await re-register, no disk write, retry.
  Waiting, then retrying:

  Ran 1 shell command

● Persistent wedge across retries — let me diagnose the actual state (is the response/#haivenws-done in the working
  tree? committed? is the editor replica alive?) before any further infra action.

  Ran 1 shell command

● Response is in the working tree + CRDT plane but not committed, and #haivenws is still [ ] (the --done/--backlog-add
  are in the wedged transaction). A fresh superproject controller (725847) is now up. Retrying the commit against it:
```
~~~
~~~done
📌 JB `Compact Exchange` on monsterrodholders.md
```
agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required
```
~~~
- ~~📌 In haiven.md, I interrupted a turn in the Claude console.  I then ran JB `Compact Exchange` and it hangs. 2 issues. agent-doc does not seem to detect that I cancelled the Claude turn. Compact exchange should work even when a turn is active.~~
- ~~📌 haiven.md response was not committed.~~
- ~~📌 Please look at the remaining queue items & the backlog. Many of these items are likely stale. Please close anything that is stale as moot and keep anything still relevant.~~
- ~~📌 do [#zqv8]~~
- ~~📌 do [#crde]~~
- ~~📌 do [#steeringobservableset]~~
- ~~📌 do [#jbcompactexchange]~~
- ~~📌 do [#goal1d0k]~~
- ~~📌 do [#idlewatchturnactive]~~
- ⏭️ 📌 do [#jblocalhistverify]
- 📌 do [#compactcrdtretry]
- 📌 do [#capturebacklogatomic]
- 📌 do [#opcapturelazily]
- ~~📌 do [#tmuxfocusfollow]~~
- ~~📌 do [#clippy197cleanup]~~
- 📌 do [#jbtsiftnosub2verify]
- 🚧 📌 do [#restartstderrbleed2]
- 📌 do [#5bzd]
- 📌 do [#sidecarstate]
- 📌 do [#64we]
- 📌 do [#adstatechart2]
- 📌 do [#adstatechart3]
- 📌 do [#adstatechartkt]
- 📌 do [#adstatechartjs]
- 📌 do [#ipcauthority]
- 📌 do [#ipcliveness]
- 📌 do [#ipcverhandshake]
- 📌 do [#ipcpostcommitinv]
- 📌 do [#staleplugindeploy]
~~~done
📍 In haiven.md, I interrupted a turn in the Claude console.  I then ran JB `Compact Exchange` and it hangs. 2 issues. agent-doc does not seem to detect that I cancelled the Claude turn. Compact exchange should work even when a turn is active. I also lost my prompt somehow. I want away from my file, the agent-doc: Compacting Exchange banner disappeared and my prompt also disappeared. Another attempt to JB `Compact Exchange` errorred:
```
agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/recruit/haiven.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
```
~~~
~~~done
📍 In haiven.md, I interrupted a turn in the Claude console.  I then ran JB `Compact Exchange` and it hangs. 2 issues. agent-doc does not seem to detect that I cancelled the Claude turn. Compact exchange should work even when a turn is active. I also lost my prompt somehow. I want away from my file, the agent-doc: Compacting Exchange banner disappeared and my prompt also disappeared. Another attempt to JB `Compact Exchange` errorred:
```
agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/recruit/haiven.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
```
~~~
- ~~In haiven.md, I interrupted a turn in the Claude console.  I then ran JB `Compact Exchange` and it hangs. 2 issues. agent-doc does not seem to detect that I cancelled the Claude turn. Compact exchange should work even when a turn is active.~~
- 📌 t
- 📌 do [#5qjj]
- ~~📌 do [#jbcompactexchange]~~
- ⏭️ 📌 do [#jblocalhistverify]
- 📌 do [#compactcrdtretry]
- ⏭️ 📌 do [#jblocalhistverify]
- 📌 do [#compactcrdtretry]
- ⏭️ 📌 do [#jblocalhistverify]
- 📌 do [#compactcrdtretry]
- 📌 do [#capturebacklogatomic]
- 📌 do [#opcapturelazily]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
We are not using an evented FD because this path deliberately has no live bidirectional connection to the plugin. The evented transport is the socket IPC path; file IPC exists specifically for “socket absent/degraded/stale but plugin file
watcher may still apply patches”. We could use notify/inotify/kqueue as an implementation detail for the filesystem wait, but it still needs the same timeout, nack, ack-content, committed-cycle, and post-apply validation fallback. The
right extraction is probably a focused FileIpcAckWaiter/sidecar waiter with today’s poll backend first, then an evented backend as an optimization once it is out of orchestration.
---
```
● Finalize deferred with an IPC edge case: socket reported already_applied (response landed in the live buffer) but
  disk_missing_response_probe failed and it deferred ("document changed after merge; retry after typing stops"). Per
  the closeout rules I must not force-disk here — I retry the editor path. Let me first check the actual on-disk state.

  Ran 1 shell command

● The response was captured (cycle at response_captured) and socket-applied to a live JetBrains buffer
  (already_applied), but not yet flushed to disk — a live agent-doc sync --focus process for this doc is holding the
  editor buffer, so the disk-probe deferred ("retry after typing stops"). Per closeout rules I retry the editor path
  (no force-disk). Let me retry the finalize:

  Ran 1 shell command

● Same deferral, and notice: current_len=3546 stayed constant but current_hash changed between the two attempts
  (02e1c199 → 64233287) — the live buffer is actively oscillating (my boundary insertions + a live sync --focus process
  for this doc). The response is durably captured (response_sha256=d7ddb028). This is the already_applied +
  buffer-differs deadlock. Before choosing recovery, I must know if there's a real JetBrains editor or just an orphaned
  sync process holding a phantom buffer.

  Ran 1 shell command

● No live JetBrains IDE, no active listener (both sockets 0B/stale), and the sync process has exited. The
  already_applied acks came from a now-dead stale endpoint — the "live buffer" is phantom. The response is durably
  captured and disk is still the clean 2344-byte prompt. This is the documented stale-endpoint → force-disk recovery
  (no live editor to preserve)
JB `Compact Exchange` on this document had an error. As I was typing this prompt a JB `File Cache Conflict` occurred. I lost what I was typing:
```
agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: editor delivery ACK recovery for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md did not settle within 8000ms; the canonical response remains retained in CRDT + Lazily state and the editor reconnect continues asynchronously (no force-disk or operator recovery required)
```.---
 Let me first confirm HEAD hasn't advanced (no concurrent commit), then force-disk
  finalize.
```
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#5qjj] steeringmirrorverify|[operator-verify] Restart IntelliJ IDEA 2026.1 to activate plugin 0.2.258 (if stale bytecode appears, uninstall, restart, reinstall the signed local zip, and restart); during an active turn, make multiple exchange edits and verify the JetBrains banner and VS Code status item report the aggregate count/full hover, then run agent-doc once after editing and confirm the handoff queues without submitting a partial edit.
- [ ] [#zqv8] [operator-verify] #jbrelaylive Restart IntelliJ IDEA 2026.1 to activate the installed JetBrains plugin 0.2.247, rerun Run Agent Doc on tasks/professional/equityfundingsource.md while the dim Summarize recent commits suggestion is visible, and confirm dispatch succeeds without a protected-input refusal or a long UI freeze.
- [ ] [#crde] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978030079-4 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/dispatch_only_dispatch_start_proof content_hash=sha256:3251e072b5c1857ba06e9ba613e1004b5eefef103db7003d7dcf7efe0d698ea8]
  evidence: JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978020608-2 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof)
- [ ] [#steeringobservableset] #agent-doc-bug Phase 6 durable graph adoption: back RealtimeSteeringSet with a lazily reactive OBSERVABLE SET (not a drain-one-head QueueCell) keyed on the body-aware identity (Phase 3.1), reconciled on the reliable-sync plane; session-check reads set-emptiness (all directives settled) instead of re-diffing HEAD↔buffer. Plan: src/agent-doc/tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md (Phase 6). [recommended]
- [ ] [#jblocalhistverify] [operator-verify] #jblocalhistcrash live proof: install editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.232.zip (uninstall -> restart -> install -> restart), then with a corrupt Local History store trigger Run Agent Doc and confirm dispatch proceeds (no aborting AssertionError) and idea/ops log shows active_document_save_failed then normal sendToTerminal.
- [ ] [#compactcrdtretry] [focused-cycle] #agent-doc-bug JB Compact Exchange refused (CPC relay, retry_crdt_merge). The ZERO-live-editor variant is now fixed by #stale-lease-cpc-authority (0.34.75, compact converge shares apply_cpc_write). REMAINING: the live-editor concurrent-edit case (operator typing while a real editor is attached) still needs a bounded CAS retry around compact converge with retry-idempotent archive creation; fail closed with an actionable document-changed-during-compaction message when the compacted component drifted. Distinct from #jbcompactcommit. Plan: tasks/agent-doc/plan-compact-crdt-merge-retry.md
- [ ] [#capturebacklogatomic] [focused-cycle] #agent-doc-bug A finalize that captures the response but fails before recording backlog mutations strands in response_captured with requires_backlog_capture=true + pending_added_this_cycle=false; preflight then refuses closeout and errors "still response_captured after recovery", forcing a manual mv-aside because replaying the capture re-trips the backlog gate. Fix: capture the whole cycle intent (response + backlog mutations) atomically, and let preflight supersede an uncommittable capture instead of looping. Exposed by this turn. Plan: tasks/agent-doc/plan-capture-includes-backlog-mutations.md
- [ ] [#opcapturelazily] [focused-cycle] #sidecarstate op-capture (.agent-doc/op-capture/<doc-hash>.json, agent-doc-op-capture-io #qnodemerge4) is a bespoke JSON sidecar source of truth: durable ordered EditorOps + base_hash that preflight repair / CRDT merge REPLAY as authority then consume+clear (base-hash-guarded so safe). Because it is a separate replay source, clearing captures/ + state/cycles/ aside did not stop the churn this turn. Migrate onto the lazily state backbone (co-located with cycle-phase state) so captured-op authority is one coherent atomically-clearable model. 96-ref cross-crate surface; folds into #sidecarstate. Plan: tasks/agent-doc/plan-op-capture-lazily-state-migration.md
- [ ] [#jbtsiftnosub2verify] [operator-verify] #jbtsiftnosub2 live proof (0.34.74): with NO tmux pane for a doc, trigger JB Run Agent Doc and confirm the reopen trigger SUBMITS (or auto-resubmits) instead of sitting typed-but-unsubmitted; ops.log shows fresh_route_stranded_trigger_resubmit + fresh_route_start_acknowledged_after_resubmit; a stuck submit now fails closed with a visible FreshStart startup-miss.
- [ ] [#restartstderrbleed2] #restartstderrbleed2 #agent-doc-bug Fresh route-owned start bleeds CLI boot stderr into the agent pane (visible on first manual submit); distinct from shipped #restartstderrbleed. Plan: src/agent-doc/tasks/agent-doc/plan-fresh-start-stderr-bleed.md
- [ ] [#5bzd] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_start_proof pane=%25 actor_generation=1137 editor_attempt_id=unknown dispatch_proof_state=none elapsed_ms=12773 diagnostic_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1783437151060-direct_pane_dispatch_start_unproven-codex-_25-9831ae756eb0.txt ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/dispatch_start_proof content_hash=sha256:1ec714087cfb4c5fc783eb9ad2f02a563124b3df04828fbe1140370937f40135]
  evidence: JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978030079-4 dispatch_proof_state=none elapsed_ms=10168 diagnostic_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1783978049569-direct_pane_dispatch_start_unproven-codex-_83-627a6490e502.txt ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_start_proof,result=accepted_without_dispatch_start_proof)
  evidence: JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978020608-2 dispatch_proof_state=none elapsed_ms=10147 diagnostic_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1783978049960-direct_pane_dispatch_start_unproven-codex-_83-627a6490e502.txt ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_start_proof,result=accepted_without_dispatch_start_proof)
- [ ] [#sidecarstate] [focused-cycle] #agent-doc-bug Replace hot-path snapshot/cycle/live-buffer sidecars with the lazily state backbone for document authority, lifecycle recovery, and crash-reentrant pipeline stages; auto-start/reconcile the document model before preflight exposes missing CRDT replica errors; reconcile drift inside agent-doc on re-entry. Plan: tasks/agent-doc/plan-lazily-state-backbone-sidecar-retirement.md
- [ ] [#64we] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=tasks/professional/equityfundingsource.md stage=supervisor_dispatch_start_proof pane=%156 actor_generation=1039 editor_attempt_id=1783140065173-1 dispatch_proof_state=none elapsed_ms=10134 diagnostic_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/route-submit/1783140083790-supervisor_dispatch_start_unproven-codex-_156-ad5c195b73fc.txt ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=supervisor_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/supervisor_dispatch_start_proof content_hash=sha256:503e36d813c831fce9bfe367973c345adce5a2f2620dccc4fa0be6e4363b5087]
- [ ] [#adstatechart2] [focused-cycle] #agent-doc-feature Phase E rung 2 (observability): wire session-check/status to READ adstatechart configuration() as an advisory named-snapshot (transport.x editor_sync.x closeout.x supervisor) alongside today's ops.log markers. No behavior change to closeout. Guard semantics must match whatever A-D shipped. PRD: tasks/agent-doc/prd-adstatechart-local-process-statechart.md
- [ ] [#adstatechart3] [focused-cycle] #agent-doc-feature Phase E rung 3 (load-bearing): switch adstatechart to the typed lazily 0.19 ChartBuilder, bump agent-doc-state-backbone to lazily 0.19, use ThreadSafeStateChart for cross-thread status observation, and replace the ad-hoc editor_synced check at the finalize/commit site with chart send(commit, guards) plus a fail-closed fallback when the feature is off. SimWorld: N wedged-finalize->direct-commit cycles never commit from stale disk. Live editor proof gated behind decomposition + install. PRD rung 3.
- [ ] [#adstatechartkt] [focused-cycle] lazily-kt 0.9.0 SHIPPED (ChartBuilder + ThreadSafeStateChart + shared fromStates + context-free engine; GitHub Packages via release.yml, tag v0.9.0; full test suite green). REMAINING: bump the JetBrains plugin's lazily-kt dependency to 0.9.0 and consume ChartBuilder/ThreadSafeStateChart for the merge-ownership state machine (#mergestatemachine3 fold-in).
- [ ] [#adstatechartjs] [focused-cycle] lazily-js 1.1.0 SHIPPED (ChartBuilder + statechart.d.ts builder types + shared fromStates; tag v1.1.0; 169/169 tests green). No thread-safe/async chart in js (no reactive Context, no threads). REMAINING: npm publish is operator-2FA-gated (#lzjsnpmpub: npm login then npm publish --access public), then bump the VS Code plugin @lazily/js dep to 1.1.0 and consume ChartBuilder.
- [ ] [#ipcauthority] [focused-cycle] #agent-doc-bug IPC root-cause A (data-loss authority ambiguity). DONE this pass: durable recovery sidecar for operator text dropped by the content_ours merge (commit 317a336f) — write_policy::content_ours_drops_operator_text + agent_doc_fs::preserve_dropped_operator_buffer wired into all 3 run_entry ContentOurs persist sites; concurrent operator text now lands in .agent-doc/recovery/ instead of vanishing (closes the #qftlossdelta artifact gap). REMAINING: the deeper unification (make the CRDT the single closeout authority, demote content_ours/snapshot to audit-only) so the merge never needs the safety net — larger refactor, own cycle.
- [ ] [#ipcliveness] [focused-cycle] #agent-doc-bug IPC root-cause D (transport/state liveness). DONE this pass: corrupt controller-state auto-quarantine (commit e06301e6) — agent_doc_fs::read_valid_or_quarantine wired into project_controller::read_bootstrap; a 0-byte/unparseable controller-state.json is moved aside + re-bootstrapped instead of wedging start (class 11 auto-recovery). REMAINING (blocked on decomposition landing, dirty files): editor IPC listener read_line no timeout (fd/thread leak on half-open client), connect_sync no connect timeout, file-IPC no negative-ack. Touches ipc_socket.rs, write/ipc/transport.rs, agent-doc-ipc-io, editor plugins.
- [ ] [#ipcverhandshake] [focused-cycle] #agent-doc-bug IPC root-cause B: cross-process version skew has no negotiation (staleness inferred from mtime/inode; drives stale_install + stale-supervisor CRDT corruption). Extends the shipped #stale-plugin-detect. Fix: add a build-id/protocol-version field to the IPC handshake (agent-doc-ipc-protocol) so a newer finalize client refuses or forces-reload on mismatch. status.rs already carries injected_binary_version to extend to the wire.
- [ ] [#ipcpostcommitinv] [focused-cycle] #agent-doc-bug IPC root-cause C: structural invariants enforced on wrong write path. #boundaryaccum root fix landed (git.rs unconditional snapshot collapse) but needs a POST-COMMIT verification gate that fails loudly/self-heals if HEAD carries more than one boundary or a duplicated scaffold. Reuse ipc_corruption.rs detect_duplicated_scaffold. Touches git.rs, ipc_corruption.rs, flow/closeout.rs. Plan: tasks/agent-doc/plan-boundary-accumulation-committed-blob.md.
- [ ] [#staleplugindeploy] After the module-decomposition refactor lands: make install + push the shipped #stale-plugin-detect change (submodule commit 9b38d017). Deferred this cycle: installing would deploy the uncommitted decomposition WIP as the live binary. Verify a live stale plugin then surfaces the stale_plugin preflight warning.
- [ ] [#vscsaveproj] #agent-doc-bug VS Code save_document observability parity: processSaveDocumentSignal (editors/vscode/src/extension.ts) only writes outcomes to outputChannel, while JB saveDocumentViaDocument records StateProjectionBridge surface ops for missing_file/missing_document/saved/failed. Add the VS Code state-projection surface-op analogue so save_document outcomes are observable in the state mirror (not just the log), plus a behavioral test driving processSaveDocumentSignal end-to-end (open doc -> signal -> save -> ack-content).
- [ ] [#restartbleednonroute] #agent-doc-bug Harden auto-install child stdio against fd2==pane: run_auto_install_steps_once (project_controller/rpc.rs) blindly dups fd2 via auto_install_child_stdio_plan(); when SupervisorStderrRedirect is inactive (non-route-owned TUI, or route-owned but maybe_start fell back to inactive on error) fd2 is the agent pane and make-install output still bleeds. Open the session project_root .agent-doc/logs/supervisor-stderr.log and pass its fd to auto_install_child_stdio_plan_to_fd(log_fd) so build output is off-pane regardless of route ownership; add a redirect-inactive regression.
- [ ] [#gd88] [operator-verify] #restartstderrbleed live re-proof on a fixed binary: after the supervisor rebuilds onto a build containing auto_install_child_stdio, trigger a restart/auto-install idle boundary and confirm the agent pane shows NO make/cargo bleed, NO tmux split-window, and NO submitted /usage.
- [ ] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [ ] [#af88] [focused-cycle] #agent-doc-bug IPC wedge hardening follow-ups from the 2026-07-01 audit (wedges C + A FIXED this cycle: supervisor IPC listener now spawns per-connection threads so a half-open client cannot freeze all supervisor dispatch; editor IPC send now has set_send_timeout). Remaining: (E) pty.rs stdin.lock() held across the blocking read — evaluate a raw-fd read that preserves the restart/quit-prompt stdin coexistence in start.rs; (B/D) editor IPC listener per-connection read_line has no timeout (thread/fd leak under a persistent half-open client) — add a bounded backstop; (F) connect_sync has no connect timeout (mitigated today by the 3s wait_for_controller deadline loop). Also decide the AGENTS.md 'No async' convention nuance for the server-scale roadmap.
- [ ] [#claimnewpane] [focused-cycle] #agent-doc-feature Claim for Tmux Pane should open a new document in a fresh pane in the SAME tmux session (never replace the current pane or move the window to another session). Plan: tasks/agent-doc/plan-claim-new-pane-same-session.md
- [ ] [#boundaryaccum1] [focused-cycle] #agent-doc-bug Scattered agent:boundary markers accumulate in committed content (3 in this doc: d0dd7c4e, qactsync1, ftimm0001) — violates the exactly-one-boundary invariant. Root cause: reposition_boundary_in_snapshot (git.rs:1613) hands the collapse to the finalize/IPC path via the active-run guard AND delegates the working-tree rewrite to the IDE plugin via the IPC-listener guard; when finalize wedges on stale_install and the response lands via direct commit (and the stale plugin never applies the IPC reposition signal), nothing re-enforces the single-boundary invariant on the binary's own staged/committed blob, so one boundary accretes per wedged cycle. Fix: always collapse boundaries in the binary-owned snapshot/staged blob independent of the guards (git stages a snapshot, not the live editor buffer, so it never races user text) and scope the guards to the working-tree disk rewrite only; add a post-commit invariant check that reconciles or fails loudly if HEAD carries >1 boundary; promote the aspirational reposition_boundary_removes_stale/apply_patches_boundary_invariant tests to real regressions + SimWorld coverage for N wedged-finalize→direct-commit cycles. Distinct from #fullboundary (convergence gate reduces the wedge trigger; this makes the invariant self-healing regardless of write path). Plan: tasks/agent-doc/plan-boundary-accumulation-committed-blob.md
- [ ] [#qactsync] [focused-cycle] #agent-doc-feature Bidirectional realtime sync of queue-activation state: when the operator adds/removes go on agent:queue OR changes the queue: frontmatter value, the realtime system converges BOTH representations to the operator edit (operator-authoritative, never revert). Today explicit_queue_go_mode reads both but they are not synced, so halting needs editing both (go-marker churn). Define a canonical QueueActivation state + edit-provenance propagation; deterministic on two-sided conflict; fixed-point on synced input. SimWorld both directions + conflict; make tmux-ci + live editor proof. Plan: tasks/agent-doc/plan-queue-activation-state-sync.md
- [ ] 🚧 [#ftimmediate] #agent-doc-feature Free-text prompts begin work immediately: a free-text exchange/queue prompt is an executable directive that should start work at once; if the agent determines a dedicated/fresh session is needed (high accretion or focused-cycle scope), it should auto /clear or request a PCP restart and RUN the work, not park it as an operator-gated defer. Tighten the #drain-no-defer / #qfocsup boundary so focused-cycle free-text prompts get run-after-clear rather than left for the operator.
- [ ] [#qeditrace] [focused-cycle] #agent-doc-bug Pre-run queue edits partially reverted: when the operator adds multiple queue heads before a run (or JB Run Agent Doc) with no active turn, only the FIRST added head survives and the rest revert to the pre-edit queue. Distinct from the during-turn race - the preflight queue-maintenance reconcile drops all-but-first operator addition. Investigate preflight/maintenance.rs: queue_journal record + record_live_buffer (#qftloss mode-6), #mirrorall backlog-to-queue mirror, #rz3a freshly-added hold-out, #7r2s manual-add pin, #qdedupsync dedup - hypothesis is convergence keeps the first new head (pin/anchor match) but journals-without-replaying the rest. Repro 2026-07-01 reitrades.md; live dup heads in bugs2 (do #tmuxsynccrashlog x2, do #60ev x2). Add SimWorld coverage for multi-line pre-run operator-add survival + fail-closed recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#jbcompactcommit] [operator-verify] Live JB Compact Exchange no-uncommitted-changes proof: after the fixed binary is installed + recycled, run JB Compact Exchange on a large session doc and confirm HEAD lands the compacted content (no green git gutter / no snapshot>visible drift on next preflight). Grep ops.log for compact_commit_head_mismatch (must be ABSENT on success; present = the new fail-closed guard fired with a reset --from-current recovery). Rust fix shipped in agent-doc-orchestration/src/compact.rs commit_compacted_authoritative (#jb-compact-commit-left-uncommitted).
- [ ] [#tmuxsynccrashlog] [operator-verify] Replacement for #tmuxsynccrash: JB Sync Tmux Layout live repro log review. After the operator runs repeated JB Sync Tmux Layout against the repro layout, inspect idea.log, agent-doc ops.log, and tmux server pid/session evidence; close only if the server pid stays stable with repair_layout no-op/passive supersede markers, otherwise file a follow-up bug with the failing log markers. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] [#60ev] plan-dynamic-context-injection-ledger.md
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#coredecomp] [focused-cycle] Keep the former `agent-doc-core` God-crate surface decomposed into owned pure crates. Audit `pending.rs`, `queue_item_lifecycle.rs`, `template.rs`, `crdt_sync.rs`, `component.rs`, and related modules; move element-local semantics into `agent-doc-element-*`, document composition into `agent-doc-document`, merge/CRDT merge policy into `agent-doc-merge`, and FFI ABI into `agent-doc-ffi`. Do not recreate `agent-doc-core` as a compatibility facade or migration alias, and do not move runtime IO, snapshots, commits, IPC, tmux, or controller effects into pure crates.
- [ ] [#orchdecomp] [focused-cycle] Decompose the `agent-doc-orchestration` God crate along effect boundaries. Audit turn lifecycle, document realtime scheduling, queue/exchange lifecycle, closeout/commit, editor IPC, supervisor/controller state, tmux integration, recovery/repair, and session routing; move stable responsibilities into `agent-doc-turn`, `agent-doc-document`/document-realtime, `agent-doc-supervisor`/controller-state crates, `agent-doc-editor`, `agent-doc-tmux`, and narrow closeout/recovery crates as appropriate. Leave `agent-doc-orchestration` as a shrinking effect adapter only, not a deprecated compatibility facade; callers should move directly to the owning focused crate and pure merge/document code should stay out of orchestration.
- [ ] [#skillharnessplugin] [focused-cycle] Create the agent-doc integration plugin for `skill-harness`: implement an agent-doc-owned `SkillHarnessPlugin` adapter, define its detection and skill-path contract without depending on `agent-doc-core`, register/document the integration path, and add tests proving `skill-harness` can resolve agent-doc through the plugin boundary after `agent-doc-core` removal.
- [ ] [#crdtauth6] [operator-verify] CRDT authority live proof: verify the existing JB DocumentListener -> CrdtReplicaForwarder.forwardLocalDelta path, Kotlin Unix-socket ReplicaTransport, VS Code koffi replica seam, and Rust fan-out stay connected in a live editor session. This is proof/parity work only; operator-visible text is already authoritative and must not wait on this item. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#kj5flive] [operator-verify] #mergestatemachine3 live proof replacement for cleared review #kj5f: after the latest JB plugin update is installed, verify JB+VSCode attach/detach/ownership facts flow through the merge-ownership state machine via #lazilystatesync, then run live editor-attached/detached repros for File Cache Conflict and the #6b5h wedge with logs sufficient to prove/disprove the fix before filing follow-up bugs. Plan: tasks/agent-doc/plan-merge-control-state-machine.md
- [ ] [#suprestassoc-live] [operator-verify] Restart the `tasks/professional/equityfundingsource.md` supervisor in a live editor/tmux setup and verify the actor/admin projection keeps EFS bound to its own pane/session without re-associating to `tasks/agent-doc/agent-doc-bugs2.md`.
- [ ] [#agentsignals] [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [ ] [#elementgraphprd] [future/separate] Define the single-document element layout for both single-model sessions and multi-harness orchestration, including typed graph/workers/jobs/schedule elements plus an advisory perceptron graph that scores visible signals without directly mutating runnable work. PRD: tasks/agent-doc/prd-element-layout-perceptron-graph.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#qftlossdelta] #agent-doc-bug focused invariant-loss follow-up from failed #qftlossproof live proof: `.agent-doc/logs/ops.log:4338964-4338997` shows ACK/live-buffer content at len 16676 hash `83f08ab7e6be49a671f96f7c0ffbb71fe28bba858929b883c5904140cabd5192`, then closeout committed len 16669 hash `dc278d260b5b2a3efffeb12c428f0f81f5be1c8373fdd3ef044eee4b39adda4f`; `live_buffer_classify` suppressed the differing operator-capable buffer with `reason=write_provenance_newer_than_buffer` and no durable sidecar preserving the 16676-byte content was found. Fix the operator-wins merge/write-provenance path so concurrent operator text is preserved end-to-end or the write fails closed with a recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
