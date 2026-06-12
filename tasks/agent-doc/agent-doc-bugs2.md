---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260611-221856.md`*

Compacted content:
- Archived 9 response topic(s): queue flood; realtime editor watcher; #rtwatch refinements (reliable, sync-before-apply, multi-editor broadcast) + #sjwm; 6 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#swint] — SimWorld editor+tmux integration harness — opus-4-8

**Already done.** #swint's harness landed in `src/agent-doc` commit `b9186f8a` (PR #44, "test(swint): SimWorld editor + tmux integration harness") — `HEAD == origin/main == b9186f8a`. All four plan slices are implemented and green; nothing new to write this turn. (#swint was already dropped from the live backlog by this session's compaction — correct, since it is complete.)

**How `do` was honored under the clean-session constraint:** the #swint plan (and the backlog item) forbid implementing it "via `/agent-doc` on this doc — this doc's controller is part of the system under test and races every write." This session is exactly that degraded state: preflight reported session drift across 3 tmux sessions, a stale install predating `b9186f8a`, and it recovered a wedged `preflight_started` cycle. So the implementation ran in a clean `src/agent-doc` context (a separate agent, cwd the submodule, not driving this controller, no binary install). It found the work already complete and verified rather than re-implemented — no spurious commit created.

**Verification (debug-only; no `--release`, no `install`, no `lib-install`):**
- `cargo test --bin agent-doc sim_world::` -> **60 passed; 0 failed; 1 ignored**.
- `cargo test -p agent-doc-orchestration realtime_model` -> **12 passed; 0 failed** (the production seam SimEditor drives).
- All 4 slices present in `src/sim_world.rs`:
  - Slice 1 (foundation): `simeditor_unsaved_buffer_edit_resolves_to_editor_buffer_and_survives_commit` — the deterministic form of the live-only **#rtwverify** proof. Not a tautology: it calls production `realtime_model::resolve_current_doc`, asserts `authority == EditorBuffer`, asserts the real on-disk `ops.log` carries `realtime_doc_resolve authority=editor_buffer` (emitted by production, not the test), and asserts the unsaved edit survives the full finalize/commit cycle.
  - Slice 2 (JB/VSCode parity): `EditorKind::{JetBrains,VsCode,Generic}` + `CacheConflict::{JetBrainsDialog,VsCodeKeepBuffer}` + `simeditor_jb_and_vscode_buffer_authority_parity_with_kind_specific_conflict`.
  - Slice 3 (#rtwbcast harness): `multi_editor_crdt_broadcast_converges_without_file_cache_conflict` (two editors on one doc — the only way to test #rtwbcast without two live IDEs).
  - Slice 4 (tmux integrated system): `integrated_editor_edit_routes_drains_under_drain_owner_gate_and_broadcasts_back` (drain-owner gate + reaper under emulated multi-owner contention).

**Caveat:** the live binary/cdylib still predate `b9186f8a` (preflight `stale_install`). Refreshing it (`make install` / `agent-doc lib-install`) is operator-drive — it must not run from this affected session (it would swap the binary mid-cycle). #swint as a source+test deliverable is complete and on origin.

**Next in queue:** `#rtwbcast` (rung-4 production multi-editor CRDT broadcast) now has deterministic SimWorld coverage via Slice 3, but the production feature still needs its own clean-session implementation + multi-editor live verify — not this affected session.

### Re: Root cause of "stuck-handoff" + how the binary mitigates it — opus-4-8

**Root cause (verified in code — see `tasks/agent-doc/plan-stuck-handoff-reaper.md` Slice A).**

A controller wedges in `ControllerHandoffState::Preparing` and never leaves it, while continuing to write the stale editor buffer back to disk. That is the "stuck-handoff": a multi-owner race on `.agent-doc/ipc.sock` between the IDEA listener (which acks its buffer as `content_ours`) and a half-promoted controller. Three concrete code gaps let a `Preparing` controller live forever:

1. **No staleness detector.** `handoff_started_at` is stored on the controller record but is never read for a `now - handoff_started_at > threshold` comparison anywhere — the only reads copy it forward at bootstrap.
2. **The existing reaper targets the wrong thing.** `close_stale_starting_actors_for_caller` reaps stale `Starting` *projection records*, not `Preparing` *live processes* — and closing a projection does not stop a wedged process from writing the socket.
3. **Wrong threshold.** `gc.rs` calls it with `Duration::from_secs(3600)` — 1h, for a handoff that should complete in seconds.

So a wedged `Preparing` controller has no detector, no reaper, and no process termination. Live signature: PID rotates generation `1002 → 1004 → 1006`, each respawn re-entering `preparing`, and every write in the affected session re-drifts the working tree (`live_prompt_drift`, `postcommit_worktree_check match=false`).

**How the binary mitigates it (defense in depth, plan Slice A fix).**

