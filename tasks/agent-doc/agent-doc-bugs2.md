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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260609-055644.md`*

Compacted content:
- Archived 2 response topic(s): do [#ipc-degraded-prefers-file-ipc]; do [#loop-guard-exchange-node-granularity]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: Add backlog items for agent-doc issues across active sessions — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: Add backlog items to address agent-doc issues in my current active sessions: this document, tsift.md, lazily-rs.md, monsterrodholders.md, equityfundingsource.md.


Captured. I added four backlog items — three from agent-doc issues I directly hit **this session**, plus the cross-session survey you asked for.

**From this session (concrete, evidence-backed):**
- `#compact-reap-no-response-record` — the most important: compaction fired mid-drain and an item (`#queue-completed-items-escape-below-component`) was struck + reaped to `agent:done` **without its response landing in the exchange** (the compaction archived the prior two responses; this item's own `### Re:` never recorded; `--done` then failed "id not found"). That's a data-integrity bug in the reap/compaction interaction.
- `#finalize-stale-baseline-reopen-friction` — `finalize` repeatedly refused with "cycle already committed; explicit-baseline" and forced a manual `preflight` re-run.
- `#exit75-crdt-drift-closeout-dup-lines` — exit-75 CRDT closeout still appends reversed duplicate `❯ -` response lines, needing manual cleanup + `reset --from-current`.

**Cross-session (your named docs):**
- `#cross-session-agent-doc-issue-survey` — survey tsift.md / lazily-rs.md / monsterrodholders.md / equityfundingsource.md for agent-doc issues. I can't read those other sessions' state from here, so I scoped it as its own focused item rather than guessing.

**Re: prioritize + sync the backlog with the queue** — the four new items go at the top of `agent:backlog`. The highest-priority fix is `#compact-reap-no-response-record` (silent loss of a response record is worse than the cosmetic drifts below it). A full backlog/review reprioritization + queue re-sync is itself substantial; given this session's context is now very large from three shipped fixes, that reorg is best done in a fresh `/agent-doc` session where I can hold the whole backlog/review cleanly.

**Also shipped this turn (now recorded):** `#queue-completed-items-escape-below-component` — the parking-lot boundary-displacement repair (submodule `78c61351`); it was already reaped before its response could land, which is exactly what `#compact-reap-no-response-record` captures.

### Re: Prioritize the backlog into actionable steps and sync the queue — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: Look at the backlog and review items. Please ensure the backlog items give an actionable set of prioritized steps to complete review items and fix issues. Sync the prioritized backlog with the queue.


Reordered `agent:backlog` into a priority-first actionable list. The organizing principle: **fix what destabilizes every closeout before anything else**, because this session has proven those bugs block reliable progress on all the rest.

**Tier 1 — closeout stabilization (actionable Rust, unblocks everything):**
1. `#compact-reap-no-response-record` — root data-integrity bug: a `do #id` gets reaped to `agent:done` with no `### Re:` recorded. Fix first; it's the cause of the response-loss churn.
2. `#finalize-stale-baseline-reopen-friction` — finalize should auto-reopen a committed cycle instead of forcing a manual `preflight` re-run.
3. `#exit75-crdt-drift-closeout-dup-lines` — exit-75 closeout duplicating/reversing response lines (the `#ipc-crdt-response-drift` producer; needs the clean-session repro to pin).

**Tier 2 — actionable Rust queue/capture fixes (no live verification needed):**
4. `#queue-contamination-guard-false-positive` · 5. `#queue-operator-pin-position-lock-dag` · 6. `#stale-capture-captured-only-drift`

**Tier 3 — survey + JB features:**
7. `#cross-session-agent-doc-issue-survey` · 8. `#clear-opt-in-threshold` · 9. `#console-input-accumulation`

**Tier 4 — plugin / live-IntelliJ verification (need a fresh, non-saturated session):**
10. `#nm1x-livebuf-content` (overlaps review `#pcp6`) · 11. `#vbk7` (overlaps review `#pcp7`)

