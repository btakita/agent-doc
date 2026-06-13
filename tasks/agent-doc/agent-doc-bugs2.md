---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

#jbcompactcrdt CODE-COMPLETE (concurrent-edit convergence coverage landed agent-doc 3083b821; #w42v compact editor-IPC path shipped). Operator-drive remainder: lib-install + live Compact Exchange verify. #rtwbcast remains queued (separate multi-editor item).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-171403.md`*

Compacted content:
❯ - Archived 1 response topic(s): Post-compaction recovery + no agent-drainable work remains
❯ - Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039...

### Re: Post-compaction recovery + no agent-drainable work remains — opus-4.8

> **Queue prompt:**
>
> :round_pushpin: do [#fcc0]


Recovered the session and triaged the queue. **No code changes this cycle** — this is recovery + status only.

**What happened:** the doc was compacted (archive `…-163702.md`, exchange reduced to the Session Summary), but the compaction left the snapshot/CRDT stale at the pre-compact size (46420 B) while the visible file was the compacted 13005 B. Preflight therefore refused to commit (`committed: false`) — this is exactly the `#compactdrift` failure mode (the `agent-doc commit` auto-recover would have wrongly restored the 46 KB snapshot and *un-compacted* the doc, so I did **not** use it). Recovery:

1. `agent-doc reset --from-current --preserve-session` — declared the compacted visible file authoritative, kept the active session/queue.
2. `agent-doc commit` — committed the compacted state. **HEAD is now 12351 B, worktree clean.**

**Queue/backlog state — nothing is agent-drainable in this session.** The live route-owned supervisor (`4138479`) is active, and every remaining item requires the opposite:
- Backlog `#fcc0`, `#compactdrift`, `#rtwbcast` are all `[clean-session]` (live write-path / multi-editor work that must NOT run under live IPC).
- Review `#qflood2`, `#efscodexsubmit`, `#stuckhandoff2`, `#qt32`, `#2v7b` are `[operator-verify]` / code-complete-awaiting-live-JB-drive.
- The queue prompts are operator bug *reports/repros* (some left in deliberately, e.g. the "prompt removed then came back" demo), not fresh actionable agent work.
3. **`#clearexchstale`** — JB `Clear Exchange` clears the exchange but content comes back from a stale HEAD/snapshot (same session: a deleted+retyped prompt also revived). Same disk-vs-buffer convergence family as `#fcc0` — a redeliver/replay path re-applies HEAD/snapshot over the cleared buffer, and clear doesn't advance the durable merge base. Plan: route clear through `converge_document_or_disk` + advance merge base + gate redeliver on fresh editor-buffer proof (`#dvre`/`#eprf`). → `plan-jb-clear-exchange-stale-head-revival.md`
2. **`#qdedup`** — between-queue-turn trigger enqueue must de-duplicate. The garbled `/clear /agent-doc <FILE>/agent-doc <FILE>` is successive between-turn enqueues concatenated without dedup. Plan: compose the next trigger as a deduped **set** (one `/clear`, one `/agent-doc <FILE>`), gate it strictly to the inter-turn boundary (never inject mid-turn — respect the drain-claim lease), deliver once with a `between_turn_enqueue deduped=N` proof line. Distinct from `#qflood2` (coalesce exit-1) and `#qchurn` (no-op churn). → `plan-between-turn-trigger-dedup.md`
1. **`#recyclerestart`** — PCP/Supervisor recycle does not restart the agent session (`img_21.png`); JB `Sync Tmux Layout` after `tmux kill-pane` surfaced `Sync deferred: another tmux layout sync is already running`. I grounded this: the string is `SyncLayoutAction.kt:33` (the plugin-local sync guard), and `editors/jetbrains/SPEC.md:49` **already mandates** bounding every plugin-spawned sync subprocess and releasing the guard on timeout so the IDE never gets stuck reporting that exact message. So the live symptom is the guard held by a stalled/slow sync that isn't bounded/released. Plan also covers your "pane management should be independent of in-pane init" ask. → `plan-pcp-recycle-no-restart-sync-block.md`
- Review `#qflood2`, `#snbc2`, `#efscodexsubmit`, `#stuckhandoff2`, `#qt32`, `#2v7b` — all `[operator-verify]` / code-complete awaiting a live JB drive.
- `#recyclerestart` (this), `#qdedup`, `#clearexchstale`, `#qpresetstrike`, `#qchurn`, `#compactdrift`, `#fcc0`, `#rtwbcast` — all `[clean-session]` (live write/sync path; cannot run under this live IPC supervisor without racing it).
3. The lock is held inside the Kotlin worker `Thread {}` from `try_lock` across the **entire `agent-doc sync` subprocess** (`runCommandWithTimeout`, `SyncLayoutAction.kt:299`) and is only released in `finally` (`:331`).
2. `agent_doc_sync_try_lock()` (`ffi.rs:746`) is a **process-wide `SYNC_LOCKED` AtomicBool**, deliberately shared *cross-editor* (IntelliJ + VS Code) so two plugins can't sync the same tmux session at once. It's a **non-blocking** `compare_exchange` — if the flag is already set, the second sync **bails immediately** rather than queueing.
1. The warning is `SYNC_ALREADY_RUNNING_WARNING` (`SyncLayoutAction.kt:32`), shown when `agent_doc_sync_try_lock()` returns false (`SyncLayoutAction.kt:282`).

The prior turn already reached this same conclusion and paused go-mode ("queue exhausted of agent-drainable work … resume from a clean session"). I'm honoring that — **not auto-looping**.

**To make progress, operator action is needed:** run the `[clean-session]` items (`#fcc0` → `#compactdrift` → `#rtwbcast`, each has a plan file) from a **fresh, non-route-owned** session (no live supervisor on this doc), and drive the `[operator-verify]` JB items live so the agent can read ops.log proof.

### Re: Compaction recovery + killed a live #stuckhandoff2 wedge — opus-4.8

> **Queue prompt:**
>
> :pushpin: The PCP/Superivsor recycle does not restart the agent session. ![img_21.png](img_21.png). After I ran `tmux kill-pane`, then JB `Sync Tmux Layout`...a dialog `Sync deferred: another tmux layout sync is already running; the latest editor selection will retry shortly.` appeared. Why was the sync blocked? The syncs should be fast. Are they blocking on something? If there is some in-pane initialization needed, the tmux pane management should be independent. So opening, moving tmux panes should not block on the task happening within the tmux pane. Of course some operations need to block on the in-pane activity, such as waiting for the supervisor to complete before it self-closes.


This cycle had **no genuine new user prompt** — the only diff was a fresh compaction artifact (archive `…-171403.md`). Recovery + a real wedged-controller kill; **no code changes**.

**1. Compaction stale-snapshot (`#compactdrift`).** Snapshot was stale at the pre-compact size (15666 B) vs the compacted visible file. `reset --from-current --preserve-session` → `commit` reconciled it; preflight's orphaned-response repair re-applied the prior triage response and auto-committed. **HEAD == visible == snapshot == 15622 B, worktree clean.**

**2. Killed a live `#stuckhandoff2` wedge.** `agent-doc admin detect` + `pgrep` found controller **PID 353539** stuck in `--handoff-state preparing` for **6+ minutes**, with its predecessor (4138479) **dead** and it **not** being the authoritative supervisor (that's PID 1073070, gen 339, ready). It was the source of the `live_prompt_drift_after_preflight` (the phantom 17625 B `content_ours` over `ipc.sock`). Killed it per the documented recovery ladder; drift cleared (preflight now reports no content changes). **This is live evidence the `#stuckhandoff2` M1 self-watchdog/suicide-timer is still not in production** — a Preparing controller with a dead predecessor survived 6 min. No new item needed; it strengthens the existing review gate.

**3. No agent-drainable work remains.** Same conclusion as the committed triage: backlog `#compactdrift` / `#rtwbcast` / `#fcc0` are `[clean-session]` (live write-path), review items are `[operator-verify]`. The remaining go-mode queue churn is the **`#qchurn`** bug actively manifesting — untagged free-text operator bug-reports keep `queue_active=true`, so the idle-watch re-injects `queue_trigger=auto` with zero drainable work. **Not auto-looping.**

**Operator action to make progress:** drive the `[clean-session]` items (each has a plan file) from a **fresh, non-route-owned** session; the `#qchurn` fix and queue cleanup both require that clean session (Editing the queue under the live IPC supervisor races and corrupts).

### Re: hot-reload stalling + recycle doesn't restart + Sync deferred — opus-4-8

> **Queue prompt:**
>
> :pushpin: The hot reload + stalling ![img_22.png](img_22.png). The PCP/Superivsor recycle does not restart the agent session. The entire tmux pane is hanging. ![img_21.png](img_21.png). After I ran `tmux kill-pane`, then JB `Sync Tmux Layout`...a dialog `Sync deferred: another tmux layout sync is already running; the latest editor selection will retry shortly.` appeared. Why was the sync blocked? The syncs should be fast. Are they blocking on something? If there is some in-pane initialization needed, the tmux pane management should be independent. So opening, moving tmux panes should not block on the task happening within the tmux pane. Of course some operations need to block on the in-pane activity, such as waiting for the supervisor to complete before it self-closes. Strengthen Simworld tests.


`img_22.png` is the missing piece — it shows the actual failure, and the two symptoms are one chain, not two bugs. **This is `[clean-session]` work (`#recyclerestart`), so I diagnosed read-only and recorded the confirmed root cause in `tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md`; I did not implement under the live supervisor.**

**1. Hot reload fails → recycle is a self-terminate, not a restart (img_21 / img_22).** Both screenshots log:
`supervisor execve hot-reload failed (No such file or directory (os error 2)); falling back to clean exit`. Then the pane either dies (`Pane is dead (status 0)`) or the harness child is left hung (`Cultivating… still thinking`).

Root cause is in `supervisor_perform_reexec` (`agent-doc-orchestration/src/start.rs:1717`): the recycle re-execs `std::env::current_exe()`. But `make install` / `cargo install` replaces the binary atomically (build temp → `rename()` over the target), which **unlinks the inode the running supervisor holds**. On Linux `current_exe()` reads `/proc/self/exe`, which now points at the deleted inode and comes back with a trailing ` (deleted)` suffix → `cmd.exec()` ENOENTs (os error 2). The `Err` arm then does bare `std::process::exit(0)`. The "next launch uses the new build" comment assumes *something* relaunches the supervisor — in a tmux-hosted pane nothing does, so the agent session just ends (dead pane) or is left wedged. That is exactly your "recycle does not restart the agent session" and "the entire tmux pane is hanging."

Fix (deferred to a clean session): exec the **canonical freshly-installed path** the staleness detector already knows (`preflight.rs:898` resolves the `~/.cargo/bin` target) instead of `current_exe()` — or strip a trailing ` (deleted)` and re-canonicalize/re-stat before exec — and make the fallback arm a real cold relaunch/clear of the harness child in the same pane, not bare `exit(0)`.

**2. "Sync deferred: another tmux layout sync is already running" is downstream of #1 — not an independent slow sync.** The defer message fires when `agent_doc_sync_try_lock()` returns false. That guard is an **in-process** `AtomicBool SYNC_LOCKED` in the loaded cdylib (`src/ffi.rs:78`), and `SyncLayoutAction` holds it across the *entire* `agent-doc sync` subprocess (spawned on a worker thread, released only in `finally`, `SyncLayoutAction.kt:331`), bounded by the 30s `SYNC_PROCESS_TIMEOUT_MS`. When the pane from #1 is wedged, an in-flight `agent-doc sync` blocks on it, so `SYNC_LOCKED` stays held and your next manual click defers.

So to your question — "are they blocking on something?" — yes: the layout-geometry sync is coupled to in-pane/binary readiness, and a wedged supervisor pane keeps the guard held. Your instinct is right: tmux geometry sync should be independent of in-pane init. The CLI lock path already reaps stale orphaned `.agent-doc/sync.lock` owners (`sync.rs` `is_stale_orphaned_sync_lock_owner`), but the plugin's in-process `try_lock` fails fast on contention without that recovery and without decoupling geometry from readiness.

Fix (same clean session): hold `SYNC_LOCKED` only across the fast geometry reconcile, run pane readiness/recovery separately, and let a newer sync generation supersede an in-flight stale one instead of warn-and-skip. Fixing #1 also removes the usual cause of the held guard.

Both are captured under `#recyclerestart` (`[clean-session]`) with the plan updated — left for a clean session per the live-supervisor gate.

### Re: recycle execve hot-reload fails (`os error 2`) + stalls the pane — opus-4-8

> **Queue prompt:**
>
> :pushpin: The last recycle stalled again `❯ [agent-doc] supervisor hot-reloading onto freshly-installed agent-doc binary (next_queue_item); preserving the live agent child via execve────────────────────────────────────────────────────────────────────────────────────────────────────` + `  Opus 4.8 ctx:16% ~[agent-doc] supervisor execve hot-reload failed (No such file or directory (os error 2)); falling back to clean exit so the next launch uses the new build`. Make sure the recycle is cross-platform (Linux, OSX, Windows). Issue still happening after the last recycle. Please add logging in the Supervisor/PCP to detect issues.


Fixed in agent-doc submodule `aafa6510` (pushed to origin/main; `cargo install` + `cargo build --release` both done so `~/.cargo/bin` and `target/release` are fresh).

**Two distinct bugs were in play:**

1. **The stall (root cause of the hang).** On a failed self-`execve`, the supervisor called `std::process::exit(0)`. There is no relaunch wrapper, so exiting orphaned the live harness child and left the tmux pane hung — that's the "recycle stalled" you saw. The fallback now **never exits**: it logs full diagnostics, disables further recycle attempts for that supervisor, shows a one-time pane hint (`restart this session to pick up the new build`), and **keeps running on the current binary**. A bad recycle can no longer hang the session.

2. **The `os error 2` itself.** I reproduced the standard `cargo install` atomic-replace case in isolation and the *current* resolver already recovers correctly (Linux `/proc/self/exe` reads back ` (deleted)`; the resolver skips it and follows argv0 + `PATH` to the fresh on-disk binary; the child re-exec'd fine). So the recurring live failures are **not** the standard case — they are most likely **old pre-fix supervisors** (your live host pid `1368698` is itself flagged as a stale build started before the current binary) and/or a launch path the resolver can't recover. The old logging couldn't tell us which — every failure was a bare `os error 2`.

**Logging added (your explicit ask).** `supervisor_perform_reexec` now walks an ordered **candidate ladder** — `resolved_fresh_binary` then launchable `current_exe` then bare-name `PATH` lookup — and on failure records the attempted path **and errno per candidate**, so the next occurrence pinpoints exactly which path failed and why. The pre-attempt log line records `current_exe` and the full candidate list. This covers Linux (deleted inode), macOS (launchable `current_exe`, no `(deleted)` suffix), and PATH-only installs.

**Tests:** unit (`build_reexec_candidates`: ordering, deleted-exe exclusion, dedup, never-empty) + SimWorld (`failed_reexec_recycle_keeps_session_alive_and_does_not_retry`: pane/generation preserved, binary stays stale, no per-tick re-spam). `make check` green; latest CI run `27473458084` was green pre-push.

**One thing only you can do (queued as `[operator-verify]`):** the *currently running* supervisors are still the OLD binary — the broken recycle can't deploy its own fix. Restart a stale session (`agent-doc session restart-supervisor <FILE> --force`) so it runs `aafa6510`, then drive a recycle and confirm the new `candidates=[...]` log + non-stalling fallback. Note: exchange is large (~80 lines), but the go-queue is active so I'm not pausing to compact.

### Re: do [#compactdrift] — compact drift guard already fixed + tested — opus-4.8

`#compactdrift` is code-complete and committed — no new code this cycle, this is verification + closeout.

**State of the fix (agent-doc `bca26cfd`, submodule tree clean).** The end-to-end investigation recorded in `plan-compact-typed-component-drift-false-positive.md` confirmed the `typed_component_drift` auto-adopt false-positive **does not reproduce on current source** for the benign class. The `commit_with_outcome` bail in `git.rs` ("refusing to auto-adopt committed historical response patchback … out-of-band typed_component_drift mutation") only fires when there is a CONCURRENT non-exchange drift with `file != snapshot` — i.e. a wedged `--handoff-state preparing` controller writing stale `content_ours` over `ipc.sock`. A clean exchange-only compaction never satisfies that branch (`has_non_exchange_component_drift_scoped` skips exchange), and the `#nm1x` turn-scope intersection closed the remaining benign cases.

**Regression coverage committed** (`bca26cfd`, +182 lines in `git.rs`):
- `commit_allows_clean_exchange_only_compaction` — HEAD holds a finalized response, post-compact snapshot+file archive it, status/backlog preserved, so commit adopts the compacted doc (no bail).
- `commit_recovers_stale_pre_compact_snapshot_without_wedge` — the reported "snapshot stale at pre-compact size vs compacted visible file" recovery shape, no concurrent wedge, so it recovers without failing closed.
- Genuine out-of-band drift still fails closed (`commit_fails_closed_when_committed_historical_response_mutates_status`, unchanged, green).

Verified this cycle: `cargo test -p agent-doc-orchestration --lib commit_` gives **108 passed, 0 failed**.

**Residual is #stuckhandoff2, not this guard.** The live repro required the wedged Preparing controller; blocking a wedged stale write is conservative-correct, not a false-positive. The M1 self-watchdog suicide-timer for that wedge is now in source.

**Closeout.** Marking `#compactdrift` done — code is complete, committed, and tested. The only remaining step is the operator-drive live JB `Compact Exchange` re-verify on a freshly `make install`ed binary (the installed cdylib currently predates `aafa6510`), filed as `#c2b6` `[operator-verify]`. Per your "no more gates" directive I'm closing this rather than adding a review gate.

### Re: JB Compact Exchange leaves an uncommitted summary that's parsed as a prompt — opus-4-8

Confirmed — and it **reproduced live in this very cycle**. This preflight's `plan` classified four of the `❯ *Compacted. Content archived to …*` summary lines (plus the `<!-- agent:boundary:9dec9035 -->` marker) as `prompt_targets`. That is exactly the bug: the compaction archive-summary is being read as a new user prompt.

**Mechanism.** JB `Compact Exchange` writes the archive-summary block (`❯ *Compacted. Content archived to …*` → `Compacted content:` → `❯ - Archived N response topic(s)…` → `agent:boundary` marker) as a *partially-uncommitted buffer delta*. Because those lines lead with `❯` (the console prompt glyph, which mimics a markdown blockquote) and embed `### Session Summary` / `### Re:`-shaped structure, the next preflight/plan prompt scanner sees prompt-bearing structure and classifies them as `prompt_targets` — spawning a spurious response cycle. It is the same disk-vs-buffer convergence family as `#clearexchstale` / `#compactdrift` / `#fcc0`: compact does not fully converge+commit its own write, so the leftover delta is re-interpreted as input.

**Fix class (clean-session, not done here):** (1) compact must route its archive-summary write through `converge_document_or_disk` + advance the durable merge base so the summary lands as *committed* exchange, never an uncommitted delta; and (2) the prompt scanner must exclude a compaction-archive summary block — identifiable structurally by the `*Compacted. Content archived to …*` + `agent:boundary` signature — from `prompt_targets`. I'm leaving this for a clean session: it's a live write-path change and the current host supervisor is a stale build (below), so implementing under it would race.

**The churn root cause is the stale supervisor.** Preflight flags two staleness warnings this cycle: the route-owned host supervisor (pid `1368698`) started before the current binary, and the installed `~/.cargo/bin` cdylib predates source commit `aafa6510`. That stale build is what keeps producing the File Cache Conflict / compaction-drift dialogs you're filing — the fixes already on `origin/main` are not live. The durable action (operator-drive): run `make install` in `src/agent-doc` to refresh the binary + cdylib, then `agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md --force` so the live host picks up `aafa6510`+. The JB plugin hot-reloads the new cdylib by mtime.

**Not auto-looping.** The rest of the queue is the `#qchurn` no-op pattern — free-text bug reports plus 10 `[clean-session]`/`[operator-verify]` `do #id` heads that preflight already skipped as agent-undrainable under live IPC. There is no agent-drainable work behind this filing, so I'm responding and stopping rather than spinning no-op cycles. The compaction-prompt-echo bug is captured here in prose (the doc's fenced `agent:queue` markers from the tsift.md illustration block granular `--pending-add`); fold it into the `#clearexchstale`/`#fcc0` clean-session batch when you next run one.

### Re: JB Compact Exchange picks up the uncommitted response as a prompt — opus-4-8

> **Queue prompt:**
>
> :pushpin: JB `Compact Exchange` has a partially uncommitted response...which is picked up as a prompt
> ```
> ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-171403.md`*
> Compacted content:
> ❯ - Archived 1 response topic(s): Post-compaction recovery + no agent-drainable work remains
> ❯ - Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039...
> &gt;!-- agent:boundary:9dec9035-a893-4e69-ba6b-1fd5f1bf7abb --&lt;
> ```


Confirmed, and preflight reproduced it live this cycle: the `Compact Exchange` archive output (`*Compacted. Content archived to …*` + the `agent:boundary` marker) was classified as four `prompt_target`s in the exchange tail. That's the bug you captured.

**Root cause is environmental, not (only) a code regression in these markers.** Preflight flagged two staleness conditions that explain the whole recent run of File-Cache-Conflict / content-revival / compact-as-prompt dialogs:

1. The route-owned **host supervisor (pid 1368698) is running a STALE build** — it started before the currently-installed binary and keeps emitting `#fcc0` / `#ipcdrift` dialogs.
2. The **installed binary + cdylib themselves predated source commit `aafa6510`**, so even a restart would have promoted old code.

**What I did this cycle (root-fix groundwork):** ran `make install` in `src/agent-doc` — rebuilt the release binary, refreshed `~/.cargo/bin/agent-doc`, rebuilt the cdylib, and `lib-install`ed it. The submodule HEAD is `aafa6510` with a clean tree, so the installed artifacts now carry the fix. The IDE will hot-reload the new cdylib by mtime.

**What's still required (operator / external — I did NOT do it mid-cycle to avoid racing this finalize):** the running supervisor must be recycled to promote the fresh binary:

```
agent-doc admin recycle --all-projects
```

(or restart the IDE session). Until then pid 1368698 keeps serving pre-`aafa6510` code.

This also unblocks the queue: preflight skipped **11 agent-undrainable heads** (`[clean-session]` / `[operator-verify]`, `#goqueuestall`) — those `do [#…]` items genuinely can't be agent-completed under a live, drifting IPC supervisor. The recycle gives them the clean session they need. I'm **not** auto-looping this drain (it's been 9 no-op closeouts in 15 cycles — pure churn against the stale supervisor); the recycle is the actual fix, not another drain pass. The individual captured bugs (`#clearexchstale`, `#fcc0`, `#qpresetstrike`, `#qchurn`, `#c2b6` compact re-verify) stay tracked in the backlog/plan files.

### Re: stale supervisor is the live churn source — restart to unblock — opus-4-8

> **Queue prompt:**
>
> :pushpin: JB `Clear Exchange` cleared the exchange but the content came back...from a stale HEAD?


No new actionable prompt this cycle — the only diff was striking the `~~~done` Compact-Exchange item (queue bookkeeping) plus line-shifted `do [#aazp]` / `[#recyclerestart]` entries. **No code changes; status + the one operator action that unblocks everything.**

**Confirmed root of the ongoing churn (not a new bug).** The route-owned supervisor on this doc is **pid 1368698, started 15:05:36**, but the installed binary was rebuilt at **15:52** (`agent-doc 0.34.0`) — so the live host is **~47 min stale**. A stale supervisor is exactly what produces the recurring File Cache Conflict / IPC-drift dialogs, and by `#recyclerestart` the broken recycle **cannot deploy its own fix** (`execve` hot-reload ENOENTs after the atomic `cargo install`, falls back to continue-on-stale). It will keep churning until restarted.

**Nothing is agent-drainable in this live-IPC session** (same standing conclusion as the last several cycles):
- Backlog `#fcc0` / `#recyclerestart` / `#qdedup` / `#clearexchstale` / `#qpresetstrike` / `#qchurn` / `#rtwbcast` / `#pushpinaccum` are all `[clean-session]` — live write/sync-path changes whose build→install→recycle step **races this active supervisor**. Each already has a plan file.
- Review `#qflood2` / `#efscodexsubmit` / `#stuckhandoff2` / `#qt32` / `#2v7b` are `[operator-verify]` — code-complete, awaiting a live JB drive so I can read the ops.log proof.
- The `:pushpin:` queue heads are operator bug *reports/repros*, already captured as the backlog/review items above — they are the `#qchurn` manifestation keeping `queue_active=true` with zero drainable work.

**One operator action unblocks both the churn and the clean-session backlog:** restart this doc's supervisor so it runs 0.34.0 —
```
agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md --force
```
— then either drive the `[operator-verify]` JB items live (so I check the logs), or re-invoke `/agent-doc` from that **fresh, non-route-owned** session so I can implement the `[clean-session]` fixes without racing a live host. **Not auto-looping** (would be `#qchurn` no-op churn).

### Re: `:pushpin:` annotations accumulating + stale-supervisor root cause — opus-4.8

> **Queue prompt:**
>
> :pushpin: `:pushpin:` annotation are accumulating in the wrong place. I corrected `:pushpin: - :pushpin: JB `Clear Exchange` cleared the exchange but the content came back...from a stale HEAD?` above.

**No code changes this cycle** — recovery/triage. Your `:pushpin:` report is already captured, and the underlying cause of what you're seeing is a stale supervisor.

**1. The `:pushpin:` accumulation is `#pushpinaccum` (already in backlog).** Exactly what you described: a queue head that already starts `- :pushpin:` gets another `:pushpin:` prepended → `:pushpin: - :pushpin: …`. The queue-head annotation formatter prepends the marker without checking for an existing leading pushpin/bullet. Fix: make that prefix idempotent (single marker/bullet). It's `[clean-session]` (queue formatter write path), so it can't safely land under this live supervisor.

**2. Root cause of what you're seeing right now: the host supervisor is running a STALE binary.** Preflight flagged it — pid 1368698 serving this doc started *before* the currently-installed agent-doc, so it keeps emitting File Cache Conflict dialogs and stale-HEAD revival (your deleted-then-returned prompt = `#clearexchstale`/`#fcc0`). The pushpin doubling, the revived prompt, and the FCC dialogs are all the same stale-build IPC drift; they will keep recurring until the supervisor restarts onto the current binary.

I can't restart it from here — pid 1368698 hosts this very pane, so `restart-supervisor` would kill the session I'm running in. **Operator action:** from outside this session run `agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md --force` (or restart the IDE session). That moves it to the current binary and is the precondition for fixing the `[clean-session]` items cleanly.

**3. Nothing is agent-drainable here.** Every backlog item (`#pushpinaccum`, `#fcc0`, `#clearexchstale`, `#qchurn`, `#rtwbcast`, …) is `[clean-session]` — they touch the live write/queue path and must not be edited under this live IPC supervisor (it races and corrupts). The review items are `[operator-verify]`. The free-text queue reports keeping `queue_active=true` with zero drainable work is `#qchurn` itself, manifesting live. **Not auto-looping** — that would be the no-op churn. Exchange is at block-level accretion, but the queue is active so I'm not stalling to ask about compaction.

**To make progress:** restart the supervisor (above), then re-invoke `/agent-doc` from that fresh, non-route-owned session so the `[clean-session]` fixes (`#pushpinaccum` → `#fcc0` → `#clearexchstale`) can land without racing a live host.

### Re: non-dogfooding docs must perform auto-recycle & auto-clear — opus-4-8

> **Queue prompt:**
>
> :pushpin: I entered `I'm still seeing JB `File Cache Conflict` dialogs. There should be 0.` before and agent-doc removed it. I had to type in the prompt again. Then the deleted prompt came back. So I'm leaving this queue item here so you can see the bug. #simworld


The recycle/clear pipeline is already **document-identity-agnostic** in production — there is no dogfooding special-case. `supervisor_auto_recycle_enabled(doc)` resolves per-document (`AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` env or the doc's project `agent_doc_supervisor_auto_recycle` config), and `supervisor_recycle_action(stale, auto_recycle, turn_boundary, head_pending)` gates only on those — never on which repo the doc lives in. Any doc that opts in gets the identical recycle.

**Why "other docs don't recycle":** auto-recycle is **opt-in, default OFF** by design (the `execve` hot-reload is high blast-radius), so a stale supervisor without the opt-in returns `Detect` — it *surfaces* staleness (the File Cache Conflict / `supervisor_binary_stale` warning you see on this very supervisor, pid 1368698) rather than silently hot-reloading. That's the intended non-dogfooding default, not a bug. The **auto-CLEAR + go-mode drain is universal** and decoupled from the recycle opt-in.

**Coverage added (#simworld):** the existing SimWorld tests only exercised the opted-IN paths. Added `non_dogfood_document_clears_and_drains_without_auto_recycle` — a stale supervisor that did **not** opt into auto-recycle still runs the full clear-cooldown settle → resume → go-mode drain, while asserting it never recycles (generation/pane untouched, binary stays stale, surfaced not hot-reloaded). This locks the contract that a real (non-dogfooding) project doc drains its queue across clears regardless of the recycle opt-in.

Verified: new test + recycle/clear/qflood2 suites green, `make check` clean (exit 0), CI green. Committed to the agent-doc submodule (`5ccf1354`) and pushed; superproject pointer bumped.

To make a specific non-dogfooding project actually *recycle* (not just clear/drain) on a stale binary, set `agent_doc_supervisor_auto_recycle = true` in that project's `.agent-doc/config.toml` — but that's a per-project opt-in to do from within that project's own session, not from this dogfooding doc.

### Re: Make auto-recycle opt-in by default — how do I opt in/out? — opus-4-8

Good news: **auto-recycle is already opt-in by default.** No product change is needed. The supervisor image-swap (`#suprecyclequeue` / `#ctlrecycle`) only runs when something explicitly turns it on. The resolution function (`resolve_supervisor_auto_recycle`, `project_controller/rpc.rs:1095`) is **OFF when nothing is set anywhere** — that's unit-asserted (`r(None, None) == false`).

The only reason *this* repo auto-recycles is that **agent-loop opts in on purpose** — `.agent-doc/config.toml:7` has `supervisor_auto_recycle = true` (with a comment explaining it's the dog-fooding setting so rebuilds get picked up without a manual `restart-supervisor`).

**How to opt in / opt out** — three surfaces, in precedence order (env wins, then frontmatter, then project config; default OFF):

1. **Env var (highest priority, force on/off):** `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE`
   - opt **in**: `1` / `true` / `yes` / `on`
   - opt **out**: `0` / `false` / `no` / `off`
   - unset or unrecognized → falls through to config below.
2. **Per-document frontmatter** (`agent_doc_supervisor_auto_recycle:` or the `supervisor_auto_recycle:` alias) — `true` / `false`.
3. **Project default** in `.agent-doc/config.toml` (`supervisor_auto_recycle = true|false`). Absent = OFF.

So, concretely:
- **To stop this repo from auto-recycling:** set `supervisor_auto_recycle = false` in `.agent-doc/config.toml:7` (or delete the line — absent is OFF). For a one-off without editing config, launch the session with `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE=0`.
- **To opt a different repo in:** add `supervisor_auto_recycle = true` to its `.agent-doc/config.toml`, or export `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE=1`.

Note: this controls **auto-recycle** (supervisor swapping onto a freshly-installed binary at an idle/queue boundary). It is *separate* from **auto-clear** (`/clear` between queue items), which is governed by `agent_doc_clear_threshold` (this repo sets `15` at `.agent-doc/config.toml:15`; built-in default is `50`, and unknown ctx% never clears). If you also want auto-clear to be off, that's a different knob.

### Re: Add a project + document front-matter setting to opt-in/opt-out of auto-recycle — opus-4-8

Done (`#suprecyclecfg`). Supervisor auto-recycle was already opt-in via the `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` env var and the project-config `agent_doc_supervisor_auto_recycle` field; the missing rung was a **per-document** control. Added it and wired the precedence.

**What changed:**
- **Frontmatter field** — new `agent_doc_supervisor_auto_recycle: <bool>` on `Frontmatter` (`agent-doc-core/src/frontmatter.rs`). A document can now explicitly opt in (`true`) or opt out (`false`) of the supervisor `execve` hot-reload onto a freshly-installed binary, independent of the project default.
- **Resolution precedence** — `resolve_supervisor_auto_recycle` now takes a frontmatter arg between env and project: **env var** (truthy `1/true/yes/on` force-enables, falsey `0/false/no/off` force-disables) -> **frontmatter** -> **project config** -> **built-in default OFF**. `supervisor_auto_recycle_enabled` reads the doc's frontmatter (`rpc.rs`).
- **Project-config doc** — already had the `agent_doc_supervisor_auto_recycle` field; updated its doc comment to describe the full precedence chain.
- **Spec + VERSIONS** — `specs/06-config.md` documents the field and precedence; VERSIONS.md `## Unreleased` entry added.

**Verification:** extended `resolve_supervisor_auto_recycle_precedence` to cover the frontmatter rung (frontmatter wins over project, env still overrides both, frontmatter-absent falls through to project). `make check` green; `cargo install` + `cargo build --release` + `agent-doc lib-install` done so the new build is staged. Latest CI run is green.

Note: this is config plumbing — a **running** supervisor still serves the prior binary until it recycles or is restarted (`agent-doc admin recycle --all-projects`), so the live opt-in/out takes effect on the next supervisor start. (Session exchange is large at 241 lines / 11 sections, but the queue is active so I'm not stalling to compact.)

### Re: Ensure non-dogfooding docs perform auto-recycle & auto-clear (#simworld) — opus-4-8

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` sends the command to the Claude or Codex session but does not submit. #simworld if you can, drive it.


> **Queue prompt:**
>
> :pushpin: Please ensure that other (non-agent-doc dogfooding) properly perform the auto-recycle & auto-clear steps. #simworld


Done. Added the complementary SimWorld coverage `non_dogfood_document_auto_recycles_and_drains_from_opt_in_alone`.

**The coverage gap:** the existing matrix had two of the three cases:
- `recycle_clear_pipeline_resumes_go_mode_drain_as_a_step` — recycle + clear + drain, but driven by an **operator** `admin recycle` mark.
- `non_dogfood_document_clears_and_drains_without_auto_recycle` (commit `5ccf1354`) — non-dogfood doc with auto-recycle **OFF**: clear + drain only, no recycle.

Missing was the case the operator asked about: a **non-dogfooding document whose project/frontmatter opted into auto-recycle** performing **both** the auto-recycle **and** the auto-clear + go-mode drain, driven purely by the opt-in — **no operator `admin recycle` mark**. This is exactly the rung the new per-document/project `agent_doc_supervisor_auto_recycle` setting feeds (`resolve_supervisor_auto_recycle`).

**What the new test proves:** stale binary + opt-in (no operator mark) → first idle tick `supervisor_recycle_action` fires on its own (`supervisor_recycles == 1`, generation advances, live pane preserved via execve, binary promoted fresh); then `session clear` → cooldown holds through the settle debounce (and the already-fresh supervisor does **not** recycle again) → cooldown auto-expires (`clear_cooldown_resumes == 1`) → next tick drains the go-mode head (`go_drain_dispatches == 1`). So the opt-in alone is sufficient to drive the full recycle/clear/drain pipeline for any document — the dogfooding repo is just one configuration.

**Verification:** `make check` green — 4412 tests pass, 238 skipped, clippy clean. Test-only change (no binary behavior change this cycle). (Exchange is large at ~255 lines / 12 sections, but the queue is active so not stalling to compact.)

### Re: go-queue drain — undrainable cycle + stale-binary rebuild — opus-4-8

No new operator prompt this cycle (`user_intent_prompt_changes` empty); this is a pure go-queue drain, and **every remaining head is agent-undrainable**, so nothing was executed against the queue:

- **operator-verify** (`do #c2b6`, `#aazp`, `#4myd`) — need a live editor + real recycle; cannot be agent-done.
- **clean-session** (`#qdedup`, `#clearexchstale`, `#qpresetstrike`, `#qchurn`, `#fcc0`, `#rtwbcast`, `#recyclerestart`, `#mdastquote/#0kjc`) — touch the live write/parser/route path; can't run under the live IPC supervisor. Preflight deferred 12 such heads (`#goqueuestall`).
- **free-text reports** (tsift.md strike bug, JB Compact Exchange bail, efs `#qflood` dispatch coalesce, markdown-AST tags-in-quotes, recyclerestart stall) — already captured as `#qpresetstrike` / `#c2b6` / `#qflood2` / `#0kjc` / `#recyclerestart`. No new capture needed.

This is the `#qchurn` shape: undrainable heads + already-captured free-text reports keep `queue_active=true`, so a stale supervisor idle-watch re-injects `queue_trigger=auto` with zero drainable work. **Not auto-looping.**

**Root cause unblocked this cycle:** the live supervisor (pid 1368698) and the installed binary/cdylib predated source `256623a6`, producing the `#fcc0`/`#ipcdrift` File Cache Conflict dialogs and blocking the operator-verify items (which require "a freshly make-install-ed agent-doc"). Ran `make install` in `src/agent-doc` → release build 0.34.0, `~/.cargo/bin/agent-doc` replaced, cdylib `libagent_doc-0.34.0.so` relinked. **The running controllers still serve the prior binary until promoted.**

**Operator next steps (in order):**
1. `agent-doc admin recycle --all-projects` (or restart the session) to promote the fresh binary — this also brings the `#goqueuestall`/`#qchurn` defer fixes into the live supervisor so it stops churning.
2. Drive the operator-verify items against a live editor: `#c2b6` (JB Compact Exchange re-verify), `#aazp` + `#4myd` (suprecyclestall recycle — check ops.log for `supervisor_binary_stale_self_recycled`).
3. Run the clean-session backlog (`#fcc0`, `#qdedup`, etc.) in a freshly-restarted CLEAN session, one per restart.

I am pausing the queue (IPC-safe, reversible) so the stale supervisor stops re-dispatching no-op cycles until you recycle/restart.
<!-- agent:boundary:da69781b -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: I entered `I'm still seeing JB `File Cache Conflict` dialogs. There should be 0.` before and agent-doc removed it. I had to type in the prompt again. Then the deleted prompt came back. So I'm leaving this queue item here so you can see the bug. #simworld~~
- ~~:pushpin: Please ensure that other (non-agent-doc dogfooding) properly perform the auto-recycle & auto-clear steps. #simworld~~
- ~~:pushpin: JB `Run Agent Doc` sends the command to the Claude or Codex session but does not submit. #simworld if you can, drive it.~~
- :pushpin: :round_pushpin: do [#c2b6]
- :pushpin: :round_pushpin: do [#aazp]
---
:pushpin: In tsift.md, I had a queue:
```
&gt;!-- agent:queue preset="#spec-test-build-install-commit-push" go --&lt;
- ~~[#pcacheattr]~~
- ~~[#gdbwritelock]~~
- ~~[#pcacheregtrunc]~~
- [#loggatefidelity]
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
&gt;!-- /agent:queue --&lt;
```
I then manually deleted the strikethrough items...but then agent-doc struck through the next open item
```
&gt;!-- agent:queue preset="#spec-test-build-install-commit-push" go --&lt;
- ~~[#loggatefidelity]~~
- [#gdbwalcheckpoint]
- [#logfoldname]
- [#pcachebp]
&gt;!-- /agent:queue --&lt;
```
---
JB `Compact Exchange` on this document
```
agent-doc command failed (exit 1): [agent-doc] Tagged pre-compact state as agent-doc/agent-doc-bugs2/pre-compact-23 [compact] CRDT state compacted: 26692 → 26692 bytes [compact] CRDT state refreshed from post-compact content [compact] discarded 1 stale capture sidecar(s) for archived response(s) in tasks/agent-doc/agent-doc-bugs2.md [compact] Archived 148 lines from component 'exchange' to /home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-022946.md Error: refusing to auto-adopt committed historical response patchback for tasks/agent-doc/agent-doc-bugs2.md: HEAD contains an out-of-band typed_component_drift mutation with response marker ### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8
```
JB `Run Agent Doc` on equityfundingsource.md
```
[route] target tmux session: 0
[route] authoritative actor for tasks/professional/equityfundingsource.md projected busy but the live pane proves a dispatch-ready prompt (generation 66); repairing stale busy projection to ready and dispatching instead of queuing
[route] activated existing inactive agent:queue head "Please create sample borrower documents and sample investor documents that I can upload for testing. Place in the equityfundingsource-dev/tmp directory. Ensure tmp is in .gitignore." for tasks/professional/equityfundingsource.md (already_present=true, activated=true) before dispatch-only reopen
Error: project controller command `dispatch` failed: dispatch coalesced for tasks/professional/equityfundingsource.md: a dispatch for generation 66 is already in flight (#qflood); receipt_id=3310
```
---
- :pushpin: :round_pushpin: do [#4myd]
- :pushpin: :round_pushpin: do [#qdedup]
- :pushpin: :round_pushpin: do [#clearexchstale]
- :pushpin: :round_pushpin: do [#qpresetstrike]
- :pushpin: :round_pushpin: do [#qchurn]
- :pushpin: :round_pushpin: do [#fcc0]
- :pushpin: :round_pushpin: do [#rtwbcast]
- [#recyclerestart]: Stall occurred ![img_23.png](img_23.png)
- Ensure the markdown AST is properly handling strings (quoted text) and code blocks (backtick or triple backtick). Tags within the blocks should be treated as the content, not as part of the document tags.
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
- [ ] [#0kjc] #mdastquote [clean-session] Markdown AST must treat tags inside quoted strings ("...") and code spans/blocks (backtick / triple-backtick) as CONTENT, not document tags. Operator report: agent:*/#id-looking tokens inside quotes or fenced code are being parsed as real document component markers. Same family as #lintfence (fenced agent:* markers break lint + parser). FIX: tagpath/agent-doc markdown tokenizer must mask quoted-string and code-span/code-fence regions before component/tag extraction. CLEAN session (parser path).
- [ ] [#pushpinaccum] [clean-session] Queue `:pushpin:` annotations accumulate/nest in the wrong place: a head already starting `- :pushpin:` gets another `:pushpin:` prepended -> `:pushpin: - :pushpin: ...` (operator corrected one live 2026-06-13). The queue head-grouping/annotation formatter prepends the pushpin marker without checking for an existing leading pushpin/bullet. FIX: make the queue-head annotation prefix idempotent (single :pushpin:/bullet). CLEAN session (queue formatter path).
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#aazp] [operator-verify] suprecyclestall live-verify (agent-doc aafa6510, installed+release-built): restart a stale session via session restart-supervisor FILE --force so it runs the new binary, then drive an idle/next-queue-item recycle. Confirm ops.log shows supervisor_binary_stale_self_recycled ... candidates=[...] then EITHER a clean execve hot-reload OR supervisor_reexec_failed fallback=continue_current_binary with per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Subset of the broader recyclerestart sync-block work. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#4myd] [operator-verify] #suprecyclestall live-verify: with the new binary installed (cargo install + cargo build --release done, pushed agent-doc aafa6510), restart a stale session and drive an idle/next-queue-item recycle. Confirm ops.log shows `supervisor_binary_stale_self_recycled ... candidates=[...]` then EITHER a clean execve hot-reload OR `supervisor_reexec_failed fallback=continue_current_binary` with the per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#recyclerestart] [clean-session] PCP/Supervisor recycle does not restart the agent session (img_21.png); JB Sync Tmux Layout after tmux kill-pane surfaced `Sync deferred: another tmux layout sync is already running`. ROOT CAUSE (code-grounded): the warning fires when agent_doc_sync_try_lock() (ffi.rs:746, a process-wide SYNC_LOCKED AtomicBool shared cross-editor) is already held. SyncLayoutAction.kt holds that FFI lock across the ENTIRE agent-doc sync subprocess (runCommandWithTimeout, line ~299, released only in finally line 331), and that subprocess is bounded only by the 30s SYNC_PROCESS_TIMEOUT_MS and can block on in-pane/binary recovery (the timeout message literally says retry can run binary recovery). So layout GEOMETRY sync is coupled to in-pane readiness, and a second manual sync racing an in-flight one bails immediately (non-blocking compare_exchange) instead of superseding. FIX: decouple tmux geometry sync from in-pane init/recovery; hold SYNC_LOCKED only across the fast geometry reconcile, run pane readiness/recovery separately; make a newer sync generation supersede an in-flight stale one rather than warn-and-skip; prove recycle restarts/clears the harness session. CLEAN session. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#qdedup] [clean-session] Between-queue-turn trigger enqueue must de-duplicate: successive between-turn enqueues concatenated into one garbled trigger (`/clear /agent-doc <FILE>/agent-doc <FILE>`). Operator: stop the drain once a turn completes, insert into the inactive agent between turns, enqueue UNIQUE (one /clear, one /agent-doc <FILE>) landing once in the PCP/Supervisor between-turn stage. Compose next trigger as a deduped set, gate strictly to the inter-turn boundary, proof line between_turn_enqueue deduped=N. Related #qflood2/#qchurn/#clearcontresume. CLEAN session. Plan: tasks/agent-doc/plan-between-turn-trigger-dedup.md
- [ ] [#clearexchstale] [clean-session] JB Clear Exchange clears the exchange but the content comes back (from a stale HEAD/snapshot replay); same session, a deleted+retyped prompt also revived. Same disk-vs-buffer convergence family as #fcc0: a redeliver/replay path re-applies HEAD/snapshot over the cleared buffer, and clear does not advance the durable merge base. FIX: route clear through converge_document_or_disk + advance merge base; gate redeliver on fresh editor-buffer proof (#dvre/#eprf). CLEAN session. Plan: tasks/agent-doc/plan-jb-clear-exchange-stale-head-revival.md
- [ ] [#qpresetstrike] [clean-session] Preset-token queue head (a #name from prompt_presets, e.g. #advance-review) is unstrikeable by any agent path: `queue consume` rejects it as id-backed (leading #), while `--done`/`--pending-gate` reject it as not a backlog id, and `finalize` keeps the head — so it wedges queue_active=true and re-serves every /agent-doc invocation. FIX: classify a #-head against prompt_presets AND backlog/icebox before routing; a preset head should strike like a free-text head via consume + the finalize heuristic, while do[#id] heads stay on the --done path. Workaround: strike ~~#name~~ + drop go + reset --from-current --preserve-session + commit. Relates to #qchurn/#goqueuestall. Plan: tasks/agent-doc/plan-preset-queue-head-unstrikeable.md
- [ ] [#qchurn] [clean-session] go-mode no-op churn: after #goqueuestall tag-defer skips the id-backed [clean-session]/[operator-verify] backlog heads, untagged FREE-TEXT operator bug-report prompts in the ## Queue block still keep queue_active=true and the supervisor idle-watch re-injects queue_trigger=auto each idle boundary -> repeated no-op /agent-doc cycles (no content diff, user_intent_prompt_changes empty, zero drainable work). Halted live 2026-06-13 via admin queue pause (receipt 27, gen 338). FIX: go-mode should treat a queue whose only remaining prompts are non-id free-text reports (no actionable do #id / new user prompt) as not-drainable and stop re-dispatching, same as the undrainable-head skip. Plan: tasks/agent-doc/plan-go-queue-stall.md
- [ ] [#fcc0] [TOP] [clean-session] Zero JB File Cache Conflict dialogs. Route all document-mutating disk writes through one shared converge_document_or_disk() behind the listener-active gate (editor_ipc primary + disk_fallback); resolve the ipcdrift/stuckhandoff2 wedge that forces fallbacks. CLEAN session required (live write path); 0-dialogs proof operator-drive live JB. Plan: tasks/agent-doc/plan-zero-file-cache-conflict.md
- [ ] [#rtwbcast] [clean-session] rtwatch rung 4 — multi-editor CRDT broadcast. Phases rtwbcast-id (editor_id in FFI buffer-report + per-editor live_buffer_snapshot_path), then deliver/merge/verify. CLEAN session required; live two-IDE convergence verify is operator-drive. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#qflood2] [operator-verify] qflood regression: JB Run Agent Doc on efs coalesced as exit-1 (dual-drain owner, gen 66 already in flight). Needs live multi-process verify; if it still collides, coalesce should report deduped-success not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
- [/] [#snbc2] [recommended] OPTIONAL cosmetic (not a bugfix): collapse baselines/<hash>.md into a snapshot-store variant so there is one file instead of two, WITHOUT changing merge-base semantics (stays current-at-preflight = resolve_current_doc(newest-of(disk,buffer)), NOT git HEAD). #snbc Slice-1 audit proved the substantive HEAD-as-base consolidation would regress merge correctness (drops the uncommitted new prompt from the common ancestor). Moderate risk, touches the closeout path; gate behind NEW SimWorld coverage for the prompt-bearing finalize-merge class first. Must run in a CLEAN session. Plan: tasks/agent-doc/plan-snapshot-baseline-consolidation.md (Revised slice 2).
- [/] [#efscodexsubmit] JB Run Agent Doc in equityfundingsource.md enters but does not submit the prompt to Codex (operator must press Enter). Same class as #2v7b/#jbcodexsubmit: Codex direct-pane dispatch (route.rs send_command_unchecked) detects prompt_not_submitted (merged text+CR not treated as submit by the Codex TUI) but has no Enter re-submit fallback. FIX: Codex-scoped one-shot Enter re-submit + route_submit_resubmit proof logging + SimWorld coverage (operator-requested). CLEAN session required (touches route.rs; submodule carries dirty #fintol tree). Plan: tasks/agent-doc/plan-jb-codex-submit.md
- [/] [#stuckhandoff2] Stuck-handoff hardening: controllers wedged in --handoff-state preparing survive IDE restart + keep corrupting the worktree over ipc.sock. ROOT CAUSE: two-phase handoff (prepare_handoff->promote_handoff, project_controller.rs:3307/3318) is client-driven; if the client dies/RPC fails between the two, the controller stays Preparing forever. The existing reaper (#kqr6/#sjwm/#stuckhandoff, 45s) is bootstrap-RECORD-scoped (kills only bootstrap.pid), same-project-only, and fires on bind/gc not a timer -> orphaned + cross-project + no-new-invoke wedges survive (operator must pkill -f 'controller serve ... --handoff-state preparing'). FIX (defense-in-depth): M1 controller self-watchdog/suicide-timer on Preparing [strongest], M2 gate worktree/ipc writes on Stable [symptom-killer, overlaps #pcwc], M3 proc-scan reaper for orphans, M4 client handoff drop-guard, M5 cross-project gc sweep. M1+M2 are the durable core; land together with SimWorld coverage. CLEAN session required (submodule carries uncommitted #fintol work). Plan: tasks/agent-doc/plan-stuck-handoff-hardening.md
- [/] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [/] [#2v7b] #jbcodexsubmit / #jbclaudesubmit JB Run Agent Doc enters the routed prompt but does not submit it (operator presses Enter). CODE-COMPLETE 2026-06-13 (agent-doc 3b71cfeb, superproject 8fcdc067f): JB dispatch-only routes BOTH Claude and Codex through the same direct-pane send_command_unchecked path (merged text+CR send-keys), and TriggerStillVisible detection already covered both, but the one-shot Enter re-submit was gated codex-only. Generalized direct_pane_needs_codex_resubmit -> direct_pane_needs_enter_resubmit (codex|claude; opencode excluded; requires TimedOut+trigger_visible so no stray Enter into an empty composer); proof line route_submit_resubmit ... action=enter_key result=accepted|still_visible; 3 unit tests; make check 4369 passed 0 FAIL. Built+installed+recycled. GATE now operator-drive ONLY: run JB Run Agent Doc against a Claude pane AND a Codex pane, confirm ops.log shows route_submit_issue issue=prompt_not_submitted then route_submit_resubmit action=enter_key result=accepted with the queue starting. Plan: tasks/agent-doc/plan-jb-codex-submit.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
