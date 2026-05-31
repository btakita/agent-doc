---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 70b4d2d9-c397-4c86-8c2b-9effed66f82e
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
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260531-042017.md`*

Compacted content:
- Archived 2 response topic(s): monsterrodholders Run Agent Doc idle-but-queued stall; #closeout-recovery-state-machine
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#codex-owned-pane-prompt-miss] — opus-4-8

> **Queue prompt:**
>
> do [#codex-owned-pane-prompt-miss]


Fixed the owner-pane self-invocation prompt miss and shipped it (submodule `dede28d8`, both binaries built + installed, `make check` green, pushed).

**Root cause.** The recursive same-pane deadlock guard fired *after* `git::commit()` pre-commit and `start_run_cycle`. So when a Codex-owned pane re-invoked `agent-doc <FILE>` while an unresolved exchange prompt was still pending, pre-commit baselined that prompt into `HEAD`, the guard then abandoned the empty cycle, and the next preflight saw `no_changes` — the user prompt was silently lost.

**Fix (fallback path from the plan — fail-closed, prompt-preserving).**

1. **Early guard in `run.rs`** — a new `owned_pane_self_invocation_detail` detector (shared with the late deadlock guard so they cannot disagree on ownership). When it matches *and* `session_check::unresolved_exchange_prompt` returns a pending prompt, `run` bails **before** pre-commit and before opening a cycle. `owned_pane_prompt_miss_diagnostic` names the prompt and the in-pane recovery path and tells the operator not to re-run the same direct command. The prompt stays uncommitted and executable.
2. **Late guard unchanged for the no-prompt case** — queue-only / no-unresolved-prompt recursion still reaches the `#recguard-abandon` path and abandons the empty cycle.
3. **`session_check` backstop** — an abandoned `recursive_direct_invocation_blocked` cycle whose document still carries an unresolved exchange prompt is now reported as a missed-prompt recovery instead of accepted as terminal closeout (defense in depth for older cycles / alternate paths).

**Tests.** Split the old integration test into two: owner-pane + unresolved prompt → early fail-closed before pre-commit (no cycle opened, prompt retained); owner-pane + queue-only/no-prompt → still abandons the recursive cycle. Added a `session_check` unit test for the backstop. `make check` passes.

**Spec/docs.** Updated `README.md` and `specs/07-core-commands.md` with the early-guard + backstop contract.

Gated `#codex-owned-pane-prompt-miss` (code-complete, awaiting a real Codex same-pane live-verify) and queued `#codex-owned-pane-prompt-miss-followups` for the plan's *preferable* enhancements (structured `owned_pane_self_invocation` preflight result shape + Codex guidance/hook regression) plus the live-verify step.

### Re: do [#stuck-capture-compact-false-positive] — opus-4-8

> **Queue prompt:**
>
> do [#stuck-capture-compact-false-positive]


Done — shipped the durable reconciliation (submodule `7cc2ead5`, both binaries built + installed, `make check` green, pushed).

**What was already in place.** The false positive was suppressed at two levels: `compact` discards capture sidecars whose response was archived (`discard_captures_for_archived_responses`), and `stuck_captured_cycle` re-checks the HEAD-referenced compact archive each preflight and returns `None` when the response is materialized there (with a test).

**The remaining gap (the genuine fix).** That preflight suppression was *not durable*: it re-reads the archive on every pass and never settles the capture. If the referenced compact archive were later GC'd, `response_materialized_in_head_compact_archive` would go false again, `stuck_captured_cycle` would re-flag stuck, and preflight would re-suggest a `write --commit` that re-injects the already-archived response — the exact bug class.

**Fix.** `preflight` now runs `reconcile_compacted_committed_capture` *before* the stuck detection: when a committed-cycle capture's body is absent from `HEAD` but proven present in a HEAD-referenced compact archive, it durably marks the capture `Discarded` — the same terminal state compaction assigns — and logs `reconcile_compacted_committed_capture`. It advances only the active capture (loaded by the cycle's own `capture_id`), is idempotent once discarded, and after it runs `stuck_captured_cycle` returns no warning **even if the archive no longer exists**.

**Test.** `reconcile_compacted_committed_capture_discards_and_survives_archive_gc` proves the discard, idempotency, and — the key durability property — that after `std::fs::remove_file(archive)` the capture still does not flag stuck. Spec updated in `07-closeout-commands.md`.

### Re: do [#codex-final-response-live-verify] — opus-4-8