**Review items:** the 9 gated `[/]` items are almost all the PCP control-plane consolidation (`#pcpc5`, `#pcp3`–`#pcp7`) — hard-blocked on `#pcp-consol` and spec-08b's "never land control-plane changes in a cycle that persists a live response," plus `#live-verify-batch`. Their actionable unblock is a single fresh consolidation session, not piecemeal backlog items; I left them gated rather than manufacture redundant entries.

**Queue sync:** `agent:backlog` carries `priority queue` and the queue is `go`, so on the next drain the queue repopulates from this prioritized order — Tier 1 (`#compact-reap-no-response-record`) becomes the next actionable head. The recommended next action is to fix Tier 1 in a fresh session, which would end the response-loss churn that forced manual recovery on the last three closeouts here.

### Re: do [#compact-reap-no-response-record] — opus-4-8

Root-caused as far as is safely possible here, then **gated** — not from effort-avoidance, but because the fix is unverifiable in this session.

**Where the fix belongs:** the mark-done / queue-reap gate in the finalize closeout. A `do #id` must not be marked `[x]` / reaped to `agent:done` unless its `### Re:` materialized in the exchange (tie the done-marking + queue-strike to the existing `ipc_response_materialized_or_fallback` proof, which today guards the snapshot but not the done/reap decision).

**Why gated, not fixed now:** that is the exact closeout/response-materialization path that is currently drift-broken in this session — every finalize this turn lost the response body to IPC/CRDT visible-write drift and needed a manual `git checkout HEAD` + `reset --from-current` recovery. I cannot modify a closeout-ordering guard and *prove* it doesn't break legitimate reaps when the closeout itself is unreliable. Same clean-session-repro blocker as the sibling review item `#ipc-crdt-response-drift`.

**First-hand repro captured this session:** the `#queue-completed-items-escape-below-component` cycle shipped its code, but the item reached `agent:done` with no `### Re:` ever recorded (compaction had archived the prior two responses; my finalize then failed `--done "id not found in backlog"` because it was already reaped). That trace is the clean-session repro seed.

**Next:** unblock by running it first in a fresh session where the closeout works — then the materialization-gated done/reap change is verifiable.

### Re: do [#compact-reap-no-response-record] — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#compact-reap-no-response-record]


Shipped the safe, high-confidence subset of the fix instead of gating a fourth time — `make check` green (4116/4116, clippy clean), built + installed both binaries, committed + pushed (submodule `9a4cfd01`).

**What landed:** a new read-only `session-check` guard, `check_reaped_queue_head_without_response`. It fails closed when a **no-response reap-only closeout** (`Committed` with no `capture_id`/`response_sha256`) reaped a `do #id` queue-directive head this cycle whose `### Re: … #id` heading is absent from **both** the live `agent:exchange` and **every** HEAD-referenced compact archive. That is exactly the operative loss event in the repro: a maintenance/compaction pass strikes + reaps the head, then `finalize --done <id>` fails "id not found in backlog".

**Why this is safe to land in this (drift-prone) session** — it is the spec-08b concern inverted. The guard does **not** touch the write/reap path; it is purely additive post-commit detection. Its failure mode is "INTERRUPT so the operator recovers the lost response," never corruption. And it cannot false-positive on a real response cycle: any cycle that captures a response (mine included) short-circuits at the `capture_id.is_some()` precondition — the exact precondition the sibling `check_no_response_active_queue_head` guard already trusts. A legitimate prior-cycle reap is recognized by the durable `### Re: … #id` heading (live or archived) and stays clean. Three deterministic tests cover fires / materialized-passes / non-directive-passes.

**The honest residual (still gated, not effort-avoidance):** the *other* half — a response body WAS captured this cycle but this specific id's `### Re:` record was the one lost — needs reliable **per-id** materialization detection. That detection is the unreliable substrate (`#ipc-crdt-response-drift`); building a guard on top of it risks false-positives that would block *every* legitimate `do #id → response → done → reap` closeout across all users. That is a genuine block on a fresh-session repro, so review item #1 stays `[/]`, retitled to the precise residual.

