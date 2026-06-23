# Versions

agent-doc is alpha software. Expect breaking changes between minor versions.

Use `BREAKING CHANGE:` prefix in version entries to flag incompatible changes.

## 0.34.43

- **Prose queue heads no longer disappear as pruneable noise (`#freshprosequeue`).** The queue drainability classifier now treats ordinary operator prose as real queue work even when it is phrased as a declarative bug report rather than an imperative. `queue prune-noise`, `session-check` stale-noise counts, and supervisor idle-watch dispatch still skip/clear structural artifacts such as console status lines, log-only fenced blocks, bold response fragments, and agent comments, but they no longer strike natural-language queue items like "Queue items are being struck without being worked on" before an agent answers them. Coverage updates the continuation, prune-noise, and preflight drainability contracts.

## 0.34.42

- **Windows release builds compile the library GC liveness probe.** `agent-doc gc-libs` now keeps Unix `kill(pid, 0)` probing behind Unix guards and uses the native Windows process handle API for PID liveness on Windows, so release packaging no longer trips over the missing `libc::kill` symbol while still cleaning stale versioned library locks.

- **Cycle-state sidecar mutations now pass through a lazily transition table (`#c7j5`).** The first PCP/session-actor cutover slice adds `cycle_state_machine::CyclePhaseMachine`, backed by `lazily::ThreadSafeStateMachine`, and routes the phase-changing sidecar mutators through typed `CycleEvent` transitions before the durable `.agent-doc/state/cycles/<hash>.json` journal is written. This keeps the sidecar as crash recovery while giving the controller/session actor a shared transition authority for the next cutover slices.

- **Editor-IPC shorter ACK mismatches now replay missing agent responses and stale CRDT overlays self-heal (`#ack-shorter-replay`).** When an editor ACK sidecar is shorter than the intended target only because it is missing a newly materialized `### Re:` response block, the convergence path now hash/length-proves the stale buffer and refreshes the editor to the target response instead of refusing the write and leaving the cycle interrupted. Stale overlay CRDT projections are also rebuilt from the authoritative fallback baseline on first mismatch, so repeated merge-base calls stop re-reading the same stale overlay and stop producing fallback-overlay hot loops. Coverage adds regressions for safe shorter ACK replay and stale-overlay rebuild/rate limiting.

## 0.34.41

- **Windows release builds compile the supervisor hot-reload path again.** The `#ctlrecycle` Unix `execve` adoption path now keeps its stderr redirection, raw-fd adoption, and startup-miss UTC formatting behind platform guards, so non-Unix release builds fall back to the normal spawn/relaunch behavior instead of compiling POSIX-only symbols.

## 0.34.40

- **Completed short free-text queue heads no longer survive as active queue residue (`#qheadresidue`).** Short heads such as `deploy` now count as answered when, and only when, exchange history contains an explicit labeled `> **Queue prompt:**` echo for that exact head. `session-check` now interrupts if that proved-answered free-text head is still active in `agent:queue`, preventing a later queue cycle from re-running stale completed work.

- **Paused-queue supervisor failsafe no longer self-stalls behind its own drain-owner lease (`#qstallguard-failsafe-lease`).** The paused-queue fallback used the same drain-owner sidecar as an in-session `/loop` and wrote `owner=supervisor-failsafe` before later gates proved a trigger was actually submitted. If a later gate skipped (or after the fallback-drained turn closed), the fresh self-written lease made the pause gate report `queue_control_paused` and suppress the next valid drain until the 90s TTL expired. Drain-owner freshness now has a loop-only reader; idle-watch pause and stale-recycle-yield gates defer only to a real `/loop` owner, stale `supervisor-failsafe` sidecars are ignored, and the failsafe proof log is emitted only after an actual submit/resubmit succeeds. Coverage: loop-only drain-owner test plus updated idle-watch lease integration.

- **Preset-backed free-text queue reports with fenced diagnostics now drain instead of disappearing behind operator pins (`#qfreetext-sep`).** A live `agent-doc-bugs2.md` queue head was typed as prose plus a fenced route error followed by a `---` separator, ahead of several `[operator-verify]` pinned heads. The parser kept that prose as inert `Freeform`, and the drainability classifier also treated prose+fence as noise under a preset, so preflight reported only the operator-verify heads and `queue_drainable_head_count=0`. The queue parser now treats separator-terminated prose blocks as multiline prompts, and a preset-bearing queue treats a prose lead followed by fenced diagnostics as drainable work while preserving pure all-log blocks as pruneable noise. `queue prune-noise` now deletes only all-log multiline evidence blocks under a preset and preserves prose reports for closeout response/strike.

- **Explicit `agent:` harness switches replace stale authoritative actor bindings instead of hard-failing route (`#actor-switch-rebind`).** When a document switches from `agent: claude` to `agent: codex`, an old healthy `claude-code` actor record no longer produces a permanent `bound to harness claude-code, not codex` failure. Route now recognizes the explicit frontmatter harness change, logs the mismatch as stale, and falls back to the normal create/rebind path for the newly resolved harness. Healthy wrong-harness actors still fail closed when the document did not explicitly declare the new expected harness.

## 0.34.39

- **Manual Sync Tmux Layout now closes crash-left response commit boundaries (`#sync-jbccc-repair`).** Full `agent-doc sync` / `session doctor --repair` now runs the existing `jb_cache_conflict_cancel` detector before pane-liveness checks can return early. When a machine crash or canceled editor writeback leaves the visible session document and snapshot containing the assistant response while `HEAD` still lacks it, the sync repair path performs the same narrow `git::commit(file)` recovery as preflight and proves the recoverable shape is gone before continuing with layout repair/reconcile. Coverage adds a deterministic sync repair regression for the committed-cycle/snapshot-drift shape, alongside the existing preflight recovery tests.

## 0.34.38

- **JetBrains `Run Agent Doc` now fences the full startup ready-probe from supervisor `/clear` injection (`#jbrunclear`).** Dispatch-only reroutes write a short-lived route-submit marker before the latest-run prompt-ready wait begins, not only after text injection starts, so the idle-queue supervisor cannot interleave a context-reset `/clear` or `/new` while an editor route is still proving the recovered pane is dispatch-ready. The marker records `reason=dispatch_only_ready_probe`, set/clear ops lines, and the idle watcher logs when a persisted or orphan context-clear draft waits on `reason=route_submit_in_flight`, giving the 2026-06-23 reboot shape direct proof instead of leaving a stray clear in the composer.

- **Dispatch-only reroutes no longer let a stale startup-log window override authoritative ready proof.** Operator correction on 2026-06-23: the JetBrains `Run Agent Doc` failure was not a slow boot — the Codex pane was up in under two seconds, idle, and manually usable, while route still refused with `unblocker=wait_for_dispatch_ready_prompt` because `dispatch_only_requires_ready_probe` trusted the latest open `codex_start` log and then only accepted a short live pane-capture proof. The dispatch-only startup gate now also consults the authoritative actor binding for the same session/pane and bypasses the stale startup-log wait only when the healthy supervisor reports a current-generation `Ready` actor and the normal prompt-ready barrier is satisfied (`prompt_ready`, `dispatch_ready_prompt`, or `idle_pane_reconcile`). Busy/starting/degraded/wrong-pane actors still fall through to the existing fail-closed live probe. This keeps startup reroutes prompt-gated without blocking an already-proven idle actor on a stale log tail.

- **JetBrains `Run Agent Doc` keeps a 120-second dispatch-readiness window as extra slow-start margin.** The editor action invokes `agent-doc route --dispatch-only --plain-trigger --wait-for-ready 120`, but this is not the root fix for the stale-startup-log/ready-actor dispatch bug above; it only prevents genuinely slow starts from exhausting the editor-side wait too early.

- **Release packaging now uses publishable manifests and an ordered crates.io publish path (`#98t4` / `#adpublishpkg`).** Internal `agent-doc-*` crates now carry the release version and versioned path dependencies, so Cargo can strip paths when publishing instead of rejecting path-only dependencies. `agent-doc-sqlite` and `agent-doc-orchestration` are publishable crates, `make version-sync` verifies all internal publish-unit versions match the top-level crate and PyPI metadata, and `make publish-crate` publishes `tmux-router`, internal crates, and the CLI in dependency order with crates.io index visibility waits and skip-existing behavior. The tracked manifests now use registry `agent-kit 0.4.1` and `lazily 0.12.0`; the live tmux-router API dependency is explicit as sibling `tmux-router 0.3.11`. PyPI builds now check out that sibling in CI and local publish includes sdist again, fixing the prior sdist path-dependency/lock collision shape.

- **ACK-mismatched queue-consume convergence now clears only the proven stale editor artifact (`#fcc0-ack-mismatch`).** When an active editor listener ACKs a queue-consume patch but the ACK-content does not match the intended target, the write still fails closed and refuses the external disk write. The new recovery step first proves the mismatch is the narrow stale queued-prompt blockquote artifact in `agent:exchange` with no drift outside `exchange`; only then it sends a hash/length-guarded `refresh_content` message to restore the editor buffer to the pre-consume document, preventing a later editor flush from persisting the stale queue strike. If the ACK content contains a real concurrent prompt or other non-artifact drift, the refresh is skipped and the editor-owned content is preserved. Coverage includes positive and negative queue-consume ACK-mismatch regressions, the existing editor-IPC success path, and the FlowCore reason-budget audit.

## 0.34.37

- **Supervisor restart/context-clear recovery now submits visible `/clear` drafts before queue triggers (`#clearresubmit`).** The idle-watch pending-payload detector now treats context-clear slash commands separately from `agent-doc ...` triggers, using the same active-composer evidence as explicit `session clear`: a visible Codex `/clear` or OpenCode `/new` draft is recognized only when no later idle prompt proves it already submitted. The supervisor also runs an orphan-clear recovery before the paused-queue gate, so a recycle, marker expiry, or durable `admin queue pause` cannot strand `/clear` in the input and require the operator to press Enter before the next `agent-doc <FILE>` drain. Coverage includes Codex/OpenCode active-composer detection and stale-scrollback rejection.

## 0.34.36

- **Realtime cross-editor broadcasts now deliver node-keyed patches (`#rtndsync`).** The `realtime_model::broadcast_editor_change` path no longer queues peer editor convergence as component-only replacement payloads. It now computes `node_patches` from the target peer buffer to the CRDT-merged buffer, includes peer-baseline raw/transient-normalized hashes for generation fencing, and logs node/component patch counts. JetBrains can therefore allow unrelated live-buffer drift while ACK-gating the targeted node proof; VS Code consumes the same native node patch plan under its editor-generation apply proof. Legacy component patches remain in the payload as older-plugin fallback and are skipped by current plugins for components already covered by node patches. Coverage includes the realtime payload unit test and the SimWorld two-editor broadcast convergence path.

- **Node-keyed IPC patches now carry target-node source proof before ACK (`#node-ack-merge`).** Existing-node `node_patches` (`remove`, `replace`, `move`, `strike`, `unstrike`) now include the expected target-node markdown in IPC payloads, and the shared native patcher rejects the mutation when that exact node has drifted. JetBrains uses the same native dry-run proof to bypass whole-document generation drift only for pure node-patch payloads whose targeted nodes are still current, so unrelated editor-buffer drift no longer blocks an ACK-able node merge, while stale target nodes still fail closed before ACK. Socket/file IPC payloads also carry baseline hashes for normal generation fencing. Coverage spans markdown-AST stale-node rejection, FFI drift preservation, orchestration payload JSON, and JetBrains/VS Code schema parsing.

- **Editor file-IPC patches are no longer deleted without ACK-content proof (`#ackcontent-delete`).** JetBrains and VS Code patch watchers now treat the `*.ack-content` write as part of patch success for response patches and `save_document`: if the editor cannot write the ACK-content sidecar (missing FFI/root, write failure, or failed document save), `applyPatch` returns false and the single-use patch file stays in place for binary retry instead of being deleted with only a transient editor-buffer mutation. This closes the observed stale-state/File Cache Conflict path where a live editor consumed `.agent-doc/patches/<id>.json`, failed to leave the ACK-content proof, and the binary later saw `no_ack` with no patch left to replay. Source guard tests cover both editor integrations.

- **Orphan-response repair now fails closed when the captured response does not materialize (`#mrh-response-loss-stalled-queue`).** The repair path that replays retained/captured responses now re-reads the repaired document and requires the normalized captured response block to be present before it clears the pending capture or advances the cycle. A malformed replay that only leaks body bullets into a previous response, drops the `### Re:` heading, or otherwise leaves a prompt-only tail now preserves the capture for retry instead of committing a false-success repair. Regression coverage pins the body-only/materialization-missing shape observed during the MRH/install closeout recovery.

- **Strict template closeout now rejects body-only assistant patchbacks (`#strict-re-heading`).** `finalize` / strict session-document `write --commit` paths now require a real `### Re:` response heading in `patch:exchange` or unmatched response text before response capture or visible mutation, so a stale-supervisor/IPC retry cannot commit body bullets without the assistant heading. Queue-continuation guidance now distinguishes degraded transport from stale-binary supervisors: recycle/yield the stale supervisor, then continue draining on the fresh binary.

## 0.34.35

- **`#freshqueueauth` — fresh operator queue heads stay authoritative unless an explicit removal proof exists.** `queue consume` now tells agents the safe next operation for an id-backed head: complete/gate it through closeout, explicitly acknowledge a correction head with the new `agent-doc queue consume --ack-id <id>` path, or leave it queued. `--ack-id` strikes an exact id-backed queue head while preserving the still-open backlog item, so correction/acknowledgement heads tied to open work can be cleared without falsely marking the work done. `queue prune-noise`, session-check guidance, and queue removal ops logs now use predicate/proof wording (`base_hash`, `source_component`, `operation`, `proof`) so fresh drainable operator prompts are not described as stale/noise unless the exact noise/orphan predicate was proven.

- **`#orphanqhead` — `queue prune-noise` now bulk-strikes orphan id-backed queue heads.** A `do [#id]` / `[#id]` head whose id names no open `agent:backlog` item ("orphan") was already excluded from `queue_drainable_head_count` (`head_is_drainable`), but it had no bulk removal path: `queue consume` rejects id-backed heads, `--done <id>` is a no-op, and `prune-noise` skipped anything carrying an `#id`. So the orphan sat at the queue head, was excluded from the drainable count, yet BLOCKED the leading-run `queue consume` from reaching answered free-text heads behind it — the `#qchurn` no-op loop (the live `:pushpin: [#kcb5]` repro, whose backlog item had been dropped as a `#6b5h` duplicate). Fix: `prune_noise_queue_heads` now also collects orphan id-backed head node keys (`orphan_id_queue_head_node_keys`) and strikes them alongside noise, through the same editor-IPC-converged write path. Gated on an `agent:backlog` component being present (a free-form id-head queue treats id-heads AS the work and is left alone), and preserves any id still naming open backlog work — including deferred `[operator-verify]` / `[focused-cycle]` items. Complements the existing targeted `queue consume --id <id>` escape hatch with a position-independent bulk sweep. Full suite + clippy green.

## 0.34.34

- **`#qstallguard` Layer C HOTFIX — rate-limit the paused-queue failsafe drain (it was re-flooding).** The 0.34.32 Layer C fall-through had NO rate-limit: on every supervisor idle-watch tick where a paused queue had a drainable head and no in-session loop owner, it logged `queue_paused_failsafe_single_owner_drain` and fell through — reintroducing the exact `#rt83`/`#qflood` per-tick (~2/sec) flood the pause exists to prevent (observed live: 3000+ ops.log lines; log-only — the downstream `turn_active` guard prevented actual pane dispatch, but the supervisor never reached an idle recycle boundary). Fix: the drain-owner lease is now computed ONCE before the pause gate and the failsafe CLAIMS it as `supervisor-failsafe` when it dispatches, so the gate (`paused_idle_watch_should_skip`, keyed on a fresh lease) defers every subsequent tick until the lease TTL (90s) expires — single-owner cadence (≤1 dispatch / 90s), never the per-tick flood. The dispatch this tick still proceeds because the drain decision uses the PRE-claim lease value. A fresh lease now means "in-session `/loop` owner OR the supervisor's own recent failsafe claim" — either defers. Confirmed live: flood rate dropped from ~16/8s to 0/15s after the host recycled onto this build. Full suite + clippy + `make tmux-ci` green. (Known follow-up: the drain-owner lease path is keyed on the raw doc-path string, so relative-vs-absolute callers can hash to different lease files — a `[focused-cycle]` item; within a single supervisor process the claim/read agree, so the rate-limit holds.)

## 0.34.33

- **`#qstallguard` Layer B/C interaction fix — the supervisor failsafe drain no longer false-fires the stall guard.** Layer B (`drain_stall`) drops a continuation-pending marker at a clean in-session closeout; Layer C lets the supervisor idle-watch perform a single-owner failsafe drain of a paused queue. Without coordination, when the supervisor drained (paused-failsafe OR normal go-mode), the next drained agent's preflight would see the marker with no in-session drain-owner lease and emit a spurious `queue_stall_detected` — even though the supervisor *was* actively continuing the drain (not a stall). Fix: the idle-watch clears the continuation-pending marker at its drain-`Dispatch` decision, so a supervisor-progressed drain is correctly not classified as an in-session stall. Full suite + clippy + `make tmux-ci` green.

## 0.34.32

- **`#qstallguard` — make non-stalling of a drainable queue a code-enforced invariant, not advisory prose.** A live dogfooding stall (the binary reported `queue_continuation_required=true` / `queue_drainable_head_count=1` and the agent stopped anyway) exposed that the extensive "do not stall" guidance in `SKILL.md` is advisory — an LLM can always synthesize a plausible stop reason from item prose. Three defense-in-depth layers, each pure-function unit-tested (the regression-proofing that survives refactors):
  - **Layer A — drainability is a typed attribute, never inferred.** New `[focused-cycle]` execution-context tag (`agent_doc_core::pending`): the operator's binary-read knob for "agent-doable but needs its own dedicated cycle, do not auto-drain in the loop" (e.g. merge-core / supervisor-core work needing `make tmux-ci` across live panes). `ExecutionContext::loop_undrainable()` is now the SINGLE authority for "the loop must not auto-drain this head" = `[operator-verify]` (needs a human) ∪ `[focused-cycle]`; `[clean-session]` is excluded (it drains in place, `#qcontdrain`). `deferred_backlog_ids` (continuation calc) and `partition_drainable_backlog_ids` (backlog→queue sync) both key off `loop_undrainable()`. The agent can no longer reclassify a drainable head as undrainable by reading its description — absent a tag, a drainable head is drained.
  - **Layer B — binary-detected stall guard** (`drain_stall.rs`). A clean closeout that still requires continuation drops a one-shot continuation-pending marker (`session-check`); the next preflight reconciles it and emits a hard `queue_stall_detected` warning + `ops.log` line when drainable work remained, the loop did not continue (no fresh drain-owner lease), and no valid stop reason applied (a real user prompt / `queue: stop` / drained queue — a degraded/stale supervisor, high accretion, and `semantic_completion_match` are explicitly NOT valid stop reasons). The marker is one-shot so the diagnostic fires once per stall.
  - **Layer C — pause throttles to single-owner, it does not disable the failsafe** (`start/idle_watch.rs`). An accepted `admin queue pause` is the `#rt83`/`#qflood` flood guard; it previously made the attended in-session `/loop` the ONLY drainer, so a stalled loop stranded the queue. Now the supervisor idle-watch skips a paused queue ONLY when an in-session loop owns the drain (fresh drain-owner lease) or nothing is drainable; with no loop owner and a drainable head it performs a single-owner failsafe drain (falling through to the normal `turn_active` / route-in-flight / cooldown-guarded drain decision — one dispatch per turn, never the 2/sec flood). Logs `queue_paused_failsafe_single_owner_drain`.
  - New unit tests: `focused_cycle_tag_is_loop_undrainable_but_clean_session_is_not`, the `drain_stall` suite (stall fires / no-marker inert / loop-continuation clears / each valid stop reason suppresses / degraded supervisor is not a valid stop / marker one-shot roundtrip), `paused_failsafe_drains_only_when_no_loop_owner_holds_a_drainable_head`. Full suite + clippy + `make tmux-ci` green. End-to-end SimWorld coverage of the pause-failsafe + the operator live two-pane verification are tracked as a `[focused-cycle]` follow-up.

## 0.34.31

- **`#orchver` — the stale-binary warning no longer lies "launched as 0.1.0".** `supervisor_stale_warning_message` (and the `content_ours_adoption_refused_stale_supervisor` ops.log lines it feeds) stamped the controller/supervisor version from `ControllerBinaryIdentity.version`, which was recorded via `env!("CARGO_PKG_VERSION")` **inside the `agent-doc-orchestration` crate**. That internal workspace crate is pinned at `0.1.0` and never bumped in lockstep with the top-level `agent-doc` binary (now `0.34.x`), so **every** controller/supervisor reported "launched as 0.1.0" regardless of the real build — misleading the operator into thinking an ancient binary was running when only the binary len/mtime comparison actually drives staleness (the version field is display-only; it never affected the `recorded != current` decision). Observed dogfooding on `monsterrodholders.md` (boost-client), whose long-lived `agent-doc start --route-owned` supervisor genuinely needed a recycle after a `cargo install` but reported the bogus `0.1.0`. Fix: the binary crate injects its real `CARGO_PKG_VERSION` once at `main()` startup via `project_controller::set_binary_version`; `current_binary_identity()` stamps that injected value and falls back to the orchestration crate version only for library-only callers / tests. New unit tests `identity_version_prefers_injected_binary_version` + `identity_version_falls_back_to_crate_version` cover both paths. The underlying staleness detection is unchanged — a genuinely stale supervisor is still flagged; the warning now names the true installed version so `agent-doc admin recycle` guidance is trustworthy. Full suite + clippy green.

## 0.34.30

- **`#qconvbaseline` — ROOT FIX for the "every finalize drifts while I edit the doc" race.** When a live JB plugin listener owns a document, preflight queue maintenance converges the corrected queue shape (auto-pins, backlog→queue mirrors, do-prompt sort, `queue:` control) into the **editor buffer + snapshot** via IPC with **no disk write**. But the baseline was saved (`run.rs`, just before queue maintenance) from the **pre-convergence disk** content, so at finalize the converged editor buffer differed from both the baseline and `content_ours` *outside* `exchange` — tripping `live_prompt_drift_after_preflight` on **every** cycle, which forced the `content_ours` carry-forward + a recovery `agent-doc commit`. That is the recurring race observed dogfooding this session (and a contributor to the `#editorbufwin` / `#docdriftgrace` / `#hap7` family). Fix: after queue maintenance, `realign_baseline_to_converged_queue` splices the converged **queue component** into the pre-maintenance disk content and re-saves the baseline — so `content_ours` matches the editor buffer's queue and only GENUINE concurrent user edits trip the drift guard. The splice is queue-scoped: `exchange` / boundary markers are preserved exactly, so non-queue preflights (e.g. orchestrate streaming) are untouched, and a no-convergence cycle is a no-op. New regression test `queue_convergence_realigns_baseline_so_finalize_sees_no_false_drift` proves the false drift before / clean after / and that a real concurrent user prompt still drifts. Full suite (4837) + clippy green.

## 0.34.29

- **`#kcb5` Phase 1 groundwork (`#kcb5a`): editor-less CLI finalize-wedge decision primitive (seam-isolated, not yet wired).** A pure-CLI agent-doc session (no JetBrains IDE; `controller serve` as the sole daemon) wedges every finalize: the controller hosts the editor-IPC socket even with no plugin attached, so `is_listener_active` returns true (socket connectable) while there is no editor endpoint behind it — the fail-closed disk-write guard then refuses the write (`no_ack` → `retry_without_disk_write`) and only `--force-disk` succeeds. Root cause: "socket connectable" ≠ "live editor present." This release lands the safe decision core only: `decide_editorless_disk_fallback(socket_connectable, editor_endpoint_proven, consecutive_no_ack, threshold, force_disk_requested) -> {FailClosed | ForceDiskNoEditor | ConvergeViaEditor}` in `agent-doc-orchestration::flow::document_mutation`, with the safety invariant that a PROVEN live editor still fail-closes on unproven delivery (preserves `#editorbufwin` / the FCC guard) while an editor-less / no-listener / `--force-disk` case routes to disk. Unit truth-table coverage + a `editorless_cli_sim_force_disk_but_live_editor_fail_closed` SimWorld scenario. **No live-path rewire yet** — the finalize/converge guard still behaves identically; wiring is Phase 3 (`#kcb5c`), gated behind the editor-presence signal (Phase 2 `#kcb5b`) and an operator editor-less live repro (Phase 4). Plan: `tasks/agent-doc/plan-kcb5-editorless-cli-finalize-wedge.md`. Full suite (4836) + clippy green.

## 0.34.28

- **Plugin-side reconnect re-read: a stale editor buffer no longer reverts the binary's committed writes (`#yzer` / `#evmhplugin`, the plugin half of `#evmh`).** When the JB plugin was disconnected from IPC (supervisor down, plugin/cdylib reload) the binary may have committed control-plane content to disk/HEAD, leaving the open editor buffer stale. On the next `save_document` the stale buffer would overwrite HEAD — the `#postcommit-ipc-worktree-corruption` direction. The plugin now reconciles on IPC (re)connect: in `PatchWatcher.registerRoot`, after the socket listener starts, it walks every open `.md` session document under that root and asks the new binary FFI `agent_doc_reconnect_buffer_decision(root, file, buffer)` whether the buffer is stale. The decision is owned by the binary (pure `decide_reconnect_buffer` in `agent-doc-orchestration::flow::document_mutation`): it re-reads disk only when the buffer is **provably** stale — it equals a recent prior commit of the file (`ffi_show_prior_blobs`) **and** disk equals clean `HEAD` (`ffi_show_head`). Otherwise it keeps the buffer, so genuine unsynced user edits are never clobbered (editor wins, per `#editorbufwin`). A `reread_disk` decision carries the disk content; the plugin applies it via `applyReconnectReread` (re-checks the live editor generation, `setText` + `saveDocument` to clear the dirty flag). Emits a `reconnect_buffer_decision decision=... #yzer` marker to `ops.log` for live verification. New tests: `decide_reconnect_buffer` unit coverage (in_sync / reread / keep) plus the `reconnect_buffer_sim_rereads_stale_then_keeps_user_edits` SimWorld scenario (re-read a prior-commit buffer, keep an offline-edited buffer). Binary half (`reset --from-current` converge seam) shipped in 0.34.27. Full suite (4832) + clippy green. **Live reconnect verification is operator-gated** (needs a real editor disconnect/reconnect).

## 0.34.27

- **`reset --from-current` resume-clear now routes through the listener-guarded converge seam (`#evmh` / `#cyh0`).** The default `reset --from-current` (without `--preserve-session`) clears the `resume` frontmatter pointer and rewrote the session document with a bare unguarded `std::fs::write`. When a live JB editor listener held the document open, that disk write diverged the editor buffer from disk and raised a `File Cache Conflict` — one of the recovery-path FCC triggers diagnosed in the agent-doc-bugs2 dogfooding session (the others — `apply_compacted_document` via `#w42v`, the post-commit worktree reconcile, and `write.rs` `atomic_write` — were already listener-guarded or are the intentional `--force-disk`/IPC-unavailable fallback). The resume-clear write now goes through `agent_doc_orchestration::write::converge_or_disk_write(..., "reset_resume_clear")`: a live editor listener converges the change through the buffer (no FCC), and with no listener it falls back to the same CLI disk write as before, so headless `reset` is byte-identical. `reset --from-current --preserve-session` already only rebuilt sidecars and never touched the document, so it was never a trigger. New regression test `reset::tests::from_current_routes_resume_clear_through_converge_seam` asserts the resume-clear write is source-labelled in `ops.log` (`reset_resume_clear_writeback ... transport=disk_fallback`), proving it routes through the seam rather than a bare write; existing reset tests confirm the headless path is unchanged. Full suite + clippy green. (The plugin-side reconnect "re-read disk/HEAD when the editor buffer is stale" half of `#evmh` remains separate Kotlin/FFI follow-up work.)

## 0.34.26

- **Operator-deleted structure the agent targeted is now surfaced in `agent:exchange` (`#hap7` / `#qdup`, deleted-structure rule).** Second half of the scoped-merge no-structural-duplication fix. The node-keyed `semantic_merge` already prevents the operator-reported queue-prompt *duplication* (a concurrent operator queue edit can no longer duplicate/reverse/drop adjacent structure — see regression tests `qdup_operator_queue_add_during_exchange_turn_no_duplication` and `qdup_one_changed_node_leaves_siblings_byte_identical`). The remaining gap was the plan's deleted-structure rule: when the operator **deletes** a node the agent's content this cycle targeted (an `OperatorDeletedAgentEditedNode` outcome), the deletion correctly stood (node never resurrected) but the dropped agent edit was only carried forward as a next-cycle ack — and the live-prompt-drift convergence scopes acks to the `exchange` active area, so a queue/backlog deletion ack could be silently dropped. Now `semantic_merge` records each such fact as an `exchange_notes` entry and injects a one-line blockquote note into the merged `agent:exchange` component (before a trailing boundary marker if present), so the operator sees the dropped agent edit **this** cycle, independent of the scoped ack carry-forward. The note is a blockquote (never a `### ` heading or `❯` prompt) so it cannot be misclassified as a response turn or user prompt by the convergence/drift gates; injection is idempotent (a note already present in the body is not re-added) and a no-op when no `exchange` component exists. The in-editor document remains the source of truth — the agent change is never merged back. New unit tests in `semantic_merge.rs` (`qdup_*`) plus SimWorld end-to-end coverage (`hap7_sim_operator_queue_add_during_exchange_turn_no_duplication`, `hap7_sim_operator_deleted_agent_targeted_node_noted_in_exchange`). Full suite + clippy green.

## 0.34.25

- **Answered free-text queue heads with a pasted code-fence log are now struck (`#ftstrike-fence`).** Operator-reported: answered free-text queue items (e.g. `JB Run Agent Doc on equityfundingsource.md did not submit` followed by a fenced route/console log) stayed unstruck in the queue forever, even though the response quoted and addressed them. Root cause: the position-independent answered-free-text strike (`#ftstrike`, `strike_answered_free_text_queue_heads`) matched a head by checking whether its *entire* normalized node text appeared inside the response's quoted-prompt blockquotes. For a head whose body is dominated by a pasted log, that whole-text key can never appear in a blockquote (nobody quotes the full log back), so `free_text_head_answered_by_response` always returned false and the head was never struck — it then fell behind newer heads and orphaned. Fix: match on the head's **prose prefix** (every line before the first ` ``` ` / `~~~` fence) via the new `free_text_head_match_prose`, so a code-fenced report strikes when its prose lead is quoted. The ≥4-significant-word guard and the blockquote-only requirement are preserved (a head that is all log, with an empty prose prefix, still never matches — no false strikes). Regression test `code_fenced_free_text_head_strikes_on_prose_lead_match`; full suite + clippy green.

## 0.34.24

- **Stale-binary recycle-yield: a self-draining `/loop` now yields one boundary so the supervisor can hot-reload onto a freshly-installed binary (`#wd40` / `#staleloop-recycle-restart`).** A continuously self-draining Claude Code `/loop` holds a fresh drain-owner lease AND keeps the harness `turn_active` back-to-back, so the route-owned supervisor never reaches its turn-boundary recycle and a freshly-installed binary never hot-reloads — the root of the recurring `content_ours` finalize drift + `#rt83` phantom-pin flood seen when dogfooding across a mid-session `cargo install`. Previously the operator had to manually `make install` + `agent-doc admin recycle` + end-turn to force the boundary. This automates it: when the supervisor idle-watch detects its own binary is stale AND a self-driving loop owns the drain AND a recycle WOULD fire at a boundary (not a bare `Detect`, and not after the Phase-3 kill+relaunch escalation is exhausted), it writes a short-TTL per-document recycle-yield request sidecar (`.agent-doc/recycle-yield/<hash>.json`, default 120s TTL, `AGENT_DOC_RECYCLE_YIELD_TTL_SECS` override). While that request is live, `queue_continuation::detect`, `preflight`, and `session-check` drop `queue_continuation_required` and surface `RECYCLE_YIELD_GUIDANCE` (an intentional, temporary yield — NOT a drained queue or a stop reason), so the in-session loop ends its turn cleanly; the idle boundary lets the `execve` recycle fire on its own, and the fresh (no-longer-stale) supervisor clears the request so the drain resumes on the new binary. Mid-turn `execve` stays out of scope — the yield is exactly what produces a clean boundary without a mid-write swap. New module `recycle_yield.rs` (request producer/reader/clear + pure freshness predicate) mirrors the `drain_owner` sidecar layout; pure policy `decisions::stale_drain_recycle_yield_requested` is unit-tested via truth table; the supervisor's own idle-watch drain uses `live_drainable_continuation_head` (not this), so it is unaffected and resumes after recycling. Full suite + clippy green.
- **Conventions: `#deploy-just-do-it` — agents execute every agent-doable release/deploy sub-step (version bump, `VERSIONS.md`, `make check`, commit, install + `lib-install`, push, `admin recycle`, tag, publish) without asking; only the live human eyeball is operator-gated, recorded as a non-blocking `[operator-verify]` follow-up.** Replaces the old "manual testing gate" that blocked publishing on operator confirmation. Documented in `AGENTS.md`.

## 0.34.23

- **Route trigger injections now emit a `dispatch_inject attempt=N` ops.log marker so a post-restart multi-inject regression is provable from logs (`#rdypoll` §D / img_52).** Operator-reported (2026-06-20): after restarting an agent-doc session, JB `Run Agent Doc` typed the `agent-doc <FILE>` trigger ~7 times into the harness composer with none submitted; on retry it worked, but "the restart state should not lag." The readiness gates that prevent the stacking landed in 0.34.21 (`#jbtsiftnosub` cold-start) and 0.34.22 (`#runexitrestart` restart-drain), but there was **no log marker proving how many times the trigger was actually injected** — so an operator who hit the duplicate stacking could not prove/disprove from `ops.log` whether a given dispatch re-typed. This adds a `dispatch_inject file=… pane=… harness=… transport=<direct_pane|supervisor_ipc> attempt=N` marker at both real injection funnels (`send_command_once_unchecked` direct-pane text+Enter, `dispatch_via_supervisor_ipc_with_mode` IPC inject) keyed off a process-global monotonic counter. A healthy dispatch logs `attempt=1` exactly once; a multi-inject regression (or a legitimate but visible `not_dispatched` full-trigger resend) shows `attempt=2`, `attempt=3`, … making the stacking class directly auditable. The route process is short-lived (one logical dispatch per `agent-doc route` invocation), so the monotonic counter cleanly answers "did this dispatch type the trigger more than once?" SimWorld coverage: `route_sim_restart_drain_waits_for_dispatch_ready_prompt_before_send` now also asserts the `dispatch_inject` marker is **absent** across all 7 not-ready restart ticks (`dispatch_injects == 0`) and present exactly once as `attempt=1` after `PromoteStartingPromptReady` — never `attempt=2` (a new `dispatch_injects` coverage counter mirrors the production marker through the model's accept-dispatch seam). Full suite + clippy green. **Live-verify (operator):** the two-pane restart repro (restart a session, JB `Run Agent Doc`, then check `ops.log` shows a single `dispatch_inject attempt=1`) and the `cargo install` / `admin recycle` deploy are operator-gated.

## 0.34.22

- **The supervisor idle-watch queue-drain now waits for the harness dispatch-ready prompt after a session RESTART, closing the restart variant of the JB `Run Agent Doc` no-submit-duplicate race (`#runexitrestart`).** Operator-reported (2026-06-20): after RESTARTING an agent-doc session, then JB `Run Agent Doc` on `equityfundingsource.md`, the `agent-doc <FILE>` trigger was typed ~7 times into the harness composer with none submitted. This is the RESTART sibling of the cold AUTO-START race `#jbtsiftnosub` fixed in 0.34.21: the route auto-start path (`route::startup`) and both existing-pane route paths (`dispatch_only_send_reopen` requires_ready_probe, `ensure_existing_pane_ready_for_dispatch`) already gate dispatch behind `wait_for_agent_ready_outcome` / `ready_prompt_candidate` (the strong `is_dispatch_ready_prompt_line` predicate), but the **supervisor idle-watch drain loop** (`idle_watch.rs`, the one looping site that can re-type each tick) gates on `idle_queue_prompt_visible`, which off the `actor_state == Ready` fast path falls back to the *weak* `child_output_prompt_visible` → `matches_prompt`. On a fresh restart the actor is `Starting`, the edge-triggered pty `terminal_screen` buffer can render a prompt *glyph* (matching `matches_prompt`) while the restarted composer is not yet submit-ready, so the per-idle-tick drain re-injected the trigger into a not-ready composer (Enter never submits) and each tick stacked another un-submitted copy — the operator's ~7 duplicates. The `#qflood2` pre-send dedup (`supervisor_pane_payload_already_pending`) cannot reliably catch a partially-rendered restarting composer, so it did not suppress the stacking. Fix: `idle_queue_prompt_visible` now, when the actor is NOT yet `Ready` but the weak pty-buffer signal is positive, re-verifies against a **fresh tmux capture** of the owned pane via the new `supervisor_pane_dispatch_ready` helper (mirroring `supervisor_pane_has_busy_cue`'s live-capture pattern) using the same canonical `route::ready_prompt_candidate` / `is_dispatch_ready_prompt_line` predicate the route and cold-start gates use. A fresh capture that proves a submit-ready empty composer dispatches; one that shows only a not-yet-ready glyph fails closed and defers the drain this tick (no trigger typed, nothing to re-stack); an unreadable/absent capture (`None`) conservatively falls back to the prior pty-buffer signal so a transient capture failure never permanently suppresses a legitimate drain. The `actor_state == Ready` fast path and the OpenCode/Codex idle-chrome paths are unchanged; `idle_queue_prompt_visible` has exactly one production caller (the idle-watch drain), bounding the blast radius. SimWorld coverage: `route_sim_restart_drain_waits_for_dispatch_ready_prompt_before_send` (new `DispatchIdleQueueDrainAfterRestart` command + `drain_into_restarting_pane_blocks` coverage) restarts a ready session to `Starting`, then asserts the idle-watch drain fails closed on **all 7** ticks (records `dispatch_into_restarting_pane`, `route_dispatch_acceptances == 0`, `go_drain_dispatches == 0` — no duplicate triggers), then dispatches exactly once after `PromoteStartingPromptReady`. Full suite + clippy green. **Live-verify (operator):** the two-pane restart repro (restart a session, JB `Run Agent Doc`) and the `cargo install` / `admin recycle` deploy are operator-gated.

## 0.34.21

- **Three merge/commit-core safety fixes that cascaded in a live degraded session: compaction overlay-CRDT staleness, reset queue-journal clear, and queue-consume head-divergence reconcile (`#editorbufwin` Fix A).** (1) **Compaction overlay-CRDT staleness** (`compact.rs`): a CRDT-mode `compact --commit` no longer leaves the overlay CRDT carrying the PRE-compaction (large) markdown. The early template-branch `save_document_crdt(file, &compact(&crdt_state), &content)` was the defect — it saved the overlay with the large `&content`, so later cycles re-projected (`load_overlay_crdt`→`to_markdown`) snapshot(large) > visible(small) and tripped `guard_no_stale_snapshot_reset_drift`'s "looks like a manual cleanup" refusal. That early stale save is removed; `apply_compacted_document(..., refresh_crdt=true)` is now the single authoritative CRDT writer and rebuilds the overlay from the COMPACTED text (fresh `CrdtDoc::from_text` / `OverlayCrdtDoc::from_markdown`, which also supersedes the old tombstone-GC step since fresh docs carry no tombstones). (2) **Reset queue-journal clear** (`reset.rs`): `reset --from-current [--preserve-session]` now clears the crash-durability queue journal (`queue_journal::clear`, mirroring the commit-time clear) after rebuilding the sidecars. The rebuilt snapshot/baseline IS the new durable queue baseline, so a pre-reset journal window (heads recorded while older prompts were live) is superseded — without this, answered+compacted heads would be re-inserted by `queue_journal::replay_missing` at the next `start` and resurface over the current queue. (3) **Queue-consume head divergence** (`write/queue_consume.rs`, `#editorbufwin` Fix A): the snapshot/content_ours head is the OLD head (the live user queue addition is deliberately NOT absorbed into content_ours), while the document head read fresh from disk is the user's live editor-buffer addition, so a benign live editor buffer made the head-equality check hard-bail EVERY cycle (the remaining-queue check below already tolerated this kind of divergence). The head check now reconciles — log `queue_consume_head_divergence_reconciled reason=live_buffer_addition_authoritative` and proceed using the DOCUMENT head as authoritative — but ONLY when the divergence is explained by recorded dropped-queue evidence (`cycle_state::dropped_queue_prompts`, written by the ipc write path); with no evidence it keeps the hard-bail as a corruption guard. content_ours/snapshot composition is untouched (the `ipc_live_prompt_drift_content_ours_ignores_unproven_live_queue_deletions` invariant still passes). Tests: `compact_advances_snapshot_and_crdt_so_next_preflight_does_not_refuse`, `preserve_session_clears_stale_queue_journal_so_compacted_heads_do_not_resurface`, `queue_consume_head_divergence_reconciles_with_dropped_queue_evidence` (+ negative `…_without_evidence_still_bails`). Full suite + clippy green. **Live (operator):** the `cargo install` / `admin recycle` deploy and the live zero-drift proof against a running route-owned supervisor remain operator-gated.

## Unreleased

- **Busy `session clear` now queues one deferred clear and dedupes repeats (`#p6a0`).** A non-interrupting `agent-doc session clear` against a busy active auto-loop now records a single deferred clear for the supervisor's next proven idle boundary instead of asking the operator to retry. Repeated clears while that marker is pending report the already-deferred state and do not refresh the marker, extend cooldown, or inject another `/clear` into the active turn. Coverage adds queue-preemption, session-clear message, and SimWorld regressions.

- **Stale-supervisor self-recycle proof now covers the File Cache Conflict refusal loop (`#fccsup`).** Added a regression that ties the default-on queue-boundary recycle policy to the host-supervisor inode guard: a stale supervisor with a pending queue head must choose `RecycleImmediate`, and once `supervisor_binary_stale_self_recycled` maps the installed inode, the stale-supervisor `content_ours` refusal guard is no longer eligible. Also corrected stale code comments that still described supervisor auto-recycle as default-off.

- **Supervisor idle-queue submits now defer while an editor is actively typing the queue head (`#jbtypingguard`).** Operator-reported via JetBrains `Run Agent Doc`: a binary-owned auto continuation could add `/clear` plus `agent-doc tasks/...` while the operator was still typing in `agent:queue`, racing the manual JB route that submits the absolute-path `agent-doc /.../<FILE>` trigger and leaving both prompts unsubmitted. Root cause: preflight already honored the cross-process typing sidecar, but the long-lived supervisor idle-queue watcher only gated on route-in-flight, clear-settle, queue-edit leases, and pane idleness; it did not treat live editor typing as input ownership, and route-owned supervisors can hold a relative document path while JetBrains records typing against the absolute path. Fix: idle-watch checks both the current and resolved absolute document paths for the typing sidecar, logs `idle_queue_watch_skipped ... reason=editor_typing_active`, and the reset/drain decision policy now has explicit `SkipEditorTyping` outcomes. Coverage: pure reset/drain decision tests plus `idle_queue_typing_guard_checks_absolute_editor_path`.

- **Codex/Claude/OpenCode direct-pane routed dispatch now retries submit at least once per second while the `agent-doc <FILE>` trigger remains visibly drafted (`#jbcodexsubmit` / `#jbclaudesubmit`).** Operator-reported via JB `Run Agent Doc`: Codex routes were either not submitting or appeared very slow because the Enter retry loop waited a full 5s submit-acceptance window before nudging again. The acceptance window is now 1s, the default Enter retry cap is 30 attempts to preserve roughly 30s of recovery, and the cap remains env-tunable through `AGENT_DOC_DIRECT_PANE_MAX_ENTER_RESUBMITS`.

- **A stale-binary supervisor now auto-recycles during a continuously self-draining session by asking the in-session loop to yield one boundary (`#wd40` / `#staleloop-recycle-restart`).** The supervisor hot-reloads onto a freshly-installed binary only at a turn boundary (`prompt_visible && !turn_active`; see `supervisor_recycle_action`). A long in-session Claude Code `/loop` drain holds a fresh drain-owner lease AND keeps the harness `turn_active` back-to-back, so the supervisor never reaches that boundary — a freshly-installed binary never hot-reloads and the stale supervisor persists for the whole session (the root of the `content_ours` finalize drift + `#rt83` phantom-pin flood, since `#supselfheal` already notes a stale *binary* does not self-heal by lease expiry; it needed a manual `make install` + `admin recycle` + end-turn). Fix: when `idle_watch` detects its own binary is stale, a self-driving loop owns the drain, and a recycle WOULD fire at a boundary, it writes a short-TTL recycle-yield request sidecar (`.agent-doc/recycle-yield/<hash>.json`, new `recycle_yield` module, default 120s TTL, `AGENT_DOC_RECYCLE_YIELD_TTL_SECS` override). The attended in-session loop reads it at its next inter-item boundary — `queue_continuation::detect` returns no continuation, `session-check` prints `queue_recycle_yield=true` + the new `RECYCLE_YIELD_GUIDANCE`, and preflight drops `queue_continuation_required` with the same guidance — and yields one boundary instead of re-triggering. The resulting idle turn lets the `execve` recycle fire on its own; the fresh (no-longer-stale) supervisor clears the request and the drain resumes on the new binary (the loop may `agent-doc drain-claim <FILE> --release` to hand back immediately rather than wait for the lease TTL). The supervisor's OWN idle-watch drain uses `live_drainable_continuation_head` (not `detect`), so it is unaffected and resumes the drain after recycling. A bare `Detect` (auto-recycle opted out, no admin/wedge) does not request a yield (it would only stall the drain), and an exhausted Phase-3 kill+relaunch escalation does not yield-loop. New `ops.log`/session-log marker `supervisor_recycle_yield_requested` (`reason=stale_binary_drain action=signal_loop_yield`) proves the request live. Mid-turn `execve` stays OUT of scope — the supervisor owns the in-flight cycle CRDT/write-queue/IPC state (rebuilt fresh after re-exec); the yield is exactly what produces a clean boundary. New pure decision `stale_drain_recycle_yield_requested` (unit-tested truth table) + `recycle_yield` module unit tests + `detect_yields_when_supervisor_requests_recycle_yield` integration test; full suite + clippy green. **Live-supervisor-critical (operator):** the two-pane `make tmux-ci` self-recycle repro and the `cargo install` deploy are operator-gated. Plan: review item `#wd40`.

- **Auto-start route dispatch now waits for the harness dispatch-ready prompt before sending, closing the JB `Run Agent Doc` cold-start race (`#jbtsiftnosub`).** Operator-reported (2026-06-19): JB `Run Agent Doc` auto-started a fresh supervisor/tmux pane, typed the `agent-doc <FILE>` trigger into the Claude composer, but did NOT submit it. Root cause: a cold-start race distinct from the crashed-harness case (`#1vhn`/issue A). The "starting actor reroutes are prompt-gated" invariant already promotes a `starting` actor to `ready` only after a harness-specific dispatch-ready prompt is observed, but the **auto-start** path (`route::startup::auto_start_ext`) that creates a fresh pane did not hold the actual send behind that same gate: after `wait_for_agent_ready` proved a (possibly transient) dispatch-ready prompt while the Claude TUI was still coming up, the path went straight to `dispatch_routed_reopen` with no re-verify immediately before the send, so the trigger keystrokes could land in a not-yet-submit-ready composer and the Enter never registered as a submitted prompt. Fix: a new `reverify_auto_start_dispatch_ready` bounded-poll gate (`AUTO_START_DISPATCH_READY_REVERIFY_TIMEOUT`, 5s) runs immediately before the managed auto-start send; it re-captures the fresh pane and proceeds only when `ready_prompt_candidate` still proves a dispatch-ready harness prompt. If the bound elapses while the pane is still cold-starting it fails closed with claim/restart guidance and logs `dispatch_into_starting_pane` (`reason=harness_not_dispatch_ready_before_auto_start_send`); a pane that has dropped to a bare interactive shell during cold-start is distinguished and logged as `dispatch_into_shell` (the issue-A signature). The new `auto_start_dispatch_ready_block` helper classifies the pane state (`StartingPane` vs `DeadShell`) so the diagnostic distinguishes the cold-start race from the crashed-harness case from a normal dispatch. SimWorld coverage: `route_sim_auto_start_dispatch_waits_for_dispatch_ready_prompt_before_send` (new `DispatchAutoStartRoutePrompt` command + `auto_start_starting_pane_blocks` coverage) asserts that an auto-start dispatch into a still-`Starting` pane fails closed and records `dispatch_into_starting_pane`, then dispatches and proves submitted once the dispatch-ready prompt is observed. Full suite + clippy green. Plan: `tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md` (Section C). **Live-verify (operator):** the two-pane cold-start repro (JB `Run Agent Doc` auto-starting a fresh pane) and the `cargo install` deploy are operator-gated.

- **Per-node semantic merge now preserves free-text / fenced queue heads, fixing the persistent `live_prompt_drift` editor-IPC convergence failure (`#qdup-freetext`).** Root cause of the recurring degraded session where **every** write fails `ack_mismatch` / `live_prompt_drift_after_preflight` (557 consecutive blocked events observed on `agent-doc-bugs2.md`, **zero** `live_prompt_drift_semantic_merged` successes): the node-keyed `semantic_merge` reconstructs a non-`exchange` component **only** from its `- ` bullet list items. A queue head that is multi-line *free text* — an operator-pasted console block (`:pushpin:` line + a fenced ```` ``` ```` block between `---` rules) — is not a bullet item, so `overlay::components` never parses it as an `Item`. `merge_components_into_body` then emitted only the bullet items and **dropped** every other inner line, so the merge silently lost the free-text head, tripped its own `dropped_queue_prompt_lines_after_content_ours` anti-data-loss gate, declined the merge **on every cycle**, and left every IPC write stuck on the blocked `content_ours` carry-forward path — surfacing to the operator as permanent buffer corruption / queue churn that "only an IDE file reload clears." Fix: a new `merge_nonexchange_inner` preserves operator non-bullet inner content (free-text heads, fenced code blocks, `---` separators, blanks) verbatim — tracking fenced spans so a `- ` inside a fence is not mistaken for a bullet — while still replacing the bullet-item region with the merged item set (placed at the first bullet position; appended after the prose when the component has no operator bullets). This generalizes the buffering already used for `exchange` heading-prose turns to all components, so a disjoint operator/agent edit around a free-text head now converges instead of blocking. Tests: `freetext_fenced_queue_head_survives_per_node_merge` (markdown-ast unit) and `smconv_preserves_freetext_fenced_queue_head_on_drift` (IPC convergence: the guard now adopts the merge instead of blocking, head preserved verbatim); FlowCore hot-path token budget updated (`ipc.rs` `guard_` 16→17, the new test's guard call); full suite (4807) + clippy green. Plan: `tasks/agent-doc/plan-scoped-crdt-merge-no-structural-duplication.md` (`#hap7`/`#qdup`).

- **Log timestamps are now human-readable ISO-8601 UTC (`#opslogts`).** Operator request: "ops.logs should have each entry contain a [readable] timestamp." Every operational log entry was prefixed with a bare Unix epoch (`[1781771180]`), which is illegible when reading the supervisor session log / `ops.log` to verify reported issues (e.g. correlating the `#tsiftmdcrash` SIGTERM to wall-clock time). New `agent_doc_core::log_time` module formats epochs as `YYYY-MM-DDTHH:MM:SSZ` and parses them back, with **no external date dependency** (Howard Hinnant's civil-date algorithms). All writers now emit ISO: `ops.log` (`ops_log`), the supervisor session log (`start::log_event`, `startup_miss`), cycles.jsonl (`iso_timestamp`), and the `/tmp` write-dedup / sync debug logs. Crucially, `parse_log_timestamp` is **backward-compatible** — it accepts both a bare epoch and ISO — so every timestamp **reader** keeps working across the switch: the staleness/accretion windows (`session_accretion`), startup-miss windows (`startup_miss`), and the `gate_verify` ops.log scanner (incl. the `s760_clear_decision_clear_true` verifier, which compares marker times to `set_at`). The helper lives in `agent-doc-core` so the core `gate_verify` scanner and the orchestration writers share one implementation. Tests: `log_time` known-vector + epoch/ISO round-trip (incl. leap day) + garbage-rejection, and the `ops.log` integration test now asserts an ISO bracket that round-trips. SPEC (`specs/supervisor.md`, `specs/07-core-commands.md`, `specs/07-closeout-commands.md`) updated.

- **Capability-proof give-up no longer SIGTERMs the live hosted harness child (`#tsiftmdcrash`) — root-fix for the "tsift.md turn crashed and killed the session while the tmux pane stayed alive" report.** Root cause, found in `tsift-v0.1.log`: the managed OpenCode session on pane `%78` was killed **twice** with `opencode_exit code=143` (143 = 128+15 = SIGTERM), each time at the same instant as `opencode_capability_proof status=failed attempts=3` ("opencode child network probe timed out after 45s"). The capability-proof thread's `GiveUp` branch called `shared.kill_child()` (`start.rs:1844`), SIGTERM-ing the **live interactive harness the operator was actively using** because a *separate background* `opencode run` network-probe child timed out (a false negative — the real TUI was "Thinking normally"). The supervisor owns that kill, so it stayed alive and the pane stayed active, then auto-restarted the child 2s later — exactly the operator's "crash that killed the session process while the pane stayed active" symptom. The kill was also redundant: the `Failed` gate already blocks **all** prompt dispatch via `capability_dispatch_blocker`, so no unsafe work can be auto-dispatched even with the child alive. Fix: the `GiveUp` branch no longer kills the child — it keeps the gate `Failed` (dispatch disabled), marks the actor `Blocked`, surfaces the diagnostic, and logs `<harness>_capability_proof_live_child_preserved reason=dispatch_gated_not_killed`. The operator's live session survives; they fix the environment / stop / restart to re-prove. Test: `failed_capability_proof_gate_blocks_dispatch_so_live_child_need_not_be_killed` (locks in that the `Failed` gate is itself the complete dispatch block that makes preserving the child safe). SPEC (`specs/codex-support.md`) + README updated. **Live-verify (operator):** on a managed OpenCode/Codex session, force a proof failure (e.g. network blip) and confirm the pane's harness stays alive with `…_capability_proof_live_child_preserved` in the session log and **no** `exit_code=143` kill.

- **Closeout now reaps dead-PID live-buffer sidecars, not just patch files (`#lbreap`).** The `#sqdrift` storm only happened because closed-IntelliJ orphan live-buffer sidecars accumulated unbounded (this doc had 187). `#fccreap` reaped dead-pid *patch* files at closeout but never the *live-buffer* sidecars, and `#sqdrift` reaps a dead peer only when a broadcast happens to touch it. New `reap_stale_jetbrains_live_buffers` runs in the post-commit closeout (beside `reap_stale_jetbrains_consumers`): it removes `.agent-doc/live-buffer/<stem>.jetbrains-<pid>-<uuid>` sidecars whose embedded pid is provably dead, never touching legacy no-editor-id or non-JetBrains sidecars. So orphans self-clear every cycle instead of piling up into a broadcast storm. Tests: `jetbrains_live_buffer_pid_parses_pid_from_sidecar_name`, `reap_removes_only_dead_pid_live_buffer_sidecars`, `reap_live_buffers_is_noop_on_missing_dir`; full suite (4802) + clippy green.

- **Realtime cross-editor broadcast no longer storms patches to dead editors (`#sqdrift` / `#fccreap2`) — root-fix for the recurring degraded session.** Root cause of the per-finalize `live_prompt_drift_after_preflight` + `postcommit_worktree_check match=false` degraded session: `realtime_model::broadcast_editor_change` built its peer set from **every** `live-buffer` sidecar with no liveness check, so a pile of closed-IntelliJ orphan sidecars (one per past window, accumulated over days) each became a broadcast target — and the broadcast was even being triggered with a **dead originator**. Observed live: 247 `realtime_broadcast_queued` events in a few minutes fanning out to ~22 dead `jetbrains-<pid>` consumers (all pids dead, no live IntelliJ), each (a) re-creating the dead-pid patch file the `#fccreap` reaper had just cleared and (b) merging against that dead editor's *divergent stale buffer* (merged_len 6300…73989), one of which then leaked into the finalize IPC-proof path as the drift candidate. Fix: `broadcast_editor_change` now liveness-filters the originator (a dead-pid origin skips the whole broadcast) and the peers (dead-pid peers are dropped **and** their orphan live-buffer sidecars reaped via `clear_live_buffer_for_editor`), so a dead editor is never a broadcast origin or target and the orphan sidecars self-heal. JetBrains ids carry the owning pid (`jetbrains-<pid>-<uuid>`); non-JetBrains ids (no embedded pid) are conservatively treated as live. Logs `realtime_broadcast_skipped reason=dead_origin_editor` and `realtime_broadcast_dead_peers_reaped count=<n>`. Tests: `editor_id_is_live_filters_dead_jetbrains_pids_only`, `broadcast_editor_change_skips_dead_origin`, `broadcast_editor_change_drops_and_reaps_dead_peer`; full suite (4799) + clippy green. (Separate from the stale-binary-supervisor self-heal `#supselfheal`, which the swapped-binary timing also exercised.)

- **Direct queue edits hold a queue-edit lease that preflight + the idle-watch defer to (`#sqedit-race` Phase 2).** The live-IPC-supervisor race on direct queue edits is the compounding of three concurrent queue writers (multiple plugin consumers → Phase 1 `#8bfz`, already shipped; preflight queue maintenance; supervisor idle-watch) observing a *torn intermediate* queue mid-edit and round-tripping it into corruption. Phase 2 adds the writer-side single-writer guarantee for the direct queue-edit commands: a new per-document `.agent-doc/queue-edit-owner/<hash>.json` lease (short self-healing TTL, default 15s; mirrors the `drain-owner` lease) is held for the whole `agent-doc queue prune-noise` / `agent-doc queue consume` read-modify-write via an RAII `QueueEditGuard` (released on drop, incl. early return / error). The two *other* concurrent queue writers now defer while a **different, live** process holds a fresh lease: `run_queue_maintenance` returns early without mutating (logs `queue_maintenance_deferred reason=queue_edit_lease holder_pid=<pid>`), and the supervisor idle-queue-watch skips the dispatch tick (logs `idle_queue_watch_drain_skipped reason=queue_edit_in_flight`). The short TTL makes this a brief yield, not a stall — the edit settles and the next preflight/tick proceeds normally on the clean queue. New `queue_edit_owner` module with the lease primitive, freshness predicate, foreign-holder detection (different-pid + fresh + live), and RAII guard. Tests: 5 `queue_edit_owner` unit tests + `run_queue_maintenance_defers_while_foreign_queue_edit_lease_held` (proves no mutation under a foreign lease, then resumes once cleared); FlowCore hot-path token budget updated (`maintenance.rs` `reason=` 3→4); full suite (4796) + clippy green. Remaining: Phase 3 (idempotent malformed-entry normalization — partly landed via `#qdup-bare-id`/`#qnoise-multiline-strike`/`#pushpinaccum`) and Phase 4 (operator-gated `make install` → `admin recycle` → live `prune-noise` no-reinjection proof). Plan: `tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md`.

- **Routed dispatch detects a prompt that never landed and re-sends the full trigger (`#jbrundispatch` directive 2).** Operator directive on the "killed the pane + autostarted a new pane → Run Agent Doc stalled" report: *"the supervisor should detect if the prompt was not dispatched into the session, and send the prompt and submit the prompt."* Root cause: `poll_direct_pane_acceptance` treated an empty composer as a successful submit even when the trigger was **never** observed there — so a send that silently no-op'd into a not-ready pane (the pane-kill+restart case) was misreported as `Accepted`, and nothing re-dispatched. Fix: a new `not_dispatched` outcome — set only when the trigger was never seen in the composer AND the pane is sitting at an **idle dispatch-ready prompt** (`pane_idle_dispatch_ready`, reusing `is_dispatch_ready_prompt_line`). For an agent-doc trigger that starts a turn, a genuine submit leaves the pane *processing* (not idle), so empty+idle+never-seen reliably means non-dispatch and is safe from re-sending a real submit (which would double-run the agent). `send_command_unchecked` now re-sends the **full** trigger (text+Enter, not a bare Enter — there's no draft to submit) up to `direct_pane_max_enter_resubmits()` times until it lands, logging `route_redispatch_not_landed`, and reports a genuine `TimedOut` (not a false `Accepted`) if the budget exhausts. Tests: `pane_idle_dispatch_ready_distinguishes_non_dispatch_from_fast_submit`; dispatch suite (130) + `make check` green. Pairs with `#jbclaudesubmit` (retry-until-submitted) to close both halves of `#jbrundispatch`; both still want live verification of the actual failing pane state.

- **Routed-dispatch retries Enter until the trigger is submitted, with a higher + env-tunable budget (`#jbclaudesubmit`).** Operator directive on the "some JB `Run Agent Doc` ops don't submit to Claude Code" report: *"the supervisor should retry until the prompt is submitted."* The direct-pane submit path already re-sends a bare Enter while the routed trigger stays drafted in the composer (`send_direct_pane_enter_resubmit_until_stable`), exiting the moment the trigger is consumed — but the cap was a fixed `DIRECT_PANE_MAX_ENTER_RESUBMITS = 3`. Because Claude Code has no submit-proof hook (dispatch is accepted-only — text+Enter delivered without confirmation), a slow-to-focus composer could exhaust the 3-nudge budget before it consumed the Enter, leaving the trigger sitting unsent. Raised the default to 6 and made it env-tunable via `AGENT_DOC_DIRECT_PANE_MAX_ENTER_RESUBMITS` (`direct_pane_max_enter_resubmits()`), so the operator can crank "retry until submitted" without a rebuild during the live repro. The loop still exits immediately on submit, so the higher cap only costs wall-clock on a genuinely stuck pane. Tests: `direct_pane_enter_resubmit_is_bounded_while_trigger_remains_visible` updated to the tunable cap; full dispatch suite (129) + `make check` green. This is the "doesn't submit" half of `#jbrundispatch`; needs live verification (the pane-state where it failed) to confirm it closes the report vs needing a focus/readiness fix.

- **Preflight now collapses duplicate bare `[#id]` queue heads the mirror re-emits (`#qdup-bare-id`).** Operator-reported ("In equityfundingsource.md, I typed in queue items and agent-doc duplicated the queue items") and corroborated on this doc: `[#sqedit-race]` and `[#qpausemix-verify]` each appeared **twice** in the live queue. Root cause: the only id-dedup wired into preflight maintenance was the AST node-key dedup, which deliberately preserves occurrence-indexed duplicates, and the id-aware `dedup_live_prompts` was never wired in (preserving `do [#id]` duplicates is the deliberate `#queue-dedup-destroys-intentional-duplicates` invariant). So when the backlog→queue mirror / CRDT replay re-emitted a **bare** `[#id]` reference head (the pure mirror form, no `do`), nothing collapsed it. New `queue::dedup_bare_id_reference_heads` runs in `run_queue_maintenance` after the node-key dedup: it collapses duplicate **bare** `[#id]` / `#id` reference heads (pin markers stripped) to the first occurrence, while deliberately leaving `do [#id]` **directive** duplicates intact (intentional "run it twice" intent) and never touching free-text heads (incl. a directive citing an id with trailing text like `#id continue the drain`) or multiline blocks. Tests: `dedup_bare_id_reference_heads_collapses_mirror_duplicates`, `dedup_bare_id_reference_heads_noop_without_bare_duplicates`; the `preflight_preserves_intentional_duplicate_tracked_queue_prompt` invariant still green; full queue (481) + maintenance (67) suites + `make check` green. Backlog: `#sqedit-race` (the queue-edit-race hazard this is one facet of).

- **`queue prune-noise` now clears multiline/fenced pasted-evidence heads, not just bulleted noise (`#qnoise-multiline-strike`).** Operator-reported (monsterrodholders/agent-doc-bugs2): the queue kept showing duplicates of already-completed prompts that "no number of drains" cleared — "only a file reload in IDEA clears it." Root cause: `queue prune-noise` enumerated heads via the `markdown_ast` `item_nodes` overlay, which recognizes ONLY bulleted (`- …`) lines and skips fenced code, so operator-pasted `:round_pushpin:` console dumps — surfaced by `queue::parse` as multiline `---`-fenced `Prompt` heads or, for a bare ```` ``` ```` console paste, as a run of preserved `Freeform` lines — were invisible to the strike path and accumulated on disk forever (the editor convergence then faithfully re-pushed them). Fix: (1) new `queue::parse_spans` is the single byte-range-aware source of queue-head segmentation, and `prune_noise_queue_heads` excises multiline noise `Prompt` blocks AND pasted-evidence `Freeform` lines (new `queue::is_noise_freeform_line`, which preserves `---`/`~~~` separators and `re [#id]` references) by exact range, alongside the existing bulleted node-key strike; (2) the drainability classifier `is_drainable_queue_head_with_context` now demotes any **multi-line** head text (a console dump / `---`-wrapped multi-bullet paste) to noise — even when a line carries a stray `[#id]` (the `#5eq8`-in-a-console-dump false positive) — so the drain, the `queue_stale_noise_lines` counter, and `queue prune-noise` agree; a single-line `do [#id]` directive that merely happens to be `---`-wrapped stays drainable and is preserved. Validated against the live flooded doc: 61 noise entries excised (0 ``` fences / `:round_pushpin:` / `agent:boundary` left) with every id-backed directive — including the `---`-wrapped `#tsiftmdcrash` — preserved. Tests: `prune_noise_excises_multiline_fenced_paste_blocks_under_a_preset`; existing prune/queue suite (479) green. `make check` green. Backlog: `#prunenoise-live`.

- **Post-commit HEAD repair is now ack-gated under a live editor, and paused-queue preflight output names the reason (`#pcwcfailfix` / `#qpausemix`).** The `#pcwcdiskfree` listener-active path no longer claims editor-IPC reconciliation before `refresh_content` actually acks. If the editor refresh no-acks/errors, post-commit cleanup falls back to the authoritative `HEAD` disk write and logs `transport=disk_after_failed_editor_refresh`, preventing the corrupted working tree from re-seeding the next `live_prompt_drift` cycle. Separately, controller-paused queues now surface `queue_pause_reason` and pause-aware `queue_continuation_guidance`, so `queue_paused: true` beside `queue_continuation_required: true` is explicitly documented as "unattended supervisor auto-injection paused; attended loop still drains." Coverage: `postcommit_worktree_auto_reconcile_writes_disk_when_editor_refresh_fails`, `postcommit_worktree_auto_reconcile_skips_disk_write_with_active_listener`, `continuation_guidance_explains_controller_pause_reason`, and `run_queue_maintenance_controller_pause_surfaces_flag_without_stalling_continuation`. SPEC/README updated.

- **Done-id collection ignores prose citations (`#donemirrorreap`).** The preflight already-done-mirror reap was removing a gated `[/] [#fullboundary]` review item because `#fullboundary` is *cited in prose* inside the `#ftstrike` `agent:done` entry ("behind do `[#fullboundary]`"). `collect_agent_done_ids_with_root` scanned the whole done-component/archive text via `extract_pending_ids_from_text`, harvesting every bracketed id anywhere. New `agent-doc-core` `extract_done_item_own_ids` collects only each list-item's FIRST `[#id]` (its own identity, skipping checkbox markers and prose/continuation lines); done-id collection now uses it for both the inline `agent:done` component and the external `archive=` file. A `[#id]` cited in an item's description no longer marks that id done. Tests: `extract_done_item_own_ids_ignores_prose_citations`, `..._handles_checkbox_and_skips_prose_lines`; existing mirror-reap tests still green. `make check` green. SPEC updated.

- **Free-text queue heads are struck when answered, regardless of position (`#ftstrike`).** Operator-reported: "my free-text queue items are not immediately struck as if they are addressed." The leading-head consume only strikes a contiguous leading run and stops at an id-backed head, so a free-text report sitting behind an unfinished `do [#id]` head (e.g. behind `do [#fullboundary]`) was never struck even after the response addressed it. New closeout pass `strike_answered_free_text_queue_heads` (write.rs Phase 3c, after the leading-head consume): strikes every non-struck free-text head whose text the committed response answers, matched conservatively via `free_text_head_answered_by_response` — the head text (priority markers stripped, normalized to lowercase alphanumeric words, ≥4 significant words) must appear inside the response's `>` quoted-prompt blockquote region. A head merely mentioned in prose is NOT struck, so an unaddressed operator report is never silently dropped. Runs independent of the leading-head `queue_consumption_allowed` decision, strikes document + snapshot in sync (`consume_queue_nodes_by_key`), best-effort. Mirrors `strike_done_queue_head_prompts` for id-heads. 10 unit/adversarial tests (only-mentioned head not struck; short head not matched; head behind an id head selected; idempotent re-strike). `make check` green. SPEC updated. Plan: `tasks/agent-doc/plan-freetext-queue-strike-on-address.md`.

- **Convergence-gated inter-queue-item boundary Phase 1 — decision core + loud force-disk playback (`#fullboundary`).** Foundation for serializing the queue so item N+1 does not dispatch until item N proves a quiescent close (the root fix for the `content_ours` / `live_prompt_drift` / `postcommit_worktree_check match=false` / `inflight=5` `send_failed` drift family — the drain lease `#kp5z` serializes dispatch ownership but NOT editor convergence). New pure `convergence_gate` module: `ConvergenceFacts` (committed, editor_converged, inflight==0, actor_idle, elapsed/timeout) + `convergence_gate_decision` → `Dispatch` / `Defer { unmet }` / `ForceDiskFallback { unmet }` (no I/O, fully unit-tested). New `convergence_playback` module: `ConvergencePlayback` artifact written to `.agent-doc/playback/<doc-hash>/<cycle-id>.json` (ordered IPC attempt sequence + inflight, snapshot/baseline/HEAD hashes, candidate vs content_ours lengths/hashes, cycle/run/actor/supervisor identity, closeout state-machine transitions) + an ERROR-level `convergence_gate_force_disk_fallback severity=error … playback=<path>` ops-log line via `record_force_disk_fallback`. 16 unit tests; `make check` green. **Phase 2 (the remaining live-supervisor-critical wiring — call the gate inside the supervisor idle-queue-watch / drain inter-item dispatch path, and trigger the real `--force-disk` write + playback on a bounded timeout) needs a focused clean cycle with `make check` + `make tmux-ci` across two live panes**, which cannot run from the live driving session. SPEC updated. Plan: `tasks/agent-doc/plan-fullboundary-convergence-gate.md`.

- **Restart-agent Phase 1a — harness-change detection + boundary-gate (`#agentreloadrestart`).** Groundwork so changing `agent:` in frontmatter (e.g. claude→opencode) can take effect on restart. New `agent_doc_agent_change_restart` knob (env `AGENT_DOC_AGENT_CHANGE_RESTART` > frontmatter > project config > default ON; resolver `resolve_agent_change_restart` / `agent_change_restart_enabled`). New pure boundary policy `start::decisions::agent_change_restart_decision` (gated exactly like the `#supselfheal` `supervisor_recycle_action`: act only at a quiet dispatch-ready prompt, never mid-turn, only when the harness actually changed + knob on). The supervisor idle-queue watch now re-resolves the harness from CURRENT frontmatter each tick and, on a change (deduped per new harness), logs `harness_change_detected old=… new=… gate=…` (ops.log) + `agent_restart_boundary_gate … note=phase1b_execution_pending` (session log) so an `agent:` edit is observable + operator-live-verifiable. Tests: `agent_change_restart_decision_policy`, `resolve_agent_change_restart_precedence`. **Phase 1b (the restart EXECUTION — re-derive the launch spec in the supervisor restart loop / no-preserve-child recycle so the new harness spawns fresh) is the remaining live-supervisor-critical wiring** that the backlog item mandates a focused clean cycle with `make check` + `make tmux-ci` for; the Phase-1a logging exists precisely so that live gate can prove/disprove detection first. Plan: `tasks/agent-doc/plan-restart-agent.md`.

- **An explicit operator `Run Agent Doc` now starts even on a paused queue (`#qpauserun`).** Operator-reported: JB `Run Agent Doc` "did not start. It should have started" — the controller dispatch RPC blocked it with `failed_stage=queue_paused`. A `paused` queue control governs *auto-draining the queue*, not whether the operator can run a cycle, so an explicit operator reopen must not be blocked by it (same split as `#qpausego`: a pause stops the unattended injector, not the attended action). The dispatch RPC now admits a dispatch whose `command_kind` is an explicit operator reopen (`managed_reopen` / `dispatch_only_reopen`, classified by `dispatch_command_kind_is_operator_reopen`) past a deliberate operator/admin pause — one-shot: the pause row stays, so unattended callers (`idle_queue_continuation` / `/loop`) remain blocked until `admin queue resume`. EXCEPTION: a stale-supervisor churn-stop pause (`#jbrestale`) still blocks every caller, so the route path restarts the stale supervisor and re-dispatches once instead of admitting a reopen against a stale supervisor. Coverage: `dispatch_operator_reopen_bypasses_paused_queue` (+ existing pause/marker tests updated to use an auto command_kind for the pause-block assertions).

- **Operator queue adds now survive a supervisor/pane crash+restart (`#qdurcrash`).** An operator adds an `agent:queue` item, the turn starts, the supervisor + tmux pane crash and restart, and the add was GONE — it lived only in the editor buffer / in-memory CRDT and the reloaded snapshot predated it. New crash-durable journal (`queue_journal.rs`, `.agent-doc/queue-journal/<doc-hash>.jsonl`, append-only + fsync): `record` (preflight queue maintenance) durably captures every operator queue prompt the binary observes; `replay_missing` + `merge_missing_into_content` (supervisor startup, `run_with_reap_policy`) re-insert journaled prompts absent from the reloaded document so the crash+restart replays the pending edit instead of dropping it; `clear` (on every `commit_success`) empties the journal once the queue state is durable in the snapshot, bounding the journal to operator additions observed since the last commit. **Additive + conservative:** it only ever re-adds missing prompts and never removes anything, so it does NOT re-wire the post-commit worktree reconcile that the `#fintol2`/`#pcwc` carry-forward invariant guards (full `make check` green, including those tests). A struck/consumed prompt is treated as present and never resurrected. Known gap (plugin-side, out of scope here): an add lost to a crash *before* any cycle observes it (pure editor-buffer, never flushed to disk/binary) cannot be journaled by the binary — that needs a plugin buffer-flush-on-edit. Coverage: `record_then_replay_recovers_a_lost_queue_add`, `replay_does_not_resurrect_a_consumed_item`, `record_is_idempotent_and_clear_empties_the_journal`, `merge_is_a_noop_without_a_queue_component`, `absent_journal_replays_nothing`. Plan: `tasks/agent-doc/plan-qdurcrash-queue-edit-crash-durability.md`.

- **Accepted `admin queue pause` now suppresses the unattended supervisor auto-injection on a `go`-mode queue, without stalling the attended in-session loop (`#qpausego`).** An accepted `agent-doc admin queue pause <FILE>` records a durable controller `queue_controls` row that the controller *dispatch* RPC already honored (`failed_stage=queue_paused`), but the supervisor idle-queue watch injects `agent-doc <FILE>` triggers straight into the pane — bypassing the dispatch RPC — so a `go`-mode auto-queue kept re-dispatching after an accepted pause (the unattended flood). The idle-watch now consults the new best-effort, read-only `queue_continuation::document_queue_controller_paused(file)` (resolves project root + canonical document id, reads the effective `queue_controls` state from `.agent-doc/state.db`; returns `false` — never paused — when no control-plane DB exists or a read errors, logging the error to stderr) and defers its drain (`queue_dispatch_skipped ... reason=queue_control_paused`) while the pause is active. `preflight` surfaces a new `queue_paused: bool` for visibility. **The pause deliberately does NOT drop `queue_continuation_required` / `queue_drainable_head_count` and does NOT short-circuit `queue_continuation::detect`:** the attended in-session `/loop` is the legitimate single-owner drain and keeps working real queue backlog (stalling it on a pause strands genuine drainable items — `queue: stop` frontmatter / `--- stop` fences are the in-session stop control). `admin queue resume` (state `resumed`) clears the flag; `drain` (`draining`) is not `paused`. Coverage: `document_queue_controller_paused_false_without_state_db`, `document_queue_controller_paused_reflects_paused_then_resumed`, `detect_still_continues_when_controller_paused`, `run_queue_maintenance_controller_pause_surfaces_flag_without_stalling_continuation`. Backlog: `#qpausego`.

- **Route no longer consumes/loses an uncommitted operator queue head on JB `Run Agent Doc` (`#qdispatchloss`).** Route selects an inactive `agent:queue` head from the live on-disk document, but the JetBrains/VS Code plugin can sync an *uncommitted* operator queue edit to disk before it reaches a git-committed snapshot. Dispatching that head moved a possibly half-typed line into the agent prompt and then lost it — the consume never landed in a committed snapshot, so the item disappeared and the turn stalled uncommitted (the no-crash sibling of `#qdurcrash`). `inactive_route_queue_head_in_content` now proves the candidate head is backed by the committed snapshot (`snapshot::load`) before surfacing it: a head absent from a present committed queue — or any head when the committed snapshot has no queue component — fails closed (returns `None`, logs `route_dispatch_uncommitted_head ... reason=head_not_in_committed_snapshot decision=defer`), so the activate/dispatch path no-ops and the operator's edit survives for the next cycle, which commits the queue edit first and dispatches it from the committed snapshot. Conservative by design: a missing/unreadable/unparseable snapshot allows the head (bootstrap escape hatch). Applies only to the inactive-activation path; active-queue continuation heads (`queue_active: true`) flow through `queue_continuation::live_continuation_head` and are unaffected. Coverage: `route_defers_uncommitted_queue_head_not_in_committed_snapshot`, `route_dispatches_committed_queue_head`, `route_queue_head_unbacked_when_committed_snapshot_has_no_queue`, `route_queue_head_backed_allows_when_no_committed_snapshot`. FlowCore `route.rs` `reason=` budget 8→9. Plan: `tasks/agent-doc/plan-uncommitted-queue-item-dispatch-loss.md`.

- **`#smsim` (semantic_merge Phase 5) — deterministic SimWorld coverage of the operator↔agent concurrent-edit matrix; all five semmerge phases now complete.** New `SimWorld::converge_semantic_merge` runs the production `semantic_merge_scoped` (the same `exchange`-active scoping the real `#smconv` `try_semantic_merge_convergence` applies) and four `semmerge_sim_*` scenarios + four coverage counters lock the merge/IPC data-loss family: node-disjoint auto-merge (agent strike + concurrent operator add both survive, no ack), same-node operator-wins + ack inside the active area, the identical conflict OUTSIDE the active area auto-resolving operator-wins with NO ack (`#smturnactive` gating), and operator-deleted-an-agent-edited-node keeping the deletion + raising the ack. Test-only. Plan: `tasks/agent-doc/plan-semantic-ast-merge.md`.

- **`#smqstrike` (semantic_merge Phase 3) verified complete + merged-tree coverage added.** The queue-consume strike already routes through the node tree by id across all three write paths — editor-IPC per-node `strike` patches (`build_ipc_node_patches_json`/`queue_consume_node_patches`), disk-path divergence reconcile (`queue_consume_reconciles_diverged_snapshot_instead_of_bailing`, the merged document wins so a drifted operator queue edit never aborts the strike), and the `#smconv` content_ours convergence where `semantic_merge` treats `struck` as a node-disjoint flag-edit. Added `smqstrike_struck_head_survives_concurrent_operator_queue_add` proving a struck head and a concurrent operator queue add both survive the merged tree with no ack. No behavior change (the contract landed incrementally with `#smconv`); test + docs only. Plan: `tasks/agent-doc/plan-semantic-ast-merge.md`.

- **Semantic-merge acks are carried into the next cycle's response as an acknowledgement turn (`#pk3f` / `#semmerge-ack-turn`, Phase 4).** `#smconv` (0.34.7) applies node-disjoint operator↔agent changes and operator-wins on same-node conflicts, then logged `semantic_merge_ack_pending` to `ops.log` for any non-applied agent change — but nothing carried that fact into the *agent's* next turn, so an operator-deleted-agent-edited-node / same-node-override / operator-revived-agent-deleted-node was silently won by the operator with no acknowledgement in the exchange. The convergence path (`write/ipc.rs`) now persists each `requires_ack` `AckRequest` to cycle_state via `record_semantic_merge_acks`; `start_preflight` carries un-surfaced acks forward exactly one cycle (driven by a `surfaced` flag, not a millisecond-collidable cycle-id compare) into the new `CycleState.pending_semantic_merge_acks`, and preflight surfaces them as `semantic_merge_acks` plus a companion `semantic_merge_ack_pending` warning so the existing "surface warnings" skill path drives the agent to acknowledge the non-applied change. The merged document is unchanged — operator content already won — so this only adds the courtesy acknowledgement; no content is created or lost. New stable `AckReason::token()` is the wire format cycle_state persists. Coverage: `ack_reason_tokens_are_stable` (markdown-ast); `record_semantic_merge_acks_tags_current_cycle_and_dedupes`, `start_preflight_carries_prior_cycle_acks_forward_exactly_once`, `semantic_merge_ack_recorded_after_carry_chains_to_next_cycle` (cycle_state); `preflight_output_semantic_merge_acks_roundtrip` (preflight). Plan: `tasks/agent-doc/plan-semantic-ast-merge.md`.

- **`semantic_merge` ack emission is scoped to the turn-active area so an unrelated operator edit no longer raises ack noise (`#msn6` / `#smturnactive`, Phase 6).** `#smconv` (0.34.7) applies node-disjoint changes and operator-wins on any same-node conflict regardless of turn scope, logging a `semantic_merge_ack_pending` for *every* same-node collision — including the common case where the operator edits the queue / a backlog item while the agent writes its response (the firsthand `queue: stop` + cross-head drift). `agent-doc-markdown-ast::semantic_merge` now takes a first-class turn-active node-set: new `ActiveNodes` (whole-component via `active_component` or node-granular via `with_node`) + `semantic_merge_scoped(base, ours, theirs, &active)`. The merged document and per-node outcomes are **identical** to `semantic_merge` (the operator still wins every same-node conflict — no content is ever lost or changed); only `requires_ack` is filtered: a conflict whose node is OUTSIDE the active area auto-resolves silently, while an in-area collision still raises its `AckRequest`. The convergence caller (`write/ipc.rs::try_semantic_merge_convergence`) marks the `exchange` component active (the turn-active area is the exchange tail), so operator drift in queue/backlog/frontmatter that collides with an agent edit no longer emits `semantic_merge_ack_pending` noise; only an exchange-response-area collision does. `semantic_merge` is unchanged (legacy all-active behavior preserved for every other caller). Coverage: `scoped_conflict_outside_active_area_drops_ack_but_keeps_operator_value`, `scoped_conflict_inside_active_area_keeps_ack`, `scoped_active_set_is_node_granular`, `scoped_empty_active_set_drops_all_acks`. Plan: `tasks/agent-doc/plan-semantic-ast-merge.md`.

- **Single live IntelliJ plugin consumer is elected per document so concurrent windows can't race patch application into File Cache Conflicts (`#8bfz` / `#fcconeowner`).** `#fccreap` (0.34.8) reaps *dead*-pid consumer patch files but does nothing about concurrent *live* ones: with N windows open on one workspace, each registers its own consumer id `jetbrains-<pid>-<uuid>` (`TypingTracker.kt`), watches `.agent-doc/patches/`, and applies + `saveDocument`s the same untargeted (broadcast) patch — N live writers racing into the File Cache Conflict / cross-buffer drift family. New `agent-doc-orchestration::plugin_owner` adds a per-document single-owner lease (`.agent-doc/plugin-owner/<hash>.json`, mirrors `drain_owner`) claimed atomically via `create_new` (O_EXCL) so two instances racing for an unowned/stale lease cannot both win. Ownership is sticky while the owner keeps applying (it refreshes the heartbeat on every patch event) and self-healing: a stale heartbeat OR a provably-dead owner pid (`kill(pid,0)`) hands ownership to the next live consumer, and explicit `release_plugin_owner` on dispose hands it over immediately without waiting out the 30s TTL (`AGENT_DOC_PLUGIN_OWNER_TTL_SECS` override). Exposed via FFI `agent_doc_plugin_owner_try_acquire` / `agent_doc_plugin_owner_release`; `PatchWatcher.processPatchFile` gates only **untargeted** (`editor_id == null`) patches behind the lease (editor-targeted patches already have a unique consumer and bypass it), and non-owners leave the patch file for the owner instance to apply + delete. Fail-open by construction: any IO/FFI error (or an older binary missing the symbol) returns "apply", so a single-instance setup is never worse off than before the lease. Coverage: `plugin_owner` unit tests (`first_consumer_wins_second_defers_while_owner_is_live`, `dead_owner_pid_hands_ownership_to_next_consumer`, `stale_heartbeat_hands_ownership_to_next_consumer`, `release_only_removes_own_lease`, `end_to_end_real_paths_elect_single_owner`). Operator-verify: needs a live multi-window IntelliJ test. Plugin 0.2.171.

- **Stale-binary supervisor self-heals from a wedged editor write or a failed re-exec instead of an indefinite wedge (`#supselfheal` Phases 2–5, `#supselfheal-rest`).** Completes the wedge plan on top of the Phase 1 `explicit_admin` override. `supervisor_recycle_action` now takes two more typed evidence inputs: `write_wedged` (Ph2, `#supselfheal-wedgetrigger`) and `reexec_failed` (Ph3, `#supselfheal-reexecescalate`), and gains a new `EscalateKillRelaunch` action. (1) **Wedge trigger:** the write/converge closeout derives a typed `write_wedged` fact from repeated `send_failed`/`no_ack` against a nominally-active JB listener (the `#fcc0e` de-wedge latch) — `write_wedged_from_ipc_failures` + the supervisor-facing `editor_ipc_write_wedged` reader — and logs `write_wedged_supervisor_recycle_requested` instead of looping silent refusals. A wedge against a stale binary overrides the default-OFF opt-out and recycles immediately at the turn boundary (it must never stay `Detect`, and never waits for an idle boundary that may never come). (2) **Re-exec escalation:** when the in-place `execve` recycle cannot start (deleted-inode `ENOENT` from a fresh `make install`, or another syscall error), the policy returns `EscalateKillRelaunch` and the idle watch escalates to a bounded (`MAX_REEXEC_ESCALATIONS`) kill+relaunch of the harness child — reusing the `#supkill-bg` drain-and-relaunch path — instead of looping `continue_current_binary` forever. (3) **Guidance:** the SKILL.md auto-loop note and `runbooks/commit.md` no longer claim a stale supervisor self-heals via drain-lease expiry; they document the wedge-triggered recycle / `admin recycle` / bounded kill+relaunch as the actual non-disruptive recovery for a stale **binary**. Coverage: pure decision-table tests for every new row (`supervisor_recycle_action_write_wedge_overrides_opt_out`, `supervisor_recycle_action_reexec_failure_escalates_to_kill_relaunch`, `reexec_escalation_bound_caps_retries`), converge classifier/reader tests, and SimWorld reproductions of the firsthand session (`wedged_opted_out_supervisor_recycles_on_write_wedge`, `failed_reexec_escalates_to_bounded_kill_relaunch`). Plan: `tasks/agent-doc/plan-stale-supervisor-wedge-no-selfheal.md`.

- **`supervisor_recycle_action` gains an `explicit_admin` override so `agent-doc admin recycle` can recycle a stale-binary supervisor (`#supselfheal` Phase 1 policy core, `#supselfheal-adminrecycle`).** The route-owned supervisor recycle policy (`start/decisions.rs::supervisor_recycle_action`) previously returned `Detect` (surface only, keep running the stale binary) whenever auto-recycle was opted OUT — so an explicit `admin recycle`, the gentle fix the closeout path itself recommends, had no policy path to actually clear a stale supervisor. The predicate now takes `explicit_admin`: an operator/agent `admin recycle` request overrides the default-OFF opt-out and returns `RecycleImmediate` for a stale supervisor at the next turn boundary, while still respecting `turn_boundary` (never drops a live turn) and staying a no-op when the binary is fresh. Exhaustively unit-tested (`supervisor_recycle_action_explicit_admin_overrides_opt_out`). The live `admin recycle` → route-owned-supervisor IPC adapter that flips this input to `true` (a non-disruptive supervisor-directed recycle that does not refuse busy panes) is the queued follow-up `#supselfheal-adminwire`; the idle-watch and SimWorld callers pass `false` until it lands, so current behavior is unchanged.

- **`parse_close_marker` accepts the standard `<!-- /agent:name -->` close-marker spelling so `Component.end_byte` spans the full component (`#gszq`/`#mdastclose`).** In `agent-doc-markdown-ast` `overlay.rs`, `parse_close_marker` only recognized the legacy `<!-- agent:/name -->` (`agent:`-then-slash) form. For the real spelling `<!-- /agent:name -->` (slash-then-`agent:name`) — the spelling every session document and the crate's own test fixture actually use — it returned `None`, so the component never matched its explicit close and was only *implicitly* closed by the next open marker (or EOF). `components()`/`items()` still parsed correctly via implicit-close (item parsing was unaffected), but `Component.end_byte` pointed just past the **open** marker instead of through the close line, so any consumer slicing a component by its `start_byte..end_byte` byte span got a truncated range. `parse_close_marker` now accepts both spellings (`/agent:name` and `agent:/name`); `end_byte` spans the full component. Coverage: `end_byte_spans_full_component_for_both_close_spellings`.

- **Closeout reaps stale dead-PID IntelliJ consumer patch files (`#fccreap`).** The JetBrains plugin registers a per-instance consumer id `jetbrains-<pid>-<uuid>` (`TypingTracker.kt`), and per-instance patch files `<doc_hash>.jetbrains-<pid>-<uuid>.json` land in `.agent-doc/patches/`. When an IntelliJ instance dies/restarts — or multiple windows are open on one workspace — those files **accumulated and were never reaped** (observed: 17+ dead-PID files up to 143 KB that regenerated after manual cleanup), bloating the patches dir and feeding the multi-instance IPC confusion behind the File Cache Conflict / cross-buffer drift family. `fire_post_commit` now best-effort reaps them (next to the existing `reap_local_model_leases` closeout reap): `reap_stale_jetbrains_consumers(project_root)` scans the patches dir, parses the pid from each `jetbrains-<pid>-<uuid>.json`, and removes only files whose pid is provably dead — Unix `kill(pid,0)` where `ESRCH` ⇒ dead/reap and `EPERM` ⇒ alive/keep, never the current pid, never a non-`jetbrains-` file (base `<hash>.json`/`.vscode` variants are skipped), and a no-op on non-Unix. Pure, injectable core (`jetbrains_consumer_pid`, `reap_stale_jetbrains_consumers_with`) so the decision is unit-tested without real processes; closeout never fails because a reap could not run. This is the durable defense against the multi-instance condition (operator-side mitigation is still collapsing to one IntelliJ window). Coverage: `jetbrains_consumer_pid_parses_pid_and_rejects_non_matching`, `reap_removes_only_dead_pid_consumer_files`, `reap_is_noop_on_empty_or_missing_dir`.

- **Live editor-buffer drift now node-merges instead of dropping the agent's response (`#smconv` / `#semmerge` Phase 2).** Root-cause fix for the `live_prompt_drift_after_preflight` → `content_ours`-adoption transition that dropped every agent response (new `### Re:` turn, queue strike, backlog edit) whenever the operator's editor buffer drifted from the agent baseline at closeout — the corruption that forced the manual `git checkout HEAD` / `reset --from-current` recovery dance. `write/ipc.rs::guard_ipc_snapshot_adoption_against_live_prompt_drift` now tries a node-keyed `agent_doc_markdown_ast::semantic_merge::semantic_merge(base, candidate, content_ours)` BEFORE the existing `#fintol2` line-merge / `content_ours` fail-close: when the operator and agent edited disjoint nodes (the common case — operator flips `queue: stop` + edits the queue while the agent strikes a *different* head and appends a `### Re:` turn) it applies BOTH change-sets in one clean commit. A conservative "safely applicable" gate requires the AST to apply (non-empty components on all three sides), a structurally-clean re-parse, and ZERO dropped agent prompt/queue/response content (`dropped_prompt_lines_after_content_ours` / `dropped_queue_prompt_lines_after_content_ours` empty + every new `### Re:` heading preserved) — otherwise it falls through to today's recorded-evidence `content_ours` path unchanged (fail-closed only when the merge can't be represented). Logs `live_prompt_drift_semantic_merged`; `requires_ack` outcomes (same-node operator-wins, operator-deleted-an-agent-node) are applied (operator-wins already encoded in `merged_doc`) and logged `semantic_merge_ack_pending` pending the Phase-4 ack-turn. Coverage: `smconv_merges_heading_prose_response_preserving_both_changesets`, `smconv_disjoint_drift_merges_both_change_sets`, `smconv_same_node_conflict_is_safe`, `smconv_declines_on_structurally_corrupt_ours_falls_through`. FlowCore budget bumped (`ipc.rs` `guard_` 12→16 test-call, `reason=` 17→18).
- **`semantic_merge` models `### Re:` heading-prose exchange turns as append-only nodes (`#semmerge-owner` heading-prose extension).** The shipped Phase-1 `semantic_merge` keyed exchange turns as list-item bullets (`- re [#id]`), but real session docs author turns as `### Re: <topic> — <model>` h3 heading-prose blocks the overlay never modeled as items — so a node-merge silently dropped the agent's response turn (the gap that made `#smconv` decline for every real session). `semantic_merge` now splits the `exchange` component into heading-keyed blocks (key normalized to ignore a trailing `(HEAD)` boundary annotation and `~~`-strike wrappers), and appends any `### Re:` block present in the agent's exchange but absent from the operator's — before the trailing `<!-- agent:boundary -->` marker, exactly one boundary preserved — as `AppliedAgentAdd` outcomes. Append-only by construction (no in-place prose merge). `overlay.rs` is unchanged; the heading split is local to `semantic_merge`. Coverage: `exchange_appends_agent_new_heading_prose_turn`, `exchange_head_marker_does_not_split_turn_identity`, `exchange_boundary_marker_preserved_and_new_turn_before_it`.

- **Shadow-open-backlog guard no longer hard-wedges on `agent:queue` `[#id]` heads (`#qheadsync` orphan-shadow).** `pending::detect_shadow_open_items` excluded tracked-work and `exchange` components from its scan but NOT `agent:queue`, so a queue head referencing an id absent from the live backlog — a reaped id's lingering `[#id]: note` head (e.g. `[#jbacceptwedge]: 0.2.170 installed`), or any `#mirrorall`-mirrored `do [#id]` whose item is reaped — was misclassified as an "open backlog item that exists only outside the live backlog" and made `enforce_no_shadow_open_backlog` (preflight repair step) `bail!` before the done-strike maintenance pass could clear it. That was the orphan-shadow wedge that forced the manual `Edit`+`reset --from-current`+`commit` dance after `--done` this session. Queue `[#id]`/`do [#id]` entries are legitimate references to backlog items, not shadow backlog items hiding in commented-out prose, so the queue component is now excluded from the scan like `exchange`/tracked-work already are (genuine shadow items in HTML-comment/non-component prose are still caught). This matters more post-`#mirrorall`, which deliberately places many `[#id]` heads in the queue. Coverage: `detect_shadow_open_items_ignores_agent_queue_id_heads` (reaped + mirrored queue heads not flagged; a commented-out shadow item still caught); existing shadow tests unchanged. This is the orphan-shadow half of the `#qheadsync` queue-head reconciliation; the answered-free-text auto-strike half remains.

- **Backlog→queue sync mirrors ALL queue-attr backlog items, including `[operator-verify]` (`#mirrorall`).** Operator: "The backlog items are not in the queue right now. They should be in the queue. We should have redundancy so it not only immediately adds the item into the queue once created, but also adds backlog items that were missed in previous turns." Previously `run_queue_maintenance` narrowed the sync source to the drainable subset (`partition_drainable_backlog_ids`), so `[operator-verify]` items were skipped from the queue entirely (the `#goqueuestall`/`#qcontdrain` anti-thrash rule). They are now mirrored into the queue as `do [#id]` heads so the queue is a complete worklist — an operator-verify head surfaces the operator instructions carried in the item text (which `#qheadsync`-era bolding renders as `**Operator action: …**`). Crucially `backlog_ids` is NOT narrowed: `head_is_drainable` still defers operator-verify ids via `deferred_backlog_ids`, so `queue_drainable_head_count` continues to exclude them and the in-session auto-drain loop is NOT re-armed by a mirrored operator-verify head. Because the per-preflight sync is idempotent (existing/struck ids are never re-added) and runs every cycle, it doubles as the **reconciliation sweep** the operator asked for: a backlog item missing from the queue (added before activation, or lost in a prior-turn drift like `#qheadsync`/`#733r` was) is re-mirrored on the next preflight, not just on creation. Phase 2 (required before a mirrored queue is resumed from pause): the supervisor idle-watch (`start/idle_watch.rs`) must apply the same drainability defer so operator-verify-only heads do not re-injection-thrash (`#rz3a`, previously DO-NOT-IMPLEMENT — the mirror-all decision reverses that gate); until it lands the mirrored queue stays operator-paused. Coverage: `run_queue_maintenance_mirrors_operator_verify_into_queue_but_keeps_it_nondrainable` (mirrors `[operator-verify]` into the queue AND asserts `drainable_head_count == 1`); existing `partition_drainable_backlog_ids_skips_only_operator_verify` (the pure partition is unchanged) and the queue-sync suite stay green.

- **`session-check` self-clears the `#queue-user-edit-overwrite` wedge by id (`#qheadsync`).** Operator: "You should automate the wedge clear." Observed on `agent-doc-bugs2.md`: after an IPC `content_ours` merge, `session-check` stayed `INTERRUPTED` indefinitely on dropped queue edits `[#qpauseux]`/`[#docdriftgrace]` even though both were preserved in HEAD as struck `~~do [#id]~~` heads from a prior cycle. No `reset` variant (`--preserve-session`, bare, `--force-disk`) cleared it because the only escape was a manual supervisor restart / patch surgery. Root cause: `check_dropped_queue_prompt_guard` proved preservation only by (a) normalized text identity — which cannot bridge the recorded bare `[#id]` form against HEAD's `do [#id]`/struck spelling — and (b) a consumed-check scoped to *this cycle's* resolved ids, while the strike happened earlier. The guard now also clears when the dropped prompt's `[#id]` is present in the committed/visible `agent:queue` in **any** form, including struck/consumed lines: new `queue_ids_including_struck` strips the strike wrapper before id extraction (`committed_queue_head_ids` deliberately skips struck lines for live-head accounting, so a loss-detection-only variant is required). A struck head visibly reached the document, so its id is preserved, not silently lost — the guard self-clears the stale `dropped_queue_prompts` marker instead of wedging the session. Genuinely lost ids (absent from the committed queue) still fail closed. Coverage: `session_check_clears_dropped_queue_marker_when_id_preserved_in_other_spelling`; existing `session_check_fails_closed_on_dropped_queue_edit` proves a real loss still interrupts. Plan: `tasks/agent-doc/plan-answered-queue-head-autostrike.md`.

- **Post-commit worktree reconcile no longer raw-writes the session doc behind a live editor (`#pcwcdiskfree`).** Operator: ":pushpin: JB `File Cache Conflict` still occurring. Please replace all direct file writes on the hot path." Root cause: `emit_postcommit_worktree_check` ran `std::fs::write(file, &head_doc)` *unconditionally* whenever `postcommit_worktree_lost_committed_content` returned true, then called `send_postcommit_editor_refresh` to push HEAD content through the editor IPC. The disk write was the recurring `File Cache Conflict` source — it fired behind IntelliJ's open buffer on every detected lost-committed-content drift, which during normal go-mode drains was usually just unsaved-editor divergence rather than real corruption. The reconcile is now listener-aware: with a JB (or VS Code) IPC listener active it **skips the disk write entirely** and lets `send_postcommit_editor_refresh` carry HEAD content back through `refresh_content` — the editor buffer becomes authoritative and the disk catches up on the IDE's next save (logged `postcommit_worktree_auto_reconciled ... transport=editor_ipc_skipped_disk_write`). With no listener (headless / CI), it still writes HEAD to disk authoritatively (logged `transport=disk`) so committed content is restored even when no editor is attached. Closes the last normal-operation session-doc disk write on the post-commit hot path; the remaining raw disk writes are the authoritative recovery/scaffold/migration paths that must hit disk even when the editor/IPC path is itself wedged. The `start_fake_listener` test helper now models `refresh_content` correctly (applies the message's `content` field instead of reading from disk), so the live-editor test asserts the editor IPC refresh, not the (now-removed) disk write. Coverage: `postcommit_worktree_auto_reconcile_skips_disk_write_with_active_listener`, `postcommit_worktree_auto_reconcile_writes_disk_without_listener`; existing `postcommit_worktree_auto_reconcile_refreshes_live_editor_buffer` and `postcommit_worktree_check_logs_match_false_for_real_corruption` continue to pass. FlowCore `reason=` token budget bumped 11 → 13 in `tests/test_cli.rs`.

- **Closeout now auto-reaps crashed-session GPU leases (`#kgleasereap`).** `tsift kg extract`'s cooperative GPU lease (`#kgleasewire`) previously needed a manual `tsift local-model lease reap --unload-empty` to reclaim pid-dead holders left by crashed extractor runs and unload the now-unreferenced model. The closeout (post-commit) hook now runs that reap automatically, mirroring the existing `tsift-memory` closeout-capture seam: it spawns `tsift local-model lease reap --unload-empty --json` under the resolved project root only when a `.tsift/gpu-lease.json` registry is present, and every failure mode (no project root, no registry, `tsift` not on PATH, non-zero exit) degrades to a logged stderr warning — closeout never fails because a reap could not run. This is safe during an active cycle because `#kgreflease` only reclaims pid-dead or TTL-expired holders, never a concurrent live extractor's lease. The closeout seam is used instead of the raw idle-tick so the reap stays off the queue-drain hot path. Pure `reap_command_args` builder (default `--unload-empty`, optional `--lease-file`/`--host` overrides) plus an explicit skip-without-project-root path. Coverage: `reap_command_args_defaults_to_unload_empty_without_optional_flags`, `reap_command_args_appends_lease_file_and_host_when_given`, `reap_skips_without_project_root_and_never_spawns`.

- **Remaining session-doc disk-write sites audited against a live editor (`#fccaudit`).** Extends `#fccqueue`: every `std::fs::write`/`atomic_write` call site that targets the session document is audited against an active JB editor IPC listener and either routed through the `#fcc0` converge gate or documented as editor-safe. Two normal-path gaps were routed through `converge_or_disk_write`: the `agent-doc write --status` component replace (`status_set`) and the post-commit ephemeral guard-marker strip (`strip_guard_markers`, removing `<!-- no-pending-capture -->` / `<!-- no-pending-done-guard -->`) — with a listener active they converge through editor IPC (no `File Cache Conflict`), with no listener they fall back to the same byte-identical disk write. Already-safe sites are documented: exchange compaction (`#w42v`), the post-commit boundary reposition (skips the working-tree write while a listener is active), and the `#pcwc` post-commit worktree reconcile (HEAD-authoritative repair of a tree that *lost* committed content, followed by an editor-buffer IPC refresh). Recovery/scaffold/migration writes (`claim` scaffold, `repair` orphan recovery, preflight migration/repair, `session-check` over-application remedy, `reset` resume-clear) stay authoritative must-hit-disk by design — they restore correctness when the editor/IPC path may itself be wedged. `(HEAD)` heading annotations and `agent:boundary` markers are deliberately out of scope (the working tree/editor preserve them). Coverage: `set_writes_status_to_disk_without_listener`; `strip_guard_markers` routes through the existing `converge_or_disk_write` gate tests.
- **Backlog→queue sync honors `not-before=YYYY-MM-DD` scheduling preconditions (`#backlog-not-before`).** Operator: "if items are gated or have preconditions that are not met such as a date in the future, do not add the backlog item into the queue." Gated `[/]` items were already excluded; this adds a date precondition. An open backlog/icebox item carrying a `not-before=YYYY-MM-DD` token is held out of the `queue`-attribute sync while the current UTC date is before that threshold (`pending::active_item_ids`, `active_item_priorities`, and `active_enqueue_item_ids` all exclude it — an explicit `:inbox_tray:`/`/enqueue` marker does not override an unmet date), and becomes eligible on/after the date. It stays a normal open `[ ]` entry the whole time (a soft schedule, not a `[/]` gate, never auto-gated). The token must start at a word boundary and parse as a strict `YYYY-MM-DD`; malformed values are ignored. Day math uses a proleptic-Gregorian `days_from_civil`, so no date crate was added. New: `pending::item_not_before_day` / `item_precondition_unmet` / `today_civil_day`. Coverage: `active_item_ids_holds_future_not_before_items`, `active_enqueue_item_ids_holds_future_not_before_items`, `item_not_before_day_parses_and_validates`, `item_precondition_unmet_compares_against_today`, `run_queue_maintenance_holds_future_not_before_backlog_item_out_of_queue`.
- **Preflight queue maintenance no longer raw-writes the session doc behind a live editor (`#fccqueue`).** Operator reported the IntelliJ `File Cache Conflict` dialog *still* fired during go-mode queue drains. Root cause: `run_queue_maintenance` persisted its mutations (queue body sync, opening-tag activation-token strip, `queue:` frontmatter state) with four unconditional `std::fs::write(file, …)` calls, bypassing the 08b write-authority routing the finalize/response path already uses — so every preflight queue-maintenance cycle touched disk behind the open editor buffer and tripped the conflict dialog. The four sites now route through a single `persist_queue_maintenance_doc` gate: with a JB editor IPC listener active it converges the queue shape through the editor (`converge_live_buffer_queue_shape` → plugin `setText` + `saveDocument`, no external-modification dialog) and skips the disk write, recording `write_authority action=routed surface=queue_maintenance` in `ops.log`; with no listener it writes to disk exactly as before (byte-identical non-IDE behavior). This brings queue maintenance to the same `#fcc0` converge-or-disk discipline the pending/review maintenance sites already use. The private `.agent-doc/` snapshot is still written directly (never open in the IDE). No plugin change required — the convergence patch handler already persists conflict-free. Coverage: `run_queue_maintenance_routes_through_ipc_without_disk_write_when_listener_active`; the existing no-listener `run_queue_maintenance_go_mode_appends_fresh_backlog_into_nondrained_queue` proves the disk-write path is unchanged without a listener.
- **Route ready-prompt barrier now accepts supervisor-proven `idle_pane_reconcile` transitions (`#monster60stimeout`).** A JB `Run Agent Doc` after a session-close/reopen could wait the full 60s route timeout even though the actor was already `ready` with `runtime_state=ready supervisor_health=healthy`, because the edge-triggered pty redraw missed re-emitting a prompt shape `ready_prompt_candidate` recognized and the fallback only matched `prompt_ready`/`dispatch_ready_prompt` reasons. The supervisor's idle-watch records `idle_pane_reconcile` only after `supervisor_pane_has_busy_cue == Some(false)` — direct pane evidence the pane is idle — so the barrier now accepts that current-generation transition as ready proof too, eliminating the 60s stall. Coverage: `transition_proves_ready_accepts_idle_pane_reconcile`, `transition_proves_ready_rejects_*`.
- **Same-cycle `--pending-add` closeout now populates active go-mode backlog queues (`#pendingaddqueuesync`).** `finalize` / `write --commit` apply pending-add mutations after preflight queue maintenance, so a captured follow-up could land in `agent:backlog priority queue` without a matching active queue head until another preflight happened to repair it. Closeout now appends ids recorded in `cycle_state.pending_added_ids` into active go/start queues whose backlog carries a recognized `queue` attribute, after current-head consumption and before commit. The helper is append-only to preserve the current runnable queue order, skips done/already-queued/operator-verify ids, updates the snapshot queue region, and leaves non-go persisted-active queues under the existing amplification guard. Coverage: `closeout_sync_appends_same_cycle_pending_add_in_go_mode`, `closeout_sync_holds_same_cycle_pending_add_without_go_mode`.
- **Managed Claude Code supervisors now keep routine stderr out of the foreground TUI too.** The earlier stderr-bleed fix redirected Codex and OpenCode supervisor diagnostics but left Claude attached to supervisor stderr, so stale-busy reconcile, restart, and hot-reload messages could still paint over Claude Code after `/clear` or recycle. Claude now participates in the same managed-TUI stderr redirection policy as Codex/OpenCode in both `agent-doc start` and `agent-doc run`: routine diagnostics go to `.agent-doc/logs/supervisor-stderr.log` / `run-stderr.log`, with verbose and non-managed stderr behavior unchanged. Coverage: `is_tui_harness`, `run_stderr_redirect_harnesses_include_claude_codex_and_opencode`.
- **Idle-queue Codex trigger dedupe now recognizes relative drafts before resubmitting.** The supervisor already avoided stacking an identical drain payload, but it compared the absolute `agent-doc /abs/.../tasks/monsterrodholders.md` trigger against the visible composer text literally. If Codex was already showing the equivalent relative draft `agent-doc tasks/monsterrodholders.md`, the idle watcher appended another trigger on each idle tick while the operator was typing in the queue. The pending-payload check now reuses route's relative/absolute draft equivalence before appending, so it presses the submit key once instead of flooding the composer. Coverage: `supervisor_pending_payload_matches_relative_codex_agent_doc_draft`.
- **Codex footer-only route readiness now accepts the shorter `Context N% use` suffix.** A JetBrains `Run Session Context` reroute after clear could still spend the full startup wait and fail as `latest run is still booting` when Codex rendered only `gpt-5.5 xhigh · ... · Context 0% use` instead of the previously-covered `Context N% used` form. The shared context-status predicate now accepts both suffixes, preserving the existing busy/protected prompt guards. Coverage: `ready_prompt_candidate_accepts_codex_context_use_footer_without_prompt` and `idle_chrome_only_output_accepts_codex_context_use_suffix`.
- **Codex idle-queue handoffs now honor opted-in context-threshold resets for ordinary heads.** The Codex Stop-hook could correctly log `codex_fresh_context_handoff` when `agent_doc_queue_context_reset`/`agent_doc_clear_threshold` required fresh context, but the supervisor receiver had been narrowed to clear only explicit `[clean-session]` heads, so the next ordinary queue head still dispatched into the old pane. The idle-queue watch now reuses the Codex reset reason at the safe idle boundary, sends `/clear`, waits for settle, then drains the ordinary head; route-in-flight, turn-active, pending-clear, and one-clear-per-head gates still apply. Coverage: `codex_opted_in_context_reset_dispatches_for_ordinary_head`.
- **Codex footer-only idle panes now satisfy route startup readiness after busy/protected checks.** JetBrains `Run Agent Doc` could spend the full startup wait on Codex panes whose bottom line was only `gpt-... · ... · Context N% used`, then fail as `latest run is still booting` even though there was no active-turn cue or drafted input. Route readiness now accepts bottom Codex idle status chrome the same way the status/clear path already did, while still rejecting background-terminal busy cues and hook-review/protected prompt states. Coverage: `ready_prompt_candidate_accepts_codex_footer_without_prompt` and `ready_prompt_candidate_rejects_codex_busy_footer_without_prompt`.
- **JetBrains Run Agent Doc now submits an existing relative-path draft instead of appending a duplicate absolute trigger.** The direct-pane pre-submit guard now treats a visible `agent-doc <relative-path>` draft as equivalent to the routed absolute-path trigger when the absolute target ends with the relative path, so a Codex pane already showing `agent-doc tasks/monsterrodholders.md` receives the submit key instead of `agent-doc /abs/.../tasks/monsterrodholders.md` being appended. The stale-scrollback guard still requires no later idle prompt. Coverage: `direct_pane_existing_draft_detection_matches_relative_codex_path`.
- **Codex background terminals now block route and idle-queue prompt injection (`#codexbgbusy`).** Codex can show an input prompt while a background terminal is still running; typing there queues a message instead of dispatching it. `HarnessConfig::dispatch_blocker_reason` now treats `Waiting for background terminal (... esc to interrupt)` as `active codex turn` evidence, so route readiness and supervisor idle-queue drains skip injection until the background task finishes. Coverage: `has_busy_cue_detects_codex_background_terminal_with_idle_prompt`.
- **Managed Codex supervisors now keep routine stderr out of the foreground TUI (`#codexstderrtui`).** Codex is now classified with OpenCode as a TUI harness for `agent-doc start`, so supervisor `eprintln!` diagnostics are redirected to `.agent-doc/logs/supervisor-stderr.log` instead of printing over the Codex screen after `/clear`, restart, stale-busy reconcile, or stale-binary hot-reload. Coverage: `is_tui_harness`.
- **IPC live-prompt drift no longer treats stale queue omissions as authoritative deletions (`#qdelipc`).** When socket ACK content, file-IPC ACK content, or file-read fallback content diverges after preflight, baseline `agent:queue` prompts missing from that candidate are now preserved in the agent-owned `content_ours` snapshot instead of being removed as if the user deleted them. The live-prompt-drift branch logs `queue_live_deletion_ignored ... reason=unproven_ipc_candidate_queue_deletion`, while the disjoint outside-edit tolerance refuses to forward-merge candidates that drop baseline queue prompts. Normal closeout queue consumption and done-id handling remain the deletion authority. Coverage: `ipc_live_prompt_drift_content_ours_ignores_unproven_live_queue_deletions`, `preserve_content_ours_over_live_queue_deletions_keeps_baseline_prompts`, and `fintol_queue_deletion_is_not_forward_merged_with_outside_edit`.
- **VS Code Run/Clear actions now match the JetBrains per-document command contract.** `Run Agent Doc` clicks dedupe behind the first alive plugin-spawned route process, `Clear Session Context` cancels a still-dispatching route process before invoking `agent-doc session clear <FILE>`, and `Run Agent Doc` clicks during an active clear queue until the clear and any selected clear-refusal recovery action finish. Added `editors/vscode/SPEC.md`, refreshed the VS Code README/session-command docs, and bumped the local VS Code package to `0.2.28`. Coverage: `editorCommandState.test.ts` plus the existing VS Code session UI command/refusal tests.
- **JetBrains Clear Session Context can preempt a stalled Run Agent Doc dispatch.** A Clear click while a plugin-spawned Run route process is still dispatching now cancels that route handle and proceeds through the shared `agent-doc session clear <FILE>` path instead of showing "blocked while Run Agent Doc is dispatching." Repeated Run clicks still dedupe behind the first alive route, and Run clicked during an active clear still queues until synchronous clear completion. Coverage: `normal clear preempts active run dispatch`, `run completion after preempting clear is ignored`, and `canceling active route removes and cancels current run`.
- **Active editor convergence failures no longer fall back to external file edits (`#fcc0-no-external-write`).** The shared editor-convergence gates now allow direct disk fallback only when no JetBrains/VS Code IPC listener is running. If a listener is active but component convergence has no delta, no ack-content proof, an ack mismatch, no terminal ack, or a send failure, the write logs `<source>_writeback ... transport=blocked reason=... action=refuse_external_disk_write` and fails closed instead of writing behind the editor and triggering File Cache Conflict dialogs. The live-prompt auto-recovery wedge also refuses its adopted-snapshot disk write under an active listener, logging `[jbstalecache] auto_recovery_disk_write_blocked ... reason=editor_ipc_unconfirmed`; no-listener recovery still writes as before. Coverage: `converge_document_or_disk_blocks_disk_fallback_with_active_listener_without_ack_content`, `converge_or_disk_write_blocks_plain_disk_fallback_with_active_listener_without_ack_content`, and `try_auto_recover_live_prompt_drift_blocks_disk_fallback_with_active_listener_without_ack_content`.
- **JetBrains Run Agent Doc polls visible drafts with bounded Enter retries.** Direct-pane route submit now keeps pressing the harness submit key up to three times while the exact routed trigger remains visibly drafted, stopping as soon as the trigger disappears or acceptance is observed. Each retry logs `route_submit_resubmit ... action=submit_key key=Enter result=... attempt=N`, so Codex non-submit reports can distinguish a missed first Enter from a trigger that stayed stuck through the bounded retry loop. Coverage: direct-pane retry-bound tests and updated proof-line assertions.
- **JB Clear Session Context no longer fails when only a stale actor pane can be captured.** Supervisor IPC `/clear` proof now verifies and retries Enter only when the live supervisor reports its actor pane; when the supervisor accepts the command but does not expose a pane id, `agent-doc` logs `session_clear_submit_verification_skipped ... reason=no_supervisor_actor_pane` instead of treating a default-tmux capture failure as proof that `/clear` was not submitted.
- **JetBrains Run Agent Doc no longer accepts a first empty Codex capture as submit proof.** Direct-pane route submit now requires empty input to remain stable before treating the prompt as accepted, so a delayed Codex composer draft can still become visible and receive the shared `Enter` submit-key retry. If Codex later reaches accepted-without-dispatch-start proof and the same routed prompt is visibly drafted, route sends one late `Enter` retry, logs `route_submit_late_resubmit ... cause=dispatch_start_unproven_prompt_visible`, and rechecks dispatch-start proof. Coverage: direct-pane acceptance state tests plus existing routed submit/resubmit proof tests.
- **JetBrains Run Agent Doc route attempts now have end-to-end diagnostic correlation (`#jbrouteattemptid`).** The JetBrains plugin passes its durable Run Agent Doc attempt id into `agent-doc route`, and the binary stamps that id on tmux input events, route submit observations/issues, route latency lines, route pane snapshots, and bounded Enter re-submit proof lines. This keeps the existing tmux-router/session reconciliation layer but makes a live "text typed but Enter not submitted" repro traceable from the click ledger in `.agent-doc/state/editor-route-attempts/` to the exact `tmux_text_enter` / `key=Enter` proof in `ops.log`. Coverage: JetBrains route ledger tests and input-diagnostic attempt-id formatting.
- **Active editor IPC failures no longer fall back to direct document writes.** CRDT stream/finalize, explicit IPC, streaming flush, sidecar-normalization, and IPC dedupe repair paths now fail closed when active socket/file IPC times out, lacks response proof, or cannot prove editor-owned visible repair: the pending response is retained, unconsumed file-IPC patches stay queued for the editor, diagnostics log `recovery=retry_without_disk_write`, and queue consumption/snapshot/CRDT/commit/direct document repair are skipped until a retry succeeds through the editor path. Explicit `--force-disk` remains the operator-controlled direct-write escape hatch.
- **`--force-disk` closeout now covers queue consumption, not just response placement.** A wedged active-listener recovery can place the response body directly but still fail if the follow-up queue-consume phase silently re-enters editor convergence. The strict write/finalize closeout now threads `force_disk` through queue consumption and done-id marking as one controlled escape hatch, so a forced recovery can reach a coherent write+consume+commit boundary. Coverage: `force_disk_closeout_queue_consume_bypasses_active_listener`.
- **Idle queue restart drains ordinary JetBrains heads as triggers unless an opted-in context reset is required.** Added a monsterrodholders regression proving a Codex idle-queue restart drain for an ordinary `Run Agent Doc` head sends `agent-doc <FILE>` with no `/clear` when no reset reason is active; explicit operator clears, `[clean-session]` heads, and opted-in context-threshold/accretion resets can still interleave a harness context reset.
- **JetBrains Run Agent Doc dispatch-only delivery is harness-neutral (`#jbrunparity`).** Dispatch-only Claude Code, Codex, and OpenCode reroutes now use one success policy: accepted shared tmux text+`Enter` delivery is successful and logs `proof=accepted proof_scope=accepted_only` when no stronger proof appears. Codex hook proof and OpenCode pane-state proof still upgrade telemetry to `dispatch_start`, but Codex hook tracking no longer suppresses accepted-delivery progress or gates success differently from the other harnesses. Coverage: `dispatch_only_progress_policy_is_harness_neutral`, `dispatch_only_submit_proof_gate_accepts_enter_delivery_for_all_harnesses`, `dispatch_only_proof_policy_accepts_enter_delivery_for_all_harnesses`, and the ignored live-tmux parity regression.
- **JetBrains Run Agent Doc now trusts the shared Enter submit path and avoids ungated threshold-clear churn (`#jbsimpleroute`).** Dispatch-only reroutes no longer fail the IDE action after tmux accepts the shared text+`Enter` submit merely because dispatch-start proof did not arrive inside the proof window; they complete as `proof=accepted proof_scope=accepted_only`, matching the simpler cross-harness delivery contract. The supervisor idle-queue watch does not clear ordinary queue heads unless the project/document opted into queue context reset and a reset reason is active; explicit operator clears and explicit `[clean-session]` heads remain clear sources. Coverage: dispatch-only accepted-delivery policy tests, `clean_session_head_forces_context_reset_policy`, and focused idle-queue context-reset tests.
- **Stale-supervisor freshness warnings no longer recommend destructive force commands.** The route-owned host supervisor stale-binary warning now points routine refreshes at `agent-doc admin recycle` (idle-boundary recycle) or normal `agent-doc session restart-supervisor <FILE>` (busy panes refuse), and explicitly keeps force/discard recovery scoped to genuinely wedged owners. Compaction stale-supervisor CRDT-interleaving messages were aligned with the same non-destructive guidance, preventing agents from copying stale-binary warnings into an active Codex turn and interrupting it. Coverage: `host_supervisor_stale_warning_message_uses_non_destructive_refresh` plus the controller stale-warning assertions.
- **Closed same-supervisor sessions now replace stale actor state immediately.** When a Codex/OpenCode child is closed and the same route-owned supervisor reports a newer session/generation for the same pane, controller register/heartbeat accepts the newer actor instead of rejecting it as stale. Stale `queue_paused` controls from `stale route-owned supervisor (pid N)` churn are cleared once a newer actor transition or a different live supervisor PID proves the pause was superseded, so JetBrains Run Agent Doc no longer stays blocked behind an already-answered archived head after reopening a session in the same supervisor. Coverage: `controller_supervisor_heartbeat_replaces_closed_same_supervisor_session`, `dispatch_clears_stale_supervisor_pause_that_predates_current_actor`, and the stale-supervisor queue-pause classifier tests.
- **Tmux submit parity is now documented and tested as text plus named `Enter` (`#jbtmuxenter`).** The shared submit profile test now asserts Codex, Claude, OpenCode, and unknown harnesses all build the same tmux command shape: `send-keys -t <pane> <text> Enter`, with trailing `\r`/`\n` stripped from the text. Specs/README now state that tmux paths must never use literal CR/LF submit bytes; the raw child-PTY fallback remains explicitly separate and its diagnostics were renamed to `raw_pty_*_enter_byte`.
- **Response recovery no longer turns assistant proof tails into user prompts (`#resprectail`).** Template repair now strips leaked `❯ ` prompt markers from assistant-owned proof/list lines anywhere inside a `### Re:` response block, including no-pending already-applied recovery, while preserving prompt-like quoted prose. Coverage: `strip_prompt_prefix_from_response_body_first_lines_strips_late_proof_lines` and `repair_without_pending_strips_response_body_prompt_prefixes`.
- **JetBrains Run/Clear actions now share a per-document state machine (`#jbrunclearstate`).** The plugin no longer cancels an alive `Run Agent Doc` route process on a repeated click; the first route process owns the submit/proof wait and later clicks are deduped with a durable `route_already_in_flight` / `route_process_already_in_flight` attempt stage. A Run click during an already-running normal clear queues the latest Run intent until the clear completes synchronously. A binary-deferred clear does not release that queued Run immediately. Coverage: `EditorCommandStateMachineTest` and `starting a route while one is alive keeps the first submitter`.
- **Supervisor context clears now survive JetBrains/Codex restart races (`#jbclearrestart`).** Idle-queue owned `/clear` submits now write a short-lived per-document `context-clear-in-flight` marker. A recycled supervisor treats that marker as authoritative, blocks drains until the clear has settled, and sends one shared Enter-profile submit key when the clear command is still visible in the composer. The marker is cleared after the same `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` fresh-idle debounce used by the in-memory gate, so repeated `Run Agent Doc` restarts no longer stack `/clear` or strand a drafted clear after the watcher loses local state. Coverage: `context_clear_marker_is_active_until_cleared`, `context_clear_marker_ignores_stale_payloads`, `context_clear_marker_resubmits_visible_pending_clear_once`, and `context_clear_marker_blocks_until_settled_idle_prompt`.
- **JB `Run Agent Doc` now fences route submit from idle-queue `/clear` injection (`#jbrouteinflight`).** Route dispatch writes a short-lived per-document `route-in-flight` marker while the editor-triggered `agent-doc <FILE>` submit is awaiting acceptance/proof. The supervisor idle-queue watcher treats that marker as a first-class reset/drain skip reason, logs `idle_queue_watch_skipped ... reason=route_submit_in_flight`, and does not advance clear-settle debounce counters from the pre-submit composer. This prevents the observed `agent-doc <FILE><CR>/clear<CR>` concatenation where the route and idle supervisor wrote into the same Codex prompt window. Coverage: `route_submit_marker_is_active_until_guard_drops`, `route_submit_marker_ignores_stale_payloads`, `idle_queue_context_reset_waits_for_route_submit_to_finish`, `idle_queue_drain_waits_for_route_submit_to_finish`, and focused `route_submit`/`idle_queue` suites.
- **JB `Run Agent Doc` / idle-queue Codex submits use one shared text+Enter tmux operation (`#jbcodexcm`).** `Run Agent Doc`, `/clear`, and idle queue continuation now share `tmux send-keys -t <pane> <text> Enter`, and empty-text re-submit sends only `Enter`. Route, idle-queue, and session-clear one-shot resubmits use the same profile key and log `action=submit_key key=Enter`. The idle context-reset watcher also latches a clear-in-flight across in-place queue head edits, preventing a fresh `/clear` from being sent on every keystroke while an active queue prompt is still being typed. Coverage: `submit_profiles_keep_harness_submit_policy_in_one_place`, `routed_trigger_submit_diagnostic_names_codex_enter_key`, `context_reset_in_flight_dedupes_active_head_edits`, and the Enter-key raw-reader tmux test.
- **The tmux submit profile no longer carries a fake delivery choice.** The follow-up review correctly called out that the one-variant delivery enum made Codex support look more special than it is. `TmuxSubmitProfile` is now a single policy surface: it always emits `tmux_text_enter`, and `Enter` is the diagnostic submit-key label.
- **Workflow invariants now have an autofix planner (`#wfinvautofix`).** Added `agent-doc autofix <FILE>` to consume the doctor report and `workflow-invariant-catalog-v1`, plan invariant-keyed remediations, record `workflow_autofix:<invariant>:<hash>` proof markers in the append-only proof ledger, de-duplicate repeated symptoms by invariant id/fingerprint, and execute only the v1 whitelisted safe repairs under `--apply`. Operator/destructive/manual actions remain gated with exact commands or required proof. Coverage: `cargo test -p agent-doc-orchestration autofix`.
- **Workflow invariants now have a diagnostic doctor command (`#wfinvdoctor`).** Added `agent-doc doctor <FILE>` (alias `diagnose`) to evaluate `workflow-invariant-catalog-v1` against optional preflight/session-check JSON, live session-check inspection, cycle state, ops-log markers, controller freshness, git/snapshot state, parent gitlink drift, and editor sidecars. Each invariant reports `ok`, `recoverable`, `operator`, or `blocked` with exact repair commands or operator actions; missing required evidence is blocked with the command to gather it. Coverage: `cargo test -p agent-doc-orchestration doctor`.
- **Workflow invariants now have a machine-readable catalog (`#wfinvcatalog`).** Added `flow::workflow_invariants` with stable ids, fact sources, ok predicates, disproof markers, severity, safe remediation, operator-gated remediation, and SimWorld/regression coverage for queue continuation, stale supervisor, closeout commit, editor convergence, generation redirect, and parent gitlink invariants. The catalog serializes as `workflow-invariant-catalog-v1` so doctor/autofix work can evaluate data instead of scraping prose.
- **Route/write UI outcomes now use a typed vocabulary (`#archuistates`).** Added the `ui-outcome-v1` user-facing outcome contract with stable tokens for `queued_behind_owner`, `recovered_and_retried`, `deferred_for_operator_proof`, `no_drainable_work`, `real_component_conflict`, and `blocked_with_exact_unblocker`. Route, session-check, controller dispatch-blocked proof payloads, and JetBrains route/write conflict surfaces now emit these fields while preserving legacy prose for compatibility.
- **Closeout recovery transition table coverage is explicit (`#smtransitiontests`).** `CloseoutRecoveryState::ALL` now drives pure decision-table tests for every recovery state, prompt-context priority, and stale-capture supersession proof. A proptest integration guard checks the same policy boundary across generated state/input combinations, and SimWorld now has an umbrella scenario covering queue edits during write fragmentation, stale compaction/full-content sources, stale/sidecar ACK repair, already-applied ACK recovery, and JB `Run Agent Doc` prompt-context queuing during an open closeout.
- **Closeout recovery mutations share one primitive (`#smrecoverymutate`).** `flow::closeout::apply_closeout_recovery_mutation` now owns replay-baseline refresh, sidecar rebuild/reset-from-visible, restore-from-HEAD, and stale-capture retirement mechanics. `capture::validate_replay`, `repair` stale-capture retirement, and metadata recovery now route through that primitive and log `closeout_recovery_mutation ... reason=...`, so queue-only replay, reset-from-visible, and stale-capture retire paths cannot drift in snapshot/CRDT/capture-state side effects.
- **Route consumes typed closeout recovery decisions (`#smrouteconsume`).** Routed/JB pre-dispatch closeout drains now carry `CloseoutRecoveryDecision` through the route boundary instead of surfacing raw capture or snapshot blocker strings. Existing active queue heads wait behind the unresolved closeout with a typed `closeout recovery ...` blocker, prompt-bearing reroutes queue behind the closeout, and terminal failures name the missing proof plus recommended recovery command.
- **Closeout recovery evidence is gathered through one typed API (`#smcloseoutevidence`).** `flow::closeout::gather_closeout_recovery_evidence` now collects the visible markdown hash, snapshot hash, active cycle phase, active capture state, response-body presence or supersession proof, queue-only drift proof, editor live-buffer/IPC degraded state, and controller/supervisor stale-binary warning in one read-only evidence record. `decide_closeout_recovery` now consumes that evidence for stale-capture supersession proof instead of requiring each caller to rediscover it. Coverage: `recovery_evidence_gathers_hash_cycle_capture_and_fresh_editor_state`, `recovery_evidence_proves_queue_only_drift`, and `recovery_evidence_reports_superseded_capture_heading`.
- **Closeout recovery now has a typed action decision boundary (`#smcloseoutdecision`).** `flow::closeout::CloseoutRecoveryDecision` maps the existing recovery classifier into `AlreadyCommitted`, `ReplaySafe`, `RetireStaleCapture`, `ResetSidecarsFromVisible`, `QueuePromptForAfterCloseout`, or `Blocked` outcomes, so route/JB recovery can consume policy-shaped decisions instead of interpreting low-level closeout errors. Route's closeout-block classifier now asks this boundary before queuing an operator prompt behind an unresolved closeout. Coverage: `recovery_decision_maps_states_to_typed_outcomes` plus the route closeout-block decision tests.
- **JB `Run Agent Doc` recovers legacy stale-supervisor queue pauses (`#jbrestale`).** Route now treats a markerless `failed_stage=queue_paused reason=#qchurn ... stale host supervisor pid<N> ...` dispatch error from an old route-owned supervisor the same as the newer `supervisor_restart_redirect` bail: restart the stale supervisor once, lift the pause, and retry instead of surfacing a hard JetBrains error.
- **Tmux submit now has a single shared profile (`#tmuxenter`).** The supervisor, idle queue, and direct-pane submit paths route through one profile-owned submit helper, so a post-restart `/clear` or `agent-doc <FILE>` draft cannot hide a missed submit key behind a successful text write. Current live-pane delivery is the single text+Enter tmux operation; route also preserves redacted pane-output snapshots under `.agent-doc/logs/route-submit/` when an existing draft, missed submit, or accepted-without-proof dispatch needs forensic evidence. Coverage: `submit_profiles_keep_harness_submit_policy_in_one_place`, `pending_payload_enter_resubmit_is_scoped_and_one_shot`, `route_pane_snapshot_preserves_redacted_terminal_capture`.
- **Captured response replay now tolerates queue-only live drift (`#queueeditcap`).** If a turn reaches `response_captured`, the operator edits only `agent:queue`, and the response has not yet crossed write/commit, `validate_replay` no longer deadlocks route/JB Run Agent Doc behind `captured response baseline no longer matches current document`. The replay guard now proves the snapshot still matches the capture and that replacing the live queue body with the snapshot queue body restores the document byte-for-byte, then refreshes only the capture file hash and lets the normal replay path apply the preserved response onto the queue-edited document. Non-queue drift and snapshot drift still fail closed. Coverage: `validate_replay_refreshes_baseline_for_queue_only_drift`.
- **Supervisor auto-install is now scoped to agent-doc dogfood session documents (`#supautoinstall-scope`).** The dogfood crate-root resolver no longer treats every document in a superproject containing `src/agent-doc` as eligible for build/install. Agent-doc sessions under `tasks/agent-doc/`, legacy agent-doc task docs, and docs inside the agent-doc source checkout can still auto-install; sibling project sessions such as `tasks/professional/equityfundingsource.md` and `tasks/software/lazily-rs.md` now resolve no auto-install crate root even if `AGENT_DOC_SUPERVISOR_AUTO_INSTALL` or config/frontmatter is truthy. Coverage: `dogfood_crate_root_rejects_unrelated_superproject_docs`.
- **Controller/admin status now exposes first-class binary freshness proof (`#freshnessstatus`).** `agent-doc controller status` and `agent-doc admin inspect --json` include a `freshness` object with installed binary identity, installed/running inode comparisons, stale/unknown/fresh classification, and operator guidance; plain `admin inspect` prints a compact `freshness=controller:<state>,supervisor:<state>` summary. Coverage: `controller_process_freshness_classifies_inode_identity`, `controller_status_reports_startup_binary_identity`, and `controller_queue_control_rejects_stale_generation_and_blocks_dispatch_when_paused`.
- **JB `Run Agent Doc` no longer silently retries Codex latest-run boot timeouts.** A dispatch-only `latest run is still booting ... (timed_out)` refusal has already spent the route ready-wait window, so the JetBrains plugin now surfaces the persisted route diagnostic immediately instead of re-running the 60s wait up to four times. Authoritative-actor startup failures remain retryable, and active-turn failures still show the still-running notification.
- **Between-turn supervisor handoffs now de-duplicate repeated `/clear` + `agent-doc <FILE>` requests (`#qdedup`).** The idle supervisor path now has a shared set-based planner for fresh-context handoff commands, emits the requested `between_turn_enqueue deduped=N kept=/clear,/agent-doc result=delivered` proof line when a clear-plus-drain sequence lands, and SimWorld proves repeated handoff requests buffer during an active turn then deliver one normalized command set at the idle boundary. Coverage: `between_turn_enqueue_plan_keeps_one_clear_and_one_trigger`, `between_turn_enqueue_plan_counts_concatenated_trigger_duplicate`, `qdedup_between_turn_enqueue_waits_for_idle_and_dedupes_command_set`.
- **The dogfood supervisor refresh stopgap runbook is retired (`#dfrefresh-retire`).** The stale manual make/install/restart fallback has been removed from the bundled runbooks, skill catalog, and installed harness mirrors now that dogfood supervisor auto-install plus stale-binary recycle cover the bootstrap path. The `tsift-memory 0.1.70` dependency is also resolved from crates.io instead of a sibling checkout so CI can build the updated dependency graph.
- **Major dependency constraints are upgraded through the current API surfaces (`#ar27-majors`).** Updated the held-back agent-doc dependency majors for `instruction-files`, `notify`, `portable-pty`, `pulldown-cmark`, `rusqlite`, `sha2`, `signal-hook`, `similar`, `toml`, `ureq`, `yrs`, and `zip`, with API migrations for `ureq 3` response/body handling, `sha2 0.11` digest formatting, and `portable-pty 0.9`'s `MasterPty::tty_name`. The `rusqlite 0.40` move is kept link-safe by aligning the first-party `tsift-memory` path dependency, and the old `generic-array 0.14.7` chain is removed by moving first-party `tagpath` to `sha2 0.11`. Verification: `cargo check -p agent-doc`, affected-crate cargo tests, and full `make check`.
- **JB Clear Session Context now retries drafted Codex/Claude `/clear` commands once (`#jbclearctxsubmit`).** The direct-pane clear path now polls the live pane after sending the harness clear command; if Codex or Claude still show the command in the active composer, it logs `session_clear_submit_observation ... issue=prompt_not_submitted`, sends one bare profile submit key, and records `session_clear_submit_resubmit ... result=accepted|still_visible|capture_failed`. Stale scrollback is guarded by treating a later prompt-prefix line as a newer composer, so an old `/clear` transcript does not trigger a stray submit key. Coverage: `clear_command_visible_detects_codex_active_composer`, `clear_command_visible_treats_empty_composer_as_submitted`, `clear_command_visible_ignores_stale_scrollback_before_idle_prompt`, `clear_direct_submit_retry_is_scoped_to_visible_codex_or_claude_drafts`, `clear_submit_proof_lines_report_prompt_issue_and_retry_outcome`.
- **MCP finalize close-after-capture recovery now has strict CLI regression coverage (`#codexmcpfinalize`).** The MCP finalize test suite now seeds the exact interrupted state left when `agent_doc_finalize` durably captures a response and the transport closes before the write/commit boundary. The next real `agent-doc preflight` must replay the capture, create exactly one closeout commit, clear the pending response, and leave `session-check` green without a manual reset. Coverage: `mcp_finalize_close_after_capture_recovers_on_next_preflight_once`.
- **Queue head removal diagnostics now name the removed head and proof source (`#mrhqueuepreserve`).** The session-check queue provenance guards now log explicit proof for removed id-backed heads (`backlog_resolved_or_removed`, `cycle_lifecycle_outcome`, or `current_directive_target`) and removed free-text heads answered by committed response history. This keeps active-turn queue convergence auditable: preserved additions remain queued, authorized deletions name their source, and missing proof still fails closed. Coverage: `queue_head_removal_guard_logs_proof_source_for_authorized_id_removals`, `free_text_queue_head_guard_logs_response_proof_source_for_removed_head`.
- **Backlog priority queue sync now reports represented ids and has monsterrodholders coverage (`#mrhbacklogqueuesync`).** `agent-doc queue sync` now explains active backlog ids that were skipped because an existing queue prompt already represents the same `#id` (for example `advance [#id]`, not only canonical `do [#id]`), and reports newly materialized ids. The CLI regression covers the monsterrodholders `queue: start` + `agent:queue ... priority go` + `agent:backlog priority queue` shape, commits the synced queue, and proves `session-check` stays clean afterward. Coverage: `test_queue_sync_materializes_priority_go_backlog_and_session_check_stays_clean_after_commit`.
- **Queue consume now refuses stale-position strikes on id-backed heads (`#qmisstrike-regression`).** The queue-consume planner now rechecks the live head before applying a positional free-text strike; if the head has drifted or been reordered onto an id-backed `do [#id]` item, it logs `queue_consume_refused_id_backed_head_without_explicit_signal` and leaves the item runnable unless an explicit `--done`/`--pending-gate`/`--pending-edit` id or pre-commit prompt/heading-target proof matched that same id. Coverage: `qmisstrike_regression_refuses_reordered_id_backed_head_without_explicit_signal`.
- **Supervisor hot-reload now has JB Run Agent Doc proof mapping (`#suprehotreload-agent`).** The live stale-binary recycle success path writes an `ops.log` proof marker, and SimWorld now models a JB `Run Agent Doc` cycle reaching the recycle boundary: success records a preserved-pane fresh-binary proof, while failed `execve` maps directly to `#recyclerestart-verify/#aazp/#4myd` instead of requiring live operator inspection. Coverage: `suprehotreload_agent_maps_jb_run_agent_doc_to_fresh_binary_proof`, `suprehotreload_agent_maps_reexec_failure_to_existing_operator_verify_buckets`.
- **Foreign-owned queue loss now has an agent-verifiable recovery audit (`#lazilyqrestore-agent`).** `agent-doc queue recover-lost <FILE>` reconstructs historical queue heads from the current snapshot/baseline, editor patch sidecars, and git history, then reports restore candidates only when a snapshot/baseline/sidecar-backed prompt is absent from the current queue and not accounted for by its `#id` in the document or done archive. Git-history-only prompts are reported separately as review context, and a zero-candidate report emits explicit `user_removal_or_completion_proof` evidence, so foreign-owned documents such as `tasks/software/lazily-rs.md` can be audited without taking over the live pane. Coverage: `recover_lost_queue_reports_patch_candidate`, `recover_lost_queue_emits_proof_when_id_is_accounted`, `recover_lost_queue_reads_git_history`.
- **Recyclerestart now has agent-verifiable kill-pane + sync-guard proof (`#recyclerestart-agent`).** SimWorld now models the post-install recycle path recording binary-promotion versus session re-clear/drain proof markers, plus a killed-pane `Sync Tmux Layout` path that defers only for a fresh holder and supersedes stale plugin-local guards through the production FFI sync-lock decision. Coverage: `recyclerestart_agent_verifies_kill_pane_sync_guard_and_reclear_proofs`.
- **Exchange compaction no longer recursively replays prior compact summaries or ordered-list response details.** The default compact digest now recognizes an existing compacted `### Session Summary`, carries forward only its compact metadata, and stops before the previous `Prior summary/context` payload. This prevents repeated compactions from duplicating archived response details or surfacing reversed ordered lists in the live exchange summary. Coverage: `exchange_compact_default_summary_does_not_replay_prior_compact_lists`.
- **Codex/Claude idle queue drains now submit already-drafted restart triggers instead of stalling.** When the supervisor idle-watch sees its drain payload or `/clear` already visible in the owned pane composer, it no longer treats that as handled for Codex/Claude. It sends one bare profile submit key keyed by the live queue head, logs `idle_queue_watch_resubmit ... action=submit_key key=Enter`, and keeps the retry one-shot. This closes the restart path where a Codex session came back with `agent-doc <FILE>` drafted but the queue stopped until the operator pressed submit. Coverage: `pending_payload_enter_resubmit_is_scoped_and_one_shot`, existing `idle_queue` filtered tests.
- **Realtime multi-editor broadcast now has headless targeted-delivery proof (`#rtwbcast-simproof`).** `SimEditor` now records per-editor live-buffer sidecars, consumes production targeted broadcast patch files, ignores non-target peer patches, ACK-deletes applied files, and proves JB + VS Code buffers converge without disk writes or conflict markers through `multi_editor_crdt_broadcast_converges_without_file_cache_conflict`. The production `compute_broadcast` seam also fast-paths rebroadcasts where the originator already contains a stale peer's base-relative deltas, preventing CRDT re-merge of already-converged agent-doc component markers. Coverage: `compute_broadcast_rebroadcast_preserves_component_boundaries`, `multi_editor_crdt_broadcast_converges_without_file_cache_conflict`.
- **Clean-session supervisor drains now leave ops-log proof for the forced fresh-context reset (`#cleandrainsup-agent`).** When the supervisor idle-watch owns a `[clean-session]` queue head, the `/clear` it sends before dispatch now emits `idle_queue_watch_context_reset` to `.agent-doc/logs/ops.log` with the document, harness, target, head hash, and `#cleandrainsup` reason. The existing `idle_queue_watch_drain` marker then proves the following dispatch, so the fresh-agent sequence can be verified locally without a live operator replay. Coverage: `clean_session_reset_ops_log_precedes_drain_submit`.
- **Compact Exchange now has agent-verifiable proof for committed response archival (`#compactdrift-agent`).** Added a git-backed fixture for `agent-doc compact <FILE> --component exchange --commit` where `HEAD` contains finalized `### Re:` response history, the compacted exchange converges through the editor-IPC component patch path, and the compact closeout commits without tripping the committed-historical `typed_component_drift` guard. The closeout spec now states that exchange-only archival of already-committed response history is not typed-component drift when non-exchange components are preserved. Coverage: `compact_with_commit_converges_committed_response_head_without_historical_drift_guard`.
- **Codex queue context-threshold clears now use real Codex session token counts (`#clearcodex`).** The Codex Stop-hook continuation path now locates the newest matching `~/.codex/sessions/**/rollout-*.jsonl` for the document project, reads the latest `token_count` event, and computes ctx% from `last_token_usage` plus `model_context_window`. When `agent_doc_queue_context_reset` is enabled and that pct crosses `agent_doc_clear_threshold`, the hook logs `[s760] ... pct=N clear=true`, records a `transcript context ... >= clear threshold ...` reason, and hands the queue continuation to the supervisor via `codex_fresh_context_handoff` instead of continuing inside the already-loaded pane. Missing/unreadable Codex token counts still fail safe with `pct=none clear=false`. Coverage: Codex JSONL parser/locator tests plus Stop-hook threshold handoff regression.
- **Spent prompt-preset queue pauses self-heal on dispatch instead of requiring manual queue edits (`#qpresetstrike`).** A durable controller `queue_paused` reason like `advance-review preset head is spent ... Operator can clear the '- #advance-review' line` is now revalidated against the live document before blocking JB Run Agent Doc / route dispatch. If the preset head is already absent, dispatch clears the stale pause and proceeds; if the registered preset token is still the live head, dispatch consumes it through the canonical queue consumer, clears the pause, and proceeds. Non-spent operator pauses still fail closed, and stale-supervisor pause recovery keeps its separate restart marker. Coverage: `dispatch_repairs_spent_preset_pause_when_head_is_absent`, `dispatch_repairs_spent_preset_pause_by_consuming_present_preset_head`, plus the existing queue-pause/stale-supervisor dispatch tests.
- **Codex Stop-hook fresh-context continuations now hand off to the supervisor instead of punting `/clear` to the operator (`#codexpcphandoff`).** When a Codex queue continuation needs fresh context because the exchange was compacted after the last tracked clear, the hook records the requested head, emits `codex_fresh_context_handoff ... result=queued supervisor=idle_queue_watch`, and allows the turn to close so the existing supervisor idle-watch can perform the clear/settle/dispatch sequence. Normal in-turn continuations still block with MCP/finalize guidance; only the fresh-context branch moves to PCP/supervisor ownership. Coverage: `stop_marker_fallback_requires_clear_after_exchange_compaction`, `stop_tracked_state_hands_fresh_context_continuation_to_supervisor`.
- **JB `Run Agent Doc` no longer stacks repeated Codex reopen text when the same trigger is already drafted.** Before direct-pane dispatch appends `agent-doc <FILE>`, route now checks the recent composer for the exact target trigger. If the trigger is already visible for Codex or Claude, it sends one bare profile submit key and re-polls instead of appending another copy; if an idle prompt appears below the visible trigger, the line is treated as stale scrollback and normal dispatch proceeds. Coverage: `direct_pane_existing_draft_detection_enters_only_current_codex_draft`, `direct_pane_existing_draft_detection_handles_wrapped_codex_path`.
- **Codex idle queue drains now submit the bare `agent-doc <FILE>` reopen instead of a long owner-continuation prompt.** JetBrains `Run Agent Doc` and supervisor idle-queue continuation now share the same harness-native Codex entrypoint (`agent-doc tasks/...md`) instead of injecting a multi-line "Agent-doc active queue continuation" prompt into the TUI. Slash-command queue heads still submit literally, and the queue-continuation response contract stays in the installed harness instructions/runbooks rather than the editor/supervisor payload. Coverage: `idle_queue_drain_payload_uses_trigger_for_codex`.
- **`content_ours` adoption now refuses known-stale supervisors and repairs proven duplicate singleton component blocks (`#dupcontent2`).** The IPC adoption guards for `live_prompt_drift_after_preflight` and `prompt_duplication_in_ack_content` now fail closed when `stale_supervisor_warning_for_doc` classifies the serving controller/supervisor as `supervisor_binary_stale`, leaving the candidate snapshot in place and logging `content_ours_adoption_refused_stale_supervisor` plus an `ipc_proof_insufficient invariant=supervisor_binary_stale` breadcrumb. Separately, the IPC snapshot dedupe pipeline now repairs duplicate singleton components only when the pre-write/before document proves exactly one canonical block and the candidate contains that exact block plus injected duplicates; otherwise the existing structural corruption refusal remains the safety net. Coverage: stale-supervisor refusal tests for both adoption guards and `ipc_snapshot_dedupes_duplicate_singleton_component_from_before_content`.
- **Preset-bearing go queues no longer misclassify genuine prompt lines as stale noise (`#goqnoise`).** `queue_continuation` now treats a queue marker `preset="..."` as supplying the directive verb for each non-empty, non-fenced free-text `Prompt` line, so noun-phrase feature requests and terse lines like `deploy` remain drainable instead of producing `queue_continuation_required=false queue_stale_noise_lines=N`. Fenced console/evidence pastes still self-defer as noise even under a preset queue, and `deploy` is now recognized as a standalone directive verb for non-preset queues. Coverage exercises `detect`, `live_drainable_continuation_head`, `drainable_head_count`, and `queue_stale_noise_lines` on the live CPA/deploy repro shape. Plan: `tasks/agent-doc/plan-go-queue-noise-misclassifies-directive-heads.md`.
- **The controller self-watchdog now reaps a `Stable`-but-stranded handoff replacement, not just a `Preparing` one (`#stuckhandoff2` M1b — fixes the root IPC-drift cause).** Live-repro 2026-06-15: a wedged `controller serve … --handoff-state preparing` orphan survived ~31 minutes racing the IDE/ipc.sock buffer and corrupted a session doc mid-finalize (injected `❯ ` prompt glyphs, spliced duplicated/reordered response lines). ROOT CAUSE (confirmed in code): `promote_handoff` flips a handoff replacement straight to `handoff_state = Stable` + clears `handoff_started_at` the instant the client asks — `ControllerHandoffState::Promoted` is parsed/serialized but NEVER written as a production transition. A client that dies AFTER `promote_handoff` but BEFORE `std::fs::rename(temp_sock → public_sock)` (`handoff_stale_controller`) leaves a `Stable`-in-memory controller stranded on its `controller-handoff-*` temp socket. M1's `controller_self_watchdog_should_suicide` only fires on `Preparing`/`Promoted`, so it could not see the `Stable` orphan — `stale_preparing_controller_self_reaped` had ZERO occurrences ever, and only the slow `/proc`-cmdline gc/M5 sweep reaped these at 7–21 min. FIX (M1b, structural, `rpc.rs`): new `controller_handoff_replacement_is_stranded(handoff_temp_socket, launched_elapsed, threshold)` keyed off the launch socket, not in-memory state — a replacement launched on a temp `controller-handoff-*` socket whose path STILL EXISTS past the threshold proves the promote rename never completed (a healthy handoff removes it), so it self-reaps regardless of `handoff_state`. Wired into the serve-loop `WouldBlock` branch as `should_suicide || is_stranded`. `controller_self_watchdog_suicide` hardened with a generation-ownership guard so a stale stranded generation never clobbers a newer clean controller's shared on-disk record to `Failed` (it still exits to stop the buffer race). Net: a wedged orphan now self-clears within the 45s threshold regardless of when its client died, and the next bind promotes a clean controller — no manual `pkill` / `git checkout` recovery. Coverage: 6 new deterministic unit tests in `project_controller.rs` (stranded-when-temp-persists, not-stranded-after-rename, not-stranded-within-threshold, none-socket-never-stranded, suicide-marks-failed-for-stranded-`Stable`, suicide-preserves-superseded-generation). Plan: `tasks/agent-doc/plan-stuck-handoff-hardening.md`.
- **`agent-doc lib-install` now AUTO-recycles running controllers onto the freshly-installed binary instead of only printing the hint (`#autorecycle-on-install`, upgrades `#ctlrecycle` R4 from print-only to action).** Closes the recurring `supervisor_binary_stale` / `#fcc0` / `#no-mid-session-install` pain: after a `lib-install`, the JetBrains plugin hot-reloads the cdylib by mtime but already-running agent-doc controllers/supervisors keep serving the PRIOR binary until they recycle. R4 only PRINTED `run \`agent-doc admin recycle --all-projects\``; the operator (and dogfood loop) still had to run it by hand. `lib_install::run_paths` now calls `recycle_controllers_all_projects()` directly after a successful install, so every running controller is marked to recycle at its next idle boundary (the same idle-gated `recycle` RPC `admin recycle --all-projects` sends — it fires only at a turn / inter-queue-item boundary, never mid-turn, so triggering it from install is safe). Reports `[lib-install] auto-recycle: N controller(s) marked … M skipped`. Best-effort: a recycle error never fails the install — it logs a warning and falls back to the manual-recycle hint (no swallowed errors). Opt out with a falsey `AGENT_DOC_RECYCLE_ON_INSTALL` (`0`/`false`/`no`/`off`), which restores the print-only hint. Pairs with the supervisor self-recycle (`#supselfheal`) and auto-install (`#supautoinstall`) so a freshly-built agent-doc goes live everywhere without a manual recycle step. Coverage: `lib_install::tests::recycle_on_install_default_on_and_opt_out_is_falsey` (default-on + falsey opt-out resolution; the cross-project process recycle itself is not SimWorld-modelable). Plan: `tasks/agent-doc/plan-proactive-recycle-on-install.md`.
- **The free-text strike repair no longer mis-strikes the next open id-backed queue head (`#qmisstrike`).** Operator report (tsift.md) + live repro (agent-doc-bugs2.md): after a free-text head was consumed (or the operator manually deleted already-struck items), `finalize`'s strike repair struck the *next* still-open head — an id-backed `[#id]` head the response never answered — and `session-check`'s `#queue-clear-unrun-items` guard had to catch it and force a manual un-strike + `reset --from-current`. ROOT CAUSE: `repair::first_queue_head_is_free_text` (the guard gating `strike_recovered_free_text_queue_head` → `consume_queue_prompt_force_disk`, which strikes the leading head by POSITION) used a brittle `do [#` / `do #` prefix test. That test only recognized the `do `-prefixed spelling, so a bare `[#id]` or pin-prefixed `:pushpin: [#id]` / `:round_pushpin: [#id]` head (the spellings the operator's queues carry after manual edits or pin promotion) was mis-classified as *free text* and struck by position. FIX: delegate the guard to the authoritative `write::queue_head_is_free_text_prompt` classifier (which already resolves `#id`, `[#id]`, `do [#id]`, pinned, and preset spellings correctly via `topic_resolves_to_exact_id`), so the repair strike path agrees with the finalize path — an id-backed head is struck only via `--done` / `--pending-gate` / `queue consume`, never by the positional free-text heuristic. Coverage: `repair::first_queue_head_free_text_check_excludes_id_backed_head` (bare/pinned `[#id]` heads classify as not-free-text; a genuine no-`#id` head still strikes; inactive queue strikes nothing) + extended `write::queue_consume::free_text_queue_head_detection` (bare/pinned `[#id]` spellings). Plan: `tasks/agent-doc/plan-finalize-misstrike-next-head.md`.
- **Multi-phase auto-loop policy: routing a phase to review no longer terminates the go-mode drain (`#mphaseloop`).** Operator directive (2026-06-14): a go-mode drain must continuously advance a multi-phase task until it is DONE or legitimately blocked (clean-session / operator-verify / external outage). "Needs review" is NOT a terminal stop — when a phase needs human/external validation, it moves to `agent:review` as a gated `[/]` item and the drain KEEPS advancing the remaining phases/queue; only done/blocked terminate the drain. Codifies the `SKILL.md` `#drain-no-defer` + "complete over gate" policy into the binary closeout: at a successful commit that still owes a drainable queue continuation, when this cycle added an open `agent:review` item relative to the pre-commit HEAD (a phase routed to review rather than completed/blocked), the binary emits `drain_continue_after_review file=… next_head=… (#mphaseloop)` to `ops.log` — the proof that review-routing advanced to the next drainable head instead of stalling the queue. The continuation gate is unchanged (`queue_continuation_required = active && drainable_head_count > 0`); adding a review item never drops a still-drainable head out of the loop. New pure helper `queue_continuation::review_phase_routed(prior, current)` (open-`agent:review`-item delta, review-component-scoped so a growing backlog/queue is never misread as a routed phase). Coverage: `review_phase_routed_detects_added_open_review_item`, `review_phase_routed_counts_only_review_component`.
- **VS Code parity for the editor-save drift resolution (`#jbeditorsavedrift-vscode`).** Completes the follow-up flagged by `#jb-editor-save-resolves-drift` (below), which shipped the JetBrains/IntelliJ socket `save_document` path only. The binary's post-commit carry-forward flush (`flush_editor_buffer_to_clear_drift` in `git.rs`) was socket-only, so a VS Code-only session — which watches `.agent-doc/patches/` instead of running the IPC socket listener — never got the save request and the carry-forward drift recurred. Now the flush prefers the socket (JetBrains) and falls back to writing a `.agent-doc/patches/save-document.signal` file (JSON `{ file, patch_id }`) when no socket listener is active or the send returns no ack, mirroring the existing `vcs-refresh.signal` channel. The VS Code extension's `PatchWatcher` watches that signal, flushes the matching editor buffer to disk via `TextDocument.save()` (clearing the dirty flag), and writes the saved buffer to the ack-content sidecar (`.agent-doc/ack-content/<patch_id>.md`) — the VS Code mirror of the JB plugin's `saveDocumentViaDocument`. Proof line: `postcommit_editor_save_flushed … transport=socket|file_signal` (or `…_skipped` on failure). Spans the binary (`git.rs` file-signal fallback) + the VS Code extension (`extension.ts` `save-document.signal` watcher + new pure `saveSignal.ts` helpers). Coverage: `postcommit_carry_forward_superset_writes_file_signal_without_socket_listener` (Rust), `saveSignal.test.ts` `parseSaveDocumentSignal` / `ackContentSidecarPath` (TS). FlowCore `git.rs` `reason=` budget bumped 9→11 with audit. Live proof (operator-drive, needs a live VS Code window): type unsaved edits into a session doc, run a cycle, confirm the buffer flushes to disk without the same drift recurring.
- **A live-editor-buffer drift now resolves by asking the plugin to SAVE instead of stalling (`#jb-editor-save-resolves-drift`).** Operator directive (2026-06-14): "if you're talking about the JB editor buffer, have the plugin save the file to resolve the drift — we should not stall in this scenario." When `finalize`/`write` detects `live_prompt_drift_after_preflight` (the IntelliJ editor buffer holds unsaved edits ahead of disk, `content_ours_len > candidate_len`), the prior behavior adopted `content_ours` as a **next-cycle carry-forward** snapshot — which stalled the response AND left the editor dirty, so the next cycle re-detected the same drift and could raise a File Cache Conflict when the binary later wrote disk under the still-dirty buffer. Now, in the proven-socket `socket_ack_content` path, once the drift guard adopts `content_ours` the binary sends a new `save_document` IPC request to the live plugin (`ipc_socket::send_save_document`); the plugin runs `FileDocumentManager.saveDocument()` (flushing the buffer to disk AND clearing the editor's dirty flag) and writes the saved buffer to the ack-content sidecar, which the binary adopts as a clean **this-cycle** on-disk snapshot (`FileRead`) via `upgrade_live_prompt_drift_with_editor_save` — committing the response now instead of carrying it forward. Proof lines: `live_prompt_drift_editor_save_requested` → `live_prompt_drift_resolved_via_editor_save` (or `…_unreachable`/`…_no_sidecar` when no live editor answers, in which case it falls back to the prior carry-forward without regressing — never stalls harder). Spans the binary + the FFI cdylib (orchestration `send_save_document` + reconcile, shipped via `cargo build --release && agent-doc lib-install`) + the JB plugin (`save_document` handler in `PatchWatcher.handleSocketMessageV2` → `saveDocumentViaDocument`, plugin `0.2.164`). Coverage: `send_save_document_sends_typed_message_with_patch_id`, `reconcile_live_prompt_drift_via_editor_save_returns_saved_buffer`, `upgrade_live_prompt_drift_with_editor_save_flips_content_ours_to_filewrite`, `upgrade_live_prompt_drift_with_editor_save_no_listener_falls_back`. Live proof (operator-drive, needs a live IDE): type unsaved edits into a session doc, run a cycle, confirm ops.log shows `live_prompt_drift_resolved_via_editor_save` and the response commits without a stall or File Cache Conflict dialog. VS Code plugin parity is a follow-up (this ships the JB/IntelliJ path the operator hit).
- **The in-session `/loop` now drains `[clean-session]` heads in place — `queue_continuation_required` stays true while any non-`[operator-verify]` head remains (`#qcontdrain`). BREAKING CHANGE: clean-session is no longer deferred to the supervisor.** Operator directive (2026-06-14): "you should not have stalled. set queue_continuation_required=true." Supersedes the `DrainScope::Loop` clean-session deferral added by `#goqueuestall`/`#cleandrainsup`/`#freshgrant`: the in-loop agent used to defer `[clean-session]` heads under a live editor-IPC listener and stop, handing them to the supervisor idle-watch. But when the supervisor was itself stalled (the recurring live failure this fixes — `#recyclerestart`/`#suprecyclestall`), nobody drained them and the queue stranded. Now `queue_continuation::deferred_backlog_ids_with_ipc_scoped` and the go-mode backlog→queue sync (`partition_drainable_backlog_ids`) defer ONLY `[operator-verify]` (which genuinely needs a human) in **both** scopes; `[clean-session]` is always drainable, so the `/loop` drains it in the current session rather than stalling. The supervisor still force-`/clear`s before its own dispatches (`head_requires_clean_session`), and the `#freshgrant` short-TTL grant + `_live_ipc`/`DrainScope` plumbing are retained on the signatures but no longer gate the deferred set (follow-up: remove the now-dead grant machinery). `session-check`'s no-response active-head guard now interrupts on a committed-without-response clean-session head regardless of live IPC. SKILL.md auto-loop section + `specs/07-orchestration-commands.md` (`#goqstall2`) updated. Coverage: `deferred_backlog_ids_defers_only_operator_verify`, `loop_drains_clean_session_regardless_of_grant_or_ipc`, `loop_and_supervisor_both_drain_clean_session`, `partition_drainable_backlog_ids_skips_only_operator_verify`, `no_response_active_queue_head_interrupts_on_clean_session_head_regardless_of_ipc`. Live proof (operator-drive): with this build installed, leave only `[clean-session]` heads at an active go-mode queue under a live IDE and confirm preflight reports `queue_continuation_required: true` (the `/loop` drains them) instead of `false`.
- **A supervisor-cleared in-loop agent now DRAINS the `[clean-session]` head it was re-dispatched for, instead of re-deferring it (`#freshgrant`).** Completes the operator directive (2026-06-14): "redefine `[clean-session]` as a fresh agent session — if the loop restarted, it should run; if an idle-install + restart is needed, it should run without stalling." `#cleandrainsup` made the supervisor idle-watch force-`/clear` + re-dispatch a `[clean-session]` head to a freshly-cleared agent, but that freshly-cleared in-loop agent ran preflight in `DrainScope::Loop`, which still deferred `[clean-session]` under a live editor-IPC listener (`deferred_backlog_ids_with_ipc_scoped`) — so `queue_continuation_required` came back `false` and the fresh agent declined the very head it was cleared for, churning a no-op (`#qchurn`). Now: when the idle-watch sends the `#cleandrainsup` `/clear` for a clean-session head, it writes a short-TTL **fresh-context grant** (`write_clean_session_grant_for_head` → `.agent-doc/clean-session-grants/<hash>.json`, ops.log `clean_session_fresh_context_grant … (#freshgrant)`). The `DrainScope::Loop` deferral consults `active_clean_session_grant_ids` and does NOT defer a granted clean-session head — the freshly-cleared agent IS the clean session the tag asks for, so it drains that head. `[operator-verify]` stays deferred in both scopes regardless of any grant. The grant is bounded (`CLEAN_SESSION_GRANT_TTL_SECS = 600`): an un-drained grant expires and reverts to the deferral fail-safe so it can never enable in-loop clean-session drains in a later accreted session. Pairs with the already-landed auto-install + recycle (`#supautoinstall`/`#supselfheal`) so the binary change a clean-session item needs is installed + recycled without stalling the drain. Coverage: `loop_drains_granted_clean_session_under_live_ipc`, `clean_session_grant_roundtrips_filters_and_expires`. Live proof (operator-drive): with this build installed, leave a `[clean-session]` head at the active queue head under a live IDE, let the supervisor idle-watch fire `idle_queue_watch_context_reset … (#cleandrainsup)` + `clean_session_fresh_context_grant … (#freshgrant)`, and confirm the re-dispatched agent's preflight reports `queue_continuation_required: true` for that head (drains it) instead of `false`.
- **Exchange clear/compact keeps stale HEAD out of the durable merge base (`#clearexchstale`).** The exchange compaction/clear boundary now has explicit regression coverage for the stale-HEAD revival class: stale editor-cache/live-buffer proof still fails closed before any document or snapshot write, and a successful empty exchange replacement advances the snapshot to the cleared document so later preflight/repair paths cannot replay the archived pre-clear exchange. Coverage: `component_compact_rejects_stale_editor_cache_when_snapshot_is_stale`, `component_compact_empty_message_advances_snapshot_after_exchange_clear`. Plan: `tasks/agent-doc/plan-jb-clear-exchange-stale-head-revival.md`.
- **The supervisor auto-installs the agent-doc source after finalize, DEFAULT-ON in dogfood sessions (`#supautoinstall`/`#r18j`).** Closes the bootstrap gap behind `#supselfheal`/`#ctlrecycle`: a stale supervisor could self-recycle onto a freshly-installed binary, but nothing *installed* the binary after the operator edited agent-doc source — the dogfood loop still required a manual `cargo install`. The idle supervisor now runs an install rung after finalize (in the idle supervisor process, never the finalize client) that rebuilds + installs the agent-doc source, emitting `supervisor_auto_install_started` → `supervisor_auto_install_succeeded`, after which the existing `#supselfheal` staleness check fires `supervisor_binary_stale_self_recycled` and the session continues on the new build. The install rung precedes the `#ctlrecycle` recycle rung at the idle boundary. Dogfood-only — it never fires for non-dogfooding documents. Opt out with a falsey `AGENT_DOC_SUPERVISOR_AUTO_INSTALL` env / `agent_doc_supervisor_auto_install` frontmatter / `.agent-doc/config.toml` knob. Live proof (operator-drive, needs a live editor + supervisor restart): edit agent-doc source in a dogfood session, finalize, confirm the three ops.log lines and that the session keeps running on the new binary. Plan: `tasks/agent-doc/plan-supervisor-auto-install-after-finalize.md`.
- **The supervisor now self-drives `[clean-session]` queue heads instead of deadlocking under a live editor (`#cleandrainsup`).** Root-fixes the go-mode auto-loop stall the operator hit with the IDE open: a queue full of `[clean-session]` backlog items never drained. `queue_continuation::deferred_backlog_ids` deferred a head when `operator_verify_required || (clean_session_required && live_ipc)`, and **both** consumers shared that set — the in-session Claude Code `/loop` (`drainable_head_count` → `queue_continuation_required`) AND the supervisor idle-watch (`live_drainable_continuation_head`). With the IDE open (`live_ipc == true`) every `[clean-session]` head was deferred in both paths, so the loop stopped and the supervisor's `active_head` was `None` — nobody ever drained them, even though SKILL.md promised "the supervisor owns the `/clear` and re-dispatches the next item to a freshly-`/clear`ed agent." The drainability filter is now scoped (`DrainScope::Loop` vs `DrainScope::Supervisor`): the in-session loop still defers `[clean-session]` under a live IPC listener (it cannot give the head the fresh context the tag asks for, so it stops and hands off), but the supervisor idle-watch defers only `[operator-verify]` and DRAINS `[clean-session]` heads — force-`/clear`ing before each dispatch (`head_requires_clean_session` → `clean_session_head_forces_context_reset`, independent of the `agent_doc_queue_context_reset` opt-in) so each clean-session item runs in a genuinely fresh agent context. `[operator-verify]` stays deferred in both scopes (genuinely needs a human). SKILL.md auto-loop section updated to document that stopping on `queue_continuation_required == false` for clean-session heads hands them to the supervisor rather than abandoning them. Coverage: `deferred_scoped_supervisor_drains_clean_session_under_live_ipc`, `head_requires_clean_session_maps_head_id_to_tag`, `clean_session_head_forces_context_reset_policy`. Plan: `tasks/agent-doc/plan-cleandrainsup-supervisor-drains-clean-session.md`.
- **A stale route-owned supervisor now self-recycles onto the freshly-installed binary by default (`#supselfheal`). BREAKING CHANGE: supervisor auto-recycle defaults ON.** Automates the "harder recovery path." Previously a supervisor that went stale after a `cargo install` only logged `supervisor_binary_stale_detected` and kept re-filing File Cache Conflict / IPC-drift dialogs (`#fcc0`/`#ipcdrift`) until an operator manually ran `restart-supervisor` (which itself fails `generation N is closed` when the wedged controller can't cooperate), `interrupt-clear --force`, or `kill <pid> && agent-doc start`. The turn-boundary blue/green `execve` self-recycle machinery already existed (`#ctlrecycle` R3 / `#supkill-bg`) but was gated behind the opt-in `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` / frontmatter / project knob. `resolve_supervisor_auto_recycle` now defaults that resolution to ON, so a stale supervisor recognizes its own staleness (`process_binary_is_stale`) at the next turn / inter-queue-item boundary and replaces its own image with the fresh binary **in place**, preserving the live harness child + pane (zero-gap red/green — no dropped turn, no window without a supervisor). Safeguards retained: recycle fires only at a turn boundary (`prompt_visible && !turn_active`), never mid-turn; it is debounced at idle so a momentary lull never thrashes the child and fires immediately only at the deliberate inter-queue-item restart point; and a failed `execve` disables further attempts for the process lifetime (`#suprecyclestall`, never `process::exit`). Opt out with a falsey `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` env / frontmatter / `.agent-doc/config.toml` `agent_doc_supervisor_auto_recycle = false`. **Bootstrap caveat:** a supervisor ALREADY running the pre-`#supselfheal` binary cannot self-heal retroactively (old code lacks the default-on logic) — restart it once onto a `#supselfheal` build, then it is hands-off. Coverage: `stale_supervisor_self_recycles_at_turn_boundary_by_default`, `opted_out_document_clears_and_drains_without_auto_recycle`, updated `resolve_supervisor_auto_recycle_precedence` default-on assertions.
- **`start --route-owned` no longer wedges on a pane-move against an up-to-date controller actor (`#startgencas`).** Root-fixes the recurring live `Error: project controller command \`start_session\` failed: controller failed to start session actor` wall. `start/run.rs` computed the new ownership generation in two branches: when the launcher pane matched the registry it bumped (`next_generation` = `infer + 1`), but when the launcher pane differed from the registry's *stale* pane it handed the controller the **un-incremented** current generation via `infer_latest_generation`. The controller's `start_session` CAS unconditionally expects `start_generation - 1` as the prior generation, and `infer_latest_generation` returns `max(controller-actor gen, session-log gen)` — so once the controller actor caught up to the inferred latest (the normal healthy steady state) the no-bump value *equalled* the live generation and the CAS failed closed (`compare-and-swap failed: expected N-1, found N`), surfacing as the wrapped "failed to start session actor" error. A pane move IS an ownership transition, so `start` now always takes the `next_generation` (`infer + 1`) path and logs the `ownership_transition`, keeping the value handed to the controller aligned with the CAS contract regardless of registry-pane staleness. The fix lives in the `start` process (which computes the generation it sends), so it takes effect on the next `agent-doc start` without a controller restart. Coverage: `start_after_pane_move_against_up_to_date_actor_bumps_past_cas` (seeds an up-to-date actor at gen N, asserts the un-bumped start fails the CAS and the bumped start passes and re-asserts ownership on the new pane).
- **`start` no longer wedges forever once the session log runs ahead of the committed actor generation (`#startgenlogdrift`).** Follow-up to `#startgencas` — the same `controller failed to start session actor` wall, but a self-sustaining variant. `start/run.rs` writes the `session_start` / `ownership_transition` log lines with the *intended* generation BEFORE the controller commits the start, so any start that loses the controller CAS still appends its un-committed generation to the session log. `infer_latest_generation` returned `max(controller-actor gen, session-log gen)`, so one failed start left the log one ahead of the committed actor generation; the next start then inferred that inflated value, bumped past it, and the controller CAS rejected it again (`expected <log>, found <db>`) — appending a yet-higher generation and running the divergence away (observed live: actor stuck at gen 85 while the log climbed 86→98, every `start` failing). The committed controller actor record is the sole CAS authority, so `infer_latest_generation` now returns it directly whenever a record exists and consults the optimistic session log only as a bootstrap/legacy fallback before the control plane has any record for the document. This also self-heals an already-diverged session: the next `start` reads the committed generation, bumps once, and the CAS clears — no manual log/DB surgery. Coverage: `infer_latest_generation_ignores_optimistic_log_ahead_of_actor_record` (committed actor at gen N, session log inflated to N+10, asserts inference stays at N and the next generation is N+1).
- **Supervisor auto-recycle is now configurable per project AND per document (`#suprecyclecfg`).** The route-owned supervisor's `execve` hot-reload onto a freshly-installed binary (`#ctlrecycle` R3 / `#suprecyclequeue`) was opt-in only via the `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` env var and the project-config `agent_doc_supervisor_auto_recycle`. A per-document frontmatter `agent_doc_supervisor_auto_recycle: <bool>` now slots into the resolution between them, so a single document can explicitly opt in or out independent of the project default. Resolution precedence: env var (truthy/falsey force) → frontmatter → project config → built-in default (flipped to ON by `#supselfheal`, above). `resolve_supervisor_auto_recycle` takes the new frontmatter arg and `supervisor_auto_recycle_enabled` reads the doc's frontmatter; spec (`specs/06-config.md`) and the project-config doc comment list the full precedence. Coverage: extended `resolve_supervisor_auto_recycle_precedence`.
- **A benign in-flight dispatch coalesce reports deduped-success instead of exit-1 (`#qflood2`).** When `JB Run Agent Doc` (or any route dispatch) hits an identical dispatch for the same cycle already in flight, the controller correctly suppresses the re-send (never piling the trigger into the busy pane), but the route caller surfaced that suppression as `project controller command \`dispatch\` failed: dispatch coalesced … (#qflood)` — an exit-1 that read as an error and made the operator manually clear the queued prompt. The coalesce is a benign dedup (the requested work is already running), so it now reports success: the coalesce bail carries a stable `failed_stage=coalesced_in_flight` marker, `authorize_controller_dispatch` classifies it across the IPC boundary into a typed `RouteDispatchAuthorization::CoalescedDeduped` outcome, and every route dispatch site returns the already-running dispatch pane via `route_dispatch_deduped_pane` **without re-sending** (logged `route_dispatch_deduped reason=in_flight_coalesce`). The new enum makes the deduped case a compile-time-exhaustive match at all four send sites, so no path can fire a re-send on a coalesce (which would be the flood) and a coalesce can never dead-end as exit-1. The send-suppression (the actual flood guard) is unchanged; only its reporting changed, matching the SimWorld model that already returns deduped-success. The live multi-process collision repro stays operator-drive. Coverage: `qflood2_coalesce_marker_survives_ipc_wrapping`, extended `qflood_coalesces_busy_in_flight_redispatch_and_releases_on_ready`. Plan: `tasks/agent-doc/plan-queue-dispatch-flood.md`.
- **Proactive recycle-on-install: idle controllers/supervisors self-recycle onto a freshly-installed agent-doc (`#ctlrecycle`).** Root-fixes the recurring "already-running agent-doc keeps serving the OLD binary after `cargo install` until manually restarted" churn (the `#ctlstalebin` dispatch reject was only a per-dispatch backstop). Each long-lived process now compares its launch identity against the installed binary (`current_binary_identity` stats the install *path*, so it sees the new build while still running the old mapped inode) and recycles itself when idle, debounced (`AGENT_DOC_RECYCLE_IDLE_GRACE_SECS`, default 5s; fail-open on stat errors). **R1** — the controller serve-loop idle poll exits with `controller_self_recycled reason=stale_binary` once no dispatch is in flight for any document (`has_any_open_in_flight_dispatch`) and it is `Stable`; the next `connect_or_launch` relaunches the fresh binary (state is on disk). **R2** — `agent-doc admin recycle [--project-root R] [--all-projects] [--json]` marks running controllers to recycle at their next idle boundary (a `recycle` RPC, idle-gated so it never interrupts a turn) — deterministic for the release flow (`cargo install && agent-doc admin recycle --all-projects`). **R3** — the `start --route-owned` supervisor self-recycles from its idle-queue watch when idle + stale, **opt-in behind `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE`** (it ends the live agent child, so default OFF logs `supervisor_binary_stale_detected` instead); chose clean `process_exit`-and-relaunch (`supervisor_binary_stale_self_recycled via=process_exit`) over re-exec for safety. **R4** — `agent-doc lib-install` prints the recycle hint. Coverage: `process_binary_is_stale_matches_and_differs`, `recycle_debounce_decision_requires_continuous_idle_grace`, `supervisor_stale_action_policy` (pure predicates + sentinel pattern; process recycle is not SimWorld-modelable). Plan: `tasks/agent-doc/plan-proactive-recycle-on-install.md`.
- **Stale-binary dispatch backstop recycles a running controller onto a freshly-installed agent-doc (`#ctlstalebin`, #stuckhandoff2 follow-up).** A controller whose own recorded `controller_binary` no longer matches the installed binary keeps serving OLD code. `connect_or_launch` already hands *cross-process* callers off to a fresh controller (the common `cargo install` recycle), but a dispatch that still reached the stale controller's `handle_dispatch` (in-process co-host, or a narrow handoff race) was admitted — letting an old-binary controller keep driving session writes until a manual restart (the operator's observed ~1h churn after a `cargo install`). `handle_dispatch` now refuses such a dispatch with a `controller_binary_stale` rejection receipt + `dispatch_refused_stale_binary` ops.log line, and `authorize_dispatch` retries the dispatch exactly once so the retry's `connect_or_launch` promotes the fresh binary through the two-phase handoff (`dispatch_retry_after_stale_binary`). Fail-open on any binary-stat error — a transiently-unreadable path never blocks a live dispatch. Coverage: `dispatch_refused_when_controller_binary_stale`. The in-process `start --route-owned` supervisor's own self-recycle (its whole process is the old binary) remains a separate follow-up.
- **Queue-consume editor IPC is node-keyed and generation-fenced (`#queueconsume-stale-fence`).** The shared editor convergence path now tags socket queue-consume payloads with both the raw baseline hash and a transient-marker-normalized baseline hash, and expresses the queue head completion as an exact markdown-AST `node_patches` strike instead of a broad legacy `queue` component replace. JetBrains enforces the generation fence against the live editor buffer before mutation, so a delayed socket/file patch from an earlier run is dropped once the queue has moved on while benign `(HEAD)` / boundary / guard / pipeline marker churn still passes. When no editor listener is running, the CLI falls back to the guarded disk write; when a listener is active and cannot apply/prove the node patch, it fails closed rather than replaying a stale queue replacement or writing behind the editor. Coverage: `queue_consume_editor_convergence_payload_is_node_keyed_and_fenced` and `PatchGenerationFenceTest` normalized-hash cases.
- **Finalize-tolerance + post-commit-repair groundwork; behavior-flip rungs held for operator confirmation (`#fintol`/`#pcwc`/`#rtwbcast`).** Three phases from `plan-clean-exchange-run.md` landed only their pure, seam-isolated, non-behavior-changing groundwork after the wiring was found to conflict with a deliberate shipped invariant:
  - `#fintol1` — `write::response_target_disjoint_from_user_edit(baseline, content_ours, candidate)`, a pure conflict-scope primitive proving (via a confined-outside-`exchange` check plus a conflict-free 3-way merge that preserves both sides) whether a concurrent user edit is disjoint from the response target. Unit coverage `write::tests::fintol_*` (disjoint queue edit / response-body-rewrite collision / new-exchange-prompt / no-edit). NOT wired into the commit path: today's gate already preserves a disjoint outside-`exchange` edit by carrying it forward UNCOMMITTED (the finalize succeeds) — a shipped invariant asserted by `finalize_preserves_late_comment_tail_edit_outside_exchange_uncommitted` (+6 integration tests). Forward-merging it into the same cycle's commit would flip commit-now vs carry-forward, so `#fintol2`/`#fintol3` are held for an operator decision against a live Phase-0 re-baseline.
  - `#pcwc` — post-commit worktree corruption now auto-repairs only the proven lost-committed-content class: when the working tree drops committed HEAD lines and adds no carry-forward user directive, `emit_postcommit_worktree_check` restores the file to HEAD and sends a guarded `refresh_content` socket message so a live JetBrains buffer stops writing the stale payload back. The editor applies that refresh only when the live buffer still matches the stale hash/length, preserving legitimate carry-forward supersets and concurrent user edits. Coverage: `postcommit_worktree_*`, `RefreshContentPreconditionTest`, and the hot-path token budget update for the new `reason=` logs.
  - `#rtwbcast` Option C — `realtime_model::compute_broadcast(base, originator, peer) -> BroadcastMerge { merged, originator_echo_suppressed }`, the pure MERGE-ONLY multi-editor convergence seam (no delivery). The SimWorld `multi_editor_crdt_broadcast_converges_without_file_cache_conflict` test now drives this production seam instead of an inline `merge_contents_crdt`. Coverage `realtime_model::tests::compute_broadcast_*`. Full multi-editor delivery (Options A/B — an `editor_id` FFI ABI bump, per-editor sidecars/patch files, both editor plugins, a two-live-IDE verify) remains an operator design decision.
- **Model-projected snapshot baseline — Rungs 2-4, now default (`#mps`).** Re-homes the merge baseline (the `--baseline-file` common ancestor the finalize merge uses) from the on-disk `.md` file onto the structured document model. **On by default**; opt out with `AGENT_DOC_MPS=0` (`false`/`no`/`off`) for the pure `.md` path. Defaulting on is non-regressing by construction while the `.md` remains the cross-check cache: the model projection is used only when it is byte-identical to the legacy `.md`, and the proven `.md` wins on any divergence. Rung 2 (pin): `preflight`'s `save_baseline_content` also persists the baseline as an overlay sidecar (`baselines/<hash>.overlay.yrs`) via `snapshot::save_baseline_model`, logging `mps_baseline_pin`. Rung 3 (flip): `write::read_explicit_baseline` sources the base by projecting that overlay (`snapshot::load_baseline_model`), logging `mps_baseline_resolve source=model|md_backstop|md_fallback` and, on disagreement, a loud `mps_baseline_divergence` (with first-differing byte) while preferring the `.md` backstop. Rung 4 (derive): the `.md` baseline is the derived cross-check cache, no longer the read authority — its removal (making the model standalone) is the remaining step, gated on production divergence logs staying clean. The baseline overlay is a separate sidecar from the crdt-runtime overlay so the cutover never perturbs the stream/crdt write merge, and is migrated on rename. Coverage: `snapshot::tests::mps_baseline_model_*` (round-trip, absent→None, md-backstop-on-divergence, projection-when-no-md, idempotent delete, first-diff-byte); the full 4317-test suite now runs through the model path. Verified end-to-end through the installed binary with no env set: preflight pins the overlay + emits `mps_baseline_pin`, finalize resolves `source=model diverged=false`, response merges and commits; `AGENT_DOC_MPS=off` writes only the `.md`. FlowCore hot-path token budget bumped +2 (`reason=no_model|model_error`) with audit.
- **Model-projected snapshot baseline — Rung 1 shadow instrument (`#mps`).** First, zero-behavior-change rung of the migration that re-homes the merge baseline from the on-disk `.md` snapshot onto the structured document model (`tasks/agent-doc/plan-model-projected-snapshot.md`, successor to the superseded `#snbc`). Adds `snapshot::overlay_projection_is_byte_stable` — a pure check that round-trips content through the same `OverlayCrdtDoc::from_markdown → encode_state → decode_state → to_markdown` pipeline the merge base (`crdt_merge_base_state`) uses — and an env-gated shadow probe wired at the central `snapshot::save` funnel (`AGENT_DOC_MPS_PROJECTION_PROBE=1`) that emits a grep-able `mps_projection_equiv ok|drift …` ops.log marker on real traffic without imposing overlay encode/decode cost on the default hot path. This proves the migration's load-bearing precondition (byte-stable projection; obstacle 2) before any cutover. Offline evals (`snapshot::tests::mps_projection_byte_stable_*`) confirm byte-stability for inline, template/queue, exchange-append, boundary-marker, empty, and unicode shapes. No merge outcome or persisted artifact changes; reads from the overlay remain shadow-only.
- **Deterministic SimWorld editor + tmux integration harness (`#swint`).** Added `SimEditor` to `src/sim_world.rs`: a deterministic editor-buffer actor that speaks the production durable live-buffer protocol (`debounce::record_live_buffer_digest_content`) and reads "current document" back through the production `realtime_model::resolve_current_doc` seam, so the File-Cache-Conflict / IPC-drift / queue-flood classes (previously live-IDE-only) are now regression tests. Slice 1 — `simeditor_unsaved_buffer_edit_resolves_to_editor_buffer_and_survives_commit` is the deterministic `#rtwverify` proof (unsaved buffer wins, emits the `realtime_doc_resolve authority=editor_buffer` ops.log marker, edit survives commit) plus `simeditor_save_then_close_falls_back_to_disk_authority`. Slice 2 — `simeditor_jb_and_vscode_buffer_authority_parity_with_kind_specific_conflict` (JetBrains/VS Code agree on read authority, differ only on the surfaced `CacheConflict` signal). Slice 3 — `multi_editor_crdt_broadcast_converges_without_file_cache_conflict` drives two emulated editors through `merge::merge_contents_crdt` (`#rtwbcast`). Slice 4 — `integrated_editor_edit_routes_drains_under_drain_owner_gate_and_broadcasts_back` connects the editor seam to the route/controller model and the public `drain_owner` lease (`#kp5z`). New production primitive `debounce::clear_live_buffer` models the editor-close lifecycle (clears the sidecar so the cycle falls back to disk); coverage `clear_live_buffer_removes_sidecar_and_is_idempotent`. Spec: `specs/12-deterministic-simulation.md` (`#swint` section).
- **Compact Exchange now converges through the editor instead of a direct disk write (`#w42v`).** `compact::apply_compacted_document` (the single replacement boundary for all compact paths) previously wrote the full compacted document to disk via `atomic_write_if_current_pub`, which diverged from an open JetBrains buffer and raised a `File Cache Conflict`. It now calls `write::try_compact_editor_converge` first: when a JB/VS Code IPC listener is active it sends component `op:replace` patches for the changed components (mirroring the `#q7jm` live_prompt_drift convergence — never `fullContent`) and verifies the editor ack matches the compacted target, logging `compact_writeback transport=editor_ipc`. The guarded direct write is now only the no-listener fallback, logging `compact_writeback transport=disk_fallback reason=no_listener` (or `reason=listener_degraded` only while no listener is accepting connections); active-listener no-delta/no-ack/mismatch/send failures log `transport=blocked` and fail closed. Spec updated in `07-closeout-commands.md`; coverage: `try_compact_editor_converge_falls_back_to_disk_without_listener`. Live JB verification (conflict gone) is operator-drive.
- **Dispatch-source observability for the queue-flood diagnosis (`#kp5z`).** `project_controller::authorize_dispatch` (the single production funnel every queue-continuation dispatch passes through) now logs `queue_dispatch_invoked file=… pane=… generation=… command_kind=… payload=…` on every invocation. Pure observability (no behavior change) so an operator flood repro reveals which caller re-invokes `dispatch` while the pane is mid-turn, pinning the source before a busy-gate/dedupe fix.

- **JB `Run Agent Doc` and `Clear Session Context` on Codex now use the shared tmux submit profile (`#jbcodexsubmit`).** The shared `sessions::send_submitted_text_for_harness` live-pane path sends Codex command text through the same profile as queue drains and clear retry. Current diagnostics report `tmux_text_enter` / `Enter`; the existing one-shot resubmit remains as bounded recovery if an older draft is already visible. Coverage: `submit_profiles_keep_harness_submit_policy_in_one_place`, `routed_trigger_submit_diagnostic_names_codex_enter_key`, and `simworld_jb_run_and_clear_share_codex_enter_submit_contract`.
- **JB Stale File Cache recovery now converges through editor IPC first (`#q7jm`).** The `live_prompt_drift_after_preflight` auto-recovery path no longer writes the adopted snapshot directly to disk while a JetBrains listener is active. It now sends component `op: "replace"` convergence patches through the editor, verifies ack-content against the adopted snapshot, and logs `transport=editor_ipc`; the direct disk write remains only as the no-listener `transport=disk_fallback` path. If the listener is active but editor convergence is unproven, recovery logs `[jbstalecache] auto_recovery_disk_write_blocked` and fails closed. JetBrains and VS Code patch appliers now honor explicit component `op` overrides ahead of marker `patch=append` / `mode=append`, so convergence can replace an append-mode exchange body without duplicating the response. Coverage: `try_auto_recover_live_prompt_drift_prefers_editor_ipc_when_listener_active`, `live_prompt_drift_convergence_patches_builds_replace_patch_for_exchange`, and editor-side `op` override wiring tests.
- **Codex auto-queue owner continuations report their harness submit mode.** The idle queue watch now logs `idle_queue_watch_drain` with the actual submit mode and file-scoped prompt hashes, while Codex Stop-hook queue blocks log `codex_stop_queue_continuation` proof for tracked-state and durable-marker sources. Codex live-pane deliveries use the shared text+Enter tmux submit path. This closes the drafted-but-not-submitted owner-continuation path from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Queue convergence IPC now carries the queue body (`#mrhpcdrift`).** The
  preflight halt/drain convergence patch now sends the corrected `agent:queue`
  component body alongside `queue_auto` and canonical `queue:` frontmatter.
  This closes the live-editor gap where an open IntelliJ buffer could accept
  tag/frontmatter convergence yet keep stale queue lines and flush them back
  over the disk/snapshot repair, regenerating IPC drift on the next preflight.
- **Structured live-buffer-equals-disk marker carries proof fields
  (`#lvbremain`).** `visible_write_live_buffer_matches_disk` now logs
  `source`, `expected_len/hash`, `disk_len/hash`, `live_len/hash`, and
  `live_ts` when the editor live-buffer digest diverges from the merge base but
  exactly matches the current disk content. That makes the IntelliJ
  edit-during-finalize proof an anchored, self-contained `ops.log` line instead
  of a token that can be confused with `queue_diff_active_prompt_differs` prose.
  Regression `visible_write_reconcile_treats_editor_matching_disk_as_reconcilable_drift`
  now asserts the structured fields.
- **Structured `#s760` clear-decision gate verifier (`#ktw8`).** Added the
  built-in `s760_clear_decision_clear_true` ops-log verifier for destructive
  queue-turn `/clear` proof. It accepts only anchored
  `^[epoch] [s760] clear-decision ...` lines with `optIn=true`, numeric
  `pct >= threshold`, and `clear=true`; prose-only mentions inside
  `queue_diff_active_prompt_differs` remain pending, and structured false fires
  (`clear=true` below threshold or no-clear at/above threshold) fail closed.
  Regression coverage:
  `scan_s760_clear_decision_requires_anchored_clear_true_at_threshold`,
  `scan_s760_clear_decision_ignores_queue_diff_prose_only`,
  `scan_s760_clear_decision_fails_clear_true_below_threshold`,
  `scan_s760_clear_decision_fails_clear_false_at_threshold`,
  `gate_verify_s760_builtin_ignores_queue_diff_prose_only`, and
  `gate_verify_s760_builtin_auto_resolves_on_anchored_clear_true`.
- **08b cutover COMPLETE — removed the out-of-process writers and flipped all three authority
  defaults (removal rung).** This retires the gate ladders now that in-process hosting + ordered
  write queue + plugin read-only are the only behavior; **there is no rollback flag.**
  - **Write authority:** every editor-visible `.md` write routes through the session actor's single
    ordered write queue unconditionally. Removed `AGENT_DOC_WRITE_AUTHORITY`, the `WriteAuthorityGate`
    enum, `current_gate`, and the bare-`atomic_write` `off` bypass. `.agent-doc/` sidecar/snapshot
    writes and owner-thread-reentrant writes still take the raw path. The process-wide session-actor
    runtime lazily spawns a per-document owner thread on first write, so standalone `agent-doc write`
    serializes correctly without a running `start` session.
  - **Supervisor host:** `agent-doc start` always hosts the harness child through
    `supervisor::in_process::InProcessSupervisor`. Removed `AGENT_DOC_SUPERVISOR`,
    `supervisor_authority.rs`, and the out-of-process `session.wait()` host branch in `start.rs`. The
    external Unix-socket IPC boundary (CLI/editor callers) is unchanged.
  - **Plugin watch:** the JB plugin WatchService file-apply path is unconditionally read-only.
    Removed `AGENT_DOC_PLUGIN_WATCH` and the `active` path; `agent_doc_plugin_watch_readonly` always
    reports read-only (`watch_authority::plugin_watch_is_readonly`). The 0.2.159 plugin already calls
    this export, so no plugin rebuild is required — the cdylib refresh (`lib-install`) flips it live.
  - `make check` green; gate-flag unit/SimWorld tests rewritten to the unconditional end state.
  - **BREAKING CHANGE:** removes the `AGENT_DOC_WRITE_AUTHORITY` / `AGENT_DOC_SUPERVISOR` /
    `AGENT_DOC_PLUGIN_WATCH` rollback flags. Setting them now has no effect.
- **Demote the JetBrains plugin WatchService to read-only buffer reporting (`#dsqa` / `#pcp7` —
  08b cut-over residual phase 2).** New `AGENT_DOC_PLUGIN_WATCH` rollback flag
  (`watch_authority.rs`, mirroring the `#dav9` / `AGENT_DOC_SUPERVISOR` ladder):
  `active` (default = today's behavior) / `read-only` (demoted). At `read-only`, the JB
  plugin's autonomous NIO `WatchService` file-apply path no longer applies patches it
  observes under `.agent-doc/patches/` — the single controller-owned watcher (`#pcpc4`) plus
  the socket IPC command channel become the sole writer to the live editor buffer, killing the
  second-watcher race that produced `live_prompt_drift_after_preflight` /
  `ipc_socket_already_applied_live_buffer_diverged` even under in-process hosting (the `#dav9`
  swap moved *who hosts the child*, not *who writes the buffer*). The plugin reads the flag fresh
  via the new `agent_doc_plugin_watch_readonly` FFI export (the flag lives in the binary, so the
  operator opts in once on the `agent-doc` session and every IDE instance honors it without
  depending on the IDE's inherited environment); the export emits a structured
  `plugin_watch_readonly` `ops.log` marker so the cut-over is log-verifiable (`#q6js` /
  `#lvbremain`). **Default `active` is unchanged** — shipped users keep the WatchService applier
  until an operator opts in, and the gate fails safe back to `active` on an unrecognized value so
  a typo can never strand a live editor without an applier. The socket IPC apply path (the
  controller's writer arm into the editor) stays active in both modes. JB `pluginVersion`
  0.2.158 → 0.2.159. Coverage: `mode_parses_known_values_and_defaults_active`,
  `mode_unknown_value_falls_back_active`, `mode_is_readonly_only_at_demotion_rung`,
  `current_mode_reads_env_and_defaults_active`, `plugin_watch_readonly_default_active_returns_zero`.
  Remaining for `#q6js`/`#lvbremain`: the live operator drive (restart with
  `AGENT_DOC_PLUGIN_WATCH=read-only` + the 0.2.159 plugin installed, drive an edit-during-finalize
  cycle) so the agent can confirm zero-drift + the `visible_write_live_buffer_matches_disk` marker
  from `ops.log`.
- **Land the in-process supervisor hosting swap (`#dav9` / `pcpc5e1` authority rung).**
  `AGENT_DOC_SUPERVISOR=in-process` now actually hosts the harness child through
  `supervisor::in_process::InProcessSupervisor` instead of only logging the gate.
  `PtySession::take_child` hands the `portable-pty` child to the adapter, which
  reaps exits non-blockingly via `try_wait` (no reaper thread/channel), drives
  heartbeat + crash policy through its `tick` loop, and owns `kill`. `start.rs`'s
  `run_with_reap_policy` branches on `supervisor_hosts_in_process`: in-process
  mode hands off the child and drives the tick loop (honoring stop/restart/
  route-complete by killing once so the next tick reports the exit), while
  keeping the reader/writer/resize/auto-trigger/idle-watch plumbing and the
  Unix-socket IPC boundary; the outer restart loop still owns respawn (the
  adapter factory refuses an in-adapter respawn). **Default `off` is unchanged**
  — shipped users still block on `session.wait()` exactly as before; this is the
  dormant, flag-gated authority rung, not the live default flip. New coverage:
  `adopt_hosts_real_child_through_tick_loop`, `pty_supervised_child_reaps_real_exit_code`,
  `pty_supervised_child_kill_terminates_live_child`, `pty_supervised_child_monitor_rejects_stdin`,
  `kill_child_lets_tick_report_exit_without_halt`. Remaining: the live-gated
  default flip + out-of-process writer removal (`#q6js`), and the plugin
  WatchService demotion (`#dsqa`), both of which require live operator proof.
- **Fix UTF-8 char-boundary panic in closeout heuristics (`#heurpanic`).**
  `heuristics::has_quantified_remaining_work` computed a 30-byte look-back window
  start as a raw byte offset (`pos - 30`) and sliced `&lower[start..pos]`. When a
  response line placed a multibyte char (e.g. an em-dash `—`) so that `pos - 30`
  landed inside it, the slice panicked (`byte index … is not a char boundary`),
  crashing `finalize`/`session-check` (`rc=134`) on otherwise-valid responses and
  wedging closeout. The window start now rounds down with `floor_char_boundary`.
  Regression test `quantified_remaining_work_handles_multibyte_char_in_window`.
- **`#lvbremain` verification markers now reach `ops.log` (`#x9ds`).** Two of the
  marker emissions the `#lvbremain` ops.log scan expects were stderr-only and so
  always scanned 0. The cross-session-reject reject path (`claim.rs`) now also
  records its `[claim] cross-session-reject pane_id=.. pane_session=.. configured=..`
  marker to `ops.log` (it previously went only to stderr as the plugin-branch
  signal). The early-ack success path (`ipc_socket.rs`) now records a marker
  carrying the exact `early_ack_pending` predicate token to `ops.log` (the
  human stderr line "early-ack pending" is hyphenated and never contained the
  token); new testable `ipc_socket::early_ack_ops_marker`. The third marker,
  `run_clear_coalesce` (`#9adk`), is **not** a binary behavior — it is the
  JetBrains-plugin `InvocationCoalescer` (Run/Clear dedup) whose marker goes to
  `idea.log` and is operator-verified via the `#lvbatch` live batch, so there is
  no ops.log emission site to wire; the ops.log-scan expectation for it was a
  category error and is dropped. Tests `enforce_cross_session_claim_errors_on_reject`
  (asserts the marker reaches ops.log) and `early_ack_ops_marker_carries_predicate_token`.
- **Ops-proof auto-completion no longer reaps a same-cycle add (`#5b28` /
  `#opsproof-samecycle-add`).** A gated `agent:review` item (or `agent:backlog`
  item) added in the same `write`/`finalize` invocation via `--review-add` /
  `--pending-add` / `--pending-add-gated` could be opportunistically ops-proof
  auto-completed on the cycle it first appeared, when its text carried a
  completion marker plus a commit hash (e.g. a `#s760d` operator-verify gate that
  cites "Code shipped <commit>"). The existing `#opsproof-falsepos` guard compared
  against the on-disk snapshot, but in the finalize path the same invocation
  re-syncs that snapshot to include the new item, so the snapshot test could not
  tell a brand-new add from a pre-existing one. The add primitives now record the
  ids added this cycle in `cycle_state` (`pending_added_ids`), and the reap
  excludes them. `review_add` returns its assigned id; new
  `cycle_state::{record_pending_added_ids, pending_added_ids}`. Regression test
  `ops_proof_does_not_reap_same_cycle_added_gated_item`.
- **Transcript-token context-% pre-emptive `/clear` gate (`#s760c`).** The
  supervisor idle-queue watch now derives its pre-emptive `/clear` decision from
  the **real** harness context-usage % (cumulative session-transcript tokens ÷
  model context window, via the `#s760a`/`#s760b` `context_pct` source) instead of
  the exchange-size accretion heuristic. On each idle gap, when the default-off
  `agent_doc_queue_context_reset` opt-in is set, it locates the active Claude
  transcript (newest `*.jsonl` under `~/.claude/projects/<project-hash>/`), computes
  ctx%, emits the canonical `[s760] clear-decision optIn=.. threshold=.. pct=..
  clear=..` line to `ops.log`, and fires the tracked `/clear` only when
  `pct >= clear_threshold_for_doc` (`agent_doc_clear_threshold`, default 50). New
  pure, unit-tested `context_pct::{clear_decision, claude_projects_subdir,
  latest_claude_transcript}`. Fails safe per the destructive-`/clear` invariant: an
  unknown model, missing/empty transcript, or unsupported harness (Codex/OpenCode)
  yields `pct=None` and **never** clears; the compaction-after-clear safety case is
  preserved; everything stays behind the opt-in (default off). Operator live-verify
  of the destructive path on a throwaway opted-in doc remains `#s760d`. Plan:
  `tasks/agent-doc/plan-s760-transcript-ctx-clear.md`.
- **Opportunistic gated-review auto-verification (`#optverify`).** A gated `[/]`
  `agent:review` item can now carry a typed proof/disproof predicate so the binary
  auto-proves or auto-disproves it from `ops.log` markers during the normal
  preflight read — no dedicated live-verify session. Set it with
  `agent-doc backlog <FILE> set-verify <id> "verify=ops_log:<marker>;disproof=ops_log:<text>"`
  (or `--pending-set-verify id=<spec>` on `write`/`finalize`); the gate-set time
  is stamped automatically. The predicate persists inline as a
  `<!-- gate-verify verify=… disproof=… set_at=… -->` annotation (new pure
  `agent_doc_core::gate_verify` module). The scan is fail-safe: a proof marker at or
  after `set_at` with no disproof → `provable`; a disproof marker → `failed`
  (**disproof wins ties**); neither → `pending`; stale pre-`set_at` markers never
  count. Preflight surfaces a `gate_verify` result per predicate-bearing gated item
  and logs `optverify review=<id> status=<…>`. The `[/]→[x]` auto-transition is
  **opt-in and default off** — only when `agent_doc_gate_autoverify` is true
  (frontmatter, then project `.agent-doc/config.toml`) does a `provable` gate flip,
  staged to both the working tree and snapshot; otherwise the status is only
  surfaced and a human gate is never silently flipped. Spec:
  `specs/07-closeout-commands.md` (preflight section). Phases `#optv1`–`#optv4`.
- **Early-ack IPC activated (`#saevon`).** Flipped `EARLY_ACK_ENABLED false → true`
  in `ipc_socket.rs`: the sender now auto-tags live closeout `patch` messages with
  `early_ack: true`, so an early-ack-aware listener emits a `pending` ack the instant
  it receives the patch — before the blocking apply — decoupling the sender's liveness
  probe from plugin apply latency (root of the R2 false ack-timeout / degrade-vote
  class). The two-phase protocol was landed dormant and unit-tested in a prior cycle;
  this flip activates auto-injection. Skew-safe: older listeners ignore the unknown
  `early_ack` field and still get exactly the prior single terminal ack. Live
  verification (`#xkpf` / `#lvb-run`) greps `ops.log` for `[ipc-socket] early-ack
  pending emitted before apply` with a paired terminal ack and no `ack_timeout` /
  `false_success`. Test `early_ack_tagging_is_dormant_by_default` → renamed/inverted
  to `early_ack_tagging_is_active_for_patches`. `ipc_socket.rs`,
  `specs/process-topology.md`.

- **Consumed (struck) queue items are no longer recorded as dropped user edits
  (`#dropqueue-consumed-falsecount`).** `dropped_queue_prompt_lines_after_content_ours`
  counted only `content_ours` *active* queue prompts, so a queue item the user
  added this cycle that `content_ours` consumed (struck `~~…~~`) read as
  "dropped" — tripping the `#queue-user-edit-overwrite` guard with a false
  `session-check` INTERRUPTED on a correct closeout. The detector now counts
  consumed (`QueueEntry::Completed`) items toward coverage via
  `queue_prompt_texts_including_consumed`. This is the source-level counterpart
  to `#exch-intermix-falsedrop` (which fixed the auto-recovery gate). `write.rs`;
  regression test added.

- **`#exch-intermix` auto-recovery no longer fails closed on a false-positive
  dropped-prompt record (`#exch-intermix-falsedrop`).** `try_auto_recover_live_prompt_drift`
  bailed whenever the cycle recorded ANY `dropped_exchange_prompts` /
  `dropped_queue_prompts`. But that record compares the divergent IPC candidate
  against `content_ours`, so a queue item consumed (struck) this cycle is logged
  as "dropped" even though it survives in the adopted snapshot — stranding the
  response and forcing a manual `git checkout HEAD` / `reset --from-current` /
  `finalize --force-disk` recovery (hit live on the `#opsproof-falsepos`
  closeout: `dropped_queue_prompt_recorded count=3` blocked a snapshot that was
  the complete correct document). Recovery now bails only when a recorded
  dropped prompt is genuinely absent from the adopted snapshot; a consumed
  (struck) or preserved prompt still present no longer blocks it. The
  snapshot↔disk containment gate remains authoritative for current on-disk
  content. `write.rs`; unit + integration coverage added.

- **Ops-proof auto-completion no longer false-positives on cited dependency
  work or same-cycle adds (`#opsproof-falsepos`).** Preflight pending
  maintenance previously reaped an open backlog/review item as done whenever its
  text held a completion marker (`DONE`/`SHIPPED`/…) plus a commit hash and no
  blocker word — even when the marker only described already-landed *dependency*
  work and the item itself was still actionable. A freshly-added backlog item
  citing a prior commit could be auto-archived on the same cycle it was created.
  `classify_ops_proof_completion` now requires the marker to be an open item's
  own *leading status verb* (the status prefix before the first clause break);
  gated `[/]` items keep the marker-anywhere behavior. Pending maintenance also
  refuses to reap any item absent from the cycle-start snapshot (a brand-new
  same-cycle add). `preflight.rs`; SimWorld/unit coverage added.

- **08b document write-authority cutover gate ladder (`#pcpc5cut`, rungs
  `pcpc5a`–`pcpc5c`).** New `AGENT_DOC_WRITE_AUTHORITY` env gate routes the
  central `write::atomic_write` for editor-visible session documents up the 08b
  migration ladder (`specs/08b-single-process-control-plane.md`
  §"Document write and watch authority"): `off` (default — unchanged bare
  `atomic_write`, the rollback target), `shadow` (raw write unchanged, reports
  the would-route decision to `ops.log`), `dual-write`/`authority` (route the
  real write through the session actor's single ordered write queue,
  `write_queue::serialized_atomic_write`, serializing supervisor and
  agent-finalize writes at the root of the R6 self-race / exit-75 drift), and
  `removed` (reserved for supervisor/plugin-writer removal; routes like
  `authority` at this layer). A thread-local owner-scope re-entrancy guard
  (`write_authority::owner_scope_guard`, installed by
  `write_queue::run_serialized`) keeps the queue's inner `atomic_write` on the
  raw path so a routed write cannot deadlock the document's blocking mailbox.
  `.agent-doc/` sidecar writes are never routed. Default-off, opt-in, instant
  rollback by unsetting the env. The `removed` rung and the single-watcher /
  editor WatchService read-only demotion remain gated on live IntelliJ/Codex
  proof (review `#xkpf`).

- **VS Code reports full editor-buffer content to the live-buffer sidecar
  (`#f5d2` / review `#pcp6`).** The VS Code typing listener now calls
  `agent_doc_document_changed_digest_content` (full buffer text) instead of the
  len/hash-only `agent_doc_document_changed_digest`, matching the JetBrains
  `TypingTracker`. This completes the editor side of `#pcp6`: the binary FFI
  (`agent_doc_document_changed_digest_content`), content sidecar
  (`record_live_buffer_digest_content`), and `live_buffer_diverges_from_content`
  classifier already shipped, and the JB plugin already sent content — VS Code
  was the remaining digest-only reporter. With both editors sending content, a
  genuine unsaved editor edit (buffer ahead of disk) can be positively
  scope-classified by the reconcile guard instead of failing closed on the
  mtime heuristic. New `native.documentChangedDigestContent` binding + function.
  VS Code `0.2.25`. End-to-end live-editor verification of the classify-vs-
  fail-closed behavior remains gated on `#xkpf`.

- **Pre-emptive `/clear` decision predicate (groundwork) (`#s760` / review
  `#clear-opt-in-threshold`, phase 2).** Added the pure, unit-tested
  `ContextResetDecision.shouldClearBeforeDispatch(contextUsagePct, clearThreshold,
  optIn)` that pins the phase-2 semantics: clear before a Run Agent Doc dispatch
  only when the operator opted into `agent_doc_queue_context_reset` AND the live
  Claude Code context-usage % is known and at/above the configured
  `clear_threshold` (an unknown % or a 0 threshold never clears — fail-safe).
  This is **groundwork only and not yet wired**: the two genuinely live-gated
  pieces — reading the live context-usage % from the Claude Code pane, and
  triggering the clear in `SubmitAction`'s pre-dispatch path — plus their live
  in-IDE verification remain gated on `#xkpf`. JB plugin `0.2.158`.

- **JetBrains plugin coalesces rapid-fire Run Agent Doc / Clear invocations
  (`#9adk` / review `#console-input-accumulation`).** A per-document,
  per-action `InvocationCoalescer` guard skips a second invocation of the same
  action for the same document within a short window (default 750ms), so a
  rapidly re-fired action (auto-loop tick racing a manual click, or a double
  key-chord) can no longer stack a duplicate `/agent-doc` or `/clear` keystroke
  at the terminal layer below the route/queue dedup. `SubmitAction` keys on
  `run:<routeKey>` and `ClearSessionContextAction` on `clear:<routeKey>`, so a
  deliberate Run-then-Clear for the same doc is NOT coalesced — only same-kind
  rapid re-fires collapse. The coalesce decision is pure given `nowMillis`
  (unit-tested in `InvocationCoalescerTest`); live in-IDE behavioral
  verification of rapid-fire coalescing is gated on `#xkpf`. JB plugin
  `0.2.157`.

- **Early-ack IPC protocol landed (dormant) (`#ipc-early-ack` / `#saev`,
  Phase 2).** The socket IPC handshake now supports a two-phase ack:
  `ipc_socket::start_listener` emits a `pending` ack the instant it receives a
  patch that opted in (`"early_ack": true`) — before the blocking apply — and
  `send_message` reads acks in a loop, treating a `pending` ack as liveness-only
  (it keeps waiting for the terminal ack and never returns a false success).
  This decouples the sender's liveness probe from plugin apply latency (the R2
  false-timeout / degrade-vote race). New `AckClassification::Pending`;
  `message_requests_early_ack` / `early_ack_line` / `early_ack_tagged_message`
  helpers; the early ack is owned by the Rust transport, so no plugin/callback
  change is needed (ffi listener inherits it). Backward/skew-safe: senders that
  do not set `early_ack` get a single terminal ack exactly as before, and the
  read loop runs once. Activation is gated by `EARLY_ACK_ENABLED` (off — the
  sender does not yet tag patches) pending end-to-end verification under live
  typing load (`#xkpf`). Tests in `ipc_socket.rs`
  (`classify_ack_treats_pending_status_as_pending`,
  `send_message_handles_early_then_terminal_ack`, dormancy + flag-parse);
  spec `specs/process-topology.md` R2. Plan:
  `tasks/agent-doc/plan-ipc-transport-reliability.md`.

- **Editor plugins render a choice dialog on a cross-session claim reject
  (`#jb-claim-cross-session`, plugin half).** Building on the binary
  `cross-session-reject` marker, the JetBrains "Claim for Tmux Pane" action
  (`ClaimAction`, plugin 0.2.156) and the VS Code claim command
  (`crossSession` + `extension.ts`, 0.2.24) now parse the marker
  (`parseCrossSessionReject`) and prompt **Force Claim** (`claim --force`) /
  **Switch Project Session** (`session set <pane_session>` then re-claim) /
  **Cancel** instead of surfacing the raw exit-1. Unit coverage:
  `ClaimActionTest` (JB) and `crossSession.test.ts` (VS Code), each covering
  ordered/unordered/missing-field/no-marker parsing. Live click-through
  verification against a genuinely cross-session pane remains gated on a live
  IntelliJ/VS Code session (`#xkpf`). Plan:
  `tasks/agent-doc/plan-jb-claim-cross-session.md`.

- **Agent prompts now expose a stable prompt-cache prefix
  (`#pcache-boundary`).** Direct `run` prompts and Codex owner-pane queue
  continuations now render through a shared prompt-cache boundary helper: stable
  response contracts come before the boundary, while volatile diff, document,
  queue-head, status, and compaction/accretion context stays after it. Tests
  assert that queue heads, file paths, prompt-target sections, and accretion
  diagnostics remain below the boundary so prompt-cache fingerprints do not churn
  on ordinary session state changes.

- **`claim` emits a structured cross-session-reject marker for editor plugins
  (`#jb-claim-cross-session`).** On a cross-session **Reject** (target pane lives
  in a tmux session other than the configured project session, configured session
  alive, no `--force`), `claim` now prints a stable machine-readable line to
  stderr before the human bail: `[claim] cross-session-reject pane_id=<id>
  pane_session=<session> configured=<session>` (`CROSS_SESSION_REJECT_MARKER` +
  `cross_session_reject_marker` with stable field order). Editor plugins
  (JetBrains "Claim for Tmux Pane", VS Code claim command) can branch on this
  marker to render a Force claim / Switch project session / Cancel dialog instead
  of surfacing the raw exit-1 message. The human bail text is preserved unchanged
  for terminal use. Unit coverage in `claim.rs`
  (`cross_session_reject_marker_carries_stable_fields`); contract in
  `specs/07-session-tmux-commands.md`. The plugin choice dialog itself remains
  gated on live-IntelliJ/VS Code verification (backlog `#4wxr`).

- **A newly-canonical tmux session closes the superseded session
  (`#canonical-session-close`).** `agent-doc session set <name>` now closes the
  old session and prunes it from the model after the new session becomes
  canonical, instead of leaving registered panes spanning two tmux sessions
  (the recurring "session drift: registered panes span 2 tmux sessions" warning).
  After migrating the `agent-doc` + `stash` windows, `set` calls the new
  `resync::close_superseded_session(old)`, which closes the old session **only**
  when it is a pure agent-doc orphan: every remaining window is agent-doc-managed
  (`agent-doc`, `stash`, `stash-*`) and no pane runs a live agent process. A
  session holding any unmanaged user window or a live agent is preserved, so the
  cleanup never destroys unrelated work. New tmux helpers `list_window_names`,
  `list_session_panes`, and `kill_session` in `tmux-router`; live-tmux coverage in
  `resync.rs` (kills a pure orphan, preserves a user-window session, treats an
  already-gone session as closed); spec in `specs/07-session-tmux-commands.md`.

- **Overlay-as-merge-base is order-stable for the append case
  (`#ipc-drift-order-stable-merge`).** Verified the suspect overlay merge-base
  path (`5fd64b26`) cannot reverse a new `### Re:` response's lines or hoist it
  above the prior committed response when a foreign supervisor appends to the
  document tail mid-generation. The overlay source is used as the merge base
  only when its markdown projection is byte-identical to the cycle baseline
  (otherwise the merge falls back to the baseline text), so the derived base can
  never reorder committed exchange content. Added an end-to-end test
  (`overlay_merge_base_is_order_stable_for_exchange_append`) driving the real
  `crdt_merge_base_state` overlay path for an exchange-with-response document,
  fixed a stale client-id comment in `crdt::merge`, and documented the invariant
  in the document-format spec.
- **Concurrent-supervisor superproject write-back is serialized
  (`#ipc-drift-writeback-serialize`).** The git-dir-scoped commit lock already
  serializes the staging/commit critical section per resolved git dir; this is
  now verified for the cross-supervisor superproject case the drift report hit
  (a monsterrodholders submodule session's `submodule pointer` commit
  interleaving with another session's superproject commit). A submodule
  document's parent-gitlink update and a sibling superproject-root document's
  closeout contend on the same superproject lock, so two supervisors writing
  back to one superproject cannot interleave partial commits or strand a
  captured response outside `HEAD`. Added a deterministic concurrent test
  (`superproject_writeback_serializes_pointer_update_and_root_commit`) plus the
  closeout-spec invariant.
- **Clean CRDT merges reconcile foreign disk writes instead of stranding the
  response (`#ipc-drift-visbuf-reconcile`).** When the on-disk document diverges
  from the merge input but the live editor buffer does *not* (a foreign
  agent-doc supervisor appended mid-generation, not a pending user edit), the
  template/stream write paths now re-read the fresh disk content
  (`visible_write_disk_drift_reconcilable`), re-merge the captured response
  against it, and retry — bounded to a few attempts — rather than failing closed
  with "visible editor buffer differs" and leaving the captured response outside
  HEAD (the `stuck_captured_cycle` symptom). A genuine unsaved editor-buffer edit
  still fails closed. Only persistent drift past the attempt bound falls back to
  the old fail-closed behavior.
- **Route-owned queued reruns no longer write `agent:queue auto`.** Busy
  dispatch-only reroutes now create or update a plain `agent:queue`, strip a
  legacy `auto` attribute from any touched queue tag, and use `queue_active:
  true` / start fences for activation. Editor diagnostics accept both the new
  `active agent:queue` wording and older `agent:queue auto` output. Active
  session drift checks now ignore ordinary post-exchange scratch/comment edits
  after commit while still interrupting on real component edits. Tests:
  `route_enqueue_dispatch_prompt_creates_visible_plain_queue_and_snapshot`,
  `route_activates_existing_inactive_auto_queue_head_as_plain_queue_for_busy_deferral`,
  `route_enqueue_dispatch_prompt_supersedes_single_auto_queue_prompt`,
  `session_check_ignores_active_session_post_commit_comment_only_drift`, and
  JetBrains `TerminalUtilTest`.

- **Queue slash commands now run as commands after the current turn.** Active
  queue heads such as `/clear` or `/model sonnet` are classified in the managed
  idle-queue drain path, submitted literally to the owner pane at the next idle
  prompt, consumed from `agent:queue`, committed, and then the remaining queue
  resumes. The idle-queue watcher now also waits for the hook-owned turn-active
  marker to clear before submitting queued work, so a visually idle prompt cannot
  race ahead of the full Stop/idle boundary; queued `/clear` and context reset
  clears use the supervisor's gate-exempt clear submit path instead of generic
  prompt injection. Codex Stop-hook and `session-check --codex-final-gate`
  diagnostics now name these heads as queued slash commands instead of prompts to
  answer. Direct preflight, plan, and run synthetic active-queue-head paths now
  keep slash-only heads command-only as well: surrounding whitespace is trimmed
  for slash classification, no `preflight_started` response cycle opens, no
  prompt targets or repo actions are planned, and owner-pane self-invocation
  diagnostics tell Codex to let the supervisor submit the command instead of
  answering/finalizing it in `agent:exchange`. Tests:
  `parse_slash_commands_trims_surrounding_whitespace`,
  `preflight_does_not_open_cycle_from_active_queue_slash_command`,
  `build_plan_treats_active_queue_slash_command_as_command_handoff`,
  `active_queue_prompt_diff_ignores_slash_command_head`,
  `owned_pane_queue_handoff_diagnostic_uses_supervisor_for_slash_command`,
  `queue_command::tests::*`,
  `idle_queue_drain_waits_for_turn_status_idle_even_with_visible_prompt`,
  `idle_queue_context_reset_waits_for_turn_status_idle`,
  `auto_trigger_clear_command_bypasses_dispatch_gate_and_submits_enter`,
  `idle_queue_drain_payload_submits_literal_clear_command`,
  `idle_queue_drain_payload_submits_any_literal_slash_command`,
  `complete_idle_queue_slash_command_head_consumes_and_commits`,
  `stop_blocks_clean_closeout_when_auto_queue_has_clear_command`, and
  `stop_blocks_clean_closeout_when_auto_queue_has_generic_slash_command`.

- **Exchange slash commands now enter the same after-turn command path.**
Routed `agent:exchange` prompts whose pending text is a literal slash command
such as `/clear`, with or without a `❯` prompt prefix, are copied into
`agent:queue auto` as an unpinned literal command head, then the managed
idle-queue supervisor submits and consumes them after the current turn instead
of reopening agent-doc and answering them as prose. Tests:
`classify_prompt_bearing_changes_promotes_bare_slash_command_to_prompt_target`,
`route_enqueue_bare_exchange_slash_command_for_idle_drain`, and
`route_enqueue_exchange_slash_command_keeps_literal_head_for_idle_drain`.

- **Actor pane binding now recovers cross-document aliases.** Route/start actor
store writes atomically close and clear the displaced document's pane binding
before storing the incoming owner. This prevents editor navigation to files such
as `lazily-rs.md` from leaving that document pointed at the previous
`agent-doc-bugs2.md` pane after recovery. `session status` no longer treats a
closed actor's stale pane as live evidence, and `session doctor --repair` clears
old closed actor pane projections. Tests
`binding_a_pane_evicts_other_documents_bound_to_it`,
`sessions_projection_removes_displaced_cross_document_owner`, and
`closed_actor`.

- **Free-text queue consumption now requires exchange history.** Session-check
  no longer treats a sidecar response hash or binary consume marker as sufficient
  proof that a preflight free-text queue head was answered. If the head is gone
  from `agent:queue`, the committed `agent:exchange` must contain the response or
  queue-prompt echo, otherwise `#lr-queue-patchback-miss` fails closed. Tests:
  `free_text_queue_head_guard_fires_when_binary_consume_lacks_response` and
  `free_text_queue_head_guard_passes_with_committed_response_echo`.

- **Codex idle queue drain no longer reinvokes the owning pane.** The
  supervisor idle-queue watch now drains Codex `agent:queue auto` heads by
  injecting an in-owner-pane continuation instruction instead of the recursive
  `agent-doc <file>` trigger. Claude and OpenCode keep their configured trigger
  command. This keeps JetBrains `Run Agent Doc` prompts queued behind a busy
  Codex owner from stalling on the recursive-direct-invocation guard once the
  owner goes idle.

- **`agent-doc focus` is the fast pane handoff by default.** The default focus
  path now has the former editor fast-focus behavior: it selects an already
  visible pane immediately, defers stash surfacing to `sync --no-autostart`, and
  does not perform additive promotion work in the foreground. The previous
  synchronous promote-and-select behavior is still available for manual use as
  `agent-doc focus <file> --blocking` or `--synchronous`; the older
  `--no-stash-promote` flag is retained as a hidden compatibility no-op. Current
  JetBrains and VS Code tab selection now call plain `agent-doc focus <file>`
  with a short editor-side timeout and leave slow/missing-pane work to the
  debounced reconciler. This keeps navigation to documents such as
  `lazily-rs.md` from letting a long-running CLI focus attempt delay the UI
  handoff. Bumped the JetBrains plugin build version to `0.2.153` and the VS
  Code extension version to `0.2.23`.

- **Structured overlay CRDT is the merge-base authority
  (`#md-ast-crdt-merge-base`).** Template/CRDT merge paths now derive their
  text CRDT base from the structured `.overlay.yrs` sidecar when its markdown
  projection matches the active cycle baseline. If the overlay sidecar is
  absent, corrupt, or stale relative to the explicit baseline, the merge falls
  back to the baseline text and logs the fallback reason. Tests
  `crdt_merge_base_state_prefers_matching_overlay_projection` and
  `crdt_merge_base_state_falls_back_when_overlay_projection_is_stale`.

- **Safe-passive sync pre-locks the pane handoff.** `sync --no-autostart
  --focus <file>` now selects a live local actor projection before waiting on
  `.agent-doc/sync.lock`, so a contended reconcile no longer gates the visible
  pane switch. When no local actor record exists, sync tries skip-wait pane
  provisioning through nonblocking startup locks and defers stale/dead/blocked
  records to the existing locked guard path. Tests
  `safe_passive_sync_focuses_local_projection_when_sync_lock_is_contended` and
  `try_startup_lock_reports_busy_without_waiting`.

- **Session-isolated self install.** `agent-doc self-install` now installs the
current committed checkout from a temporary sibling git worktree, preserving
relative Cargo path dependencies like `../agent-kit` while avoiding dirty files
from concurrent sessions in the shared `src/agent-doc` checkout. It runs
`cargo install --path .`, builds the release cdylib, installs it with the
existing atomic `lib-install` path, and removes the worktree unless
`--keep-worktree` is set. Test
`isolated_worktree_uses_committed_head_not_dirty_checkout`.

- **JetBrains Run Agent Doc recognizes active-turn skip-wait refusals.** The
JetBrains route-failure classifier now maps the route core's
`pane is busy on an active ... turn` dispatch-only refusal to the immediate
still-running notification instead of the persistent route-failure path, so the
`#jb-run-agent-doc-busy-active-turn-stall` skip-wait fix is visible to IDE
users. Test `active-turn skip-wait route refusal is reported as still running
immediately`.

- **JetBrains File Cache Conflict keeps visual highlighting fresh.** The
JetBrains plugin now reschedules `VisualHighlighterManager` on the File Cache
Conflict pending, Cancel, accepted/reload, and deferred-patch-applied paths so
agent-doc markdown visual tokens are reapplied after the IDE resolves the
dialog. Test
`file cache conflict path refreshes visual highlighters`.

- **Preflight/plan propose semantic completion matches.** The shared
`tsift-memory` session-memory path now exposes advisory semantic completion
candidates for open backlog/review items and free-text queue prompts that are
highly similar to done-state memory events. `preflight` emits
`semantic_completion_match` warnings and `plan` includes the same warning text;
the signal is proposal-only and does not mark work done. Tests
`semantic_completion_matches_done_archive_for_free_text_queue_prompt` and
`build_plan_warns_on_semantic_completion_match_for_free_text_queue`.

- **Preflight auto-completes deterministic ops-proof tracked work.** During
pending maintenance, active `agent:backlog` and `agent:review` items with
explicit completion markers plus commit or successful-CI proof are promoted to
done, removed from the active surface, archived, recorded in cycle state, and
logged as `auto_complete_ops_proof`. Blocker language such as partial,
remaining, reopened, deferred, false-closeout, or follow-up work keeps items
active. Test `pending_maintenance_auto_reaps_ops_proof_done_items`.

- **Preflight reaps stale active mirrors for archived done ids.** During pending
maintenance, active `agent:backlog` and `agent:review` items whose ids already
exist in inline `agent:done` or the configured external `agent:done archive=...`
are removed from the live tracked-work surface without appending duplicate done
archive entries. Queue maintenance continues to exclude those ids from
backlog-to-queue sync and now has explicit external-archive strike coverage.
Tests
`pending_maintenance_reaps_inline_done_backlog_and_review_mirrors`,
`pending_maintenance_reaps_external_done_archive_backlog_and_review_mirrors`,
and `run_queue_maintenance_excludes_external_archive_done_ids`.

- **Codex installs now register the agent-doc MCP server.** `agent-doc skill
install --harness codex` writes `[mcp_servers.agent-doc]` into
`.codex/config.toml` and the Codex Stop hook prefers the
`agent_doc_preflight` / `agent_doc_plan` / `agent_doc_finalize` /
  `agent_doc_session_check` continuation path when that server is configured,
  while preserving the existing in-pane CLI fallback for runs without MCP.

- **Session clear ignores its own drafted control command.** Codex protected
  prompt detection now treats `agent-doc session clear`, `interrupt-clear`,
  `stop`, and restart control lines as operator commands instead of unsaved
  drafted prompt text, so the clear command no longer blocks itself. Ordinary
  drafted prompt text remains protected. Tests
  `protected_prompt_input_reason_ignores_agent_doc_session_control_commands`
  and `protected_prompt_input_reason_keeps_agent_doc_non_control_text_protected`.

- **Runtime snapshots persist the structured markdown-overlay CRDT
  (`#md-ast-document-model`).** Template/CRDT write, stream, IPC fallback,
  socket-ack repair, compact, commit-refresh, reset/rebuild, and closeout
  recovery paths now save a structured `.agent-doc/crdt/<hash>.overlay.yrs`
  sidecar from the same markdown snapshot as the legacy text `.yrs` merge
  state. The overlay projection is now the preferred merge base when it matches
  the active cycle baseline, while the legacy state remains for older binaries
  and editor plugins. Tests
  `document_crdt_save_persists_legacy_and_overlay_state` and
  `ensure_initialized_migrates_after_move_with_existing_session`.

- **Preflight queue dedup is node-keyed, not text-keyed
  (`#md-ast-document-model`).** The active queue cleanup pass now delegates
  duplicate cleanup to the markdown-AST mutation layer and only removes duplicate
  durable queue node keys. Repeated `do [#id]` or free-text prompts remain
  executable queue intent instead of being collapsed by raw prompt text. Test
  `preflight_preserves_intentional_duplicate_tracked_queue_prompt`.

- **Icebox queue sync no longer auto-promotes parked work.** `agent:icebox`
  may still be sorted by priority and individual icebox items can still opt into
  queueing with per-item enqueue markers, but a component-level `queue`
  attribute on `agent:icebox` now warns and does not populate `agent:queue`.
  A drained queue plus drained backlog remains terminal until work is explicitly
  moved to backlog, queued manually, or marked for enqueue. Tests
  `run_queue_maintenance_does_not_sync_icebox_into_empty_queue` and
  `misplaced_component_attr_warning_flags_queue_sync_attr_on_icebox`.

- **No-response active-head guard checks only the current queue head.** A stale
  no-response/reap-only cycle no longer blocks session closeout just because
  later `do [#id]` queue items still exist in backlog while a free-text prompt
  sits ahead of them. The guard now compares recorded ids only with the first
  live queue prompt. Test
  `no_response_active_queue_head_passes_when_later_do_item_is_not_current_head`.

- **Pending shadow guard ignores exchange transcripts.** The shadow-backlog
  detector no longer treats checklist or ordered-list `[#id]` lines inside
  `agent:exchange` response history as live pending shadows. Completed items
  archived elsewhere, such as lazily-rs `#ipc1`, no longer block
  `session-check` just because an earlier response listed next steps.

- **Missing-response recovery trusts committed exchange bodies without capture
  metadata.** `session-check` now treats any committed `agent:exchange`
  `### Re:` body as sufficient proof that a missing-response closeout has been
  repaired, even when the stale queue-drain cycle lost `capture_id` /
  `response_sha256` metadata. This lets `agent-doc write --commit` recovery
  clear interrupted sessions like `tasks/software/lazily-rs.md` instead of
  re-failing forever. Test
  `committed_without_response_body_guard_passes_recovered_exchange_body_without_capture_metadata`.

- **Idle queue stale-busy recovery preserves same-head dedup.** The supervisor
  idle-queue watcher no longer clears `last_dispatched` while reconciling a
  stale busy actor over an idle pane. This stops a stuck active head from
  repeatedly injecting `agent-doc <FILE>` after each reconcile tick, while still
  allowing dispatch when the head drains or advances. Test
  `stale_busy_reconcile_preserves_already_dispatched_head_dedup`; spec
  `specs/supervisor.md`.

- **Completed queue items can be marked in place.** Explicit done IDs now mark
  matching `agent:queue` prompts completed even when the matching queue item is
  not part of the active contiguous head-consumption range. This preserves the
  current head while striking opportunistically completed queued work in both the
  document and snapshot. Tests
  `done_id_marks_later_queue_prompt_completed_without_consuming_head` and
  `done_id_marking_ignores_already_completed_queue_prompt`; spec
  `specs/07-orchestration-commands.md`.

- **Queue overwrite guard tracks free-text queue items.** The
  `#queue-user-edit-overwrite` detector now compares parsed `agent:queue`
  prompt entries by count instead of relying on prompt-target diff
  classification, so user-added free-text queue bullets adjacent to the current
  head are recorded before a `content_ours` IPC adoption can silently delete
  them. Tests
  `dropped_queue_prompt_lines_after_content_ours_captures_adjacent_free_text_items`,
  `..._empty_when_items_are_owned`, and `..._counts_duplicate_user_items`.

- **Backlog queue priority visibly annotates promotions.** A backlog
  component carrying both `queue` and `priority` now triggers queue priority /
  auto-DAG ordering for the synced `agent:queue` even when the queue marker
  itself has no `priority` token. Automatically promoted queue prompts are
  annotated with `:round_pushpin:`, while priority route dispatches are inserted
  with the operator `:pushpin:` marker and dedupe against bare equivalents.
  Tests `run_queue_maintenance_backlog_queue_priority_sorts_and_marks_promoted_item`
  and `route_enqueue_priority_dispatch_*`; specs `specs/pending-system.md`,
  `specs/07-orchestration-commands.md`.

- **Session memory retrieval (`#agent-doc-memgraphrag-retrieval`).**
  `agent-doc memory index/search` now indexes current session tracked work
  (`agent:backlog`, `agent:review`, `agent:icebox`, `agent:done` including
  repo-relative `.done.md` archives) plus live exchange response summaries into
  `.tsift/memory.db` through the shared `tsift-memory` library crate. Search
  combines persisted memory with the current document and ranks locally for
  already-tracked / already-fixed dedupe checks without embedding tsift's heavy
  codebase index in the per-cycle hot path. Tests `memory_cmd::*`; specs
  `SPEC.md`, `specs/07-core-commands.md`.

- **Per-item enqueue markers populate `agent:queue` (`#queue-enqueue-action`).**
  Open backlog/icebox/pending items containing `:inbox_tray:`, `/enqueue`, or a
  Markdown-decorated `enqueue` token such as `**enqueue**` now append `do [#id]`
  to `agent:queue` without requiring the whole component to carry a `queue`
  attribute. The path is idempotent, excludes gated/done/unmarked items, works in
  both preflight and `agent-doc queue sync`, and lets explicit markers bypass the
  plain active-loop fresh-item hold. Tests
  `active_enqueue_item_ids_returns_marked_open_items`,
  `run_queue_maintenance_enqueue_marker_populates_queue_without_backlog_attr`,
  `collect_backlog_queue_sync_reads_enqueue_markers_without_attr`, and
  `sync_accepts_enqueue_marker_without_queue_attr`. Spec
  `specs/07-orchestration-commands.md`.

- **Watch daemon emits node-keyed document events (`#md-ast-realtime-watcher`).**
  The markdown AST crate now exposes `events::diff_node_events`, producing
  insert, remove, replace, move, strike, and unstrike events keyed by the same
  semantic node ids used by mutations and IPC patches. The watch daemon seeds a
  per-file node snapshot for watched session documents and logs `document_node_events`
  JSON batches on subsequent file changes, giving realtime enqueue/follow-up work
  a stable node-keyed event stream. Tests
  `diff_node_events_reports_insert_with_anchors`,
  `diff_node_events_reports_strike_by_stable_node_key`,
  `diff_node_events_reports_reorder_without_text_matching`, and
  `update_node_snapshot_emits_node_keyed_events_after_seed`.

- **Markdown-AST IPC patches are node-addressed (`#md-ast-ipc-node-patches`).**
  Queue closeout now derives semantic occurrence node keys for live queue items
  and strikes non-draining queue heads through the AST mutation layer, so
  intentional duplicate prompt text is not consumed by text matching. IPC
  payloads retain legacy component `patches` while adding explicit `op` /
  `node_id` metadata and a `node_patches` array for item-level insert, strike,
  unstrike, replace, remove, and move operations. JetBrains and VS Code patch
  DTOs accept the new fields. Tests
  `queue_consume_uses_node_keys_to_preserve_duplicate_prompt_identity`,
  `build_ipc_node_patches_json_tracks_strike_and_insert_by_node_key`,
  `build_ipc_node_patches_json_tracks_reorder_without_text_matching`,
  `build_ipc_patches_json_seeded_boundary_is_stable_across_rebuilds`, and
  `parsePatchJson preserves node-addressed component patch fields`. Specs
  `specs/02-document-format.md`, `specs/07-orchestration-commands.md`.

- **Deprecated `queue_active:` frontmatter line no longer gets stuck in a
  document (`#queue-active-deprecated-line-stuck`).** `merge_queue_state`/`write`
  already drop the legacy `queue_active:` line when they re-serialize, but a doc
  whose hot path preserves frontmatter byte-precisely never re-serializes it, and
  the diff layer classifies any `queue_active:` line as managed state — so its
  removal reads as a no-op and is never committed. The legacy line stayed in the
  file forever even after the canonical `queue:` control took over (operators saw
  a persistent `queue_active: true` that "couldn't be removed"). Preflight repair
  now drops it once, byte-precisely, directly on disk + snapshot via
  `strip_deprecated_queue_active_line` — but ONLY when the canonical `queue:`
  control is present, so no queue state is lost. Idempotent; legacy-only docs
  (no `queue:`) keep their line. Tests
  `strip_deprecated_queue_active_line_drops_legacy_when_canonical_present` +
  `..._keeps_legacy_without_canonical`.

- **Session-check self-heals a late-IPC committed-response over-application
  (`#late-ipc-patch-response-uncommitted`).** When a wedged/slow IPC listener
  applies a stale queued patch after the cycle already committed, it re-adds a
  duplicate `### Re:` block to the working tree even though the real response is
  in HEAD — and session-check previously reported an unrecoverable interruption
  that stalled the `agent:queue` auto-loop. The mutating session-check
  entrypoints (`enforce_clean_closeout` on the `finalize` boundary,
  `run_with_options` for direct-exec `agent-doc session-check`) now restore the
  committed HEAD in place via `self_heal_late_ipc_overapplication` (logs
  `late_ipc_response_overapplication_self_healed`) instead of bailing — the same
  remediation `preflight` applies, taken only when `detect_late_ipc_response_overapplication`
  proves it safe. The read-only `inspect*` family stays non-mutating. Test
  `enforce_clean_closeout_self_heals_late_ipc_overapplication`. Spec
  `specs/07-closeout-commands.md`.

- **Queued IPC fallback patches carry a generation token for late-apply fencing
  (`#late-ipc-patch-duplicate-stall`).** A boundary-reposition IPC that times out
  queues a durable fallback patch file; a wedged/slow applier could apply it
  minutes late — after the cycle already committed — re-materializing a duplicate
  `### Re:` block and stalling the auto-queue. The write side already fences a
  fresh send for an already-committed cycle (`try_ipc`) and reposition-only
  patches already carry no response body, so `queue_file_ipc_reposition_boundary`
  now also tags the queued `.agent-doc/patches/<hash>.json` with `cycle_id` and a
  `baseline_hash` of the live doc it targets, giving the asynchronous applier the
  same generation token to drop a superseded patch. Test
  `queued_file_reposition_patch_carries_generation_token`. The plugin-side
  consumption of the token (`PatchWatcher` apply fence) and listener de-wedge are
  tracked follow-ups; spec `specs/07-closeout-commands.md`, plan
  `tasks/agent-doc/plan-late-ipc-patch-duplicate-stalls-queue.md`.

- **Free-text queue head consumed despite a prompt-prefix flip on an answered
  prompt (`#free-text-head-consume-genuine-not-struck`).** The answered-free-text-head
  decision (`cycle_answered_foreign_exchange_prompt`) diffs the *normalized
  snapshot* baseline against the *live* editor buffer, and the buffer preserves
  `❯` prompt prefixes on already-answered prompts that the snapshot normalized to
  the bare form. A pure `do x` → `❯ do x` prefix flip surfaced as an added
  `+❯ …` diff line and was wrongly read as a *new foreign* exchange prompt,
  blocking the free-text head strike and treadmilling the auto-loop (live repro:
  the axocoatl evaluation head answered by a committed `--stream` finalize yet
  never struck). Fix: a `❯` added line counts as foreign only when its
  normalized text is absent from the baseline entirely; a prefix flip on a
  prompt that already existed (in `❯ X` or bare `X` form) is no longer foreign.
  Genuinely new unrelated `❯` prompts still keep the head queued. Added
  `AGENT_DOC_DEBUG_QUEUE_CONSUME` env-gated instrumentation that logs each `❯`
  added line and its classification. Regression test
  `free_text_head_struck_despite_prompt_prefix_flip_on_answered_prompt`. Spec:
  `specs/07-closeout-commands.md`.

- **Retain-don't-reread runbook nudge in the shared SKILL source.** The
  `## Runbooks` section now instructs agents to read each runbook at most once
  per session and reuse the in-context copy instead of re-reading, re-opening
  only after a content change or compaction. Targets measured redundant runbook
  reads (heaviest on per-cycle shell harnesses such as Codex re-`cat`ing
  closeout runbooks every cycle); renders to all harness surfaces via
  `skill.rs`. Contract-safe (no per-harness divergence). Re-measure Codex's
  redundant-read rate before considering harness-level read-once enforcement.

- **No-change short-circuit detects committed no-response repair cycles
  (`#jb-codex-nochange-after-repair`).** When `agent-doc run` finds no
  document/snapshot diff but the latest cycle was a `Committed` no-response
  bookkeeping-only closeout (repair/reap following an abandoned recursive
  invocation or failed run), the classifier now returns an `Abnormal` verdict
  with a typed diagnostic naming the cycle id, last event, and recovery command,
  instead of plain "Nothing changed since last run". This prevents JB `Run Agent
  Doc` from showing a misleading "No changes were detected since the last run"
  after a repair cycle that followed a Codex recursive self-invocation
  abandonment. A committed cycle with a response body remains `Clean` regardless
  of bookkeeping. Spec: `specs/07-closeout-commands.md` (#jb-codex-nochange-after-repair).

- **Dedicated `blocked_in_interactive_substate` route guard reason (`#snrun`).**
  When a dispatch-only `Run Agent Doc` reopen is refused because the live pane is
  stuck in an interactive shell substate (`reverse-i-search` / history search)
  rather than a dispatch-ready composer, the fail-closed path now emits the
  dedicated `RoutedReopenGuardReason::BlockedInInteractiveSubstate`
  (`prompt_ready_barrier` FlowEvent) and a stage-specific error ("blocked in an
  interactive terminal substate") instead of the generic
  `dispatch_only_busy_actor_not_ready`. Pure helpers
  `routed_reopen::is_interactive_shell_substate_reason` /
  `dispatch_only_blocked_guard_reason`; regression
  `interactive_substate_gets_dedicated_guard_reason`. FlowCore hot-path token
  budget bumped (route.rs `guard_` 7→12, audited). The interactive-substate
  detection + multi-source dispatch-start proof were already implemented; this
  closes the remaining deterministic diagnostic refinement.
  Plan: `tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md`.

- **JB plugin logs the exact `setText` payload at debug level
  (`#jb-settext-payload-log`, plugin 0.2.145).** `PatchWatcher.kt` now logs the
  full `result` payload before `document.setText` on both the
  `applyPatch.component` and `repositionBoundary` paths, behind
  `LOG.isDebugEnabled` (only when `#com.github.btakita.agentdoc` debug logging is
  on). Previously the category logged only content hashes + lengths
  (`documentMutationDiagnosticUtil`), which was insufficient to capture the exact
  corrupting payload for the IPC-duplication family (`#ipcfullprompt-recur2`,
  `#wy0y`/`#6cmx`). Unblocks the Path B capture session.

- **BREAKING CHANGE: opt-in agent-doc documents (`#4a6p`).** A plain `.md` is no
  longer auto-converted into an agent-doc session. `route`, `run`, and `start`
  now fail closed before injecting `agent_doc_session:` frontmatter unless the
  document opts in via (1) any agent-doc-managed frontmatter field
  (`Frontmatter::has_agent_doc_marker()` — `agent_doc_*`, `session`, `agent`,
  `resume`, model overrides, `*_args`, `branch`, `queue_active`,
  `prompt_presets`, …), (2) a `[documents] include = [...]` glob in
  `.agent-doc/config.toml`, or (3) the `documents.auto_session_for_all_md = true`
  escape hatch (restores old behavior). Existing session docs already carry
  `agent_doc_session:`/`agent_doc_format:` so they are unaffected; only brand-new
  plain notes/README `.md` change behavior. The gate does **not** mutate the file
  when it refuses, and a malformed frontmatter block bypasses the gate so its own
  contextual YAML error surfaces. New pure predicate
  `project_config::is_agent_doc_document(rel_path, content, config)` + minimal
  zero-dep glob matcher `project_config::glob_match` (`*`/`?`/`**`); FFI
  `agent_doc_is_session_document(path)` for editor plugins to gate Run Agent Doc /
  SubmitAction. `agent-doc init`/`claim` remain explicit per-file opt-ins.
  Regressions: `documents_gate_tests::*` (core truth table + glob),
  `frontmatter_io::tests::gate_*` (fail-closed, file untouched, frontmatter +
  config-glob opt-in). Plan: `tasks/agent-doc/plan-opt-in-agent-doc-documents.md`.
  Follow-up (editor-side): wire the FFI gate into JB `SubmitAction.update` + the
  VS Code extension (needs a plugin version bump + live-verify).

- **Queue no longer re-mints completed `do [#id]` refs (`#ynra`).** The preflight
  backlog→queue sync now excludes ids already archived in `agent:done` before
  minting `do [#id]` prompts. Previously a lingering active backlog `[ ]` bullet
  whose id was also in `agent:done` was minted into the queue, struck by the
  done-strike pass that same cycle, then re-minted the next cycle — churning
  forever on a completed ref. `agent:done` ids are computed once up front and
  reused by both the sync filter and the strike pass. The done-strike pass also
  now reaps a resolved `do [#id]` **anywhere** in the queue (not only the head),
  so an already-orphaned completed ref behind a still-live head no longer lingers
  and trips the shadow-backlog guard. Regressions:
  `run_queue_maintenance_excludes_done_ids_from_backlog_sync`,
  `strike_done_queue_prompts_strikes_non_head_resolved_ref`.

- **Mutation-time identity-collision rejection (`#preset-item-id-collision-enforce`,
  part 1).** `agent-doc write --pending-add` / `--pending-add-after` /
  `--pending-add-before` / `--pending-add-back` / `--pending-add-to` now fail
  closed when given an **explicit** custom id (`id=<id>` or `[#id]`) that collides
  with a frontmatter `prompt_presets` key or an existing active
  backlog/review/icebox item id, so a new ambiguous `#id` is never written. Auto-id
  adds (no explicit prefix) are never blocked. Builds on the existing
  `detect_identity_collisions` registry (now factored through
  `document_active_identities` + `identity_collision_for_new_id`). The riskier
  dispatch-time halves (hard preflight/session-check block on a pre-existing
  collision; queue-generation refusal) remain intentionally deferred — the
  existing `preset_item_id_collision` preflight *warning* stays the dispatch-time
  signal — to avoid over-blocking live sessions with pre-existing collisions.
  Tests: `add_rejects_explicit_id_colliding_with_prompt_preset`,
  `add_rejects_explicit_id_colliding_with_active_item`,
  `add_allows_explicit_noncolliding_id`,
  `add_allows_auto_id_even_when_text_mentions_preset`,
  `identity_collision_for_new_id_reports_existing_sources`.

- **Empty pending/icebox bullets no longer get a phantom id
  (`#icebox-empty-item-phantom-id`).** A stray content-less bullet (`- [ ]`
  with no description and no continuation — e.g. an editor/IPC insertion before
  a component close marker) was being assigned a backfilled `[#hash]` id,
  producing a phantom tracked item whose "description disappeared" (observed:
  `- [ ] [#1k5y]` in an icebox). `pending::backfill` now **drops** content-less
  items instead of manufacturing an id for them, which both prevents the phantom
  and self-heals an already-cemented id-only empty item on the next maintenance
  pass. Items with empty header text but a real indented continuation are
  preserved. Regressions: `backfill_drops_content_less_empty_bullet`,
  `backfill_drops_id_only_empty_item_self_heal`,
  `backfill_keeps_empty_text_with_continuation`.

- **Managed capability-proof failure is recoverable (`#codex-capability-proof-unrecoverable`).**
  Two fixes so a transient network blip no longer permanently wedges a managed
  Codex/OpenCode session:
  - **Bounded re-prove.** The capability-proof thread now retries the
    network/SSH/writable-root probe with exponential back-off before committing
    the dispatch gate to `Failed`. Between attempts the gate stays `Pending`
    (gated but recoverable) and the session log records
    `..._capability_proof status=retry attempt=<n>/<max>`. Retry budget, base
    back-off, and probe timeout are configurable in frontmatter and
    `.agent-doc/config.toml` (`managed_proof_max_attempts`,
    `managed_proof_retry_backoff_secs`, `managed_proof_probe_timeout_secs`;
    defaults 3 / 2s / 45s, frontmatter wins). Pure helper `proof_retry_decision`
    has deterministic unit coverage.
  - **Gate-exempt operator recovery.** The supervisor IPC layer now gates only
    real prompt dispatch (`Inject`); the new `Clear` control method plus `Stop`
    and `Restart` bypass the capability gate. `agent-doc session clear` /
    `session interrupt-clear` deliver `/clear` through the gate-exempt `Clear`
    method, so they stop or clear a proof-`Failed` session without `kill -9`
    instead of failing with `prompt dispatch is disabled`. Auto-trigger /
    auto-queue dispatch stays gated.
  - Regressions: `proof_retry_decision_*`, `resolve_managed_proof_policy_*`,
    `ipc_method_gate_classification_only_gates_inject`,
    `handle_ipc_clear_bypasses_failed_capability_proof`,
    `handle_ipc_stop_bypasses_failed_capability_proof`.

- **Queue-audit partial-completion advisory (`#queue-audit-partial-completion`,
  WARN-only).** A queue-completion audit that reports the queue as "none
  complete" while citing several completed substeps — collapsing partial progress
  into all-or-none — now trips `check_queue_audit_partial_completion_guard`, which
  recommends classifying each row as complete / partially complete / not-started
  with the completed substeps and exact remaining condition. Conservative,
  WARN-only (never blocks closeout), suppressed by `<!-- no-queue-audit-guard -->`.
  Per the binary-vs-skill rule, per-row classification is response-contract
  guidance (skill/spec); the binary only flags the unambiguous collapse.
  Regressions: `queue_audit_guard_warns_when_none_complete_collapses_partial_progress`,
  `queue_audit_guard_quiet_when_partial_states_already_given`,
  `queue_audit_guard_quiet_when_not_about_queue`,
  `queue_audit_guard_quiet_without_extra_completion_evidence`,
  `queue_audit_guard_suppressed_by_marker`.
- **Auto-queue no longer strands live items when a new head is inserted
  (`#completed-queue-residue-regression` / `#queue-auto-no-continue`).**
  `detect_head_prompt_modified` compared only the first queue prompt's text, so
  inserting (or reordering) a new item ahead of the still-present in-flight head
  registered as an in-place `item_modified` edit. Preflight then halted the
  queue, stripped `auto`, set `queue_active: false`, and stranded every remaining
  live `do [#id]` as inactive residue — the auto-queue stopped instead of
  advancing. Now a head change only counts as a modification when the snapshot
  head prompt is genuinely gone from the current queue (edited in place or
  removed); a prepend/reorder of a still-present head is treated as a
  re-prioritization and the queue advances to the new head, staying active.
  Regressions: `head_prompt_modified_false_when_new_item_inserted_ahead_of_present_head`,
  `head_prompt_modified_false_on_reorder_promoting_existing_item`,
  `head_prompt_modified_true_when_head_text_edited_in_place`,
  `head_prompt_modified_false_when_head_unchanged`.
- **Gated-phase split advisory (`#gated-followup-split-enforcement`, WARN-only).**
  When a directed `do [#id]` cycle keeps a tracked item open (`--pending-edit` /
  `--review-edit` / `--pending-gate`) whose body enumerates multiple
  gated/remaining phases (the word "phase" + ≥2 parenthesized phase markers like
  `(2b)`/`(3)` framed by a gating signal) without breaking them into discrete
  child backlog IDs, `session-check`'s new `check_gated_phase_split_guard` warns
  to split each phase into its own child ID so deferred work stays independently
  trackable/queueable (sibling of `#blocked-closeout-followup-capture` and the
  SKILL "one backlog ID per actionable phase" rule). WARN-only — never blocks
  closeout — and suppressed by a `<!-- no-gated-phase-split-guard -->` marker.
  Regressions: `gated_phase_split_guard_warns_on_multi_phase_kept_open_item`,
  `gated_phase_split_guard_quiet_when_phases_already_split_into_child_ids`,
  `gated_phase_split_guard_quiet_for_single_phase_item`,
  `gated_phase_split_guard_suppressed_by_marker`,
  `gated_phase_split_guard_is_advisory_not_blocking`.
- **`preflight --probe` is a side-effect-free inspection mode
  (`#preflight-probe-side-effect-free`).** A diagnostic `agent-doc preflight`
  used to open a `preflight_started` cycle even when it was only inspecting
  state, leaving an open cycle that later wedged `session-check` (the empty-cycle
  churn from the recursive owner-pane diagnostic path, Proposed-Fix #4 of
  `#recursive-repair-state-drift`). `agent-doc preflight --probe <FILE>` now emits
  the same JSON but never opens a `preflight_started` cycle. The default
  response-bound preflight (and internal callers like `orchestrate`) keep opening
  the cycle that binds the upcoming response. Regressions:
  `preflight_probe_does_not_open_cycle_even_with_dispatchable_diff` (probe leaves
  no open cycle) and the existing
  `preflight_opens_cycle_from_active_queue_when_document_has_no_diff` (contrast:
  default path still opens it).
- **Supervisor idle-queue watch drains route-enqueued busy-queue heads
  (`#jb-run-agent-doc-busy-queue-dispatch-deadlock`).** When a busy-pane
  `Run Agent Doc` route appends a prompt to `agent:queue auto` and returns `Ok`,
  the drain was harness-delegated and a Claude session not running `/loop` had no
  guaranteed trigger, so the queued head could sit forever (operator-perceived
  "deadlock"). The supervisor now runs a long-lived idle-queue watch alongside
  the one-shot restart auto-trigger: on each busy→idle transition it drains a
  live `queue_active: true` ready head (shared
  `queue_continuation::live_continuation_head`) by injecting the harness trigger
  through the existing capability-gated `auto_trigger_inject_command` path. The
  drain decision is the pure, tested `idle_queue_drain_decision` — dispatch only
  when idle with a fresh head, never inject mid-turn (no-inject-into-active-turn),
  dedup a still-present head to avoid hot-looping, and clear the dedup once the
  head drains. Regressions: `idle_queue_drain_dispatches_when_idle_with_fresh_active_head`,
  `idle_queue_drain_skips_when_pane_busy_even_with_active_head`,
  `idle_queue_drain_skips_when_no_active_head`,
  `idle_queue_drain_dedups_already_dispatched_head`,
  `idle_queue_drain_fires_again_when_head_advances`. Live end-to-end verification
  on a real busy Codex/Claude pane stays operator-gated.
- **JetBrains Run Agent Doc retries busy actor wait timeouts.** When
  `agent-doc route --dispatch-only --wait-for-ready` waits behind an
  authoritative actor that is busy because another operator command is still
  draining, the CLI can fail with "dispatch-only route will not inject ... the
  authoritative actor is busy did not return to a dispatch-ready prompt". The
  JetBrains action now classifies that timeout as a retryable still-running
  route outcome instead of a persistent failure, so the existing retry loop can
  catch the actor when it becomes ready shortly after the first 60s wait.
  Regression: `dispatch-only busy actor wait timeout is retryable not
  persistent failure`. JetBrains plugin bumped to `0.2.142` and installed into
  the local IDEA 2026.1 profiles.
- **Binary-owned auto-queue continuation final gate (`#codex-auto-queue-stalled-final-gate`).**
  Codex auto-queue continuation previously depended on the `codex-stop` hook finding
  tracked in-memory session state, which the live failure (`monsterrodholders.md`
  `#seocat`→`#seopdp`) showed is too fragile — a clean document still owed a
  continuation but Codex sent a final answer. New shared `queue_continuation::detect`
  is the single source of truth (`queue_active` + `agent:queue auto` + active
  `resolve_activation` + a ready, unmodified head); `active_auto_queue_prompt` now
  delegates to it. Every clean binary closeout (`finalize` / `write --commit` /
  `repair` / already-committed no-op) reconciles a durable
  `.agent-doc/queue-continuations/<doc-hash>.json` marker (written when owed, cleared
  on drain / `auto` removal / `queue_active` false / head advance). `codex-stop` now
  consults that marker when no tracked session state exists — re-confirmed against the
  live document so a stale marker never forces a spurious block — and still blocks the
  final answer, failing closed on a repeated non-advancing head. `agent-doc session-check`
  surfaces `queue_continuation_required=…` / `next_queue_prompt=…`; the new strict
  `agent-doc session-check <FILE> --codex-final-gate` exits nonzero when continuation is
  required. New `queue_continuation` unit tests, two `codex_hook` marker-fallback tests,
  and a `codex_hook_integration` strict-gate test. Live verification still gated on
  `#codex-auto-queue-live-verify`.
- **Recursive direct-invocation guard abandons its empty cycle (`#recguard-abandon`).**
  When a Codex-backed `agent-doc <FILE>` runs inside the same tmux pane that already
  owns the document, the recursive-invocation guard fails fast with the existing
  `recursive direct invocation would deadlock` diagnostic — but it previously left the
  freshly-opened preflight cycle in `preflight_started`, so `session-check` reported an
  interruption and the owner session stayed wedged until a manual `agent-doc cancel`.
  `run.rs` now marks that empty cycle terminal (`Abandoned`) via a new
  `abandon_run_recursive_cycle` (the guard fires before any response capture, so nothing
  is lost); `session-check` accepts the terminal abandoned state automatically. Regression:
  `recursive_direct_invocation_abandoned_cycle_passes_session_check`.
- **Multi-retry / late-IPC response duplication hardened (`#finalize-retry-ipc-response-duplication`).**
  A closeout needing several finalize/write retries with the IDE IPC listener active and
  concurrent editing could leave a duplicated `### Re:` response block whose stale copy had
  its body lines wrongly prefixed with the `❯ ` user-prompt marker, fail-closing
  `session-check` until a manual `git checkout`. Fixes: `canonicalize_answered_prompt_prefixes`
  never `❯`-prefixes a prose block that butts directly against a preceding response heading
  (it's that response's body, not a user prelude); dedupe response-block normalization is
  now `❯`-insensitive and drops the corrupted copy of a duplicate pair; the late-IPC
  over-application / JB-cache replay detectors recognize the `❯`-corrupted-duplicate shape,
  so `preflight` restores clean HEAD automatically. Regressions in `dedupe`, `git`, and
  `session_check`.
- **Prompt-prefix dedup gap fixed (`#prompt-duplicated-while-typing`, partial).** The
  shared-core append dedup (`component::append_patch_already_present` via
  `normalize_append_patch_content`) stripped boundary / `(HEAD)` markers but not the
  `❯ ` user-prompt prefix. A synthesized boundary-aware exchange patch and the live
  editor buffer can differ by exactly that prefix, so the `contains` dedup missed and
  the prompt re-appended → the duplicate-while-typing report. `normalize_append_patch_content`
  now strips a leading `❯ `/`❯` prefix (`strip_user_prompt_prefix`), making dedup
  prefix-agnostic and symmetric (cannot collapse a distinct prompt — the glyph is
  presentation, not content). The JetBrains plugin loads the cdylib by path and
  hot-reloads on mtime change, so `agent-doc lib-install` ships this to the live editor
  with no plugin reinstall. Tests: `append_patch_already_present_ignores_user_prompt_prefix`,
  `append_patch_distinct_prompts_not_deduped`,
  `append_with_caret_does_not_duplicate_prefixed_prompt`. The structural buffer-snapshot
  race (synthesized patch carries the buffer at T1 while it advanced to T2) remains
  tracked. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md.

- **Queue head no longer struck on halt/refusal responses (`#queue-strike-on-halt`).**
  Consuming the active `agent:queue` head now requires an explicit completion
  signal. The CLI `finalize` / `write --commit` path requires a closeout flag —
  `--done`, `--pending-gate`, or `--pending-edit "<id>=…"` — naming the head id
  (or a genuine fresh operator prompt-target / `do queue` trigger); the old
  "`### Re:` heading mentions the head → consume" heuristic is removed, so a halt
  response that explains why the item should stay open no longer silently strikes
  it. The Codex Stop-hook auto-close path (no closeout CLI flags) still consumes
  from a heading but only on an exact topic match (`### Re: do [#id]`), never on a
  modified heading like `### Re: #id halt`. New `queue_head_has_explicit_completion_signal`
  in `write.rs`; `response_topic_matches_queue_head` narrowed to exact-match.
  Coverage: `explicit_signal_*` + `heading_topic_matches_head_exactly_only`
  (write.rs) and `halt_response_does_not_strike_queue_head_but_done_flag_does`
  (sim_world.rs). Spec: `07-orchestration-commands.md` + `SPEC.md`. Plan:
  `tasks/agent-doc/plan-queue-strike-on-halt-response.md`.

- **Queue/IPC buffer convergence seam (`#adoc-queue-ipc-buffer-divergence`,
  root cause #2).** Queue maintenance now converges a live route-owned editor
  buffer to the committed inactive queue shape after a halt/drain. Previously a
  content-only IPC patch could not change the `<!-- agent:queue auto -->`
  opening-tag attribute or the `queue_active:` frontmatter, so a live IDE buffer
  re-added `auto`/`queue_active: true` on its next flush and the snapshot/HEAD
  drift loop regenerated on every preflight. New `agent_doc_converge_queue_auto`
  FFI export (`agent-doc-core`, takes a C int for a stable JNA ABI) rewrites the
  queue opening-tag attribute; `ipc_socket::send_queue_convergence` carries the
  desired `queue_auto` state plus the `queue_active` frontmatter; preflight's
  `run_queue_maintenance` pushes the convergence through the listener after each
  halt/drain disk write (best-effort, non-fatal). JetBrains `PatchWatcher`
  parses `queue_auto` and applies it via `NativePatching.convergeQueueAuto` in
  both the Document-API and VFS apply paths (plugin `0.2.137`). Deterministic
  SimWorld repro: `queue_maintenance_converges_live_ipc_buffer_on_item_modified_halt`
  starts a simulated IPC listener, halts an active auto-queue, and asserts a
  single convergence message + idempotent follow-up. Plan:
  `tasks/agent-doc/plan-queue-ipc-drift.md`.

- **`agent-doc-core` v0.1.0 published to crates.io.** The pure document data
  layer (`#adcr` extraction: component parsing, frontmatter, template, CRDT,
  pending, diff classification, model tier, syntax, and the full pure C-ABI FFI
  surface) is now a standalone published crate. `publish = false` removed; all
  dependencies are crates.io crates (no path/git deps). Enables third-party FFI
  consumers and the editor-plugin slim-link target (link `agent-doc-core` —
  ~9.87s cold / 74 crates — instead of the full `agent-doc` orchestration crate
  — 129s / 266 crates). The `#k9e1`/`#epv5`/`#vb8h`/`#e130` FFI relocations
  moved all 15 pure FFI functions into `agent_doc_core::ffi` ahead of this.
- **Strict finalize appends no longer overwrite prior exchange responses when
  the explicit baseline is stale.** For template/CRDT append-mode exchange
  writes under `finalize` or strict `write --commit`, if the supplied
  `--baseline-file` is missing exchange content already committed in `HEAD`,
  the write path now applies the response on top of `HEAD` before producing
  `content_ours`, IPC snapshots, or commit-staged snapshots. This keeps
  back-to-back finalizes from dropping the previous `### Re:` block and logs
  `explicit_baseline_rebased_to_head` when the repair path is used. Regression:
  `finalize_stream_rebases_stale_exchange_baseline_to_head`. Closes
  `#finovrwr`.

- **OpenCode dispatch-only reroutes now have dispatch-start proof.** Route
  captures the OpenCode pane before submit, waits for the routed trigger to
  leave the composer, and accepts proof only when the pane leaves idle chrome
  within the OpenCode redraw budget. Proven OpenCode delivery now logs
  `proof=pane_state_changed proof_scope=dispatch_start`; accepted-only
  OpenCode delivery still fails closed.

- **Finalize now consumes answered queue-synthetic prompts.** When an active
  `agent:queue auto` head is the only prompt diff, `finalize` can now consume
  it after the response is written if the captured `### Re:` heading targets
  the queue head's id (for example `#spec-test-build-install-commit-push`).
  Unrelated baseline prompts still preserve the queue head.

- **Queued JetBrains Run Agent Doc reroutes now survive live prompt edits.**
  When `route --dispatch-only` queues a busy-actor rerun by saving
  `agent:queue auto` to the snapshot but `HEAD` still lacks that handoff, the
  next preflight auto-commits the route-owned queued snapshot before diffing.
  If the user edits the visible prompt meanwhile, that edit stays uncommitted
  in the working tree and becomes the fresh prompt diff instead of wedging the
  queue behind the generic `snapshot differs from HEAD` recovery hint.
  Repeating the editor action with updated prompt text now replaces the sole
  live route-owned `agent:queue auto` prompt instead of leaving stale wording
  queued behind the active turn.

- **Template exchange appends now keep response headings block-separated.**
  When a `<!-- patch:exchange -->` response starts with `### Re:`, the
  boundary-replacement and fallback append paths insert a blank line after
  non-empty prior exchange content. This prevents Markdown renderers from
  joining a new response heading to the previous paragraph when the prior
  response lacked a trailing blank line.

- **JetBrains Run Agent Doc now surfaces queued busy-actor reroutes.** When
  `agent-doc route --dispatch-only` accepts a prompt by adding it to
  `agent:queue auto` behind a busy authoritative actor, the IDE action now
  treats that output as a queued/still-running outcome instead of silent
  success. The notification keeps the route details copyable and tells the
  user the request is waiting for the active turn to drain.

- **Socket/file ACK-content sidecars can no longer commit duplicated user
  prompt text.** The write path now treats editor ACK content as a
  whole-buffer observation that still must pass response-aware prompt
  multiplicity checks before snapshot adoption. If the sidecar has extra
  user-prompt copies relative to the agent-owned `content_ours` response image,
  `agent-doc` logs `ipc_snapshot_adoption_blocked
  reason=prompt_duplication_in_ack_content`, saves `content_ours`, marks the
  cycle so commit staging cannot absorb the bad buffer, and repairs the visible
  duplicate through the guarded disk repair path. This closes the
  `tasks/professional/equityfundingsource.md` corruption shape where a narrow
  editor patch succeeded but the full-document ACK sidecar carried duplicated
  prompt text while the user was typing.

- **JetBrains Run Agent Doc retries transient dispatch-only Codex boot/busy
  refusals without masking protected input.** The IDE retry loop now recognizes
  the binary's `latest run is still booting` route refusal when the ready probe
  ended on `active codex turn` or `timed_out`, so fast repeated clicks do not
  strand behind a stale startup projection or still-running turn. Shell history
  search and other protected-input blockers remain terminal route failures.

- **JetBrains File Cache Conflict Cancel recovery is now pinned for the
  direct `write_applied` wedge.** The preflight regression suite now covers
  the exact Cancel-shaped closeout where the working tree and snapshot already
  contain the response but `HEAD` does not and the cycle is still
  `write_applied`. Preflight must classify that as
  `jb_cache_conflict_cancel` and close the missing commit boundary
  automatically, matching the already-covered committed-cycle variant and the
  JetBrains plugin Cancel contract.

- **Claude Code auto-loop guard no longer blocks on routine
  managed-component state edits.** The SKILL.md auto-loop rule previously
  fired only when `prompt_bearing_changes` was empty or exactly the
  queue-synthetic head prompt. In practice every meaningful queue cycle
  produces queue-activity toggles, queue item add/strike lines, or
  backlog/review/done item edits that preflight classified as
  `content_edit` / `prompt_target` and tripped the guard. Net effect: the
  auto-loop almost never fired for real queue work. Preflight now emits a
  new `user_intent_prompt_changes` field that filters the same change list
  through `diff::change_is_managed_state_only`, which recognises
  queue/backlog/review/done component-marker lines, `queue_active:`
  frontmatter flips, `- do ...` queue items (including struck `- ~do ...~`),
  and standard task-list items as managed state rather than user prompts.
  The SKILL.md auto-loop section now reads `user_intent_prompt_changes`
  instead of `prompt_bearing_changes` so routine session bookkeeping does
  not interrupt the queue drain. Real user prompts (free-text questions,
  imperative directives outside the managed components) still appear in
  `user_intent_prompt_changes` and continue to block. 7 new unit tests in
  `diff::tests::change_is_managed_state_only_*`. Plan: `#ccloopguard`.

- **JetBrains plugin (0.2.131) now emits `already_applied` socket-IPC acks
  via the new FFI v2 listener.** When the plugin's apply path detects that
  the incoming patch produces no structural change against the live editor
  buffer (response body already present from a prior socket retry, the
  in-process dedup cache, or the force-disk sentinel), it returns
  `2 → {"type":"ack","status":"error","reason":"already_applied"}` instead
  of `1 → status:ok`. The binary's `is_already_applied_error` gate then
  skips the file-IPC fallback that would otherwise stack a duplicate
  `### Re:` heading on top of the live buffer. New FFI export
  `agent_doc_start_ipc_listener_v2(project_root, callback)`; the v1 export
  remains for older plugins. JB plugin prefers v2 and falls back to v1 on
  binaries that don't export it (`UnsatisfiedLinkError` / `NoSuchMethodError`).
  Closes `#ipcpluginalready`.

- **File-IPC fallback hash-skips response patches that are already applied
  to the live buffer.** Defense-in-depth complement to the `already_applied`
  socket-IPC gate (and to `#ipcpluginalready` until every plugin emits the
  signal). In `try_ipc`, when the patches are response-bearing (contain at
  least one `### Re:` heading) and `apply_patches(current, patches)` is a
  structural no-op against the live file (boundary markers excluded), the
  file-IPC fallback short-circuits as success without writing the patch
  file. Non-response (prompt/component) patches still flow through the
  existing path so its no-ack guard for unacknowledged live-edit IPC stays
  authoritative. New test `try_ipc_file_fallback_skips_when_patches_already_applied_to_live_buffer`.
  Closes `#ipcfilehashskip`.

- **Test fixture migration: `agent:pending` → `agent:backlog`.** The
  `tagpath lint --dialect agent-doc` gate added in the prior release
  blocked the deprecated `agent:pending` component name. Migrated 31 sites
  in `tests/finalize_integration.rs` and 4 sites in `tests/run_integration.rs`
  to the canonical name; 30 previously-failing integration tests now pass.
  `tests/pending_integration.rs` keeps the legacy alias intentionally
  (those tests exercise the alias migration path). Closes `#ipclegacyfix`.

- **SimWorld regression coverage for the IPC corruption + duplicate response
  race when the user types into the post-`/agent:exchange` scratch comment
  during finalize.** New deterministic scenario in `src/sim_world.rs` exercises
  the `is_already_applied_error` gate: when socket IPC returns
  `{"type":"ack","status":"error","reason":"already_applied"}` after the plugin
  has applied the patch via a prior socket retry, the file-IPC fallback must be
  skipped so the response is not duplicated on top of the live buffer. Includes
  the counterfactual dedupe-recovery path to prove `dedupe_ipc_snapshot_content`
  still collapses the duplicate if the gate ever regresses. Also adds two
  integration-style tests for the `recover_empty_response_for_strict_closeout`
  wrapper in `src/write.rs` covering the full `agent-doc dedupe` →
  `agent-doc write --commit` (empty stdin) recovery path: the dedupe-only drift
  is committed through the binary path under strict closeout, and the
  non-strict path stays read-only. Closes Phases 1 and 5 of
  `tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md`.

- **`finalize` / `write --commit` now invoke `tagpath lint --dialect agent-doc`
  before the snapshot/commit boundary.** Malformed session-document
  directives — for example `<!-- agent:done archive PATH -->` missing `=` —
  now fail closed at the lint gate with a structured error pointing at the
  rule, line, column, and fix hint, rather than crashing deep inside
  `finalize`. The gate is a library call against `tagpath`'s agent-doc
  dialect (no subprocess overhead). Mode resolution: CLI `--lint=off|warn|
  strict` > frontmatter `agent_doc_lint_dialect: off|warn|strict` >
  workspace `.agent-doc/config.toml` `[lint] dialect` > default (`warn`).
  Default behavior: errors block, warnings surface on stderr. `strict`
  promotes warnings to errors; `off` skips the gate (logged via `ops_log`
  for audit). New module: `src/lint_gate.rs`.

- **Newly activated auto queues no longer stall as modified in-flight work.**
  Preflight now treats a queue that was inactive in the snapshot and newly
  activated by the current document as operator-authored input, snapshots the
  activated queue body as the closeout baseline, and reserves
  `queue_halted: "item_modified"` for queues already active in the snapshot.
  This prevents intentional queue rewrites plus `auto` from stripping the
  auto attribute before the first queued item can run.

- **Full-document editor IPC is disabled end-to-end.** The binary no longer
  emits `fullContent` socket/file IPC payloads, even for no-component append
  fallbacks, repair redelivery, or operator mutations. Scope rejection still
  logs for template/component documents, committed-cycle cleanup still runs for
  stale fallback patches, and otherwise eligible paths log
  `full_content_ipc_disabled` before falling back to guarded disk/snapshot
  repair. JetBrains and VS Code now also reject or delete legacy/foreign
  `fullContent` payloads without applying whole-document editor replacements.
  Added a separate Mermaid reference for the repeated corruption chain. Bumped
  local plugin builds to JetBrains `0.2.129` and VS Code `0.2.21`.

- **Commit closeout repairs live prompt prefix duplicates before staging.**
  Snapshot-staged commit closeout now runs a narrow in-exchange prompt duplicate
  repair before staging, collapsing adjacent prefixed/raw copies of prompt text
  already represented in the snapshot. When the repaired file is only
  prompt-prefix-equivalent to the snapshot, the snapshot advances to that
  repaired content so queue-freeze and other prompt-only closeouts do not leave
  a duplicated prompt in the editor buffer or misclassify it as out-of-band
  drift.

- **Route/preflight now preserve visible post-exchange scratch comments.**
  Duplicate-prompt cleanup now treats the current visible document as ownership
  proof for ordinary HTML comments below `agent:exchange`, so route, preflight,
  and final closeout do not empty scratch comments the user typed before the
  mutation. Generated duplicate comment residue can still be scrubbed when it is
  absent from both the baseline/snapshot and the current file used for the
  write, while exact duplicate answered prompt tails inside `agent:exchange`
  remain auto-cleaned. The regression suite now includes a SimWorld/integration
  no-delete matrix across route cleanup, preflight recovery, direct write,
  IPC/plugin handoff, repair write-commit, compact exchange, and generated
  residue diagnostics.

- **Compact Exchange no longer emits full-document editor IPC.** Template
  exchange compaction now uses the visible idle + compare-and-swap direct-write
  guard even when an editor patch directory is present. This removes the
  `compact_exchange` fullContent path that could replace an active editor buffer
  immediately before the operator drafted the next prompt.

- **Queue closeout consumption now requires active-head proof.** Strict
  `finalize` / `write --commit` no longer consume an active queue head solely
  because the pre-response baseline and live document have no prompt diff. The
  closeout must see the exact queue-head prompt, a queue-synthetic run diff, or
  a matching `--done <id>`, so unrelated already-baselined prompts such as
  `#next-steps` cannot advance `agent:queue auto`.

- **Repair closeouts preserve auto queues unless the response targets the
  head.** Missing-response materialization through `write --commit` or the Codex
  Stop hook now leaves the active queue head and `auto` attribute intact unless
  the closeout carries explicit head proof, such as a matching `--done <id>` or
  a `### Re:` topic for the current queue item.

- **Codex child process launch retries transient executable-busy errors.**
  The Codex/OpenCode backend now retries child process spawn when Linux returns
  `ETXTBSY` for a just-written executable. This hardens the streaming resume
  retry tests and normal child launch path against CI filesystem races without
  masking other spawn failures.

- **Editor repair cleanup now distinguishes snapshot-only from redelivery.**
  The editor specs now make typed IPC repair decisions explicit: snapshot-only
  repair stays binary-owned, narrow `normalize_prefix_lines` + boundary
  reposition payloads stay on the normal patch path, and full-content redelivery
  is disabled in the first-party binary. VS Code and JetBrains tests now pin the
  narrow repair shape so pure-reposition shortcuts cannot absorb it.

- **File IPC sidecar-normalization fallback now has narrow repair coverage.**
  The file-IPC fallback path is covered by a regression proving prefix-only
  sidecar divergence queues a `patches: []` repair with `normalize_prefix_lines`,
  boundary repositioning, and stale-buffer proof. The closeout spec now calls
  out socket and file IPC as the same narrow-first contract and disables
  full-content redelivery.

- **The tsift.md duplicate-content IPC incident is now a named regression.** A
  focused fixture models stale duplicate-response repair planning while the
  visible `tasks/software/tsift.md` buffer receives a new prompt. The regression
  proves response-fallback full-document redelivery skips socket/file IPC,
  leaves the live buffer untouched, and logs the disabled/stale proof decision.

- **Codex Stop hooks now keep harness-native auto queues moving.** After a
  clean `finalize` / `session-check`, `codex-stop` now detects an active
  `agent:queue auto` with a ready next head prompt, blocks final-answer
  delivery, and tells Codex to invoke `agent-doc <FILE>` again in the same
  turn. The hook records the requested head and fails closed if a repeated
  continuation reaches Stop without the queue head advancing, preventing
  infinite hook loops.

- **Manual queue closeouts can drain explicit done-backed batches.** Strict
  `finalize` / `write --commit` closeouts now consume all contiguous active
  queue head prompts whose `do #id` items were resolved by repeated `--done`
  flags, while still stopping before the first unresolved prompt and proving the
  same queue range against the saved snapshot before mutation. This lets a
  harness-native response that handled a whole queued batch close the queue in
  one binary-owned commit instead of leaving later completed queue items behind.

- **Prompt-normalization overruns no longer force-commit.** The
  `MAX_NORMALIZE_USER_LINES` guard now logs `normalize_threshold_exceeded
  action=passthrough` and leaves the content unchanged for the typed
  repair/closeout path, removing the broad force-commit workaround that could
  absorb unrelated drift from inside prefix normalization.

- **Duplicate-prompt repair now has one write-path pipeline.** Closeout,
  content-ours normalization fallback, and IPC snapshot repair now share a
  canonical duplicate-prompt artifact repair that handles adjacent duplicate
  response blocks, answered prompt tails, post-exchange duplicate prompt
  comments, before-content prompt-line duplicates, and live prompt prefix
  variants in one audited pass. The aggregate
  `duplicate_prompt_artifact_repair` log records which artifact classes changed
  while preserving the existing narrow diagnostic markers.

- **IPC repair state is now a typed decision.** Sidecar-normalization fallback
  and duplicate-response IPC dedupe now resolve a single repair decision carrying
  the repaired snapshot content, snapshot source, disk-repair reason, bad editor
  buffer fingerprint, normalization targets, and explicit editor-redelivery flag
  before touching disk or sending editor repair IPC. Prefix-only sidecar
  divergence now tries a narrow `normalize_prefix_lines` + boundary-reposition
  patch before full-content repair, and repair/redelivery ops logs include patch
  ids, hashes, prefix counts, duplicate-prompt counts, and stale-proof skips.
  This keeps stale-editor redelivery, disk repair, and snapshot save behavior on
  one auditable branch.

- **Owned scratch comments survive duplicate prompt cleanup.** Closeout,
  preflight, and route duplicate-prompt cleanup now preserve post-exchange HTML
  comment lines that were already present in the pre-response baseline/snapshot
  or in the visible document used for the mutation. The scrub still removes
  generated duplicate prompt residue with no ownership proof and preserves the
  comment shell, but it no longer empties a user's parked scratch prompt such as
  the `tsift.md` `#next-steps` comment after the prompt is answered.

- **Answered prompt tails after the exchange boundary are scrubbed before redispatch.**
  Template normalization, preflight, and route cleanup now remove an exact raw
  prompt tail after the latest `agent:boundary` when that prompt block already
  has an assistant response earlier in `agent:exchange`. Preflight runs the
  cleanup before the commit step can reposition the boundary, preventing the
  already-answered prompt from reappearing as fresh prompt-bearing diff.

- **Mixed scratch comments preserve unrelated lines during duplicate cleanup.**
  When generated post-exchange HTML comment residue lacks ownership proof,
  cleanup removes only the duplicate prompt lines from multiline comments
  without applying a fuzzy whole-comment match that can erase unrelated
  scratch/log-triage text in the same comment. Added editor-visible and
  preflight regressions for the live `agent-doc-bugs2.md` mixed-comment shape.

- **Full-content replacements now bind to their computed source buffer.** Compact
  Exchange and other operator-owned whole-document replacements stamp editor IPC
  with the exact source buffer used to compute the replacement, not a late disk
  reread, and direct disk fallback uses the same visible-current compare-and-swap
  guard. Socket full-content ACKs are also rejected before snapshot save when the
  materialized document differs from the payload. This closes a live-typing race
  where a compact/full-content write could accept or persist content derived from
  an older buffer while the user was typing the next prompt.

- **Dispatch-only editor reroutes recover degraded authoritative panes.** When
  JetBrains `Run Agent Doc` finds an authoritative actor pane whose supervisor
  socket is missing or whose runtime actor state is absent, route now keeps that
  pane as the recovery target if it is still the current registered/live owner.
  The reroute records controller dispatch, logs
  `route_dispatch_only_authoritative_degraded_direct_pane`, and then uses the
  normal direct-pane readiness/blocker/proof gates before submitting, avoiding a
  first-open manual `agent-doc start <FILE>` rebind when the live pane is already
  dispatch-ready.

- **Freeform duplicate prompt residue now fails closed.** After the safe
  post-exchange HTML comment scrub runs, route, editor-visible normalization,
  final template reconciliation, and IPC snapshot dedupe reject remaining
  duplicate or near-duplicate prompt text in ordinary post-exchange Markdown
  outside tracked components. This keeps arbitrary manual Markdown edits from
  being silently committed or dispatched when there is no ownership proof for
  deleting or relocating the duplicate text.

- **Missed response materialization no longer closes as already committed.**
  IPC ACK/sidecar success now proves that the expected response body actually
  materialized before saving the snapshot, logging
  `ipc_materialization_missing_response` and falling back when the editor
  returns prompt-only or partial response content. `agent-doc commit` also
  refuses the `snapshot == HEAD` already-current no-op when an active captured
  response is absent from the staged snapshot, leaving the cycle recoverable
  through `agent-doc write --commit <FILE>`.

- **Preflight baseline capture is tied to the stable visible diff.** Preflight
  now waits for the shared editor typing indicator before any document-mutating
  recovery, commit, pending maintenance, or duplicate prompt residue cleanup.
  The emitted baseline is saved from the same stable visible content used for
  diff computation, preventing cleaned baselines from diverging from editor
  replayed prompt/comment content.

- **Generated post-exchange duplicate prompt comments are cleaned.** IPC
  snapshot dedupe and final template reconciliation remove ordinary HTML
  comment bodies after `agent:exchange` only when they duplicate or
  near-duplicate a prompt already present in the exchange and lack
  baseline/snapshot/current-visible ownership proof. Unrelated and visible
  scratch comments stay user-owned and remain outside `agent:exchange`.

- **Route pre-dispatch preserves visible scratch comments.** `agent-doc route`
  still removes exact duplicate answered prompt tails before sending a routed
  reopen, but ordinary post-exchange HTML comments already visible in the file
  are ownership-protected instead of being emptied as duplicate prompt residue.

- **Lower-agent job packet MVP.** `agent-doc plan` now emits deterministic
  lower-agent routing fields (`dispatch_candidate`, task class, risk,
  parallelism, model tier, context budgets, write scope, proof requirements,
  dispatch mode, and tsift context commands). New `agent-doc jobs
  create/list/status/collect` commands generate `agent-doc-job-packet-v1`
  markdown packets under `.agent-doc/jobs/<cycle>/`, expand compound `do`
  directives into one packet per target, derive target-specific write scopes
  from backlog path references, optionally write operation docs, attach tsift
  context and bounded graph acceptance evidence when available, and collect
  validated `agent-doc-worker-result-v1` envelopes for parent review without
  applying patches or bypassing finalize.

- **tsift dispatch-trace audit data now rides with graph-backed orchestration.**
  `agent-doc plan` / `orchestrate` now collect `dispatch-trace-v1` alongside
  graph-db evidence and conflict matrices, fail closed on missing projection
  hashes, worker feedback, replay/repair commands, or graph links, and attach
  that audit context to each normalized lower-agent job packet. Sequential/DAG
  child closeouts now append a hidden `worker_result` line with status, target
  id, touched files, tests, and follow-up ids before `finalize`, allowing the
  next tsift projection to connect worker outcomes back into graph evidence.

- **tsift conflict-matrix orchestration now carries the full planner contract.**
  `agent-doc plan` and orchestration prompts now preserve the
  `conflict-matrix-v1` context-pack, cached diff, impact, ranked candidate,
  conflict, worker prompt packet, token budget, semantic ranking fields, and
  normalized lower-agent job packet emitted from tsift. Graph-backed
  plan/orchestrate now rejects stale or underspecified envelopes before
  dispatch: evidence packets must be `graph-db-evidence-v1` with packet ids,
  projection hashes, replay commands, and repair commands; conflict matrices
  must be `conflict-matrix-v1`; worker packets must be
  `worker-prompt-packet-v1` with packet ids, projection hashes, token budgets,
  and explicit fail-closed prompt text. Parallel orchestration now blocks
  unless tsift explicitly reports `can_parallel=true` and
  `fail_closed=false`, so shared symbol/test risks cannot slip through just
  because they are not file-level fail-closed conflicts.

- **IPC duplicate-response detection now uses normalized response deltas.**
  IPC timeout fallbacks and ack-content normalization fallbacks compare the
  normalized `base -> content_ours` response insertion hunks against the
  current `agent:exchange` before adopting editor-applied content. Boundary
  churn, ordinary comments, and prompt-prefix-only normalization are ignored,
  but a single overlapping response body line is no longer treated as proof
  that the plugin applied the full response. This prevents both false adoption
  and CRDT replay of an already-visible editor response.

- **Pending-done guard now distinguishes kept-open pending mutations from completion.**
  Same-cycle `--pending-edit`, `--pending-gate`, `--pending-ungate`,
  `--pending-reorder`, and gate-type edits record a kept-open id ledger that
  suppresses missing-`--done` warnings for items intentionally left active or
  gated. The guard still scans response text for real completion signals,
  including `### Re: do [#id]` headings with later commit/push/verification
  evidence, so completed `do #id` batches no longer slip through just because
  ids only appeared in the response heading.

- **Mixed duplicate-scaffold closeouts now fail closed.**
  When a duplicated template scaffold lands between two `agent:exchange` close
  markers and strands live prompt text in that duplicated segment, the
  closeout normalizer now refuses automatic repair and logs a typed
  `flow::document_mutation` event with `reason=mixed_duplicate_scaffold_tail`;
  editor/FFI normalization also rejects the shape. Pure duplicated scaffold
  with no live text is still dropped automatically, but mixed live-typing
  content is preserved for explicit recovery instead of being reordered or
  duplicated during closeout.

- **Legacy full-content editor IPC proof remains diagnostic.** The binary keeps
  source-buffer proof helpers, but first-party CLI paths now skip `fullContent`
  emission by default and editor plugins no longer apply legacy/foreign
  whole-document replacements.
  Bumped local plugin builds to JetBrains `0.2.127` and VS Code `0.2.20`.

- **FlowCore now has an executable guard/proof regression gate.**
  Routed-reopen prompt-ready and dispatch-proof failure reasons now pass through
  `RoutedReopenGuardReason` instead of free-form strings from `route.rs`, and a
  source-token budget test flags unaudited new hot-path guard/proof/reason
  tokens before they can bypass the owning FlowCore enum/event.

- **Clear Session Context no longer treats the `agent-doc` wrapper process as
  blocking evidence by itself.** File-scoped `session clear` now blocks on
  protected prompt input or explicit busy cues such as an active Codex turn,
  hook-review prompt, or help screen, but proceeds for ordinary idle/status
  panes even when `pane_current_command=agent-doc`. JetBrains now parses legacy
  `active_agent_doc` clear refusals as typed busy-session warnings and exposes a
  standalone `Interrupt and Clear Session Context` action. Bumped the JetBrains
  plugin build version to `0.2.126`.

- **Template closeout uses one prompt reconciliation pass before visible writes.**
  Direct template/CRDT disk writes, IPC timeout fallbacks, and repair replays
  now run the same duplicate-prompt reconciliation that IPC snapshots use,
  before saving snapshots or replacing the document. The scanner is
  response-block aware, so prompt text quoted in assistant prose is preserved
  while duplicate live prompt copies are removed before closeout.

- **Editor IPC patches now prove the live buffer generation before mutation.**
  JetBrains and VS Code capture the editor buffer text plus generation after
  typing debounce and re-check that proof immediately before component append,
  socket IPC, and full-content repair writes. Stale generation mismatches now
  reject the editor mutation without ACK, and socket `status:error` acks are no
  longer treated as successful delivery. Bumped local plugin builds to JetBrains
  `0.2.125` and VS Code `0.2.19`.

- **Visible writes now prove the merged current document is still current.**
  Template/CRDT disk writes, IPC timeout fallbacks, and repair replays now
  re-read the session markdown after the active-typing guard and fail closed if
  the file changed after the response merge was computed. This keeps late
  scratch-comment or live exchange typing visible for the next cycle instead of
  committing a stale merge that can reintroduce duplicate/corrupted content.

- **FlowCore active-typing guard now blocks visible document writes.** Direct
  disk write paths consult `flow::document_mutation` before snapshot/document
  mutation and fail closed when the shared typing indicator never reaches idle.
  JetBrains and VS Code patch watchers now treat typing-debounce timeouts as
  no-mutation retry states instead of applying patches or boundary reposition
  while the user is still typing. Bumped the JetBrains plugin build version to
  `0.2.123`.

- **FlowCore owns the next closeout, mutation, and session-cycle slices.**
  `flow::document_mutation` now parses and classifies template patchback shapes
  before visible writes across template, stream, IPC, and repair replay paths,
  including orchestrate-origin plain-response rejection. `flow::closeout` owns
  the strict terminal transaction for commit, snapshot convergence, parent
  gitlink verification, session-check, and fallback-patch cleanup. `preflight`
  and `plan` now share `flow::session_cycle` prompt-target and finalize-command
  helpers so pending `--done` / cross-document add requirements come from one
  typed cycle contract.

- **Routed-reopen FlowCore owns the authoritative actor action slice.** The
  authoritative actor ready-wait facts, retry budgets, recovery hints,
  delivery-action classifier, and dispatch-start proof typing now live in
  `flow::routed_reopen`. `route.rs` maps tmux/supervisor/controller runtime
  facts into those pure helpers, then performs only the selected side effect.

- **Routed-reopen FlowCore owns the first route decision kernel.** Delivery mode,
  dispatch-start proof, degraded-authority refusal, runtime guard, and
  prompt-ready-barrier classifiers now live in `flow::routed_reopen`; `route.rs`
  maps supervisor/controller facts into those typed decisions and remains the
  tmux/supervisor/controller I/O coordinator. The large route test module was
  split out to `src/route/tests.rs` so live tmux fixtures no longer live inline
  in production routing code.

- **FlowCore mirror-mode typed events are in place.** Added the first `flow` module set for session-cycle, routed-reopen, closeout, document-mutation, operator-clear, and orchestration-batch ownership; ops summary now groups `flow_event` diagnostics by flow stage, and route/closeout/write paths emit initial mirror events for prompt-ready failures, commit closeout completion, and malformed patchback parse failures. The new flow map documents hot-path ownership and duplicated state checks for the next extraction phases.

- **Clear Session Context recognizes Codex's `Write tests for @filename` idle placeholder.** Operator status/clear readiness now treats the current dim Codex suggestion `› Write tests for @filename` as prompt-ready idle evidence, so an `agent-doc` wrapper pane with only that placeholder and the Codex model/cwd/context footer no longer stays classified as `alive-busy prompt_ready=false`. Real drafted input, queued drafts, shell search, active permission prompts, and panes showing `Working (... esc to interrupt)` still fail closed.

- **Codex Stop parent-pointer regression now accepts earlier strict-closeout blocks.** The Stop-hook submodule closeout regression now only requires stale parent gitlink drift when the response commit advanced inside the submodule and the parent-pointer commit is the failing layer. If strict closeout fails earlier before the submodule commit advances, the hook still blocks and preserves tracking, and the spec now states that no parent gitlink drift is required in that branch. This closes `#wnj2` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Clear Session Context treats an idle Codex footer below old transcript text as idle.** Operator status/clear evidence now accepts a bottom Codex model/cwd/context footer as `prompt_ready=true` even when previous assistant output remains visible above it, while drafted prompt input, queued composer state, and other busy cues still fail closed. Route dispatch still requires a real dispatch-ready prompt before injecting a reopen.

- **JetBrains Clear Session Context recognizes active-pane refusals.** The plugin now parses the binary's newer `session_clear refused ... pane ... is still active` output, including the generic `agent-doc command failed` wrapper, and shows the typed running-session warning with retry/status/interrupt/copy actions instead of surfacing the raw command failure. Bumped the JetBrains plugin build version to `0.2.122`.

- **JetBrains Clear Session Context typed warning was live-validated.** A live IDEA replay against `tasks/agent-doc/agent-doc-bugs2.md` now surfaces the typed running-session warning for an active `agent-doc` pane, including retry guidance, interrupt-clear recovery, and the latest pane output. The editor spec and regression suite now pin that observed warning shape.

- **JetBrains Clear Session Context keeps live-pane busy evidence authoritative.** A follow-up 0.2.122 replay showed the actor/controller projection can be `ready` while the direct Codex pane is still running the active `agent-doc` turn. The JetBrains refresh-retry readiness helper now has coverage that `alive-busy prompt_ready=false` does not retry clear from that state; the spec names waiting, refresh-after-idle, or explicit interrupt-clear as the valid operator choices.

- **Terminal user follow-ups no longer emit late closeout no-ops.** When the previous cycle is already committed and the working tree only contains a new user follow-up prompt, `agent-doc commit` now treats that state as prompt handoff instead of re-emitting `commit_noop` / `commit_already_current` lifecycle bookkeeping. Open recovery cycles can still close as already-current when needed, but idle post-finalize prompt typing no longer looks like another delayed closeout.

- **CI checks out sibling path dependencies.** Pull-request CI now clones `btakita/agent-kit` and `btakita/tmux-router` next to the `agent-doc` checkout before running `make check`, matching the local workspace layout required by the `../agent-kit` Cargo path dependency and the `Cargo.toml` tmux-router patch.

- **CI now names the tmux integration leg explicitly.** The GitHub Actions workflow labels the normal suite as `Run make check` and the live tmux sweep as `Run make tmux-ci`, with a visible `Running make tmux-ci` marker in the step log so reviewers can confirm the tmux leg executed.

- **Preflight now cleans duplicate prompt scratch comments before baseline capture.** When a submitted prompt is already present in `agent:exchange` and the same text remains in the ordinary HTML comment below `<!-- /agent:exchange -->`, preflight removes only that duplicate comment before the previous-cycle commit/baseline step. Unrelated scratch comments remain outside exchange, and the snapshot still excludes the live prompt. Added a focused preflight regression and updated the closeout spec.

- **Compact Exchange IPC is no longer blocked by committed response-cycle state.** The compact command now sends its full-document replacement through an operator-mutation IPC path, so JetBrains can apply Compact Exchange through the Document API even after the prior agent-doc response is already committed instead of falling back to a direct disk write and surfacing an external-file-change dialog. Added a regression for committed-cycle Compact Exchange IPC and refreshed the shared editor spec.

- **Clear Session Context recognizes the default Codex idle placeholder.** The operator status/clear readiness path now treats `› Ask Codex to do anything` plus the Codex model/cwd/context footer as an idle composer, so JetBrains Clear Session Context does not fail closed with `current_command=agent-doc prompt_ready=false` when the pane is only showing the default Codex placeholder/status UI. Drafted prompt input, queued drafts, and shell search still fail closed and point to `session interrupt-clear`.

- **Post-exchange hidden prompt duplicates are cleaned during closeout.** Final template reconciliation and IPC ack-content snapshot dedupe now remove ordinary HTML comments below `agent:exchange` only when the comment body duplicates or near-duplicates an exchange prompt, including already-answered prompt residue after boundary repositioning, while preserving unrelated scratch comments outside the exchange. Added focused write-path regressions and updated the closeout spec.

- **Starting actor route timeouts now coalesce per generation.** When repeated editor reroutes hit the same authoritative actor pane while that current generation is still booting, route now records one typed `route_authoritative_actor_starting_not_ready` timeout for that pane/generation and logs later retries as coalesced waits until the actor reaches ready, closed, or blocked. Added route-state regressions plus SimWorld coverage for the repeated-starting-timeout schedule. This closes `#rtbr` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Supervisor PTY filter diagnostics no longer print into managed prompts.** Child-output Kitty keyboard-mode preserve/drop traces are now opt-in via `AGENT_DOC_TMUX_INPUT_DIAG` / `AGENT_DOC_DEBUG_STDIN`, so normal Claude Code prompt editing and history search do not show `[agent-doc] tmux_input_event source=supervisor.pty_filter ...` lines in the managed pane. Route, queue, supervisor IPC, auto-trigger, tmux submit, and permission-prompt input diagnostics remain available at their normal input boundaries.

- **Drained auto queues now clean up already-completed residue on preflight.** If an `agent:queue auto` block has no remaining prompt entries because every item was already marked complete, preflight now clears the queue body, removes `auto`, syncs the snapshot, and leaves `queue_active: false` instead of preserving a completed queue run for later cycles.

- **Direct active-queue runs are explicitly single-step resumable.** Bare-path / `run` invocations now synthesize the active queue head when `queue_active: true` and the document has no diff, consume one queue prompt before strict closeout, and print a continuation diagnostic when an `agent:queue auto` block still has prompts remaining. Re-running the same command advances the next prompt instead of silently no-oping.

- **Prompt-prefix normalization now preserves committed ownership state through fallback closeouts.** The write path preserves HEAD prefix state when rebuilding `content_ours`, repairs stripped prompt prefixes in sidecar/content_ours fallback paths, and covers bare final prompt repair after merge/adoption. Focused regressions cover committed assistant lines staying unprefixed, committed user prompts staying prefixed, IPC sidecars that strip `❯ `, and final bare prompt repair. This closes `#pfxleak2`, `#bppfxstrip2`, and `#lastpfx` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Active queue prompts no longer get hidden behind empty document diffs.** When `queue_active: true` and the document matches its snapshot, `preflight` and `plan` now synthesize the queue head item as the prompt diff, so `agent-doc <FILE>` opens a real cycle instead of returning `no_changes=true`. Added regressions for the `#oobpmt` queue-resume shape and updated the git integration spec.

- **Preflight now warns on harness/document mismatch.** `agent-doc preflight` compares frontmatter `agent:` against the active Claude Code, Codex, or OpenCode harness, emits a structured `harness_mismatch` warning without blocking intentional handoffs, and the skill contract tells harnesses to surface it while keeping active-harness attribution and closeout behavior.

- **Direct template writes now strip safe progress chatter before exchange patchbacks.** When a direct `agent-doc <FILE>` / write closeout receives plain progress commentary followed by a valid `patch:exchange`, the write path now reuses the replay guard and applies only the sanitized patch body. Trailing, interstitial, transcript-shaped, or full-document unmatched content still fails closed instead of being appended into `agent:exchange`. This closes `#rspdigest` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **CRDT live prompt prefix-variant duplicates are repaired before closeout.** When live typing races with IPC/CRDT writes and leaves adjacent prompt lines where an earlier partial line is a prefix of the completed prompt, the write path now repairs only the live prompt tail after the last exchange boundary, preferring the longer line and leaving assistant prose untouched. IPC snapshot dedupe uses the same repair before saving or redelivering editor content. Added regressions for the observed OpenCode arrow-key prompt duplication shape and documented the plan in `tasks/agent-doc/plan-crdt-live-prompt-prefix-duplicate.md`.

- **CRDT closeout now fails closed on duplicate scaffold mixed with live user text.** Template normalization now runs the duplicate-scaffold repair path when CRDT/write merging creates a second `<!-- /agent:exchange -->` close marker with copied queue/backlog/done scaffold. Pure duplicated scaffold is repaired, but mixed scaffold plus live user text is rejected instead of being committed or silently dropping text. Added regressions for the observed `agent:exchange` live-typing corruption shape and documented the plan in `tasks/agent-doc/plan-crdt-duplicate-scaffold-closeout.md`.

- **Claude skill auto-update no longer defaults to context compaction.** Rendered Claude and shared instruction surfaces now use `agent-doc skill install --harness claude --reload restart` by default and reserve `--reload compact` plus `/compact` prompting for sessions that explicitly opt into `agent_doc_auto_compact` in frontmatter or project `.agent-doc/config.toml`. Updated the harness runbook and closeout docs so large-session/session-accretion signals stay advisory instead of triggering an implicit Claude compaction path. This addresses the latest `root.md` auto-compaction report in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Clear Session Context now ignores Codex dim placeholder text.** Protected-input detection for `agent-doc session clear <FILE>` now captures ANSI pane state and treats Codex faint placeholder text as idle chrome, so JetBrains clear no longer refuses with `reason=drafted_prompt_input` when the live pane only shows placeholder/status UI such as `gpt-5.5 high ... Context ... used`. Real non-dim typed prompt input, queued drafts, and shell search still fail closed and point to `session interrupt-clear`. This addresses the latest JetBrains Clear Session Context blocker in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Live-typing duplicate prompt repair now happens before IPC snapshots commit.** Socket and file IPC closeouts compare the post-apply exchange against the pre-write exchange and remove any extra copy of an already-present user prompt line, preferring the normalized `❯` prompt form. The write path logs `ipc_prompt_duplicate_repaired` before saving the snapshot, and `session clear` can now proceed on an unprotected live pane even when stale startup projection and fresh user prompt drift coexist. A named SimWorld regression now covers the JetBrains Clear Session Context sequence: stale starting actor clear, prompt-only document drift, dispatch-only reroute blocked until ready, and duplicate prompt repair before commit. This addresses the latest JetBrains `/clear` plus duplicate prompt report in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Answered closeout markers no longer keep committed cycles open.** `session-check` now treats active-session post-commit drift as closed when there is no unresolved prompt marker and the remaining differences are confined to answered exchange metadata (`❯`, `(HEAD)`, boundary ids) plus backlog metadata. Unrelated status/body edits still fail closed. Added regressions for the unfinished `agent-doc-bugs2` closeout shape and documented the follow-up plan in `tasks/agent-doc/plan-finish-closeout-after-answered-marker-drift.md`.

- **Sidecar normalization fallback now has direct repair diagnostics coverage.** A regression recreates the `#normfallback` shape from `tasks/agent-doc/agent-doc-bugs2.md`: the plugin ack-content sidecar strips a required prompt prefix, the binary rejects that primary snapshot with `reason=prefix_divergence`, repairs the snapshot and working tree from the normalized fallback, and records the `sidecar_normalization_fallback_repaired_working_tree` ops-log marker required by the closeout spec.

- **Stale preflight repair now has direct stale-checkpoint race coverage.** A regression now binds a partial-response checkpoint writer to an open `preflight_started` cycle, lets repair abandon that stale prompt-bearing cycle, and proves the original writer stops with `partial_response_checkpoint_stopped` instead of writing another checkpoint for the abandoned cycle. The backend spec names stale-preflight abandonment as part of the checkpoint stop contract for `#staleckpt` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Starting actor route waits now have deterministic prompt-barrier coverage.** The route wait decision is factored into a pure poll classifier and covered for the `starting -> busy -> ready` schedule: dispatch remains blocked through restart-bootstrap `busy` and through `ready` without prompt proof, then releases only when ready state, dispatch-ready prompt proof, and dispatch eligibility agree. The route spec now names that conjunctive gate for `#startroute` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Pending-only empty write closeouts cover completed items.** `write --commit` with empty stdin now has regression coverage for `--done` as well as `--pending-add`: it reaps and archives the completed item, commits the document, leaves the exchange untouched, and passes `session-check`. The closeout spec now names both add-only and done-only pending mutation shapes for the `#writeempty` contract in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Terminal closeout lifecycle updates are idempotent.** Repeated repair/replay/no-op bookkeeping after a cycle is already `Committed` no longer rewrites cycle-state, refreshes committed capture timestamps, or re-emits `capture_committed_after_replay`; late fallback rejection diagnostics now include the patch id. `agent-doc ops summary` also separates `commit_noop drift_kind=none` and protected-input clear refusals into expected-behavior buckets so routine no-op closeouts and fail-closed clear guards do not read as actionable bugs. This closes `#sp3q` and `#s8cs` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Agent-doc managed-cycle cleanup now avoids stale closeout work.** Orchestrate wraps clean plain template-mode child responses as explicit `patch:exchange` closeouts, avoiding the zero-template-patch write path; partial-response checkpoint writers stop once their original cycle commits, is abandoned, or is replaced; route-owned reap diagnostics now report `post_commit_user_follow_up` when the remaining dirty document content is a new user prompt; safe-passive sync uses live local actor projections before controller actor lookup; and repeated managed network child proof in one process reuses a same-command/args/environment success. This closes `#ds58`, `#djwb`, `#m2hx`, `#ha62`, and `#aymr` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Codex non-streaming patchback now filters progress chatter.** Direct Codex child response capture now selects the last `item.completed` `agent_message` before `turn.completed` as the durable response body instead of concatenating every assistant message from the JSONL stream. Multiple assistant messages without a final turn boundary now fail closed as ambiguous, preventing progress/status prose from being committed into template session documents. This closes `#codexverbosepb` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Codex required-SSH drift detection now requires live SSH evidence.** Command-execution parsing no longer treats arbitrary command output that merely mentions a required host plus an old `socket: Operation not permitted` as active SSH capability loss, so searches through `.agent-doc/captures` or logs cannot abort a resumed Codex run. Actual SSH commands still fail closed on bare EPERM output, and failure details now include the command string. This closes `#sshcapfalsecapture` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Codex hook-review route blockers now include recovery guidance.** Dispatch-only reroutes that see `route_dispatch_only_blocked reason=codex hook review prompt` now tell the operator to open `/hooks`, approve or disable the pending hook change, wait for the idle composer, and rerun the route/editor action instead of falling back to a generic idle-prompt hint. Updated route specs and regression coverage. This closes `#hookreviewroute` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Ops summaries now separate expected follow-up noise from anomalous drift.** `agent-doc ops summary` now buckets benign `post_commit_user_follow_up`, `post_commit_local_drift kind=user_follow_up`, and `commit_noop drift_kind=user_follow_up` events separately from working-tree drift/noop diagnostics. No-op closeouts now log their drift kind in `ops.log`, making routine user follow-up reruns distinguishable from real post-commit local edits. This closes `#opsnoisereduce` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Interrupt-clear timeouts now preserve final blocker evidence.** `agent-doc session interrupt-clear <FILE>` timeout logs and user-facing errors now report the final live-pane state, evidence source, prompt-ready value, current command, and recent pane tail after the protected clear discard path, instead of reducing the result to `outcome=timed_out` plus a loose last command. This closes `#interruptcleartimeout` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Backlog-section prompt patchback cleanup.** Template and CRDT closeout now remove newly-added raw prompt-target lines from `agent:backlog` / legacy `agent:pending` after the response is merged into `agent:exchange`, while preserving normal tracked backlog edits and pending state changes. This closes `#backlogorphan` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Sequential orchestration parent closeout now survives stale binary paths.** Parent-owned lifecycle commands in `agent-doc orchestrate --mode sequential --from-exchange` now resolve a launchable `agent-doc` binary before spawning `preflight`, `finalize`, or `session-check`, falling back when `current_exe()` points at a binary removed during local install work. Spawn failures include binary, cwd, and PATH-presence context, and regressions cover sanitized PATH and stale-current-exe resolution. This closes `#synchorchstop` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Sequential orchestration now freezes exchange task lists.** `agent-doc orchestrate --mode sequential --from-exchange` records the source markdown task list at parent start and rechecks it after each child closeout. If the live list is edited mid-run, the parent writes a deterministic interruption response, leaves remaining and newly added tasks open for the next explicit run, and exits before launching the next step instead of hanging. This closes `#orchmidrun` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Interrupt-and-clear now recovers Vim/Neovim prompts.** The explicit `agent-doc session interrupt-clear <FILE>` discard path now watches the managed pane after sending harness interrupt keys. If the interrupt opens Vim/Neovim, it sends one forced `:qa!` recovery before continuing the idle/closed wait; if the pane still does not settle, the timeout names the last observed command and gives an exact manual recovery action. Editor specs now keep that recovery in the binary-owned path. This closes `#clearinterruptvim` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Supervisor-to-tmux input now has raw end-to-end coverage.** The live tmux suite now includes a supervisor IPC test that drives the real tmux pane input path into a raw harness process and asserts the submitted prompt text, Enter delivery, arrow-key escape sequences, and final Enter bytes. This closes `#tmuxe2etests` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Tmux input paths now emit structured diagnostics.** Route, queue dispatch, supervisor IPC/auto-trigger injection, harness-aware tmux submits, stdin forwarding transforms, Kitty keyboard-mode preserve/drop decisions, and OpenCode permission-prompt key translations now emit `tmux_input_event` lines with source, destination, transform, key, byte count, and harness where known. Prompt text is represented by length plus SHA-256, giving regressions stable log assertions without leaking raw typed content. This closes `#opencodeinputdiag` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Route-owned reap no longer preserves panes for stale renderer tails.** The route-owned completion guard now trusts the supervisor actor's `ready` prompt state when deciding whether a committed one-shot pane can be reaped, while still preserving panes for explicit blocking prompt states such as queued drafts, permission prompts, hook-review prompts, history search, and clean-exit restart prompts. Managed PTY filtering also strips OSC title updates so transient title text such as `Working ... esc to interrupt` cannot enter prompt sampling. This closes `#ownedreapbusy` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Clear/restart now guard starting owned panes before tmux input.** Session operator clear/restart no longer trust controller acceptance alone while the actor record, matching or legacy session-scoped supervisor runtime, or matching supervisor lease still says `starting`. Clear now requires a dispatch-ready composer and a clean post-commit document hash before submitting `/clear`; restart allows either dispatch-ready composer evidence or the clean-exit restart prompt, but also fails closed on post-commit document drift. Refusals log `session_operator_starting_guard_refused`. This closes `#clearstartingrace` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Orchestrated template closeout now accepts clean plain child responses.** `write` no longer requires an explicit `patch:exchange` block for orchestrate-origin template responses when the child returned a single clean assistant body; the existing unmatched-content synthesis appends it to `agent:exchange`. Patch-bearing orchestrate responses still require `patch:exchange`, mixed patch/unmatched output still fails, and transcript-shaped, full-document, or multiple-response dumps are rejected before write. Updated orchestration and closeout specs with focused write-path regressions. This closes `#orchplainresp` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Late IPC fallback writers now stop at committed cycle state.** The write path now distinguishes a committed-cycle IPC skip from a consumed IPC patch, cleans stale fallback patch JSON with a claimed-patch sentinel, and avoids logging `ipc_write_consumed` / re-running already-current closeout work for a terminal cycle. Added regression coverage and documented the terminal IPC cycle guard. This closes `#latefallbackloop` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Direct pane submit telemetry no longer reports proven Codex reroutes as false timeouts.** Route now records direct tmux input acceptance latency separately from the later harness dispatch-start proof, waits to classify the direct-submit outcome until proof is known, and budgets the direct pane submit path around the full tmux/control-mode acceptance window plus capture-poll slack. If Codex proves the routed prompt was consumed after pane-input acceptance was not directly observable, ops logs now say `acceptance_unobserved_dispatch_proven` instead of `timed_out` / `over_budget`. Updated route regressions and session tmux specs. This closes `#directsubmitbudget` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Starting actor reroutes now refresh terminal lifecycle states immediately.** While route is waiting for a `starting` authoritative actor to become dispatch-ready, a supervisor refresh to `closed` or `blocked` now stops the wait and surfaces that terminal actor state instead of burning the startup-ready timeout and reporting stale `starting` state. Updated route specs and added SimWorld plus tmux-backed route coverage. This closes `#startreadytimeout` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **OpenCode live-pane submits now send real Return instead of newline.** Harness-aware tmux submissions use OpenCode's Kitty keyboard Return sequence for routed reopens, supervisor IPC injects, auto-triggers, and file-scoped `/clear`, so OpenCode panes whose TUI keymap distinguishes `return` from `ctrl+j` submit the prompt instead of inserting a blank line. Updated the session tmux spec and tmux-router coverage.

- **Completed work can now live in an explicit external done archive.** `agent:done archive=<repo-relative>.done.md` appends reaped backlog/icebox entries to the named markdown file instead of growing the session document, creates the archive when missing, rejects unsafe paths, suppresses duplicate retry entries, and lets preflight/session-check use archived IDs as dropped-history proof. Updated pending specs and runbook guidance. This closes `#donearchiveattr` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Clear Session Context no longer blocks ordinary active/status panes.** File-scoped `agent-doc session clear <FILE>` is treated as an explicit operator action again: direct `alive-busy` evidence alone no longer fails closed, so JetBrains/VS Code Clear Session Context does not get stuck behind Codex status/footer panes such as `gpt-5.5 high ... Context 60% used`. The remaining clear guard is scoped to protected prompt-input states such as permission prompts, queued drafts, shell search, or drafted user input; those refusals record `session_clear_protected_input_guard_refused` and point operators to `agent-doc session interrupt-clear <FILE>` for an intentional discard. This closes the latest Clear Session Context repro in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Editor sync guards no longer stay wedged after a dead-pane sync stalls.** JetBrains and VS Code now bound plugin-spawned layout-sync subprocesses. If one stalls while the binary is dealing with killed or stale tmux panes, the plugin terminates that subprocess, releases its local sync guard, and leaves the latest selection pending so a retry can run the binary recovery path instead of permanently showing `Sync deferred: another tmux layout sync is already running`. Updated shared editor specs and bumped local plugin builds to JetBrains `0.2.118` and VS Code `0.2.17`.

- **Managed OpenCode permission arrows no longer leak escape text.** While the supervisor sees an active OpenCode `Allow once` / `Allow always` / `Reject` permission prompt, legacy arrow-key escape sequences from stdin are translated to the prompt footer's Tab/BackTab selector controls before they reach OpenCode. Normal OpenCode prompt editing remains unchanged, and the regression covers the `^[[C` / `^[[D` leak shape from a live permission dialog.

- **Editor prompt answers now run from the owning session cwd.** `agent-doc prompt --all` entries include `cwd`, and the JetBrains/VS Code prompt UIs use that root when calling `prompt --answer` instead of assuming the current IDE workspace root. Failed answer submissions now clear the temporary suppression key so the still-active prompt can reappear. Added process-level JetBrains coverage for the prompt-answer command cwd and a live tmux integration regression proving OpenCode answers send Tab rather than a raw left/right arrow escape. Bumped local plugin builds to JetBrains `0.2.117` and VS Code `0.2.16`.

- **Editor prompt answers now use the `prompt --answer` positional contract.** JetBrains and VS Code prompt UIs accept flat `agent-doc prompt --all` entries with `selected`, keep the selected state in their prompt item model, and send the selected option's one-based position to `agent-doc prompt --answer` instead of forwarding the displayed TUI option number. Bumped the local-testing plugin builds to JetBrains `0.2.116` and VS Code `0.2.15`.

- **OpenCode permission prompt answers now use the actual TUI selector state.** `agent-doc prompt --answer` now captures OpenCode panes with ANSI attributes before parsing, so it can read the highlighted `Allow once` / `Allow always` / `Reject` option instead of falling back to option 0. OpenCode automation now moves with the prompt footer's Tab/BackTab selector contract rather than arrow keys, matching the live failure evidence where arrows leaked into the prompt as literal `^[[C` / `^[[D` text.

- **OpenCode permission prompts now preserve keyboard negotiation.** The OpenCode supervisor preserves OpenTUI's Kitty keyboard-mode sequences instead of stripping them with terminal query noise. The prompt-answer path relies on the prompt footer's Tab/BackTab selector contract and still accepts the `Allow always` follow-up confirmation prompt.

- **OpenCode dispatch-only startup probes now use the OpenCode redraw budget.** JetBrains `Run Agent Doc` can hit an OpenCode pane just after the controller has seen the idle splash but before the second startup-window guard catches the same prompt. Dispatch-only routing now gives OpenCode the longer harness-specific prompt/recovery budget instead of the short Codex-style boot probe, avoiding false `latest run is still booting` refusals after OpenCode is already accepting input.

- **OpenCode idle splash now promotes managed sessions to ready.** OpenCode 1.14 can render an idle composer as the splash chrome (`Ask anything...`, build-plan text, command/footer hints, cwd/version status) without a standalone `>` prompt or `context ... % used` footer. Shared harness readiness now treats that chrome-only splash as dispatch-ready, so start, route, and session status promote the actor instead of timing out with `route_authoritative_actor_starting_not_ready` after the capability proof succeeds.

- **Managed capability proof results now use tmux status messages.** Successful and failed Codex/OpenCode/Claude managed proof diagnostics still go to the session log, but `start` now surfaces the user-visible `[start] managed ... capability proof` line with `tmux display-message` targeted at the owned pane instead of writing it into the agent pane transcript. This keeps proof diagnostics from interfering with TUI prompt detection or the next agent input.

- **OpenCode proof output no longer strands startup in `starting`.** OpenCode prompt readiness now ignores supervisor capability-proof diagnostics and treats an otherwise chrome-only `context ... % used` footer as an idle composer. That lets route/start promote a proven OpenCode actor to `ready` and dispatch the trigger instead of timing out with `route_authoritative_actor_starting_not_ready` after `opencode_capability_proof status=proven`.

- **Strict closeout now reports slow commit phases and fails explicitly on stale parent gitlinks.** `finalize` / strict `write --commit` record a `closeout_latency` diagnostic when response durability crosses the closeout budget, with per-phase timings for commit retries, cycle-state checks, session-check, and cleanup. Submodule-hosted documents now fail closed after the bounded parent-pointer retry if the parent `HEAD:<submodule>` still differs from the submodule `HEAD`, naming `agent-doc commit <FILE>` as the idempotent recovery. This closes `#rspcmtdelay` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **JetBrains Run Agent Doc now forces the plain reopen prompt.** The JetBrains action calls `agent-doc route --dispatch-only --plain-trigger`, and route applies that flag by sending `agent-doc <FILE>` even when the document's normal harness trigger template is slash-command based. This keeps editor reruns from injecting `/agent-doc ...` into sessions such as `root.md` where the IDE action must send the plain Codex-compatible form. Bumped the JetBrains plugin build version to `0.2.114`.

- **Cross-harness JetBrains reruns can replace stale actor records.** Route now treats a stored harness mismatch as authoritative only when the old actor still has a healthy live supervisor and a non-closed state. Dead panes, closed actors, and unreachable supervisor records fall through to fresh start/rebind, so running JetBrains `Run Agent Doc` in Claude after closing a Codex session no longer fails on `bound to harness codex, not claude-code`. Updated route specs and added focused coverage for the live-vs-stale mismatch guard.

- **OpenCode managed sessions now prove required SSH before dispatch.** OpenCode startup now records `opencode_capability_proof` for SSH-gated documents, runs a bounded `opencode run --format json` child probe with isolated SSH options, and blocks auto-trigger, supervisor injection, managed route, and dispatch-only route until the current proof succeeds. OpenCode child probe failures such as `socket: Operation not permitted` now fail closed as managed-pane SSH capability denial instead of letting the agent discover the sandbox error mid-response. Route and session-status proof checks are harness-aware so OpenCode is not incorrectly held to Codex writable-root contracts. This closes `#opencodecapfail` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Post-commit follow-up prompts no longer look like missed patchback repair.** When `commit` sees `snapshot == HEAD` and the live file only adds a later user follow-up, it now logs a dedicated `post_commit_user_follow_up` marker and suppresses `prior_patchback_without_response_body` / `out_of_band_write` noise. The follow-up still remains uncommitted for the next response cycle, but ops diagnostics no longer imply a missing assistant response body. This closes `#codexpatchbodyloop` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **IPC timeout closeout deletes stale fallback patches.** The CRDT stream IPC timeout path now removes the queued `.agent-doc/patches/<hash>.json` file after its local write and git commit succeed, while still leaving the claimed-patch sentinel for any watcher that already observed the file. This prevents a late editor file-watcher pass from replaying the same response after the binary has already committed it. Added a child-process regression for the exit-75 timeout path. This closes `#ipc-timeout-dup` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Clear Session Context direct-pane delivery recognizes Codex idle placeholders.** File-scoped `agent-doc session clear <FILE>` uses the resolved direct pane or supervisor path after controller authorization and idle proof. Codex status now also recognizes the current `› Explain this codebase` idle placeholder as prompt-ready evidence. This closes the follow-up JetBrains Clear Session Context repro in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Clear Session Context no longer treats Codex status-only panes as busy.** File-scoped `session status` and `session clear` now classify Codex panes that show only model/cwd/context status chrome, with no prompt input or busy cue, as direct idle evidence. That lets operator clear override stale actor/supervisor busy projection while keeping route dispatch gated on a real dispatch-ready prompt. JetBrains also drops the unused response-status busy FFI surface, documents that Clear Session Context must always ask the binary instead of blocking on plugin-local busy state, and bumps the JetBrains plugin build version to `0.2.113`. This closes the latest JetBrains Clear Session Context stale-busy repro in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Dispatch-only Codex proof gating now explicitly covers ready actor reroutes.** The hook-visible Codex accepted-but-unproven guard already lives in the shared dispatch-only submit helper, so ready authoritative actors and startup-window reroutes both fail closed when pane acceptance never becomes routed submission proof. Added a non-tmux regression for the accepted-only gate and clarified the README/session specs. This closes `#4w5x` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **JetBrains Clear Session Context now recognizes wrapped protected-busy failures.** The plugin parser accepts the exact `agent-doc command failed (exit 1): Error: session_clear refused ... alive-busy` notification shape and less predictable pane-tail text, so the IDE shows the typed running-session warning with Refresh/Interrupt/Status/Copy actions instead of falling back to the generic command-failed error. Bumped the JetBrains plugin build version to `0.2.112`.

- **Base-index layout repair now runs during the active preflight.** When the pre-diff layout check finds the current tmux session missing window index `0`, preflight now removes the stale deferred-repair counter, runs `repair_layout` immediately, and rechecks layout before emitting JSON. If automatic repair cannot run, stderr names the explicit `agent-doc session doctor <FILE> --repair` action instead of silently waiting for a second detection. This closes `#baseindexrepair` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Dispatch-only proof scope is explicit across harnesses.** `route --dispatch-only` now logs both `proof` and `proof_scope` so Claude Code and OpenCode accepted pane delivery is labeled as accepted-only instead of being mistaken for Codex-style consumed/submitted dispatch-start proof. Codex keeps its hook-backed dispatch-start proof behavior when hooks are visible. Added route regressions and updated the session tmux spec. This closes `#clauderouteproof` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **JetBrains real markdown navigation now always validates the actor/supervisor path.** A true `selectionChanged` event still runs the guarded background `sync --no-autostart` reconciliation even when the visible/focused signature was already marked synchronized. The immediate focus fast path remains best-effort for existing panes, while the background sync owns the safe cold-start when a document like `tasks/software/corky.md` has no actor, preventing later `Clear Session Context` from surfacing `stage=missing_actor`. Bumped the JetBrains plugin build version to `0.2.110`.

- **JetBrains Clear Session Context now surfaces protected busy panes as a typed running-session result.** The CLI still fails closed when direct live-pane evidence says the pane is `alive-busy`, but the JetBrains plugin now parses that refusal and shows a warning with the pane id, current command, and latest pane tail plus Retry clear, Show status, and Copy details actions instead of a raw `agent-doc command failed` notification. Updated editor specs and added parser/message regressions.

- **Stale prompt-bearing preflight cycles are abandoned, not placeholder-closed.** If a pane dies after `preflight_started` before any response capture exists, and the live document still has an unresolved prompt target, `repair` now abandons the stale empty cycle after the bounded timeout instead of forcing a manual placeholder response. The prompt remains in the working document, so the next `preflight` opens a fresh cycle and handles it normally; recent empty cycles still fail closed to avoid stealing a live concurrent turn. Added cycle-state, repair, and preflight regressions. This closes `#preflight-started-recovery` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Prompt+response exchange drift now fails closed.** `session-check` now treats an uncommitted appended exchange chunk containing both a user prompt and a new assistant `### Re:` / `## Assistant` marker as uncommitted response drift instead of ignoring it as prompt-bearing local drift. Prompt-only tails still route through the prompt-tail guard. Added a regression for the SessionShare root `#rspcmt7` shape where the visible response closeout landed in `tasks/root.md` but the owning repo stayed dirty.

- **Clear Session Context now works after a closed actor generation.** The project controller still rejects `blocked` actors and non-clear commands for `closed` actors, but an explicit `session_clear` operator command now records an `operator_closed` acceptance so the CLI/editor can send `/clear` to the live harness context before the next run. Added a controller regression and updated the session tmux command spec. This closes the latest JetBrains `Clear Session Context` closed-generation repro in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Harness-agnostic uncommitted exchange drift detection.** `session_check.rs` now detects when a committed cycle has exchange-content changes in the working tree that differ from the committed snapshot, regardless of which harness (Codex, OpenCode, Claude Code) owns the session. Previously `detect_active_session_post_commit_drift` required Codex session tracking (`CODEX_THREAD_ID`) and silently returned `None` for all other harnesses, allowing uncommitted responses to pass all guards. The new `detect_uncommitted_exchange_drift` function checks snapshot vs working tree directly and fires as a fallback in all three committed-cycle branches. Added regression tests proving the guard catches exchange drift without Codex session state and does not fire for status-only drift. This closes `#rspcmt6` in `tasks/agent-doc/agent-doc-bugs2.md` and extends `tasks/agent-doc/plan-response-patchback-uncommitted.md` with the harness-agnostic drift evidence.

- **OpenCode CLI-only-output anti-pattern.** The OpenCode section of `runbooks/harness-invocation.md` now explicitly names the anti-pattern of outputting a response to the CLI without piping it through `agent-doc finalize` — response text visible in the console but absent from the session document is the same closeout violation as skipping finalize entirely. The shared Hot Path Digest in `SKILL.md` reinforces that the response does not exist until it crosses `finalize` or `write --commit`. Added regression tests proving session-check catches an OpenCode prompt-only exchange tail and the runbook section names the anti-pattern. This closes `#noexchopencode2` in `tasks/agent-doc/agent-doc-bugs2.md` and follows `tasks/agent-doc/plan-opencode-no-exchange-patchback.md`.

- **Direct-chat preset write-back invariant.** When a session-document preset (for example `#commit-push`) triggers repo work through a direct Codex chat turn, the turn is not complete until the response is written back with `agent-doc write --commit <FILE>` and `agent-doc session-check <FILE>` passes. The `harness-invocation.md` runbook now explicitly states this invariant. Added a regression test proving session-check catches a prompt-only exchange tail when a direct-chat preset completes repo work but writes no response patchback. This closes `#rspcmt5` in `tasks/agent-doc/agent-doc-bugs2.md` and extends `tasks/agent-doc/plan-response-patchback-uncommitted.md` with the direct-chat closeout invariant.

- **OpenCode direct-exec session-check guard.** The OpenCode harness runbook now requires `agent-doc session-check <FILE>` immediately after `finalize` and after manual `write --commit`, matching the existing Codex fail-closed contract. `runbooks/commit.md` and `README.md` now name both Codex and OpenCode for the direct-exec post-write guard. `session_check.rs` error messages no longer reference "the active Codex session" or "the Stop hook" exclusively — they use harness-agnostic language. This closes `#rspcmt4` in `tasks/agent-doc/agent-doc-bugs2.md` and extends `tasks/agent-doc/plan-response-patchback-uncommitted.md` with OpenCode-specific closeout evidence.

- **Closeout and starting-actor diagnostics now name the next command.** `agent-doc commit <FILE>` no longer lets the "already committed" no-op message sound like a full closeout when later user follow-up prompts remain; it now says to rerun `agent-doc <FILE>` or use `agent-doc write --commit <FILE>` for a missing response. Route's `starting` authoritative-actor failure now says to wait and rerun, and names `agent-doc start <FILE>` for stuck-owner recovery. This closes `#rspcmt3` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **OpenCode harness support.** `agent: opencode` now resolves to an OpenCode managed pane with `agent-doc <file>` trigger routing, `opencode_model` / `opencode_args` frontmatter and config aliases, and a minimal non-streaming `agent-doc run --agent opencode` backend that invokes `opencode run`. This supports OpenCode model IDs such as `zai/glm-5` via the same `--model` injection path.

- **`#agent-doc-bug` declaration chains now preserve backlog order.** `agent-doc plan` now expands multiple prompt-bearing `#agent-doc-bug` declarations into ordered expected add mutations for explicit backlog targets, and logs the declaration/final insertion order for multi-item batches. The first declared bug remains above later bugs unless the response explicitly documents an intentional priority override. This closes `#bugchainorder` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Standalone boundary setup no longer advances the commit snapshot.** `agent-doc boundary` still writes a transient marker into the working document and signals the editor, but it no longer updates the saved snapshot. That prevents the next preflight/commit from turning marker-only setup churn into a noisy boundary-only git commit.

- **Route no longer dispatches into `starting` authoritative actors.** Managed and dispatch-only reroutes now wait for a `starting` controller actor to refresh to `ready` before recording a dispatch attempt or sending tmux/supervisor input; if the actor stays `starting`, route fails closed with a state-gate diagnostic instead of creating an interrupted startup cycle. `busy` actors remain eligible for one supervisor-owned queued reopen. Added route regressions and updated the routing/session-actor specs. This closes `#startingdispatch` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Stale `starting` actor cleanup no longer trusts a live PID forever.** Normal `preflight`, `start`, `sync`, and `gc` cleanup now keep a one-hour-old `starting` actor only when the recorded supervisor PID is alive and its lease heartbeat is still fresh. A stuck `agent-doc start --route-owned` process with an old heartbeat is closed and projected from SQLite on the next normal cleanup pass. Added a regression for the live-PID/stale-heartbeat case and updated the session actor specs. This closes `#startgcleak` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Direct `agent-doc run` now stops when pre-commit repair consumes the whole diff.** If the initial diff only reflected an already-committed missed patchback and the pre-commit repair brings the snapshot back to `HEAD`, `run` rechecks the diff and fails before child-agent dispatch with an `agent-doc write --commit <FILE>` recovery hint. Added an integration regression proving a configured child agent is not invoked. This closes `#emptyrsprepair` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Agent-owned partial patchbacks can be adopted from empty strict repair writes.** `agent-doc repair` now adopts already-visible responses from interrupted `response_captured` / `write_applied` cycles even when no pending response artifact remains, and strict `write --commit` with empty stdin runs that adoption path before failing as an empty response. This closes `#partialpatchbackadopt` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Blocked-stop repair now replays guard-prefixed patch payloads.** The shared replay guard now accepts known closeout guard comments such as `<!-- no-pending-capture -->` around otherwise valid patch responses, while still blocking transcript/full-document dumps. `agent-doc repair` now writes the sanitized replayable payload returned by the guard, so patch bodies extracted from leading progress commentary are actually used instead of only classified. Added replay guard, repair, and Codex Stop-hook regressions. This closes `#blockedstopextract` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Stale `starting` actor cleanup now runs on normal paths, not just daily GC.** `preflight`, `start`, and `sync` now run the lightweight controller actor cleanup every cycle, closing one-hour-old `starting` records when no fresh supervisor heartbeat or live supervisor PID proves that generation is still booting. The full orphan-file GC remains on the `.agent-doc/gc.stamp` daily cadence. Added regressions for preflight with a fresh GC stamp and caller-specific actor transitions. This closes `#autogcstart` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Editor IPC prefix repair now repositions before normalization.** JetBrains and VS Code patch application now move the exchange boundary before applying `normalize_prefix_lines`, so prompts typed after the previous boundary marker are inside the user region seen by the ack-content sidecar. This should keep clean closeouts from repeatedly logging `sidecar_normalization_fallback reason=prefix_divergence`. Added editor regressions and updated the plugin IPC spec. This closes `#sidecarfallbackstill` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Tracked-work completion now uses `--done`.** `write` and `finalize` now expose `--done <id>` as the public flag for marking either `agent:backlog` or `agent:icebox` work complete. The old `--pending-done` spelling and the transitional `--backlog-done` spelling are accepted as deprecated aliases with warnings, while `plan` and recovery hints now emit `--done`. This closes the CLI rename request in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Prompt-only exchange tails now fail closed after closeout.** `session-check` now scans the live `agent:exchange` tail after otherwise-clean closed cycles and interrupts when it ends in a prompt-looking block with no later assistant response, even if that prompt already matches the committed snapshot. This catches direct Codex/manual turns like the May 10 `#vt-agent-deploy` patchback miss where implementation commits succeeded but the final response never landed in the session document. This closes `#rootpatchmiss` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **BREAKING CHANGE: completed backlog archives now use `agent:done`.** The completed/reaped archive component was renamed from `agent:backlog-done` to `agent:done`, and `agent:backlog-done` / `agent:pending-done` are no longer accepted as archive aliases by closeout, history replay, or pending resolution. `agent-doc migrate` rewrites both legacy tags to `agent:done`, and newly reaped items create or append to `agent:done`. This closes the follow-up archive rename request in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Cross-document backlog capture now has a binary-owned target path.** `write` and `finalize` accept `--pending-add-to <file> <text>` for explicit backlog targets, fail closed when the target file is missing or lacks a backlog component, and `plan` now surfaces those target files in `pending_mutations` / finalize hints. Closeout guards no longer let a current-document `--pending-add` bypass explicit target validation, preventing `#agent-doc-bug` items from landing in the wrong session document. This closes `#crossdocpend` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Prompt-prefix normalization now uses opt-in response-block exits.** The `content_ours` normalizer no longer leaves an inserted assistant response block just because a response sentence looks prompt-like, and target-based prefix repair must match an explicit `normalize_prefix_lines` target before it can resume after a `### Re:` block. This keeps assistant questions and preset-looking evidence lines bare while still repairing real follow-up prompts after a boundary or canonical prompt-target diff. This closes `#spfxnorm` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Direct `agent-doc run` waits now emit and persist heartbeats.** After preflight opens the response cycle, long non-streaming child-agent waits print `[run] heartbeat ...` progress every `AGENT_DOC_RUN_HEARTBEAT_SECS` seconds (default 30) and update the open cycle state's `updated_at` / `last_event` without advancing the phase. Timeout diagnostics still replace the heartbeat with the recoverable timeout event, but operators and Codex can now see phase/cycle progress while the child is legitimately still running. This closes `#runhb` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Compact Exchange now uses editor IPC before falling back to disk writes.** `agent-doc compact <file> --component exchange --commit` delivers its full-document replacement through the existing JetBrains/VS Code IPC watcher when available, so the active markdown buffer is mutated through the editor document API instead of triggering an external-file-change dialog. Added compact IPC regression coverage and refreshed the shared editor specs.

- **Sync layout memory now lives in the project controller store.** `agent-doc sync` imports legacy `.agent-doc/last_layout.json` once when `.agent-doc/state.db` has no layout row, then reads and writes the controller-backed `layout_states` table as the authoritative column-memory state. `last_layout.json` is still emitted for compatibility, but drifted JSON no longer overrides SQLite. This closes `#stateproj` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Submodule closeout now fails closed on stale parent gitlinks.** Strict `finalize` / `write --commit` and `session-check` now verify that a submodule-hosted document response is committed both in the submodule and through the parent repository submodule pointer. If the inner document commit succeeds but the parent pointer commit fails, closeout reports the missing parent layer and prescribes idempotent `agent-doc commit <file>` recovery. This closes `#rspcmt2` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Managed Codex capability proof now reports phase timings.** Successful `codex_capability_proof` events include `timings_ms` for host DNS, child network, required SSH, launcher writable-root checks, child writable-root checks, and total proof time, so slow `agent-doc start` runs show which capability phase is expensive. The Codex child probe prompts are also shorter while keeping the same shell checks and success markers. This closes `#caplat` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Prompt-prefix repair no longer treats prefixed response headings as prompt starts.** Prefix normalization now recognizes `❯ ### Re:` as an assistant response boundary, so a stale repair target list cannot cascade `❯ ` onto the response body, verification bullets, or commit evidence after a temporarily prefixed heading.

- **Direct `agent-doc <file>` invocation can no longer hang silently after opening preflight.** `run` now bounds the agent-child wait with `AGENT_DOC_RUN_AGENT_TIMEOUT_SECS` (default 1800s), records a recoverable `preflight_started` timeout event with cycle/pane/actor diagnostics on timeout, and rejects recursive Codex direct invocations from the same tmux pane that already owns the document before nesting another Codex child. `session-check` now surfaces those timeout events with concrete retry/restart guidance. This closes `#preflighthang` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Codex network-required sessions now prove network from inside a Codex child.** `codex_network_access: enabled` still clears inherited `CODEX_SANDBOX_NETWORK_DISABLED`, but managed `start` now also runs a bounded `codex exec --json` probe under the same launch args and requires a successful command-execution marker from DNS plus HTTPS checks. Failures distinguish host DNS, child DNS, sandbox/network denial, timeouts, and refused connections before route trusts or reuses the pane. This closes `#codexnonet` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Claimed IPC timeout patches are now durable skip signals.** When the CLI completes an IPC-timeout response by writing the document directly, `.agent-doc/claimed-patches/<patch_id>` now remains in place so every editor watcher pass skips the stale patch instead of only the first consumer. JetBrains also deletes the patch file on the inner EDT dedup path. This reduces post-closeout external edits that could replay the same response block and make later turns look duplicated. Bumped the JetBrains plugin build version to `0.2.106`.

- **Managed Codex panes now prove capabilities before reuse.** Codex `start` records a `codex_capability_proof` event after successful live network, isolated SSH, and writable-root probes whenever the document requests network access, `required_ssh_targets`, or extra `--add-dir` roots. Route no longer trusts a ready managed Codex actor without a current proof after the latest `session_start`; it restarts fresh once with the original launch contract before rerouting, and `session status` reports whether the proof is `proven`, `missing`, or `not_required`. This closes `#codexcapstale` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Managed reroutes keep supervisor-PID recovered panes on supervisor IPC.** When a registered pane no longer exposes the document path in child argv but the healthy supervisor PID still maps to that pane, normal route now treats supervisor IPC as the readiness boundary instead of downgrading an unrecognized prompt probe to a focus-only no-op. This restores the supervisor-PID fallback regression and updates the routing specs.

- **Safe-passive focus-only sync preserves already-visible focused siblings.** When an editor focus event supplies only the focused markdown file after a turn ends on another pane, sync now prefers the remembered or visible column that already owns that file before falling back to active tmux pane replacement. This keeps `docs.md`-style sibling panes selected in place instead of collapsing/replacing the old active pane. Added pure and tmux regressions and updated the session/tmux specs.

- **Safe-passive post-lock focus stays on the editor fast path.** `sync --no-autostart` now prefers the local actor projection for post-lock focus before issuing a controller actor-binding RPC, caches any controller fallback for the rest of the sync cycle, and keeps post-lock focus timing out of the broad `window_resolution` bucket. This targets the current `#syncbudgetstill` traces where one slow actor focus could both double-count as window resolution and trigger another controller lookup later in the same safe-passive sync.

- **IPC normalization fallback now respects concurrent non-exchange edits.** When a plugin sidecar strips a prompt prefix and the binary falls back to normalized `content_ours`, the fallback first merges the current disk content against the explicit pre-response baseline. Deleting a scratch HTML comment while the response is running now stays deleted instead of being restored by prefix repair. Added a regression and updated the closeout specs.

- **Safe-passive focus-only sync preserves visible splits without saved layout state.** If an editor event supplies only the focused markdown file and `.agent-doc/last_layout.json` is absent, sync now derives the sibling projection from registered panes already visible in the target `agent-doc` window before reconciling. This prevents post-turn editor sync from collapsing a visible split to one pane. Added a tmux regression and updated the session/tmux specs.

- **`agent-doc focus` no longer waits on the project controller RPC.** The editor immediate-focus path now selects a live local actor projection from `.agent-doc/session-actors.json`, then falls back to `sessions.json`, without launching or blocking on the controller actor-binding request. Background `sync --no-autostart` still owns slower reconciliation and projection repair. Added focused regressions and updated the focus/editor specs.

- **Editor document switches now attempt immediate focus before background reconciliation.** JetBrains and VS Code automatic tab sync issue a best-effort `agent-doc focus <file>` as soon as a markdown selection changes, then let the existing debounced `sync --no-autostart` reconciliation run in the background. Missing panes still fall through to reconciliation, while existing-pane handoffs feel snappy. Added VS Code command-arg coverage, updated the editor specs, and bumped the JetBrains plugin build version to `0.2.105`.

- **Automatic editor sync now skips superseded deferred retries.** If a rapid document switch leaves an older automatic sync running and that older process later reports a retryable preserved-layout or sync-lock-contention result, JetBrains and VS Code no longer schedule a delayed retry for that intermediate snapshot. The completed process is allowed to finish in the background, and only the latest selected document is replayed. Added plugin regressions, updated the shared editor specs, and bumped the JetBrains plugin build version to `0.2.104`.

- **Safe-passive sync now defers live stash-agent ownership proof on changed selections.** The first safe-passive cleanup pass after an editor selection/layout change still prunes stale registry entries, idle stash shells, and retained-dead non-stash panes, but it preserves live unregistered agent panes in stash instead of spending seconds proving whether each one is still owned. Full sync and explicit repair paths keep the deeper kill-or-preserve cleanup. Added a focused stash cleanup regression and updated the sync spec. This closes `#stashprunefast` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Project controller launch now falls back when `current_exe()` is stale after local installs.** Lazy controller startup no longer fails with bare `No such file or directory (os error 2)` when the running agent-doc process points at a binary path that was removed or replaced. Controller launch and bootstrap identity now prefer the live current executable, then fall back to the invoked command or `agent-doc` on `PATH`, and only then fail with a diagnostic that names the skipped stale path. Added focused resolver regressions and updated the controller specs. This closes `#syncbudget-regress` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Dispatch-only route now submits to healthy `starting` controller actors instead of refusing editor reruns.** If the controller and healthy supervisor still report `starting`, `route --dispatch-only` keeps the same direct-pane submit boundary as file-scoped `session clear` instead of focusing and dropping the rerun. When the pane is visibly dispatch-ready, route also promotes stale lifecycle state to `ready`, but split-pane submission no longer depends on that prompt probe. Added a four-pane tmux regression and updated the routing spec. This closes `#editorswitchctl` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Ordinary HTML comment bodies no longer count as prompt extensions.** The escaped-conversation/template repair scanners now ignore non-agent `<!-- ... -->` ranges the same way they already ignore code spans, so prompt-like scratch notes typed after `<!-- /agent:exchange -->` stay outside exchange instead of being moved into the live prompt tail. Session-check and write-path prompt-drift decisions also classify comment-stripped bodies. Added component, template, and session-check regressions for multiline HTML comment bodies.

- **`finalize --pending-done` now closes `do #id` turns in one pass.** Passing `--pending-done <id>` records a tracked-work mutation before closeout guards run, so pending-capture treats the item resolution as the required backlog outcome instead of demanding a second repair/finalize attempt. If preflight or repair already reaped the item into `agent:pending-done`, the flag is now an idempotent warning instead of a fatal missing-id error. Added focused write, pending, and finalize regressions and updated the closeout/pending specs. This closes `#finalize-do-cascade` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Editor document switches now focus through the controller actor before slow sync work.** Safe-passive `sync --no-autostart --focus <file>` resolves the focused markdown file through the live controller actor binding and selects that pane before waiting on `.agent-doc/sync.lock`, prune cleanup, ownership proof, or tmux-router reconciliation. A stale starting sibling session or contended sync can still defer layout reconciliation, but it no longer leaves tmux focus stuck on the wrong document. Added a tmux regression and updated the sync specs. This closes `#editorswitch` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Safe-passive sync now rate-limits repeated stash cleanup on unchanged layouts.** The common editor-selection path still prunes stale registry entries and retained-dead non-stash panes on every sync, but repeated `sync --no-autostart` runs with the same visible column/window mapping skip the expensive `prune_stash_windows` and `prune_stash_panes` work inside a short throttle window. Focus-only selection churn now logs near-zero stash cleanup subphases instead of spending the safe-passive budget rescanning orphaned stash panes. Added focused regressions and updated the sync spec. This closes `#syncprune` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Sync ownership proof now reuses per-cycle controller/live-owner facts.** A single sync run no longer re-queries the same document/session/pane actor binding and supervisor-backed live-owner proof across pre-reconcile ownership checks, synthetic tmux-router registry construction, and post-router registry projection. Added a regression for the per-cycle cache and updated the sync spec. This closes `#syncproof` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Cross-document sync no longer waits behind another document's closeout pane.** Manual `Sync Tmux Layout` and passive editor autosync still protect panes that own open `preflight_started`, `response_captured`, or `write_applied` cycles from DETACH, but a protected pane no longer turns a different requested document into a deferred no-op. Sync now attaches/focuses the requested pane immediately around the protected closeout owner, accepting temporary visible pane growth instead of blocking editor navigation. Updated tmux regressions and the sync specs for the `agent-doc-bugs2.md` repro.

- **Protected sync edge coverage now has deterministic SimWorld traces and fewer default-suite tmux variants.** Added named `#tmuxbudget` simulator traces for protected-layout handling, detachable-pane replacement, and preserve-layout focus handoff, plus simulator corpus coverage counters for sync protected/replacement/focus decisions. The default suite keeps safe-passive real-tmux smokes for pane/window movement, but duplicate manual protected-layout tmux variants are ignored behind the matching simulator traces and documented in the deterministic simulation spec.

- **Sync latency now names the expensive phase instead of hiding it in broad buckets.** Manual and passive sync emit `sync_lock_wait`, prune subphases, `controller_actor_lookup`, and `projection_refresh` alongside the existing window, prune, ownership, router, and safe-passive total timings. The live `#synclag` traces showed recent slow manual syncs spending 1.3-1.9s in prune while tmux-router stayed in the tens of milliseconds, so prune now reports registry, metadata-fetch, stash-window, stash-pane, and retained-dead cleanup subphases. Stash-pane cleanup also uses the already-fetched `pane_current_command` metadata instead of sleeping to resample every obvious foreign process.

- **Automatic editor tab sync now always uses passive sync instead of the focus shortcut.** The manual Sync Tmux Layout action already used `agent-doc sync --no-autostart`, which owns stash rescue, protected closeout handling, and safe replacement of detachable visible panes. The automatic VS Code and JetBrains tab-selection planners could still choose `agent-doc focus` for single-file handoffs, leaving editor navigation unable to reproduce manual sync's pane/focus result. Automatic tab sync now dispatches passive sync for every real selection/layout change, with updated plugin regressions and specs for `#autosync`.

- **Sync can now replace an unprotected visible pane even while another visible pane is protected by an open closeout.** The protected-layout guard no longer turns every hidden requested document into a no-op just because a different visible pane is mid-closeout. Manual `Sync Tmux Layout` and passive editor autosync now preserve the protected pane, displace an unprotected unwanted pane when one is available, and focus the requested pane. Added tmux-backed regressions and updated the session/tmux command spec.

- **Project controller IPC now fails closed around stalled clients.** Controller request and response reads have bounded timeouts, the server handles accepted clients independently so an idle socket cannot monopolize `.agent-doc/controller.sock`, and `status --ensure` releases its readiness stream before issuing the status RPC. Added regressions for response timeout and idle-client isolation, and updated the controller specs. This closes `#ctrlsock` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Project controller clients now invalidate stale controller binaries before RPC dispatch.** The controller bootstrap/status contract records the startup agent-doc binary path, version, size, and modified timestamp, and `connect_or_launch` compares that stamp against the caller before reusing an active socket. Missing or mismatched binary identity now triggers a controller shutdown and lazy relaunch, preventing local rebuilds or installs from leaving an old controller process that rejects newly-added RPCs such as `session_status` as unknown commands. Added focused controller identity regressions and updated the controller command spec. This closes `#ctrlreload` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Project Controller Phase E routes operator commands through the controller boundary.** `agent-doc session status/history/attach/restart/clear/doctor` now use controller-owned actor state for operator reads and command staging: status includes controller leases, recent command attempts, and projection drift; history prefers durable actor transitions; attach creates the manual handoff generation through controller IPC before refreshing `sessions.json` as a projection; restart and clear record an accepted or rejected operator stage before supervisor/tmux delivery. Added focused controller and clear-path regressions and updated the session actor/command specs. This closes `#pcops` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Project Controller Phase D moves actor-backed route/sync authority behind controller IPC.** Route and sync now request the document actor binding from the project controller before consulting supervisor-backed registry compatibility evidence, and route records controller `dispatch` attempts before managed or dispatch-only submits to the actor pane. Stale session, pane, or generation requests fail closed before input is sent; `session-actors.json`, session-log, registry-rebind, and process-tree evidence remain projection or repair diagnostics. Specs and controller regressions cover actor binding lookup, accepted dispatch attempts, and stale-generation rejection. This closes `#pcroutes` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Project Controller Phase C now routes supervisor lifecycle facts through controller IPC.** `agent-doc start` lazy-launches the project controller, records the starting actor generation through `start_session`, registers the supervisor pid/socket lease, and reports prompt-ready, busy dispatch, waiting-input, blocked, and closed transitions through controller-owned actor updates. Stale lifecycle reports now fail closed on session/pane/generation mismatch, supervisor leases keep runtime state current, and specs/tests cover the controller registration path. This closes `#pcsuper` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Claude streaming prompt writes now tolerate early child exit.** If the child exits before reading stdin, a `BrokenPipe` during prompt write is treated as normal subprocess termination so the streaming iterator can surface the real nonzero exit status and stderr diagnostics.

- **Project Controller Phase B now persists actor records through SQLite before emitting JSON projections.** `session_actor.rs` routes actor load/store through the controller state boundary, `project_controller.rs` owns `.agent-doc/state.db` tables for documents, transitions, leases, dispatch attempts, and projection diagnostics, and compatibility projections are emitted from committed state. Existing `sessions.json` entries are reconciled to the controller actor binding, while missing or failed projections record drift diagnostics without rolling back the authoritative actor transition. Added focused controller regressions and updated the session-actor/controller specs. This closes `#pcstore` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Explicit-baseline closeout now survives session document moves after preflight.** When a document is moved after `preflight`, rename migration can move `.agent-doc/baselines/<old-hash>.md` to the new hash before `finalize` reads the explicit `--baseline-file`. The write path now retries the migrated current-hash baseline, preserving the strict explicit-baseline contract instead of failing into a no-baseline fallback. Added a regression and updated the closeout/snapshot specs. This closes `#pathmove` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **JetBrains protected-layout sync warnings now identify the blocking pane.** The live `tasks/software/tsift.md` replay showed the backend was correctly preserving pane `%208` while its `preflight_started` closeout was open, but the JetBrains notification collapsed that into a generic "another pane" warning. `SyncLayoutAction` now parses the protected pane list from sync output and includes the pane id, phase, and document path in the visible warning, with editor spec/test coverage. Bumped the JetBrains plugin build version to `0.2.99`.

- **Prefixed assistant response labels no longer reopen committed cycles.** The prompt-target classifier now normalizes optional `❯`, list markers, and markdown emphasis before checking known assistant labels, so lines like `❯ **Verification:** ...` and `❯ **Commit / push:**` stay response prose while real prefixed follow-ups still start prompt runs. JetBrains prefix repair mirrors the same ordering, with Rust/session-check and editor regressions. Bumped the JetBrains plugin build version to `0.2.98`. This closes `#respfx` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Codex latest-prompt lookup now skips malformed hook-state entries.** `codex_hook.rs` no longer lets one unreadable or partially written session JSON hide a valid newer prompt for the same file, which keeps parallel hook-state churn from making `load_latest_prompt_for_file` return `None`. Added a direct regression and updated the shared spec.

- **IPC `content_ours` prefix fallbacks now repair the working tree before commit.** When plugin sidecar verification rejects a normalization result, `write.rs` still falls back to normalized `content_ours`, but it now writes that same repaired content back to disk before returning success. This prevents a later commit from capturing a plugin-stripped `❯ ` prompt prefix even though the snapshot was already repaired. The same closeout follow-through tightened the fresh-prompt classifier so stale prefix-repair target lists containing `Commit / push:` cannot prefix later assistant evidence labels. The closeout spec and regression coverage now assert snapshot preservation, working-tree preservation, and stale-target assistant-label suppression. This closes `#pfxcours` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Concurrent prompts added during explicit-baseline closeout now fail closed.** `write.rs` now classifies live disk drift against the pre-response baseline before the response is merged, so a prompt typed after preflight but before `finalize` cannot be mistaken as answered by the response that was already in progress. The committed snapshot stays at `content_ours`, `session-check` interrupts on the unresolved `prompt_target`, and the closeout spec plus integration coverage now encode the contract. This closes `#concprompt` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Streaming responses now leave durable partial checkpoints before final closeout.** `capture.rs` now maintains a `.partial.json` checkpoint ledger beside final response captures, saving the first non-empty streamed response and then changed partial output at most every 30 seconds without advancing the cycle to `response_captured`. Both `agent-doc stream` and CRDT orchestration streaming feed that checkpoint writer, with regressions proving the partial checkpoint survives before final closeout and remains diagnostic-only. This closes `#chkptcap` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **JetBrains protected-layout sync warnings are now deferred-sync UX instead of raw CLI diagnostics.** Manual `Sync Tmux Layout` still warns visibly when a protected visible pane is mid-closeout, but the notification is concise and the full CLI output stays in logs. Automatic tab sync now treats both preserve-layout markers (`[sync] sync preserved...` and `[sync] safe passive sync preserved...`) as deferred retry states unless the output includes `safe_passive_layout_preserved_reselected_focus`, covering the `agent-doc-bugs2.md` to `tasks/software/tsift.md` navigation repro. Bumped the JetBrains plugin build version to `0.2.97` and refreshed editor specs/tests for `#jbsyncwarn`.

- **Assistant response tails committed in `HEAD` now have explicit prompt-prefix regression coverage.** The `#pfxleak3` repro was a narrower variant of the `content_ours` fallback leak: a new prompt inserted directly below a prior assistant response tail could make the tail look like part of the prompt run. The closeout spec now states the `HEAD` prefix-state invariant explicitly, and the write-path regression proves the tail remains bare while only the new `do [#pfxleak3]...` prompt receives `❯ `.

- **Post-commit prompt-prefix repair no longer treats assistant `Commit / push:` evidence labels as prompt targets.** The `#pfxleak` closeout reproduced the remaining leak live: a historical bad `❯ Commit / push:` line in committed assistant content caused the IPC prefix-repair signal to add `❯ ` to a later assistant response label after the commit, tripping `session-check` as an unstarted prompt. The plain-response classifier now recognizes `Commit / push:` before the generic `commit ...` prompt heuristic, and both target extraction and prefix application refuse to propagate stale assistant-label targets. Added regressions for the target extractor, full-document prefix repair, and IPC patch-content normalization.

- **`content_ours` prompt-prefix normalization now preserves multi-line user prompts after stale inserted response blocks.** The `#pfxstrip2` repro showed a stale snapshot keeping the normalizer in agent-response mode long enough to skip ordinary `Please ...` prompt bodies, while a later preset-like prompt still received `❯ `. The write path now reopens a blank-separated fresh prompt run after an inserted response, prefixes every nonblank prompt line outside fences, and preserves already-committed `HEAD` prefix state bidirectionally so prefixed user prompt lines stay prefixed while prior agent response lines stay bare. Added regressions for the multi-line prompt strip and both HEAD prefix-state directions, and updated the closeout spec.

- **Closeout drift noise is narrowed after evaluating the Claude Code + Codex logs.** `session-check` no longer treats plain `content_edit` drift as an unstarted closeout after a committed cycle, so minor already-answered transcript edits do not force a second finalize. The `content_ours` prompt-prefix fallback now preserves unprefixed exchange lines already committed in `HEAD`, preventing prior agent response lines from gaining `❯ ` and needing a follow-up normalize commit. Template repair also keeps relocated live prompts out of the saved snapshot so preflight still sees them as user work.

- **Repeated no-op closeout churn is advisory again instead of an automatic compact handoff.** `plan.rs` no longer converts the repeated `commit_noop` subset of session-accretion into a mandatory `agent-doc compact ... --commit` command, so a document without an explicit compaction request continues normal repo work and closeout. The closeout spec, README, planning runbook, and regression test now state that session-accretion signals can suggest compaction but must not force it. This fixes the unwanted autocompaction reported in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Dispatch-only Codex accepted-but-unproven failures no longer print an optimistic fallback line first.** `route.rs` now suppresses the legacy "accepted but no proof" progress message when `Run Agent Doc` is in dispatch-only Codex mode and hook tracking is visible, leaving the final accepted-but-unproven error as the only user-facing outcome. Added route/plan regressions and refreshed the session-routing and JetBrains specs.

- **Sync layout cardinality and passive focus proof now share one visible projection contract.** Manual `agent-doc sync` preserves open closeout panes from DETACH while letting different requested documents attach and focus immediately around them. Preserve-layout focus handoffs for genuinely blocked files still print the `safe_passive_layout_preserved_reselected_focus` proof to command output, and the JetBrains automatic sync planner treats that proof as applied instead of retrying a selection that already focused the requested visible pane. Added tmux-backed and JetBrains planner regressions plus spec updates for `#syncfocuscard`.

- **Template repair now relocates prompt-only drift that lands between `agent:exchange` and markdown section breaks.** The latest `#oobprompt` repro in `tasks/agent-doc/agent-doc-bugs2.md` was narrower than the earlier escaped-response gap bug: `repair`/`preflight` already fixed `### Re:` or `## Assistant` tails stranded before later components, but a bare prompt target such as `do [#id]...` typed after `<!-- /agent:exchange -->` and before a plain `###` / `## Pending` section marker stayed outside exchange because the shared detector only keyed on escaped response headings. `template.rs` now isolates prompt-target blocks in that exchange-to-section gap, feeds them through the same guard/repair path, and leaves the structural separator outside the exchange. Added direct template regressions plus a repair-path normalization test. This closes `#oobprompt` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Required SSH prelaunch probes now isolate themselves from shared SSH socket state.** `agent::codex` was proving `required_ssh_targets` by running real `ssh <target> true` checks through the operator's normal SSH config, which meant ControlMaster/ControlPath multiplexing or forwarded-session side effects could leak out before the managed session even started. The probe path now forces isolated SSH flags (`ControlMaster=no`, `ControlPath=none`, `ClearAllForwardings=yes`, `PermitLocalCommand=no`) on both alias and direct-host checks, tightens the failure text to call out the isolated pre-launch probe scope, and adds unit coverage for the no-shared-socket contract. This closes `#sshcut` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Safe passive sync preserve-layout guards now keep tab focus moving across already-visible panes.** The latest `tasks/agent-doc/agent-doc-bugs2.md` regression came from the new preserve-layout exits in `sync --no-autostart`: when a blocked or protected missing file forced safe passive sync to skip tmux-router reconciliation, the command also skipped the final pane selection, so switching editor focus between already-visible docs could leave the `agent-doc` tmux window stuck on the old pane. `sync.rs` now reselects the requested pane before either preserve-layout return when that file is already visible, and added tmux-backed regressions for both the blocked-file and protected-pane guard paths. This closes the latest sync-focus regression in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Warn/block bounded context packs now expose a lightweight response TOC plus targeted retrieval commands.** Added `agent-doc response-toc` to enumerate current live `### Re:` sections alongside matching archived response sections for the same document, and `agent-doc response-fetch` to load exact live or archived sections with bounded neighbors. `prompt_context.rs` now includes that TOC in warn/block context packs and explicitly points agents at `response-fetch` for on-demand neighboring history instead of relying only on the fixed recent-turn window. Added unit + CLI regression coverage and updated the command/spec docs. This closes `#restoc` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Warn/block bounded context packs now anchor response history to the prompt's position in `exchange` instead of always replaying the newest `### Re:` turns.** `prompt_context.rs` now locates each prompt target inside the live exchange and includes the enclosing response block for inline prompt edits or the immediately previous response for tail follow-ups, while still falling back to the old recent-turn slice if no clean anchor can be found. Added regressions for both anchor shapes and updated the orchestration/README docs to match. This closes `#wv7g` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Dispatch-only Codex reroutes now fail once with a precise "accepted but unproven" reason instead of looking non-responsive.** `route.rs` now classifies Codex clean-exit restart prompts as an immediate dispatch blocker, routes healthy authoritative-actor `Run Agent Doc` submits through the same checked live-pane helper as other dispatch-only reroutes, and requires hook-visible Codex reroutes to produce bounded submission proof instead of silently succeeding on bare tmux acceptance alone. Added regressions for the restart-prompt blocker and the hook-visible authoritative dispatch-only failure shape, and updated the session-routing plus JetBrains specs. This closes `#fye2` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Resumed required-SSH Codex streams now discard stale prelude text before the fresh retry.** `agent::codex` no longer blocks required-SSH capability-drift recovery just because the resumed stream already emitted assistant text. For SSH-gated resumed streams it now buffers early agent chunks until the stream proves required SSH success or completes successfully, retries fresh once even after a stale prelude, and drops the buffered resumed prelude if that retry fires. Added streaming regressions for the exact "assistant prelude, then SSH failure" report plus the successful SSH release path, and updated the agent-backend spec and README. This closes `#sshprelude` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Codex Stop now records tool-only/auth-interrupted closeout misses instead of surfacing a generic empty-response block.** `codex_hook.rs` now saves a blocked-stop diagnostic even when `last_assistant_message` is empty, includes the tracked prompt in that artifact, and tells the operator that this often means Codex stopped after a tool-only/authentication step such as an MCP OAuth / `authenticate` flow before the final closeout was emitted. Updated the bundled skill, harness runbook, git-integration spec, and shared spec so MCP auth is explicitly a sub-step that still must end through `finalize` / `write --commit` plus `session-check`. This closes `#257p` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Session-accretion heuristics are now advisory only.** `plan.rs` no longer blocks normal turns on churn-heavy session metrics, and `preflight.rs` no longer auto-compacts exchanges at all, including documents that still carry legacy `auto_compact` frontmatter. The bounded recent-context pack stays in place for warn/block accretion prompts, but it no longer has a binary-enforced compact/block side effect. Updated regressions and command/docs text accordingly. This addresses the latest critical usability report in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Session-accretion turns now keep full documents intact and send bounded recent-turn context instead of auto-compacting mid-turn.** `preflight.rs` no longer auto-compacts template exchanges just because session-accretion heuristics tripped. `prompt_context.rs` now builds the warn/block response-context pack with prompt targets, session summary, backlog head, recent `### Re:` turns, and an explicit "ask for more previous turns if needed" instruction, so long sessions stay intact on disk while resumed prompts stay bounded. Added regressions for the no-auto-compact preflight path plus the richer recent-turn prompt pack. This closes `#ratecmp` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Preflight/plan now surface deterministic context-accretion signals without enforcing a hard stop.** Added `session_accretion.rs`, which summarizes per-document exchange growth, recent closeout churn, and restart-heavy reopen signals from the existing document/session logs without replaying full transcripts. `preflight` now emits a structured `session_accretion` advisory when those local heuristics trip, and the prompt-building path can still choose a bounded recent-context pack from that report, but `plan` no longer fails closed on the hard-stop tier. Added regressions for large exchanges, repeated no-op closeouts, restart-heavy churn with an active startup-miss, the preflight JSON surface, and the non-blocking plan contract. This closes `#ctxacc` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Sync reconcile now preserves panes whose documents still have an open closeout cycle.** `sync.rs` now re-enables tmux-router's DETACH protection only for panes whose registered document is still in `preflight_started`, `response_captured`, or `write_applied`, so layout reconciliation warns and leaves that pane visible instead of stashing it mid-closeout. Added regression coverage for both the open-cycle detector and the sync reconcile replay that keeps the in-flight pane visible. This closes `#busychk` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Stale empty `preflight_started` cycles now auto-close on the next preflight instead of trapping the document in manual recovery.** The `#staleflt` repro from `tasks/agent-doc/agent-doc-bugs2.md` showed a narrow crash window where a pane could die after `start_preflight()` but before any response capture existed, leaving later `preflight` runs with an open cycle that had no replay artifact and no exact hash proof. `repair.rs` now treats that shape as a bounded stale-empty-cycle case: if the cycle is still `preflight_started`, has no capture, shows no visible patchback, and is older than the timeout, it is closed as a no-op before the new preflight cycle opens. Added repair/preflight regressions plus spec/skill updates for the stale-empty timeout contract. This closes `#staleflt` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Required SSH metadata can now resolve from project config, and missing mappings fail closed before launch.** `frontmatter::parse_for_file()` now resolves effective SSH requirements from document frontmatter plus project-local `.agent-doc/config.toml` mappings (`[ssh.docs."<path>"]`, `[ssh.profiles.<name>]`), so known ops docs no longer bypass the required-SSH contract just because frontmatter omitted `required_ssh_targets`. `preflight`, `plan`, `run`, `start`, and `route` now consume the path-aware parse, and they stop immediately when a configured SSH-dependent document resolves no targets or references a missing profile. Added config/frontmatter/preflight regressions and the fresh-restart route guard needed to keep the suite green under the new path-aware parse. This closes `#sshmeta` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Phase-4 authoritative actor dispatch is now explicitly closed out in spec and regressions.** `route.rs` already switched normal reroutes onto the authoritative actor record and supervisor IPC in `312851e`, with later follow-ups covering harness aliasing and waiting-input recovery, but the phase item still lacked direct proof for the remaining hard-stop states. Added tmux-backed route regressions that prove `blocked` and `closed` authoritative actors fail closed without injecting a duplicate reopen into either the actor pane or a stale registered pane, and updated the session-actor contract to pin the full phase-4 state matrix. This closes `#sgown4` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **JetBrains passive tab sync now trusts the selection event target instead of a potentially stale `selectedTextEditor` snapshot.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro showed a narrower editor-side skip than the queued-replay race: when the user switched from `equityfundingsource.md` back to `agent-doc-bugs2.md`, JetBrains could enter `selectionChanged` with `event.newFile` already updated while `FileEditorManager.selectedTextEditor` still pointed at the previous file. That made the automatic snapshot/dedup planner think focus had not changed and suppress `agent-doc sync --no-autostart` entirely. `EditorTabSyncListener` now treats the event file as the authoritative active markdown target for automatic snapshots, falls back to the selected editor only when no event target exists, and adds a regression covering the stale-selected-editor shape. Bumped the JetBrains plugin build version to `0.2.94` and updated the plugin spec with the same callback contract.

- **Explicit `repair` now fails closed when only later prompt drift remains after committed historical patchback recovery.** The `#rprdrift` repro in `tasks/agent-doc/agent-doc-bugs2.md` exposed a bad explicit-repair downgrade: `repair::run()` could legitimately find no pending/capture artifact, but `agent-doc repair` then returned `No pending response found` even though `session-check` would still interrupt the same committed cycle after repairing the snapshot from `HEAD` and noticing later prompt-bearing drift. `repair()` now re-runs the closeout interruption check on no-op outcomes and surfaces that same failure instead of pretending the document is clean. Added a regression covering the exact committed-patchback-plus-follow-up-prompt shape and updated the closeout/spec docs to make the fail-closed contract explicit.

- **Completed backlog reap now removes malformed flush-left spill with the done parent item instead of orphaning it in backlog.** The latest `#mlreap` repro in `tasks/agent-doc/agent-doc-bugs2.md` showed `pending::reap_with_items()` only dropping the tracked `[x] [#id]` line while leaving adjacent flush-left command/diff transcript spill behind as generic backlog text. Reap now strips the leading non-structural text block that immediately trails a completed parent, preserves true structural separators such as headings/comments, and archives that spill with the removed item so preflight/repair/backlog reap no longer leave orphan prose behind. Added regression coverage in `pending.rs`, the backlog CLI integration, and preflight's live-prompt preservation path.

- **Codex `UserPromptSubmit` now finds the real `agent-doc <FILE>` line after injected prompt preambles.** The latest `#rspcmt` closeout miss showed a direct Codex `agent-doc ...` turn can arrive at the hook wrapped in AGENTS/instruction text, so the old "first non-empty line only" parser never tracked the target doc and the `Stop` hook had nothing to recover or block. `codex_hook.rs` now scans the prompt from the end, skips fenced placeholder examples like `agent-doc <FILE>`, and records the last real invocation line instead. Added hook regressions for prompt-preamble parsing and the resulting active-session post-commit drift recovery path.

- **Hash-prefixed pending ids now resolve on the actual mutation path, not just in closeout guards.** The `#9aep` tsift repro showed an inconsistency between agent-doc's backlog guards and its write-time pending mutations: `cycle_state` and `session-check` already normalized `#id`, but `pending_cmd::done()` and the lower-level pending ops still compared raw strings, so `agent-doc finalize --pending-done '#9aep'` failed with `id not found in backlog/icebox` even though the backlog item existed. The pending mutation layer now strips one optional leading `#` and lowercases ids across done/edit/gate/ungate/reorder/set-gate-type lookups, and added regressions for `op_done`, `write --pending-done '#id'`, and `finalize --pending-done '#id'`.

- **Safe passive sync now locks the exact VS Code mixed-root split replay into spec and tmux regression coverage.** `sync.rs` already preserved visible layout when a passive `--no-autostart` file stayed blocked, but the coverage was still generic. The latest `#vssplitreplay` closeout now names the concrete `tasks/agent-doc/agent-doc-bugs2.md` + `src/session-share/tasks/claudescore-3.md` split, proves that blocked sibling files do not stash either healthy visible pane, and records the same replay shape in the session/tmux spec so the visible mixed-root layout cannot silently collapse back into a new authoritative pane set.

- **Path-scoped manual repo commits now fail closed on staging drift in the installed instruction surface.** The bundled `SKILL.md`, `commit.md`, `harness-invocation.md`, `compound-task-steering.md`, `SPEC.md`, `README.md`, and git-integration spec now require agents to resolve the intended non-session path set first, stop immediately on any stage failure, verify the staged diff still matches the intended set, and commit only that validated set before `finalize` / `write --commit` closes the session document. Added regression coverage in `skill.rs` so future installs keep the stricter pathset-validation rule.

- **Skill/runbook commit ordering now explicitly keeps session docs off manual repo commits.** The bundled `SKILL.md`, `commit.md`, `harness-invocation.md`, `compound-task-steering.md`, and git-integration spec now state that compound `commit + push` work must exclude the active session document from any ordinary repo `git commit`, defer the session-doc closeout to `agent-doc finalize` / `write --commit`, and only push after that binary-owned closeout commit lands. Added regression coverage in `skill.rs` so future installs keep the stricter staging/order rule.

- **JetBrains automatic splitter replay now uses the latest captured event snapshot instead of re-sampling editor state after the previous sync finishes.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro showed one remaining race in rapid `Go to Next Splitter` sequences: the plugin could queue a replay correctly, then rebuild its command from a later background-thread view of `FileEditorManager` and land tmux on the first splitter hop instead of the final one. `EditorTabSyncListener` now snapshots the exact active file plus detected split layout on each selection event, replays the newest captured snapshot after an in-flight sync, and uses a column-aware visible signature so splitter identity survives replay dedup. Added JetBrains regression coverage for the column-aware replay signature, bumped the JetBrains plugin build version to `0.2.93`, and updated the shared plugin spec to require event-snapshot replay instead of live re-sampling.

- **Automatic editor sync now replays the latest queued selection/layout request instead of silently dropping it while another sync is running.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro was not primarily the 100 ms debounce delay itself. Both editor plugins could coalesce selection churn correctly, then lose the actual requested handoff because the automatic concurrency guard simply returned when a sync/layout command was already in flight. That meant selecting another visible agent doc during an active sync often did nothing until the user manually ran Sync. VS Code now recomputes tab-sync state from the live editor after each automatic run and immediately replays the newest queued request when generation changed mid-flight. JetBrains now does the same for tab-selection sync, and its layout-change detector also schedules one immediate replay when a newer automatic request lands during an in-flight layout reconcile. Added focused plugin regressions for the queued replay contract and updated the shared editor specs.

- **Bare session-document `write` no longer reports success after a synthetic/template `write_stream` leaves closeout open.** The historical BuildParty `dev.md` repro in `tasks/agent-doc/agent-doc-bugs2.md` showed a narrower closeout gap than the earlier generic missed-commit family: the CRDT/template write path had already preserved the response, capture, and synthetic `write_stream` state, but the command still looked successful until a later explicit `agent-doc commit` finally recorded `commit_success`. `write.rs` now keeps that response/capture state for recovery but immediately fails closed when a real session doc uses bare `agent-doc write` and the cycle remains open, so `response_captured` / `write_applied` can no longer masquerade as a completed turn. Added an integration regression that proves the bare stream write returns nonzero, preserves the synthetic `write_applied` evidence, and still lets a later explicit `agent-doc commit` finish the boundary.

- **Answered-prompt closeout canonicalization no longer rewrites prior assistant tail prose into fake `❯ ` prompts.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro exposed an over-greedy commit-time heuristic in `git.rs`: when a real answered prompt such as `do [#tailpatch]...` shared one contiguous block with the previous response tail, the closeout canonicalizer could prefix the whole block and commit assistant prose like `There were no actionable follow-up items to capture.` as if it were user input. The canonicalizer now starts at the first prompt-like line in that block and only prefixes from there onward, preserving multi-line prompt bodies without swallowing the assistant tail above them. Added a regression covering the exact mixed tail + `do [#...]` shape.

- **Manual `[x]` backlog completions now survive same-cycle history replay checks.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro exposed a second completed-backlog regression after the resurrection fail-closed work: preflight/repair could reap a user-edited `[x]` tracked item, but the backlog replay guard still only exempted ids recorded through `--pending-done`, so the just-reaped id looked "dropped from history" and could be restored from the older `[ ]` / `[/]` state. `cycle_state` now exposes a unified resolved-id set from both explicit `pending_done_ids` and same-cycle `reaped_pending_ids`, and the preflight/session-check history guards both consume that merged set. Added regressions for the manual `[x]` reap path in preflight and session-check.

- **Template closeout now guards and repairs escaped conversation in the gap between `agent:exchange` and later components.** The latest `tasks/agent-doc/agent-doc-bugs2.md` `#tailpatch` repro was not a total write-path bypass: the existing template guard only scanned after the final parsed component, so a prompt/response block that slipped between `<!-- /agent:exchange -->` and later sections such as the stray `###` marker or `agent:backlog` could still survive closeout even though the document remained parseable. `template.rs` now shares one outside-exchange range detector across the fail-closed guard, explicit repair, and manual-tail strip paths, so inter-component escaped conversation is blocked on normal write/finalize and recoverable through repair when the structure is safe. Added regressions for guard, repair, and strip on the exchange-to-backlog gap shape.

- **Normalization-divergence IPC fallbacks now re-apply prompt prefixes before saving snapshots.** When ack-content sidecar verification rejects editor output because a `normalize_prefix_lines` target is missing its `❯ ` prefix, both socket and file IPC fallback paths now run the target-based exchange prefix repair over `content_ours` after preserving any on-disk backlog mutations. This closes the `#bppfxstrip` shape where a sidecar-divergence fallback could still save a bare `do #...` prompt into the committed closeout baseline. Added regression coverage and updated the closeout/plugin specs.

- **Editor-side prompt-prefix repair now runs after exchange patch application, and pure-reposition fast paths no longer swallow normalization-only repairs.** The latest `tasks/agent-doc/agent-doc-bugs2.md` regression was an editor-plugin convergence bug rather than a snapshot-classification miss: the binary already emitted `normalize_prefix_lines`, but the JetBrains plugin applied that repair before later exchange/unmatched patches and could overwrite the fixed `❯ ` lines in the ack sidecar, while the VS Code reposition-only shortcut treated `patches: []` as a pure boundary move even when the payload still carried `normalize_prefix_lines`. JetBrains now normalizes the exchange user region after component/unmatched patch application and before boundary/head cleanup in both the Document and VFS paths, and the VS Code watcher now reserves its reposition-only debounce shortcut for truly empty boundary moves. Added a targeted VS Code regression for the patch-shape gate and refreshed the shared plugin spec for the pure-reposition contract.

- **Completed-backlog reap now fails closed if the same ids reappear in the live backlog or icebox before closeout.** The latest `tasks/agent-doc/agent-doc-bugs2.md` regression was not a one-sided snapshot bug: `repair`/`preflight` could reap user-marked `[x]` items correctly, but a stale local/editor rewrite could put those ids back into the live `agent:backlog` before the same cycle reached `git::commit()`. Because closeout treated that as generic post-commit local drift, HEAD stayed clean while the working tree resurrected the supposedly removed items and the next preflight had to reap them again. `cycle_state` now records ids reaped during the active cycle, `preflight.rs` and `repair.rs` publish those ids when they remove completed tracked work, and `git.rs` now blocks closeout if any of those ids reappear in the live backlog/icebox before commit. Added regression coverage for the new cycle-state ledger and the fail-closed commit guard.

- **Post-claim route sync now stays on the caller's tmux server, so isolated verification no longer mutates the live `agent-doc` window.** The latest `tasks/agent-doc/agent-doc-bugs2.md` pane-retention repro was not a normal editor sync failure: local verification was still calling `sync_after_claim(...)` with an injected `Tmux`, but the helper delegated to `sync::run(...)`, which silently jumped back to the default tmux server. In practice that meant a route/unit-test reconcile using dummy files like `file_a.md` / `file_b.md` could stash a visible sibling pane such as `src/session-share/tasks/buildparty-investor-demo/dev.md` out of the operator's real `agent-doc` window, after which a normal sync would merely rescue it back. `route.rs` now keeps that reconcile on `sync::run_with_tmux(...)`, and added a regression that proves the injected server's overflow pane is stashed locally instead of the default server being touched. Updated the sync-layout spec with the same invariant.

- **Dispatch-only live-pane reroutes no longer impose a second startup-ready gate that file-scoped clear never had, and tmux command submissions now route through one helper at the call sites.** The latest `tasks/agent-doc/agent-doc-bugs2.md` ops-log evidence showed that `session clear` was already succeeding via `delivery=direct_pane_submit`, but `route --dispatch-only` could still refuse the same pane with `still booting` because `dispatch_only_send_reopen(...)` ran an extra ready-probe loop before it was allowed to use that direct tmux submit path. `route.rs` now keeps the supervisor-IPC boot-window probe only for supervisor-owned reopen delivery; direct live-pane reroutes stay on the same single-submit tmux helper that clear already uses. I also rewired the remaining command-submit call sites in `route.rs`, `queue_dispatch.rs`, and `parallel.rs` to use `sessions::send_submitted_text(...)` instead of open-coded `tmux.send_keys(...)`, so tmux-bound command submission is centralized at the call site layer instead of only by convention. Added/updated tmux regressions for the starting-pane reroute behavior and refreshed the session/tmux docs.

- **Dispatch-only authoritative reroutes now stay on the live-pane tmux submit path even while the actor still reports `starting` or `busy`, and supervisor inject has a real socket-to-tmux regression.** The latest `tasks/agent-doc/agent-doc-bugs2.md` ops-log evidence showed the remaining mismatch clearly: file-scoped `session clear` was already using `delivery=direct_pane_submit`, but prompt-bearing `route --dispatch-only` after clear could still take the authoritative actor's optimistic supervisor-IPC queue path whenever the actor runtime still reported `starting`/`busy`. That kept `Run Agent Doc` on a different delivery boundary than the known-good clear path. `route.rs` now keeps dispatch-only authoritative reroutes on the same live-pane `send_submitted_text(...)` helper even in that short starting/busy window instead of queueing through supervisor IPC, and `start.rs` now has a socket-backed integration regression that drives a real supervisor IPC listener into an isolated tmux pane so the supervisor-owned submit boundary is covered beyond mocked writers.

- **Run/clear tmux submits now share one direct-pane helper, and file-scoped clear resolves the same live pane precedence as dispatch-only reroute.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro still left one structural mismatch: routed reopens, supervisor-owned injects, and file-scoped `session clear` were all supposed to share the same tmux submit boundary, but agent-doc still had multiple direct-pane wrappers and `session clear` only trusted the registry pane before dropping back to supervisor IPC. `sessions.rs` now owns the canonical live-pane submit helper used by route, start/supervisor inject, and `session clear`, and `session_actor_cmd.rs` now resolves direct-pane clear targets in authoritative-actor, live-owner, then registry order before it ever falls back to supervisor IPC. Added pane-selection regressions and updated the tmux session spec so `Clear Session Context` follows the same live-pane preference model as `Run Agent Doc`.

- **Shared tmux submit now pauses briefly before `Enter`, which fixes real Codex slash-command submits while preserving Claude behavior.** The latest `tasks/agent-doc/agent-doc-bugs2.md` investigation finally used isolated live harness panes instead of shell-loop stand-ins. That replay showed the current `tmux send-keys -l ... ; send-keys Enter` helper left `/clear` and `/help` drafted inside Codex even though the same path still worked in Claude. A 50 ms gap between the literal text injection and the submit key made the exact same Codex panes execute the slash command immediately. `tmux-router::Tmux::send_keys()` now uses that delayed submit contract for every live-pane command injection, `agent-doc` logs the mode as `tmux_literal_enter_delayed`, and tmux-router now carries a regression that fails if the submit helper stops leaving enough separation for managed TUIs that coalesce same-tick paste bursts.
- **Tmux-bound command submissions now go through one normalized text path and stop retrying synthetic `Enter` presses.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro showed that agent-doc was still carrying multiple overlapping newline/CR workarounds even after the live-pane submit boundary had been unified. `supervisor::ipc` now normalizes submitted command text once for tmux-bound injects, leaves raw `\r` encoding only for the direct PTY-writer fallback, and route/queue-dispatch no longer send follow-up `Enter` retries after the first tmux submit. That strips the accumulated defensive submit branches back to one literal-text-plus-Enter tmux path for `Run Agent Doc`, `session clear`, queue dispatch, and supervisor-owned reopen injects.

- **Live-pane reroutes and file-scoped clear now use one literal-text plus named `Enter` tmux submit path, and they log which delivery branch fired.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro still showed drafted `\n` behavior even after multiple carriage-return-focused fixes, which meant the remaining shared risk was the tmux submit primitive itself. `tmux-router::Tmux::send_keys()` now always batches literal text plus a named `Enter` instead of using the ASCII `send-keys -H ... 0d` fast path, so `Run Agent Doc` and `Clear Session Context` cross the same live-pane submit boundary as the known-good Claude clear flow. `route --dispatch-only` and file-scoped `session clear` now also write explicit ops-log markers with both the delivery path and submit mode so the next live replay can prove whether the command went direct to the pane or through supervisor IPC. Added tmux-router regression updates for the new submit contract and refreshed the session/tmux spec plus README.

- **Dispatch-only live-pane reroutes now always use the same direct pane submit boundary as `session clear`.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro showed one remaining split in the Enter handling model: healthy authoritative-actor reroutes already typed the bare reopen through the live pane, but plain registered-pane `route --dispatch-only` reroutes could still fall back to one-shot supervisor IPC injects. `route.rs` now keeps dispatch-only reopens on the resolved live pane's tmux input path for both authoritative and non-authoritative existing sessions, so `Run Agent Doc` and `Clear Session Context` share the same carriage-return submit boundary instead of diverging by route branch. Added a registered-pane dispatch-only regression and updated the tmux/session spec plus README to document the unified pane-submit rule.

- **Dispatch-only reroutes now keep using the authoritative pane even when supervisor state is missing, and they log that degraded branch explicitly.** The latest `tasks/agent-doc/agent-doc-bugs2.md` Claude repro showed a mismatch between two editor-adjacent flows: file-scoped `session clear` could still work because it only needed the live bound pane, while `route --dispatch-only` refused the authoritative-pane path as soon as supervisor IPC stopped reporting a healthy runtime/actor state and then fell back to stale registry heuristics that could send nothing. `route.rs` now keeps the strict supervisor gate for the normal authoritative IPC path, but dispatch-only reroutes may reuse the same authoritative pane directly when that pane is still the current registered/live-owner binding. The route path now writes explicit ops-log diagnostics for both the degraded authoritative fallback and the skipped-fallback shape so the next live replay shows exactly why editor reroute did or did not stay on the actor pane. Added a focused Claude tmux regression and updated the session/tmux spec.

- **Authoritative dispatch-only reroutes and file-scoped `session clear` now submit straight to the live pane when the current binary already owns that pane boundary.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro showed one stale-supervisor surface still left after the earlier Enter fixes: editor `Run Agent Doc` and file-scoped `agent-doc session clear <FILE>` could still relay through an already-running supervisor process even when the newer binary had already identified the authoritative pane and corrected the submit semantics. `route.rs` now sends `route --dispatch-only` reopens directly through the authoritative pane's tmux input path once the actor-owned pane is ready, and `session_actor_cmd.rs` now sends `/clear` directly to the authoritative pane when that pane is alive on the default tmux server, falling back to supervisor IPC inject only when no directly addressable authoritative pane is available. Added regressions for both direct-pane paths plus the default-server fallback, and updated the session/tmux spec to document the direct-pane boundary for editor reroutes and file-scoped clear.

- **Supervisor-owned reopen and clear injects now use the claimed pane's tmux input path instead of writing raw bytes directly into the child PTY.** The lingering `tasks/agent-doc/agent-doc-bugs2.md` Enter regression was deeper than newline normalization or stale plugin installs: authoritative route/session-clear/auto-trigger injects still wrote the submit payload straight to the managed child PTY, while the only path proven to behave like a real Enter in live tmux panes was the pane-input `send-keys` boundary. `start.rs` now keeps supervisor IPC as the authoritative control surface but re-delivers submitted input through the claimed pane's tmux key path, so `Run Agent Doc`, `Clear Session Context`, queue-dispatch, and auto-trigger all share one real terminal submit method. Added a start-level tmux regression that proves IPC inject now submits through the pane path rather than only a mocked PTY writer, and updated the routing/session specs plus README to document the tighter contract.

- **Dispatch-only Codex reroutes no longer turn a tracked `/clear` into an editor-side restart.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro showed that the shared Enter-submit fix had landed, but `route.rs` was still applying the older tracked-`/clear` fresh-restart policy before every `agent-doc route --dispatch-only` dispatch. That made `Run Agent Doc` restart Codex instead of sending the expected bare `agent-doc <FILE>` reopen into the live session. Dispatch-only reroutes now keep using the existing supervisor-owned submit path after `session clear`, while the managed non-dispatch route retains the tracked-`/clear` fresh-restart contract for explicit CLI recovery. Added a dispatch-only regression that proves the authoritative actor path still dispatches after a tracked clear without requesting restart, and updated the route/editor runbooks/specs so the editor contract and backend behavior match again.

- **Supervisor-owned command injection now shares one explicit Enter-style submit helper across clear, queue-dispatch, route, and auto-trigger paths.** The latest `Clear Session Context` repro in `tasks/agent-doc/agent-doc-bugs2.md` exposed that supervisor inject senders were still hand-assembling submit bytes in multiple places (`\n`, `\r`, or receiver-side normalization), even though tmux fallback already had a single batched text+Enter contract. `supervisor::ipc::submit_bytes()` now defines the canonical single-line submit payload, `session clear` and queued slash-command dispatch use it directly, route’s supervisor reopen helper delegates to it, and auto-trigger now emits the same Enter byte sequence instead of its own bespoke formatting. Added regression coverage for the shared helper plus exact injected bytes on the `session clear` and queue-dispatch paths, and updated the session/tmux spec so supervisor-owned command injection keeps one explicit Enter method instead of drifting across call sites.

- **Existing managed reroutes now stay on the supervisor-owned reopen path instead of falling back to direct tmux typing.** `route.rs` still uses tmux only to provision a fresh shell/supervisor, but once a managed Claude/Codex session exists the reopen path now goes through supervisor IPC for both managed reroutes and dispatch-only editor reroutes. That removes the remaining split-brain path where non-authoritative live panes could still receive direct `send-keys` reopen traffic, and it makes manual supervisor restarts resolve back onto the same socket-owned boundary instead of silently succeeding through pane typing. Dispatch-only still keeps its one-shot behavior, but the one shot is now a single supervisor inject. Updated the routing docs/README to make the supervisor-only reroute contract explicit.
- **Fallback tmux submits now use one byte-stream command plus carriage return instead of split text/Enter writes.** The latest `tasks/agent-doc/agent-doc-bugs2.md` repro still drafted `agent-doc …` into the managed Codex composer even after the supervisor IPC newline normalization fix, which meant the remaining failure surface was the non-authoritative tmux fallback path. That path still reused `tmux-router`'s literal-text send followed by a separate `Enter`, leaving a gap where managed panes could observe the reopen text without consuming it as one submit. `tmux-router::Tmux::send_keys()` now normalizes trailing line endings away and, for ASCII command payloads such as routed reopens and `/clear`, emits one `tmux send-keys -H ... 0d` command so the pane receives the full text plus carriage return as one stream. Non-ASCII payloads keep the old literal-text fallback. Added `tmux-router` regressions for the exact hex command shape and trailing-line-ending normalization, and updated the routing spec so non-supervisor submits keep the same explicit carriage-return contract as supervisor IPC.
- **Authoritative actor reroutes now queue one prompt-bearing reopen even while the supervisor still reports `starting` or `busy`.** The latest JetBrains `Run Agent Doc` repros in `tasks/buildparty-investor-demo/dev.md` and `tasks/agent-doc/agent-doc-bugs2.md` were failing earlier than the existing busy-pane optimism ladder: once route resolved a healthy authoritative actor, `route.rs` still hard-bailed on supervisor states `starting` and `busy` before it ever tried the existing optimistic dispatch behavior. That meant a live pane that was still accepting keystrokes could reject reroutes with `route will not inject a new trigger because the authoritative actor is busy` even though the supervisor IPC path was available. `route.rs` now allows one optimistic supervisor-IPC reopen for prompt-bearing reroutes while the authoritative actor is `starting` or `busy`, while keeping `waiting_input`, `blocked`, and `closed` fail-closed. Added authoritative-actor regressions for both the busy and still-starting cases, and updated the routing/session-actor specs to document the queue-first boundary.

- **Authoritative actor reroutes now compare canonical harness identities instead of raw supervisor binary labels.** The latest `tasks/buildparty-investor-demo/dev.md` JetBrains repro was not a stale actor record: the durable store correctly recorded `harness: claude-code`, but `route.rs` still compared that value against the live supervisor binary name `claude` and failed closed with `bound to harness claude-code, not claude`. Route now normalizes the live harness into the same canonical identity set used by `.agent-doc/session-actors.json` before validating the authoritative actor record, and added a focused regression proving a healthy Claude-owned actor remains dispatchable through the authoritative route path. Updated the routing/session-actor specs so `claude` vs `claude-code` stays an aliasing detail instead of a routing failure.

- **Supervisor IPC reroutes now normalize submit newlines to carriage return before writing to the managed PTY.** The latest JetBrains `Run Agent Doc` repro in `tasks/agent-doc/agent-doc-bugs2.md` was not just a stale-busy route policy issue: authoritative-actor reroutes and other supervisor IPC inject paths were still forwarding `...\n` verbatim, while the local auto-trigger path already used a carriage-return submit. In raw managed Codex/Claude sessions that let the routed reopen draft a literal newline into the composer instead of acting like Enter, which then left the actor stuck in `Busy` and caused follow-up reroutes to fail closed against the same pane. `start.rs` now normalizes supervisor-injected submit bytes (`\n` and `\r\n`) to `\r` before writing to the child PTY, and added regression coverage around both auto-trigger and IPC inject behavior. This closes the latest JB-plugin routed-submit failure from `tasks/agent-doc/agent-doc-bugs2.md`.

- **Phase-9 verification now locks the single-owner actor contract into both regressions and plugin diagnostics surfaces.** `session_actor.rs` now explicitly rejects stale generation/session updates in unit coverage, preserving the monotonic actor-store boundary after the phase-8 ownership cleanup. The editor specs now require plugin verification for exact `session status` display, actor-backed `session clear` wiring, and durable stage-specific route-dispatch failures. VS Code now mirrors the JetBrains durability expectation by writing routed dispatch failures into a dedicated output surface instead of only a transient toast, while JetBrains unit coverage now proves the session-status and `session clear` command wiring helpers directly. This closes `#sgown9` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Phase-8 now removes legacy owner election from the normal route/start/sync path.** `route.rs`, `start.rs`, and `sync.rs` now treat the authoritative actor record plus the supervisor-backed registered binding as the only normal-path ownership inputs. Latest-open session-log panes, `session_end origin=registry_rebind ... next_pane=...` successors, and generic same-file process-tree matches still surface as diagnostics and explicit repair signals, but they no longer let a stale pane silently reclaim authority or get re-registered during ordinary reroute/sync work. Passive sync now blocks on that legacy associated-pane evidence instead of auto-recovering it, and the route/start regressions now distinguish direct stale-registry state from authoritative actor-backed handoffs. This closes `#sgown8` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Phase-7 now keeps sync repair behind explicit repair commands instead of mutating tmux/session state on the normal path.** `sync.rs` no longer runs hidden `repair_layout(...)` passes or closeout replay when it notices a missing pane during ordinary sync. Instead, normal sync captures diagnostics, records the session-loss evidence, and fails closed with an explicit repair instruction whenever stash/window drift or an open `preflight_started` / `response_captured` / `write_applied` cycle would have required repair. The corresponding repair work now lives on explicit surfaces: `agent-doc repair <FILE>` still owns document-cycle recovery, and `agent-doc session doctor <FILE> --repair` now also runs the file-scoped layout/missing-pane repair helpers before re-reporting status. Added sync regressions for the new inspect-only boundary and updated the tmux/session-actor specs. This closes `#sgown7` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Forwarded `Ctrl+D` no longer has a committed-turn keepalive exception.** The old pane-retention hardening still left a committed-cycle `Ctrl+D` policy branch and closeout probe in `start.rs`, even though the user-facing contract had already moved back to "show the quit menu." `start.rs` now removes that lingering `ctrl_d_committed_cycle_restart_fresh` policy path entirely, so stdin-forwarded EOF/Ctrl-D always reaches the canonical `Enter`/`q` prompt, even immediately after a successful document cycle. The obsolete committed-cycle settle probe/tests are gone, and the README/spec/internal guidance now matches the actual behavior again. This closes the latest follow-up in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Forwarded Codex `Ctrl+C`/`Ctrl+D` now always surface the quit menu instead of silently chaining fresh restarts.** The latest `agent-doc-bugs2.md` repro was two separate policy bugs in `start.rs`: stdin-forwarded `Ctrl+C` was still classified through `CrashPolicy` as a transient non-zero exit before the quit-menu override could run, and stdin-forwarded `Ctrl+D` still short-circuited to `RestartFresh` whenever the previous run had already committed or had exited before surfacing a prompt. `start.rs` now treats a forwarded operator `Ctrl+C` as clean-exit policy input for supervisor bookkeeping, and any forwarded operator `Ctrl+D` or terminating `Ctrl+C` now routes to the canonical `Enter`/`q` prompt regardless of committed-cycle provenance. Only genuinely promptless clean exits without a forwarded operator key still auto-recover. Added start-level regression coverage and updated the Codex/supervisor docs. This closes the latest `Ctrl+C`/`Ctrl+D` restart loop in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Supervisor quit prompts now force a canonical local tty mode so Enter works in managed Claude/Codex sessions again.** The latest `agent-doc-bugs2.md` repro was not another restart-policy misclassification: the quit menu itself still used `read_line()` after restoring whatever stdin termios the parent harness originally gave `agent-doc`, and some managed binding sessions left that inherited tty raw-ish enough that `Enter` arrived as literal `^M` bytes instead of terminating the prompt read. `start.rs` now derives an explicit canonical prompt mode from the saved tty state before every restart/quit menu, re-enabling `ICANON`, `ECHO`, signal handling, and `ICRNL`/newline output for the local supervisor prompt without changing the raw child-forwarding path. Added a start-level regression around the prompt termios normalization and updated the supervisor/Codex docs. This closes the latest Enter-key quit-menu regression in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Editor popup numbering now reserves the primary digits for active document flow instead of low-frequency recovery actions.** JetBrains and VS Code now put `Compact Exchange` and `Restart Supervisor Process` in the primary numbered popup, while `Run with Junie` and `Force Claim for Tmux Pane` remain available from a non-numbered overflow path. The binary also exposes the explicit `agent-doc session restart-supervisor <FILE>` surface (with `session restart` kept as a compatible alias) so both plugins call a clearly named supervisor restart API instead of a vague session label.
- **Phase-6 actor operator commands and editor controls now route through one authoritative session surface.** `agent-doc session` still keeps the existing tmux-session pinning flow (`session`, `session set`, bare `session clear`), but it now also exposes actor-backed `status`, `history`, `attach`, `restart`, file-scoped `clear`, and `doctor` commands. Those commands read the durable actor record, session log, startup-miss marker, and supervisor IPC state instead of inventing separate tmux heuristics. JetBrains and VS Code now surface the same shared controls for Show Session Status, Restart Session, Clear Session Context, and Copy Session Diagnostics, keeping the operator UI aligned with the single-owner actor model.

- **Codex stdin-forwarded Ctrl+C now restores the supervisor quit menu instead of looking like a crash.** The current `agent-doc-bugs2.md` repro was not another generic restart-policy failure: `start.rs` already handled stdin-forwarded EOF/Ctrl-D on the clean-exit path, but a live pane `Ctrl+C` still arrived as `exit_kind=signal exit_signal="Interrupt"` and fell through `CrashPolicy` as a transient non-zero exit. That made the supervisor auto-restart after two seconds instead of offering the cooked-mode `Enter` / `q` choice. `start.rs` now tracks stdin-forwarded `Ctrl+C` explicitly, prompts only when that forwarded byte actually terminated the Codex child, and leaves route/plugin-injected interrupts on the existing automatic recovery path. Added start-level regression coverage for the new forwarded-interrupt classifier and quit-menu branch. This closes the latest `Ctrl+C` quit-menu regression in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Phase-1 single-owner session actor semantics are now pinned in spec and emitted in session logs.** `agent-doc` now documents a stable session-actor contract in `specs/08a-session-actor-contract.md` and starts writing monotonic ownership-generation provenance to `.agent-doc/logs/<session>.log`. Fresh `start` generations record `ownership_transition ... prior_generation=... new_generation=...`, and registry handoffs now include the same generation metadata on the transition, supersession, and `session_end origin=registry_rebind` lines. Legacy logs still infer generation count from repeated `session_start` events for compatibility, but new paths now emit explicit generation fields that later actor-store phases can consume without re-deriving ownership history from tmux heuristics.

- **Codex keepalive EOF/Ctrl-D once again restores the supervisor quit menu on the normal path.** The local `#ctrldmenu` regression in `tasks/agent-doc/agent-doc-bugs2.md` was caused by an over-broad keepalive hardening: `start.rs` treated every forwarded stdin EOF/Ctrl-D as `RestartFresh`, which removed the cooked-mode `Enter`/`q` decision path even when the child had already shown a real prompt and the operator was intentionally trying to quit. `start.rs` now only keeps the restart-fresh exception for the two existing fail-closed cases: child runs that already committed a document cycle, and fresh/fresh-restart runs that clean-exit before surfacing a prompt. Ordinary Codex keepalive Ctrl-D exits return to the quit menu again, while the remaining resume-failure prompt still treats prompt-time EOF as `restart fresh` rather than `quit`. Added start-level regression coverage for the restored split strategy. This closes `#ctrldmenu` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Dispatch-only Codex reroutes now follow same-file restart handoffs before surfacing a stale `still booting` error.** The busy-pane recovery path could already trigger a fresh supervisor restart, but the final `dispatch_only_send_reopen(...)` probe still treated the original pane as fixed and failed closed after the first 2s ready wait. In the live JetBrains `Run Agent Doc` repros for `tasks/claudescore-3.md` and `tasks/monsterrodholders.md`, that surfaced `dispatch-only codex reopen refused ... still booting` even when the supervisor had already rebound the same document session to a fresh pane or started a newer generation on the same pane moments later. `route.rs` now gives that boot-window timeout one bounded recovery pass: it watches the session log + same-file registry entry for a newer open generation, retries the same pane when a fresh start generation appears there, and follows an alive same-file successor pane when the supervisor hands the session off. Added route-level regression coverage for both the same-pane restart and same-file handoff decisions. This closes the latest JB-plugin `Run Agent Doc` false-refusal shape from `tasks/agent-doc/agent-doc-bugs2.md`.

- **Normal tmux turn paths now fail closed instead of killing panes or manufacturing duplicate stash fallbacks.** `start.rs` no longer auto-focuses, restarts, or supersedes another alive pane for the same document during ordinary `agent-doc start`; it now errors with explicit tmux inspect/capture/kill commands so the user chooses the winner manually. `route.rs` also dropped the "create then stash" fallback branches that could proliferate hidden duplicate panes when `split-window` failed or when an `agent-doc` window already existed without a safe registered anchor. On the sync side, ordinary missing-pane recovery now keeps dead panes retained for diagnostics instead of calling `tmux kill-pane(...)`; only explicit repair flows remain allowed to clean panes up destructively. Added regressions for the new start/route error surfaces and the retained-dead-pane sync path.

- **Dispatch-only reroutes now refuse to transiently rebind another file's pane before readiness checks finish.** `route.rs` was still calling `register_dispatch_target(...)` before it had proven the candidate pane was safe to reuse for the requested file. In the live `#jbpdrop` repro, that let `tasks/software/tsift.md` briefly emit `session_superseded old_pane=%177 new_pane=%169` even though `%169` was the authoritative `tasks/agent-doc/agent-doc-bugs2.md` Codex pane, creating exactly the post-success pane-theft churn the user observed. Route now validates that an existing dispatch target is either already registered for the requested file or currently unbound before any re-register happens, and it fails closed on cross-file reuse instead of emitting a temporary `registry_rebind` that later has to be undone. Added a regression that proves the original file keeps `%169` while the requesting file keeps `%177`.

- **Committed Codex keepalive restarts now discard inherited pre-prompt `Ctrl-D` bytes instead of letting the fresh successor quit itself.** The earlier pane-retention change correctly flipped committed `Ctrl-D` exits from `prompt_user` to `restart_fresh`, but the immediate successor run could still inherit the same raw `Ctrl-D` byte before it ever surfaced a prompt. In the live `monsterrodholders.md` repro that produced `ctrl_d_committed_cycle_restart_fresh`, then a second clean exit with `ctrl_d=true`, `ctrl_d_prompt_user`, and `user_quit_after_ctrl_d` on the successor pane. `start.rs` now suppresses stale pre-prompt `Ctrl-D` bytes only for that keepalive successor, while still forwarding fresh `Ctrl-D` normally once the child has shown a real prompt. Added start-level regression coverage for the byte-filter helper. This closes `#kpane` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Dispatch-only route now refuses to inject into a pane whose latest run is still in the fresh-start boot window.** `route --dispatch-only` intentionally skips the heavier ack/auto-fix machinery, but it was still treating any alive registered pane as immediately injectable. In the live `monsterrodholders.md` churn, that allowed a bare reopen to be sent to pane `%175` even though its latest session-log run was still just `codex_start mode=fresh` with no ready prompt yet, which made the follow-up route path look accepted right before later missing-pane churn rebound the owner to `%176`. Dispatch-only route now does one short ready probe when the latest open session-log run for that pane is still at its start event with no committed cycle yet; if the prompt never becomes dispatch-ready in that window, route fails closed instead of sending the reopen into a still-booting pane. Added a tmux-backed regression for the new guard. This closes `#mrhroute` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Live `registry_rebind` successors now remain authoritative even when PID/process-tree provenance drifts.** `sync.rs` already used `session_end origin=registry_rebind ... next_pane=...` to block passive cold-start only while the successor pane was still alive, but the main live-owner proof path still ignored that same tmux/session-log handoff evidence once the successor pane became the registered owner. If supervisor PID or process-tree matching changed after the handoff, sync could downgrade the still-live successor to `registered_pane_unowned` and start replacement/recovery churn again. Live-owner recovery now accepts an alive rebind successor before falling back to generic same-file process-tree matches, so pane continuity follows the tmux handoff itself instead of requiring stale PID identity to survive. Added regressions for direct live-owner reuse plus registered-pane proof on a rebind successor.

- **Passive sync now ignores stale `registry_rebind` closeouts once their successor pane is gone, while still honoring a live handoff pane.** `sync.rs` previously treated any latest `session_end origin=registry_rebind ...` as a permanent `--no-autostart` blocker, even after the recorded successor pane had died or drifted away. That stranded mixed-root documents like `src/boost-client/tasks/monsterrodholders.md` until a full autostart cycle recreated them, which in turn made later reconciles look like arbitrary pane replacement. Sync now recovers an alive rebind successor as an ownership proof source, and it only blocks passive cold-start while that successor pane is still alive and rooted to the same document. Added regressions for live-successor recovery plus stale-successor passive reopen.

- **VS Code split-layout sync now preserves editor groups instead of flattening every visible markdown tab into one tmux column.** The extension was still building `agent-doc sync --col a,b,c` for both manual sync and automatic tab-sync, even when the user had separate visible editor groups. In narrow shared `agent-doc` windows that let tmux-router reinterpret a side-by-side layout as one stacked column and stash a healthy running pane during passive reconciliation. The VS Code extension now emits one `--col` per visible editor group, keeps empty split placeholders so non-markdown side panes do not collapse column identity, and makes tab-sync dedup/signatures track column structure instead of just the flat file set. Added TypeScript regressions for split columns, placeholder columns, and split-with-one-markdown tab sync. This closes the latest `claudescore-3.md` passive-stash gap from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Codex now treats bare SSH `socket: Operation not permitted` output as required-SSH capability drift when the command context proves the target.** The previous resumed-session detector only matched transcript lines that already contained the required alias/host term, so a Codex `command_execution` event like `command: "ssh monsterrodholders-server true"` with `aggregated_output: "socket: Operation not permitted"` leaked through as a raw task failure and skipped the fresh-retry path. `agent/codex.rs` now inspects command-execution context: if the command itself proves SSH against a declared `required_ssh_targets` entry, bare socket EPERM output still triggers the existing one-time fresh retry and then fail-closed required-SSH error path. Added direct detector coverage plus blocking/streaming regressions, while keeping localhost/CDP EPERM on its separate capability-drift path. This closes `#ssheperm` in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Committed routed Codex runs no longer close their tmux pane just because `Ctrl-D`/stdin EOF was forwarded during the child run.** The live `monsterrodholders.md` repro was no longer a sync/rebind ownership bug: pane `%166` completed `commit_success`, then `start.rs` saw `ctrl_d_forwarded`, dropped into the quit prompt path, and logged `user_quit_after_ctrl_d`, which closed the still-healthy claimed pane immediately after a successful document cycle. The supervisor now inspects the latest session-log run before applying the Ctrl-D clean-exit policy. If that run already recorded a committed `document_cycle`, Codex restarts fresh and keeps the pane attached instead of offering the quit prompt. Added session-log parsing coverage for committed-cycle detection plus start-level regression coverage for the new restart-fresh branch. This closes the latest `monsterrodholders.md` pane-drop from `tasks/agent-doc/agent-doc-bugs2.md`.
- **JetBrains route failures now stay copyable after the first notification moment.** `TerminalUtil.sendToTerminal()` was still collapsing `agent-doc route --dispatch-only` failures into a plain IDE error notification, which made startup-miss and pending-drift diagnostics effectively transient when the user launched `Run Agent Doc` from the plugin. JetBrains now persists the exact route output under `.agent-doc/state/editor-route-errors/`, marks the failure notification as important, and adds copy/open actions so the original binary-owned error remains available without paraphrasing. Added Kotlin unit coverage for the saved diagnostics path and exact-output persistence. This closes `#jberr` in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Optional closeout sidecar reads now treat late `ENOENT` as absence instead of a hard failure.** `session-check` and the closeout helpers were still using `exists()`-then-`read()` for cycle-state, capture, startup-miss, ops-log, pre-response, and CRDT sidecars. Under full-suite tempdir churn, that left a narrow race where a sidecar could disappear after discovery but before the read, bubbling `No such file or directory (os error 2)` out of otherwise-valid closeout checks such as `session_check_skips_pending_done_warning_when_id_was_recorded`. Optional sidecar loads now read directly and downgrade only `NotFound` to `None`, preserving other I/O failures while eliminating the transient `ENOENT` flake. Added unit coverage for the shared optional-read helper. This advances `#sceno` in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Sync now prefers the newest open session-log pane over stale same-file process-tree matches during live-owner recovery.** `sync.rs` already accepted generic `agent-doc`/harness argv matches as a fallback ownership hint, but it checked that process-tree evidence before the latest open session-log owner. In the live `monsterrodholders.md` reroute loops, that let an older pane that still had a same-file Codex process win back ownership immediately after a fresh replacement pane had already recorded the newest `session_start`, which in turn caused `registered_pane_missing` on the fresh pane and rebound the registry to the stale pane. Live-owner recovery now checks path provenance, supervisor identity, and the newest open session-log owner before generic process-tree matching, so a fresh pane that has already started the latest run stays authoritative unless stronger cross-file proof says otherwise. Added a tmux-backed regression for the stale-process-tree vs fresh-session-log conflict. This advances `#mrreap` in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Fresh routed auto-starts now keep the fresh pane authoritative instead of handing dispatch back to an older same-session pane during boot.** `route.rs` was still re-reading `sessions.json` after the fresh-pane ready wait and would follow any concurrent same-session rebind back to an older pane, even when that rebind came from a layout/sync race rather than real ownership proof. In the live JetBrains `agent-doc-bugs2.md` repro this surfaced as `fresh_route_dispatch_handoff ... fresh_pane=%144 dispatch_pane=%127`, immediately superseding the new pane inside the same `agent-doc` window and making the completed run look like it had disappeared. Fresh-route dispatch now re-registers and uses the pane it just created unless that pane is cross-file invalid, so post-start geometry churn cannot steal the reroute away from the new pane. Added a regression that forces a competing registry rebind during boot and proves the fresh pane still receives the reopen and remains authoritative. This advances `#jbpdrop` in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Split the command spec monolith into focused sibling specs and added a reusable split runbook.** `specs/07-commands.md` is now the stable command-spec index, while the normative detail moved into `specs/07-core-commands.md`, `specs/07-session-tmux-commands.md`, `specs/07-closeout-commands.md`, and `specs/07-orchestration-commands.md`. Added `runbooks/split-spec-files.md`, bundled it into installed harness runbooks, and documented the stable-index split rule plus the managed-vs-custom ownership boundary in `CLAUDE.md` / `README.md`.
- **Sync now fail-closes when an alive pane is still the latest open session-log owner, instead of fabricating `registered_pane_missing`.** `sync.rs` already refused to reuse an alive registered pane without live-owner proof, but it could still fall through to `repair_missing_registered_pane(...)` immediately afterward and synthesize pane loss even when the session log still showed that same pane as the newest open run. That was enough to orphan live `monsterrodholders.md`/mixed-root panes after a routed reopen or post-success restart window. Sync now treats that shape as a fail-closed ambiguity window, records explicit `registered_pane_open_session_log_owner ... action=fail_closed` provenance, and blocks replacement for the cycle instead of rebinding over the pane. Added regression coverage for the new session-log-owner guard.
- **Sync now fail-closes when an alive Codex pane still has drafted input, instead of logging synthetic pane loss and rebinding over it.** `sync.rs` already required live-owner proof before trusting an existing registered pane, but an alive pane that temporarily lost that proof could still fall through to `repair_missing_registered_pane(...)`, record synthetic `registered_pane_missing`, and provision a replacement even while the Codex composer still held live drafted input. Sync now reuses the shared harness prompt parser to detect protected Codex composer/search states, records explicit `registered_pane_protected ... action=fail_closed` provenance, and blocks replacement for that cycle instead of emitting `session_end origin=sync_missing_pane`. Added harness/sync regression coverage for drafted prompts, queue-state protection, and idle-placeholder non-matches. This advances `#prreap` in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Route now derives its tmux session from the requested file/layout roots instead of only the launcher CWD.** `route.rs` now reuses the same root-aware session chooser as `sync`: a nested-repo `agent-doc route` without explicit window context honors the target file's own nearest `.agent-doc/config.toml` pin, and a mixed-root editor layout prefers the shared workspace-root pin over the focused child root. This prevents JetBrains `Run Agent Doc` from auto-starting nested documents into the wrong submodule session when the visible split already proves a shared workspace `agent-doc` window. Added route regressions for both the single nested-file and mixed-root layout cases, and updated the routing/editor specs.
- **Passive editor sync now favors a fast pane handoff before the heavier ownership-recovery machinery runs.** `sync.rs` now lets `agent-doc sync --no-autostart` reuse the latest matching session-log pane immediately, fall back to an alive registered pane rooted to the same document when no direct match exists, and cold-start a fresh pane right away when the document has no matching or exclusive registered owner. This removes the slow process-tree/supervisor scan from the common editor-selection path while keeping the heavier recovery logic for non-happy-path cases. Added a regression covering alive registered-pane reuse on the passive path.
- **Sync now refuses to treat an unrelated live pane as a document owner, and passive/fail-closed files no longer borrow spare panes during reconcile.** `sync.rs` now requires a registered pane to still prove live ownership before reusing it, so a merely alive pane cannot satisfy another same-root document just because the registry drifted. When recovery or `--no-autostart` intentionally leaves a managed file unresolved, agent-doc now tells `tmux-router` not to donate a same-column or spare visible pane to that file, preventing `tasks/software/tsift.md` and similar selections from reusing the old `agent-doc-bugs2.md` pane. Added regressions for unowned-alive pane rejection and the safe-passive no-alias path.
- **Safe passive mixed-root sync now preserves the existing visible tmux layout when a blocked file cannot be provisioned.** The earlier no-alias guard stopped `sync --no-autostart` from donating a spare pane to the blocked file, but the reconcile phase could still collapse the shared `agent-doc` window down to whichever foreign pane remained resolved, effectively making that foreign pane authoritative anyway. `sync.rs` now short-circuits before tmux-router reconciliation whenever passive sync leaves any visible file blocked, so the current live panes stay visible and the binary emits a warning instead of stashing the workspace pane out from under the user. Added regression coverage for the preserved-layout path and updated the session/tmux sync spec. This closes the remaining `#jbsubroot` mixed-root passive-sync replay from `tasks/agent-doc/agent-doc-bugs2.md`.
- **`agent-doc sync` now reuses the recorded layout by default and re-normalizes tmux windows after reconcile.** `sync.rs` now reads and writes `.agent-doc/last_layout.json` from the resolved sync scope instead of blindly anchoring it to the caller's CWD, and a no-`--col` `agent-doc sync` replays that saved layout as its default input. Sync also runs `repair_layout` again after `tmux_router::sync`, then pushes `agent-doc` back to index `0` and stash windows directly after it so post-reconcile pane mutations do not leave the tmux window order drifted. Added regressions for recorded-layout fallback, shared-root layout-state scoping, stash-window index normalization, and tmux-router overflow-stash discovery.
- **Windowless mixed-root sync now stays on the shared workspace tmux session, and stash rescue no longer leaks panes into the caller's current session.** `sync.rs` now derives its session pin from the visible document set's shared `.agent-doc` root before consulting the ambient tmux client, so alternating focus between workspace-root and child-root documents can no longer ping-pong the same layout between session `4` and session `1`. On the tmux side, `tmux-router` now targets the source pane's own session when breaking a stashed pane into a new window, closing the exact bug where rescuing a pane from session `4` stash could recreate `agent-doc` under the currently attached session `1`. Added regressions for the mixed-root windowless sync selection, the CWD-independent rescue path, and the tmux break-pane session-preservation contract.
- **Editor tab-sync now suppresses JetBrains/VS Code split-layout bounce-back.** In split editor layouts, JetBrains fires a spurious `selectionChanged` for the other split's file ~1 second after the user navigates to a file, causing the tmux pane focus to bounce back. Both plugins now track the pre-command focused file and suppress re-focus events that target that file within a 1.5-second settle window after a successful focus/sync command. Added `BounceBack` classification to both planners, regression tests for bounce-back suppression and expiry, and updated the session-routing spec.
- **Missing-pane sync recovery now fails closed when closeout replay itself needs manual repair.** `sync.rs` already tried `repair()` before starting a replacement pane, but a replay failure such as `pending/backlog patch changed non-list content` still only logged a warning and then fell through to auto-start. Sync now treats that shape as a deterministic repair-needed state: it records the missing-pane provenance, preserves the durable closeout capture, and skips replacement auto-start until the user repairs the document. Added a regression for the `response_captured` + unsupported backlog patch shape behind the latest `monsterrodholders.md` churn.
- **Busy-route progress logging no longer panics on Unicode prompt lines during live reroutes.** `route.rs` was trimming the "Still waiting for ..." tmux status line with a raw byte slice, which panicked as soon as a captured Codex prompt/status line included multibyte glyphs such as the ellipsis in `~/.../boost-clien…`. Route now truncates those diagnostics on char boundaries, and a regression locks the live `monsterrodholders.md` reroute shape that previously crashed in the busy-pane replay.
- **Passive `sync --no-autostart` can now cold-start only after it proves there is no live owner left.** The earlier editor-sync hardening correctly stopped passive tab/layout churn from replacing visible panes, but it also left `dev.md`, `claudescore-3.md`, and similar documents stranded after their last pane had already exited cleanly. `sync.rs` now distinguishes "do not replace a live or ambiguous owner" from "never start anything": safe passive sync still runs the full owner-recovery / startup-miss / recent-loss guards, but if no live owner survives and the latest session log is genuinely closed, it may provision a new pane so editor selection brings the document back into the `agent-doc` window. Passive sync still refuses that cold-start when the latest closeout is only `session_end origin=registry_rebind`, because that shape means a newer pane era may still own the document elsewhere. Added passive-autostart guard coverage plus updated command/editor specs.
- **`claim` now treats normalized registry keys as document identity instead of mistaking them for session UUIDs.** The session registry is keyed by canonical absolute file path, but `claim.rs` was still comparing that key directly to the current document's `agent_doc_session`. That made a document's own live pane look like a foreign claim and caused `Claim for Tmux Pane` on submodule-backed files such as `monsterrodholders.md` to provision a duplicate pane instead of reusing `%75`. `claim` now recognizes same-document ownership by canonical document identity, improves the conflicting-claim log label, and applies the same canonical matching when clearing stale claims. Added regressions for both normalized-registry-key and relative-entry-file shapes.
- **Codex Stop-hook captures now normalize safe backlog patches before durable replay.** Replayable template closeouts that include a `patch:backlog` block no longer persist the raw backlog patch into the pending/capture ledger. The capture path now applies the same safe backlog normalization used by the write pipeline first, strips the backlog patch from the stored response body, and leaves recovery to replay only the exchange-safe payload. This closes the latest `monsterrodholders.md` pane-loss shape where `sync_missing_pane_closeout_recovery` failed on `pending/backlog patch changed non-list content` and then replaced the running pane anyway.
- **Split-layout tab selection now stays on non-destructive sync instead of plain focus.** The earlier shared JetBrains/VS Code tab-sync contract still treated any unchanged visible markdown set as a pure `agent-doc focus <file>` move, which could leave a selected document stranded in `stash` and therefore missing from the visible `agent-doc` tmux window. Both plugins now keep multi-document visible layouts on `agent-doc sync --no-autostart ...` even when only the active tab changes, while single-document tab switches still use `focus`. Added Kotlin and TypeScript regressions plus updated editor specs.
- **Codex can now require and prove SSH capability before trusting resumed sessions.** Documents may declare `required_ssh_targets` in frontmatter, and the Codex backend now probes those SSH targets before launch. When a resumed Codex session later surfaces a target-specific SSH failure, agent-doc treats that as capability drift, retries once with fresh `codex exec`, and then fails closed if the required SSH capability still cannot be proven. Added frontmatter round-trip coverage plus Codex backend regressions for alias/config degradation and SSH-triggered fresh retry.
- **JetBrains and VS Code tab-selection sync now share the same non-destructive focus contract.** Both editor plugins now distinguish pure active-file changes from visible-layout changes: a tab switch with the same visible markdown set issues `agent-doc focus <file>`, while any visible-set change issues `agent-doc sync --no-autostart ...` instead of an autostart-capable sync. JetBrains no longer routes tab selection through provisioning sync from the focused root, and VS Code no longer treats every tab change as a layout sync. Added focused Kotlin and TypeScript regression coverage plus updated the JetBrains tab-sync spec.
- **Codex Stop-hook closeout now salvages valid template patchbacks even when `last_assistant_message` includes plain progress commentary ahead of the patch body.** The latest direct `agent-doc <FILE>` BuildParty repro still reached the Stop hook, but replay failed closed because the final assistant payload mixed two ordinary progress lines with a valid `patch:exchange` + `patch:backlog` closeout. `replay_guard.rs` now treats the narrow "plain prose prefix, then clean patch suffix" shape as recoverable by stripping the prefix and replaying only the patch body, while still blocking transcript markers, structured unmatched text, trailing/interstitial unmatched content, and full component dumps. Added replay-guard coverage plus a Codex Stop-hook regression that proves the sanitized patch replay commits cleanly without leaking the commentary into the document. This closes `#adinv2` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **JetBrains cross-root sync now keeps workspace column memory even when focus moves onto an unmanaged nested-root markdown file.** The plugin had still chosen the focused file's nearest `.agent-doc` root as the `agent-doc sync` working directory, so focusing `src/agent-doc/specs/08-session-routing.md` while `tasks/agent-doc/agent-doc-bugs2.md` and `src/boost-client/tasks/monsterrodholders.md` shared the screen made sync read/write `src/agent-doc/.agent-doc/last_layout.json` instead of the workspace root state that remembered the left pane. JetBrains sync now uses the single visible root when all visible markdown files belong to one agent-doc root, but falls back to the workspace root `.agent-doc/` whenever the visible layout spans multiple roots. Added unit coverage for the single-root and cross-root root-selection cases, and updated the JetBrains/plugin specs.
- **Windowless sync now honors the live project tmux-session pin before inheriting the caller's attached session.** `sync.rs` had drifted from the documented session-resolution contract and was resolving its target session as `--window -> current session`, which let an attached session like `1` take over even while `.agent-doc/config.toml` still pinned a live session `0`. Sync now shares the same precedence contract as route: explicit window/session context first, then live project `tmux_session`, then current session, with harness fallback remaining route/start-only. Added route + sync regressions for the live-pin and dead-pin cases, and expanded the session-routing spec with an editor-to-tmux truth table covering `agent-doc`/`stash` outcomes.
- **JetBrains cross-root split reporting no longer drops the outer markdown pane when focus moves into a nested submodule.** The plugin was still filtering visible markdown files to the focused file's resolved root before building `sync` and routed layout hints, so a workspace-root + submodule split could oscillate between the correct two-column absolute layout and a one-column `monsterrodholders.md` report. That one-column report let `agent-doc sync` legitimately stash the other pane, which in turn fed later session-drift / stale-session cleanup noise. JetBrains now preserves all visible markdown files as absolute paths across both sync and route layout reporting, keeps empty columns for mixed splits, only rewrites submodule-local workspace-relative paths when needed, and bumps the plugin build version to `0.2.88`. Added unit coverage for cross-root sync normalization, visible-file collection, and routed layout arg generation.
- **Sync now treats the latest open session-log pane as fail-safe live-owner proof before replacement.** `sync.rs` no longer limits associated-pane recovery to argv/process-tree or supervisor-socket evidence. When a managed document's latest session log still shows an open pane, and that pane is still tmux-alive in the same project root, sync now accepts it as an ownership proof source, re-registers it through the shared associated-pane path, and only considers `registered_pane_missing` replacement after that fail-safe proof is exhausted. Added tmux-backed regressions for direct live-owner reuse and associated-pane recovery via session-log provenance. This advances `#ownergap` in `tasks/agent-doc/agent-doc-bugs2.md`.

- **Missing-pane sync recovery now reopens stranded closeouts before it starts another pane.** `sync.rs` already logged `registered_pane_missing` / dead-pane provenance, but it only self-healed stale `preflight_started` locks. If the owning pane disappeared after `response_captured` or `write_applied`, the durable capture could stay stranded until a later manual/preflight recovery. Sync now attempts the same binary-owned recovery path immediately on pane loss: `response_captured` replays through `repair` + strict closeout, `write_applied` finishes the missing commit boundary when the file/snapshot already prove the response landed, and the session log records explicit `sync_missing_pane_closeout_recovery_*` provenance before replacement starts. Added sync regressions for both the replayed-response and already-applied commit-boundary shapes. This advances `#jbcap` in `tasks/agent-doc/agent-doc-bugs2.md`.
- **JetBrains passive sync no longer reads `.agent-doc/sessions.json` or live tmux state to decide window/autostart policy.** The plugin now reports only absolute layout/focus file paths to `agent-doc sync`, preserves empty column placeholders for mixed markdown/non-markdown splits, and claim / force-claim no longer inject a plugin-chosen `--window`. This removes the Kotlin-side duplicate ownership heuristic so passive autostart, ambiguous-owner fail-closed behavior, cross-root tmux targeting, and remembered two-pane restoration live solely in the Rust binary. Added JetBrains unit coverage for absolute-path sync command generation and bumped the plugin build version to `0.2.87`.
- **Editor-driven tmux sync now has a non-destructive mode, and JetBrains startup no longer auto-runs `resync --fix`.** `agent-doc sync` now accepts `--no-autostart`, which keeps reconciliation/layout updates from auto-starting replacement sessions when pane ownership is uncertain. JetBrains automatic layout listeners, claim follow-up sync, and VS Code's editor-driven sync paths now use that mode so passive editor activity cannot replace a visible pane just because startup/restore briefly lost ownership proof. JetBrains project-open also switched from `agent-doc resync --fix` to a report-only `agent-doc resync` audit, shrinking plugin-triggered tmux close/replacement surface area down to explicit recovery paths such as duplicate/stash cleanup inside the CLI itself. Added JetBrains unit coverage for the non-autostart sync command and the non-destructive startup audit contract. This addresses the latest `#jbptrk` guidance in `tasks/agent-doc/agent-doc-bugs2.md`.
- **JetBrains repeat `Run Agent Doc` clicks now supersede stale plugin-spawned route processes instead of waiting behind them.** The editor `SubmitAction` already stopped inferring "already running" from local state, but a previous `agent-doc route --dispatch-only` process could still stay alive long enough that the next click felt blocked after a canceled Codex turn and `/clear`. `TerminalUtil.sendToTerminal()` now tracks one in-flight route process per document, terminates the stale process when the user reruns the action, suppresses stale-process failure noise, and immediately launches a fresh dispatch. Added a focused Kotlin unit test and bumped the JetBrains plugin version so the fix is installable. This closes the latest "second Run Agent Doc should immediately resend after `/clear`" report in `tasks/agent-doc/agent-doc-bugs2.md`.
- **JetBrains `Run Agent Doc` is now silent on success and progress instead of emitting any route-side UI hint.** The earlier cleanup removed the dedicated in-flight balloon, but the remaining success/progress hint path was still surfacing as a bottom-right IDE notification for some users. `TerminalUtil.sendToTerminal()` now only logs successful reroutes and reserves JetBrains notifications for real route failures, while the JetBrains editor spec/agent notes now describe the fire-and-forget contract explicitly. This closes the latest "remove the bottom right notification on `Run Agent Doc`" report in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Prompt-bearing diff classification now suppresses stale-boundary raw-answer tails before `preflight`, `plan`, and write-path consumers ever see them.** The earlier `buildparty-investor-demo/repo.md` fix taught `session_check.rs` and routed cycle-ack gating to ignore a stale-boundary prompt that was already followed by plain assistant completion prose (`I updated ...`, follow-up bullets, etc.), but the lower-level `diff.rs` classifier still emitted that same tail as three fresh `prompt_target` blocks. That left `agent-doc preflight` / `agent-doc plan` falsely reopening completed work even after repair had already proven the tail was answered. The shared prompt-bearing classifier now drops answered prompt runs at the source, so `preflight`, `plan`, route/session-check, prompt-prefix normalization, and write-path snapshot decisions all agree on the same actionable tail. Added a regression with the exact `src/session-share/tasks/buildparty-investor-demo/repo.md` stale raw-answer shape.
- **Editor Run now has an explicit dispatch-only route mode instead of layering more busy-session heuristics on top of JetBrains / VS Code hotkeys.** `agent-doc route --dispatch-only` resolves the owning pane, sends the bare `agent-doc <FILE>` reopen, and returns without route-owned startup-miss gating, `/clear` relaunch policy, busy-pane recovery, or cycle-ack waiting. JetBrains `Run Agent Doc` now saves and dispatches immediately with that mode, and VS Code's Run action no longer blocks behind a plugin-local "Command already in progress" gate. Managed `agent-doc route` keeps the existing guarded behavior for CLI callers that still want binary-owned recovery. Added route regressions for dispatch-only busy-pane dispatch and timed-out bare reopen acceptance, and updated the editor specs / README.
- **JetBrains `Run Agent Doc` no longer self-blocks repeat reroutes, and live Codex reroutes now stay optimistic once the correct pane has accepted the bare reopen.** The JetBrains action no longer short-circuits on a stale local "route already in progress" flag, so cancel + `/clear` no longer gets trapped in the editor before the CLI runs. On the backend, `route.rs` still validates the target pane/file binding and still records startup-miss diagnostics, but once a live Codex pane for that file has accepted the bare `agent-doc <FILE>` reopen, missing routed submission proof or missing follow-up cycle-ack no longer fail-closes the reroute. The same optimistic rule now covers the alive-pane busy-session ladder after scoped fix / fresh restart / bounded interrupt recovery, while dead panes still fail closed. Added route regressions for missing-ack, same-cycle committed churn, and alive-busy timeout shapes, plus JetBrains plugin verification. This closes the latest `/clear` reroute blocker from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Routed cycle-ack gating now ignores stale-boundary prompt tails that already have raw assistant completion prose, not just formal `### Re:` blocks.** `session_check.rs` and `route.rs` already shared the "answered prompt below a stale boundary" detector, but it only recognized a later `### Re:` / `## Assistant` marker. The latest JetBrains `Run Agent Doc` failure for `src/session-share/tasks/buildparty-investor-demo/repo.md` hit the older raw-tail shape instead: the stale boundary was followed by the user prompt, then plain assistant completion prose (`I updated ...`) and bullets, so route kept waiting 30 seconds for a ghost `pending prompt_target`. The detector now also treats a narrow set of assistant-style completion lines and follow-up bullets as an answered tail, and new regressions prove both `session-check` and routed cycle-ack gating skip that raw-response shape while still keeping plain unanswered prompts actionable. This closes the latest JB reroute startup-miss false positive from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Busy same-document Codex reroutes now probe `C-g` before the generic interrupt closeout so reverse-history-search panes can recover instead of fail-closing.** `route.rs` already had the bounded same-document interrupt ladder (`Escape` + `C-c`) after scoped fix and fresh restart, but the latest JetBrains `agent-doc-bugs2.md` reroute still stranded the live pane in shell-history search and never reached a dispatch-ready prompt again. The busy-pane recovery path now sends one short `C-g` readiness probe first, immediately reuses the pane when that clears a latent `reverse-i-search` / history-search substate, and only falls back to the existing `Escape` + `C-c` sequence when the probe does not restore readiness. Added a tmux regression that requires `C-g` to recover the live pane and updated the route command spec. This closes the latest JB-plugin reroute failure from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Same-document routed busy-pane failures now preserve the final recovery stage instead of collapsing back to the stale pre-interrupt timeout.** `route.rs` already ran the scoped `agent-doc fix`, one bounded fresh supervisor restart, and one bounded Codex interrupt recovery for the `agent-doc-bugs2.md` `#selfrt` family, but the final fail-closed error still reused the older pre-interrupt `timeout` detail even when the last readiness check had already proven a more specific blocker like `reverse-i-search`. The interrupt recovery path now returns structured `ready / blocked / timed_out / skipped` outcomes, the final busy-session closeout surfaces that bounded-interrupt stage detail directly, and a regression covers the same-document shape where a healthy supervisor stays authoritative but the interrupted pane still lands in `interactive shell reverse-i-search`. This closes `#selfrt` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Repair/Stop-hook closeout can now adopt a visible response patchback even after the replay payload is gone.** `repair.rs` now detects the narrow shape where the live document already contains a fresh `### Re:` / `## Assistant` block that the snapshot lacks, but no pending/capture artifact or replayable `last_assistant_message` survived. Instead of leaving that response as plain working-tree drift that still needs a separate human commit, repair synthesizes the visible response back through the existing already-applied dedup path, advances snapshot + `write_applied`, and lets the normal strict closeout helper commit it. Added repair and Codex Stop-hook regressions for the routed-no-ack / visible-response recovery shape. This closes `#8zjh` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Template write/repair now recover the duplicated `agent:exchange` close-marker shape before failing on a stranded pending response.** `write.rs` no longer stops at the raw `closing marker <!-- /agent:exchange --> without matching open` parser error when a merged template document still has the real exchange opener plus a second escaped close marker after the response tail. The normalization path now detects that exact unmatched-close chain, uses `template.rs` to move the escaped response block back inside the real exchange component, drops the stray duplicate close, and only then re-runs the normal transcript/tail guard. `repair.rs` applies the same canonicalization when fixing no-pending template drift, so the May 1 `claudescore-3.md` `#xguard` family can finish through the binary-owned repair/write path instead of requiring manual response surgery. Added direct template and write-path regressions plus spec updates. This closes `#xguard` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Routed Codex submission-proof gating now respects nearer `.codex` shadowing instead of assuming every workspace-root hook install is reachable from nested repos.** `route.rs` already learned to scan every tracked `.agent-doc` ancestor for `.codex/hooks.json`, but that was still too optimistic for child repos like `src/session-share` when a nearer `.codex` path existed as a file or hookless boundary. In that shape the live Codex pane never emits `UserPromptSubmit` state for the reroute, so waiting for hook-backed submission proof only creates a new false failure after tmux already accepted the bare reopen. Route now only requires hook-backed dispatch-start proof when the rerouted file can actually see that hook install on its own upward `.codex` walk, and a new regression covers the nested-shadowing case. This closes `#cs3intr` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Fresh-restart routed cycle-ack retries now follow authoritative pane handoff before the second reopen, and they fail at the correct stage when the replacement pane never becomes dispatch-ready.** `route.rs` already had a one-shot fresh-restart retry after a live Codex reroute was accepted/consumed but never started a new document cycle, but that retry still waited on and resent into the original pane even if supervisor recovery had already moved the session to a replacement pane. The retry path now re-resolves the authoritative pane after the fresh restart, keeps the original resolved absolute reopen path for the second send, and surfaces a dispatch-readiness failure when the replacement pane stalls in a blocked shell substate instead of misreporting the outcome as another generic "no new cycle started" startup miss. Added a regression that forces the fresh-restart retry handoff to a replacement pane and updated the routing spec. This closes `#rbgap` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Routed Codex submission proof now stays enabled across nested `agent-doc` roots when the workspace-level install owns the hooks.** `route.rs` used the nearest `.agent-doc` root to decide whether hook-backed dispatch-start proof was available, which silently disabled the stronger "submitted/consumed" stage for child repos like `src/session-share` and `src/boost-client` when only the workspace root had `.codex/hooks.json`. Route now scans every tracked `.agent-doc` ancestor for hook installation, matching `codex_hook.rs`'s cross-root state storage, so child-repo reroutes keep the explicit "accepted vs submitted vs consumed" partition instead of collapsing back to the weaker acceptance-only path. Added direct regressions for the nested-root positive and no-hook negative cases, and updated the routing spec. This advances `#rbgap` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Resumed Codex backend turns now auto-discard stale local-browser/CDP capability drift and retry fresh once.** `agent/codex.rs` now watches resumed `codex exec resume <id>` responses for the specific local socket EPERM signature (`Operation not permitted` on `127.0.0.1:9222` / `localhost:9222`). When that appears before a real response lands, agent-doc treats the resume as poisoned capability inheritance, reruns the same prompt once through a fresh `codex exec`, and lets the fresh thread replace the saved `resume` id instead of trusting the stale one again. Added blocking and streaming regressions plus spec updates. This closes `#cxcdp` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Fresh Codex sessions that die before showing a prompt now restart fresh instead of blindly resuming, and fresh-route startup-miss recovery no longer hands dispatch back to the pane it just replaced.** The remaining `claudescore-3.md` `/clear` reroute miss was not in `route.rs`'s path handling; it was in the supervisor clean-exit policy. A fresh/fresh-restart Codex child could exit `0` before ever surfacing an idle prompt, and `start.rs` would still treat that as a healthy clean exit and chain `--continue`, which later collapsed into `auto_trigger_timeout reason=no_prompt_after_30s`. The supervisor now tracks whether the current child ever exposed an idle prompt and treats a promptless clean exit on a fresh run as failed startup provenance, forcing a fresh restart instead of resume. Separately, when route has already deregistered a startup-miss pane and launched a fresh replacement, the post-ready handoff check now carries that replaced pane as explicit blocked provenance instead of relying only on the persisted startup-miss file that was just cleared. That prevents the fresh pane from handing the reopen straight back to the stale owner during startup. Added start/route regressions and updated the supervisor / command specs. This closes `#clrrt` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Fresh-restart routed retries now preserve the original resolved reopen path instead of downgrading to `file.display()`.** `route.rs` already resolved routed triggers to an absolute `agent-doc <FILE>` path on the first send, but the one-shot fresh-restart retry after a missed cycle ack rebuilt the reopen from the caller path and could resend `agent-doc tasks/claudescore-3.md` into a `src/session-share` Codex pane. The retry path now reuses the same resolved absolute file path as the initial dispatch, and added regressions cover both the generic fresh-restart resend and the relative-document submodule shape. Updated the routing spec to make the retry-path invariant explicit. This closes the latest `/clear` reroute miss from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Component parsing no longer panics when the non-agent comment preview lands inside a multibyte glyph.** `component.rs` now bounds its fast-path `<!-- ... -->` preview on UTF-8 char boundaries before checking whether a comment is really an `agent:` marker. That keeps ordinary prose comments near `❯` and other multibyte text on the normal ignore path instead of panicking on a sliced preview, while preserving the existing structured errors for malformed real component nesting. Added a regression for the `#utf8p` repro shape and documented the valid-UTF-8 no-panic invariant in `SPEC.md`.
- **Busy same-document Codex reroutes now get one bounded interrupt recovery before the final fail-closed error.** `route.rs` still refuses to append a bare reopen into a genuinely non-idle pane, but after the normal scoped-fix and fresh-restart ladder is exhausted it now sends one interrupt sequence to the authoritative live Codex pane, waits for a real empty prompt again, and reruns the same bare `agent-doc <FILE>` reopen once before giving up. This keeps routed follow-ups from fail-closing just because the live pane was stranded in a shell substate or other stale busy UI after recovery, without dropping the existing multiline/drafted-composer safety checks. Added a route regression for the interrupt-recovery retry path and updated the routing spec. This closes the latest busy-pane reroute failure from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Routed Codex reruns now fail fast on interactive shell substates and accept broader submission proof for the same document.** `harness.rs` now classifies busy Codex panes by reason, including interactive terminal substates like `reverse-i-search`, so `route.rs` stops burning the full idle wait on panes that can never accept a reroute and immediately falls into the existing scoped-fix / bounded-restart path. On the post-send side, routed Codex proof no longer requires the hook store to echo the exact bare reopen text; any newer tracked prompt state for the same document now counts as submission proof, while an exact prompt match still records the stronger "consumed" stage. That lets route distinguish "drafted", "accepted but no submission proof", "submitted", and "consumed" without collapsing hook races back into false failures. Added harness/route regressions and updated the route command spec. This closes `#snrun` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Routed Codex reopen now proves harness consumption before the later cycle-ack health check, and healthy busy-pane no-op fixes go straight to one fresh reroute.** `codex_hook.rs` now exposes the latest tracked prompt state for a document, and `route.rs` uses that `UserPromptSubmit` hook record as an explicit dispatch-start proof for bare `agent-doc <FILE>` reroutes when Codex hooks are installed. That means route can now fail with stage-specific diagnostics for "still drafted in tmux", "accepted but never consumed by Codex", or "consumed but no document cycle started" instead of collapsing those shapes into the same startup-miss timeout. In the same simplification pass, the no-op same-document busy-pane branch no longer injects into the still-busy pane and only later decides whether to restart; after one scoped fix, a still-healthy authoritative pane now gets one bounded fresh restart and final reroute. Added route regressions for the fresh-reroute path and updated the command spec. This advances `#runsm` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Tracked Codex `/clear` reroutes now restart fresh before dispatch to preserve the original launch policy.** `codex_hook.rs` now exposes the latest tracked prompt for a document and flags an exact `/clear` as a capability-reset marker. Before `route.rs` reuses an otherwise healthy live Codex pane, it now checks that marker and forces one fresh supervisor restart before injecting the next `agent-doc <FILE>` reopen, so the original `codex_args`, writable roots, and network policy are reapplied instead of trusting post-clear resume inheritance. Added hook-level regression coverage for latest-prompt lookup plus a route regression that proves dispatch lands only in the fresh post-clear session. This closes `#clrpr` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Codex busy-pane reroute now gets one fresh-session retry when command acceptance never clears after `/clear`.** `route.rs` already had a no-op same-document busy-pane recovery path for healthy supervisors, but it still fail-closed if the follow-up bare reopen stayed visibly drafted in the pane long enough for `send_command_checked` to time out. The busy same-document Codex branch now performs one bounded fresh supervisor restart, waits for the authoritative pane handoff/readiness, resends the reopen, and still requires the normal routed cycle ack before success. Added a regression that keeps the trigger visibly stuck in the old pane, forces the fresh restart handoff, and proves the routed reopen lands in the replacement pane. This closes the latest `/clear` reroute regression from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Direct `agent-doc run` now reuses the pending/backlog normalization gate from `write` and `repair`.** `run.rs` no longer rejects a valid template response just because it still contains a legacy `patch:backlog` block. The run path now normalizes backlog mutations before `replace:pending` enforcement and reuses the same real-response-body proof as the other write paths, so a normal `patch:exchange` + `patch:backlog` closeout no longer dies early on `replace:pending block forbidden`. Added a regression for the direct run template path and captured the remaining live validation as `tasks/agent-doc/plan-run-template-backlog-normalization-validation.md`.
- **Successful closeouts now repair transient live-file drift back to the committed blob instead of only cleaning the snapshot.** `git.rs` now reuses the same authoritative `HEAD` cleanup after a real git commit that it already used for `commit_already_current` no-op closeouts: if the working tree still differs from the just-committed document only by agent-owned closeout artifacts such as `(HEAD)` heading attribution or stale/fresh boundary churn, post-commit cleanup rewrites the live file back to committed `HEAD`, refreshes CRDT sidecars, and leaves the owning repo worktree clean. Added regression coverage for the real-commit path. This closes `#cs3turn` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Codex idle-placeholder readiness is now structural instead of a three-string allowlist.** `harness.rs` now accepts the observed idle suggestion family by shape, including future variants like `› Explain this module in @filename`, as long as they still match the safe canned-placeholder form and target markers such as `@filename` or `my current changes`. This keeps routed Codex reopen triggers from fail-closing every time the composer suggestion text changes, while still rejecting real drafted user input and queue-only/busy panes. Added harness and route regression coverage and updated the routing spec. This closes `#cdxidle` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Codex route readiness now recognizes the newer idle composer suggestion `› Improve documentation in @filename`.** Recent Codex panes can be fully idle while rendering that placeholder above the footer instead of a bare prompt glyph. `harness.rs` now treats it the same as the previously-known idle suggestions, so `route.rs` no longer misclassifies that pane as busy and fail-closes a valid reroute. Added harness regression coverage and updated the routing spec. This closes the latest live `src/session-share/tasks/claudescore-3.md` reroute miss recorded in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Busy-pane supervisor restarts now wait for authoritative handoff before retrying route.** `route.rs` still allows a one-shot retry when a same-document pane is busy, the scoped fix made no changes, and the supervisor is only restartable, but it no longer re-probes the stale pane immediately after requesting that restart. Route now waits for the document's registered owner to move, and if a new pane takes over it waits for live-owner proof on that replacement before retrying dispatch. This keeps routed follow-ups from fail-closing against the old pane mid-shutdown or immediately re-restarting the fresh owner before its process-tree/file provenance settles. Added a regression that forces the old-pane-to-new-pane restart handoff and proves the routed trigger lands in the replacement pane.
- **Fresh routed auto-starts now follow same-session ownership handoffs instead of dispatching into the throwaway boot pane.** `route.rs` still creates and registers a fresh pane before launching `agent-doc start`, but after the ready wait it now re-reads the authoritative binding and, if startup reused an already-running pane for the same document session, dispatches the routed reopen into that recovered owner instead of the temporary new pane. This keeps route from surfacing a misleading busy/error path while the real Codex pane is already idle, and it avoids leaving the live follow-up tied to the wrong shell pane after a startup-time handoff. Added a regression that forces the registration to move to an existing owner during fresh boot and proves the trigger lands in the recovered pane.
- **No-pending repair now canonicalizes repeated prompt/response tails instead of only moving stale boundaries.** `repair.rs` now runs the full safe template normalization path even when there is no pending/captured response to replay, so a document that already shows a visible `### Re:` block next to a bare prompt target regains its required `❯ ` prefix during preflight/repair instead of fail-closing forever on the same typed-component-drift guard. Added a regression for the no-pending repeated-response shape that was still blocking routed reopen on `src/session-share/tasks/claudescore-2.md`, which closes the `#wcrp` repair gap in `tasks/agent-doc/agent-doc-bugs2.md`.
- **Parallel tmux full-suite routing/sync regressions now pin per-document registry roots instead of ambient `cwd`.** `route.rs` now looks up split-anchor panes from the target document's own `.agent-doc` project root, `sync.rs` writes synthetic tmux-router registries to an absolute path captured at creation time, and the cross-file split-anchor regression test now registers its anchor pane against an explicit base dir instead of ambient process state. This closes the remaining `#rtanch` full-suite flakes from `tasks/agent-doc/agent-doc-bugs2.md`, where parallel tests could make route miss an existing anchor pane or make tmux-router treat both cross-root panes as dead/missing simply because another test changed `cwd`. Added/strengthened full-suite regression coverage via the existing route and cross-root sync tests.
- **Manual `start` now clears dead stash registrations instead of fail-closing behind them forever.** `start.rs` still refuses to replace an alive pane when open startup-miss or session-log provenance says that pane may still own the document, but it now makes the stash-stranding case explicit: if the registered pane is alive only in a `stash` window, no live owner can be proven, the supervisor socket is gone, and both the startup-miss + session-log checks already show the old run is closed, `start` deregisters that stale stash binding and claims the current pane. Added stash-specific regression coverage and updated the start command spec. This closes the latest `agent-doc start ... still alive but no live owner was proven` repro from `tasks/agent-doc/agent-doc-bugs2.md`.
- **JetBrains `Run Agent Doc` now reaches tmux faster and stays visibly in-flight while route is working.** The plugin's explicit submit debounce dropped from 1500ms to 500ms, so a manual rerun is not held for an extra 1.5 seconds after the last keystroke before it even spawns `agent-doc route`. While the route subprocess is active, JetBrains now keeps an information notification open instead of relying only on a brief inline hint, then expires that notification and shows the usual success hint when route exits. This makes slow routed acks and tmux/session recovery windows visible from the IDE side while keeping final success lightweight. Bumped the JetBrains plugin build version to `0.2.82`.
- **Busy same-document reroutes with pending prompt drift now fail closed instead of reporting false success.** `route.rs` still focuses the authoritative pane and avoids force-restarting a healthy supervisor after a no-op scoped fix, but it no longer returns success for that shape. When the live Codex/Claude session is still busy and the document has unresolved prompt-bearing drift, route now emits a tmux display-message diagnostic and exits with the same busy-session error so JetBrains/CLI callers surface the blocked reroute instead of silently swallowing it. Added a regression that keeps the no-restart guarantee while requiring the fail-closed error path. This closes the latest `Run Agent Doc` false-success shape from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Full-suite verification is now explicitly fail-closed against "unrelated" or "flaky" waivers.** The bundled `agent-doc` skill and `SPEC.md` now state that a red project verification run must be treated as a real blocker even when the failing tests look outside the changed codepath. A turn must either fix the failing suite or report the concrete blocker and capture the follow-up in backlog before closeout. Added skill-bundling coverage so the installed Claude/Codex instructions keep that rule.

- **Direct local `cargo install` now resolves the sibling `agent-kit` crate without manual patch flags.** `src/agent-doc/Cargo.toml` now pins `agent-kit` with both `path = "../agent-kit"` and `version = "0.4.0"`, so `cargo install --path src/agent-doc --force` from the workspace root and `cargo install --path . --force` from `src/agent-doc` no longer fall back to the older crates.io copy that lacks `agent_kit::skill`. Added a regression test that locks this manifest contract.
- **`audit-docs` instruction discovery now prunes heavy skip dirs before descending.** `agent-kit::audit_common::find_instruction_files()` no longer uses raw recursive globbing for `src/**/...`, `.claude/**/...`, or `.agents/**/...` matches. It now walks those trees explicitly and stops descent as soon as a directory name matches `AuditConfig.skip_dirs`, so audit runs skip vendored/cache subtrees like `node_modules`, `.venv`, `target`, `.git`, `vendor`, `.next`, `dist`, and similar directories instead of traversing them first and filtering later. Added direct discovery coverage for skipped `src`, `.claude`, `.claude/skills/**/runbooks`, and `.agents` descendants.
- **Fresh routed auto-starts now rebind their own new pane immediately before the first guarded trigger dispatch.** `route.rs` still registers a fresh pane as soon as it is created so later route calls can discover it, but the first trigger send now re-checks that binding after the harness reaches its ready prompt and restores it when startup recovery cleared the temporary geometry-only entry during boot. The self-heal still fails closed if the pane was rebound to another document or a different pane already owns the same session, so cross-file dispatch protections stay intact. Added a route regression that clears the fresh-pane registration during the ready wait and proves the first `Run Agent Doc` attempt still succeeds instead of failing with `route dispatch target ... is not registered`.
- **No-op `commit_already_current` closeouts now refresh CRDT/editor sidecars when they rewrite live drift back to `HEAD`.** `git.rs` still closes transient-only `(HEAD)` / boundary churn as an already-committed no-op, but the cleanup path now also refreshes CRDT state from the committed document and emits the same editor/VCS refresh signal the plugin watches. The normal post-commit cleanup path also refreshes CRDT state after stripping guard-marker drift. This closes the `#6btt` parity gap from `src/session-share/tasks/claudescore-2.md`, where a no-op closeout could repair only the snapshot/on-disk file while stale CRDT or editor-visible state kept showing bare `compact exchange`, `(HEAD)` heading churn, or a newer boundary marker. Added regression coverage for the no-op CRDT + refresh-signal path.
- **Plain exchange-tail follow-ups now count as routed prompt work even without `?` or an imperative lead verb.** `diff.rs` now treats a non-artifact user block appended immediately before `<!-- /agent:exchange -->` as a `prompt_target`, so `session_check.rs` and `route.rs` no longer drop editor-added follow-ups like "When I run `Run Agent Doc` on this document...nothing happens..." just because they are plain prose below a stale boundary. This closes the latest `agent-doc-bugs2.md` JetBrains reroute shape where route focused the live pane as "already running" and injected nothing because it failed to see any pending prompt-bearing drift. Added direct classifier, session-check, and route regressions.
- **Codex reroutes now fail closed before dispatch if the reopen payload stops being the bare `agent-doc <FILE>` command.** `route.rs` now validates the final Codex `send-keys` payload right before injection and refuses any multiline or otherwise mutated payload instead of letting extra prompt/content text drift back into the composer and surface later as a misleading 30-second `no new document cycle started` startup-miss. Added direct guard coverage plus a live-child regression that keeps the `content_edit` reroute path on the same bare reopen contract. This hardens the `monsterrodholders.md` / `claudescore-3.md` failures recorded in `tasks/agent-doc/agent-doc-bugs2.md`.
- **`repair`/`preflight` now deterministically move stale boundaries past already-answered turns.** When a template document has no pending capture to replay but still shows a stale `agent:boundary` marker above a prompt/response pair that is already complete, `repair.rs` now treats that as safe template drift instead of just tolerating it. The repair path repositions the existing boundary marker to the true end of the completed turn, syncs the snapshot through the normal binary-owned path, and lets `preflight` commit the cleanup on the next cycle. Unanswered prompts below the boundary are still left in place and remain actionable. Added direct repair regressions for both the answered-turn repair and the unanswered-prompt no-op case. This closes the remaining deterministic-repair half of `#bdryc` from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Route now recognizes Codex's idle composer suggestion lines as dispatch-ready.** Recent Codex builds can render the empty composer as canned prompt text such as `› Run /review on my current changes` or `› Find and fix a bug in @filename` above the footer. `harness.rs` now treats those observed placeholder lines as idle chrome instead of drafted user input, so `route.rs` no longer times out on an otherwise ready pane just because the Codex UI is showing a suggestion. Real drafted text like `> agent-doc ...` or arbitrary freeform input is still rejected. Added harness and route regression coverage and updated the routing spec. This closes the latest `Run Agent Doc` failures from `tasks/agent-doc/agent-doc-bugs2.md`.
- **JetBrains now exposes a first-class `Fix Document` action for tracked markdown sessions.** The plugin adds `Fix Document` to the popup, Tools menu, editor context menu, and project view context menu, and it runs `agent-doc fix <FILE>` from the document's resolved agent-doc project root after saving buffers. This gives JB users an editor-native recovery path for the same deterministic repair flow the CLI already provides when `Run Agent Doc` surfaces a recoverable session/layout issue. Bumped the JetBrains plugin build version to `0.2.81`.
- **Busy live-pane reroutes now auto-apply the scoped fix path once before the final fail-closed error.** `route.rs` no longer surfaces the raw "not showing an idle prompt" failure on the first pass for a live same-document pane with unresolved prompt-bearing drift. Route now runs the same document-scoped repair path as `agent-doc fix <FILE>`, re-resolves the authoritative pane, and retries dispatch one time before failing closed. The follow-up behavior is now stricter: a no-op scoped fix no longer restarts an otherwise healthy same-document Codex supervisor into `resume --last`, because that could just resurrect the prior unrelated task and keep `Run Agent Doc` trapped in the same busy-pane loop. Healthy authoritative panes are still focused for visibility after the no-op fix, but the route now fails closed instead of reporting success while drift remains undispatched; only genuinely restartable supervisors remain eligible for the one-shot restart-and-retry path. Added regression coverage for both the healthy no-op-focus fail-closed path and the bounded fail-closed / retry cases, and updated the command spec. This closes the latest JetBrains `Run Agent Doc` repro from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Answered tails below a stale boundary no longer masquerade as new pending work.** `session_check.rs` now suppresses the oldest prompt-bearing change when the current exchange tail already contains that prompt below the last `agent:boundary` marker and a real `### Re:` / `## Assistant` block later in the same tail proves the turn was answered. `route.rs` picks up the same shared detector through its routed-cycle-ack gating, so reruns no longer wait 30 seconds for `pending prompt_target: ...` when the document already shows the completed response and only the closeout boundary/commit repair remains. Added direct regressions in both files and updated the command spec. This closes the `#bdryc` shape from `tasks/agent-doc/agent-doc-bugs2.md`.
- **Busy live-pane reroutes now only fail closed when there is real document drift to dispatch.** `route.rs` still refuses to inject `agent-doc <FILE>` into a non-idle Codex/Claude pane when the document has unresolved prompt-bearing changes, but a proven live pane with no pending prompt/content drift now counts as "already running": route focuses that pane and returns success instead of erroring out of the editor-trigger path. This closes the `tasks/software/corky.md` JetBrains repro from `tasks/agent-doc/agent-doc-bugs2.md`, where an already-active session blocked `agent-doc route` even though there was nothing new to send. Added a busy-pane regression that preserves the fail-closed behavior for real drift and updated the routing spec.
- **Routed Codex reopen now requires an empty composer and wrapped-trigger visibility.** `route.rs` no longer treats `› some drafted text` / `> some drafted text` as an idle dispatch target, so a live Codex pane must expose a truly empty composer before route injects `agent-doc <FILE>`. The send verification loop also now recognizes a wrapped absolute-path reopen line as still-pending input instead of declaring the command "accepted" just because the path split across multiple physical tmux lines. This closes the JB `claudescore-3.md` startup-miss shape from `tasks/agent-doc/agent-doc-bugs2.md`, where a routed reopen could be drafted into a live Codex composer, logged as accepted, and then fail closed 30 seconds later with no new document cycle. Added harness/route regression coverage for drafted Codex prompts and wrapped routed triggers, and updated the routing spec.
- **Route/session-check prompt-bearing drift detection now ignores frontmatter-only metadata edits.** `session_check.rs` now strips YAML frontmatter before classifying unresolved prompt-bearing changes, so harmless metadata churn such as `agent: codex` no longer surfaces as `content_edit` and forces `route` to wait 30 seconds for a cycle that never needed to start. `route.rs` picks up the same body-only behavior through its shared pending-change lookup, which closes the JB `claudescore-3.md` failure from `tasks/agent-doc/agent-doc-bugs2.md` where a routed Codex reopen could fail closed on `pending content_edit: agent: codex` despite there being no new user prompt in the document body. Added direct regression coverage in both `session_check.rs` and `route.rs`, and updated the backend/routing specs.
- **`replace:icebox` now parses as a real template patch instead of falling through to exchange.** `template.rs` now accepts `<!-- replace:icebox -->...<!-- /replace:icebox -->` alongside the existing `patch:icebox` form, so skill closeouts can rewrite `agent:icebox` through the binary-owned patch path without tripping the `0 template patches found` warning or dumping the list body into `exchange` as unmatched content. Updated the skill/runbook text and added parser + write regression coverage for the `#iceboxpatch` shape from `tasks/agent-doc/agent-doc-bugs5.md`.
- **Template exchange patchback now binds new responses to the oldest compatible unresolved prompt instead of blindly appending at the tail.** `template.rs` now inspects the prompt tail that lived below the previous boundary marker, matches pending ids referenced by the new `patch:exchange` response, and inserts the response immediately after the oldest matching unresolved prompt block. If the response would skip an older unresolved prompt in that tail, the write fails closed instead of silently reversing prompt/response chronology. This closes the `#pbord` shape from `src/boost-client/tasks/monsterrodholders.md`, where a newer `#wcx1` status reply could land ahead of an older unresolved `#wcup1` prompt and a later closeout would attach to the wrong turn. Added regression coverage for both anchored insertion and the skip-older fail-closed path.
- **Codex reroutes now keep the trigger payload to a bare `agent-doc <FILE>` reopen.** `route.rs` no longer appends the first unresolved prompt-bearing change onto routed Codex dispatches for closed-cycle retries. Live JB/plugin failures showed that the multiline payload could be consumed as ordinary Codex chat text, producing a conversational answer in-pane without ever starting the binary-owned document cycle, so route would correctly fail closed on the missing cycle ack. The route path now reopens only the document and relies on the session diff as the source of truth for pending work. Added regression coverage that rejects extra follow-up lines in the routed Codex payload, and updated the routing spec.
- **Preflight now fails closed on hidden uncommitted closeout drift instead of silently reporting `no_changes`.** `preflight.rs` now checks for out-of-band closeout state after repair/init but before pending maintenance or the generic commit path: a visible bypassed `### Re:` patchback or a snapshot that still differs from `HEAD` with no open/recoverable cycle now aborts preflight immediately. `session_check.rs` also names tracked side-effect files and prints the exact `agent-doc write --commit <FILE>` follow-through command in those failures. This closes the `#codcommit` shape from `src/session-share/tasks/claudescore.md`, where a Codex-side direct patchback plus `news/README.md` edits could leave the document looking answered while the binary-owned commit boundary never landed. Added regression coverage for both the hidden snapshot-ahead/no-diff preflight case and the side-effect-rich session-check diagnostic.
- **Template writes now fail closed when `patch:todo` would drop checklist items from an existing todo component.** `write.rs` counts Markdown checklist rows in the live `agent:todo` body and rejects any replacement patch whose new body contains fewer checklist items than the current component. This closes the `#ptdr` shape from `src/session-share/tasks/claudescore.md`, where a partial Phase 1 todo patch silently deleted the rest of the backlog sections because `agent:todo` still used full-replace semantics. Added regression coverage for destructive-subset rejection and same-size rewrites.
- **Later agent turns now carry forward standing document-level formatting requirements from earlier user prompts.** `prompt_contract.rs` now scans historical `❯ ...` prompt blocks for explicit structure directives such as "organize the backlog into a 2-level list" and surfaces them back into the run/stream/orchestrate agent prompts as active requirements. The prompt text also tells the responder to say so explicitly when its output contract prevents an exact match instead of silently flattening the structure. This closes the `#lvls` shape from `src/boost-client/tasks/monsterrodholders.md`, where follow-up bug-handling and transfer work could ignore an earlier backlog-organization requirement simply because it was no longer part of the latest diff. Added regression coverage in `prompt_contract.rs`, `run.rs`, `stream.rs`, and `orchestrate.rs`.
- **Normalization-divergence IPC fallback now preserves tracked backlog mutations.** When ack-content sidecar verification rejects the plugin snapshot because a required `❯ ` prompt prefix is missing, `write.rs` no longer saves raw `content_ours` by itself. Both IPC success paths now splice the current on-disk backlog/pending component back into that fallback snapshot first, so `finalize --stream --pending-add` cannot silently drop earlier pending mutations just because editor-side normalization diverged. Added regression coverage for the `#splpend` shape from `tasks/claudescore-3.md`.
- **Strict retry dedup now adopts already-present template responses before the no-edit fast path.** `write.rs` now checks for an already-visible response block before taking the `content_current == base` shortcut across the template closeout/replay paths, so a `finalize` / `write --commit` retry cannot append the same `### Re:` block a second time just because the current file already matches the retry baseline. The adopted-current path still re-runs exchange prompt-prefix normalization, which closes the `#duppb` shape from `tasks/agent-doc/agent-doc-bugs2.md` where a committed closeout-follow-up response could be replayed again with a visible `(HEAD)` copy. Added regression coverage for the same-base template retry shape.
- **Fail-closed sync recovery no longer rebinds an unrelated pane by column geometry alone.** When `sync.rs` skips auto-start for a document because a startup-miss marker or repeated recent `missing_pane` recovery window is still active, post-sync registration now refuses to mirror tmux-router's file→pane assignment unless that pane actually proves live ownership for the document. If a stale binding for that same pane was already present in the document's nearest `sessions.json`, sync prunes it instead of immediately writing the geometry-only assignment back. This closes the live `src/session-share/tasks/claudescore-3.md` shape from `tasks/agent-doc/agent-doc-bugs2.md`, where `%261` could keep being rebound to `claudescore-3.md` even after fail-closed recovery had intentionally refused to auto-start a fresh owner. Added regression coverage for the fail-closed geometry-only rebind path.
- **Synthetic tmux-router sync registry now drops ambiguous same-root duplicate pane claims before layout reconcile.** `sync.rs` now filters the per-run session-id registry it builds for tmux-router so one stale pane cannot stand in for both `src/session-share/tasks/claudescore.md` and `src/session-share/tasks/claudescore-3.md` during the same sync pass. A duplicate pane is kept only when exactly one claimant still proves live ownership (or, failing that, exactly one claimant uniquely matches the pane's project root); otherwise the duplicate pane is removed from the synthetic registry entirely so tmux-router must rehydrate a distinct pane instead of aliasing two visible documents onto one live pane. Added regression coverage for the ambiguous same-root child-repo shape and the unique-live-owner keep path.
- **Post-sync registration now fails closed when tmux-router aliases one pane onto multiple cross-root documents.** `sync.rs` now rejects duplicate file→pane assignments unless exactly one claimant matches the pane's own project root or already proves live ownership, and it prunes the losing stale registry binding instead of preserving a second cross-root alias. This closes the `agentic-harness-engineering.md` shape from `tasks/agent-doc/agent-doc-bugs2.md`, where `src/session-share/.agent-doc/sessions.json` could keep pointing at the root workspace pane `%151`, leaving the child document unable to start or sync because both registries claimed the same live pane. Added regression coverage for duplicate cross-root post-sync registration.

- **Committed cycle-state is now monotonic across later repair bookkeeping.** `cycle_state.rs` now refuses to downgrade an already-committed cycle back to `response_captured` or `write_applied` when a later repair/replay path touches the same cycle. `run.rs` now also opens a fresh `preflight_started` cycle after its pre-commit boundary so the current response closeout does not inherit that older committed state. This closes the `#stphk` shape from `tasks/agent-doc/agent-doc-bugs2.md`, where a post-commit `repair_applied` event could leave the cycle-state file open even though the capture ledger and session log already proved `commit_success`, causing the Codex Stop hook to loop on a fake unfinished turn. Added direct cycle-state regression coverage plus a repair replay test for the committed-then-replayed shape, and updated the command spec.

- **Post-commit boundary cleanup now repairs missing `❯ ` prompt prefixes in the working tree.** When IPC-side normalization verification falls back to `content_ours`, `git.rs` now compares the clean snapshot against the live document, restores any missing exchange user-region prefixes, and then repositions the boundary so the working tree catches up with the committed blob. `write.rs` also upgrades the live-listener path to send a zero-content IPC patch carrying `normalize_prefix_lines` plus `reposition_boundary` when that repair is needed, instead of sending a bare reposition signal that could only move the boundary. Added regression coverage for both the target extraction helper and the no-listener post-commit repair path.

- **Fresh-route cycle ack now survives initial supervisor restarts instead of failing 15s too early.** `route.rs` now gives fresh auto-starts the same longer start-ack budget as routed live-child dispatches (30s in production, 2s under tests), so the first `Run Agent Doc` attempt no longer fails closed when the initial pane immediately recycles through `sync_missing_pane`/startup recovery before the first `preflight_started` becomes visible. Added route-level regression coverage for a delayed fresh-start ack and logged the timeout budget on fresh-start ack success/miss paths.

- **Cross-root sync now feeds tmux-router a per-run session-id registry instead of the caller's root registry.** `sync.rs` now synthesizes a temporary tmux-router registry from each visible document's own nearest `.agent-doc/sessions.json` before reconcile. This closes the `agent-doc-bugs2.md` / `claudescore-3.md` regression where focus changes could invert left/right tmux ownership simply because tmux-router fell back to "spare pane" assignment after looking up both session ids in the wrong registry. Added isolated tmux coverage for the cross-root focus-stability repro and updated the sync command spec.

- **`start` now reuses alive session-log owners before fail-closing stale live-pane recovery.** `start.rs` now consults the owning session log's latest still-open pane as an extra provenance source whenever `sessions.json` points at an alive pane but current live-owner proof is missing. If that latest-open pane is still alive, `start` focuses and reuses it instead of falling straight into the "supervisor unavailable, no live owner proven" fail-closed path. This closes the `#asfc` repro from `tasks/agent-doc/agent-doc-bugs2.md`, where manual/editor-driven recovery could strand a healthy document behind stale registry state even though the session log still identified the last open pane. Added regression coverage for the session-log-owner reuse path and updated the start command spec.

- **Sync now reserves pane ownership per run so one live pane cannot satisfy two visible documents at once.** `sync.rs` now tracks pane ids already claimed earlier in the same reconciliation pass, treats later duplicate claimants as unresolved, and excludes those reserved panes from associated-pane recovery. This closes the `agent-doc-bugs2.md` mixed-root layout collapse where `tasks/agent-doc/agent-doc-bugs2.md` and `src/session-share/tasks/claudescore-3.md` could both believe `%75` was their live owner, causing `agent-doc sync` to collapse to a one-pane fast path instead of rehydrating the second column. Added regression coverage for same-run pane reservation conflicts and reserved associated-pane filtering.

- **Automatic prune now reaps stray retained-dead panes outside stash when another pane still owns the window.** `resync.rs` now kills unregistered `remain-on-exit` panes in non-stash windows during both automatic `prune()` and explicit `resync --fix` cleanup, but still preserves the last pane in a window for manual inspection. This closes the latent `Pane is dead` clutter reported from `tasks/agent-doc/agent-doc-bugs2.md`, where dead replacement remnants could survive indefinitely once the registry forgot them. Added regression coverage for the sibling-pane cleanup and last-pane safety guard, and updated the resync command spec.

- **JetBrains split-layout detection now follows screen position instead of focus-sensitive window order.** `LayoutDetector` now groups visible editor windows by their actual x/y bounds instead of assuming `FileEditorManagerEx.windows` is left-to-right stable. This closes the `agent-doc-bugs2.md` repro where selecting the right editor split inverted tmux pane placement, while still preserving empty columns when one split shows a non-markdown tab. Added unit coverage for reversed input order, vertical stacking, and empty-column preservation, and bumped the JetBrains plugin build version for local installs.

- **Recovered live-owner re-registration now preserves supervisor identity instead of downgrading back to transient CLI metadata.** `sync.rs`, `start.rs`, and `resync.rs` now re-register recovered panes through the owning tmux handle and restore authoritative `supervisor_pid + supervisor_instance_id` when that evidence is still available from the registry or supervisor socket. This closes the regression where a valid recovered pane was rewritten with the short-lived `route` / `sync` process PID and an empty instance id, causing the next provenance check to fall back to brittle heuristics and churn pane layout again. Added regression coverage for recovered associated panes, same-pane identity preservation without a live socket, cross-root sync registration, and the stale-live-owner route path.

- **Mixed-root editor sync now consults each document's own registry before rescuing or rebinding panes.** `sync.rs` no longer assumes the caller's current project root is authoritative for every visible markdown file. File resolution, stash rescue, associated-pane marking, path-provenance lookup, and post-sync `sessions.json` updates now all canonicalize the document path, resolve that document's nearest `.agent-doc` root, and read/write the registry there. This closes the cross-repo layout bug where `tasks/agent-doc/agent-doc-bugs2.md` could borrow `src/session-share/tasks/claudescore-3.md`'s live pane, leave a retained dead pane in the opposite slot, and accumulate duplicate path-keyed entries such as `src/session-share/src/session-share/...` in the child registry. Added regression coverage for cross-root registry resolution and per-root sync registration.

- **`sessions.json` is now path-keyed and live-owner proof is top-down by default.** `sessions.rs` now normalizes the registry around canonical absolute document-path keys, keeps `session_id` in the value, and records a `supervisor_instance_id` alongside the supervisor PID. `start.rs` stamps that supervisor identity into the registry and exposes it over IPC, while `sync.rs` now treats `pane + supervisor PID + supervisor instance id` as the primary ownership proof before falling back to tmux argv/process-tree heuristics. Added coverage across GC, route, startup-miss/session-check, and registry normalization paths, and updated the routing/supervisor command specs.

- **Active-session post-closeout drift now fails closed in `session-check` / Codex Stop recovery.** `session_check.rs` now refuses to report a committed cycle as clean when the current Codex session still owns that file and the live document changed again after the last committed closeout without reopening the binary-owned write/commit path. Instead of silently classifying that state as harmless post-commit drift, `session-check` interrupts so the Stop hook can recover from `last_assistant_message` or block the turn. Added regression coverage for both the direct `session-check` guard and the Stop-hook auto-close path, and updated the command spec.

- **Open session-log provenance now blocks halted-supervisor rebinds in `start`.** `start.rs` no longer treats `state="halted"` as sufficient authority to replace an alive registered pane when the session log still shows that same pane as the latest open run with no later child exit or `session_end`. In that stranded-owner shape, manual/editor-driven `start` now fails closed instead of emitting another `session_superseded ... origin=registry_rebind` pane era on top of unresolved in-flight work. Added regression coverage for the open-vs-closed session-log guard and updated the command spec.

- **Synthesized unmatched exchange patches now preserve visible `❯ ` prompt prefixes in JetBrains.** `write.rs` now applies `normalize_prefix_lines` when IPC has to synthesize an append-mode `exchange` patch from raw unmatched content, not just for explicit patch blocks. That closes the remaining JB-plugin shape where a prompt-bearing line such as `do #expatch. spec-test-build-install-commit-push` could still be saved visibly bare in the editor during uncommitted `(HEAD)` response state even though the Rust snapshot path already knew it should be prefixed. Added regression coverage for the synthesized-unmatched `#expatch` shape.

- **`agent-doc patch` now replaces component bodies by default, even on append-mode exchange docs.** The standalone `patch` subcommand no longer inherits a component's configured `patch=append` / `patch=prepend` mode as an implicit behavior change. Bare `agent-doc patch <FILE> exchange ...` now replaces the paired-marker body as the command synopsis promises, which fixes the `#expatch` repair path where exchange restores duplicated history instead of overwriting it. Intentional cumulative edits still exist behind the explicit `--mode append|prepend` escape hatch. Added CLI/unit regression coverage and updated the command spec.

- **Alive stale-owner panes no longer get silently replaced when `start` loses ownership proof.** `start.rs` now fails closed when a registered pane is still alive, no live owner can be proven for the document, and the supervisor socket is unavailable, instead of deregistering that pane and rebinding a fresh one. When `start` does intentionally replace an alive pane after an explicit halted/restart-failed determination, it now preserves the old registry entry until the new pane registers so the normal `session_superseded` / `session_end origin=registry_rebind` provenance is appended to the session log. Added regression coverage for the new fail-closed supervisor-health decision and updated the command spec.

- **Editor visual-token ranges now stay aligned after multibyte text.** `agent_doc_visual_tokens_json` now converts the shared scanner's internal UTF-8 byte ranges into UTF-16 document offsets before returning them to JetBrains and VS Code. This fixes the JB-plugin drift where highlights walked forward after emoji, smart punctuation, or other multibyte characters earlier in the document. Added FFI regression coverage and documented the editor-facing range contract.

- **Scratch-comment bodies now stay highlighted as comments across both editors.** The shared visual-token scanner now emits dedicated body ranges for ordinary HTML scratch comments (`<!-- ... -->`), not just the delimiter lines. JetBrains and VS Code consume that extra token so multiline scratch comments no longer fall back to raw Markdown parsing inside the comment body, which fixes the remaining JB-plugin "syntax error" rendering around commented examples and screenshot/image notes near the exchange closeout.

- **Editor overlays now mute agent-managed markdown bodies and normalize standalone bracket labels.** The shared visual-token scanner now emits agent-component body ranges plus standalone label tags such as `[recommended]`, excluding fenced/inline code, images, and checklist markers. JetBrains and VS Code both consume those new tokens so agent-managed blocks render with a muted background tint and bracket labels stop inheriting broken-link Markdown styling. This specifically cleans up the JB-plugin rendering issues where `agent:exchange`/backlog content stayed visually flat and tag-like labels looked like malformed references.

- **JetBrains plugin version bumped to `0.2.79` for the latest local-testing build.** Updated `editors/jetbrains/gradle.properties` so the next `buildPlugin` artifact and any local install/use of the bundled JB plugin carry a new patch version after the recent closeout-fix work.

- **JetBrains preserve-head cleanup now prefers committed answered prompt prefixes over stale editor buffers.** The JetBrains plugin's post-commit reposition comparator now treats already-answered `❯ ` prompt-prefix differences as the same committed content when the next meaningful exchange line is the matching `### Re:` block. That means the `preserve_head` boundary cleanup path will reuse the committed disk transcript instead of re-saving a stale unsaved editor buffer that only differs by boundary churn, model-attribution churn, or stripped historical prompt prefixes. Added regressions for the `#qprx` shape and for the unresolved-follow-up safety case where disk preference must still stay off.

- **Early Ctrl-D prompt EOFs no longer close freshly started Codex panes.** `start.rs` now treats a prompt-time stdin EOF as `restart fresh` instead of `quit` when Codex clean-exits immediately after a fresh pane start and the `Ctrl-D`/EOF prompt fires inside the early-start grace window. That closes the `monsterrodholders.md` rebind-churn shape where a transient tmux stash/rescue input race could look like `user_quit_after_ctrl_d`, close the claimed pane, and trigger `%546 -> %550 -> %552` replacement churn. Added start-level regression coverage and updated the start/supervisor specs.

- **Open preflight cycles with visible manual patchbacks now fail with an explicit follow-through message.** `session_check.rs` no longer reports a generic `preflight_started` interruption when the working tree already contains a fresh `### Re:` block that `HEAD` still does not prove. That shape now surfaces as a manual-repair / commit-boundary interruption with a concrete `agent-doc write --commit <FILE>` follow-through hint, so repaired-but-uncommitted session docs are easier to diagnose and cannot be mistaken for an ordinary stale preflight. Added regression coverage for the open-cycle manual-patchback path and updated the command spec.

- **Strict replay closeout now re-normalizes merged prompt prefixes and adopts already-present responses instead of duplicating them.** `write.rs` now re-runs exchange prompt-prefix normalization on the final merged template/CRDT document, not just on `content_ours`, so a concurrent bare `do #...` line cannot survive the merge and trip post-commit `session-check` after `finalize` already committed the response. When a manual `write --commit` / replay retry sees that the same response body is already present in the live document, the write path now adopts the current transcript and canonicalizes it instead of CRDT-merging the response a second time. `repair.rs` exposes the same normalized visible-response matcher for both recovery and write-time replay checks. Added regression coverage for the merged-prefix repair path and preserved the existing duplicate-replay tests.

- **Editor-return rebinds now preserve the canonical owner instead of churning fresh pane eras.** `start.rs` now treats a proved live owner as authoritative even if supervisor IPC is stale, and it fails closed when an alive registered pane still owns the active startup-miss marker instead of rebinding the document onto a fresh pane. `sync.rs` now clears startup-miss markers already superseded by a newer registered owner before auto-start decisions, and it skips auto-start entirely when the unresolved marker still belongs to an alive pane. This closes the `#rbret` shape where returning to an already-running document could cascade `%529 -> %533 -> %536 -> %540` registry rebinds and look like a tmux-pane crash even though the session log showed only `session_superseded` / `session_end origin=registry_rebind` provenance. Added regression coverage for superseded-marker clearing plus the new start/sync guards, and updated the command spec.

- **Session-log closeout parsing now honors metadata-bearing `session_end` events.** `startup_miss.rs` no longer treats only a bare literal `session_end` line as proof that the latest pane era closed. Session-log analysis now closes the latest run/session whenever the event token is `session_end`, even if recovery metadata follows (for example `session_end origin=registry_rebind ...` or `session_end origin=sync_missing_pane`). That keeps rebind and missing-pane recovery provenance from being misclassified as a still-open/crashed session in the remaining `#tmuxcrash` forensics path. Added regression coverage for metadata-bearing `session_end` parsing and updated the session-log spec.

- **Session logs now record document closeout phase transitions alongside harness/pane provenance.** `cycle_state.rs` now appends `document_cycle phase=... cycle=... event=...` entries to the owning `.agent-doc/logs/<session>.log` whenever a session document crosses `preflight_started`, `response_captured`, `write_applied`, or `committed`. That puts the document closeout boundary in the same timeline as `*_start`, `*_exit`, `supervisor_exit`, and dead-pane diagnostics, so `#tmuxcrash` forensics can distinguish true child death from an interrupted-but-already-committed closeout without reconstructing the boundary from separate state files. Added cycle-state regression coverage and updated the supervisor/session-log spec.

- **Stashed panes now keep dead-pane retention after `join-pane` moves.** Fresh panes provisioned for agent-doc sessions now enable tmux pane-local `remain-on-exit` instead of setting the option on the original window. That means a Claude/Codex pane moved into a stash window still retains `pane_dead_status` and visible tail output if the harness exits while stashed, closing the `#stshroe` path where sync had to auto-start a replacement because the old pane vanished before provenance could be captured. Updated the auto-start command spec and added a tmux-router regression that exits a pane only after it has been stashed.

- **Supervisor session logs now preserve child-exit provenance and shutdown reasons.** `start.rs` no longer flattens every harness exit into a bare `*_exit code=<n>` line. The session log now records `exit_kind`, signal name when applicable, and the rendered exit status text on both `*_exit` and `restart_eval`, and the supervisor now appends `supervisor_exit reason=...` immediately before the final `session_end`. This keeps true `#tmuxcrash` forensics distinguishable from ordinary clean exits or app-level nonzero exits without changing the existing startup-miss / recovery state machine. Added start-level regression coverage for signal and nonzero exit rendering, and updated the supervisor logging spec.

- **Cross-root stash pruning now preserves sibling-repo panes that still have live project-local ownership or supervisors.** `resync.rs` no longer decides stash-pane orphanhood only from the caller's current project root. Before killing an unregistered stash pane, prune now inspects the pane's own nearest project root, checks that root's `.agent-doc/sessions.json`, and consults that root's live supervisor sockets. This closes the sync churn where `src/session-share` panes were stashed out of the shared `agent-doc` tmux window, then incorrectly killed as "unregistered" by a root-workspace prune pass, which forced repeated `sync_missing_pane` auto-start loops for documents like `docs.md`, `claudescore.md`, and `claudescore-3.md`. Added regression coverage for the cross-root live-supervisor stash case and updated the command spec.

- **Repeated missing-pane recovery now fails closed before route/sync spawn more replacements.** `startup_miss.rs` now summarizes recent `supervisor_exit code=missing_pane` events from the session log, keyed by document session, and both `route.rs` and `sync.rs` consult that shared window before any blind auto-start. Once the same document records two unexpected pane-loss recoveries inside ten minutes, routed retries and editor-driven sync stop auto-provisioning fresh panes and surface a stable manual-recovery diagnostic instead of cascading more tmux churn over a repeated crash window. Added regression coverage for the shared detector plus the route/sync guard paths, and updated the command spec.

- **Session rebinds now close the prior pane era in the session log before switching panes.** `sessions.rs` now treats a same-UUID re-registration onto a different pane as a provenance boundary: before `sessions.json` overwrites the binding, it best-effort appends `session_superseded old_pane=... new_pane=...` and `session_end origin=registry_rebind ...` to the existing session log. That keeps crash/recovery forensics from showing an old pane as forever-open when `route`, `sync`, or `start` moved the document to a replacement pane. Added registry coverage for the rebind logging path and updated the command spec.

- **Halted supervisors now fail closed in route and get replaced fresh in manual start.** `start.rs` and `route.rs` no longer collapse supervisor state `halted` into the generic "restartable" bucket. Explicit `agent-doc start <FILE>` now treats a halted reused session as a crashed stale binding, deregisters it, and starts fresh instead of reviving the same halted loop in place. `route.rs` now refuses to auto-restart or auto-replace a registered pane whose supervisor already halted after repeated crashes, surfacing the pane id and restart count instead of cascading more automatic tmux churn over the same crash loop. Added regression coverage for the halted-health classifier, stale-start decision, and route fail-closed path.

- **Route no longer mistakes its own control pane for a live document owner.** `sync.rs` now narrows process-tree ownership proof so `agent-doc route <FILE>` / `claim <FILE>` utility invocations do not count as associated document panes; only the long-lived `agent-doc start <FILE>` supervisor path (plus harness-owned matches) can satisfy that proof. This closes a false duplicate-owner ambiguity found during a live tmux-backed Codex repro, where the control pane running `route` was reported alongside the real registered pane for the same document. Added regression coverage for owner-command classification.

- **Retained dead panes now preserve stashed-session crash provenance before replacement.** Fresh panes provisioned by `route.rs` now enable tmux pane-local `remain-on-exit`, and `tmux-router` now treats retained dead panes as dead rather than alive so route/sync do not accidentally reuse them. When `sync.rs` replaces a registered pane that has died, it now captures tmux's retained `pane_dead_status`, saves the last 80 lines of pane output under `.agent-doc/logs/dead-panes/`, records the open cycle phase plus capture path in the session log, and only then records the synthetic `supervisor_exit` / stale-preflight repair before replacement. `resync.rs` now also purges orphaned retained-dead stash panes once they are unregistered, so the new diagnostic preservation does not leak dead stash clutter forever. Added regression coverage in both `tmux-router` and `agent-doc` for retained-dead liveness, dead-pane provenance capture, and dead-stash purge cleanup.

- **Crash-recovery snapshot repair now heals committed answered-prompt prefix drift.** Historical snapshot self-heal no longer requires a new `### Re:` insertion when the only committed exchange difference is prompt-prefix normalization on an already-answered prompt (for example, stale `❯ do ...` vs committed bare `do ...` directly above the same response block). `commit` / `session-check` now compare snapshots with the same exchange-only normalization, repair the stale snapshot from committed `HEAD`, and stop misclassifying that drift as fresh unresolved prompt-bearing user work after crash recovery. Added regression coverage for the committed prefix-normalization path.

- **Nested backlog edits now replace stale child continuations and reassign duplicate child ids.** `pending.rs` now parses multiline `--pending-edit` payloads as a parent line plus continuation block, so editing a backlog item with a refreshed child sublist replaces the old nested content instead of appending the new lines on top of stale children. During nested-child canonicalization, existing duplicate child ids are now reassigned to fresh parent-prefixed ids, which lets damaged backlog sublists self-heal instead of preserving collisions forever. Added regression coverage in both `pending.rs` and `pending_cmd.rs`, and updated the pending command spec to document the stricter multiline-edit contract.

- **Sync now recovers supervisor-backed claimed panes before spawning a replacement.** `sync.rs` no longer relies only on argv/file-path matches when a managed document appears to have lost its pane. Before auto-starting, it now runs the shared associated-pane proof (`find_associated_panes`) so a still-alive supervisor-owned pane can be re-registered via supervisor child-PID fallback even after the foreground process tree stops mentioning the file. When that recovered pane is stashed, sync rescues it back into the `agent-doc` window; when multiple associated panes still remain, sync fails closed for that file instead of auto-starting another duplicate session. Added regression coverage for supervisor-backed associated-pane recovery and updated the sync command spec.

- **Startup-miss markers are now cleared when a newer registered pane has already taken over.** `startup_miss.rs` now detects the stale-marker shape where the persisted miss still points at an older pane, but `sessions.json` and the session log already prove a newer open start on a different registered pane for the same document. `route.rs` clears that stale marker before reuse/restart decisions, and `session_check.rs` now heals the same stale state instead of warning about a fake current crash. Added regression coverage for the cross-pane supersession path plus the post-commit session-check cleanup.

- **Nested backlog subtasks now get parent-prefixed ids and checkboxes automatically.** `pending.rs` backfill no longer leaves indented child bullets as anonymous prose when they look like subtask list items: it now canonicalizes them with checkboxes plus nested ids shaped like `[#parentid-abcd]`, using the owning flush-left parent item's id as the visible prefix. `pending_cmd.rs` now re-runs that canonicalization after granular edits/adds/state transitions so `--pending-edit` can add a sublist and get stable nested ids in the same cycle instead of waiting for a later preflight. Custom pending ids now accept hyphens to support the parent-prefixed child-id shape. Updated the pending spec/runbook text and added regression coverage for nested child-id backfill plus hyphenated id parsing.

- **Startup-miss reruns now treat later child restarts as fresh live-run provenance.** `startup_miss.rs` no longer reasons only from `session_start`; it now tracks the latest harness run boundary (`*_start` / `*_restart`) inside the owning supervisor session, so a pane that cleanly restarted the child is classified as open again instead of looking like a permanently closed or crashed pane. `route.rs` now clears retained startup-miss markers only when the same pane proves a newer open harness run after the miss, and its ops provenance now reports that latest run event directly. Added regression coverage for restarted-child session-log parsing and for the reroute helper path that must treat a later `fresh_restart` as superseding the old miss.

- **Routed startup-miss errors now surface the recorded timestamp and stop clearing unresolved live-pane misses.** `route.rs` now appends the persisted startup-miss timestamp to the fail-closed `no new document cycle started` error and to the tmux overlay diagnostic, so JetBrains/plugin error surfaces can point back to the exact recorded miss without hunting through logs. On reroute, a startup-miss marker is now cleared only when the same pane proves a newer open harness run after that miss; if the pane merely still owns the document but the session log shows a closed/timeout restart loop with no later run, route deregisters it and starts fresh instead of repeatedly reusing and re-clearing the broken pane. Added regression coverage for the closed-live-owner restart rule and for timestamped routed startup-miss failures, and updated the routing spec to document the stricter marker-retention contract.

- **Stash-loss recovery now preserves live supervisors and closes orphaned preflight cycles before replacement.** `resync.rs` no longer auto-purges an unregistered stash pane when that pane still hosts a live supervisor socket, so a temporarily unregistered stashed Codex/Claude session is preserved for later recovery instead of being silently killed as generic stash garbage. `sync.rs` now records explicit `supervisor_exit code=missing_pane` provenance in the owning session log and repairs a stale `preflight_started` cycle before auto-starting a replacement pane when a previously registered pane is truly gone. Added regression coverage for supervisor-backed stash preservation plus the missing-pane stale-preflight repair path, and updated the sync/resync command spec to document the stronger recovery contract.

- **Codex routed retries now re-submit the unresolved prompt body instead of a bare reopen.** `route.rs` now carries the first unresolved prompt-bearing change text alongside `agent-doc <FILE>` when re-dispatching into an already-live Codex pane on top of a closed cycle. That gives cancel/retry flows a fresh actionable message for the harness instead of a bare reopen that can be accepted by tmux yet produce no new document cycle. `session_check.rs` now exposes the first unresolved prompt-bearing change directly so route and session-check share the same classifier, and the routing spec documents the Codex retry payload contract. Added unit coverage for prompt-body normalization and Codex-only payload expansion.

- **Nested submodule gitdirs are now added to workspace-write harness roots.** `git.rs` now walks the current repo's `.git/modules/...` tree and exposes every nested child submodule gitdir alongside the existing submodule and superproject roots, so a session launched from `src/boost-client/tasks/...` can still commit inside `src/boost-client/src/monsterrodholders-dev` without tripping a misleading `index.lock` permission failure on the real gitdir under `.../.git/modules/...`. Added regression coverage in both `git.rs` and `agent/mod.rs`, and updated the config/command/git specs to document the deeper writable-root set.

- **`#agent-doc-bug` closeout now proves that the requested plans were actually created.** `prompt_contract.rs` now detects preset-expanded "create a plan" requirements, `preflight.rs` persists the required plan-reference count in cycle state, and `write.rs` / `session_check.rs` now fail closed when the response cites fewer existing plan files than the bug prompt described. This closes the chat-level bug-report gap where a response could enumerate backlog transfers but skip one or more required plan files. Added regression coverage for prompt-contract plan counting plus pre-commit/post-commit shortfall failures, and updated the skill/spec text to document the stricter contract.

- **Route now recognizes Claude's double-chevron composer chrome as idle.** `harness.rs` now treats lines like `⏵⏵ ... (shift+tab to cycle)` as a valid Claude prompt shape, and `route.rs` has regression coverage proving `wait_for_agent_ready()` no longer misclassifies that newer idle UI as a busy pane. This fixes routed `Run Agent Doc` failures where the pane was actually ready but route kept waiting for a bare `❯` / `⏵` line and then refused injection after 15 seconds.

- **Supervisor quit prompts now log the actual user decision and fail closed on ambiguous stdin.** `start.rs` now records whether a clean-exit / Ctrl-D / resume-failure prompt led to quit, EOF-quit, invalid input, or an explicit fresh restart, so session logs no longer jump from `ctrl_d_prompt_user` straight to another `codex_start` with no provenance. Prompt-time stdin EOF now exits the supervisor instead of being treated like an implicit restart, and stray non-empty input is rejected with a re-prompt instead of silently starting a fresh child. Added unit coverage for prompt-decision classification and input-summary logging.

- **Route/fix now treat duplicate document panes as a first-class recovery state.** `sync.rs` now enumerates every pane that still proves ownership of a document via process-tree or supervisor-PID evidence, `route.rs` only auto-picks a winner when that evidence is decisive (single owner overall, or single active-window owner with only stashed duplicates), and `resync.rs`/`fix` now re-register a unique winner before generic issue cleanup. Scoped `fix <FILE>` can also kill redundant unregistered stash panes once the winning pane is known. Ambiguous cases now fail closed with direct inspect/claim/kill commands instead of blindly reusing the first pane that happens to match.

- **Local tmux-router development is now first-class in agent-loop.** The workspace root now patches `tmux-router` to the sibling `src/tmux-router` checkout via `.cargo/config.toml`, the harness instruction surfaces (`AGENTS.md`, `SKILL.md`, `CLAUDE.md`) now tell Codex/Claude to treat that crate as a live development target when generic tmux behavior moves out of `agent-doc`, and `sessions.rs` / related helpers now delegate reusable session/key primitives to `tmux-router` instead of carrying their own shell-level copies.

- **Added `agent-doc fix` as the canonical session-repair surface, with document-scoped targeting.** `main.rs` now exposes a top-level `fix [FILE]` command, while `resync --fix [FILE]` routes through the same implementation. `resync.rs` now accepts an optional target document, limits dead-pane pruning and issue/fix application to matching registry entries for that file, and leaves unrelated stash/orphan cleanup untouched during scoped runs. Updated command metadata, CLI coverage, and `specs/07-commands.md`.

- **Preflight no longer swallows prompt-bearing status edits into step-2 OOB absorbs.** `git.rs` now rejects safe-status snapshot absorbs when the inserted status text contains prompt work, including preset-token leads like `#next-steps` and `#next-steps ...`, imperative directives, or other prompt-bearing lines. That keeps compact-follow-up status edits visible to `preflight` step 4 instead of letting step 2 commit them as prior-cycle out-of-band status churn and collapse the turn to `no_changes`. Added direct classifier coverage plus a preflight regression for the compacted-status repro, and updated the commit spec.

- **Startup-miss reruns now distinguish stranded sessions from real pane death.** `startup_miss.rs` now parses the owning session log for the latest live harness run in the session, `route.rs` logs that provenance and refuses to auto-start a replacement when the marked pane is still alive, the supervisor socket is gone, and no later child exit / `session_end` was ever recorded. `session_check.rs` now includes the same session-log detail in its startup-miss warning so the failure is visible as a stranded supervisor/startup-miss state instead of a generic tmux-pane crash. Added regression coverage for session-log parsing plus the new route fail-closed decision, and updated the routing spec.

- **`#agent-doc-bug` closeout now proves the full transferred bug set, not just target drift.** `prompt_contract.rs` now derives a minimum explicit transfer count from the prompt-bearing bug report itself, `preflight.rs` persists that count in cycle state, and `write.rs` / `session_check.rs` now fail closed when a target backlog changed but the response only enumerated a smaller set of transferred `[#id]` items than the bug prompt actually described. Existing promised-id enforcement still proves that every enumerated new id landed in the target backlog; the new guard blocks the earlier partial-transfer shape where only a subset of the reported bugs was captured. Added regression coverage for prompt-contract counting plus pre-commit/post-commit shortfall failures, and updated the command spec and transfer runbook to document the stricter `#agent-doc-bug` inventory contract.

- **Explicit backlog-target closeout now proves every newly promised `[#id]` landed.** `preflight.rs` now snapshots the baseline open-item ids for each prompt-contract target named by `Add to the backlog of ...`, and `write.rs` / `session_check.rs` now compare any new tracked-item ids listed in the response body against the live target backlog before allowing closeout. A target backlog merely changing is no longer sufficient when the response promises multiple new items: if some listed ids are still missing, `finalize` and `session-check` fail closed with the missing-id set. Added regression coverage for both pre-commit and post-commit enforcement, and updated the command spec plus transfer runbook to document the stronger contract.

- **Startup-miss diagnostics no longer get stranded in the harness input buffer.** `route.rs` now renders fresh-start and routed-trigger startup-miss notices through a tmux-owned `display-message` overlay instead of drafting `echo '...'` text into the pane input area. That keeps Codex/Claude panes visibly recoverable without making the session look hung behind an unsent composer line. Added route coverage for retry-command rendering and for the regression that no longer leaves drafted `echo` text in the pane, and updated the command spec to document the overlay contract.

- **Strict template / CRDT closeout now fails before IPC when the response has no real body.** `write.rs` now proves that a template-mode response contains at least one non-empty non-backlog/non-frontmatter patch or a non-empty unmatched body that can be synthesized into `exchange` / `output` before the strict closeout can proceed. Empty `patch:exchange` shells, frontmatter-only payloads, or normalization-only responses therefore fail before `ipc_write_consumed` / commit instead of silently consuming the turn as a zero-patch closeout. Added unit coverage for the proof helper plus finalize integration coverage for the strict CRDT reject path.

- **Shared docs now require an explicit security review before cross-document access.** `frontmatter.rs` adds `agent_doc_collaboration: shared` plus `agent_doc_security_review: <review-id>`, `extract.rs` now blocks cross-document `extract` / `transfer` when a shared source or target lacks that review marker, and `plan.rs` now blocks shared `do #id` work when the referenced backlog/icebox item points at another `.md` plan without the same review proof. Auto-created transfer targets inherit the source document's shared/review metadata. Updated the security spec, README, and pending-ops runbook, and added regression coverage for the new frontmatter, transfer guard, and plan blocker.

- **JetBrains/VS Code tab sync no longer suppresses the first opposite-pane selection, and the editor-side coalescing delay is now 100ms instead of 500ms.** The shared tab-sync planners were still carrying a 1.5s bounce-back filter that could classify a real left/right split selection as noise immediately after the prior sync, which matched the latest "first click to the other side does nothing" report. Both plugins now dispatch every real tab-selection state change, keep only exact-state dedup, and reduce the editor-side debounce to 100ms so visible split handoff stays low-latency. Added Kotlin and TypeScript regressions that prove an unchanged split still syncs on the first opposite-pane selection. Bumped the local-testing plugin builds to JetBrains `0.2.91` and VS Code `0.2.11`.

- **Editor plugins now visually distinguish agent-doc markdown structures from ordinary prose.** `syntax.rs` adds a shared Rust token scanner exposed through the new `agent_doc_visual_tokens_json` FFI export, and both editor plugins now consume that canonical range set instead of maintaining their own parsers. VS Code 0.2.10 adds live markdown decorations for agent component comments, patch comments, boundary markers, `### Re:` headings, `❯` prompts, tracked `[#id]` tags, and ordinary HTML scratch comments. JetBrains plugin 0.2.78 applies the same token stream through editor highlighters. Fenced/inline code examples are intentionally excluded so markup samples remain untouched. Updated the shared editor spec and editor integration guide to document the new highlighting contract.

- **Harness prompt intent now survives `no_changes` direct-entry turns.** `codex_hook.rs` now persists the last Codex `UserPromptSubmit` text alongside the tracked document, and `preflight.rs` / `plan.rs` now consume that harness prompt body (or an explicit `AGENT_DOC_HARNESS_PROMPT` override) when the document itself has no diff. The binary strips the leading `agent-doc <file>` invocation, synthesizes a prompt-bearing diff from the remaining chat text, and reuses the normal diff/prompt-contract pipeline so direct harness prompts such as `#agent-doc-bug`, `#code-review`, or `do #id ...` no longer collapse to `no_changes` / `No changes detected since the last snapshot.` simply because the user asked in chat instead of editing the document first. Added regression coverage for env-backed harness prompts, Codex-thread prompt lookup, preflight cycle opening, and plan output for preset-expanded backlog capture plus existing `do #id` resolution.

- **Backlog and icebox now support ordered parent items for explicit priority.** `pending.rs` now recognizes flush-left `1. ...` / `2. ...` parent entries alongside `- ...`, preserves them through backfill/edit/done/reap/transfer, and when any tracked item in a backlog or icebox uses ordered style the binary canonicalizes the whole tracked surface as a sequential ordered list in current item order. Granular mutations therefore keep numeric priority lists valid after adds, reorders, and selective transfers instead of treating them as inert prose or leaving stale ordinals behind. `pending_cmd.rs` also now lets legacy `remove` / `prune` helpers understand ordered parents. Updated the pending spec/runbook/transfer docs and added regression coverage for ordered parsing, renumbering, nested continuations, extraction, and legacy helper compatibility.

- **Backlog and icebox items now preserve nested indented lists as part of the parent task block.** `pending.rs` now treats only flush-left tracked parent lines as work entries and attaches following indented continuation lines to that parent item, so nested subtasks/dependencies survive backfill, edit, done, reap, reorder, shadow/history guards, and archive writes instead of being misparsed as standalone backlog entries. `extract.rs` now moves those nested blocks with their parent during selective `--items` transfers. Updated the pending spec/runbook text and added regression coverage for nested parsing, reorder, transfer, shadow detection, and archive preservation.

- **`do #id` closeout now treats icebox items as tracked work.** `session_check.rs` and `write.rs` now enforce missing-`--pending-done` against still-open ids from both `agent:backlog` / legacy `agent:pending` and `agent:icebox`. `pending_cmd.rs` now resolves `--pending-done <id>` in either tracked list surface, `preflight.rs` reaps completed icebox items through the same snapshot/archive closeout path as backlog items, and `plan.rs` emits `resolve_existing` for `do #id` directives that target icebox-only work. Updated the pending runbook/spec text and added regression coverage across pending mutation, precommit/session-check guards, plan output, and preflight maintenance.

- **Backlog and icebox headings are now preserved by granular mutations.** `pending.rs` now parses backlog bodies with non-item lines intact, so markdown headings and blank separators inside `agent:backlog` / `agent:icebox` survive backfill, reap, add, done, edit, clear, reorder, gate, and resolve operations instead of being dropped between the first and last bullet. `write.rs` also now normalizes accidental backlog replace-patches against the full non-item skeleton, which allows unchanged headers to survive the compatibility path while still rejecting real non-list edits. Updated the pending runbook/spec text and added unit + write-path regression coverage for header preservation.

- **Transfer now treats `agent:icebox` as a first-class tracked list surface.** `extract.rs` now accepts `--items` for `icebox` as well as `backlog`/legacy `pending`, resolves the backlog alias consistently during transfer lookups, auto-creates missing targets with the full status/exchange/queue/backlog/icebox scaffold, and when moving a non-list component also carries both backlog and icebox items into the target instead of only the backlog. Updated the transfer runbook/command spec and added regression coverage for auto-created target scaffolding plus selective icebox transfer.

- **Full exchange compaction now carries forward live backlog, queue, and icebox context by default.** `compact.rs` now replaces a full `exchange` compact with a default `### Session Summary` that includes the archive pointer plus concise state from the live `agent:backlog` / `agent:pending`, `agent:queue`, and `agent:icebox` components whenever no custom `--message` is supplied. The bundled compact-exchange runbook and command spec now also direct agents to treat those components as the canonical compaction inputs, with `prompt_presets` limited to optional summary-policy tuning. Added regression coverage for the default summary plus runbook assertions for the new context rules.

- **Invalid YAML frontmatter now surfaces a document-targeted startup error instead of raw parser noise or silent sync skips.** `frontmatter.rs` now wraps parse failures with the document path, parser message, and when serde_yaml reports a location, a compiler-style excerpt of the frontmatter with a caret at the reported line/column before the `--- ... ---` repair hint. `start.rs` and `route.rs` use that wrapper directly, so malformed frontmatter fails closed with actionable feedback. `sync.rs` now logs the same contextual warning during file resolution and auto-start, mirrors it into the document's `agent:status` component when present so editor-driven auto-start failures are visible even without a pane, and clears only that managed status note once the file parses again. Added regression coverage for the shared parse wrapper, sync status round-tripping, and sync-phase error context.

- **Strict queue closeout now proves both sides before advancing the queue.** `write.rs` no longer mutates the live queue before later strict closeout gates run, and queue consumption now computes the document + snapshot transforms fully before writing either one. Required closeouts therefore keep the head prompt in place when pending maintenance / pending guards reject the cycle, instead of partially advancing the queue in the working tree or snapshot before the commit boundary. Added finalize integration coverage for the rejected-closeout case and updated the queue consumption specs.

- **Route lazy-claim no longer commandeers the tmux session's current active pane.** `route.rs` now requires explicit pane provenance for Strategy 2 recovery after a dead registered pane: `find_target_pane()` only accepts an explicit pane override, still rejects already-claimed panes, and keeps the existing non-agent-process guard. When no explicit safe candidate exists, route falls through to auto-start instead of silently adopting an unrelated Codex/Claude pane from the same tmux session, repo, or nested registry. Updated the session-routing spec and added regression coverage for the explicit-only gate.

- **`claim` now rejects live cross-session tmux mismatches unless `--force` is explicit.** `claim::run()` no longer logs and proceeds when `cross_session_decision()` resolves to `Reject`. A pane in another healthy tmux session now aborts the claim with a concrete error telling the operator to switch sessions or pass `--force`; only stale configured sessions still auto-accept. Updated the claim/session-routing specs and added regression coverage for the fail-closed enforcement helper.

- **Legacy done backlog items now get ids before reap instead of disappearing silently.** `pending::reap_with_items()` no longer tolerates completed items with empty ids; it fails closed unless callers backfill first. `backlog reap` and stale-completed-item `repair` now canonicalize missing ids/checklists before removal, so legacy/manual `- [x]` lines are reaped and archived with stable references instead of being dropped without a trace. Added regression coverage for the pure helper, CLI backlog reap, and repair path.

- **Live-owner proof now recognizes pane-relative start paths as the same document.** `sync.rs` path matching no longer requires the running `agent-doc start <file>` argv to contain the exact registry string. When a submodule-hosted pane starts with a narrowed path like `tasks/docs.md`, root-level ownership proof for the same document now still matches the longer superproject form such as `src/session-share/tasks/docs.md` by normalized path-component suffix. This closes the false `NoLiveOwner` / stale-deregister shape that appeared in root `resync` output when the supervisor socket was unavailable and only the process-tree match remained. Added regression coverage for the submodule-relative and negative-path cases.

- **Missing commit-boundary recovery is now limited to exchange-only historical patchbacks.** `repair`, `preflight`, and `session-check` now share a narrow self-heal path for open `response_captured` / `write_applied` cycles and log-only write-complete/no-commit tails: when `HEAD` already proves the response landed as an exchange-only patchback, the snapshot/cycle/capture state is advanced to committed without synthesizing a new response write. Historical bypasses that also mutate typed components such as `status` / backlog / pending, or that still leave a bare prompt target in the repaired tail, now fail closed instead of being silently adopted. Added regression coverage across `git`, `repair`, `session-check`, and `preflight`.

- **Completed-backlog repair no longer lets preflight swallow a live prompt into `no_changes`.** When `repair` reaps stale `- [x]` backlog items with no pending response/capture, it now mirrors that reap into the snapshot surgically from the snapshot's backlog/archive components instead of re-saving the whole live document. That keeps prompt-bearing exchange edits visible for the next `preflight` diff and prevents the `#nodiffswallow` shape where a plain prompt inserted before `agent:boundary` could survive in the file but disappear behind `no_changes: true`. Added regression coverage for both the direct `repair` path and the full `preflight` closeout path.

- **Strict post-write closeout is now shared across `run`, `finalize`, `write --commit`, `repair`, and the Codex Stop-hook.** `write.rs` now exposes one binary-owned helper that runs `git::commit()`, requires the cycle state to be closed, retries once when the snapshot still differs from `HEAD`, and then enforces `session-check`. `run.rs`, `repair.rs`, and `codex_hook.rs` now use that same helper instead of weaker ad hoc `git::commit()` paths. This closes the `#patchregr` family where a response path could look successful after commit/no-op closeout without proving the same post-commit invariants as `finalize`. Added regression coverage for the already-committed-plus-later-prompt-drift shape.

- **Bare `compact exchange` directives now fail closed unless the binary compaction path is used.** `run.rs` now rejects a pending diff that contains a direct `compact exchange` request instead of sending a normal agent-response cycle. `write.rs` / `finalize` apply the same pre-write guard against unresolved compaction directives, and `plan.rs` now emits a `Compact` handoff with `agent-doc compact <file> --commit` instead of a misleading finalize placeholder. Added regression coverage for both the `run`/`write` guards and the new plan handoff.

- **Route now reuses or restarts the registered pane via supervisor health before spawning a fallback session.** When `route.rs` cannot prove live ownership from tmux process args or supervisor child PID, it now still queries the registered pane's supervisor socket before treating that pane as stale. Healthy supervisors are reused in place; reachable halted/degraded supervisors get a `restart` IPC and keep the same pane; only unreachable/missing sockets fall through to deregister + fresh auto-start. This closes the duplicate-session shape where a supervisor was still alive but sitting at the Ctrl-D restart prompt. Added route coverage for both the fresh-start decision helper and the registered-pane restart path.

- **Explicit-baseline writes now keep concurrent user edits out of the next snapshot baseline.** `write.rs` now persists `content_ours` instead of merged disk `final_content` only when an explicit `--baseline-file` was supplied and the live file diverged during the response merge. That keeps user edits pasted while `finalize` is completing visible in the next diff instead of silently absorbing them into the snapshot. Non-baseline writes still persist the final merged disk state as before. Added regression coverage for both paths.

- **Stale startup-miss markers no longer spawn duplicate fallback sessions.** On rerun, `route.rs` now checks whether the pane named by a persisted startup-miss marker has since resumed proving live ownership of the document. If it has, route clears the stale marker and reuses that pane instead of deregistering it and auto-starting a second session. Added a route regression for the fresh-start decision helper and updated `specs/07-commands.md`.

- **`resync --fix` now preserves active bound panes even when session/window cleanup heuristics disagree.** `resync.rs` now requires the live-owner proof to resolve back to the registered pane itself; if another pane owns the file, the registration is treated as stale `NoLiveOwner`. When the registered pane does still prove live ownership, `WrongSession` / `WrongWindow` fix paths preserve that active bound session instead of killing or stashing it based only on foreground-command or layout heuristics. Updated `specs/07-commands.md`.

- **Stabilized the remaining tmux readiness/full-suite regressions.** The `route` prompt-readiness tests now wait for an actual idle shell before injecting their mock agent, and the `resync` wrong-window tests no longer depend on process-global cwd or fixed sleeps for pane/window relationships. This keeps the parallel `cargo test` suite deterministic without changing runtime command behavior.

- **`start` no longer relocates the launcher pane before deciding to reuse a live owner.** The wrong-session auto-relocation now runs only on the fresh-start path after `start` has already ruled out successful reuse/restart of an existing live owner pane. This closes the cross-session bug where invoking `agent-doc start <file>` from another tmux session could `join-pane` the transient launcher into the project session, making the caller's original window disappear or look like a crash even though the command was about to reuse a different pane. Added a regression test that proves the reuse focus path keeps the launcher pane in its original session.

- **Snapshot-committed guard catches response patchbacks that were never committed.** `session-check` now verifies that the current snapshot matches `git show HEAD:<file>` in the owning git root after a committed cycle. If the snapshot differs from HEAD, the response patchback is visible but was never committed — `session-check` exits `1` with a specific diagnostic. `finalize` retries the commit once before handing off to `session-check` when it detects this mismatch. Additionally, `commit` now updates the parent submodule pointer even during no-op (`commit_already_current`) cycles when the pointer is stale. Added `verify_snapshot_committed()` and `is_submodule_pointer_stale()` to `git.rs`, the snapshot-committed guard to `session_check.rs`, retry logic to `write.rs`, and 6 regression tests.

- **Backlog-replay guard detects open items silently dropped from recent history.** `preflight` and `session-check` now compare the current document's backlog against the pre-cycle baseline (`.agent-doc/baselines/`, falling back to `git show HEAD`). Open items present in the baseline but completely absent from the current document — not in live backlog, not in icebox, not in shadow/commented sections, and not in the cycle's `pending_done_ids` — fail closed. This prevents the bug where open backlog items disappear during a response cycle with no shadow copy to trigger the existing shadow guard. Added `detect_dropped_from_history()` detector in `pending.rs`, guards in both `session_check.rs` and `preflight.rs`, and 8 regression tests.

- **Codex Ctrl-D now shows quit menu instead of auto-restarting fresh.** `restart_continue_exit_strategy()` now routes `ctrl_d_forwarded` to `PromptUser` so the user sees "Press Enter to restart fresh, or 'q' to exit" instead of an automatic fresh restart. The supervisor log records `ctrl_d_prompt_user` / `user_quit_after_ctrl_d` for the new path. The `RestartFresh` handler no longer contains a dead Ctrl-D branch. Updated supervisor spec and regression tests to match.

- **Startup-miss tracking makes fresh-start failures visible instead of looking like dead panes.** When a fresh-start or routed-trigger cycle acknowledgment times out, `route.rs` now records a startup-miss marker at `.agent-doc/state/startup-miss/<doc-hash>.json` and echoes a diagnostic into the pane so the user sees "startup-miss: ..." instead of an unexplained idle shell. On rerun, route detects the marker on the registered pane, deregisters it, and auto-starts fresh instead of reusing a pane that never started a document cycle. Successful acknowledgment clears the marker. `session-check` reports a warning when a startup-miss marker exists. Added `startup_miss` module with persistence/load/clear/detection, 4 unit tests, 4 route-level integration tests, and updated `specs/07-commands.md`.

- **`start` reuse now probes supervisor health before switching focus.** When `start` finds a live owner pane, it queries the supervisor IPC `state` method. Healthy sessions get focus-switched as before. Unhealthy sessions (halted/degraded/not-running) get a `restart` IPC command; if that fails, the stale registration is cleared and a fresh supervisor starts in the current pane. Panes with unreachable or missing supervisor sockets are deregistered and replaced. This closes the case where `agent-doc start <file>` silently switched to a stuck or dead session.
- **Cross-session `start` reuse now switches the current tmux client before focusing.** When the live owner pane is in another tmux session, `start` now uses a current-client focus path that switches to the target session first, then selects the window and pane. This closes the false "switching focus" success case where the reuse path proved a live owner but left the user in the old tmux session.

- **Successful duplicate `start` reuse no longer prints shared `[sync]` probe diagnostics.** `start.rs` now uses a quiet live-owner lookup when it is only deciding whether to reuse an already-running pane, so the happy path emits only the start-level reuse/focus messages. `route` and `resync` keep the richer `[sync]` owner-proof logging they use for recovery and diagnostics.

- **Duplicate live `start` now reuses the existing pane instead of erroring.** `start.rs` now excludes the current transient `agent-doc start <file>` pane when probing for live owners, focuses any already-running owner it proves, and re-registers to that pane when the registry was stale. If the registry points at a different alive pane but no live owner can still be proven, `start` now clears that stale binding and proceeds in the current pane instead of failing closed forever. Added start-level regression coverage for reuse, stale-alive clearing, and same-pane/dead-pane cases.

- **`resync` now shares route's live-owner proof and stale-owner recovery.** `sync.rs` now exposes a shared ownership probe that first scans tmux process trees for the document path and then falls back to the per-session supervisor PID. `resync.rs` reports alive-but-unowned registrations as `NoLiveOwner`, `resync --fix` deregisters them without killing the pane, and `route.rs` now clears that same stale binding before continuing with lazy-claim / auto-start recovery instead of failing closed immediately.

- **Stash cleanup no longer preserves every unregistered agent pane by default.** During `resync --fix`, unregistered `agent-doc` / `codex` / `claude` panes in stash are now kept only when the shared live-owner proof still ties them to some registered document. Otherwise they are purged as orphaned agent panes. Added regressions for stale-owner detection, lazy-claim recovery, and stash cleanup.

- **Codex Ctrl-D clean exits now prompt the user instead of silently resuming.** `start.rs` treats stdin EOF/Ctrl-D on a clean Codex exit as a prompt path (Enter to restart fresh / q to exit) so the user can choose to quit the supervisor cleanly. Single failed resume handoffs stay on the fresh-restart path before escalating to a prompt after repeated failures. Added start-level regression coverage for the exit-strategy split and updated the supervisor/Codex support docs to match.

- **Live-pane route ownership now falls back to supervisor PID before declaring ambiguity.** `route.rs` still prefers a tmux process-tree match on the document path, but when a registered pane is alive and the long-lived `agent-doc` supervisor no longer exposes that file path in argv, route now queries the per-session supervisor socket for the live child PID and maps that PID back to the owning tmux pane. This closes the JetBrains/IDE reroute shape where a live `agent-doc` pane was refused as "ambiguous" even though the supervisor still owned the document session. Added route regression coverage for recovering the live pane via supervisor PID when argv loses the file path.

- **Failed fresh-route cleanup no longer kills the new live pane.** When route creates and registers a new pane for a document but later fails closed because fresh-start acknowledgment was not observed, `route.rs` now preserves that pane if it is still the live registered owner instead of cleaning it up as an orphan. This keeps `fresh_route_start_missing` / `fresh_route_trigger_missing` from surfacing to the user as a tmux pane crash. Added route coverage for both preserving the registered owner and still cleaning up truly unregistered panes.

- **Resume auto-trigger cancellation now cuts through the shared child-pty writer path.** Supervisor shutdown now flips both the auto-trigger stop flag and the stdin->pty writer stop path before joining either thread, the auto-trigger waits for the shared writer mutex interruptibly, and Unix child-pty writes now poll in short intervals so cancellation can break backpressure instead of hanging behind `stdin->pty`. Added regression coverage for cancelling while the writer lock is busy and updated the supervisor spec to document the shutdown ordering.

- **Resume auto-trigger now proves the prompt from current child PTY output.** The restart watcher no longer decides readiness from `tmux capture-pane` history. It now watches the filtered output emitted by the current resumed child and only injects once the latest non-empty line is a harness prompt, so stale visible prompts left in tmux scrollback cannot trigger an early resume command. Added regression coverage for latest-line prompt detection and updated the supervisor spec/module contract to match.

- **Resume auto-trigger now injects through the child pty instead of pane stdin.** The restart watcher still waits for a visible harness prompt via `tmux capture-pane`, but once the prompt appears it now writes the trigger command directly through the supervisor-owned child pty writer instead of `tmux send-keys`. That closes the `#rvinjectrace` window where a stale watcher could inject into the supervisor restart prompt or a later replacement process after the resumed child died during the trigger handoff. Added regression coverage for carriage-return injection, late cancellation before write, and closed-writer failure during the trigger window.

- **Historical snapshot repair now adopts committed `HEAD` before later local drift.** When `session-check` or `commit` sees that `HEAD` already contains a previously bypassed assistant response, snapshot repair no longer requires the live worktree to be exactly `HEAD` or `HEAD` plus an exchange-only prompt follow-up. It now advances the snapshot to the committed `HEAD` state for any later local drift that does not introduce a newer `### Re:` / `## Assistant` block beyond `HEAD`, then reclassifies the remaining user edits normally. This closes the stale-snapshot/manual-commit `#pbc2` shape where a structurally valid committed response was still misreported as a direct patchback bypass. Added regressions for both `session-check` and `commit` on the committed-head-plus-local-status-edit case.

- **`agent-doc backlog` is now the canonical backlog CLI, with `agent-doc pending` retained as a deprecated alias.** The top-level backlog management subcommand now lives under `agent-doc backlog ...`; invoking the legacy `agent-doc pending ...` spelling still works for compatibility but emits a deprecation warning directing callers to the canonical name. Updated autocomplete command metadata and integration coverage for both the canonical and deprecated spellings.

- **Completed backlog reap now fails closed when persistence is incomplete.** Preflight no longer downgrades reap-persistence problems to a warning: if it removes `- [x]` backlog items from the working tree but cannot verify the same reap in the staged snapshot, the cycle stops before commit instead of silently letting completed items survive. `session-check` now also fails closed when a supposedly clean committed document still contains stale completed backlog items from an older cycle. Added regression coverage for the happy path, the missing-snapshot-backlog failure, and the post-commit closeout guard.

## 0.33.16

- **Pending add/backlog normalization now fail closed on malformed leading id prefixes.** Active `--pending-add` parsing still accepts canonical `id=<custom> ...` and compatibility `[#custom] ...`, but it now rejects bare `[#]` placeholders, empty `id=` prefixes, and stacked leading prefixes like `[#a] [#b] ...` or `id=a [#b] ...`. The accidental `replace:pending` / `patch:pending` normalization path still repairs a lone legacy `- [ ] [#] ...` line into a generated id, but it now blocks the stacked-prefix shape before any malformed prefix text can persist into backlog content. Added unit coverage for the add-time parser and write-path regression coverage for normalize-vs-reject behavior.

- **Submodule sessions now expose the parent working tree to workspace-write harnesses.** `append_workspace_access_args` no longer limits submodule-hosted Claude/Codex sessions to external git metadata dirs. Fresh launches now also add the superproject working tree as an extra writable root, so a session started in `src/session-share` can still patch parent-repo docs such as shared backlog files without misreporting them as outside the writable root. Existing Codex resume behavior is unchanged: `exec resume` still strips `--add-dir` because the resumed thread inherits those writable roots from the original exec. Added regression coverage for both the computed workspace-access dirs and the actual appended Codex args.

- **Already-committed closeout now blocks bypassed response patchbacks.** When the staged snapshot already matches `HEAD` but the working tree contains a likely direct assistant patchback (`### Re:` / `## Assistant`) with no newer `agent-doc` cycle, `git::commit` now fails closed instead of classifying that state as ordinary post-commit working-tree drift and returning `commit_already_current`. This closes the `#pbypass1` shape where a session doc could show a restored response but stop at "Nothing has been committed," leaving the patchback outside the binary-owned commit boundary. Added regression coverage for the committed-HEAD plus bypassed-response case.

- **Session closeout now fails before commit when completed backlog items omit `--pending-done`.** `write`/`finalize` gained a pre-commit pending-done gate that compares the active response capture against still-open backlog ids and blocks commit when a response clearly completes `#id` but the cycle recorded no matching `--pending-done <id>`. Session documents now default `pending_done_guard` to `strict` unless frontmatter or project config downgrades it, while non-session docs keep the old warn default. Added unit coverage for default/recorded/warn/suppressed paths plus integration coverage proving `finalize` leaves `HEAD` unchanged when the gate trips.

- **Blank `--window` sync scope now fails safe instead of reconciling the whole tmux server.** `sync.rs` now normalizes empty/whitespace-only window overrides to "unset" before repair, auto-start scoping, and `tmux_router::sync`, and `route.rs` ignores blank `context_session` overrides the same way. This closes the tmux-instability path where a JetBrains/plugin sync passed an empty window id, producing `target_window=` / `session=""` reconcile state that detached unrelated live panes into stash and triggered follow-on duplicate starts. Added regression coverage for blank sync/window scope normalization.

- **Stash rescue no longer swaps a live pane out of view.** `route.rs` and `sync.rs` now rescue stashed session panes back into the `agent-doc` window with guarded `join-pane`, placing them on the requested left/right edge instead of preferring `swap-pane`. This closes the remaining `claudescore-3.md` tmux swap/recovery bug where a recovered pane could displace another live pane into stash and only appear to "heal" on a later reroute. Added route/sync regressions that prove the existing visible pane stays in the `agent-doc` window during rescue.

- **Duplicate live `start` now fails closed before spawning a second pane.** `agent-doc start` now checks whether the document session UUID is already registered to another alive tmux pane and refuses to launch a duplicate live harness in the new pane when it is. This closes the `corky.md` restart failure class where the same session id was repeatedly started on `%194/%196/%197/%198/%199`, destabilizing other active panes instead of reusing the already-live session. Added start-level regression coverage for alive/same-pane/dead-pane cases.

- **Already-present recovery closeouts now advance the snapshot before commit.** When a reopened repair/Stop cycle finds that the live document already contains the assistant response but the snapshot still lags behind, `repair` now advances the snapshot and `write_applied` phase before the commit boundary runs. That closes the Codex direct-patch bypass shape where the response was visible in the document, but the later commit path downgraded the turn to post-commit local drift and left it unowned. Added regression coverage for the committed-cycle + direct-patch + already-applied recovery path.

- **Boundary-artifact-only preflight now stays cycle-free.** `preflight` no longer opens `preflight_started` on pure agent-owned `(HEAD)` / boundary churn in template docs. It classifies that shape first, collapses it back to `no_changes` / already-committed closeout, and prevents that transient drift from leaking a stale user-visible lock. Added regression coverage for the exact clean-snapshot plus transient-`(HEAD)` shape that previously surfaced as `cycle started but no write/commit followed`.

- **`compact exchange` write-back now replaces `agent:exchange` for that turn.** When the user-added diff explicitly starts with a direct `compact exchange` directive, template/CRDT write paths now override the normal append mode for `agent:exchange` and force replacement semantics instead. That closes the failure where repeated compaction requests kept appending new checkpoint summaries over older `### Re:` history instead of collapsing the component to one compacted checkpoint. Added directive-detection, template apply, and repair/write regression coverage for both patch-based and raw-response closeouts.

- **Route start-ack now rejects same-cycle committed churn.** `route.rs` no longer treats mutations to an already-committed baseline cycle as proof that a new document cycle started. When a routed or fresh trigger is dispatched against prompt-bearing drift on top of a closed cycle, acknowledgment now requires a genuinely newer cycle id; same-cycle `commit_already_current` updates fail closed instead of logging a false `route_cycle_start_acknowledged`. Added regression coverage for the exact same-cycle false-ack shape.

- **Route/sync now fail closed instead of inventing fallback tmux sessions or force-moving live stash panes.** `route.rs` no longer rewrites `config.toml` when a configured `tmux_session` is dead, refuses auto-start into an implicit dead fallback session like `"claude"`/`"codex"`, and re-registers an already-running pane for the same file before lazy-claim/auto-start. `sync.rs` now preserves stashed panes that belong to another live tmux session instead of moving them across sessions during rescue. Successful replacement paths also preserve prior stash panes unless there is explicit provenance for cleanup. Added regression coverage for dead implicit fallback refusal and non-destructive stash replacement.

- **Live-pane reroutes now require real cycle acknowledgment for pending prompt drift.** `route.rs` now applies the same fail-closed start-ack rule to dispatches into an already-running pane when the document already has unresolved `prompt_target` / `content_edit` drift on top of a closed cycle. A consumed routed trigger no longer counts as success by itself; route waits for a newer per-document cycle state and fails closed if none appears. Added route coverage for both the acknowledged and missing-ack live-pane shapes.

- **Post-commit stale-buffer guard for `codex (HEAD)` drift.** JetBrains post-commit boundary reposition now prefers the just-committed on-disk document when the open buffer differs only by agent-owned `### Re:` heading attribution and/or boundary churn. That prevents the stale-buffer failure where a successful patchback commit was immediately re-dirtied to `codex (HEAD)` with a newer boundary marker. Added JetBrains regression coverage for the prefer-disk decision and Rust closeout coverage that repairs historical heading-attribution drift back to clean `HEAD`.

- **`session-check` now catches startup-miss prompt drift.** When a session document already has unresolved prompt-bearing user edits (`prompt_target` / `content_edit`) relative to its snapshot, but no newer `agent-doc` cycle ever started, `session-check` now fails closed instead of reporting the stale committed state or `no cycle state or ops.log — ok`. The Codex Stop hook inherits that signal and can auto-close the missed-start case from `last_assistant_message` through the normal repair/write/commit path. Added `session_check` and Codex hook regression coverage.

- **Session-document `write --commit` now fails closed.** `write --commit` still behaves as a best-effort helper for non-session docs and `--pending-only`, but when it is writing a response into a real session document (`agent_doc_session` / legacy `session`) it now upgrades to the same strict closeout contract as `finalize`: reject non-git docs before mutation, fail the command on commit failure, and only return success once the cycle reaches `committed`. Added CLI integration coverage for gitless/session, git-backed/session, and non-session best-effort behavior.

- **Normalize accidental pending patches before capture/replay.** When a response still contains a single list-shaped `replace:pending` / `patch:pending` block, the write path now translates it into granular pending mutations before durable capture instead of capturing first and then failing on `replace:pending block forbidden`. That closes the `response_captured` orphan path behind `#pendops`. `repair` replays the same historical capture shape through the same normalization path, while unsupported pending/backlog patch shapes still fail closed before capture. Added live-write and repair regression coverage.

- **Fresh Codex start now requires real cycle acknowledgment.** `route.rs` no longer treats a consumed `agent-doc <file>` trigger as sufficient proof that a fresh pane started successfully. After trigger injection, route now waits for a new per-document cycle state (`preflight_started` or later) before declaring success, logs `fresh_route_start_acknowledged` / `fresh_route_start_missing`, and fails closed if the file never enters a real cycle. Added route unit coverage for fresh-cycle, fast-commit, and timeout shapes. Specs updated to document the stronger startup contract.

- **Fix Codex submodule handoff.** `codex exec resume` does not accept `--add-dir`, but `append_resume_args` was passing it through from `base_args`. The Codex backend now strips `--add-dir` (both `--add-dir <DIR>` and `--add-dir=<DIR>` forms) from resume args. Resumed sessions inherit writable roots from the original `exec`, so stripping is correct behavior. Specs updated to document backend-specific handling.

- **Pending-capture guard now catches single unresolved bug/follow-up prose.** The recommendation heuristic no longer requires a numbered batch when the response clearly identifies a current issue as still needing follow-up (for example, "still hitting the older ... bug that X was meant to close"). Strict `finalize` now blocks those uncaptured single-item responses before commit, and `session-check` warns on the same shape post-commit. Added regression coverage for unresolved-vs-resolved bug prose.

## 0.33.15

- **Supervisor model injection from frontmatter.** `start.rs` now injects `--model` from `claude_model` / `codex_model` / `model` frontmatter when the freeform args (`claude_args`, `agent_args`, etc.) don't already contain `--model`. Precedence: harness-specific field (`claude_model` for Claude, `codex_model` for Codex) > generic `model` field.

- **Pre-commit pending capture gate in `finalize`.** When `pending_capture_guard: strict`, `finalize` scans the response for uncaptured recommendations before committing. If recommendation-like items are detected without `--pending-add` flags, finalize exits non-zero before the commit step.

- **`plan` emits `ExpectAdd` pending mutations.** When prompt targets contain backlog/recommendation signals ("tasks", "todo", "backlog", "what's next", "recommendations", "next steps", "action items"), `plan` emits an `expect_add` entry in `pending_mutations`. Tells the skill that finalize should include `--pending-add` flags for actionable items in the response.

- **Post-preflight planning command.** `agent-doc plan <FILE>` emits a structured planning/dispatch record with `prompt_targets`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers`.

## 0.33.14

- **Inline guard marker stripping.** `strip_guard_markers` now removes `<!-- no-pending-capture -->` and `<!-- no-pending-done-guard -->` from within content lines (not just standalone lines where the entire trimmed line equals the marker). Trailing whitespace is trimmed after removal. Previously, inline markers like `**Bold text** <!-- no-pending-capture -->` survived into committed blobs.

- **Rename `agent:pending` → `agent:backlog`.** The component is now canonically `<!-- agent:backlog -->` with `agent:pending` accepted as a backward-compatible alias. `patch=replace` attribute on backlog/pending tags is deprecated and auto-stripped. Added `agent:icebox` component to template scaffold for parked items.

- **`agent-doc migrate` command.** New subcommand for deprecated component name/attribute migrations (e.g., `pending` → `backlog`).

- **Per-harness model override.** Frontmatter `claude_model` and `codex_model` fields allow different model selections per harness, resolved through the existing tier/config precedence chain.

- **Snapshot auto-migration on document rename.** State files (snapshots, baselines, captures, CRDT) now follow when a document path changes, preventing orphaned state after renames.

- **Pane eviction guard.** `route.rs` now skips tmux pane eviction when an agent process is still active, preventing mid-response pane recycling.

- **Route trigger path resolution.** Trigger paths are now resolved to absolute paths, preventing submodule CWD misrouting when the working directory differs from the document's repo root.

- **Pending-capture heuristic fix.** Detects unconditional follow-up patterns that were false-positive-triggering the recommendation batch guard.

- **Queue component (Phase 1–3).** Parser, data model, template scaffold, preflight integration, trigger resolution, consumption, dispatch, and halt detection for `<!-- agent:queue -->` orchestration.

- **Prompt preset expansion in orchestrate.** Frontmatter `prompt_presets` are now resolved during orchestrate task expansion, and `--plan` flag previews expanded prompts without execution.

- **Post-preflight planning command.** `agent-doc plan <FILE>` emits a structured planning record (prompt targets, repo actions, required commands, pending mutations, blockers, handoff) for the skill to execute against.

- **Compound task steering runbook.** Bundled guidance for normalizing multi-clause directives into explicit sequential steps.

- **Orchestrate synonym dispatch runbook.** Natural-language phrasing like "run these in order" maps to `orchestrate --mode sequential|parallel|dag`.

- **Orphaned supervisor socket GC.** Stale supervisor sockets are cleaned up automatically.

- **IPC snapshot integrity validation.** `start` now validates snapshot integrity before launching the IPC listener.

- **Code formatting cleanup.** Applied rustfmt across 8 source files.

## 0.33.13

- **Workspace-write submodule sessions now auto-add external gitdirs.** When a session document lives in a git submodule, the harness launch path and fresh-agent backends now append `--add-dir` entries for the submodule's external gitdir under the superproject `.git/modules/...` tree plus the superproject `.git` used by parent-pointer updates. That keeps normal workspace-write Claude/Codex sessions from tripping permission failures on submodule commits while preserving the existing arg-precedence chains. Added regression coverage for external-gitdir discovery and for Claude streaming preserving extra `--add-dir` args when switching to `stream-json`.

- **`agent-doc orchestrate` now executes real DAG batches.** The shared orchestration surface still resolves task batches from repeated `--task`, `--from-file`, and `--from-exchange`, but `--mode dag` now parses optional `[id=... after=...]` metadata, falls back to the first `#token` in each prompt as the node id, validates duplicate/missing/cyclic dependencies, and runs the resulting graph in deterministic topological order through the same per-step `inject -> preflight -> fresh agent -> finalize -> session-check` lifecycle. This gives same-document fan-in semantics without pretending concurrent patchback is safe. Added unit coverage for DAG metadata parsing, unknown-dependency and cycle failures, and topological execution order.

- **Legacy `parallel` now routes through the orchestrate dispatcher.** `agent-doc parallel` remains available, but it now forwards its explicit task list into the same `orchestrate --mode parallel` routing layer used by the newer command surface instead of bypassing orchestration entirely. This keeps task normalization and mode dispatch in one place while preserving the existing parallel backend and its empty-task compatibility behavior. Added coverage for shared parallel dispatch and the legacy compatibility path.

- **Compound single-line task steering is now bundled into the skill surface.** The installed skill/runbook now explicitly tells agents to normalize directives like `do #ntoc. Add to today's news. commit + push` into explicit sequential or dependency-ordered steps before execution instead of treating them as one opaque prose task. The command spec now documents that this remains skill-side steering, not binary-owned free-form parsing, and regression coverage locks the new bundled runbook into the installed harness content.

- **Pending ordering guidance now covers late additions from an existing ordered batch.** The bundled skill and `pending-ops.md` runbook still treat front insertion as the default, but now document the exception for follow-on steps: if Step 1 / Step 2 are already captured and you later promote Step 3, add it with a canonical custom id and reorder it into place adjacent to its predecessor in the same cycle instead of prepending it above earlier steps. Added regression checks for the new bundled guidance so the skill surface keeps the `#9pw9`-style placement rule.

- **Skill auto-update now targets the active harness explicitly.** Installed instruction content now renders `agent-doc-version` from `CARGO_PKG_VERSION` instead of inheriting a stale literal from the source template, Codex environment detection now recognizes live Codex shell vars like `CODEX_THREAD_ID` / `CODEX_CI`, and the rendered auto-update step now uses harness-specific install/reload commands (`--harness claude --reload compact` for Claude Code, `--harness codex --reload restart` for Codex). Added regression coverage for the new detection signals plus rendered Codex/Claude auto-update content.

- **Prompt-prefix enforcement now reuses the prompt-bearing classifier.** `write.rs` now treats prompt-prefix targets as a shared binary invariant derived from `diff.rs`'s canonical `prompt_target` classifier instead of relying only on a separate line-shape heuristic, and `session-check` now reports bare prompt-target lines when a bypassed `### Re:`/`## Assistant` patchback left the transcript uncanonicalized. Added unit coverage for prompt-prefix target extraction and the new `session_check` failure shape.

- **Pending-capture guard in `session-check`.** Committed response captures are now scanned for recommendation-like batches (priority labels, numbered action lists, recommendation headers, imperative follow-ups) when the cycle recorded no `--pending-add` / `--pending-add-gated`. Default mode warns on stderr; `pending_capture_guard: strict` or project `[guards] pending_capture = "strict"` upgrades the condition to a nonzero `session-check`, and `<!-- no-pending-capture -->` suppresses the guard for intentional skips. Added heuristic unit coverage plus `session_check` coverage for warn, strict, suppression, and frontmatter-overrides-project precedence.

- **Unified prompt-bearing change classifier.** The diff/prompt contract no longer splits explicit `required response targets` from `inline_annotations`. `diff.rs` now classifies ordered user-authored changes as `prompt_target`, `content_edit`, `recovery_artifact`, or `boundary_artifact`, prompt builders render that typed section directly, and preflight surfaces the canonical list as `prompt_bearing_changes` while keeping `inline_annotations` as a compatibility projection. Added regression coverage for inline prompt promotion, inline correction classification, and response-artifact detection.

- **Committed captures no longer trigger repeat recovery dedup on later preflights.** `repair` now ignores terminal durable-capture states (`committed`, `discarded`) unless there is still a pending response file to reconcile, so routine `preflight` runs stop emitting the "`Response already present in document`" self-heal message after a cycle has already closed cleanly. Added regression coverage for the committed-capture/no-pending shape.

- **Post-commit editor refresh now reuses the committed boundary ID.** Standalone IPC `reposition` messages can carry the exact exchange `boundary_id`, and both editor helpers now preserve that marker instead of minting a new one after `commit()`. This closes the boundary-only dirty-worktree shape where the response was already committed but the editor saved a fresh marker afterward. Added Rust, JetBrains, and VS Code regression coverage for explicit-ID repositioning.

- **Imperative detection now recognizes natural-language pending tasks.** The executable-directive guard no longer stops at hard-coded `do #id` / `run tests` phrases: pending-item prose that starts with an imperative verb (for example `[#n8q4] Fix the cross-repo ...`) is now classified as executable intent too. That means status-only replies like "I'm starting now" are rejected for those diffs instead of letting actionable pending text be misread as non-directive continuation prose. Added unit coverage for diff extraction and finalize integration coverage for the pending-item shape.

- **Delayed recovery patchbacks now keep provenance.** Durable capture records now retain lifecycle timestamps like `replayed_at` and `committed_at`, and `ops.log` emits `capture_committed_after_replay` when a response only reaches the commit boundary after recovery replay. This preserves the distinction between "same-turn patchback succeeded" and "the response was written back later during recovery/closeout" for forensic analysis and user-facing explanations.

- **`commit` now explains post-commit local drift explicitly.** When the stripped snapshot already matches `HEAD` but the working tree still has later local edits, `agent-doc commit` now classifies that state as post-commit local drift, logs whether it was a user follow-up or broader working-tree edits, and closes the cycle without mislabeling the state as a generic out-of-band patchback warning. Added regression coverage for both the safe follow-up and later-local-edit shapes.

- **Stale snapshots can no longer rewind already-committed responses on no-op closeout.** If the snapshot lags behind a response that is already in `HEAD`, and the working tree only adds a new user follow-up on top of that committed state, `agent-doc commit` now repairs the snapshot up to `HEAD` before the `HEAD`-current no-op path runs. This prevents a later closeout from staging the old snapshot blob and momentarily rewinding the document before recovery re-adds the response. Added regression coverage for the exact stale-snapshot + follow-up shape.

- **Relative submodule doc resolution no longer falls through to outer-repo shadows.** When `agent-doc` is invoked from inside a submodule with a relative document path like `tasks/monsterrodholders.md`, path resolution now prefers the caller's existing cwd-local file before consulting the superproject root. This fixes the case where `commit` / `show_head` / related git paths could silently target an outer-repo document with the same relative path, leaving the intended submodule doc uncommitted even though the closeout logged success. Added regression coverage for the shadowed-path shape.

- **Executable-directive backstop in `run` + `finalize`.** The binary now inspects the pending user diff for imperative document directives (`do #id`, `run tests`, `build + install`, `commit + push`, and approval words like `go`) and rejects status-only/meta-only replies unless they include either concrete execution evidence or a concrete blocker. Added unit coverage for directive extraction + response classification and finalize integration coverage for the reject path.

- **Codex closeout contract hardened.** `agent-doc finalize` is now the strict happy path for normal session responses, Codex/direct-exec instructions require an immediate `agent-doc session-check <FILE>` after `finalize` or `write --commit`, and the installed Codex `Stop` hook can auto-close a pending response cycle from `last_assistant_message` before failing closed. Added CLI/integration coverage for the `finalize + session-check` path and the real Codex hook flow.

- **Codex hook state now survives root / turn drift.** The repo-local `UserPromptSubmit` / `Stop` bridge now mirrors active-session state across nested `.agent-doc` roots and still inspects the tracked document on later `Stop` events in the same Codex session, so a closeout cannot be skipped just because the harness CWD moved between the superproject and a submodule or because the next `Stop` arrives with a newer turn id. Added regression coverage for the nested-root replay path.

- **Interrupted-cycle + historical-drift repair.** `preflight` now fails closed on unrecoverable `preflight_started` cycles instead of snapshot-committing over newer live content, while `commit` / `session-check` can narrowly repair already-committed historical `### Re:` drift when `HEAD` proves the response is no longer out-of-band.

- **Bare-path compatibility restored.** `agent-doc <FILE>` once again aliases to `agent-doc run <FILE>`, keeping older wrappers working while the explicit subcommand form remains canonical.

- **Boundary cleanup invariants locked.** Boundary/head-marker cleanup is now regression-covered across the Rust path plus both editor helpers so stale boundary IDs and duplicate visible `(HEAD)` churn do not survive reposition.

- **Repo-scoped commit closeout serialization.** `git::commit()` now keys its advisory closeout lock by the resolved git dir / submodule git dir, blocks for the short critical section instead of proceeding unlocked, and retries the full stage+commit transaction when `index.lock` contention hits `update-index`, `git add`, or `git commit`. Added regression coverage for a staged `index.lock` retry and two different docs contending on closeout in the same repo.

- **`repair` now closes git-backed recovery in one command.** `agent-doc repair` (legacy alias: `recover`) no longer stops after replaying or deduping a pending response; when recovery work happened inside git it now immediately runs the normal commit boundary so repaired assistant content does not remain uncommitted until a later `preflight`. Added regression coverage for both replayed and already-applied repair paths.

## 0.33.12

- **Codex agent backend (Phase 1).** New `agent/codex.rs` implements `Agent` + `StreamingAgent` for the OpenAI Codex CLI. Parses Codex JSONL event stream (`thread.started`, `item.completed`, `turn.completed`). Session resume via `codex exec resume <id>`, fork via `codex exec resume --last`. Registered in `agent::resolve("codex")`. 11 unit tests covering event parsing, session ID propagation, and stream iterator behavior.

## 0.33.11

- **Fix: lib-install uses atomic rename to prevent mmap corruption.** `install_versioned()` in `lib_install.rs` previously used `std::fs::copy(source, &dst)` which overwrites the versioned `.so` in place (same inode). On same-version reinstall during development, this corrupted IDEA's live mmap of the `.so`, triggering a crash. Now copies to a temp file then calls `rename()` — atomic on POSIX, creates a new inode so existing mmaps stay valid. 1 new test: `same_version_reinstall_creates_new_inode`.

## 0.33.10

- **Fix: Component parser peek guard for non-agent HTML comments.** `parse()` in `component.rs` previously consumed any `<!-- ... -->` sequence in document content, causing the close-comment search to eat the next `<!-- /agent:name -->` marker. Now peeks 20 bytes after `<!--` and skips non-agent sequences (advances 1 byte) rather than consuming them. Fixes "unclosed component" errors when pending items contain literal `<!-- ` in their text. 5 new tests.

- **Fix: CRDT stale-base detection uses prefix+suffix.** `merge()` in `crdt.rs` previously only checked `common_prefix_len` to decide if the base was stale. Template documents have structural content (frontmatter, component markers, pending sections) at both ends — a short exchange meant only the prefix went uncounted, causing valid bases to be classified as stale and triggering duplicate-user-prompt bugs. Now computes `ours_shared = (prefix + suffix).min(base_len)` and uses that ratio for the 50% threshold.

- **Cleanup: Remove IPC degraded mode.** `is_ipc_degraded`, `mark_ipc_degraded`, and `clear_ipc_degraded` removed from `write.rs`. The ack-content sidecar mechanism (v0.33.x) made the degraded marker obsolete — sidecar ACK is authoritative; disk fallback handles the timeout path. Replaced with `cleanup_legacy_ipc_degraded` that removes any stale `.agent-doc/ipc-degraded` marker left by older installs.

- **JB plugin 0.2.71: writeAckContent fires on all patch paths.** Previously `writeAckContent` was only called from the VFS patch path; the two exchange-level patch paths omitted it. Now all three paths (WriteCommandAction exchange, VFS exchange, boundary-reposition) call `writeAckContent`, ensuring the ack-content sidecar always fires regardless of which code path processes the patch.

- **Fix: Makefile `test` target unsets git hook env vars.** `make test` now runs `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test`. When the pre-commit hook calls `make precommit`, git sets `GIT_DIR` to the outer repo — all temp-repo tests in the suite inherited this and routed their git subcommands to the wrong repo, causing 24+ test failures during commit. The `env -u` strips the hook vars before cargo test, restoring correct isolation.

## 0.33.9

- **Fix: CommitLock uses try_lock_exclusive to prevent indefinite hang.** `CommitLock::acquire` (git.rs) previously called `fs2::lock_exclusive()` which blocks indefinitely when another process holds the lock. In the IPC-sidecar-timeout fallback path (exit 75), the write to disk succeeded but `git::commit` blocked at the flock — causing the skill process to hang. Changed to `try_lock_exclusive()`: returns `None` immediately when contended, proceeding unlocked. Git's own `index.lock` retry loop (3 attempts with exponential backoff) handles serialization at the git layer.

## 0.33.8

- **Rename debounce (#qam7).** `agent-doc sync --rename` writes a 5s debounce marker (`.agent-doc/rename-debounce/<hash>.marker`) for the focused file; subsequent auto-start checks skip files with active markers. Prevents spurious pane creation when `FileRenameListener` (JB) or `onDidRenameFiles` (VS Code) triggers sync for a file with no alive pane. Both editor plugins now pass `--rename` on file rename/move events. JB plugin 0.2.70, VS Code extension 0.2.7.
- **Auto-start pane ID logging.** `route::provision_pane` now returns `Result<String>` (the new pane ID). Sync logs `[sync] auto-started %XX for <file>` per pane; when >1 pane starts in a single call, a batch summary is printed. Both messages written to `/tmp/agent-doc-sync.log`.
- **Tests + spec.** 5 new tests: 3 rename debounce unit tests, 2 batch summary formatting tests. Spec, contracts, and evals added for both features in `sync.rs`.

## 0.33.7

- **Boundary reposition CAS guard (JB plugin 0.2.68 + VS Code extension).** `repositionBoundaryViaDocument()` in `PatchWatcher.kt` and `repositionBoundaryWithDebounce()` in `extension.ts` now verify the document content is unchanged between the `document.text` read and `document.setText()` / `WorkspaceEdit.apply()`. If the user typed between `await_idle` timeout expiry and the write dispatch, the reposition is silently skipped rather than overwriting the new keystrokes. Adds `repositionBoundaryToEndUtil` / `findCodeBlockRangesUtil` as internal top-level functions (JB) and `repositionBoundaryToEnd` as a vscode-free module (VS Code) for unit testability. New: `RepositionBoundaryTest.kt` (7 cases) and `reposition.test.ts` (5 cases).

- **Skip working-tree boundary reposition when IPC available.** `reposition_boundary_in_snapshot()` in `git.rs` now checks for `.agent-doc/patches/` before touching the working tree. When the IDE plugin is installed (IPC path), the CLI skips the disk-level read-modify-write entirely and relies on the IPC reposition signal — eliminating the TOCTOU race where concurrent user typing could produce duplicate boundary markers in the committed state. New regression tests: `reposition_skips_working_tree_when_ipc_available` and `reposition_updates_working_tree_when_no_ipc`.

## 0.33.6

- **Inline annotation surfacing.** Preflight JSON added `inline_annotations: Vec<String>` as the original surface for user additions (`[user+]`/`[user~]`) inside agent response blocks. In later versions this becomes the compatibility projection of the broader `prompt_bearing_changes` contract.

- **False positive fixes for `inline_annotations`.** Two exclusion rules eliminate boundary artifacts: (1) `[user~]` lines where the only change is appending ` (HEAD)` to a heading are skipped — these are binary reposition artifacts. (2) `[agent]` lines that are component tags (`<!-- ... -->`), section headers (`# ...`), or blank are excluded from the "substantive agent lines after" check — end-of-exchange user input followed only by structural markers is now correctly classified as regular input, not inline annotations.

## 0.33.5

- **FFI library hot-reload (JNA + koffi).** Fixes SIGSEGV crash (PC=0x0) when `cargo install` overwrote `libagent_doc.so` while IDEA held it mmap'd via JNA. Both plugins now stat the `.so` on every `get()` / `ensureLoaded()` call; if mtime changed, they force `Native.unregister` + reload (JNA) or `koffi.unload` + reload (VS Code). One `stat(2)`/`statSync()` per FFI dispatch — negligible overhead. Race window narrows to sub-microsecond.

- **Versioned cdylib install.** `cargo install` / `make install` now writes `libagent_doc-<version>.so` and atomically updates the `libagent_doc.so` symlink via `ln -sfn` + `rename(2)`. The old inode stays alive in any running editor's mmap — editor restarts pick up the new version. Backward-compatible: `agent-doc lib-path` still returns `libagent_doc.so` (now a symlink). Legacy installs (regular file) are upgraded to the symlink layout on first install.

- **Lockfile-tracked GC (`agent-doc gc-libs`).** On JNA/koffi load, plugins write `<so-path>.lock` containing their PID; on clean exit (JVM shutdown hook / VS Code `deactivate()`), they remove the lock. `agent-doc gc-libs` walks all `libagent_doc-*.so` siblings: keeps the current symlink target and any .so whose `.lock` has a live `/proc/<pid>`; unlinks stale .so files and orphaned locks. Triggered on load, on install, and manually. Crash-safe: stale locks from SIGKILL'd processes are cleaned on next sweep.

- **Post-reload version sanity check (JB + VS Code).** After each native library (re)load, both plugins now call `agent_doc_version()` and log `[native] loaded libagent_doc v{version} from {path}` on success. Warns on null return or exception (ABI mismatch). Helps diagnose cases where a reload brings in an incompatible .so.

## 0.33.4

- **SKILL.md § 1b: pending promotion heuristic.** Agents now have an explicit rule: if a response ends with a numbered list of distinct, actionable recommendations and pending is empty (or the user asked for backlog/tasks), each recommendation must be added via `--pending-add` in the same write. Prevents actionable items from being silently lost as prose-only responses.

## 0.33.3

- **IPC sidecar timeout: fall back to disk write instead of claiming success.** `try_ipc()` previously returned `success: true` when the socket acknowledged but the sidecar ack timed out, causing the caller to skip the disk write path. If the plugin didn't actually apply the content, the response was silently lost. Fixed: sidecar timeout now returns `success: false`, so the caller falls through to the CRDT disk write path — the reliable fallback that always works.

- **IPC fallback patch file pre-write.** The disk patch file is now pre-written before socket send (overwriting any stale content) and cleaned on confirmed sidecar success. On sidecar timeout, the file is left for file watcher recovery as an additional safety net. `patch_id` deduplication prevents double-apply.

- **IDE buffer stale fix (JB plugin 0.2.64).** `repositionBoundaryViaDocument()` in `PatchWatcher.kt` now calls `reloadFromDisk(document)` after VFS refresh so the buffer picks up the CRDT-merged content before the boundary is repositioned. Previously the handler read the pre-merge buffer, repositioned the stale content, and wrote it back — burying the agent's response.

- **Runbook: agent-proposed forward actions must be `--pending-add`ed.** `runbooks/pending-ops.md` now requires any response ending with a forward-looking question ("Ready to X?", "Should we A or B?", "Shall I capture Y?") to add each concrete next-step option to `agent:pending` in the same cycle, so the proposal survives user non-reply.

## 0.33.2

- **`agent_doc_resolve_project_path` FFI export.** Editor plugins can now resolve a file's nearest agent-doc project root (the ancestor containing `.agent-doc/`) and the path relative to that root. Fixes a JetBrains plugin bug where `Run Agent Doc` on a file inside a submodule (e.g. `src/session-share/tasks/foo.md`) passed the full monorepo-relative path to the submodule's Claude session, producing `file not found`. Plugins now pass the submodule-relative path (`tasks/foo.md`) and use the submodule root as CWD.

- **IPC timeout path: CRDT merge instead of atomic_write.** The exit(75) fallback now uses the same CRDT merge as the normal disk write path, preserving all concurrent changes (user edits, pending mutations, structural modifications) — not just the `agent:pending` component. Falls back to `splice_pending_component` only if CRDT merge itself fails.

- **Recovery dedup fix.** `is_already_applied()` now checks each fingerprint line individually instead of joining them into a single substring. Fixes false negatives caused by blank-line separation between paragraphs and `(HEAD)` boundary suffixes on headings, which prevented the joined fingerprint from matching.

- **5 new tests** covering nested-submodule resolution, no-ancestor fallback, file-in-root, and recovery dedup with blank lines/boundary markers.

## 0.33.1

- **Pending parse fix: bare `[#]` placeholder accumulation.** `parse_item_line` now strips `[#]` markers instead of prepend-on-backfill, preventing placeholder accumulation across cycles.

- **Pending dedup on `--pending-add`.** `op_add` checks for identical text before appending, preventing duplicate items when the same add is retried.

- **Content-shrink guard for `--stream` writes.** `check_exchange_shrink_guard()` in `write.rs` refuses writes when new exchange content is < 10% of existing length (and existing > 100 bytes). Prevents accidental truncation from malformed heredocs or trivial payloads. Fires in both IPC and disk fallback paths. Overridable with `--force`.

- **9 new tests** for pending parse fixes and shrink guard (5 shrink guard + 4 pending).

## 0.33.0

- **Typed gate markers (`[/release]`, `[/deploy]`, `[/code-review]`, etc.):** Parser recognizes typed gates alongside plain `[/]`. Gate types are alphanumeric with hyphens/underscores, case-insensitive, stored lowercase. State machine: `[/release]` is a refinement of `[/]`; gate type is metadata on `Gated` state, cleared when resolved to `[x]`. Untyped `[/]` items are never touched by `resolve-gate`.

- **Per-file gate commands** (`agent-doc pending <FILE>`): `resolve-gate <type>` finds all `[/<type>]` items and flips to `[x]`. `set-gate-type <id> <type>` transitions `[/]` → `[/release]` (errors if not gated).

- **Project-wide `resolve-gate` command** (`agent-doc resolve-gate <type>`): Scans all `.md` files under project root (or `--scope <dir>`) for items with matching typed gates. Designed for hook integration:
  ```jsonc
  { "match": "cargo publish", "run": "agent-doc resolve-gate release" }
  { "match": "git push",      "run": "agent-doc resolve-gate deploy" }
  ```

- **Write command gate flags:** `--pending-resolve-gate <type>` and `--pending-set-gate-type id=type` for atomic pending+response cycles.

- **`--pending-add-gated` flag:** Add items pre-gated as `[/]` instead of `[ ]`. Available on both `write` and `notify` commands.

- **`--pending-only` flag:** Skip stdin reading and exchange synthesis — only apply pending mutations. Requires at least one `--pending-*` flag; incompatible with `--template`/`--stream`/`--ipc`.

- **`--status` flag on `write`:** Replace the `agent:status` component content inline during a write operation, same pattern as pending ops.

- **`status` submodule (`status_cmd.rs`):** New module for status component manipulation.

- **Notify with pending:** `agent-doc notify` gains `--pending-add`, `--pending-add-gated`, and `--no-create-pending` flags. Message is now optional when `--pending-add` is used.

- **`session clear` subcommand:** Clear the configured tmux session, returning to auto-detect mode.

- **Supervisor PTY module (`supervisor/pty.rs`):** New 526-line module for PTY-based process spawning and management within the supervisor architecture.

- **Start.rs expansion:** Major rework (+627 lines) for improved tmux detection, session routing, and supervisor integration.

- **Debounce simplification:** Removed redundant debounce logic in favor of the consolidated approach.

- **Tests:** 20 new typed-gate tests (parse, render, roundtrip, resolve, set-gate-type, scan, case insensitivity, edge cases). All 1111 tests pass, clippy clean.

## 0.32.5

- **Route idle gate tightened for busy Codex panes; bulk stash prune now reaps orphaned unregistered agent panes:** `route.rs` no longer treats every visible Codex prompt glyph as an idle routed-dispatch target. `wait_for_agent_ready()` now requires two consecutive idle-prompt samples and rejects captures that still show an active permission prompt or the Codex `tab to queue message` footer, which is a queue-only busy state rather than a true idle prompt. This closes the failure mode where route logged `codex ready after 0.0s`, injected `agent-doc <file>` into a live pane, then timed out with `no new document cycle started` because Codex had only queued the message. Tests: new `harness::has_busy_cue_*` coverage plus `route::wait_for_agent_ready_rejects_codex_queue_message_footer`. In the same pass, `resync.rs` bulk stash cleanup now matches the stricter single-pane cleanup behavior: unregistered stash panes running `agent-doc`/`claude`/`codex` are killed automatically unless live-owner proof still ties them to a registered document. This prevents repeated reroute attempts from piling up "unregistered — skipping kill (may be rescuable)" orphan panes in stash. Tests: new `resync::purge_unregistered_stash_panes_bulk_kills_unregistered_agent_without_live_owner`.
- **Fix submodule auto-start `file not found` (route.rs `rewrite_start_path`):** When the spawned tmux pane's `cwd` is narrowed to a submodule root (by `git::resolve_pane_cwd`), the `agent-doc start <path>` send-keys invocation now rewrites the caller-supplied super-root-relative `file_path` to be relative to that narrowed `cwd` before composition. Previously a path like `src/session-share/tasks/foo.md` was passed verbatim to a pane already `cd`'d into `src/session-share`, producing `Error: file not found: src/session-share/tasks/foo.md` and blocking auto-claim + auto-start on every submodule-hosted document. Fix lives at a single funnel (`auto_start_in_session`) and also feeds `send_command`'s `/agent-doc <path>` slash command for the same reason. Pure helper `rewrite_start_path(file, cwd, original) -> String` canonicalizes both sides, strips the cwd prefix, and falls back to `original` on any failure (preserves behavior for non-submodule docs, ghost paths, and files outside cwd). Tests: 4 new unit tests (`rewrite_start_path_narrows_to_submodule_relative`, `rewrite_start_path_no_op_when_file_under_cwd_with_same_prefix`, `rewrite_start_path_falls_back_when_canonicalize_fails`, `rewrite_start_path_falls_back_when_file_not_under_cwd`) plus full `route::` suite (43 passing). Forward-compatible with the supervisor track (#jg0d/#b486/#40ct/#vnp0/#6ae3/#zp02/#f7d5) — when `PtySpawnConfig.args` lands, the same helper feeds path rewriting at the new spawn funnel.
- **Binary strips trailing bare `❯` lines from exchange writes (`template::strip_trailing_caret_lines` in `apply_patches_with_overrides`):** The post-patch boundary marker `<!-- agent:boundary:... -->` lands directly after agent content, so a trailing `❯` on its own line becomes a phantom prompt-glyph row above the boundary on every cycle. Agent discipline is the wrong layer — this is now a code-enforced invariant. New pure helper `strip_trailing_caret_lines(content)` collapses all trailing lines whose trim is exactly `❯`; called on `patch.content` when `patch.name == "exchange"` and on unmatched content when it routes to `exchange`/`output` (including the auto-created-exchange path). Non-exchange components are untouched — `❯` in `notes`, `pending`, or user-authored content like `❯ follow-up` is preserved. Tests: 8 new (`strip_trailing_caret_removes_bare_prompt_line`, `_removes_multiple_trailing_lines`, `_preserves_mid_content_caret`, `_preserves_caret_with_text`, `_handles_no_trailing_newline`, `_noop_when_no_caret`, `apply_patches_strips_trailing_caret_from_exchange`, `apply_patches_preserves_caret_in_non_exchange`). Full `template::` suite: 64 passing. See [runbooks/code-enforced-directives.md](runbooks/code-enforced-directives.md).
- **SKILL.md audit + prune (293 → 112 lines, ~62% cut):** Delegated rarely-consulted workflow detail to runbooks to keep the hot-path instructions tight. New runbooks bundled via `include_str!` in `src/skill.rs::BUNDLED_RUNBOOKS` and installed to `.claude/skills/agent-doc/runbooks/` on `agent-doc skill install`: `model-tier-gate.md` (precedence chain, `required_tier` gate, `model_switch` ack — was SKILL §0c), `streaming-checkpoints.md` (when/how to flush, baseline re-save pattern — was a §1 sub-section), `document-format.md` (frontmatter fields, inline vs template mode, `<!-- agent:name -->` component conventions + inline attributes + snapshot storage — was §Document Format + §Snapshot Storage), and `code-enforced-directives.md` (promoted from project-local into the bundled set). Removed from SKILL.md: the `❯`-rule paragraph (now binary-enforced, see above), the verbose preflight JSON schema code block (the agent parses the real output), the duplicated baseline/write-back instructions between §2a and §2b, the per-mode split between append and template (unified into a single write-back block), and `## Snapshot Storage`. Preserved verbatim (hot-path on every cycle): invocation + subcommand detection, preflight call + `no_changes`/`claims`/`baseline_file` handling, slash-command dispatch via `Skill` tool, `### Re:` header rule + model attribution, pending granular-ops 3-line summary, `--stream` write-back + immediate `agent-doc commit`, and the `IMPORTANT: Do NOT use Edit tool` guard. Memory cleanup: `feedback_no_trailing_prompt_glyph.md` deleted from `~/.claude/projects/-home-brian-work-btakita-agent-loop/memory/` and its `MEMORY.md` index line removed — the rule is now a binary invariant, not a per-agent memory.

## 0.32.4

- **Pending gated-state `[/]` (#pf01, #mgdw, #h1j2, #q90h, #sx35):** New `PendingState::Gated` variant for pending items that are code-complete but awaiting an external gate (release, telemetry, field validation). Rendered as `- [/] [#id] text` in the pending component. Never auto-reaped — only `- [x]` items are reaped by preflight. Spec: `src/agent-doc/specs/pending-system.md` — includes the full state-transition matrix (§4), lifecycle diagram, and reaper rules. State machine: `Open → Gated` via `gate`, `Gated → Open` via `ungate`, `Open|Gated → Done` via `mark-done`. Illegal transitions (`ungate` from `Open`/`Done`, `gate` from `Done`) return errors; idempotent transitions (`Gate` on `Gated`, `MarkDone` on `Done`) are no-ops. Parser: `pending::parse_item_line` accepts `[ ]` / `[/]` / `[x]` / `[X]`; `PendingItem::render` round-trips. CLI: `agent-doc write --pending-gate <id>` and `--pending-ungate <id>` flags on the `write` subcommand, combinable with `--pending-add` / `--pending-done` / `--pending-edit` / `--pending-reorder` in a single call (gate/ungate run before done so `--pending-gate X --pending-done X` promotes through `Open → Gated → Done` atomically). Preflight: emits `pending_gated_count: N` in the JSON output when at least one item is gated (omitted when zero to keep happy-path output compact), alongside the existing `pending_reordered` signal. Reaper: preflight's reap pass skips `Gated` items unchanged. Tests: `tests/pending_integration.rs` covers parser round-trip for `[/]`, all valid/invalid state transitions, reaper respects `Gated`, CLI flag integration (`write_pending_gate_open_to_gated`, `write_pending_gate_idempotent_on_gated`, `write_pending_gate_done_errors`, `write_pending_gate_then_done_in_one_call`, `preflight_emits_pending_gated_count`, `preflight_omits_pending_gated_count_when_zero`). Rationale: previously, long-lived release-gated tasks had no lexical distinction from active work — they either sat in `[ ]` and competed for attention, or got prematurely `[x]`-marked and reaped before the gate actually cleared. The `[/]` character was chosen for visual distinctness from `[ ]`/`[x]` and because it's already in GFM-task-list parser tolerance ranges across common editors.

- **Rename `patch:pending` → `replace:pending` (#25ag):** The full-replacement block syntax for the `pending` component is renamed from `<!-- patch:pending -->...<!-- /patch:pending -->` to `<!-- replace:pending -->...<!-- /replace:pending -->`. The `replace:` prefix signals full-replacement semantics explicitly (all other `patch:<name>` blocks are component-scoped patches; pending uniquely replaces the whole list). Corresponding renames: `--allow-patch-pending` → `--allow-replace-pending` (CLI flag), `AGENT_DOC_ALLOW_PATCH_PENDING` → `AGENT_DOC_ALLOW_REPLACE_PENDING` (env var). Dual-accept migration: the deprecated `patch:pending` form, `--allow-patch-pending` flag (via clap alias), and legacy env var all continue to work for one release. The parser emits a stderr deprecation warning on every `patch:pending` block so callers can find and update their usage. The default-reject gate applies to both forms — enforcement recognizes `name == "pending"` regardless of which prefix opened the block. Rationale: the `replace:` prefix is a higher-signal warning to human readers that this block clobbers a list the user is actively editing, reducing the silent-data-loss failure mode that `patch:` understates. Tests: `write_rejects_replace_pending_block`, `write_rejects_legacy_patch_pending_block` (covers deprecation warning), `write_allows_replace_pending_with_escape_hatch`, `write_allows_legacy_patch_pending_with_legacy_flag`, `write_allows_replace_pending_with_legacy_env_var`, `write_rejects_replace_pending_via_library_default`. **Next release removes dual-accept:** `patch:pending` will become a hard error; update any remaining call sites now.

## 0.32.3

- **Fix: Submodule-aware git commit routing** — Files inside git submodules (`src/boost-client/tasks/*.md`, `src/session-share/tasks/*.md`) previously caused `fatal: Pathspec '...' is in submodule '...'` errors during `agent-doc commit` (preflight sweep and session-final commits). Root cause: parent-level git operations tried to stage submodule-relative paths directly in the parent index. Fix: Added `narrow_to_submodule(super_root, file) -> (PathBuf, bool)` which detects submodule boundaries. When a file is in a submodule, all git staging/commit ops (`hash-object`, `update-index`, `commit`) run inside the submodule's repo with submodule-relative paths. After commit succeeds, `update_parent_submodule_pointer()` updates the parent's submodule pointer in a separate partial commit. Tests: `narrow_to_submodule_returns_super_root_for_non_submodule_file`, `commit_in_submodule_routes_through_submodule_repo` (integration test with actual `git submodule add` sandboxing). Live verification: Two separate submodule documents (`src/session-share/tasks/claudescore.md`, `src/boost-client/tasks/monsterrodholders.md`) now commit cleanly with zero `fatal:` lines.

- **Feature: `out_of_band_write` always-on forensic logging** — Added unconditional log emission when a file's on-disk size diverges from the last snapshot, regardless of threshold. Previously, only divergences >100 bytes emitted human warnings; now all out-of-band writes emit a structured ops.log entry: `out_of_band_write file=<path> drift=<bytes> snap_len=<N> file_len=<N>`. This enables downstream analysis (aggregation, correlation with concurrent operations, drift pattern classification) without requiring the safety rail to trip (which only fires at catastrophic thresholds). Helps root-cause the recurring 135-byte snapshot-vs-file gaps observed in monsterrodholders and other in-flight sessions.

- **Feature: Safety rail with forensic logging in `normalize_user_prompts_in_exchange`** — When a user's added content (between snapshots) contains escaped newlines or other encodings that decompose during normalization, the normalization logic could diverge from the user's source. Added: (1) `normalize_threshold_exceeded` detection when decomposition deltas exceed a configurable threshold (default 500 bytes), (2) forensic logging of applied normalization counts and byte deltas, (3) automatic git commit with diagnostic context if threshold trips. Log schema: `normalize_user_prompts snap_len=<N> base_len=<N> applied=<count>` (fires on every write, no threshold), plus `normalize_threshold_exceeded file=... delta=... snap_len=... base_len=...` (fires if `delta > threshold`). Enables early detection of corruption patterns in heterogeneous editor environments (mixed CRLF, smart quotes, etc.). See ops.log for real-world drift data.

## 0.32.2

- **Feature: `env` frontmatter for per-document environment configuration:** Documents can now declare environment variables in YAML frontmatter that apply to all Bash tool calls and Claude spawns within that session. Syntax:
  ```yaml
  env:
    OPENROUTER_API_KEY: "$(passage btak/OPENROUTER_API_KEY)"
    ANTHROPIC_BASE_URL: "https://openrouter.ai/api"
    ANTHROPIC_AUTH_TOKEN: "$OPENROUTER_API_KEY"
    ANTHROPIC_MODEL: "qwen/qwen3.6-plus"
  ```
- **Shell expansion support:** Environment variable values support shell expansion (`$(command)`, `$VAR`, `${VAR}`). Cross-references work (later vars can reference earlier ones). Values are expanded at runtime; expanded secrets never appear in JSON output or logs.
- **Coverage across all paths:** Env vars apply to:
  - Interactive Claude sessions started via `agent-doc start <FILE>` (via `cmd.env()` on spawned process)
  - Non-streaming submits via `agent-doc run` (via `Claude::with_env()`)
  - Streaming submits via `agent-doc stream` (via `StreamingAgent::send_streaming()`)
  - Parallel fan-out (via unexpanded shell exports in tmux send-keys, so target shell handles expansion safely)
  - `/agent-doc` skill in existing sessions (preflight JSON returns unexpanded values; skill runs `export` in Bash)
- **Preflight JSON field:** `"env": {"KEY": "unexpanded_shell_expr"}` — skill exports these unexpanded so secret expansion happens inside the Bash call, never in JSON output.
- **New module `src/env.rs`:** 
  - `expand_values(env)` — expands all vars through the shell (used by start/run/stream paths)
  - `shell_export_prefix(env)` — builds `export K="V" && ...` string with unexpanded values (used by parallel path)
- **Tests added:** 42 existing tests + 8 new env tests covering plain values, shell expansion, cross-references, empty env, and safe quoting in send-keys commands. All 72 tests passing.
- **SKILL.md step 0c2:** Skill now exports env vars from preflight JSON into the shell before tool calls.

## 0.32.1

- **Fix: CRDT state not refreshed after `agent-doc compact`:** When a template-mode document with CRDT write strategy ran `compact`, the binary correctly rewrote the file and snapshot on disk, but the CRDT state in `.agent-doc/crdt/<hash>.yrs` was stale. On the next `agent-doc write` or `stream`, the 3-way merge loaded the stale CRDT (containing pre-compact exchange AND pre-compact pending), causing non-target components (like `agent:pending`) to be clobbered by old CRDT view of pending items. Fix: After `run_component_compact` or `run_component_compact_partial`, when `is_crdt`, refresh CRDT state by creating a new `CrdtDoc` from the post-compact content and saving it to `.agent-doc/crdt/<hash>.yrs`. This resets the CRDT to a fresh state, discarding pre-compact history (appropriate since compact is a "new epoch" operation).
- **Runbook hardened:** `.claude/skills/agent-doc/runbooks/compact-exchange.md` now explicitly forbids mutations to non-target components. Added Safety Invariants section and pre/post verification steps using git snapshots.
- **Tests added:** `crdt_compact_preserves_pending_with_state_refresh` (verifies fix), `compact_preserves_boundary_marker` (tests ❯ preservation in non-target component), `compact_working_tree_consistency` (disk/snapshot consistency).

## 0.32.0

- **Fix: Submodule-aware patch routing:** `try_ipc()` and `try_ipc_full_content()` in `write.rs` now use `git::resolve_to_git_root()` to detect submodule context. When a session document lives inside a git submodule, IPC patches are routed to the **superproject's** `.agent-doc/patches/` directory instead of the submodule's local `.agent-doc/patches/`. Previously, patches written to submodule documents (e.g. `src/session-share/tasks/claudescore.md`) would land in `<submodule>/.agent-doc/patches/` where the JetBrains plugin (which only watches the parent repo) never saw them. The fix falls back to `find_project_root()` if git resolution fails, preserving backward compatibility for non-git and non-submodule cases.
- **Tests added:** `try_ipc_routes_to_superproject_when_available` (creates a real git submodule structure and verifies patches route to parent), `try_ipc_falls_back_to_find_project_root_when_not_in_git` (fallback behavior), and `test_submodule_write_patches_dir_structure` (integration-level directory layout validation).

- **Feature: Harness-agnostic model tier selection:** New `model_tier` module defines a `Tier` enum (`auto | low | med | high`) and composes an `effective_tier` from four sources, highest precedence first:
  1. Inline `/model <x>` command in the diff (stripped from downstream diff/classifier)
  2. `<!-- agent:model -->` component content
  3. `agent_doc_model_tier` frontmatter field
  4. Diff heuristic (`suggested_tier`) based on `diff_type` + document path
- **Config: `[model.tiers.<harness>]` maps** let users customize tier→model mappings per harness (`claude-code`, `codex`, `default`). Built-in defaults: claude-code → haiku/sonnet/opus, codex → gpt-4o-mini/gpt-4o/o3.
- **Harness detection:** `detect_harness()` checks `CLAUDE_CODE_SESSION` / `CLAUDECODE` / `CODEX_SESSION` env vars and returns `claude-code | codex | default`.
- **Preflight JSON additions:** `effective_tier`, `required_tier`, `suggested_tier`, `model_switch`, `model_switch_tier` fields.
- **Diff scanner strips `/model` lines:** `scan_model_switch` runs before classification, so downstream classifier/slash-command parser never see `/model`.
- **SKILL.md step 0c (Model tier gate):** Documents how skills should read `effective_tier` / `required_tier` and either proceed, acknowledge a `/model` switch, or ask the user to `/model` before re-invoking.
- **Frontmatter field:** `agent_doc_model_tier: low | med | high | auto` on session documents.
- **Tests added:** 48 tests in `model_tier.rs` covering tier parse/resolve, harness detection, component read, scanner guards (code fence, blockquote), heuristic path boosts, composition precedence, and JSON serialization.

## 0.31.31

- **Fix: Commit-reliability — snapshot committed even on IPC timeout exit(75):** `write.rs` now saves snapshot + calls `git::commit` before `process::exit(75)`, so agent responses are preserved even when the IDE plugin doesn't ACK the patch in time.
- **Fix: Commit-reliability — commit before `result?` propagation:** `main.rs` reordered to run commit before `result?`, ensuring partial writes that saved a snapshot are always tracked in git.
- **Fix: Commit-reliability — retry on git index.lock contention:** `git.rs` retries `git commit` up to 3× with exponential backoff (100/200/400ms) when concurrent sessions cause lock contention.
- **Fix: Commit-reliability — `agent_doc_commit` FFI export:** `ffi.rs` exports `agent_doc_commit(file_path)` for IDE plugins to call after applying a patch. `NativeLib.kt` + `PatchWatcher.kt` updated to call it on the Document API path.
- **Fix: Commit-reliability — preflight cross-document sweep:** `preflight.rs` scans all tracked docs in the same project at the start of each cycle and commits any doc where the snapshot is newer than the file (missed commit backstop).
- **Fix: `project_config_path()` CWD-sensitivity:** Walks up from CWD to find `.agent-doc/` instead of always using a bare relative path. Prevents wrong-config reads when subcommands run from a subdirectory (e.g., submodule CWD drift). Falls back to CWD for uninitialized projects.
- **Tests added:** `commit_retry_logic_handles_index_lock_error`, `commit_succeeds_when_no_lock_contention` (Fix 3); `agent_doc_commit_returns_false_for_null`, `ffi_git_commit_commits_staged_file` (Fix 4); `preflight_sweep_commits_other_tracked_docs` (Fix 5).
- **Fix: `(HEAD)` marker incorrectly applied to bash comments inside fenced code blocks:** The old ad-hoc fence tracker (`is_fence_marker`) toggled `in_fence` on every line starting with 3+ backticks — including `` ```bash `` which per CommonMark can only OPEN a fence, not close one. When a `` ``` `` plain fence contained inner `` ```bash `` lines (e.g., terminal output referencing a bash command), the state inverted, causing `# On the server — run once` inside a subsequent `` ```bash `` block to appear "outside" the fence and receive a `(HEAD)` marker it must not have. Fix: replace the ad-hoc `is_fence_marker` / `in_fence` toggling in `strip_head_markers` and all four code paths in `add_head_marker` (step 1 cleanup, step 2 heading collection, step 3 HEAD heading counting, re-application loop) with CommonMark-compliant code block detection via `pulldown-cmark`. A closing fence cannot have an info string — `pulldown-cmark` correctly handles this. The re-application path also now filters out any `# comment (HEAD)` lines in git HEAD that are themselves inside a code block, preventing propagation of the baked-in bad marker across commits.
- **Test added:** `add_head_marker_bash_comment_inside_plain_fence` — exercises the specific failure path: a plain `` ``` `` fence containing a `` ```bash `` line, followed by a real heading, followed by a `` ```bash `` fence with a `# comment` line.

## 0.31.30

- **Fix: `❯ ` prefix applied to `agent:pending` patches (regression in v0.31.29):** `normalize_patch_content` was called on all IPC patches, not just exchange patches. When `normalize_prefix_lines` contained a line that also appeared verbatim in the `agent:pending` patch content, that line incorrectly received the `❯ ` prefix. Fix: gate `normalize_patch_content` on `is_append_mode_component(&p.name)` at both the primary IPC write path and the IPC timeout fallback in `write.rs`. Replace-mode components (`pending`, `status`, etc.) now always pass patch content through unchanged.
- **Test added:** `normalize_prefix_lines_skipped_for_replace_mode_components` — verifies that `agent:pending` content is not normalized.

## 0.31.29

- **`agent-doc write --commit` flag:** Runs `git::commit` immediately after a successful write. Eliminates the separate `agent-doc commit` step — the final write in the SKILL.md skill now uses `--commit`. Silently skips commit if the document is not inside a git repo (`git rev-parse --is-inside-work-tree` guard). Streaming checkpoint writes do not use `--commit`; only the final write does.
- **`git::is_in_git_repo` helper:** New `pub(crate)` function that checks whether a file path is inside a git repository.
- **SKILL.md updated:** Step 2a/2b final writes now use `--commit`; step 3 updated to reflect merged write+commit.

## 0.31.28

- **`start.rs` auto-relocate:** When claiming a pane from a terminal in a different tmux session than the project expects, automatically relocates the pane to the correct session before registration (was warn-only). Falls back to warn-only if no anchor pane exists in the expected session.
- **`relocate_if_wrong_session` helper + 3 tests:** Extracted guard into a testable `pub(crate)` function; 3 `IsolatedTmux`-based tests cover noop, cross-session success, and no-anchor fallback.

## 0.31.27

- **`pane_policy` module (tmux-router 0.3.10):** New `PaneMoveOp` + `CrossSession` enum as a mandatory gateway for all pane movement. `CrossSession::Deny` by default; `CrossSession::Allow { reason }` for intentional cross-session relocations. All 7 `join_pane` call sites in agent-doc migrated to use `PaneMoveOp`.
- **Guard `start.rs` registration:** When claiming a pane, warns if `$TMUX_PANE`'s session ≠ `project_tmux_session()` — prevents silent session drift on claim.
- **Guard `resolve_target_session` auto-update (route.rs):** No longer overwrites `tmux_session` config when a previously-configured session is dead. Only writes config when no session was previously set. Prevents session 1 from silently overwriting session 0.
- **Fix `resync.rs` WrongSession detection:** `detect_issues` now falls back to `config::project_tmux_session()` when `frontmatter.tmux_session` is absent. Panes in a wrong session are flagged even without per-document session frontmatter. `apply_fixes_to_registry` uses `PaneMoveOp::allow_cross_session("relocate WrongSession pane to project session")` to move them.

## 0.31.26

- **Fix: orphan repair dedup guard (repair.rs):** `repair::run` now reads the document before applying a pending response and checks if the content is already present using a 3-line fingerprint. If already applied (e.g., IPC path wrote the content but `clear_pending` was never called due to exit 75), the pending file is removed without re-applying. Prevents ghost-reappearance of previous responses. New test: `recover_skips_duplicate_apply`.

## 0.31.25

- **`preflight` diff-only always (preflight.rs):** `document` field is always `null` — the full document is never sent automatically. Use `agent-doc read <FILE>` to fetch on demand.
- **BREAKING CHANGE: `--diff-only` and `--with-document` flags removed from `preflight`:** Both flags removed. Diff-only is now unconditional. Any callers using either flag must remove it.
- **`agent-doc read <FILE> [--component <name>]` (read.rs):** New subcommand to fetch the full document or a single named component's body on demand. Use on the first cycle when the document is not yet in context.
- **Stash window pane check removed (preflight.rs):** `check_layout` no longer flags panes in `stash*` windows as layout issues. Stash windows hold intentional backgrounded sessions.
- **Fix: `collapsible_if` in `git.rs` (CI):** Nested `if` at line 410 collapsed to satisfy Rust 1.94.1 clippy.

## 0.31.24

- **Fix: `~~~` tilde fences protected from `❯ ` prefix normalization (write.rs):** `normalize_user_prompts_in_exchange` previously only tracked `` ``` `` (backtick) fences. Lines inside `~~~` fenced regions could incorrectly enter `user_added` and receive a `❯ ` prefix. Fixed by extracting `fence_open`/`fence_close` helpers that handle both `` ` `` and `~` fence chars with proper length tracking (matching `diff.rs`'s `fence_char`/`fence_len` approach). New test: `normalize_user_prompts_tilde_fence_interior_skipped`.

## 0.31.23

- **Fix: `❯ ` prefix normalization via IPC `fullContent` (write.rs):** When `normalize_prefix_lines` is non-empty, `try_ipc` now also sends `fullContent = content_ours` in the IPC payload (both socket and file paths). The plugin's `fullContent` path replaces the entire document, guaranteeing `❯ ` prefixes reach the editor file even when targeted string replacement fails.
- **Fix: boundary regex in `findBoundaryInComponent` + `repositionBoundaryToEnd` (PatchWatcher.kt v0.2.51):** Pattern updated from `[a-f0-9-]+` to `[a-z0-9][a-z0-9:-]*` so summary-style boundary IDs (e.g. `a0cfeb34:agent-doc-bugs`) are correctly matched.
- **Fix: boundary stripping regex in VSCode extension (extension.ts v0.2.4):** `[a-f0-9]+` → `[a-z0-9][a-z0-9:-]*` in boundary marker strip-before-replace path.
- **Regression test:** `normalize_user_prompts_restores_prefix_lost_in_file` — verifies snapshot `❯ do` is restored when editor file has bare `do`.
- **`agent-doc compact --tag <name>` (compact.rs):** Creates a lightweight git tag at HEAD before compaction as a pre-compact checkpoint. Without `--tag`, auto-generates `agent-doc/<doc-name>/pre-compact-N`. Use `--tag skip` to disable. Tagging failure is a warning, not an error.
- **`agent-doc log <FILE>` (history.rs):** Annotated git log for a session document. Walks `git log`, loads all `agent-doc/<name>/pre-compact-*` tags, and annotates matching commits in the output table (COMMIT, DATE, SUBJECT, TAG columns).
- **`agent-doc show <FILE> [--back N | --at N | --tag <name>]` (history.rs):** Shows document content at a specific point in git history. `--back N` maps to `HEAD~N`; `--at N` selects the Nth commit in log order (0 = newest); `--tag <name>` resolves the tag to its commit.
- **`agent-doc diff <FILE> --from <ref> [--to <ref>]` (history.rs):** Shows a unified diff of the document between two git refs. `--to` defaults to `HEAD`. Without `--from`, falls back to the existing live diff behavior.

## 0.31.22

- **Fix: quoted strings skip `❯ ` prefix normalization (write.rs):** `normalize_user_prompts_in_exchange` now excludes lines starting with `"` from `❯ ` prefix tagging. Previously, user-written quoted strings (e.g., `"Merge conflict with external write"`) were incorrectly tagged as terminal prompts. New test: `normalize_user_prompts_quoted_string_skipped`.

## 0.31.21

- **Fix overeager `❯ ` prefix on agent response lines (write.rs):** `normalize_user_prompts_in_exchange` now takes a `baseline` parameter. User-added lines are identified by diffing `snapshot → baseline` (not `snapshot → content_ours user_region`). After `apply_patches_with_overrides`, the boundary moves to the end of exchange — so content_ours' "user region" incorrectly included agent response lines. The fix diffs against baseline (pre-agent state), ensuring only genuine user additions get `❯ `. New regression test: `normalize_user_prompts_agent_response_not_prefixed`.

## 0.31.20

- **`❯ ` prefix normalization for exchange user prompts (write.rs):** After each agent cycle, new user-typed lines in `patch=append` exchange components are prefixed with `❯ ` to visually distinguish user input from agent responses. Implemented via `similar` diff of snapshot vs `content_ours`; only Insert lines before the boundary marker are prefixed. `normalize_user_prompts_in_exchange()` and `extract_normalization_targets()` added. 6 tests.
- **IPC-side prefix normalization (write.rs + PatchWatcher.kt v0.2.49):** `try_ipc` passes `normalize_prefix_lines: Option<&[String]>` in the IPC payload. JetBrains plugin applies `normalizeExchangePrefixes()` targeting only the user region (before `<!-- agent:boundary:UUID -->`) via targeted text replacement. Both Document API and VFS paths updated.
- **SKILL.md rule: never echo user input in patch:exchange (SKILL.md):** For `patch=append` exchange components, the patch must contain only new agent response content — echoing user input creates duplicates.

## 0.31.19

- **AGENT_PROCESSES guard on wrong-session recovery (route.rs):** `is_agent_process()` helper added. Wrong-session recovery path now skips `stash_pane`+`rescue_from_stash` for panes running non-agent processes (corky, shells, etc.) — falls through to auto-start instead. Prevents corky/foreign panes from being dragged across tmux sessions.
- **AGENT_PROCESSES guard on lazy claim Strategy 2 (route.rs):** `find_target_pane()` result is now gated by `is_agent_process()` — panes running non-agent processes are not claimed. Prevents corky from being registered as the owner of a document pane.
- **`resync --fix --session <target>` (resync.rs + main.rs):** `WrongSession` fix now supports `--session <name>` to relocate panes via `join-pane` instead of killing them. `apply_fixes_to_registry` takes `relocate_session: Option<&str>`. Falls back to deregister if no active pane found in target session.

## 0.31.18

- **Partial compact `--keep N` (compact.rs):** `agent-doc compact <FILE> --keep N` archives only exchanges older than the last N `### Re:` sections, preserving recent context. `parse_topic_sections()` helper added; 4 new tests.
- **Slash command dispatch from diff (diff.rs + preflight.rs):** `parse_slash_commands(diff)` extracts slash commands from user-added lines; preflight returns them in `slash_commands[]`; the SKILL executes each before responding. Guards: code fences, blockquotes, non-added/removed lines excluded.
- **Dedupe stale patch cleanup (dedupe.rs):** After removing duplicate blocks, deletes `.agent-doc/patches/<hash>.json` to prevent `processPendingPatches()` from re-applying removed content on next plugin startup.
- **JB plugin startup dedup guard (PatchWatcher.kt v0.2.48):** Before applying a pending patch file, compares snapshot mtime against patch file mtime. If snapshot is newer, the patch was already applied — deletes stale file and skips. Replaces the incorrect boundary-ID check from v0.2.47.
- **Cross-session pane swap fix (route.rs + sync.rs):** `rescue_from_stash()` now checks pane session before swap; uses `join-pane` for cross-session panes. Session-drift detection added to `check_layout()` in preflight.
- **PromptPoller FFI CRDT merge (editors/jetbrains):** FFI-based CRDT merge, fix unnecessary reload, preserve edits on conflict.
- **SPEC.md §7.26 + §7.28 updated:** preflight JSON now documents `slash_commands[]`; dedupe documents stale patch file cleanup.

## 0.31.17

- **CRDT duplicate bug fix (write.rs):** When boundary-synthesis consumed unmatched content into a patch, the IPC payload also sent the same content as `"unmatched"` — the plugin applied both, producing duplicates. Fixed by clearing `effective_unmatched` to `""` when synthesis occurred, on both socket and file IPC paths.
- **Write-time dedup (write.rs):** `build_ipc_patches_json` now checks if the unmatched content already exists in the target component before synthesizing a patch. Skips synthesis if a match is found, making writes idempotent.
- **SKILL.md demoted (SKILL.md):** `<!-- patch:exchange -->` wrapper is now "preferred, not required" — the binary correctly handles both wrapped and raw content paths.
- **3 new tests (write.rs):** `synthesis_dedup_skips_when_content_already_present`, `synthesis_proceeds_when_content_is_new`, `effective_unmatched_cleared_when_synthesis_consumes_content`.

## 0.31.16

- **Extreme drift snapshot re-sync (git.rs):** When `commit()` detects file is >5x larger than snapshot (typical of file move/rename), automatically re-syncs snapshot from file content. Prevents the drift loop that caused "externally saved" dialogs and lost keystrokes after renaming files.
- **Claim auto-scaffold (claim.rs):** Empty `.md` files get the full template (UUID + format + crdt + components) when claimed. Previously only wrote `agent_doc_session`, causing scaffolding to skip (no format detected).

## 0.31.15

- **Transfer auto-init (extract.rs):** `agent-doc transfer` auto-creates the target file in template mode if it doesn't exist. Creates parent dirs, generates UUID session, copies agent name from source. Always defaults to template format.
- **Write silent-drop warnings (write.rs):** `run_stream` warns when file has no template components but receives unmatched content. `try_ipc` logs `ipc_unmatched_content_dropped` to ops.log. Improved ops.log to include `ipc_patches` count alongside original `patches` count.
- **Investigation runbook:** New `runbooks/investigate-behavior.md` for debugging agent-doc behavior (ops.log, git history, affected files, common failure patterns).

## 0.31.14

- **Binding invariant enforcement (claim.rs):** When target pane is already claimed by another document, `claim` now provisions a new pane instead of erroring. Enforces SPEC §8.5: "never commandeer another document's pane."
- **Sync auto-scaffold (sync.rs):** Empty `.md` files in editor layout are automatically scaffolded with template frontmatter + status/exchange/pending components. Scaffold is saved as snapshot and committed to git immediately.
- **Transfer pending merge (extract.rs):** `agent-doc transfer` now automatically transfers the `pending` component alongside the named component. Source pending is cleared after merge.
- **SPEC.md updates:** §7.10 (claim provisions on occupied pane), §8.5 (empty file auto-scaffold in initialization step).
- **Tests:** 6 sync scaffold tests (positive + negative), 2 pending merge tests. 458 total.
- **Runbook:** `code-enforced-directives.md` — behavioral invariants enforced by binary, not agent instructions.

## 0.31.13

- **Diff-type classification (P1)**: `classify_diff()` classifies user diffs into 7 types (Approval, SimpleQuestion, BoundaryArtifact, Annotation, StructuralChange, MultiTopic, ContentAddition). Wired into preflight JSON as `diff_type` + `diff_type_reason`. 13 tests.
- **Annotated diff format (P3)**: `annotate_diff()` transforms unified diffs into `[agent]`/`[user+]`/`[user-]`/`[user~]` format. Wired into preflight JSON as `annotated_diff`. 5 tests.
- **Content-source annotation sidecar (P4)**: New `agent-doc annotate` command generates `.agent-doc/annotations/<hash>.json` mapping each line to agent/user source. SHA256 cache invalidation. GC integration. 6 tests.
- **Reproducible operation logs (P5)**: New `.agent-doc/logs/cycles.jsonl` with structured JSONL entries (op, file, timestamp, commit_hash, snapshot_hash, file_hash). Wired into all write paths + git commit. 2 tests.
- **Post-preflight eval diffs (P2)**: Moved `strip_comments` to `component.rs` (shared between binary and eval-runner). eval-runner preprocesses diffs with comment stripping.
- **Transfer-source metadata**: `PatchBlock` now supports `attrs` field. `<!-- patch:name key=value -->` attributes parsed and preserved. 3 tests.
- **JB plugin Gson migration**: Replaced hand-rolled JSON parser with `com.google.gson.JsonParser`. Fixes `\\n` unescape ordering bug. Plugin v0.2.44.
- **SKILL.md enhancements**: Diff-type routing (0b), multi-topic `---` separators (0c), process discipline clarification.
- **Domain ontology**: Interaction Model section in README.md (Directive, Cycle, Diff, Annotation). `directive.md` kernel node.
- **Module-harness**: New `ontology-references` runbook for cross-referencing domain ontology in module specs.

## 0.31.12

- **Refactor `ensure_initialized()`**: Split into 3 focused functions: `ensure_session_uuid()`, `ensure_snapshot()`, `ensure_git_tracked()`. Composite `ensure_initialized()` calls all three.
- **Rename `auto_start_no_wait()` → `provision_pane()`**: Aligns with domain ontology (Provisioning = creating a new pane + starting Claude).
- **Tests**: 8 new tests for ensure_session_uuid (3), ensure_snapshot (2), ensure_initialized (1), plus 2 helpers.

## 0.31.11

- **Sync auto-initialization**: `ensure_initialized()` now called in sync's `resolve_file`. Files with `agent_doc_format` but no session UUID get one assigned automatically on editor navigation. Fixes: files created by skills (granola import) are no longer invisible to sync.
- **Binding invariant spec**: SPEC.md section 8.5 documents the pane lifecycle invariant — document drives pane resolution, never commandeers another document's pane.
- **Domain ontology**: README.md now has Document Lifecycle, Pane Lifecycle, and Integration Layer ontology tables (Binding, Reconciliation, Provisioning, Initialization).
- **Module docs**: sync.rs, claim.rs, snapshot.rs, route.rs updated with ontology terminology.

## 0.31.10

- **Auto-init for new documents**: `ensure_initialized()` in `snapshot.rs` — claim and preflight now auto-create snapshot + git baseline for files entering agent-doc. No more untracked files after import.
- **Cross-process typing detection**: FFI exports `agent_doc_is_typing_via_file` and `agent_doc_await_idle_via_file` for CLI tools running in separate processes. `is_idle` and `await_idle` now bridge to file-based indicator when untracked in-process.
- **Diff stability fix**: `wait_for_stable_content` counter now tracks consecutive stable reads across outer iterations (was resetting within each pass).
- **IPC error propagation**: `ipc_socket::send_message` now returns proper errors instead of swallowing connection/timeout failures as `Ok(None)`.
- **Template patch boundary fix**: Improved boundary marker handling in `apply_patches_with_overrides`.
- **CI/build**: `make release` target, idempotent release workflows, version-sync check in `make check`.

## 0.31.9

- **Transfer-extract runbook**: New bundled runbook for cross-file content moves (`agent-doc transfer`/`extract`). Installed via `skill install`.
- **Compact-exchange runbook update**: Added note about preserving unanswered user input during compaction.
- **SKILL.md Runbooks section**: Added runbook links to SKILL.md so the skill knows about transfer/extract/compact procedures.
- **Housekeeping**: Gitignore `.cargo/config.toml`, resolve clippy warnings, remove accidentally committed files.

## 0.31.8

- **CI fix**: Removed `path = "../tmux-router"` override from Cargo.toml. CI runners don't have the local submodule; uses crates.io dependency exclusively.

## 0.31.7

- **Stash-bounce fix**: Removed `return_stashed_panes_bulk()` from automatic `prune()` path. Active panes now stay in stash until the reconciler explicitly needs them, eliminating the stash→return→stash loop that caused visible pane bouncing.
- **Sync file lock**: Added `flock` on `.agent-doc/sync.lock` to serialize concurrent sync calls. Prevents race conditions when rapid tab switches fire overlapping syncs.
- **Route sync removal**: Removed redundant `sync::run_layout_only` from Route command dispatch and `sync_after_claim` from route.rs. The JB plugin's `EditorTabSyncListener` is now the sole authority for layout sync.
- **Diagnostic checkpoints**: Added checkpoint logging in sync (`post-repair`, `post-prune`, `pre-tmux_router`) to pinpoint pane state at key transitions.

## 0.31.6

- **Debounce fix**: Default mtime debounce increased from 500ms to 2000ms. Configurable per-document via `agent_doc_debounce` frontmatter field.
- **Structured logging**: Added `tracing` + `tracing-subscriber` + `tracing-appender`. Set `AGENT_DOC_LOG=debug` to log to `.agent-doc/logs/debug.log.<date>`. Zero overhead when unset.
- **Pre-response cleanup bug**: `clear_pending()` now deletes pre-response snapshots after successful writes. Previously accumulated indefinitely.
- **Lock file cleanup bug**: `SnapshotLock::Drop` now deletes the lock file (not just unlocks). CRDT lock acquisition cleans stale locks (>1 hour old).
- **`agent-doc gc` subcommand**: Garbage-collects orphaned files in `.agent-doc/` directories. Supports `--dry-run` and `--root` flags.
- **Auto-GC on preflight**: Runs GC once per day via `.agent-doc/gc.stamp` timestamp check.
- **Cleanup runbook**: New `runbooks/cleanup.md` documenting `.agent-doc/` directory structure and cleanup rules.
- **Tracing instrumentation**: `tracing::debug!` at key decision points in sync, route, layout, and resync modules.
- **Source annotations for extract/transfer**: `agent-doc extract` and `agent-doc transfer` now wrap content with `[EXTRACT from ...]` or `[TRANSFER from ...]` blockquote annotations including timestamp.
- **Post-sync session health check**: After every sync, verifies the tmux session still exists. Logs `CRITICAL` if session was destroyed.
- **Route cleanup on failure**: When route fails, only panes that the current route attempt itself created are eligible for cleanup before the error propagates. Concurrent panes from sibling documents in the same tmux window are no longer treated as orphaned cleanup candidates.

## 0.31.5

- **Commit on claim**: `agent-doc claim` now commits the file after saving the initial snapshot. Ensures the first prompt appears as a diff against a committed baseline.
- **Auto-setup untracked files**: Preflight auto-adds untracked files to git (snapshot + `git add`), so `/agent-doc` works on new files without claiming first.
- **VCS refresh after commit**: `agent-doc commit` writes a VCS refresh signal file, prompting IDEs to update their git status display.
- **Preflight `--diff-only` flag**: Omits the full document from preflight JSON output, reducing token usage by ~80% on subsequent cycles.
- **Skill-bundled runbooks**: `agent-doc skill install` now installs runbooks alongside SKILL.md at `.claude/skills/agent-doc/runbooks/`. First runbook: `compact-exchange.md`.
- **JetBrains prompt button truncation**: maxLabelLen reduced from 45 to 25 characters.
- **Debounce module**: New `src/debounce.rs` for reusable debounce logic.

## 0.31.4

- **IPC reposition simplified**: Removed file-based IPC fallback from `try_ipc_reposition_boundary`. Boundary reposition now uses socket IPC exclusively (through FFI listener callback). Non-fatal on failure.
- **Inline `max_lines=N` attribute**: Component tags support `max_lines=N` to trim content to the last N lines after patching. Precedence: inline attr > `components.toml` > unlimited. Example: `<!-- agent:exchange patch=append max_lines=50 -->`.
- **Boundary-stripping in watch hash**: `hash_content()` strips boundary markers before hashing, preventing reactive-mode feedback loops where boundary repositions trigger infinite re-runs.
- **Console component scaffolding**: `agent-doc claim` now scaffolds a `<!-- agent:console -->` component for template-mode documents.
- **HEAD marker cleanup**: `git.rs` strips stray `(HEAD)` markers from working tree after commit (defensive cleanup).
- **StreamConfig max_lines**: `agent_doc_stream.max_lines` frontmatter field limits console capture lines (default: 50).
- **Tests**: 612 total. New: 4 `max_lines_*` tests in template.rs.
- **Docs**: SPEC.md, README.md, CLAUDE.md updated for max_lines and socket-only IPC.

## 0.31.3

- **Claim snapshot fix**: `agent-doc claim` now saves the initial snapshot with empty exchange content. Existing user text in the exchange becomes a diff on the next run, preventing unresponded prompts from being absorbed into the baseline.
- **Tests**: 608 total. New: `strip_exchange_content_removes_user_text`, `strip_exchange_content_preserves_no_exchange`.

## 0.31.2

- **`agent-doc dedupe`**: New command removes consecutive duplicate response blocks. Ignores boundary markers in comparison. Used to fix duplicate responses caused by watch daemon race conditions.
- **Write-origin tracing**: `--origin` flag on `agent-doc write` logs the write source (skill/watch/stream) to ops.log. Aids diagnosis when snapshot drift occurs.
- **Commit drift warning**: Warns when `file_len - snap_len > 100` bytes, indicating a possible out-of-band write that bypassed the snapshot pipeline.
- **Watch daemon busy guard**: Skips files with active agent-doc operations (`is_busy()` check), preventing the watch daemon from generating duplicate responses when competing with the skill.
- **PatchWatcher EDT fix**: Patch computation moved outside `WriteCommandAction`. No-op patches skip the write action entirely, eliminating EDT blocking and typing lag.
- **ClaimAction claim+sync**: `Ctrl+Shift+Alt+C` now calls `agent-doc claim` on the focused file before syncing, handling unclaimed/empty files.
- **Single-char truncation fix**: Single characters are treated as potentially truncated in `looks_truncated()`, requiring 1.5s stability check. Prevents partial typing (e.g., "S" from "Save as a draft.") from triggering premature runs.
- **SKILL.md**: All write examples include `--origin skill`. Version 0.31.2.
- **JetBrains plugin**: Version 0.2.40.
- **Tests**: 606 total. New: `truncated_single_chars`, `dedupe_*` (4 tests).
- **Docs**: SPEC.md §7.22 (--origin), §7.23 (busy guard), §7.28 (dedupe). CLAUDE.md module layout.

## 0.31.1

- **Declarative layout sync**: Navigating to a file in a split editor now creates a tmux pane automatically. Files with session UUIDs are always treated as Registered by sync, even without a registry entry (reverses 0.31.0 Unmanaged guard). Auto-start phase also no longer requires registry entries.
- **ClaimAction simplified**: JetBrains ClaimAction (Ctrl+Shift+Alt+C) now delegates entirely to SyncLayoutAction — removed 200+ lines of position detection, pane ID extraction, and independent auto-start logic.
- **Claim registry protection**: `agent-doc claim` refuses to overwrite an existing live claim without `--force`, preventing silent pane corruption from fallback position detection.
- **HEAD marker duplicate fix**: `add_head_marker` uses occurrence counting instead of substring matching, correctly marking new headings even when the same heading text exists earlier in the document.
- **Busy guard removed**: EditorTabSyncListener no longer blocks sync when any visible file has an active session. The binary's own concurrency guards (startup locks, registry locks) are sufficient.
- **Build stamp**: New `build.rs` embeds a build timestamp. On sync, the binary compares against `.agent-doc/build.stamp` and clears stale startup locks on new build detection.
- **Plugin binary resolution fix**: EditorTabSyncListener and SyncLayoutAction now pass `basePath` to `resolveAgentDoc()`, correctly resolving `.bin/agent-doc` instead of falling through to `~/.cargo/bin/agent-doc`.
- **JetBrains plugin**: Version 0.2.38. Requires uninstall→restart→install→restart (structural class changes).
- **Tests**: 602 total. New: `add_head_marker_duplicate_heading_text`.
- **Docs**: SPEC.md §7.10 (claim protection), §7.15 (occurrence counting), §7.20 (UUID-always-registered, build stamp). Ontology claim.md updated.

## 0.31.0

- **`agent-doc session` CLI**: Show/set configured tmux session with pane migration (`session_cmd.rs`).
- **Stash pane safety**: `purge_unregistered_stash_panes` no longer kills agent processes (agent-doc, claude, node) in stash — only idle shells. Prevents loss of active Claude sessions when registry goes stale.
- **Session resolution consolidation**: `resolve_target_session()` extracts duplicated session-targeting logic from route.rs into a single function. Config.toml is the source of truth; claim/route no longer auto-overwrite it.
- **Stale UUID handling**: Files with frontmatter session UUID but no registry entry are treated as Unmanaged by sync — prevents auto-starting sessions for unclaimed files.
- **Unused variable cleanup**: Fixed 8 warnings across route.rs and template.rs.
- **Docs**: SPEC.md §7.27 (session command), CLAUDE.md module layout updated.
- **Tests**: 601 total, 1 new (`purge_preserves_unregistered_agent_process_in_stash`).

## 0.30.1

- **FFI `agent_doc_is_idle`**: Non-blocking typing check for editor plugins to query idle state before boundary reposition.
- **JetBrains plugin typing debounce**: Boundary reposition deferred until typing stops, using FFI idle check.
- **VS Code koffi FFI bindings**: `native.ts` with koffi-based native bindings for the shared FFI library.
- **VS Code reposition boundary handling**: Boundary reposition with typing debounce via FFI idle check.
- **tmux_session config drift fix**: `route.rs` follows pane session, `claim.rs` updates config to match.
- **2 new FFI tests**: Coverage for `agent_doc_is_idle` and related FFI surface.
- **Dependencies**: `tmux-router` v0.3.8.

## 0.30.0

- **Stale baseline guard (component-aware)**: `is_stale_baseline()` now parses components and only checks append-mode (exchange, findings). Replace-mode components (status, pending) are skipped. Falls back to prefix check for inline docs. 11 new tests.
- **Busy pane guard**: `SyncOptions.protect_pane` callback in tmux-router DETACH phase + `layout.rs`. Prevents stashing panes with active agent-doc/claude sessions during layout changes.
- **Auto-start startup lock**: `.agent-doc/starting/<hash>.lock` with 5s TTL prevents double-spawn when sync fires twice in quick succession.
- **Bug 2A fix**: IPC snapshot save failure after successful write is now non-fatal with warning. Commit auto-recovers via divergence detection.
- **Bug 2B fix**: Removed commit-time divergence detection that was eating user edits into the snapshot.
- **Hook system**: `agent-doc hook fire/poll/listen/gc` CLI. Cross-session event coordination via `agent-kit` hooks (v0.3). `post_write` and `post_commit` events fired from write + commit paths.
- **HookTransport trait**: Abstract delivery mechanism with `FileTransport`, `SocketTransport`, `ChainTransport` implementations.
- **Ops logging tests**: 2 new tests for `.agent-doc/logs/ops.log`.
- **Dependencies**: `agent-kit` v0.3 (hooks feature), `tmux-router` v0.3.7 (SyncOptions).
- **Docs**: SPEC.md §6.6/§7.9/§7.20/§9.5, README.md key features, CLAUDE.md module layout.
- **Tests**: 595 total (16 new), 0 failures.

## 0.29.0

- **Links frontmatter**: Renamed `related_docs` → `links` (backward-compat alias). URL links (`http://`/`https://`) are fetched via `ureq`, converted HTML→markdown via `htmd` (stripping script/style/nav/footer), cached in `.agent-doc/links_cache/`, and diffed on each preflight. Non-HTML content passes through unchanged.
- **Session logging**: Persistent logs at `.agent-doc/logs/<session-uuid>.log` with timestamped events for session start, claude start/restart/exit, user quit, and session end.
- **Auto-trigger on restart**: After `--continue` restart, background thread sends `/agent-doc <file>` via `tmux send-keys` after 5s delay to re-trigger the skill workflow.
- **Security documentation**: README.md top-level security notice + detailed Security section. SPEC.md Section 10 with threat model, known risks, and recommendations.
- **New dependency**: `htmd` v0.5.3 (HTML-to-markdown, ~13 new crates from html5ever ecosystem, no HTTP server).
- **Tests**: 7 new tests for URL detection, HTML conversion, boilerplate stripping, cache paths. 361 total, 0 failures.

## 0.28.3

- **Write dedup boundary fix**: Strip `<!-- agent:boundary:XXXXXXXX -->` markers before dedup comparison. Boundary marker IDs change on each write, causing false negatives in the dedup check (content appeared different when only the boundary ID changed).

## 0.28.2

- **Write dedup**: All 4 write paths (`run`, `run_template`, `run_stream` disk, `run_stream` IPC) skip the write when merged content is identical to the current file. Dedup events logged to `/tmp/agent-doc-write-dedup.log` with backtrace.
- **Pane ownership verification**: `verify_pane_ownership()` called at entry of `run`, `run_template`, `run_stream`. Rejects writes when a different tmux pane owns the session (lenient — passes silently when not in tmux or pane is indeterminate).
- **Column memory**: `.agent-doc/last_layout.json` saves column→agent-doc mapping (carried from v0.28.1, now documented).

## 0.28.1

- **Column memory**: `.agent-doc/last_layout.json` saves column→agent-doc mapping. When a column has no agent doc, sync substitutes the last known agent doc from the state file. Preserves 2 tmux panes when one column switches to a non-agent file.

## 0.28.0

- **Empty col_args filtering**: `sync` now filters out empty strings from `col_args` before processing. Fixes phantom empty columns sent by the JetBrains plugin during rapid editor split changes.
- **Sync debug logging**: Added `/tmp/agent-doc-sync.log` trace logging at key sync decision points (col_args, repair_layout, auto-start, pre/post tmux_router::sync pane counts).
- **Post-auto_start stash removed**: The explicit stash after auto-start is no longer needed — `tmux_router::sync` always runs the full reconcile path (no early exits), so excess panes are stashed during the DETACH phase.
- **tmux-router v0.3.6**: Early exits removed from `sync` — the full reconcile path now runs for 0, 1, or 2+ resolved panes uniformly. Previous early exits for `resolved < 2` bypassed the DETACH phase, leaving orphaned panes from previous layouts visible.
- **JetBrains plugin v0.2.36**: Filter empty columns in SyncLayoutAction.kt

## 0.27.9

- **tmux-router v0.3.5**: Updated dependency — trace logging at key sync decision points + early-exit stash removal (preserves previous-column panes)

## 0.27.8

- **tmux-router v0.3.4**: Updated dependency — early-exit stash now derives session from pane via `pane_session()` instead of dead `doc_tmux_session` path
- **VERSIONS.md backfill**: Added entries for v0.23.2 through v0.26.6

## 0.27.7

- **Sync path column-aware split**: `auto_start_no_wait` now accepts `col_args` and computes `split_before` via `is_first_column()`. Previously hardcoded `split_before = false`, causing new panes to always split alongside the rightmost pane regardless of column position. The sync path (editor tab switches) now matches the route path behavior.

## 0.27.6

- **Bold-text pseudo-header fallback for `(HEAD)` marker**: `add_head_marker()` in `git.rs` now falls back to bold-text lines (`**...**`) when no markdown headings are found in new content. `strip_head_markers()` also handles stripping `(HEAD)` from bold-text lines.
- **SKILL.md header format guidance**: Added "Response header format (template mode)" section instructing agents to use `### Re:` headers. Bold-text pseudo-headers are supported as a fallback but real headings are preferred for outline visibility and sub-section nesting.

## 0.27.5

- **Column-aware split target**: `auto_start_in_session` picks the split target based on column position — first pane (leftmost) for left-column files, last pane (rightmost) for right-column files. Fixes 3-pane layout bug where new panes split the wrong existing pane.
- **Early-exit stash**: Before the `resolved < 2` early return in `tmux-router::sync`, excess panes in the agent-doc window are now stashed. Previously, old panes from previous layouts stayed visible when only one file resolved.
- **tmux-router v0.3.3**: Published with the early-exit stash fix.

## 0.27.4

- **Rescue stashed panes in sync**: `sync.rs` now rescues stashed panes back to the agent-doc window via swap-pane/join-pane before falling back to auto-start. Preserves Claude session context across editor tab switches.

## 0.27.3

- **Revert auto-kill**: Reverts v0.27.2 auto-kill of idle stashed Claude sessions. The `❯` prompt is the normal state of a stashed session waiting to be rescued — not an orphan indicator.

## 0.27.2

- **Auto-kill idle stashed Claude sessions**: Added auto-cleanup in `return_stashed_panes_bulk()` for stashed panes running agent-doc/claude at the `❯` prompt with no return target. (Reverted in v0.27.3 — too aggressive, killed active sessions.)

## 0.27.1

- **Fix "externally modified" popup**: Removed stale boundary disk write that caused spurious file modification notifications in editors.

## 0.27.0

- **Fix stash rescue deregistration**: Fixed pane deregistration during stash rescue operations.
- **Socket IPC**: Added `ipc_socket` module using Unix domain sockets via the `interprocess` crate for direct binary-to-plugin communication.
- **Bulk resync**: `return_stashed_panes_bulk()` for batch stash rescue operations.

## 0.26.6

- **FFI sync lock/debounce**: Added `agent_doc_sync_try_lock`/`unlock` FFI exports for cross-editor concurrency control. Added `agent_doc_sync_bump`/`check_generation` for cross-editor event coalescing.
- **Layout debounce fix**: `LayoutChangeDetector` uses generation counter instead of spawning concurrent threads per event.
- **JetBrains plugin v0.2.35**: Uses FFI sync primitives with local fallback.

## 0.26.5

- **Skip no-op IPC reposition**: IPC reposition signal skipped when boundary position is unchanged, eliminating ~64% of no-op PatchWatcher operations.
- **Handle inotify overflow**: PatchWatcher scans for missed files on inotify OVERFLOW events.
- **CI: crates.io-only dependencies**: All path dependencies (instruction-files, tmux-router, agent-kit, module-harness, existence) replaced with crates.io versions in CI workflows.

## 0.26.4

- **Prompt detection for Claude Code v2.1+**: Support numbered list format (`N. label`) in prompt option parsing alongside bracket format (`[N] label`).
- **Auto-start PromptPoller**: Plugin auto-starts PromptPoller on project open.
- **JetBrains plugin v0.2.32**: PromptPoller auto-start, `.bin/` path resolution, diagnostic logging.

## 0.26.3

- **Sync no longer auto-inits frontmatter**: Sync returns `Unmanaged` for files without session UUIDs; only `claim` adds frontmatter now.
- **Plugin mixed-layout sync**: Uses focus-only when non-`.md` files are in editor splits, preventing stashing.
- **JetBrains plugin v0.2.25**: Alt+Space popup, removed ActionPromoter (frees Alt+Enter for native JetBrains intentions).

## 0.26.2

- **Route single exit point**: Refactored route to `resolve_or_create_pane()` eliminating propagation bugs. `sync_after_claim` now runs on ALL route paths.
- **Response status signals**: File-based status signals (`.agent-doc/status/<hash>`) for cross-process visibility. FFI: `set_status`/`get_status`/`is_busy` for in-process plugin checks.
- **Auto-init unclaimed files in sync**: Sync writes session UUID for unclaimed files.
- **`agent_doc_version()` FFI export**: Runtime version tracking for plugins.
- **JetBrains plugin v0.2.24**: `is_busy()` guard in `EditorTabSyncListener` + `TerminalUtil`.

## 0.26.1

- **Sync layout authority**: `sync_after_claim` uses editor-provided `col_args`, preventing 3-pane layout regression on file switch.
- **Clippy fixes**: `doc_lazy_continuation` fixes in sync.rs, upgrade.rs. Unused variable fix in tmux-router `break_pane_to_stash`.
- **SPEC.md updates**: Added sections on project config, IPC write verification, and sync layout authority.

## 0.26.0

- **Kill pane safety**: `kill_pane` refuses to destroy a session's last window (tmux-router v0.3.0).
- **IPC verification**: Content verification catches partial plugin application failures. `--force-disk` cleans stale patches to prevent double-writes.
- **Module harness context**: All 53+ modules annotated with Spec/Contracts/Evals doc comments (468 named evals, 68% coverage).
- **Existence-lang ontology**: 9 domain terms defined (Document, Session, Component, Boundary, Snapshot, Patch, Exchange, Route, Claim). Dev dependencies: existence v0.4.0, module-harness v0.2.0.
- **README rewrite**: Concise GitHub-facing guide.

## 0.25.15

- **Sync layout repair**: Added `repair_layout()` to fix window index mismatches (agent-doc window not at index 0). Sync tests added for repair skip and move scenarios.
- **Blank line collapse on tmux_session strip**: Collapsing 3+ consecutive newlines to 2 when stripping deprecated `tmux_session` frontmatter field.

## 0.25.14

- **Sync pane repair**: Window index repair, pane state reconciliation, effective window tracking.
- **Resync enhancements**: Enhanced dead pane detection and session validation.
- **Route improvements**: Improved command routing logic.

## 0.25.13

- **Install script**: Rewritten `install.sh` with platform detection and improved install paths.
- **Homebrew formula**: Added `Formula/agent-doc.rb` for macOS/Linux Homebrew installation.
- **Deprecate `tmux_session` frontmatter**: Sync strips the field on encounter instead of repairing it. Route `auto_start` no longer attempts repair.

## 0.25.12

- **Sync swap-pane atomic reconcile**: `context_session` overrides frontmatter `tmux_session`, auto-repairs on mismatch.
- **Visible-window split**: New panes split in the visible agent-doc window instead of stash.
- **Resync report-only in sync**: `resync --fix` disabled in sync path to preserve cross-session panes.
- **tmux-router v0.2.9**: Swap-pane atomic transitions.

## 0.25.11

- **Tmux-router swap-pane atomic transitions**: Pane moves use `swap-pane` for flicker-free layout changes. CI fix for path dependencies (agent-kit, tmux-router).

## 0.25.10

- **Preflight mtime debounce**: 500ms idle gate before computing diff.
- **Unified diff context**: Diff output uses unified format with 5-line context radius.
- **Route `--debounce` flag**: Opt-in mtime polling for coalescing rapid editor triggers.
- **`is_tracked` FFI export**: For editor plugins to check file tracking status.
- **Sync no-wait auto-start**: `auto_start_no_wait` for non-blocking session creation during sync.
- **JetBrains plugin v0.2.21**: Sync logging improvements.

## 0.25.9

- **`is_tracked()` FFI export**: Conservative debounce on untracked files (fallback to local tracking).
- **Untracked file debounce fix**: Untracked files no longer bypass debounce.
- **JetBrains plugin v0.2.20**: `is_tracked` binding + FFI logging tags.

## 0.25.8

- **Preflight debounce**: Mtime-based 500ms idle gate before computing diff.
- **Unified diff context**: Switch diff output to unified format with 5-line context radius.
- **Route `--debounce`**: New flag for opt-in mtime polling to coalesce rapid editor triggers.
- **Truncation detection fix**: Smarter dot handling for domain fragments in `looks_truncated`.

## 0.25.7

- **Rename `submit` to `run`**: `submit.rs` renamed to `run.rs`; all internal "submit" terminology updated to "run".
- **FFI debounce module**: `document_changed()` + `await_idle()` FFI exports for editor-side debounce.
- **Route sync fix**: Route calls `sync::run_layout_only()` to prevent auto-start race conditions.
- **JetBrains plugin v0.2.19**: FFI debounce, conditional typing wait, layout-only sync.

## 0.25.6

- **Route `--col`/`--focus` args**: Declarative layout sync from the route command. Plugin `sendToTerminal` passes editor layout in a single CLI call.
- **Layout change detection**: `LayoutChangeDetector` using `ContainerListener` with 5s fallback poll in the JetBrains plugin.
- **EDT-safe threading**: Plugin uses `invokeLater` for Swing reads, background thread for CLI calls.
- **JetBrains plugin v0.2.17**.

## 0.25.5

- **FFI boundary reposition**: Export `agent_doc_reposition_boundary_to_end()` for plugin use.
- **Boundary ID summaries**: 8-char hex IDs with optional `:summary` suffix (filename stem). `new_boundary_id_with_summary()` wired into all write paths.
- **Snapshot boundary cleanup**: Commit path uses `remove_all_boundaries()`. Working tree cleaned via `clean_stale_boundaries_in_working_tree()` on commit.
- **JetBrains plugin v0.2.14**: FFI-first reposition with Kotlin fallback.

## 0.25.4

- **Boundary accumulation fix**: Plugin `repositionBoundaryToEnd` removes ALL boundaries, not just the last one.
- **Short boundary IDs**: 8 hex chars instead of full UUID (centralized in `lib.rs`).
- **Autoclaim pruning**: Validate file existence, prune stale entries on rename/delete.
- **Sync stale pane detection**: Detect alive panes with non-existent registered files (rename), kill stale pane and auto-start new session.

## 0.25.3

- **Fix IPC boundary reposition for prompt ordering**: All IPC write paths call `reposition_boundary_to_end()` before extracting boundary IDs. Previously the stale boundary position caused responses to appear before the prompt.

## 0.25.2

- **Fix skill install superproject root resolution**: Added `resolve_root()` to detect git superproject when CWD is in a submodule. `skill install`/`check` now writes to the project root, not the submodule's `.claude/skills/`.

## 0.25.1

- **IPC boundary reposition from commit**: After committing, send an IPC reposition signal to the plugin so it moves the boundary marker to end-of-exchange in its Document buffer. Avoids writing to the working tree (which would lose user keystrokes).

## 0.25.0

- **`agent-doc preflight` command**: Consolidated pre-agent command (recover + commit + claims + diff + document read) returning JSON for skill consumption.
- **Boundary reposition fix**: Snapshot-only reposition prevents losing user input; no working tree writes during reposition.
- **CRDT merge simplification**: Removed `reorder_agent_before_human()`, deterministic client IDs.
- **Pulldown-cmark outline**: CommonMark-compliant heading parser for outline.
- **Plugin boundary reposition via IPC**: `reposition_boundary: true` flag in IPC payloads.
- **Stash window routing**: Target largest pane, overflow to stash windows.
- **JetBrains plugin v0.2.12**: Plugin-side boundary reposition.

## 0.24.4

- **Deterministic boundary re-insertion in `apply_patches`**: Binary handles boundary re-insertion after checkpoint writes, removing the need for SKILL.md to manually re-insert boundaries.

## 0.24.3

- **Context session for auto_start**: Pass context session to `auto_start` to prevent routing to the wrong tmux session. Post-sync resync for consistency.

## 0.24.2

- **SKILL.md step 3b**: Added mandatory pending updates check each cycle.
- **`plugin install --local`**: Install JetBrains/VS Code plugins from local build directory.
- **JetBrains plugin v0.2.10**: `resync --fix` on startup.
- **JetBrains plugin v0.2.9**: VCS refresh signal fix (ENTRY_MODIFY event).

## 0.24.1

- **SKILL.md heredoc examples**: Updated bundled SKILL.md with heredoc examples for the write command.

## 0.24.0

- **`agent-doc install` command**: System-level setup that checks prerequisites (tmux, claude) and detects/installs editor plugins.
- **`agent-doc init` project mode**: No-arg `init` now initializes a project (creates `.agent-doc/` directory structure, installs SKILL.md) instead of requiring a file argument.
- **SKILL.md content tests**: CLI integration tests for skill install/check content verification.
- **Sync pane guard**: Pre-sync alive pane check prevents duplicate session creation.

## 0.23.3

- **Cross-platform sync pane guard**: `find_alive_pane_for_file()` uses `ps(1)` instead of `/proc` for Linux+macOS compatibility. Pre-sync auto-start checks alive panes before creating duplicates.
- **Clippy fixes**: Fix `collapsible_if` warnings in template.rs, git.rs, terminal.rs. Suppress `dead_code` warnings for library-only boundary functions.

## 0.23.2

- **Explicit patch boundary-aware insertion**: `apply_patches_with_overrides()` checks for boundary markers when applying explicit patch blocks in append mode, not just unmatched content. Prevents boundary markers from accumulating as orphans.
- **Version bump**: Includes all v0.23.1 fixes (IPC snapshot, HEAD marker cleanup, boundary insertion).

## 0.23.1

- **Boundary-aware insertion for unmatched content**: `apply_patches_with_overrides()` now uses boundary-aware insertion for both explicit append-mode patches and unmatched content routed to `exchange`/`output`. Previously only explicit patches used boundary markers; unmatched content used plain append.
- **IPC snapshot correctness**: `try_ipc()` now accepts a `content_ours` parameter (baseline + response, without user concurrent edits). On IPC success the snapshot is saved from `content_ours` instead of re-reading the current file, preventing user edits typed after the boundary from being absorbed into the snapshot.
- **IPC synthesized exchange patch**: When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware component patch so the plugin inserts at the correct position.
- **`boundary.insert()` cleans stale markers**: Before inserting a new boundary marker, `insert()` strips all existing boundary markers from the document. Prevents orphaned markers accumulating across interrupted sessions.
- **`boundary::find_boundary_id_in_component()`**: New public function. Scans a pre-parsed `Component` for any boundary marker UUID, skipping matches inside code blocks. Used by `template.rs` and external callers without re-parsing components.
- **Post-commit working tree cleanup**: After `git.commit()` succeeds, `strip_head_markers()` is applied to both the snapshot and the working tree file. Ensures `(HEAD)` markers never appear in the editor — they exist only in the committed version (creating the blue gutter diff).

## 0.23.0

- **Boundary marker for response ordering**: New `agent-doc boundary <FILE>` command inserts `<!-- agent:boundary:UUID -->` at the end of append-mode component content. The marker acts as a physical anchor — responses are inserted at the marker position, ensuring correct ordering when the user types while a response is being generated. Replaces the fragile caret-offset approach.
- **Boundary-aware FFI**: New `agent_doc_apply_patch_with_boundary()` C ABI export. JetBrains plugin (`NativeLib.kt`, `PatchWatcher.kt`) uses boundary markers with priority over caret-aware insertion.
- **Component parser: boundary marker exclusion**: `<!-- agent:boundary:* -->` comments are now skipped by the component parser (no longer cause "invalid component name" errors).
- **IPC boundary_id**: All IPC patch JSON payloads include `boundary_id` when a boundary marker is present in the target component.
- **SKILL.md: boundary marker step**: Updated bundled SKILL.md to call `agent-doc boundary <FILE>` after reading the document (step 1b).
- **Claim auto-start**: JetBrains plugin "Claim for Tmux Pane" action now auto-starts the agent session after successful claim.
- **JetBrains plugin v0.2.8**: Boundary-aware patching + claim auto-start.

## 0.22.2

- **SKILL.md: immediate commit after write**: Updated bundled SKILL.md to call `agent-doc commit` right after `agent-doc write`, replacing the old "Do NOT commit after writing" instruction. All sessions get the new behavior after `agent-doc skill install`.
- **Plugin default modes**: `exchange` and `findings` components now default to `append` mode in the JetBrains plugin (matching the Rust binary's `default_mode()`), so `<!-- agent:exchange -->` works without explicit `patch=append`.

## 0.22.1

- **Any-level HEAD markers**: `(HEAD)` marker now matches any heading level (`#`–`######`), not just `###`. Only root-level (shallowest) headings in the agent's appended content are marked.
- **Multi-heading markers**: When the agent response has multiple sections, ALL new root headings get `(HEAD)` markers (comparing snapshot vs git HEAD).
- **VCS refresh signal**: After `agent-doc commit`, writes `vcs-refresh.signal` to `.agent-doc/patches/`. Plugin watches for this and triggers `VcsDirtyScopeManager.markEverythingDirty()` + VFS refresh so git gutter updates immediately.
- **JetBrains plugin v0.2.7**: VCS refresh signal handling, cursor-aware FFI, VFS refresh before dirty scope.

## 0.22.0

- **`agent-doc terminal` subcommand**: Cross-platform terminal launch from editor plugins. Config-first (no hard-coded terminal list): `[terminal] command` in `config.toml` with `{tmux_command}` placeholder. Fallback to `$TERMINAL` env var. Detects stale frontmatter sessions and scans registry for live panes.
- **Selective commit**: `agent-doc commit` stages only the snapshot content via `git hash-object` + `git update-index`, leaving user edits in the working tree as uncommitted. Agent response → committed (no gutter). User input → uncommitted (green gutter).
- **HEAD marker**: Committed version of the last `### ` heading gets ` (HEAD)` suffix, creating a single modified-line gutter as a visual boundary and navigation point.
- **First-submit snapshot fix**: When no snapshot exists and git HEAD content matches the current file, treat as first submit (entire file is the diff) instead of "no changes detected".
- **Cursor-aware FFI**: `agent_doc_apply_patch_with_caret()` in shared library — inserts append-mode patches before the cursor position. `Component::append_with_caret()` in `component.rs`. JNA binding in `NativeLib.kt`.
- **JetBrains plugin v0.2.7**: Cursor-aware append ordering via native FFI with Kotlin fallback. Captures caret offset from `TextEditor` before `WriteCommandAction`.

## 0.21.0

- **`agent-doc parallel` subcommand**: Fan-out parallel Claude sessions across isolated git worktrees. Each subtask gets its own worktree and tmux pane. Results collected as markdown with diffs. `--no-worktree` for read-only tasks.
- **CRDT post-merge reorder**: Agent content ordered before human content at append boundary using Yrs per-character attribution (`Text::diff` with `YChange::identity`).
- **README**: Added parallel fan-out documentation section.

## 0.20.3

- **`agent-doc claims` subcommand**: Read, print, and truncate `.agent-doc/claims.log` in a single binary call. Replaces the shell one-liner (`cat + truncate`) that was prone to zombie process accumulation when the Bash tool auto-backgrounded it.

## 0.20.2

- **Fix: numeric session name ambiguity** (tmux-router v0.2.8): `new_window()` now appends `:` to session name (`-t "0:"` instead of `-t "0"`). Without the colon, tmux interprets numeric names as window indices, creating windows in the wrong session. Root cause of persistent session 1 bleedover bug.

## 0.20.1

- **Session affinity enforcement**: Route and auto_start bail with error instead of falling back to `current_tmux_session()` when `tmux_session` is set in frontmatter. Prevents pane creation in wrong tmux session.

## 0.20.0

- **CRDT conservative dedup** (#15): Post-merge pass removes identical adjacent text blocks.
- **CRDT frontmatter patches** (#16): `patch:frontmatter` now applied on disk write path (was IPC-only).
- **Binary-vs-agent responsibility** documented in CLAUDE.md.

## 0.19.0

- **ExecutionMode in config.toml**: `execution_mode = "hybrid|parallel|sequential"` in global config.
- **TmuxBatch**: Command batching in tmux-router v0.2.7 — reduces flicker via `\;` separator. `select_pane()` uses batch (2 → 1 invocation).

## 0.18.1

- **Revert Gson**: Hand-written JSON parser restored in JetBrains plugin (Gson causes ClassNotFoundException).
- **H2 scaffolding**: `claim` scaffolds h2 headers before components for IDE code folding.
- **SKILL.md**: Canonical pattern documented — h2 header before every component.

## 0.18.0

- **`agent-doc undo`**: Restore document to pre-response state (one-deep).
- **`agent-doc extract`**: Move last exchange entry between documents.
- **`agent-doc transfer`**: Move entire component content between documents.
- **Pre-response snapshots**: Saved before every write for undo support.

## 0.17.30

- **Immutable session binding**: `claim` refuses to overwrite `tmux_session` unless `--force`. Prevents cross-session pane swapping.

## 0.17.29

- **JNA FFI integration**: `NativeLib.kt` JNA bindings for JetBrains plugin with Kotlin fallback.
- **`agent_doc_merge_frontmatter()`**: New FFI export for frontmatter patching.
- **`agent-doc lib-path`**: Print path to shared library for plugin discovery.
- **VS Code prepend mode**: Fixed missing `prepend` case in `applyComponentPatch()`.

## 0.17.28

- **Validate tmux_session before routing**: Guard against routing to a non-existent tmux session.

## 0.17.27

- **Plugin code-block fix**: JetBrains and VS Code plugins skip component tags inside fenced code blocks. JB plugin 0.2.4, VSCode 0.2.2.

## 0.17.26

- **PLUGIN-SPEC docs update**: Document recent plugin features in PLUGIN-SPEC.

## 0.17.25

- **Stash else-branch fix**: Fix else-branch stash logic. Use `diff --wait` for truncation detection.

## 0.17.24

- **Pulldown-cmark for code range detection**: Replace hand-rolled code span/fence parser with `pulldown-cmark` in component parser. Stash overflow panes instead of creating new windows.

## 0.17.23

- **Stash overflow fix**: Overflow panes stashed instead of creating new tmux windows.

## 0.17.22

- **UTF-8 corruption fix**: Sanitize component tags in response content before writing to prevent UTF-8 corruption in `sanitize_component_tags`.

## 0.17.21

- **Indented fenced code blocks**: Component parser skips markers inside indented fenced code blocks. Scaffold `agent:pending` in claim for template documents.

## 0.17.20

- **BREAKING CHANGE: Rename `mode` to `patch`** for inline component attributes (`patch=append|replace`). `mode=` accepted as backward-compatible alias.

## 0.17.19

- **Split-window in auto_start**: Use `split-window` instead of `new-window` for auto-started Claude sessions. Resync tests added.

## 0.17.18

- **Resync `--fix` enhancements**: Detect wrong-session panes and wrong-process registrations. Renamed `--dangerously-set-permissions` to `--dangerously-skip-permissions`.

## 0.17.17

- **Parse fix**: `parse_option_line` matches `[N]` bracket format only. Fix `find_registered_pane_in_session` lookup.

## 0.17.16

- **Cursor editor support**: Add Cursor as a supported editor. `claude_args` frontmatter field for custom CLI arguments. Tmux session routing fix. VS Code extension bumped to v0.2.1.

## 0.17.15

- **Route/sync improvements**: Routing and sync refinements for multi-session workflows.

## 0.17.14

- **Plugin IPC fix**: VS Code IPC parity with JetBrains. History command improvements. Documentation updates.

## 0.17.13

- **Fix exchange append mode**: Remove hardcoded replace override in `run_stream`, allowing exchange component to use its configured patch mode.

## 0.17.12

- **Inline component attributes**: `<!-- agent:name mode=append -->` — patch mode configurable directly on the component tag.

## 0.17.11

- **History command**: `agent-doc history` shows exchange version history from git with restore support. IPC-priority writes with `--force-disk` flag to bypass.

## 0.17.10

- **Default component scaffolding**: Auto-scaffold missing components on claim. Append-mode exchange default. Route flash notification via `tmux display-message`.

## 0.17.9

- **Fix CRDT character interleaving**: Switch to line-level diffs to prevent character-level interleaving artifacts.

## 0.17.8

- **Template parser code block awareness**: Component markers inside fenced code blocks are now skipped by the template parser.

## 0.17.7

- **Fix CWD drift**: Recover and claim commands no longer drift from the project root working directory.

## 0.17.6

- **Documentation update**: Align docs with IPC-first write architecture from v0.17.5.

## 0.17.5

- **IPC-first writes**: All write paths (`run`, `stream`, `write`) try IPC to the IDE plugin via `.agent-doc/patches/` before falling back to disk. Exit code 75 on IPC timeout.

## 0.17.4

- **Tmux pane orientation fix**: Arrange files side-by-side (horizontal split) instead of stacking vertically.

## 0.17.3

- **Fix CRDT character-level interleaving bug**: Resolve text corruption caused by character-level merge conflicts in CRDT state.

## 0.17.2

- **Fix CRDT shared prefix duplication bug**: Prevent duplicate content when CRDT documents share a common prefix.

## 0.17.1

- **Fix stream snapshot**: Use replace mode for exchange component in stream snapshot writes.

## 0.17.0

- **BREAKING CHANGE: `agent_doc_format`/`agent_doc_write` split**: Replace `agent_doc_mode` with separate format (`inline`|`template`) and write strategy (`disk`|`crdt`) fields. IPC write path for IDE plugins. Layout fix.

## 0.16.1

- **Native compact for template/stream mode**: `agent-doc compact` now works natively with template and stream mode documents.

## 0.16.0

- **Reactive stream mode**: CRDT-mode documents get zero-debounce reactive file-watching from the watch daemon. Truncation detection and CRDT stale base fix.

## 0.15.1

- **Patch release**: Version bump and minor fixes.

## 0.15.0

- **CRDT-based stream mode**: Real-time streaming output with CRDT conflict-free merge (`agent-doc stream`). Chain-of-thought support with optional `thinking_target` routing. Deferred commit workflow. Snapshot resolution prefers snapshot file over git.

## 0.14.9

- **Multi-backtick code span support**: `find_code_ranges` handles multi-backtick code spans (e.g., ` `` ` and ` ``` `).

## 0.14.8

- **Code-range awareness for strip_comments**: Fix `<!-- -->` stripping inside code spans and fenced blocks. Stash window purge for orphaned idle shells.

## 0.14.7

- **Bidirectional convert**: `agent-doc convert` works in both directions (inline <-> template). Autoclaim sync improvements.

## 0.14.6

- **Auto-sync on lazy claim**: Automatically sync tmux layout after lazy claim in route. Plugin autocomplete fixes for JetBrains.

## 0.14.5

- **`agent-doc commands` subcommand**: List available commands. Plugin autocomplete for JetBrains/VS Code. Remove auto-prune (moved to resync). Purge orphaned claude/stash tmux windows in resync.

## 0.14.4

- **Claim pane focus**: Focus the claimed pane after `agent-doc claim`. `convert` handles documents with pre-set template mode.

## 0.14.3

- **Autoclaim pane refresh**: Refresh pane info during autoclaim. Template missing-component recovery on write.

## 0.14.2

- **Skill reload via `--reload` flag**: Compact and restart skill installation in a single command.

## 0.14.1

- **SKILL.md workflow fix**: Move git commit to after write step in the skill workflow to prevent committing stale content.

## 0.14.0

- **Route focus fix + claim defaults to template mode**: New documents claimed via `agent-doc claim` default to template format. `agent-doc mode` CLI command for inspecting/changing document mode.

## 0.13.3

- **Bump tmux-router to v0.2.4**: Fix spare pane handling in tmux-router dependency.

## 0.13.2

- **Sync registers claims**: `agent-doc sync` registers claims for previously unregistered files in the layout.

## 0.13.1

- **Sync updates registry file paths**: Fix autoclaim file path tracking when sync moves files between panes.

## 0.13.0

- **Autoclaim + git-based snapshot fallback**: Automatic claim on route when no claim exists. Fall back to git for snapshot when snapshot file is missing.

## 0.12.2

- **Exchange component defaults to append mode**: The `exchange` component uses append patch mode by default instead of replace.

## 0.12.1

- **Lazy claim fallback**: `agent-doc claim` without `--pane` falls back to the active tmux pane.

## 0.12.0

- **`agent-doc convert` command**: Convert between inline and template document formats. Lazy claim support. `agent-doc compact` for git history squashing. Exchange component as default template target.

## 0.11.2

- **Strip trailing `## User` heading**: Also strip trailing `## User` heading from agent responses (complement to v0.11.1).

## 0.11.1

- **Strip duplicate `## Assistant` heading**: Remove duplicate `## Assistant` heading from agent responses when already present in the document.

## 0.11.0

- **Append-friendly merge strategy**: Improved 3-way merge strategy optimized for append-style document workflows.

## 0.10.1

- **Bundle template-mode instructions in SKILL.md**: SKILL.md now includes template-mode workflow instructions for the Claude Code skill.

## 0.10.0

- **BREAKING CHANGE: Rename `response_mode` to `agent_doc_mode`**: Frontmatter field renamed with backward-compatible aliases.

## 0.9.10

- **Code-span parser fix**: Component parser skips markers inside fenced code blocks and inline backticks. Template input/output component support.

## 0.9.9

- **Template mode + compaction recovery**: New template mode for in-place response documents using `<!-- agent:name -->` components. Durable pending response store for crash recovery during compaction.

## 0.9.8

- **Relocate advisory locks**: Move document advisory locks from project root to `.agent-doc/locks/`.

## 0.9.7

- **`agent-doc write` command**: Atomic response write-back command for use by the Claude Code skill.

## 0.9.6

- **Race condition mitigations**: Stale snapshot recovery, atomic file writes, and various race condition fixes.

## 0.9.5

- **Advisory file locking**: Lock the session registry during writes. Stale claim auto-pruning.

## 0.9.4

- **Bump tmux-router to v0.2**: Update tmux-router dependency.

## 0.9.3

- **Bump tmux-router to v0.1.3**: Fix stash window handling in tmux-router.

## 0.9.2

- **`agent-doc plugin install` CLI**: Install editor plugins from GitHub Releases. VS Code extension reaches feature parity with JetBrains.

## 0.9.1

- **Stash window resize fix**: Bump tmux-router to v0.1.2 to fix stash window resize issues.

## 0.9.0

- **Dashboard-as-document**: Component-based documents with `<!-- agent:name -->` markers, `agent-doc patch` for programmatic updates, `agent-doc watch` daemon for auto-submit on file change.

## 0.8.1

- **Auto-prune registry**: Prune dead session entries before route/sync/claim operations.

## 0.8.0

- **Tmux-router integration**: Wire `tmux-router` as a dependency for pane management. Fix `route` auto_start bug.

## 0.7.2

- **Attach-first reconciliation**: Sync uses attach-first strategy with auto-register for untracked panes. Column-positional focus. Tmux session affinity.

## 0.7.1

- **Additive reconciliation**: Convergent reconciliation loop (max 3 attempts) with deferred eviction and reorder phase. Nuclear rebuild fallback.

## 0.7.0

- **Snapshot-diff sync architecture**: Rewrite sync to use snapshot-based diffing for tmux layout reconciliation. Dead window handling and column inversion fix.

## 0.6.6

- **`--focus` on sync**: `agent-doc sync` accepts `--focus` flag. Inline hint notification at cursor position in JetBrains plugin.

## 0.6.5

- **Always use `sync --col`**: Single-file sync uses column mode. Break out unwanted panes. Plugin notification balloon for detected layout.

## 0.6.4

- **Sync window filtering + layout equalization**: Filter sync to target window only. Equalize pane sizes after layout.

## 0.6.3

- **LayoutDetector fix**: Skip non-splitter Container children in JetBrains plugin 3-column layout detection.

## 0.6.2

- **Fire-and-forget Junie bridge**: Junie bridge script resolved automatically. Plugin clipboard handoff for non-tmux editors.

## 0.6.1

- **Junie agent backend**: Add Junie as an agent backend with JetBrains plugin action support.

## 0.6.0

- **`agent-doc sync` command**: 2D columnar tmux layout synced to editor split arrangement. Dynamic pane groups.

## 0.5.6

- **Commit message includes doc name**: `agent-doc commit` message format now includes the document filename. `agent-doc outline` command for markdown section structure with token counts.

## 0.5.5

- **Window-scoped routing**: Route commands scoped to tmux window (not just session). `--pane`/`--window` flags. Layout safeguards. JetBrains plugin self-disabling Alt+Enter popup (removes ActionPromoter).

## 0.5.4

- **Positional claim**: `agent-doc claim <file>` accepts file as positional argument. Editor plugin improvements and SPEC updates.

## 0.5.3

- **Bundled SKILL.md with absolute snapshot paths**: Snapshot paths use absolute paths for reliability. Resync subcommand and claims log documentation.

## 0.5.2

- **Claim notifications + resync + plugin popup**: Notification on claim. `agent-doc resync` validates sessions.json and removes dead panes. JetBrains and VS Code editor plugins added.

## 0.5.1

- **Windows build fix**: Cfg-gate unix-only exec in `start.rs` for cross-platform compilation.

## 0.5.0

- **`agent-doc focus` and `agent-doc layout`**: Focus a tmux pane for a session document. Layout arranges tmux panes to mirror editor split arrangement.

## 0.4.4

- **Rename SPECS.md to SPEC.md**: Standardize specification filename.

## 0.4.3

- **Commit CWD fix**: Fix working directory for `agent-doc commit`. SKILL.md prohibition rules.

## 0.4.2

- **SPEC.md gaps filled**: Document comment stripping as skill-level behavior (§4), `--root DIR` flag for audit-docs (§7.6), `agent-doc-version` frontmatter field for auto-update detection (§7.12), and startup version check (`warn_if_outdated`).
- **Flaky test fix**: Skill tests no longer use `std::env::set_current_dir`. Refactored `install`/`check` to accept an explicit root path (`install_at`/`check_at`), eliminating CWD races in parallel test execution.
- **CLAUDE.md module layout updated**: Added `claim.rs`, `prompt.rs`, `skill.rs`, `upgrade.rs` to the documented module layout.

## 0.4.1

- **SKILL.md: comment stripping for diff**: Strip HTML comments (`<!-- ... -->`) and link reference comments (`[//]: # (...)`) before comparing snapshot vs current content. Comments are a user scratchpad and no longer trigger agent responses.
- **SKILL.md: auto-update check**: New `agent-doc-version` frontmatter field enables pre-flight version comparison. If the installed binary is newer, `agent-doc skill install` runs automatically before proceeding.
- **PromptPanel: JDialog to JLayeredPane overlay**: Replace `JDialog` popup with a `JLayeredPane` overlay in the JetBrains plugin, eliminating window-manager popup leaks.

## 0.4.0

- **`agent-doc claim <file>`**: New subcommand — claim a document for the current tmux pane. Reads session UUID from frontmatter + `$TMUX_PANE`, updates `sessions.json`. Last-call-wins semantics. Also invokable as `/agent-doc claim <file>` via the Claude Code skill.
- **`agent-doc skill install`**: Install the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md` in the current project. The skill content is embedded in the binary via `include_str!`, ensuring version sync.
- **`agent-doc skill check`**: Compare installed skill vs bundled version. Exit 0 if up to date, exit 1 if outdated or missing.
- **SKILL.md updated**: Fixed stale `$()` pattern → `agent-doc commit <FILE>`. Added `/agent-doc claim` support.
- **SPEC.md expanded**: Added §7.7–7.13 (all commands), §8 Session Routing with use case table (U1–U11), §8.3 Claim Semantics.

## 0.3.0

- **Multi-session prompt polling**: `agent-doc prompt --all` polls all live sessions in one call, returns JSON array. `SessionEntry` now includes a `file` field for document path (backward-compatible).
- **`agent-doc commit <file>`**: New subcommand — `git add -f` + commit with internally-generated timestamp. Replaces shell `$()` substitution in IDE/skill workflows.
- **Prompt detection**: `agent-doc prompt` subcommand added in v0.2.0 (unreleased).
- **send-keys fix**: Literal text (`-l`) + separate Enter, `new-window -a` append flag (unreleased since v0.2.0).

## 0.1.4

- **`agent-doc upgrade` self-update**: Downloads prebuilt binary from GitHub Releases as the primary upgrade strategy. Falls back to `cargo install`, then `pip install --upgrade`, then manual instructions including `curl | sh`.

## 0.1.3

- **Upgrade check**: Queries crates.io for latest version with a 24h cache. Prints a one-line stderr warning on startup if outdated.
- **`agent-doc upgrade`**: New subcommand tries `cargo install` then `pip install --upgrade`, or prints manual instructions.

## 0.1.2

- **Language-agnostic audit-docs**: Replace Cargo.toml-only root detection with 3-pass strategy (project markers → .git → CWD fallback). Scan 28 file extensions across 6 source dirs instead of .rs only.
- **--root CLI flag**: Override auto-detection of project root for audit-docs.
- **Test coverage**: Add unit tests for frontmatter, snapshot, and diff modules.

## 0.1.0

Initial release.

- **Interactive document sessions**: Edit a markdown document, run an AI agent, response appended back into the document.
- **Session continuity**: YAML frontmatter tracks session ID, agent backend, and model. Fork from current session on first run, resume on subsequent.
- **Diff-based runs**: Only changed content is sent as a diff, with the full document for context. Double-run guard via snapshots.
- **Merge-safe writes**: 3-way merge via `git merge-file` if the file is edited during agent response. Conflict markers written on merge failure.
- **Git integration**: Pre-commit user changes before agent call, leave agent response uncommitted for editor diff gutters. `-b` flag for auto-branch, `--no-git` to skip.
- **Agent backends**: Agent-agnostic core. Claude backend included. Custom backends configurable via `~/.config/agent-doc/config.toml`.
- **Commands**: `run`, `init`, `diff`, `reset`, `clean`, `audit-docs`.
- **Editor integration**: JetBrains External Tool, VS Code task, Vim/Neovim mapping.
- **Backlog-required review closeout is now fail-closed.** Preflight now persists a cycle-scoped "requires backlog capture" contract derived from prompt targets plus recursive frontmatter `prompt_presets` expansion (for example `#code-review` chaining into `#follow-up-backlog`). `plan` now emits `expect_add` for those preset-driven review prompts, and `finalize` / `session-check` now fail when such a cycle records no backlog mutations unless the response explicitly states that there were no actionable follow-up items to capture. Added regressions for preset-expanded plan detection plus pre-commit/post-commit enforcement and the explicit-no-follow-ups escape hatch.
- **Pre-prompt Codex `Ctrl-D` exits now restart fresh instead of stalling reroutes behind the supervisor quit prompt.** `start.rs` now treats a forwarded `Ctrl-D`/stdin EOF on a fresh or fresh-restart Codex child that never surfaced an idle prompt as failed startup provenance, so the supervisor restarts fresh automatically instead of prompting for quit/restart. The successor run also suppresses only the stale inherited pre-prompt `Ctrl-D` byte until a real prompt appears. This closes the `monsterrodholders.md` `%179` shape from `tasks/agent-doc/agent-doc-bugs2.md`, where dispatch-only reroutes kept failing with `still booting` while the live pane sat behind `ctrl_d=true ... action=prompt_user`. Added restart-strategy regression coverage and updated the Codex/supervisor spec text.
- **Live Codex reroutes now get one fresh-supervisor retry before route records a startup-miss.** `route.rs` still requires a real document-cycle ack after injecting `agent-doc <FILE>` into a ready live pane, but when that ack never arrives on a still-live Codex session route no longer fails closed immediately. It now asks the supervisor for a one-shot **fresh** restart of that same pane, waits for the restarted Codex prompt to become dispatch-ready again, and resends the same bare reopen exactly once before falling back to the existing startup-miss error. This closes the cancel + `/clear` shape from `tasks/agent-doc/agent-doc-bugs2.md`, where the pane was alive and apparently idle but the stale conversation state would absorb the routed reopen without ever starting a new document cycle. Added regression coverage and updated the routing spec.
- **Same-document Codex reroutes no longer fail closed purely on a missed idle-prompt heuristic after a no-op scoped fix.** `route.rs` still waits for the pane to look dispatch-ready first, but when the registered pane still authoritatively owns the document, the scoped fix makes no changes, and the supervisor is healthy, route now retries the bare `agent-doc <FILE>` reopen once and requires the usual cycle-start acknowledgment before success. This removes the false-negative `monsterrodholders.md` / busy-pane route failure where Codex was effectively idle but prompt detection never stabilized, without dropping the fail-closed startup-miss proof if the reopen still does not start a cycle.
- **Repair/write normalization now preserves legacy alias tags in existing backlog items.** The pending/backlog compatibility path still rejects genuinely new duplicate custom-id prefixes, but it no longer fails replay just because an already-existing backlog line begins its free-form text with a secondary reference tag such as `[#ss01]` or `[#wpmem]`. That closes the `monsterrodholders.md` repair-blocked shape where an orphaned `<!-- patch:backlog -->` replay warned about legacy backlog syntax and then died on `duplicate leading custom id prefix` even though the live document itself had no `patch:backlog` block. Added normalization regressions for both the preserved existing-alias case and the still-rejected new-item case.
- **Dispatch-only editor reroutes now stop after one bare reopen and fail closed on explicit shell blockers.** `route --dispatch-only` still resolves the authoritative pane and sends the literal `agent-doc <FILE>` reopen, but it no longer reuses the managed route's Enter-retry acceptance loop. That means editor hotkeys will not keep pressing Enter for 5 seconds when the reopen text remains visible in pane scrollback, which could previously accept stray shell state and launch commands like `nvim`. Dispatch-only also now checks the current pane capture for explicit interactive blockers such as `reverse-i-search`, shell history search, queued drafts, or active permission prompts and refuses to inject anything else into those states. Added route regressions covering both the no-extra-Enter contract and the reverse-i-search fail-closed guard. This addresses the latest JetBrains `agent-doc-bugs2.md` unexpected `nvim` launch report.