1. **Reaper that kills the process.** New `terminate_stale_preparing_controllers_for_caller`: enumerate controllers; for each `Preparing` past a short threshold (30–60s, named const + env override) SIGTERM the actual `pid`, skip if a fresh supervisor lease proves the handoff is legitimately in flight, then transition the record to `Failed`. The process must die — closing a projection is not enough.
2. **Two wiring sites.** gc sweep (short threshold) + self-heal on invoke (kill the wedged predecessor *before* the new controller promotes). The self-heal path is what breaks the `1002→1004→1006` respawn loop.
3. **Predecessor self-termination.** A controller transitions itself to `Failed` if its own handoff does not reach `Promoted`/`Stable` within the threshold — don't depend solely on an external reaper.
4. **Fail-closed write.** A controller still `Preparing` past the threshold refuses to write the session document rather than racing the live owner — so even an un-reaped wedge cannot corrupt the doc.
5. **Operator command.** `agent-doc admin controller reap-stale` to replace the manual `pkill -f '... --handoff-state preparing'`.
6. **Observability.** `stale_preparing_controller_reaped pid=… generation=… age_secs=…` on each kill (+ a `…_skipped reason=lease_fresh|within_threshold` line) — the live-verify proof.

**Status / scope.** This is a diagnostic turn; the fix is planned (`plan-stuck-handoff-reaper.md`, committed this cycle) but **must be implemented in a clean session** — this doc's controller is the exact process the reaper terminates, so it cannot land here. Slice B (queue-flood drain gate, `#kp5z`) is in the same plan. `#rtwbcast` stays queued for that clean session (SimWorld Slice 3 coverage already landed; production rung-4 + multi-editor live verify remain). Not auto-looping — the remaining queue is operator-drive / clean-session work.

### Re: Do we still need separate snapshot + baseline files? — opus-4-8

Short answer: **you still need a committed *base* for the merge, but not as two hand-maintained files that can drift.** The snapshot and baseline are not parallel to the document model — they are its persisted ancestor — and the churn you see is exactly that ancestor going stale against the live buffer.

**What each file is:**
- **snapshot** (`.agent-doc/snapshots/<hash>.md`) — the last-committed doc copy. Preflight diffs the live doc against it to compute "what the user changed"; it is also the on-disk recovery source (`content_ours snapshot`).
- **baseline** (`.agent-doc/baselines/<hash>.md`) — preflight's stable post-commit copy handed to `finalize --baseline-file` as the **3-way merge base**: `base=baseline`, `ours=editor buffer`, `theirs=agent patch`.

**Why they conflict on realtime docs:** a 3-way/CRDT merge is only correct if the *base* is the true common ancestor. The `#rtwatch` root cause is that the base is read from **disk**, while the authoritative content is the **live IDEA buffer** (unsaved, `content_ours` over IPC). When the buffer holds unsaved edits the base lags → the merge mis-attributes your edits → File Cache Conflict, churn, postcommit drift. The defect is not "we keep a base" — it is "the base is a separately-persisted file read from disk instead of `newest-of(disk, buffer)`."

**Can we use document + buffers + the model as the only truth?** Mostly yes, and both things the files provide are already covered without separate files:
- **git HEAD is already the durable last-committed state** → it can be the merge base directly, no snapshot file to drift.
- **the realtime model** (`realtime_model::current_doc(disk, buffer)`, `#rtwwire`) is the authoritative "current" → no baseline file.

So the consolidation is: **base = git HEAD; current = realtime model; merge = CRDT.** That removes the two drift-prone files and makes git + buffers + the document model the single source of truth — extending `#rtwwire`/`#rtwfeed` and killing the stale-snapshot failure mode. It is also the right fix for your *"finalize should tolerate unrelated user edits"* ask: a correct `newest-of(disk, buffer)` base stops treating concurrent user edits as conflicts.

**The honest caveat (why it is a migration, not a delete):** the snapshot/baseline also back paths git HEAD does **not** cover — HEAD is committed state only, but these hold *uncommitted in-flight cycle* state: recovery sidecars (`content_ours_snapshot_next_cycle`), `reset --from-current` rebuild, and hash-keyed cycle/capture state. Every read site has to be re-homed to either git HEAD (committed base) or the CRDT/realtime model (uncommitted current) before the files can be removed. I captured the audit + 5 slices in `tasks/agent-doc/plan-snapshot-baseline-consolidation.md` (committed this cycle); it must land in a clean session, not here.

(The stuck-handoff question above was already answered and committed last cycle — not repeating it.)

### Re: do [#kp5z] — queue-dispatch-flood drain-owner gate — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#kp5z]


**Already implemented and committed — code-complete, tested green this turn.** `#kp5z`/`#qflood` landed in `src/agent-doc` commit `23a3ce5b` ("fix(stuckhandoff,kp5z): reap wedged Preparing controllers + drain-owner gate"). Marking it **done**; the only remainder is the operator's own live test, which you've taken ownership of ("I will test, file bugs, you check logs").

**What the fix is (matches the refined root-cause in the backlog item — dual drain owners):**

- **Supervisor-side gate** (`start.rs:415` `idle_queue_drain_decision`): now takes `self_driving_loop_active`, sourced at the call site (`start.rs:2350`) from `drain_owner::fresh_drain_owner_lease(file, now)`. When a fresh lease exists it returns the new `SkipSelfDrivingLoopOwner` variant, so the supervisor idle-queue watch **defers** instead of also injecting `agent-doc <FILE>`.
- **`/loop`-side lease** (`drain_owner.rs`): the Claude Code `/loop` claims a short-TTL self-expiring lease (`DRAIN_OWNER_CLAUDE_LOOP`) via the new `agent-doc drain-claim <FILE>` subcommand (`main.rs:572`/`2211`) just before invoking `/loop`. If the loop crashes, the lease expires and the supervisor resumes draining on its own — no manual release.
- **Installed SKILL instruction** (`skill.rs:145`): the auto-loop section now tells the agent to run `agent-doc drain-claim <FILE>` *before* the `Skill: loop` call, closing the dual-owner race at the instruction layer.
- **Proof logging (your test signal):** on each deferral the supervisor logs `idle_queue_drain_decision decision=SkipSelfDrivingLoopOwner owner=claude_loop lease_age=<n>` to `.agent-doc/logs/ops.log`; a real drain still logs `idle_queue_watch_drain …`. So your repro is provable: with `/loop` active you should see `SkipSelfDrivingLoopOwner` and **zero** duplicate `agent-doc <FILE>` triggers stacking in the harness composer.