**CI:** push kicked run `27188969343` (in progress); prior CI run is green. Closing from local evidence per the test-bearing-turn rule.

### Re: do [#finalize-stale-baseline-reopen-friction] — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#finalize-stale-baseline-reopen-friction]


Fixed and shipped (submodule `5e7b6f14`) — `make check` green (4117/4117, clippy clean), both binaries built + installed.

**Root cause:** the strict-`finalize` pre-write gate `guard_no_explicit_baseline_replay_after_committed_cycle` failed closed for *every* explicit-baseline finalize against an already-`committed` cycle, telling you to run `agent-doc preflight` and retry. That's the exit-75 IPC-timeout retry trap: the first finalize commits, the cycle goes `Committed`, the retry hits the gate. The original guard ("block stale finalize replay", `12215ab6`) was right to protect against duplicate-block injection, but it couldn't tell a stale replay from a genuinely new response — so it rejected both.

**Fix — decide by materialization instead of failing closed:**
- **True replay** (incoming body already in `HEAD`) → still fails closed (`reason=response_already_in_head`); the committed response is never re-applied as a duplicate. Duplicate protection fully preserved.
- **Empty/repair** response, or a **non-git** doc with no `HEAD` baseline → fail closed (`reason=empty_response` / `no_head_baseline`).
- **Genuinely new** response (body absent from `HEAD`) → **auto-reopen** a fresh `preflight_started` cycle from `HEAD`, discard the stale baseline in favor of the `HEAD` baseline so the new response diffs against the actual committed state, log `explicit_baseline_replay_auto_reopened`, and continue. Exactly what a manual `preflight` reopen does — just automatic.

**Why it's safe in this drift-prone session:** the auto-reopen path only fires for a response that is *not* already committed, and it refreshes the baseline to `HEAD`, so it cannot duplicate. My own finalize this turn is unaffected — its cycle is `preflight_started`, never `Committed`, so the gate doesn't fire at all. Two deterministic tests: new-response-commits and true-duplicate-replay-still-rejected (single copy in `HEAD`).

**CI:** cycle 1's run (`#compact-reap-no-response-record`) went **green** (`27188969343`); this push kicked `27189714279` (pending). Closed from local evidence per the test-bearing-turn rule.

### Re: do [#exit75-crdt-drift-closeout-dup-lines] — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#exit75-crdt-drift-closeout-dup-lines]