> **Queue prompt:**
>
> do [#codex-final-response-live-verify]


Gated — the code side is complete and verified; the remaining work is a real Codex same-pane run that I (Claude Code) can't perform from here.

**Code-complete + verified deterministically.** The `#codex-final-response-not-written` guard lives in `session_check.rs::check_committed_without_response_body_guard` and is wired into `inspect`. I ran its regressions: `committed_without_response_body_guard_fires_on_pending_mutations_no_capture`, `..._passes_with_captured_response`, `..._is_wired_into_inspect` and two siblings all pass (5/5). So a Codex turn that commits side-effect work but writes no `### Re:` close-out fails closed with "no assistant response body was captured" + the `agent-doc write --commit` recovery path, and a normal Codex turn with a response stays OK.

**Why gated, not done.** This item is explicitly *opportunistic live re-verification* via the `.codex` Stop hook in a real Codex session. That requires driving an actual Codex same-pane run, which isn't reproducible from this Claude Code session. Leaving it gated pending that live run.

**Bonus — fixed a real stuck-capture false positive found while running this cycle.** Preflight flagged the *previous* committed cycle as `stuck_captured_cycle` even though its response is in HEAD. Root cause: queue consumption inserts a `> **Queue prompt:**` echo blockquote between the response heading and body, which broke the contiguous-block match in `response_already_applied`, so `stuck_captured_cycle` falsely warned after every queue-consumed cycle (and re-suggested a `write --commit` that would re-inject the response). Fixed by stripping the echo blockquote in `normalized_response_lines` before matching (submodule `6220379c`, with unit + closeout regressions); queued `#stuck-capture-queue-echo-false-positive` to confirm across a few more cycles. This finalize's own preflight ran on the fixed binary.

### Re: do [#codex-route-busy-ctrl-g-opens-editor] — opus-4-8

> **Queue prompt:**
>
> do [#codex-route-busy-ctrl-g-opens-editor]


Done — shipped and live-verified (submodule `9be1c0fc`).

**Root cause.** `attempt_busy_existing_pane_interrupt_recovery` (route.rs) sent `C-g` *unconditionally* to a busy Codex pane before the Escape+C-c fallback. `C-g` only aborts a shell `reverse-i-search`/history-search; in the normal Codex composer or an active turn it opens the external editor (`$EDITOR`/nvim) — the same root cause as `#codex-interrupt-clear-ctrl-g-opens-editor` in `send_operator_interrupt_sequence`.

**Fix.** The reroute now sends `C-g` only when the pane is in a shell-search state: fast-path on the authoritative `blocker_reason` from the readiness wait, else a fresh **whole-capture** re-classification via `dispatch_only_blocker_reason`. Two subtleties the fix handles (both found via the live test): the readiness wait can return a timeout (`blocker_reason=None`) while the pane is genuinely in reverse-i-search, and the search line can sit *above trailing blank pane rows*, out of the last-few-lines window the normal `dispatch_blocker_reason` inspects — so whole-capture matching is required. Any other busy state goes straight to Escape+C-c and logs `route_busy_existing_pane_interrupt_skipped_ctrl_g`.

**Tests.** Added the deterministic `codex_busy_ctrl_g_gate_only_fires_for_shell_search_blocker` decision test — it covers the non-search composer / active-turn case the existing live reverse-i-search test (`..._recovers_on_ctrl_g`) never modeled, which is exactly why the regression slipped through. `make check` green; FlowCore `reason=` budget bumped +1 with an audit note for the skip diagnostic.

**Live tmux verification.** Ran the 9 `#[ignore]` tmux busy/interrupt tests with a clean env (`make tmux-ci` equivalent) — all pass, including the reverse-i-search `C-g` recovery (proves the gate still sends `C-g` in the real shell-search case) and the Escape+C-c / fresh-restart paths.

**Spec/docs.** Documented the route-side gating in `specs/07-session-tmux-commands.md` and the `AGENTS.md` convention bullet (`route.rs` now listed alongside `session_actor_cmd.rs`).

### Re: do [#cross-project-stash-pane-condition] — opus-4-8

> **Queue prompt:**
>
> do [#cross-project-stash-pane-condition]


Done — the design decision this item was gated on was already made and implemented (2026-05-30); I verified it's live in the current tree. No new code needed; the backlog text was stale.

**The design decision (resolved to C).** Per `tasks/agent-doc/plan-cross-project-stash-pane-condition.md`, nested project roots (superproject `agent-loop` + submodule `src/boost-client`) auto-detect the *same* tmux session 0 and share one stash window, so superproject sync stashes the submodule's live pane and resync re-discovers it cross-project. Options weighed:
- **A — nested-root session isolation:** rejected. Per-root sessions fragment the single-session workflow (you'd `prefix+s` to reach a submodule pane; IDE focus would have to follow).
- **B — per-root stash partitioning:** rejected as unnecessary/mildly harmful — hiding a non-focused foreign-root pane in the shared stash is *correct* layout behavior.
- **D — reap stale orphaned stash panes:** already implemented (resync.rs step 8); the observed `agent-loop` panes survive because they're legitimately live/registered, not orphans.
- **C — implemented:** the preservation event is *correct* (the pane must not be killed); the only real defect was emitting it on stderr, which the JB plugin renders as an error.

**Verified in the current tree.** All three resync sweep sites (windowed `resync.rs:~775`, bulk `~1313`, non-stash `~1808`) route `registered in its own project root — skipping kill` through `crate::ops_log::log_op(&current_root, …)` — not `eprintln!` — with comments documenting why. Spec note present at `specs/08-session-routing.md:130` ("Cross-project stash preservation is telemetry, not an error").

**Residual.** The broader "JB plugin renders *any* route/resync stderr as an error" concern is already tracked under `#jb-busy-queue-ux-feedback` (still queued), so no new backlog item is needed here.

(Closeout note: the prior finalize attempt hit an IDE IPC ack timeout — exit 75 — and deferred the visible write; recovered through `--force-disk` after clearing the stale live-buffer tracking entry.)

### Re: do [#route-drain-all-surface-live-verify] — opus-4-8

> **Queue prompt:**
>
> do [#route-drain-all-surface-live-verify]


Gated — code-complete with passing deterministic coverage; the only remaining work is a live JetBrains IDE action I can't drive from this Claude Code session.

**Implementation verified present.**
- The all-surface reap (`b239988b`) is live: `drain_open_closeout_before_routed_dispatch` (route.rs:1693) now runs the idempotent `crate::preflight::run_pending_maintenance` (backlog + review + icebox), so a completed `[x]` item left in review/icebox by a prior closeout no longer keeps the post-repair `session-check` interrupted and bails the route. Its deterministic regression `drain_reaps_completed_review_item_across_all_surfaces` passes.
- The `#pending-gate-snapshot-desync` snapshot re-sync (`ab074710`) is present in `preflight.rs` (re-syncs the snapshot to tracked surfaces on closeout) — the second behavior this item wants to exercise under the route path.

**Why gated, not done.** The item is explicitly *opportunistic live verification*: with the installed binary, stage a closeout that leaves a completed `[x]` item in review/icebox on an IDE-owned doc, then click JB **Run Agent Doc** once and confirm it dispatches without a manual repeat. That requires driving the JetBrains action in a live IDE, which isn't reproducible from here. The deterministic SimWorld coverage stands in for the logic; the live leg stays gated.

(Closeout note: used `--force-disk` proactively — the IDE IPC listener has been ack-timing-out this session, which caused an exit-75 desync on the previous item.)

### Re: do [#run-agent-doc-stale-busy-replay] — opus-4-8

> **Queue prompt:**
>
> do [#run-agent-doc-stale-busy-replay]


Done — added the deterministic replay regression this item asked for (submodule `cbf45dde`, `make check` green).

**What was missing.** The `#snrun` stale-busy repair (`busy_projection_repaired_by_ready_prompt`) was covered only by a pure-predicate unit test. There was no end-to-end replay asserting "stale busy projection + proven ready prompt → **dispatch**, not `agent:queue auto` enqueue."

**Added — SimWorld replay (deterministic, runs in `make check`).** Per `specs/12-deterministic-simulation.md`, SimWorld is the deterministic in-memory route/controller actor model (no live tmux). New command `RepairBusyProjectionWithReadyPrompt` + handler `repair_busy_projection_with_ready_prompt`, gated by the **production** predicate `busy_projection_repaired_by_ready_prompt` (not a reimplementation), plus a `busy_projection_ready_repairs` coverage counter wired into the random corpus schedule. Two scenario tests:
- `route_sim_repairs_stale_busy_projection_with_ready_prompt_then_dispatches`: SupervisorBusy → dispatch fails closed (`busy_dispatch_blocks=1`, 0 acceptances) → repair via ready prompt → dispatch accepted + proven (the exact "dispatch instead of enqueue" assertion).
- `route_sim_stale_busy_repair_requires_busy_lifecycle`: the repair is fail-closed — it does not fire unless the actor is genuinely projected Busy.

**Residual (tracked, not dropped).** The second thread in this item — distinguishing *rescued-stash accepted-only proof misses* — is already tracked under the `#snrun` review item (implement `plan-run-agent-doc-snappy-auto-remediation.md`: rescued-stash accepted-only misses + active-turn busy collisions + stale starting projections), so no new backlog item is needed.

(Closeout note: `--force-disk` again — the IDE IPC listener remains unresponsive this session.)

### Re: do [#jb-busy-queue-ux-feedback] — opus-4-8

> **Queue prompt:**
>
> do [#jb-busy-queue-ux-feedback]


Done — both concerns are addressed (the bulk by prior fixes), strengthened with a regression; submodule `ad2241f1`, JB test suite green.

**Concern 1 — queued-behind-a-busy-turn must read as informational, not an error.** Already implemented: `TerminalUtil.classifyRunAgentDocRouteFailure` maps the binary's `queued pending dispatch … in agent:queue auto` output (emitted at all three route busy-queue sites — dispatch-only busy, active-closeout drain, authoritative-actor busy) to `QUEUED_PENDING` → `notifyRunAgentDocQueued` with `NotificationType.WARNING` and the message "queued behind the active turn and should run when that turn drains." That's distinct from the `NotificationType.ERROR` used for real route failures (`notifyPersistentRouteFailure`). Covered by `queued route success is reported as queued not silent success`; the parallel busy-running case (`BUSY_RUNNING` → WARNING) is covered too.

**Concern 2 — benign cross-project resync info must not read as an error.** Resolved by `#cross-project-stash-pane-condition` (done earlier this session): the `registered in its own project root — skipping kill` preservation event was moved from stderr to `ops_log` at all three resync sweep sites, so it no longer reaches the plugin's captured route output / error path at all.

**Added this cycle.** A regression for the one untested shape — a queued-behind-busy route that *co-emits* the benign resync preservation line must still classify `QUEUED_PENDING` (the queued line keeps it informational; the benign line must not tip it to `PERSISTENT`/error). Test-only; the classification logic was already correct, so no plugin rebuild/version bump. The JB `TerminalUtilTest` suite passes.

The remaining live-IDE rendering (WARNING balloon vs error) is a thin display of this deterministically-tested classification.

### Re: do [#queue-prompt-echo-summary] — opus-4-8

> **Queue prompt:**
>
> do [#queue-prompt-echo-summary]


Done — implemented as **opt-in** to honor the recorded deferral; submodule `362ad175`, `make check` green.

**Respecting the deferral.** This item carried an explicit user decision: *"for now, let's try copying the entire prompt."* So summarization is NOT forced on. The default behavior is unchanged (verbatim copy); the capability is gated behind a new frontmatter flag.

**What landed.** New frontmatter `agent_doc_queue_prompt_echo_max_chars: <n>` (default `None` = verbatim). When set and a consumed queue prompt exceeds `n` characters, the `#queue-prompt-echo-in-response` echo records a bounded summary — the prompt's first line truncated to `n` chars (on a char boundary) + the count of elided characters + a pointer noting the full prompt is retained in `agent:queue` — instead of the full verbatim text.

**Where.**
- `frontmatter.rs`: `queue_prompt_echo_max_chars: Option<usize>` (skipped when None).
- `write.rs`: `format_consumed_prompt_echo` takes the threshold; new `summarize_consumed_prompt`; the threshold is read from the document's own frontmatter inside `embed_consumed_prompt_in_response`, and the dedup/idempotency check uses the **same** rendered echo so a summarized echo is still recognized as already-embedded on re-runs.
- 4 unit tests: verbatim when None, verbatim when under threshold, summary when over threshold, and char-boundary-safe truncation for multibyte prompts.
- Spec updated in `07-orchestration-commands.md`.

This is why this very turn's echo (and every queue-consumed cycle's) still copies the prompt verbatim — the default is preserved; enabling summarization is now a one-line frontmatter opt-in when the user is ready.

### Re: do [#free-text-queue-owner-consume] — opus-4-8

> **Queue prompt:**
>
> do [#free-text-queue-owner-consume]


Done — fixed the root cause that hung free-text queue heads; submodule `efe79b1b`, `make check` green.

**Root cause (not what the plan first hypothesized).** The finalize/repair path already had a `#free-text-queue-head-consume` rule: a free-text head (no `#id`) completes when answered, via the captured response body. But `queue_head_is_free_text_prompt` classified a head as id-backed using `queue_prompt_done_id(..).is_some()`, which matches **any** `#id` *mention*. The live repro head `Approve [#shoptiers]. What are #next-steps?` mentions two ids, so it was wrongly treated as id-backed — and since it has no single id to `--done`, neither the free-text path nor an explicit-flag path could ever strike it. The auto-queue hung ("skipped consumption because the active prompt did not target the queue head").

**Fix.** A head is id-backed only when its *entire* text resolves to a single id directive (`#id` / `[#id]` / `do [#id]`) via `topic_resolves_to_exact_id`, or to a `#preset` / queue-trigger. A free-text head that merely mentions `#id`s in prose stays free text and completes on being answered. `do [#id]` / `#preset` heads are unchanged — `#queue-strike-on-halt` safety preserved. Two regression cases added (id-mentioning prose head; single-`#id` prose head); the existing free-text/preset/halt assertions still pass. Spec updated.

**Combined with this session's `#codex-owned-pane-prompt-miss` early guard** (owner-pane recursion now fails closed cleanly before pre-commit with recovery), the full owner-pane-recursion + free-text-consume repro is resolved.

**Follow-up captured.** The plan's point 4 (a shape-aware skip diagnostic instead of the generic "did not target" line) is queued as `#free-text-queue-skip-diagnostic` — it's polish, separable from this consume-correctness fix.

### Re: do [#plugin-cleanup-menu-command] — opus-4-8

> **Queue prompt:**
>
> do [#plugin-cleanup-menu-command]


Gated — implemented in both editors and built; the only remaining step is live install + menu-click verification I can't drive from here. Submodule `043de701`.

**Both editors, thin event-reporter pattern (the CLI owns all cleanup logic):**
- **Resync / Fix Sessions** → `agent-doc resync --fix`
- **GC Stale Sessions** → `agent-doc gc`

Both are **project-scoped** (the registry/pane cleanup is project-wide, not tied to one document): they run in the focused `.md` file's project root when one is open, else the IDE workspace root, and report the outcome as an Event-Log / output-channel notification.

**JetBrains (plugin `0.2.141`).** `TerminalUtil.runProjectCleanupCommand` + `cleanupProjectRoot`; new `ResyncFixSessionsAction` / `GcStaleSessionsAction` (enabled whenever a project is open); `plugin.xml` Tools/Editor/ProjectView menu entries + added to the Alt+Enter popup overflow group; popup-action-id test updated. `pluginVersion` bumped `0.2.140 → 0.2.141`; `buildPlugin` + `signPlugin` ran (both zips produced), JB test suite green.

**VS Code.** `resyncFixSessionsAction` / `gcStaleSessionsAction` + `runProjectCleanupCommand` / `resolveCleanupCwd`; registered `agentDoc.resyncFixSessions` / `agentDoc.gcStaleSessions`; `package.json` command titles. `tsc` compile clean; 70/70 extension tests pass.

**Why gated.** Per the JB manual-testing gate, the new plugin must be installed-from-disk (signed `0.2.141` zip) and the menu items clicked in a live IDE to confirm they appear and dispatch — that live step is the remaining work. Pairs with the still-queued `#stash-session-ttl-prune` (auto hygiene) and the done `#jb-busy-queue-ux-feedback` (notification classification).

(Closeout note: `--force-disk` — the IDE IPC listener remains unresponsive this session.)

### Re: do [#stash-session-ttl-prune] — opus-4-8

> **Queue prompt:**
>
> do [#stash-session-ttl-prune]


Gated — landed the safe, non-destructive decision core; the destructive parts are deliberately deferred per the item's own "opt-in + live-verified before enabling" requirement. Submodule `f5ac089c`; plan at `tasks/agent-doc/plan-stash-session-ttl-prune.md`.

**Why not ship the whole thing.** This feature *kills live, registered, supervisor-backed* panes that are merely idle — destructive. And it can't use existing data: the registry's `started` is *start time*, not last-activity, so reaping on it would kill long-running **active** sessions. A correct TTL prune needs a new idle signal (tmux `#{window_activity}`), a config knob, and live verification on a real layout — exactly what the item says to gate. Shipping unverified destructive pane-killing in an autonomous loop is the risk the user's caution is guarding against.

**Landed this cycle (safe, default-inert).** `stash_ttl_prune_candidate` / `stash_ttl_prune_targets` + `StashTtlCandidate` — the reusable pure decision with conservative guards: TTL must be enabled (`0`/unset → reaps nothing), only an agent-doc stash-window pane is eligible, the active/visible pane is **never** reaped, and idle must strictly exceed the TTL. 5 unit tests (disabled-inert, active-never-reaped, agent-doc-only, strict-`>`, multi-candidate filter). `make check` green; binaries built + installed.

**Gated (design + destructive wiring + live verify).** Per the plan: (1) add the `#{window_activity}` idle query; (2) `.agent-doc/config.toml` `stash_session_ttl_secs` (project-level, default unset); (3) **report-only first** — log candidates without killing so the operator confirms detection on the real ~9-pane stash; (4) destructive enable behind a separate explicit flag, default off; (5) live verification + SimWorld/tmux-ci replay. Existing orphan-reap semantics unchanged; registered live sessions stay protected by default. Relates to `#cross-project-stash-pane-condition` (done) and `#jb-busy-queue-ux-feedback` (done).

(Closeout note: `--force-disk` — the IDE IPC listener remains unresponsive this session.)

### Re: do [#next-steps-backlog-capture] — opus-4-8

> **Queue prompt:**
>
> do [#next-steps-backlog-capture]


Done — resolved by existing, tested enforcement; the item's framing is stale. No new code needed (verified in the current tree).

**The concern.** A `#next-steps` response that recommends follow-ups but doesn't add them to the backlog should not pass closeout silently — and the item noted `pending_mutations: expect_add` wasn't enforced by `pending_done_guard`.

**What's actually in place (and tested).**
- `prompt_contract::prompt_requests_backlog_work` detects `#next-steps` (and any backlog-requesting prompt — "place in the backlog", "follow-up backlog", etc.) and preflight records `backlog_capture_required` on the cycle (`record_backlog_capture_requirement`).
- Enforcement runs at **both** binary-owned boundaries — `write.rs` precommit and `session_check::inspect` — failing closed with "committed response came from a prompt that **required backlog capture** … but this turn added no backlog item." The escape hatch is an explicit "no actionable follow-up items remained" statement.
- Tests pass: `precommit_blocks_backlog_required_review_without_pending_add`, `session_check_blocks_backlog_required_review_without_pending_add`, their `*_with_explicit_no_followups` counterparts, plus the `#next-steps`-specific preflight contract tests (`preflight_abandoned_stale_next_steps_prompt_stays_actionable`, `preflight_compact_follow_up_next_steps_is_not_swallowed_by_commit_recovery`).

**Why the item's framing is stale.** Enforcement was added as a **dedicated `backlog_capture_required` guard** (not folded into `pending_done_guard`, which is the right separation — backlog *capture* vs. `do [#id]` *resolution*). And "auto-add" isn't deterministically possible — the binary can't synthesize which recommendations from free-form prose become backlog items — so requiring the agent to add them (and failing closed otherwise) is the correct design, which is exactly what ships.

(Closeout note: `--force-disk` — IDE IPC listener still unresponsive this session.)

### Re: do [#ah0s] — opus-4-8

> **Queue prompt:**
>
> do [#ah0s]


Done — explicit-position `--pending-add` primitives shipped and fully tested; submodule `26f28d6f`, `make check` green.

**The footgun.** Ordering a multi-item backlog batch depended on implicit prepend-vs-argorder semantics; two agents reversed args expecting a flip and got reverse-of-intended order. The backlog is a priority-ordered **pool** with id-based consumption (not a stack/queue — FIFO lives in `agent:queue`), so position is author intent and should be set explicitly.

**New flags** (also `pending_cmd` helpers):
- `--pending-add-after <id> "text"` — repeatable `ID TEXT`; chaining `after A "B" --after B "C"` builds A→B→C deterministically (no reorder pass).
- `--pending-add-before <id> "text"` — symmetric.
- `--pending-add-back "text"` (alias `--pending-append`) — tail insert for low-priority captures.

`--pending-add` stays the cheap front-insert default.

**Implementation.** Core `pending::op_add_at` + `AddPosition` enum + `PendingLayout` positional inserts (`item_segment_index` / `last_item_index_plus_one` / `insert_item_at`); `op_add` delegates to `First`. Wired through `CommandOptions` + the write/finalize apply path + arg reconstruction + clap (`write` and `finalize`). After/before error on unknown anchor; explicit-position adds run *after* the front-insert default so an anchor added the same cycle resolves.

**Tests.** 5 core `op_add_at` cases (after/before/last/chain/unknown-anchor) + a CLI integration test (`write_pending_add_after_and_back_position_items_explicitly`). **Docs:** `specs/pending-system.md` (priority-pool model + flag table) and `runbooks/pending-ops.md` (prefer explicit-position over prepend+reorder); clap `--help` carries the flag docs. (Installed harness runbook copies regenerate on the next versioned `skill install`; the bundled source + spec + `--help` are the normative surfaces.)

(Closeout note: `--force-disk` — IDE IPC listener still unresponsive this session.)

### Re: do [#backlog-priority-attribute] — opus-4-8

> **Queue prompt:**
>
> do [#backlog-priority-attribute]


Gated on a design decision — plan written at `tasks/agent-doc/plan-backlog-priority-attribute.md`. This is a daily-UX feature whose core (marker syntax) is undecided and which tensions with the model I just shipped, so I'm not picking the syntax unilaterally.

**Why gated, not implemented.**
1. **Tension with `#ah0s` (just shipped).** `#ah0s` established the backlog as a priority *pool* where **position encodes author intent**, set explicitly via `--pending-add-after/-before/-back`. Auto-sorting by a priority attribute partially overrides author-set position, so it must be **opt-in per component** to compose rather than fight — a design choice worth confirming.
2. **Marker syntax is a real UX decision.** `PendingItem` has no priority field today; `[recommended]` is free text. Candidates — per-item `[p1]..[p9]`, a `priority=N` prefix token (mirroring `id=<custom>`), or tiering `[recommended]` — each change how items render and coexist. No obvious winner.
3. **Sort trigger/stability.** Every write vs. an explicit `--pending-reorder --by-priority` vs. preflight maintenance; must be stable (ties keep author order) and preserve gated/done partitioning.

**Recommended design (in the plan, for confirmation).** Opt-in `&lt;!-- agent:backlog sort=priority --&gt;` component attribute (absent ⇒ today's manual order, honoring `#ah0s`); per-item `priority=<1..9>` token in the canonical prefix grammar; a pure `op_sort_by_priority` stable sort applied by `run_pending_maintenance` each cycle when the attribute is present. Pairs with the next queued item `#backlog-queue-sync-attribute` (regenerate `agent:queue` from the prioritized backlog).

No code shipped this cycle — the gating decision is the marker syntax + opt-in model. Once confirmed, implementation is bounded (a pure sort fn + parse token + the opt-in attribute + maintenance wiring + tests).

(Closeout note: `--force-disk` — IDE IPC listener still unresponsive this session.)
<!-- agent:boundary:17552977 -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#codex-owned-pane-prompt-miss]~
- ~do [#stuck-capture-compact-false-positive]~
- ~do [#codex-final-response-live-verify]~
- ~do [#codex-route-busy-ctrl-g-opens-editor]~
- ~do [#cross-project-stash-pane-condition]~
- ~do [#route-drain-all-surface-live-verify]~
- ~do [#run-agent-doc-stale-busy-replay]~
- ~do [#jb-busy-queue-ux-feedback]~
- ~do [#queue-prompt-echo-summary]~
- ~do [#free-text-queue-owner-consume]~
- ~do [#plugin-cleanup-menu-command]~
- ~do [#stash-session-ttl-prune]~
- ~do [#next-steps-backlog-capture]~
- ~do [#ah0s]~
- ~do [#backlog-priority-attribute]~
- do [#backlog-queue-sync-attribute]
- do [#preset-item-id-collision]
<!-- /agent:queue -->
###
<!--
Verify [#ipcfullprompt-recur2]
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
The completed queue item was not removed from the queue. #mermaid
#spec-test-build-install-commit-push
---
equityfundingsource.md had the full-document IPC duplicate corruption. Then a new full-document IPC deletion under the prompt corruption happened. Both several times while editing the prompt.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#free-text-queue-skip-diagnostic] [recommended] Polish from #free-text-queue-owner-consume plan point 4: when queue consumption is skipped, the closeout still prints the generic '[queue] skipped consumption because the active prompt did not target the queue head'. For an id-backed head with no completion flag, name the missing proof source and the supported repair (--done/--pending-gate/--pending-edit <id>); for a free-text head edge case, point to the answered-response path. Replace the single generic line at write.rs ~1460/1469 with a shape-aware diagnostic. Plan: tasks/agent-doc/plan-free-text-queue-owner-consume.md (point 4).
- [ ] [#stuck-capture-queue-echo-false-positive] [recommended] Fix landed (submodule 6220379c): response_already_applied matched the captured response as one contiguous block, but queue consumption inserts a > **Queue prompt:** echo blockquote (#queue-prompt-echo-in-response) between the response heading and body, so stuck_captured_cycle falsely warned committed-but-not-in-HEAD after every queue-consumed cycle and re-suggested a write --commit that would re-inject the response. normalized_response_lines now strips the echo blockquote on both sides before matching; unit + closeout regressions added. Verify across a couple more queue-consumed cycles that no preflight emits the false stuck_captured_cycle warning, then mark done.
- [ ] [#codex-owned-pane-prompt-miss-followups] [recommended] Follow-ups deferred from the #codex-owned-pane-prompt-miss fix (fail-closed early guard + session-check backstop landed, submodule dede28d8): (1) the plan's *preferable* path — emit a structured owned_pane_self_invocation result shape in preflight/plan (file path, owner pane/session/actor generation, unresolved prompt excerpt, required persistence command) so Codex guidance drives an in-pane response cycle instead of only reading the bail diagnostic; (2) Codex Stop-hook / installed guidance regression asserting the owned-pane diagnostic forces an in-pane response body before final chat output; (3) live-verify with a real Codex same-pane run that has an unresolved exchange prompt. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [ ] [#codex-owned-pane-auto-queue-stuck] [recommended] Codex-owned `agent-doc <FILE>` direct invocation from the same pane can leave an active auto-queue turn stuck: the recursive guard avoids child dispatch, but the ready queue head is not handed to the owning Codex turn and no response is written. Live repro 2026-05-31 `src/boost-client/tasks/monsterrodholders.md`: `queue_active: true` + `agent:queue auto` head `re [#374n]...`; same-pane run printed `Submitting to codex...` then `recursive direct invocation would deadlock`, leaving queue/boundary drift and the head unprocessed. Fix with early owner-pane queue handoff, durable queue-target proof, no-drift guard, and Codex final-gate recovery. Plan: tasks/agent-doc/plan-codex-owned-pane-auto-queue-stuck.md.
- [ ] [#completed-queue-residue-regression] [recommended] Completed queue items can still remain visible in agent:queue after the turn is finished, despite the archived #drained-done-queue-clear fix. Treat as a regression/uncovered route or IPC path: add a committed-state guard for provable completed queue residue, audit finalize/write/preflight/run continuation drain ordering, and verify live editor/snapshot/HEAD convergence. Plan: tasks/agent-doc/plan-completed-queue-residue-regression.md.
- [ ] [#do-id-partial-closeout-state] [recommended] Partial-closeout WARN guard landed (check_partial_closeout_state_guard: a do[#id] turn whose response shows shipped + remaining-live-work signals with the directed id still open advises --pending-edit/--pending-gate narrowing; suppressible by a no-partial-closeout-guard response marker; FlowCore guard_ budget 35->40). Remaining: per-id edit-vs-gate tracking in cycle_state, opt-in strict mode, next-phase queue convergence, and the runbook note that #spec-test-commit-push completes a repo phase without a live/deploy item. Plan: tasks/agent-doc/plan-do-id-partial-closeout-state.md.
- [ ] [#closeout-recovery-state-machine] [recommended] Typed closeout recovery classifier landed: flow::closeout::CloseoutRecoveryState + classify_closeout_recovery_state + recovery_command; session_check::closeout_recovery_hint renders one typed 'Recovery [state]: command' across all nine closeout-guard call sites. Classified today: clean / open_cycle / missing_response_body / escaped_template_patch. Remaining: precise detection for reserved states direct_response_patchback, boundary_only_drift, nested_parent_pointer_stale, and folding each guard's existing diagnostic through the classifier. Plan: tasks/agent-doc/plan-closeout-repair-churn.md.
- [ ] [#preset-item-id-collision] [recommended] Enforce unique active identities across frontmatter `prompt_presets` keys and active `agent:backlog` / `agent:review` / `agent:icebox` item ids. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: `#next-steps` exists both as a prompt preset and as a backlog item, making `do #next-steps` and "top backlog item: #next-steps" ambiguous between preset expansion and backlog execution. Fix with an identity registry, mutation-time rejection, preflight/session-check fail-closed diagnostics, and regressions. Plan: tasks/agent-doc/plan-preset-item-id-collision.md.
- [ ] [#backlog-queue-sync-attribute] [recommended] Add a queue=sync|append|prepend attribute to agent:backlog / agent:icebox: queue=sync replaces agent:queue content with a do [#id] list for every item; queue=append adds item ids not already in agent:queue; queue=prepend prepends them. Composable with #backlog-priority-attribute (prioritize first, then regenerate the queue), automating the manual 'regenerate the queue to the new priority' step that currently has to be done by hand after a reorder. Requested 2026-05-30.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#backlog-priority-attribute] [recommended] Add a priority attribute to agent:backlog / agent:icebox / agent:queue that orders the item list by priority (a component-level priority flag and/or per-item priority markers), so the binary maintains priority order instead of requiring manual --pending-reorder each cycle. Automates the by-hand reprioritization done in #next-steps cycles. Composable with #backlog-queue-sync-attribute; sibling of #ah0s (explicit-position primitives). Requested 2026-05-30.
- [/] [#stash-session-ttl-prune] [recommended] Optional opt-in hygiene to cap stash-session accumulation: reap agent-doc stash panes whose session has been idle in the stash beyond a configurable TTL. Distinct from the existing orphan reap (resync.rs step 8), which only kills unregistered + no-supervisor + no-owned-file panes; live registered/supervisor-backed sessions are correctly never auto-killed, which is why ~9 agent-loop agent-doc panes accumulate in the shared session-0 stash. Must be opt-in (frontmatter/.agent-doc config) + live-verified before enabling (destructive). Relates to #cross-project-stash-pane-condition and #jb-busy-queue-ux-feedback.
- [/] [#plugin-cleanup-menu-command] [recommended] Expose session cleanup as agent-doc plugin-menu commands (JetBrains + VS Code): a 'Resync / Fix Sessions' action wrapping agent-doc resync --fix and a 'GC Stale Sessions' action wrapping agent-doc gc, so operators can clear stale routing/registry state and reap orphaned panes from the IDE without dropping to a terminal. Thin event-reporter pattern — the plugin shells the existing CLI/FFI; no cleanup logic in the plugin. Pairs with #stash-session-ttl-prune (auto hygiene) and #jb-busy-queue-ux-feedback (notification classification).
- [/] [#route-drain-all-surface-live-verify] [recommended] Live-verify the route-drain all-surface reap (submodule b239988b / superproject 4c7b06b7f) resolves the JB Run Agent Doc "completed tracked item blocks dispatch" retry: with the installed binary, reproduce a closeout that leaves a completed [x] item in review or icebox on an IDE-owned doc, then run JB Run Agent Doc once and confirm it dispatches without a manual repeat. Also exercises the #pending-gate-snapshot-desync snapshot re-sync (ab074710) under the route path.
- [/] [#codex-final-response-live-verify] [recommended] Opportunistic live re-verification of the #codex-final-response-not-written guard: do a real Codex direct/manual run that commits side-effect work, confirm session-check (via the .codex Stop hook) fails closed with the 'no assistant response body was captured' message + 'agent-doc write --commit' recovery when no ### Re: close-out was written, and that a normal Codex turn with a response stays OK. Guard + deterministic regressions already landed (session_check.rs check_committed_without_response_body_guard).
- [/] [#codex-owned-pane-prompt-miss] [recommended] Codex-owned `agent-doc <FILE>` direct invocation from the same pane can abandon recursive dispatch without processing unresolved exchange prompts. Live repro 2026-05-30: `src/boost-client/tasks/monsterrodholders.md` contained `Please assist in placing GA4 Analytics credentials in passage.`, but the same-pane recursion guard refused dispatch and no response was written. Fix with earlier owner-pane self-invocation detection, unresolved-prompt preservation, and Codex guidance/final-gate recovery. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#stash-pane-promote-on-focus] [recommended] A live-owner pane residing in the stash tmux window must be promoted/reparented into the 0:agent-doc window when the document is focused/selected, not merely selected in place. Live repro 2026-05-30 agent-doc-bugs2.md: pane %8 (live owner) is stuck in the stash window; focus/select does not move it. The focus live-owner fix (submodule e6dafa52) now selects the correct pane but does not reparent it. Tied to #cross-project-stash-pane-condition and #plugin-cleanup-menu-command.
- [/] [#clear-stops-running-process] [recommended] JB Clear Session Context / Claude Code /clear must stop the running agent-doc child so a following JB Run Agent Doc is not blocked by a stale busy actor. Live repro 2026-05-30 agent-doc-bugs2.md: after /clear, a still-running process blocked Run Agent Doc. Overlaps #cancel-orphans-preflight-cycle and the operator_clear busy-projection path in session_actor_cmd.rs.
- [/] [#jb-run-agent-doc-response-queue-contamination] [recommended] JetBrains `Run Agent Doc` can add assistant response prose to `agent:queue auto` after compact/repair drift. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: queue gained `- Yes. I drove the already-authenticated Google Ads browser session...`, copied from `### Re: #gads106demote`, alongside real `do [#nbsearch]`/`#bidstrat`/`#pdpconv` entries. Require queue-source provenance and reject assistant response bodies as queue candidates. Plan: tasks/agent-doc/plan-jb-run-agent-doc-response-queue-contamination.md.
- [/] [#jb-compact-repair-left-uncommitted] [recommended] Compact-output repair can leave the corrected session document dirty instead of committing through a binary-owned repair boundary. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: visible exchange correction differs from malformed `HEAD`, creating mixed visible/HEAD/snapshot state for later JetBrains actions. Repair should commit atomically or fail closed with an exact recovery path. Plan: tasks/agent-doc/plan-jb-compact-repair-left-uncommitted.md.
- [/] [#jb-compact-malformed-response-commit] [recommended] JetBrains `Compact Exchange` can commit malformed compact output. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md` / commit `f8fe36b`: compact summary line was committed with user prompt prefix `❯`, and archived response content remained in the live exchange shape. Add post-compact structural validation and fail closed before commit. Plan: tasks/agent-doc/plan-jb-compact-malformed-response-commit.md.
- [/] [#prompt-preempts-auto-queue] [recommended] Unanswered exchange prompts must run before active `agent:queue auto` continuation, even when the prompt was accidentally baselined into the `.agent-doc` snapshot. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: `What are #next-steps to complete review items?` was visible after the exchange boundary and duplicated in `agent:queue auto`, but the snapshot already contained the exchange prompt, so the next diff only saw queue bookkeeping and the prompt did not run. Fix: add snapshot-independent unresolved exchange prompt detection, make preflight/direct-run prefer it over queue synthetic diffs, prevent route queue snapshot recovery from absorbing unanswered exchange prompts, and defer Codex/Claude queue continuation while such a prompt exists. Plan: tasks/agent-doc/plan-prompt-preempts-auto-queue.md.
- [/] [#queue-user-edit-overwrite] [recommended] User-authored agent:queue edits can be silently overwritten/deleted by write/reset/commit queue convergence. Live repro 2026-05-30 boost-client monsterrodholders.md: user added do [#gscaccess] at queue head; agent-doc repeatedly removed/reverted it during closeout/snapshot repair. Fix by tracking queue edit provenance/epochs, preserving explicit queue edits unless the current response consumes that exact head, and failing closed on stale IPC/snapshot queue rewrites. Plan: tasks/agent-doc/plan-user-queue-edit-preservation.md.
- [/] [#ipcfullprompt-recur2] CURE ONLY (capture/detection split out to #ipcfullprompt-capture, done). Editor-side PatchWatcher.setText PREVENTION: stop the live editor buffer from producing full-document/full-tail duplication or deletion while a prompt is being typed. Gated on the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging) to pin the exact setText call shape. Add a SimWorld/Kotlin regression for the prevention once the fix lands; bump pluginVersion past 0.2.137 and reinstall the signed plugin. Stays open until prevention lands and is live-verified. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [/] [#finalize-retry-ipc-response-duplication] [recommended] Multi-retry finalize + live IPC duplicates the response and prefixes its body lines with the user-prompt marker. Live repro 2026-05-29 on agent-doc-bugs2.md: a closeout needing several finalize/write retries (lint-gate / session-check / live_prompt_drift re-runs) with the IDE IPC listener active and concurrent editing left a duplicated response block plus agent body lines wrongly prefixed (uncommitted; HEAD stayed clean; manual git checkout repaired). Fix direction: idempotent retry convergence on IPC already_applied, never prefix agent response body as a prompt, and repair/session-check auto-dedup. Relates to #prompt-duplicated-while-typing, #duplicate-response-after-commit, #ipc-dup-tracking-double-merge (plugin 0.2.138 not yet installed during repro). Plan: tasks/agent-doc/plan-finalize-retry-ipc-response-duplication.md.
- [/] [#ipc-dup-tracking-double-merge] PromptPoller double-tracking fix (plugin 0.2.138, submodule 32fb0b98 / superproject d585a33de): addFile keyed trackedFiles by project-relative path while handlePollResults keyed by "$cwd:$file", double-tracking the same VirtualFile (live: corky.md, brandon-cinquegrana.md, agent-doc-bugs2.md; trackedFiles=13 vs ffiTracked=9). Two divergent baselines -> mergeOrReload ran twice per change = full-document duplication path. Fixed by keying both via trackingKey(VirtualFile.path); convergence test + AGENTS.md invariant. GATED on live verify: install signed 0.2.138, restart, confirm no recurring duplication (corky.md). Exact setText payload uncaptured (JB debug logging was OFF). Relates to #ipcfullprompt-recur2, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing.
- [/] [#jb-load-tmux-window-verify] [recommended] Live-verify the new JB Load Tmux Window action (plugin 0.2.136): install the signed zip, confirm it autostarts/loads the tmux pane for a focused doc whose pane is not up (e.g. corky.md). If the autostart sync genuinely fails to spawn the pane in that case (vs. by-design passive open), capture the route/sync logs as a separate autostart bug.
- [/] [#jb-stale-busy-idle-footer] Three JB Run Agent Doc / Clear Session Context reports queue-instead-of-dispatch or block Clear while the live pane footer shows a Claude Code idle composer: monsterrodholders.md (actor gen 233, pane %19), equityfundingsource.md (gen 35, pane %3), tsift.md (pane %21, footer 22.4K (11%) ctrl+p commands). The tsift.md case is the strongest stale-busy candidate — Clear was blocked though the footer indicates idle. Fix: route.rs / session_actor_cmd.rs busy projection should treat the Claude Code idle-composer footer (ctrl+p commands / context % used) as idle direct evidence to repair a stale busy actor/lease projection before queuing or blocking, mirroring the existing OpenCode context % used idle-composer rule. Relates to #run-agent-doc-stale-busy-replay and #snrun (plan tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md).
- [/] [#adoc-queue-ipc-buffer-divergence] Root cause #2 IMPLEMENTED (submodule c483080a; pointer 4703f9788): live editor-buffer convergence after queue halt/drain. New core fn component::converge_queue_auto + agent_doc_converge_queue_auto FFI (C-int ABI for stable JNA); ipc_socket::send_queue_convergence carries queue_auto opening-tag state + queue_active frontmatter; preflight run_queue_maintenance pushes convergence through an active IPC listener after each halt/drain disk write (best-effort, non-fatal). JB plugin 0.2.137 parses queue_auto and applies via NativePatching.convergeQueueAuto in Document-API + VFS paths. Deterministic SimWorld repro queue_maintenance_converges_live_ipc_buffer_on_item_modified_halt + Kotlin ParsePatchJsonQueueAutoTest; make check green; both binaries built+installed (symbol verified in release cdylib); spec 07-orchestration-commands updated. GATED on live verification: install signed plugin 0.2.137, restart the route-owned agent-doc process to load the new binary, then confirm a real IDE-owned session no longer re-diverges (no recurring agent:queue diff / stuck_captured_cycle / inactive_queue_residue) after a consume->halt cycle without a raw git checkout. Plan: tasks/agent-doc/plan-queue-ipc-drift.md.
- [/] [#cancel-orphans-preflight-cycle] Canceling a run then invoking Run Agent Doc fails closed because the explicit cancel does not abandon the orphaned preflight_started cycle (route waits for staleness). Distinct from the stale-busy actor/lease items: this is per-document cycle_state left open by cancel. Plan: tasks/agent-doc/plan-cancel-orphans-preflight-cycle.md (recommend Option A — JB cancel emits a signal that abandons the no-capture preflight_started cycle). Reported 2026-05-29.
- [/] [#exchange-prompt-dropped-on-merge] User-authored exchange prompt (observed: "go") silently dropped during an IPC content_ours merge while the IDE held ipc.sock — never reaches snapshot/HEAD, assistant never responds, next preflight reports no_changes. Same drift family as #adoc-queue-ipc-buffer-divergence / #duplicate-response-after-commit but the victim is user input. Repro 2026-05-29 on src/boost-client/tasks/monsterrodholders.md (dropped the "go" authorizing #gfwconnect; assistant replied "nothing changed" for two cycles). Plan: tasks/agent-doc/plan-exchange-prompt-dropped-on-merge.md (recommend Option A — union user prompt lines / fail closed + add dropped_prompt_guard to session-check).
- [/] [#duplicate-response-after-commit] Late-IPC reposition signal re-inserts already-committed response body into working tree, wrapped in duplicate boundary markers. Symptom: next preflight interrupts with "direct response patchback without agent-doc cycle"; agent-doc write --commit returns "empty response — nothing to write"; only manual Edit-based dedup recovers. Repro: tasks/dreamable/separation.md cycle after commit 231eafc83 on 2026-05-28. Plan: tasks/agent-doc/plan-duplicate-response-after-commit.md (recommend Option A — both detect over-application in session-check and make IPC reposition idempotent).
- [/] [#prompt-duplicated-while-typing] [/] [#prompt-duplicated-while-typing] User prompt duplicated in the live editor while typing. PARTIAL FIX 1 (v0.34.0, submodule 2a6f60e5): prefix-agnostic append dedup (strip_user_prompt_prefix) for the ❯-prefix mismatch shape. PARTIAL FIX 2 (this cycle): L1 buffer-snapshot typing race — component::append_patch_already_present now recognizes when a synthesized patch carries an EARLIER keystroke-snapshot of the live region (per-line prefix alignment via patch_is_typing_snapshot_of) and no-ops instead of re-appending the divergent tail. Confirmed live in tasks/resume.md 2026-05-29: half-typed '- [#s93y]: Add to' snapshot vs live '...Add to resume' broke the old contains() substring check and re-appended the entire tail (close marker + Queue/Backlog/Done duplicated). Boundary 70bccf9b stayed intact and no new boundary was minted, ruling out the reposition path. Tests: append_patch_dedupes_midblock_typing_snapshot, append_patch_typing_snapshot_does_not_collapse_distinct_lines, append_with_boundary_does_not_duplicate_typing_snapshot. Both binaries built+installed; cdylib lib-installed for JB hot-reload. GATED on live re-verify (type a prompt into resume.md during an in-flight cycle). RESIDUAL: the exact editor-side full-tail concatenation downstream of the dedup miss was not statically traced — the dedup no-op short-circuits the append entirely so it should not surface, but confirm live. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md.
- [/] [#queue-auto-no-continue] Queue auto trigger does not continue after finalize session-check warning about pending shadow items. Steps to reproduce: (1) Session document with queue_active=true, queue_trigger=auto, and multiple queued items. (2) finalize --stream completes cycle but session-check issues pending shadow warning for items outside live agent:backlog. (3) Queue consumes head item but auto trigger does not fire for next item. (4) agent-doc write --commit with --done reaps the item, queue reports 9 remaining but auto continues stop. Expected: queue auto should continue to next item after successful closeout. Actual: queue stops after first item. Session: tasks/software/tsift.md. agent-doc version: 0.34.0. The pending shadow warnings about items appearing both inside and outside agent:backlog may be blocking the auto-continue path.
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#misfire-recovery-snapshot] Pre-auto-run recovery snapshot/tag (mirroring `compact`'s pre-mutation tag) so a destructive auto-run misfire is recoverable without git/sidecar archaeology. Proposed fix #4 of `#backlog-auto-marker-misfire` (now IMPLEMENTED for fixes #1–#3). Parked: only worth doing if a new auto-run path can mutate the backlog in place — currently the misfire is inert (queue-scoped) and surfaced (misplaced_component_attr warning). Plan: tasks/agent-doc/plan-backlog-auto-marker-misfire.md.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