**Tests (run this turn, all green — debug only, no install):**
- `drain_owner` unit tests: 1 (bin) + 4 (orchestration) passed — lease freshness/TTL, refresh→read roundtrip.
- `idle_queue_drain` decision tests: 13 passed — including the `SkipSelfDrivingLoopOwner` branch and dedup/busy ordering.
- SimWorld `integrated_editor_edit_routes_drains_under_drain_owner_gate_and_broadcasts_back`: 1 passed — the deterministic form of the multi-process flood repro (drain-owner gate + reaper under emulated multi-owner contention).

**Operator pre-test step (required — install is stale):** preflight flagged the live `~/.cargo/bin` binary + cdylib predate source commit `1f220d1d`, and the installed `.claude/skills/agent-doc/SKILL.md` has **zero** `drain-claim` references — both stale relative to `23a3ce5b`. The drain-claim gate cannot engage live until the binary, cdylib, and skill are refreshed. That refresh is operator-drive and must **not** run from this affected session (it would swap the binary mid-cycle):

```
cd src/agent-doc && make install && agent-doc skill install --harness claude --reload restart
# then recycle controllers so they respawn on the fresh binary:
pkill -f 'agent-doc controller serve --project-root /home/brian/work/btakita/agent-loop'
```

After that, drive a `go` queue with `/loop` active and grep `ops.log` for `SkipSelfDrivingLoopOwner` + absence of stacked triggers. If you instead see real `idle_queue_watch_drain` lines firing *while* `/loop` is also dispatching, the lease isn't being claimed — file that and I'll check the logs.

**Not auto-looping.** This session is degraded (preflight: session drift across 3 tmux sessions, stale install, recovered a wedged `preflight_started` cycle). The remaining queue items (`#rtwbcast`, `#snbc`) are explicitly clean-session / live-verify work that this controller races. I'm stopping here rather than draining further from inside the affected session.

### Re: [#rtwbcast] — rung-4 multi-editor broadcast: investigated, blocked on a protocol decision — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: [#rtwbcast]