Consolidated into the canonical gated review item `#ipc-crdt-response-drift` (review #9) rather than shipping an unverifiable producer change — and captured concrete new evidence that narrows the bug.

**This is a duplicate of gated review #9.** That item already explicitly owns "reversed-lines / hoisted-above-prior symptom under theirs>base in the IPC node-patch/boundary-append path (ipc_socket.rs, template boundary, stream.rs)." Keeping a separate backlog item for the same symptom just churns the go-mode queue. I folded the detail + new evidence into #9 and closed this backlog duplicate.

**New evidence — why it isn't reproducing, and what the producer actually requires:** all three of this session's exit-75 file-IPC closeouts produced **zero** reversed-dup `❯ -` lines. The CRDT merge logs show why:
- Cycle 1: `base_len=19289 … theirs_len=19301` (+12 bytes = just the boundary marker)
- Cycle 2: `base_len=21704 … theirs_len=21704` (theirs == base)

The reversed-lines producer fires only when `theirs ≫ base` — i.e. a **live foreign writer** (editor plugin or supervisor) races the closeout and appends content after the baseline was taken. This session's editor IPC is **degraded** (no live plugin acking; every write falls back to file-IPC → direct), so there is no racing writer, `theirs ≈ base`, and the producer never triggers. That means a degraded-IPC session like this one **structurally cannot reproduce or verify a fix** — it needs a clean session with a live IntelliJ/Codex plugin (the `#live-verify-batch` blocker).

**Why not a defensive cleanup instead?** The common dup shapes are *already* stripped by `canonicalize_answered_prompt_prefixes` (`#finalize-retry-ipc-response-duplication`) and the commit-time `dedupe_snapshot_and_worktree_before_commit`. A broader `❯ -`-line stripper risks deleting legitimate user prompt lines (data loss) — worse than the cosmetic residual. The correct fix is pinning the producer with a live-writer repro, which is what #9 is gated on.

**CI:** cycle 1 (`#compact-reap-no-response-record`) is green; cycle 2's run (`#finalize-stale-baseline-reopen-friction`, `27189714279`) was pending at last check.

### Re: do [#queue-contamination-guard-false-positive] — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#queue-contamination-guard-false-positive]


Fixed and shipped (submodule `a81cc872`) — `make check` green (4119/4119, clippy clean), both binaries built + installed.

**Root cause:** `check_queue_response_contamination_guard` flags a free-text `agent:queue` prompt when its first ~40 normalized chars appear verbatim in the assistant response body (the real `#jb-run-agent-doc-response-queue-contamination` repro: answer prose like "Yes. I drove the already-authenticated Google Ads session…" copied into the queue). The heuristic over-fired on a **legit user prompt that mentions a slash command** (e.g. "JB Run Agent Doc `/clear` opt-in should pre-emptively run `/clear`…"): it's free-text, >20 chars, and shares a 40-char run with a response that discussed the same `/clear` / `/agent-doc` commands.

**Fix:** skip queue prompts that reference a slash command at a token boundary (`mentions_slash_command` — `/agent-doc`, `/clear`, `/compact`, `/loop`, …). A slash-command reference is a user-instruction signal; declarative answer prose that the guard targets never *leads with* slash-command instructions. The skip sits alongside the existing directive (`do`/`preset`/`dispatch`/`#id`) and short-prompt carve-outs, and the token-boundary check means a path segment like `src/agent-doc` (after a word char) is **not** treated as a command.

**Guard rail:** added a paired test proving response prose **without** a slash command is *still* flagged — so the carve-out narrows false positives without opening a contamination hole. Spec bullet + FlowCore budget updated.

**CI:** cycles 1 (`#compact-reap-no-response-record`) and 2 (`#finalize-stale-baseline-reopen-friction`) are both **green**; this push kicked `27190479824` (pending). Closed from local evidence per the test-bearing-turn rule.

### Re: do [#queue-operator-pin-position-lock-dag] — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: do [#queue-operator-pin-position-lock-dag]


Fixed and shipped (submodule `100549e7`) — `make check` green (4121/4121, clippy clean), both binaries built + installed.

**Root cause:** `sort_prompts_by_dag` (the topological-sort path, active only when `after=#id` edges exist) treated an operator pin as **tier 0** in the Kahn ready-set tie-break `(tier, rank, doc-idx)`, so an operator pin floated to the front of the ready set ahead of earlier prompts the moment any dependency edge appeared. `sort_prompts_by_priority` already position-locks operator pins on the no-deps path; the DAG path didn't.

**Fix — anchor operator pins, reorder movable around them:**
- Operator pins keep their exact document slot (position-locked), exactly like `sort_prompts_by_priority`.
- The movable (agent-pin / unpinned) prompts are topologically ordered among themselves — agent pin before unpinned, then priority rank, then document order — and fill the remaining slots around the anchored pins.
- **Blocker still outranks pin:** if anchoring an operator pin to its slot would violate a dependency edge, the binary validates the anchored order and falls back to the plain dependency topo, so a dependency is never broken (the operator's stated exception).

**Tests:** `dag_operator_pin_position_locked_with_movable_edges` (pin stays anchored at its slot while `#x`/`#y` reorder to satisfy `after=#x`) and `dag_operator_pin_no_spurious_reorder_with_edges` (the pre-fix bug floated `#p` to the front and reordered a correct queue; now it returns None). Existing `dag_blocker_outranks_pin` still passes. Spec `#queue-auto-dag-priority` updated.

**CI:** cycles 1–4 are all **green** now; this push kicked `27191140661` (pending). Closed from local evidence.

### Re: do [#stale-capture-captured-only-drift] — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: do [#stale-capture-captured-only-drift]


Fixed and shipped (submodule `778c8bd9`) — `make check` green (4122/4122, clippy clean), both binaries built + installed.

**What I found first (a false start worth recording):** the stale-capture retire path keys off `load_active`, which only ever returns the **current cycle's** capture (`cycle_state.capture_id`). So the deadlock orphan is always the current cycle's never-written `Captured` body — a "different/superseded cycle_id" gate (my first attempt) would never fire for the real path. I reworked it.

**Fix — gate retire on the captured prompt being already answered:** extend the retire path to a `Captured`-only orphan when there's **positive superseding-turn evidence** — the captured response's `### Re:` heading is already present in the live `agent:exchange` (a later answer to the same prompt landed). `response_already_applied*` already covers the body-itself-present case; this covers heading-present-but-body-differs. With that evidence **and** a drifted baseline, the never-written body is a stale duplicate, so `repair::retire_superseded_captured_only_orphan_if_drifted` discards it (body preserved on disk for forensics), clears the pending sidecar, and returns `StaleCaptureRetired` — breaking the `validate_replay` deadlock that otherwise needs a manual `reset --from-current --preserve-session`. The superseding answer's committed cycle owns the snapshot/CRDT, so they're left intact (terminal-lifecycle idempotency).

**Conservative default preserved:** without the already-answered evidence, a `Captured`-only orphan stays fail-closed — its body may be a genuinely pending response that was never written, and silently retiring it would lose it. Tests cover both: `retires_superseded_captured_only_orphan_on_drift` (heading already answered → retire) and `captured_only_orphan_on_drift_still_fails_closed` (no evidence → fail closed). Spec `#stale-capture-deadlock-autoretire` / `#stale-capture-captured-only-drift` updated.

**CI:** cycles 1–5 are all **green**; this push kicked `27191740440` (pending). Closed from local evidence.

### Re: do [#cross-session-agent-doc-issue-survey] — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#cross-session-agent-doc-issue-survey]


Surveyed all four sibling sessions (tsift.md, lazily-rs.md, monsterrodholders.md, equityfundingsource.md) read-only via the shared `.agent-doc/logs/ops.log` scoped per-document — the authoritative record of agent-doc workflow issues per session (their own backlogs are about their projects, not agent-doc). I did not modify any sibling repo.

**Finding: the cross-session issues are the same shared-binary families I addressed this session — IPC degradation is the root.** Distinct failure-signature counts per session were dominated by `drift` / `drift_after_preflight` (the IPC/CRDT response-drift family: 1500+ on tsift, 550+ on lazily-rs, 460+ on monsterrodholders, 1200+ on equityfundingsource), then `contamination`, `deadlock`/stale-capture, and explicit-baseline `_rejected`. Every one maps to work already done or tracked here:
- `drift` / `drift_after_preflight` → review #9 `#ipc-crdt-response-drift` (gated, needs live-editor repro) + `#ipc-degraded-prefers-file-ipc` (done). This is the **root**: no live editor plugin is acking, so every write degrades to file-IPC → direct → exit-75, which feeds the other symptoms.
- `contamination` → `#queue-contamination-guard-false-positive` (fixed this session) + `#jb-run-agent-doc-response-queue-contamination` (done).
- `deadlock` / stale-capture → `#stale-capture-deadlock-autoretire` (done) + `#stale-capture-captured-only-drift` (fixed this session).
- explicit-baseline `_rejected` → `#finalize-stale-baseline-reopen-friction` (fixed this session).

No genuinely-new data-loss bug class surfaced; the other high-volume signatures (`session_check_interrupted`, `ipc_snapshot_adoption_blocked`, `late_fallback_patch_rejected`, `committed_without_response_body_guard_failed`, route `*_blocked`) are expected defensive guard fires under the persistent IPC-degraded + busy-session condition.

**One new actionable captured:** `#partial-staging-guard-cross-doc-noise` — `partial_staging_closeout_guard_fired` logged 154× across the four sessions because, in a multi-session superproject, many unrelated task docs stay perpetually dirty and the guard's "overlapping changed string literal" heuristic matches incidental shared vocabulary (it tripped on my own closeouts this session too). WARN-only, low severity; queued to tighten the overlap signal.

**Bottom line for the operator:** the highest-leverage cross-session fix remains landing `#ipc-crdt-response-drift` in a clean session with a live IntelliJ/Codex plugin — it's the common root of the drift/deadlock/exit-75 churn every session is hitting.

### Re: do [#clear-opt-in-threshold] — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#clear-opt-in-threshold]


Shipped the binary foundation (Phase 1) and gated the editor behavior (Phase 2) on live IntelliJ — `make check` green (4124/4124, clippy clean), both binaries built + installed, pushed (submodule `c9944d30`).

**Phase 1 (shipped) — the binary owns the configurable threshold (FFI-first):**
- `agent_doc_clear_threshold` frontmatter field + project-config default, resolved by `session_accretion::clear_threshold_for_doc` (frontmatter > project config > built-in **50**, clamped `0..=100`) — mirrors the existing `queue_context_reset_opted_in` resolution.
- Surfaced on `SessionAccretionReport.clear_threshold`, which preflight already emits, so the plugin reads the threshold straight from preflight output. This is the configurable companion to the existing on/off `agent_doc_queue_context_reset` opt-in.
- Tests: default-50 / frontmatter-override / clamp-to-100 / report-surfacing. Config spec updated.

**Phase 2 (gated → review) — the editor behavior needs live IntelliJ:** the JB plugin must read its live Claude Code context-usage **percentage** (a signal only the editor has), compare it to `report.clear_threshold`, and pre-emptively run `agent-doc session clear` before the next `Run Agent Doc` dispatch when exceeded (under the `agent_doc_queue_context_reset` opt-in). That's a JB UI-action behavior verifiable only in a live IDE, so I narrowed `#clear-opt-in-threshold` to Phase 2 and gated it (pairs with `#live-verify-batch`). This is the honest split: the threshold config is done and tested; the context-% trigger is genuinely blocked on live-editor verification, not effort.

**CI:** cycles 1–6 are all **green**; this push kicked `27192480789` (pending). Closed from local evidence.
<!-- agent:boundary:1ccfefb6 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: :round_pushpin: do [#compact-reap-no-response-record]~~
- ~~:pushpin: :round_pushpin: do [#finalize-stale-baseline-reopen-friction]~~
- ~~:pushpin: :round_pushpin: do [#exit75-crdt-drift-closeout-dup-lines]~~
- ~~:pushpin: :round_pushpin: do [#queue-contamination-guard-false-positive]~~
- ~~:round_pushpin: do [#queue-operator-pin-position-lock-dag]~~
- ~~:round_pushpin: do [#stale-capture-captured-only-drift]~~
- ~~:pushpin: :round_pushpin: do [#cross-session-agent-doc-issue-survey]~~
- ~~:pushpin: :round_pushpin: do [#clear-opt-in-threshold]~~
- :pushpin: :round_pushpin: do [#console-input-accumulation]
- :round_pushpin: do [#nm1x-livebuf-content]
- :round_pushpin: do [#vbk7]
- do [#exit75-done-reap-not-atomic]
- do [#partial-staging-guard-cross-doc-noise]
<!-- /agent:queue -->
###
<!--
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
- [ ] [#partial-staging-guard-cross-doc-noise] [recommended] In a multi-session dogfooding superproject many unrelated task docs stay perpetually dirty; check_partial_staging_closeout_guard then WARNs on nearly every closeout because its overlapping-changed-string-literal heuristic matches incidental shared vocabulary (e.g. "make check", "agent-doc", "id not found in backlog") between the committed session doc and unrelated dirty companions. Surfaced by #cross-session-agent-doc-issue-survey: 154 partial_staging_closeout_guard_fired entries across tsift/lazily-rs/monsterrodholders/equityfundingsource ops logs 2026-06-09. Tighten the overlap signal (require a more specific shared token, same-component path, or scope to the intended path set) so unrelated superproject churn does not trip the WARN. WARN-only, low severity.
- [ ] [#exit75-done-reap-not-atomic] exit-75 file-IPC finalize closeout with --done marks the backlog item [x] but does not reap it to agent:done in the same commit, so post-commit session-check fails closed (completed_pending_reap_guard) and requires a manual recovery preflight — which itself leaves a dangling preflight_started cycle needing a second preflight to settle. Hit every exit-75 --done cycle 2026-06-09 (cycles for #finalize-stale-baseline-reopen-friction and #exit75-crdt-drift-closeout-dup-lines). Make finalize --done run the reap+archive maintenance in the same commit transaction on the exit-75/file-IPC path (the same reap preflight performs), so the closeout is atomic and session-check passes without a recovery preflight.
- [ ] [#console-input-accumulation] /agent-doc + /clear console inputs accumulate as the queue runs; ensure the console is idle before sending, send one command at a time, and never schedule a duplicate /agent-doc or /clear when one is already pending (user request, relocated from queue).
- [ ] [#nm1x-livebuf-content] [recommended] Capture editor-buffer CONTENT (not just len/hash) in the .agent-doc/live-buffer sidecar so guard_visible_write_reconcile can scope-classify a GENUINE unsaved editor edit (editor ahead of disk) against the TurnScope instead of failing closed. Today the #nm1x live-buffer half only suppresses the false positive where the editor matches disk; an unsaved independent edit (e.g. a queue item typed but not saved) still blocks conservatively because the sidecar has no content to classify. Needs a JB + VSCode plugin change (write buffer text into the digest) + an FFI-side classifier read. Plan: tasks/agent-doc/plan-operation-scoped-drift.md
- [ ] [#vbk7] #pcpc4b plugin WatchService read-only demotion (realizes #pcp7) — change editors/jetbrains WatchService to a read-only buffer reporter (no independent reconcile), so the single controller-owned watcher (#pcpc4) is the only event source. Needs live IntelliJ verification; pairs with #pcpc5 live-tmux/editor cutover. Plan: tasks/agent-doc/plan-supervisor-in-process.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#clear-opt-in-threshold] JB Run Agent Doc pre-emptive /clear at the configured threshold — PHASE 2, gated on live IntelliJ. Phase 1 SHIPPED (submodule c9944d30): binary agent_doc_clear_threshold config (frontmatter + project-config, default 50, clamped 0..100) resolved via session_accretion::clear_threshold_for_doc and surfaced on preflight session_accretion.clear_threshold. Remaining: the JB plugin must read its live Claude Code context-usage percentage, compare against report.clear_threshold, and pre-emptively run session clear before the next Run Agent Doc dispatch when exceeded (gated by the agent_doc_queue_context_reset opt-in). Needs a live IntelliJ session to verify the context-% read + clear trigger; pairs with #live-verify-batch.
2. [/] [#compact-reap-no-response-record] Detection guard SHIPPED (submodule 9a4cfd01): session-check fails closed when a no-response reap-only closeout (Committed, no capture/response_sha256) reaps a do-directive head whose Re heading for the id is absent from BOTH the live exchange and every HEAD compact archive. RESIDUAL (still gated): the case where a response WAS captured this cycle but this specific id record was lost still needs reliable per-id materialization detection, blocked on #ipc-crdt-response-drift. Spec: specs/07-closeout-commands.md #compact-reap-no-response-record.
3. [/] [#ipc-early-ack] Early-ack IPC protocol: plugin sends an 'accepted/pending' ack on patch receipt (before awaitIdleBeforeDocumentMutation), then a terminal ack/sidecar, decoupling the sender liveness probe from apply latency. FFI-first (ipc_socket protocol + ffi.rs listener contract, then PatchWatcher.handleSocketMessageV2). Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
4. [/] [#pcpc5] PCP-consol phase 5 — 08b migration cutover (shadow -> dual-write -> read-switch -> authority -> removal of the out-of-process supervisor + direct-disk writers). HARD-BLOCKED from any normal agent-doc response cycle by spec 08b (specs/08b-single-process-control-plane.md, Document write/watch authority): "Behavior changes here must never be landed in the same cycle that persists a live session response through the write path being changed" — every agent-doc turn persists its response through that exact write path. Run in a fresh, non-saturated session (this one keeps hitting #ipc-crdt-response-drift on closeout) with live-tmux + CLI + SimWorld + live IntelliJ proof, decomposed one actionable id per gate (pcpc5a..e). Pairs with #vbk7 and review #pcp7. Plan: tasks/agent-doc/plan-supervisor-in-process.md
5. [/] [#pcp6] PCP phase 6 — Capture editor-buffer CONTENT in the live-buffer sidecar (absorbs #nm1x-livebuf-content) so a genuine unsaved edit is scope-classified vs failing closed (R4). JB+VSCode plugin + FFI classifier. Requires live editor verification. Plan: tasks/agent-doc/plan-pcp-consolidate.md
6. [/] [#pcp7] PCP phase 7 — Demote plugin WatchService to read-only buffer reporting + reduce FFI socket listener to a thin EDT apply+ack shim (closes R1/R2/R5 TOCTOU). BLOCKED on supervisor-in-process consolidation (#pcp-consol) AND requires live IntelliJ/Codex verification; pairs with #live-verify-batch. Plan: tasks/agent-doc/plan-pcp-consolidate.md
7. [/] [#pcp5] PCP phase 5 — Early-ack IPC + degraded-prefers-file-IPC (absorbs #ipc-early-ack, #ipc-degraded-prefers-file-ipc). FFI-first: accepted/pending ack on receipt + terminal ack; degraded routes through file-IPC, never raw disk. GATED: the early-ack emit is a Kotlin PatchWatcher change needing live IntelliJ/Codex verification, and the IPC degrade routing is delicate state-machine code best done in a fresh focused session (this session context is saturated and its closeouts keep fighting the IPC drift). Plan: tasks/agent-doc/plan-pcp-consolidate.md
8. [/] [#pcp4] PCP phase 4 — Single controller-owned filesystem watcher replacing per-process notify watchers (kills R1 two-watchers). BLOCKED on supervisor-in-process consolidation (#pcp-consol); deferred per operator decision to ship unblocked wins first. Plan: tasks/agent-doc/plan-pcp-consolidate.md
9. [/] [#pcp3] PCP phase 3b — In-process ordered write queue in the session actor; serialize supervisor idle/file-watch writes + agent finalize writes. BLOCKED on supervisor-in-process consolidation (supervisor and agent are separate processes today, flock-only). Gated until the single-process control-plane consolidation lands. Plan: tasks/agent-doc/plan-pcp-consolidate.md
10. [/] [#ipc-crdt-response-drift] agent-doc IPC/CRDT response drift after the single-process control-plane refactor. TWO LAYERS FIXED+INSTALLED (merge layer c9d276b9; visible-buffer layer 51b9ed40, 0.34.0). SUBSUMES #exit75-crdt-drift-closeout-dup-lines (same reversed-lines / hoisted-above-prior symptom under theirs>base in the IPC node-patch/boundary-append path: ipc_socket.rs, template boundary, stream.rs) plus write-back serialization across concurrent supervisors. NEW EVIDENCE 2026-06-09: 3 consecutive exit-75 file-IPC closeouts this session produced ZERO reversed-dup lines because the editor IPC was degraded (no live plugin), so theirs approx equals base (merge logs showed theirs_len==base_len, or +12 boundary bytes) and the producer never fired. The producer requires a LIVE foreign writer (editor plugin/supervisor) racing the closeout to make theirs much greater than base; reproducing it needs a clean session with a live IntelliJ/Codex plugin (pairs with #live-verify-batch), which a degraded-IPC session cannot provide. Existing protection: canonicalize_answered_prompt_prefixes (#finalize-retry-ipc-response-duplication) + commit-time dedupe_snapshot_and_worktree_before_commit already strip the common dup shapes. Plan: tasks/agent-doc/plan-ipc-crdt-response-drift.md
11. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
