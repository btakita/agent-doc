# Versions

agent-doc is alpha software. Expect breaking changes between minor versions.

Use `BREAKING CHANGE:` prefix in version entries to flag incompatible changes.

## Unreleased

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