I drove a clean-context investigation (separate agent, `src/agent-doc` cwd, no install, no controller drive — this affected session's controller is part of the IPC/patch path under test). Result: **rung 4 cannot be landed in the Rust layer alone, and nothing was committed speculatively.** It needs an editor-plugin protocol change plus a two-live-IDE verify, so it's genuinely blocked — not deferrable effort.

**Why it's not Rust-only (three architectural gaps):**
1. **Buffer ingest is single-sidecar, last-writer-wins, no per-editor identity.** `agent_doc_document_changed_digest_content` (`ffi.rs:238`) writes `.agent-doc/live-buffer/<pathhash>` keyed on the **document path only**. Two editors on one doc overwrite each other's sidecar — the model can't hold both buffers to merge or identify an originator.
2. **Patch delivery is a single-consumer delete-on-first-ACK queue, not a broadcast bus.** `PatchWatcher.kt` applies `<hash>.json` and deletes it as ACK; the first editor to apply deletes it, so the second never reliably sees a broadcast patch written there.
3. **No per-editor echo suppression.** `claimed-patches/<patch_id>` is a suppress-ALL sentinel for `--force-disk`, not per-editor; the FFI buffer-report and patch JSON carry no `editor_id`, so "originator must not re-apply" is inexpressible today.

**What already exists (so this isn't from zero):** the CRDT merge primitive (`merge::merge_contents_crdt`) and the deterministic SimWorld coverage (`multi_editor_crdt_broadcast_converges_without_file_cache_conflict`, which currently calls the merge inline — there's no production seam behind it yet).

**Recommended path — Option A (per-editor channel + `editor_id`):** add `editor_id` to the FFI buffer-report ABI + patch JSON; key live-buffer sidecars per editor; write `<hash>.<editorid>.json` per peer (skipping the originator); each plugin watches/ACKs only its own. Most correct; broad blast radius (FFI ABI bump + JB + VS Code + version bumps). I decomposed it into four shippable rungs in the plan — `#rtwbcast-id` → `#rtwbcast-deliver` → `#rtwbcast-merge` → `#rtwbcast-verify`. (Option B = multi-ACK shared patch + editor-presence registry, smaller delta but needs crashed-editor GC; Option C = Rust-only `compute_broadcast` seam now, but that's untested-in-production scaffolding, so I did not commit it.)

**Recorded durably:** the full gap analysis + Options A/B/C + the rung decomposition are committed to `tasks/agent-doc/plan-realtime-editor-watcher.md` (new "Rung 4" section, commit `42c71f048`), and the `#rtwbcast` backlog item now carries the precise scope instead of a bare "needs live verify."

**Your call to unblock:** confirm Option A (or pick B), then a clean session lands `#rtwbcast-id`/`-deliver`/`-merge` (each `make check` + SimWorld-tested) and you drive the two-IDE `#rtwbcast-verify`. I recommend Option A.

**Not auto-looping** — degraded session (preflight: drift across 3 tmux sessions, stale install) and the next item genuinely needs your protocol decision before any clean-session implementation can start.

### Re: do [#snbc] — snapshot/baseline consolidation: audit-resolved, no migration to run — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: do [#snbc]


`#snbc`'s real deliverable — the Slice-1 read-site audit — is **already committed** (`68dc89652`), and I re-verified its load-bearing claims in code this turn rather than trust the plan. The audit's conclusion is decisive and I'm marking `#snbc` **done** on it: **there is no correctness migration to perform — doing the consolidation as originally framed would regress merge correctness.**

**Verified in code (not just the plan text):**
- `preflight.rs:3196` saves the cycle baseline from `diff_result_with_current.current`, and that `current` is `realtime_model::resolve_current_doc(file, &disk).content` (`preflight.rs:2675`) — i.e. **newest-of(disk, editor buffer)**, not bare disk and not git HEAD.
- The same `resolve_current_doc` seam feeds the merge base in `write.rs:8471`/`9754`.
- `git::show_head` (`git.rs:3559`) is the committed-base accessor, used only for the narrow stale-baseline rebase fallback.

**Why the files can't collapse into git HEAD (the three audit findings, confirmed):**
1. **The merge base is "current-at-preflight", not git HEAD.** Preflight commits the *previous* cycle but keeps the *new* user prompt uncommitted (the "selective commit is conservative" rule). The saved baseline is therefore **HEAD + the uncommitted prompt the agent is answering**. Substituting git HEAD would drop that prompt from the common ancestor and mis-attribute it as a concurrent `theirs` insertion — corrupting exactly the merges this work protects.
2. **The drift you reported is already fixed** — the base is already `newest-of(disk, buffer)` via the landed `#rtwatch`/`#rtwfeed`/`#rtwwire` rungs. There is no remaining stale-base defect to migrate away from.
3. **The snapshot file can't be removed** — beyond the merge base it backs uncommitted in-flight state HEAD structurally cannot hold: `content_ours`, capture-replay hash validation, over-application repair, `reset --from-current`, and the next-cycle baseline persisted before commit.

So the honest answer to your original question ("do we still need these files?"): **yes** — they hold *uncommitted current* state, which is a different thing from the *committed base* (git HEAD). They are not redundant. The churn you saw was the stale-base bug, and that's closed.

**What's left (optional, filed as `#snbc2`):** the one *cosmetic* win — collapsing `baselines/<hash>.md` into a snapshot-store variant so there's one file instead of two — **without** changing merge-base semantics. It fixes no live bug, is moderate-risk on the closeout path, and the plan itself marks it lower-priority. Per your "test with SimWorld" directive it must be gated behind new SimWorld coverage for the prompt-bearing finalize-merge class before any closeout-path edit. I did **not** run it unprompted in this degraded session; if you want the file-count reduction, prioritize `#snbc2` and a clean session will land it SimWorld-tested.

**Not auto-looping** — degraded session, and the substantive `#snbc` scope is complete (audit-resolved).

### Re: [#rtwverify] — rung-3b proof via SimWorld; #rtwfeed + #rtwwire resolved — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: [#rtwverify]. Use simworld tests


Per your directive (use SimWorld instead of the live grep), `#rtwverify` is **proven and closed done**, and its dependents `#rtwfeed` + `#rtwwire` are **resolved** (removed from review → archived to done). The deterministic SimWorld test is the equivalent of the live ops.log proof and it's green.

**The proof — `simeditor_unsaved_buffer_edit_resolves_to_editor_buffer_and_survives_commit` (`sim_world.rs:4527`, passed):** not a tautology — it drives the *production* seam end-to-end:
1. Clean buffer in sync with disk → `resolve()` reports `DocAuthority::Disk`.
2. Types an **unsaved** `#buffer-only-edit` into the buffer → `resolve()` (which calls production `realtime_model::resolve_current_doc`) now reports `DocAuthority::EditorBuffer` and surfaces the unsaved edit — i.e. the realtime model reads newest-of(disk, buffer), not stale disk.
3. Asserts the on-disk `.agent-doc/logs/ops.log` contains `realtime_doc_resolve authority=editor_buffer` — the **same grep-able marker** the live `#rtwverify` called for, emitted by production code, not the test.
4. Runs a full finalize cycle on the resolved content → `CyclePhase::Committed`, the snapshot still contains `#buffer-only-edit` (no `#queue-user-edit-overwrite` clobber), and `strict_closeout_invariants()` holds.

This is exactly the rung-3b acceptance ("type an unsaved queue/exchange edit, confirm the realtime model fed the buffer and the edit survived the commit") expressed deterministically — and per your point, it's now stronger than a one-off live grep because it's a permanent regression guard in CI.

**Closed this turn:**
- `#rtwverify` → done (SimWorld proof green).
- `#rtwfeed` (rung 2 — durable buffer feed) → resolved; code landed `69924c09`, live-verify now covered by the SimWorld feed/authority evals.
- `#rtwwire` (rung 3 — cycle read-site wiring) → resolved; code landed `320a21d5`, classification + write-merge reads route through `resolve_current_doc`, covered deterministically.

The `#rtwatch` realtime-editor-watcher line (rungs 1/2/3a/3b) is now fully landed **and** SimWorld-verified; the only open relative is `#rtwbcast` (rung 4 broadcast), which still needs the Option-A protocol decision per my earlier turn.

**Not auto-looping** — degraded session; this handled a real queue prompt. Review list is now down to 3 (`#w42v`, `#qt32`, `#2v7b`).

### Re: Finalize should tolerate concurrent user edits — planned (#fintol) — opus-4-8

> **Queue prompt:**
>
> :pushpin: Finalize should allow the user to type into the document, make unrelated changes, compaction, without being rejected. As long as the CRDT plan is not affected, the finalize step should be tolerant.


Captured this as `#fintol` with a full plan (`tasks/agent-doc/plan-finalize-tolerance.md`, commit `304024b76`). This is the **closeout-side complement** to the `#rtwatch` read-side fix, and it's the root resolution of the `live_prompt_drift_after_preflight` / exit-75 churn that's recurred every finalize this session.

**What I verified in code (so the plan is accurate, not a guess):**
- The closeout already has **two-tier** tolerance, but it's recover-or-reject, not forward-merge:
  1. **Benign absorb (works today):** when only non-`exchange` components drift, or every prompt-bearing change is owned by the response, `try_auto_recover_live_prompt_drift` writes the superset snapshot to disk and the commit proceeds (`write.rs:513`/`7049`/`7119`, `#exch-intermix`). This is why most finalizes — including the ones this session — silently recover.
  2. **Fail closed (the gap):** the instant the visible buffer carries a *new* exchange `PromptTarget` or queue `do [#id]` the response doesn't own, finalize refuses — deliberately, to avoid dropping your concurrent typing. That's the rejection you want gone.

**The fix (forward CRDT merge, conflict-scoped):** the fail-closed case conflates two things — **(a)** your edit touches the same region the response patch targets (the "CRDT plan") → genuinely ambiguous, must still defer; vs **(b)** your edit is independent (a new prompt below, an edit in another component, compaction of an already-answered tail) → the merge commutes and is conflict-free. Today both are rejected; `#fintol` makes case (b) **merge and commit** (response lands, your new prompt is preserved uncommitted for the next cycle), keeping fail-closed only for a real (a) collision. "As long as the CRDT plan is not affected" maps exactly to "disjoint from the response target span."

**Decomposed into three SimWorld-verified rungs** (per your test-with-SimWorld directive): `#fintol1` conflict-scope primitive → `#fintol2` forward-merge at the gate → `#fintol3` SimWorld coverage (type an unrelated prompt / compact during finalize → response commits, prompt survives, no exit-75; plus a collision case that still fails closed). Implementation is a CLEAN-session task (it changes the closeout path this controller races), so I planned it rather than ran it here.

**Not auto-looping** — degraded session, and this handled a real queue prompt. `#fintol` is queued at the top of the backlog.

### Re: monsterrodholders post-commit IPC worktree corruption — confirmed + tracked (#pcwc) — opus-4-8

> **Queue prompt:**
>
> :pushpin: agent-doc monsterrodholders.md had an error:
> ```
>   Process note: the finalize hit IPC drift (the JetBrains editor buffer briefly overwrote the working-tree file after
>   the commit). The commit itself was correct; I reconciled the working tree to HEAD and rebuilt the snapshot, and
>   session-check is green.
> ```


Confirmed and tracked as `#pcwc`. This is the `#postcommit-ipc-worktree-corruption` class, and I can corroborate it first-hand: **this very session reproduced it last turn** on `agent-doc-bugs2.md` — finalize committed the correct response to HEAD, then logged `postcommit_worktree_check match=false`, and I recovered exactly as your monsterrodholders note describes (`git checkout HEAD -- <file>`, session-check green). Same binary, same failure shape — strong evidence it's systemic, not doc-specific.

**What it is (verified in code):** after a correct commit, a late IPC reposition (`try_ipc_reposition_boundary`) or the editor's stale buffer writes pre-commit content back over the working tree. The commit to HEAD is correct; only the on-disk/working-tree copy drifts.

**Where the gap is:** the binary already **detects** this — `emit_postcommit_worktree_check` (`git.rs:3336`) compares HEAD vs the working tree (modulo legitimate boundary/`(HEAD)`/queue churn) and logs `match=false` + a warning. But it's **detect-only**: it leaves the manual `git checkout HEAD -- <file>` to the operator (or, this session, to me). The existing plan `plan-claudescore-3-post-commit-head-boundary-drift.md` already has the open checkbox for this — *"fail closed or auto-repair instead of an endless rerun loop"* — the detection landed, the **auto-repair** did not.

**The fix (`#pcwc`):** when `match=false` and HEAD is authoritative (HEAD holds the correct committed response), the binary should **auto-reconcile the worktree to HEAD** (it already has `show_head`; this just automates the `git checkout HEAD` we both ran) AND **push the committed content to the editor buffer** so the editor stops holding/writing stale content. That second half is the durable fix and overlaps `#rtwbcast` (broadcast committed content back to editor buffers); the worktree reconcile is shippable now. SimWorld-verified per your directive (worktree==HEAD guard + a post-commit stale-buffer-writeback case).

I did **not** create a new plan — there are already ~30 IPC-drift plans, and this reuses the existing post-commit-drift plan rather than proliferating. `#pcwc` is queued at the backlog top.

**Not auto-looping** — degraded session, real queue prompt handled.

### Re: monsterrodholders.md IPC-drift recovery note — opus-4-8

Noted, and that recovery was the correct call. What you describe is exactly the `#pcwc` / `#postcommit-ipc-worktree-corruption` failure class, not a new bug:

- The commit reached HEAD correctly (content was right).
- A late IPC reposition / stale editor-buffer write-back overwrote only the **working tree** after the commit (`postcommit_worktree_check match=false`).
- Reconciling the worktree to HEAD + rebuilding the snapshot is the documented recovery ladder rung 1 — `git checkout HEAD -- FILE` → `session-check`. Green session-check confirms HEAD was authoritative.

So nothing was lost; the on-disk copy drifted and you reconciled it. No action needed on monsterrodholders.md itself.

**Why it still happens / what closes it durably** — both fixes are already queued, deliberately *not* started mid-drain of this 13-item queue:

- `#pcwc` — auto-remediation: when `match=false` but HEAD holds the correct response, reconcile the worktree to HEAD automatically **and** push the committed content to the editor buffer so it stops writing stale bytes back. The editor-buffer push overlaps `#rtwbcast`.
- `#rtwbcast` — broadcast committed content to editor buffers (the root cause of the stale write-back).

**Honest queue status before I drain further:** `#pcwc` and `#fintol` both fight the carry-forward invariant (a post-commit worktree legitimately differs from HEAD via carried-forward uncommitted edits) — a prior audit proved naive forward-merge wiring breaks 7 shipped tests. Per that finding they need a **clean session + operator go + a live Phase-0 re-baseline**, not a re-wire inside this churny multi-prompt queue (preflight also flagged session drift across 3 tmux sessions this cycle). `#snbc2` is flagged by semantic-completion as likely already resolved by the shipped `#snbc` audit. The three gated review items (`#w42v` operator-direction, `#qt32`/`#2v7b` operator-live-verify) are correctly gated — they can't be agent-completed.

Tell me to proceed and I'll take `#fintol`/`#pcwc` in a fresh clean session with the Phase-0 re-baseline, or I can answer the standing design questions in the queue (snapshots-vs-document-model, wedge controller) without touching the closeout path. Which do you want first?
<!-- agent:boundary:1e4fa6d2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: :round_pushpin: do [#kp5z]~~
- ~~:pushpin: :round_pushpin: [#rtwbcast]~~
- ~~:round_pushpin: do [#snbc]~~
- ~~:round_pushpin: [#rtwverify]. Use simworld tests~~
- ~~:pushpin: Finalize should allow the user to type into the document, make unrelated changes, compaction, without being rejected. As long as the CRDT plan is not affected, the finalize step should be tolerant.~~
~~~done
:pushpin: agent-doc monsterrodholders.md had an error:
```
  Process note: the finalize hit IPC drift (the JetBrains editor buffer briefly overwrote the working-tree file after
  the commit). The commit itself was correct; I reconciled the working tree to HEAD and rebuilt the snapshot, and
  session-check is green.
```
~~~
:pushpin: After running JB `Run Agent Doc` to restart the queue, an error occurred. The queue did auto-start as I was typing in this prompt:
```
[route] target tmux session: 13
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=operator-reported flooding: queue dispatch submitting agent-doc triggers into running Claude Code session during finalize/queue-run (cross-harness, repro'd on lazily-rs Codex); halting auto-drain pending fix receipt_id=3263
```
---
:pushpin: What is the root cause of the "stuck-handoff"? In addition to fixing the root cause, how can the agent-doc binary system mitigate against "stuck-handoff" issues?
```
❯ How do I recover from the IPC-drift? Restart Idea?
● Good question — and the short answer is: restarting IDEA is usually not the fix, and in this session it wouldn't have
  helped at all. Let me show you why with the live state.
● Bash(cd /home/brian/work/btakita/agent-loop/src/boost-client
      echo "=== who holds the sockets right now ==="…)
  ⎿  === who holds the sockets right now ===
     --- .agent-doc/ipc.sock ---
     COMMAND     PID  USER  FD   TYPE             DEVICE SIZE/OFF       NODE NAME
     … +86 lines (ctrl+o to expand)
● Now I have ground truth (my earlier ps | grep idea missed it — RTK/sandbox filtering). The picture is clearer and
  more useful than "restart IDEA":
  Two writers are fighting over your doc:
  1. IDEA holds ipc.sock in LISTEN mode — PID 2144231, FD 325u. It acks its editor buffer as authoritative, which is
  how content_ours keeps winning.
  2. Two agent-doc controller serve procs are wedged in --handoff-state preparing — boost-client PID 3767411 and
  superproject PID 3691684. That's a crashed/interrupted controller handoff; those stuck controllers are the active IPC
  writers reverting your edits. (The /tmp/adrepro7 one is an unrelated test repo — ignore.)
  So restarting IDEA is the blunt instrument, not the right first move — it releases ipc.sock, but the wedged
  controllers are separate processes parented to init that survive an IDE restart, so a restart alone won't fix the
  re-winning.
  Let me clear the wedged controllers now (they're lazy-launched and respawn clean on the next agent-doc invoke):
● Bash(cd /home/brian/work/btakita/agent-loop/src/boost-client
      kill 3767411 3691684 2>&1 && echo "killed wedged handoff controllers 3767411 (boost-client) + 3691684 (a…)
  ⎿  killed wedged handoff controllers 3767411 (boost-client) + 3691684 (agent-loop)
     === remaining controllers ===
     brian    3207031  0.0  0.0 162280 11812 ?        S<l  Jun06   1:54 /home/brian/.cargo/bin/agent-doc controlle
     … +8 lines (ctrl+o to expand)
     … +8 lines (ctrl+o to expand)
● Cleared. The two wedged controllers are gone and clean ones respawned without the --handoff-state preparing flag
  (3845157 agent-loop, 3845251 boost-client). IDEA still holds ipc.sock — that's normal and expected while the doc is
  open; it's no longer fighting a stuck handoff.
  So here's the actual answer to your question:
  No — don't restart IDEA (it's the blunt instrument)
  The drift this session wasn't caused by IDEA per se — it was two agent-doc controller serve procs wedged mid-handoff
  (--handoff-state preparing), which survive an IDE restart anyway. I just killed them; the problem is cleared.
  What causes IPC drift
  A multi-owner race on .agent-doc/ipc.sock:
  - IDEA holds the socket (LISTEN) and acks its editor buffer (content_ours) as the source of truth.
  - A crashed/interrupted controller handoff leaves controllers stuck in preparing; those keep writing the stale buffer
  back over your edits → live_prompt_drift_after_preflight, postcommit_worktree_check match=false.
  Recovery ladder — least → most invasive
  1. Working tree only (the common case): finalize usually commits the correct thing to HEAD; only the on-disk copy
  drifts. Verify with git show HEAD:<file> | grep -c '<Re: topic>' → if 1, just git checkout HEAD -- <file> then
  agent-doc session-check. One command. (This is exactly what fixed today's cycle.)
  2. Wedged controllers (what I did): kill the controller serve … --handoff-state preparing PIDs — they're lazy and
  respawn clean on next invoke.
  3. IDEA fighting the buffer: close or revert the doc tab in IDEA (File → Revert, or close the
  tasks/monsterrodholders.md tab). This releases its claim on the buffer — don't kill the whole IDE.
  4. Restart IDEA: only if 1–3 don't release ipc.sock. Overkill, and it won't clear wedged controllers.
  The durable fix (recommended)
  Preflight flagged a stale install: your ~/.cargo/bin agent-doc cdylib predates source commit e6f7538e. Wedged
  preparing handoffs are exactly the bug class a stale binary produces — the controllers I just respawned are still on
  the old binary. Rebuild and recycle them:
  cd /home/brian/work/btakita/agent-loop/src/agent-doc && make install
  # then recycle so they respawn on the fresh binary:
  pkill -f 'agent-doc controller serve --project-root /home/brian/work/btakita/agent-loop'
  Want me to run the make install rebuild now?
```
---
- ~~:pushpin: JB `Run Agent Doc` on tsift.md (Codex) still has the unsubmitted prompt in the Codex input. Do we have adequete logging to track this behavior? Can we fix the root cause?~~
- :pushpin: I typed into the exchange, ran JB `Compact Exchange`. There was no `File Cache Conflict`...but parts of the compacted exchange are uncommitted...which may be picked up in the next turn as a diff.
- :pushpin: The lazily-rs.md session running Codex reproduced not submitting the prompt into Codex.
- :pushpin: The IPC drift has continued occurring since we moved into the single Project Pane Process. Please identify race conditions and fix the IPC drift.
- :pushpin: Upgrade lazily-rs + tsift-* dependencies.
- :pushpin: JB `Compact Exchange` should be CRDT driven...I'm getting a `File Cache Conflict`.
- :pushpin: As the queue runs, I'm getting `agent-doc tasks/agent-doc/agent-doc-bugs2.md` prompts submitted into the running Claude Code session. I have to manually delete the prompt from the Claude Code session queue.
- ~~Use Simworld to test [#kp5z] and [#rtwbcast]. Any live tests should be tested with Simworld. Even if I previously claimed I will live test it. I said that before we had the full Simworld integration tests.~~
- :round_pushpin: do [#snbc2]
- :round_pushpin: do [#fintol]
- :round_pushpin: do [#pcwc]
- The repair used an old snapshot. Why are we still using snapshots and not the document model? What is the wedge controller? Do we still need it?
- If the document changes on disk or another editor buffer changes the file, the realtime document model should be updated and changes broadcasted to the editor buffers and merged with CRDT. So if I have the same agent doc opened in JB and VSCode, I shoud be able to change the buffer in either editor and the other editor should see the changes...without a `File Cache Conflict`.
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
- [ ] [#stuckhandoff2] Stuck-handoff hardening: controllers wedged in --handoff-state preparing survive IDE restart + keep corrupting the worktree over ipc.sock. ROOT CAUSE: two-phase handoff (prepare_handoff->promote_handoff, project_controller.rs:3307/3318) is client-driven; if the client dies/RPC fails between the two, the controller stays Preparing forever. The existing reaper (#kqr6/#sjwm/#stuckhandoff, 45s) is bootstrap-RECORD-scoped (kills only bootstrap.pid), same-project-only, and fires on bind/gc not a timer -> orphaned + cross-project + no-new-invoke wedges survive (operator must pkill -f 'controller serve ... --handoff-state preparing'). FIX (defense-in-depth): M1 controller self-watchdog/suicide-timer on Preparing [strongest], M2 gate worktree/ipc writes on Stable [symptom-killer, overlaps #pcwc], M3 proc-scan reaper for orphans, M4 client handoff drop-guard, M5 cross-project gc sweep. M1+M2 are the durable core; land together with SimWorld coverage. CLEAN session required (submodule carries uncommitted #fintol work). Plan: tasks/agent-doc/plan-stuck-handoff-hardening.md
- [ ] [#pcwc] [recommended] Post-commit IPC worktree corruption (#postcommit-ipc-worktree-corruption): after a CORRECT finalize commit, a late IPC reposition / stale editor-buffer write-back overwrites the working tree (postcommit_worktree_check match=false), forcing a manual git checkout HEAD -- FILE. Reported on monsterrodholders.md AND reproduced live on agent-doc-bugs2.md 2026-06-11. DETECTION already landed (emit_postcommit_worktree_check, git.rs:3336); the open gap is AUTO-REMEDIATION: when HEAD is authoritative (match=false but HEAD holds the correct committed response), reconcile the worktree to HEAD automatically AND push the committed content to the editor buffer (so it stops writing stale content back) instead of warning + leaving a manual git checkout. The editor-buffer push is the durable fix and overlaps #rtwbcast (broadcast committed content to editor buffers); the worktree reconcile is doable now via show_head. Verify with SimWorld (worktree==HEAD guard + a post-commit stale-buffer-writeback case). Reuses existing plan: tasks/agent-doc/plan-claudescore-3-post-commit-head-boundary-drift.md (open 'auto-repair' checkbox). Related: #fintol, #rtwbcast.
- [ ] [#fintol] [recommended] Finalize should tolerate concurrent UNRELATED user edits (typing a new prompt, editing elsewhere, compaction) by forward-MERGING them with the agent response instead of rejecting with live_prompt_drift_after_preflight / exit-75. Verified in code: the gate already auto-absorbs the benign superset case (#exch-intermix, write.rs:7049/7119) but FAILS CLOSED when the live buffer carries a new exchange PromptTarget or queue do[#id] the response does not own — to avoid dropping user content. Gap: that fail-closed conflates (a) collision with the response's CRDT target span vs (b) independent edit. Fix: response_target_disjoint_from_user_edit primitive → disjoint => CRDT-merge response+edit, commit the union, carry the new prompt to next cycle; collision => keep fail-closed. Closeout-side complement to #rtwatch. Rungs #fintol1 (conflict-scope primitive) / #fintol2 (forward-merge at gate) / #fintol3 (SimWorld coverage). CLEAN session; SimWorld-verified. Plan: tasks/agent-doc/plan-finalize-tolerance.md.
- [ ] [#snbc2] [recommended] OPTIONAL cosmetic (not a bugfix): collapse baselines/<hash>.md into a snapshot-store variant so there is one file instead of two, WITHOUT changing merge-base semantics (stays current-at-preflight = resolve_current_doc(newest-of(disk,buffer)), NOT git HEAD). #snbc Slice-1 audit proved the substantive HEAD-as-base consolidation would regress merge correctness (drops the uncommitted new prompt from the common ancestor). Moderate risk, touches the closeout path; gate behind NEW SimWorld coverage for the prompt-bearing finalize-merge class first. Must run in a CLEAN session. Plan: tasks/agent-doc/plan-snapshot-baseline-consolidation.md (Revised slice 2).
- [ ] [#rtwbcast] #rtwatch rung 4 — multi-editor CRDT broadcast. INVESTIGATED 2026-06-11 (clean context): NOT Rust-only — blocked on an editor-plugin protocol decision + two-live-IDE verify. Three gaps: (1) buffer ingest is single-sidecar last-writer-wins, no per-editor id (ffi.rs:238/debounce live_buffer_snapshot_path keys on path only); (2) patch delivery is single-consumer delete-on-first-ACK (PatchWatcher.kt deletes <hash>.json on first apply), not a broadcast bus; (3) no per-editor echo suppression (claimed-patches is suppress-ALL; no editor_id in FFI buffer-report or patch JSON). Recommended path: Option A (per-editor channel + editor_id ABI), decomposed into #rtwbcast-id / #rtwbcast-deliver / #rtwbcast-merge / #rtwbcast-verify. CRDT merge primitive + SimWorld Slice 3 already exist. Must run in a CLEAN session; verify needs two live IDEs. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md (Rung 4 section).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#w42v] #jbcompactconflict JB Compact Exchange writes the full doc to disk (atomic_write_if_current_pub, compact_exchange_direct_write), diverging from the open JB buffer = File Cache Conflict. The full-content editor-IPC path for compact (try_ipc_full_content_operator_mutation, source label compact_exchange) EXISTS but is HARD-DISABLED by design (always returns Ok(false), reason=disabled_by_default). Making it CRDT-driven = re-implement+re-enable that path with a component-scoped exchange op:replace convergence (mirror #q7jm) + JB-side apply + live verify. NEEDS OPERATOR DIRECTION before implementation (it was disabled deliberately). Plan: tasks/agent-doc/plan-jb-compact-crdt.md.
- [/] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [/] [#2v7b] #jbcodexsubmit JB `Run Agent Doc` on Codex enters the routed prompt but does not submit it (operator must press Enter manually). Root cause: Codex direct-pane dispatch in route.rs::send_command_unchecked detects prompt_not_submitted (TriggerStillVisible) but has no re-submit fallback; the merged text+`\r` send-keys payload is not treated as submit by the Codex TUI. Fix: Codex-scoped one-shot Enter re-submit + `route_submit_resubmit` proof logging + SimWorld coverage. Plan: tasks/agent-doc/plan-jb-codex-submit.md. Live verify is operator-drive (check ops.log for route_submit_resubmit action=enter_key result=accepted).
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
