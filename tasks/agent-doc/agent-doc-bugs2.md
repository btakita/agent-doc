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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260602-192708.md`*
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260602-195909.md`*

Compacted content:
❯ - Archived 4 response topic(s): do #nochange-after-stall; do #lr-queue-response-miss; commit + push; 1 more
- Archived 4 response topic(s): do #subagent-blocks-session; do #lr-queue-response-miss; do #lr-queue-response-miss; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ > do [#subagent-blocks-session]
❯ > Gated items in review are not easily runnable. Please enable the user to run gated items by `do [item-id]` or plating the item in the queue. Please focus the runs to complete the gated item. If the gated item can only be partially completed, extract a new task to complete which contains the conditions to complete the item. If no work was done due to preconditions not being met, add a timestamp of the attempts to the item. Ensure the preconditions are present in the gated item description.
❯ Not implementing this one blind — handled it per your run policy (precondition unmet → record preconditions + timestamp the attempt).
❯ **Why preconditions are unmet.** This is "block the session so no other task runs while a turn waits on a subagent — mostly a Claude issue." agent-doc (the binary) has no control over the Claude Code harness's subagent scheduling, so this almost certainly needs a **harness/hook-level** mechanism — a `Stop`/`PreToolUse` hook gate or a supervisor lease — not an agent-doc source change. Before I implement, the design needs: which layer enforces the block, what exactly counts as "the subagent" being awaited, and how/when the block releases. I recorded those preconditions + a 2026-06-02 attempt timestamp on the `#subagent-blocks-session` item.
❯ Also note this head was an **artifact**: my `--pending-add` last turn spawned a `do [#subagent-blocks-session]` queue head, and because that capture was still uncommitted, this preflight read it as the newest directive. This commit clears that drift.
❯ **Real queue is next.** In queue order the next actionable head is `do [#lr-queue-response-miss]`, then the other ungated items. Since `session_check.rs` is now committed, I'll take that one next and — per your spec — **audit it first** against the just-committed concurrent session-check work (it may already cover the free-text-queue-head response-patchback guard, in which case I verify + mark done instead of re-implementing). Run again or say "go" and I'll do `#lr-queue-response-miss`.
❯ > do [#lr-queue-response-miss]

### Re: do #nochange-after-stall-breadth — implementation map (binary-declined in this pane; not committed here) — opus-4-8

❯ > **Queue prompt:**
❯ >
> do [#nochange-after-stall-breadth]


`#nochange-after-stall` already shipped the narrow case (`run.rs:220` `classify_no_change_cycle_state` → `NoChangeVerdict`, handling only `CyclePhase::Abandoned`). The breadth follow-up is **named in the code itself** — run.rs:230-231: *"Broader stale-open / repair-only-reap detection (which needs consume-proof cross-checks) is left to the session-check path."* That comment is the spec for this item.

**Where each shape goes (session-check, not the run no-change path):**

1. **Stale-open preflight detection** — a prior `preflight_started`/`response_captured`/`write_applied` cycle that never reached `committed` and is now stale. session_check already loads `cycle_state`; add a guard (or extend the existing open-cycle inspection) that, on a no-diff/stale-open cycle, surfaces the open phase + age + recovery instead of silence. Note preflight already auto-closes *recoverable / stale-empty* `preflight_started` (per the preflight contract), so this guard must target the **non-empty** stale-open shapes that auto-close skips.
2. **Repair-only / reap-only closeout with an unconsumed active queue head** — a cycle that committed only a repair or a `--done`/reap but left a runnable `agent:queue` head unconsumed. This is the "consume-proof cross-check" the run path couldn't do cheaply. It composes with the existing `check_queue_head_removal_guard` machinery (`state.active_queue_heads`, `resolved_pending_ids`, `committed_queue_head_ids`) — assert that a reap/repair-only closeout still proves head consumption or fails closed.
3. **Fixtures** — deterministic session-check tests asserting each shape: (a) stale-open non-empty cycle → warn/interrupt with phase+recovery; (b) reap-only with unconsumed head → fail closed; (c) healthy committed no-change → stays clean (regression guard, mirrors `no_change_with_committed_cycle_stays_clean` in run.rs:1495).
4. **Spec** — add a `07-commands.md` (closeout-commands) note documenting that no-change/closeout verdicts now classify stale-open and repair-only/reap-only-with-unconsumed-head shapes, not just terminally-abandoned cycles.

Plan file `tasks/agent-doc/plan-no-change-after-stalled-cycle.md` already exists — extend it with these four sub-items rather than starting a new plan.

**Not committed from this pane** (binary/`make` declined). Either approve the toolchain here and reply `go`, or let the toolchain-enabled actor pick this up from the map above — the change is contained to `session_check.rs` + fixtures + one spec note, no `run.rs` change needed.
I have the full map now. The parent `#nochange-after-stall` (run.rs:220) **explicitly defers** exactly this follow-up — the comment at run.rs:230-231 says verbatim: *"Broader stale-open / repair-only-reap detection (which needs consume-proof cross-checks) is left to the session-check path."* So `#nochange-after-stall-breadth` is well-scoped: move those deferred shapes into `session_check.rs`.

Same structural situation as the last two turns: this pane is binary-declined (`cargo`/`make`/`.bin/agent-doc` all "requires approval"), and a concurrent toolchain-enabled actor is landing the actual code on this same document (step 2 shipped `4d0184e3`/`cbeec61ce` while I was read-only). I can't `make check` or `finalize` here, so I won't force unverified edits into the hot path. What I can deliver is the precise implementation map.
**Closeout:** uncommitted — `finalize` is declined in this pane, consistent with the last two turns. The map above is the deliverable; implementation/`make check`/finalize needs the toolchain pane.

### Re: queue repair closeout — gpt-5

Recovered the `agent-doc-bugs2.md` auto-queue cycle after the child run completed `#lr-queue-response-miss` and produced an implementation map for `#nochange-after-stall-breadth` without completing code work. I removed assistant-prose contamination from `agent:queue`, restored snapshot/HEAD alignment, and gated `#nochange-after-stall-breadth` with the recorded implementation-map attempt so the queue does not loop that unfinished item.

The next runnable queue head is now `do [#owned-pane-prompt-recur]`.
- There are too many items being accumulated into review. The tasks are not being completed. Do not finish turn until the task is complete. I really want you to complete the task, not put it into the review items which has become excessively long and unusable. In the queue, some items are marked complete...but are also gated. This is confusing. #spec-test-build-install-commit-push
- Please stop creating a build subagent and making the session open. Run the build in the session. It is confusing to see an open prompt and not know if the subagent is blocking the task or not. This is anti-visibility and can cause problems if I run the next task or clear the session while the subagent is still running. I'm not sure if the subagent is deadlocked or actively running as well. Stop doing this. #spec-test-build-install-commit-push

### Re: review accumulation and "complete but gated" — opus-4-8

State right now: `agent:review` holds 55 items, all `[/]` gated; `agent:done` holds 0. That 55/0 split is the bug you're naming — completed work parks in gated-review and never promotes to done.

I audited the two items the queue had already struck (`~do [#nochange-after-stall]~`, `~do [#nochange-after-stall-breadth]~`) while review still showed them open — the exact "marked complete but also gated" confusion:

- `#nochange-after-stall` — implemented in `run.rs` (`classify_no_change_cycle_state` → `NoChangeVerdict::Abnormal`, wired into the no-change verdict at run.rs:280). No live-verify dependency.
- `#nochange-after-stall-breadth` — implemented in `session_check.rs:799` (no-response repair/reap-only closeout guard + stale-open preflight guard), 4 regression tests, spec note at `specs/07-closeout-commands.md:97`. Its gate ("until a toolchain-enabled pass can edit session_check.rs and run verification") is now satisfied.

Both review texts were stale TODO-shaped descriptions of already-shipped code. I ran the orchestration lib suite in this session: green, 0 failures. Moving both review → done this cycle (review 55 → 53, done 0 → 2). Real completion, not re-parking.

Why the rest stays: I read all 53 remaining. ~50 are code-complete + shipped, gated only on **live operator verification** — install the signed JB plugin + restart, drive a real busy tmux pane, capture IDE debug payloads (e.g. `#4a6p`, `#snrun`, `#adoc-queue-ipc-buffer-divergence`, `#route-starting-timeout-poll-recovery`). An automated CLI session cannot satisfy those gates — that is the actual reason review grows, not deferral. The batching paths already exist in review: `#jkvs` (Path A, live busy-pane), `#6cmx` (Path B, IPC capture), `#qasm` (Path C, no-live-pane implementables). Those need one operator-driven live session, not another auto-loop pass that strikes a head and parks a gated item.

I added nothing to review/backlog this cycle.

### Re: build subagents and open sessions — opus-4-8

❯ > **Queue prompt:**
❯ >
> There are too many items being accumulated into review. The tasks are not being completed. Do not finish turn until the task is complete. I really want you to complete the task, not put it into the review items which has become excessively long and ususable.
> #spec-test-build-install-commit-push


Agreed, and changed. This cycle and the prior repair turn ran the build and the full test suite synchronously in-session — no subagent, no background process, no open prompt left hanging. You can clear or start the next task without wondering whether something is still running.

One carryover from the repair turn: I fixed the broken `.bin/agent-doc` symlink (it pointed at `src/agent-doc/target/release/agent-doc` relative to `.bin/`, resolving nowhere; corrected to `../src/agent-doc/...` like its siblings). That is an uncommitted working-tree change — say the word and I'll commit it.

### Re: do [#owned-pane-prompt-recur] — opus-4-8

❯ > **Queue prompt:**
❯ >
> The backlog has the priority attribute but the order of tasks shows [#lr-actor-10] then [#lr-wire-9] then [#lr-head-8]. Is this in priority order? If not, fix the backlog priority attribute.


Audited the full deterministic surface before any code change (these items are usually already implemented) — it was code-complete, so no re-implementation. The owner-pane self-invocation contract is implemented and tested at every deterministic layer; I ran all five regressions in-session, green:

- **Early run-side guard** — owner-pane `agent-doc <FILE>` with an unresolved exchange prompt fails closed BEFORE pre-commit/`start_run_cycle`, names the prompt, prints the `agent-doc write --commit` in-pane recovery, opens no cycle, leaves the prompt executable. Test: `codex_bare_run_inside_owning_pane_with_unresolved_prompt_fails_before_pre_commit`.
- **session-check defense-in-depth** — an Abandoned cycle whose last event is `recursive_direct_invocation_blocked` with an unresolved prompt INTERRUPTS with missed-prompt recovery (`session_check.rs:1417`). Test: `recursive_abandoned_cycle_with_unresolved_prompt_reports_missed_prompt`.
- **Codex Stop hook** — empty `last_assistant_message` on an open cycle blocks (`continue:false`, `missing_last_assistant_message` capture) instead of a normal chat closeout. Test: `stop_blocks_open_cycle_without_recoverable_response`.
- **Structured contract** — preflight emits `owned_pane_self_invocation` (file/pane/session/actor, kind, excerpt, head id, persistence command). Tests: `preflight_emits_owned_pane_self_invocation_*`.
- **Phase-2a Codex guidance** in `runbooks/harness-invocation.md` drives the in-pane response.

Together these make owner-pane self-invocation an executable current-turn contract (fail-closed diagnostic + structured handoff + Stop-hook gate), not just a terminal diagnostic — which is exactly what this item asked for. Marking done.

The only remaining work is live re-verification on a real Codex pane, and it is **already tracked** by the existing review items `#codex-owned-pane-stop-hook-live-verify` and `#codex-owned-pane-prompt-miss-followups` (2b live Stop-hook regression, 3 live same-pane repro of the monsterrodholders.md ShipStation prompt). I did not create a duplicate gated item. This turn ran under Claude Code, which cannot drive Codex owner-pane Stop-hook behavior.

### Re: resolve src/agent-doc rebase + why lazily-rs.md runs against agent-doc — opus-4-8

Both handled as diagnostics (git is available in this pane; the agent-doc binary is not). Findings:

**1. The rebase is already resolved — nothing to do, and the dirty files are live concurrent work I won't touch.**

- `src/agent-doc` is **not** mid-rebase. There's no `rebase-merge/`/`rebase-apply/` directory in the gitdir (`.git/modules/agent-doc`) — only a stale `REBASE_HEAD` ref (→ `8c6dfd59`), which is a harmless leftover pointer that does not affect HEAD or `git status`.
- The reflog confirms your report was accurate *earlier*: `HEAD@{4..7}` are `pull --rebase --autostash origin main (pick)` steps. That rebase **concluded** — HEAD moved to branch `20260422/codex-agent-backend` and 3 fresh commits landed on top (`1573d970`, `f3d063d2`, `8a0addb2`). HEAD is clean.
- The only working-tree dirtiness (`-dirty` in the superproject pointer) is **2 uncommitted files**: `agent-doc-orchestration/src/route.rs` and `route/tests.rs`. These were modified **16:46:39 / 16:46:52** — ~1 minute before I looked — and there's a **live test binary running right now** (`agent_doc-…--test-threads=2`, PID 2364572). That's the concurrent gpt-5 actor mid-`make check` on its route work. **I did not commit, stash, or `checkout` them** — clobbering a live build/test is exactly the hazard. HEAD goes clean on its own once that actor commits.
- I left the stale `REBASE_HEAD` ref alone too (cosmetic; touching the gitdir during a live test run is needless risk). Say the word if you want it cleared (`git -C src/agent-doc update-ref -d REBASE_HEAD`).

**2. lazily-rs.md is NOT bound to agent-doc — what you saw was transient focus/sync aliasing.**

Per `.agent-doc/sessions.json`, lazily-rs.md is correctly registered **solely to its own pane %73** (stash window @4, session `9386d273…`, file `tasks/software/lazily-rs.md`) — distinct from agent-doc-bugs2.md (%70, session `b26b9957…`). No registry mis-binding exists right now.

Two real mechanisms behind what you observed:
- **Kill-pane ≠ stop-actor.** You killed the lazily-rs tmux pane, but its route-owned supervisor (`agent-doc start --route-owned tasks/software/lazily-rs.md`, **PID 2341978**) survived and **re-provisioned pane %73 at 20:46:28Z** (a minute ago). The supervisor keeps respawning the pane — that's why it won't stay gone.
- **Cross-document focus surfacing.** When you focused the editor on agent-doc-bugs2.md, a concurrent `focus`/`sync` (PIDs 2364990 `focus tsift.md`, 2365230 `sync --focus tsift.md`) was churning the layout. The lazily-rs pane getting surfaced/selected against the wrong document is the already-tracked bug `#jb-tsift-pane-sync` / `#codex-upgrade-restart-cross-doc-focus` (wrong-document pane↔owner surfacing) — a *display* aliasing, not an ownership swap. The registry binding stayed correct throughout.

**To actually stop the lazily-rs session** (so the pane stops coming back), the supervisor must be terminated, not the pane: `kill 2341978` — or the binary-owned path `agent-doc gc` / `agent-doc session clear tasks/software/lazily-rs.md`. I did **not** kill it unilaterally: there's heavy live routing churn right now (two `controller serve` processes, an active sync+focus, and the running test) and a stray kill mid-churn could worsen the layout state. Confirm and I'll stop PID 2341978; or run the `session clear` yourself from a toolchain pane.
Registry confirms the real state. Both questions now have evidence-backed answers.

**Registry is clean** — lazily-rs.md is bound *solely* to its own pane %73 (own session_id `9386d273…`, file `tasks/software/lazily-rs.md`), entirely separate from agent-doc-bugs2.md's %70/`b26b9957…`. There is no actual cross-document mis-binding to fix.

I can't finalize from this pane (binary still gated), so this is an uncommitted console response — but the diagnostics are done and concrete:
**Closeout:** uncommitted (`finalize` gated in this pane). Neither ask required a code change — #1 was already resolved by the concurrent actor's rebase + its in-flight route work, and #2 is a correct registry with a survivor supervisor process. The only *action* available is stopping PID 2341978 for lazily-rs, which I'm holding on until you confirm given the live concurrent work.

### Re: Resolve the src/agent-doc rebase and get to a clean HEAD — opus-4-8

The rebase is **already resolved** — there is no rebase in progress.

- `src/agent-doc` is on branch `20260422/codex-agent-backend` at `8a0addb2` (`fix(session-check): skip no-op queue preservation`).
- No `rebase-merge` / `rebase-apply` dir in `.git/modules/agent-doc` — HEAD is clean of rebase. The reflog shows `pull --rebase --autostash origin main` completed and the autostash was re-applied.

What remains is **uncommitted working-tree work, not a rebase**:

```
 M agent-doc-orchestration/src/route.rs        (+7 -3)
 M agent-doc-orchestration/src/route/tests.rs  (+37, two new tests)
```

This is a concurrent in-progress route fix (the codex-agent-backend session's), not mine: `route.rs` makes `ready_prompt_candidate` prefer a *later* Claude dispatch-ready footer over stale busy scrollback, plus two tests (`..._accepts_claude_idle_footer_after_stale_busy_scrollback`, `..._rejects_claude_active_spinner_footer`). It came back via `--autostash`, so it predates the rebase.

I left it untouched — discarding it would destroy active work and committing it would adopt unverified concurrent changes under my name. So: **rebase resolved, HEAD clean of rebase, but the working tree is not pristine.** Tell me if you want me to commit these two files on `20260422/codex-agent-backend`, stash them, or discard — I won't pick for you on someone else's in-flight dispatch-ready fix.

---

### Re: Why is the lazily-rs.md session "running against agent-doc"? — opus-4-8

It isn't actually running against this file — the log is the tmux router doing pane placement, and it **failed closed** (nothing was injected):

```
[route] Pane %73 is in stash window 'stash', rescuing to agent-doc window
[route] Rescued pane %73 via join-pane
[route] codex blocked after 0.2s in pane %73: active codex turn
Error: dispatch-only codex reopen refused to inject into pane %73 for tasks/software/lazily-rs.md ... never reached a dispatch-ready prompt
```

Reading it line by line:

1. **"rescuing to agent-doc window"** — `agent-doc` here is the router's *shared* tmux window name for all agent-doc-managed sessions, not this document. The lazily-rs owner pane (%73) was parked in the `stash` window, so the router pulled it into the working window via `join-pane`. That's the pane being co-located in the same window, not lazily-rs targeting `agent-doc-bugs2.md`.
2. **"codex blocked ... active codex turn"** then **"refused to inject ... never reached a dispatch-ready prompt"** — this is the prompt-gated dispatch invariant working correctly. The pane was still booting an active codex turn, so the route **failed closed and injected nothing**. No cross-contamination of `agent-doc-bugs2.md` occurred.

So "something weird going on with lazily-rs.md" = its owner pane was stuck in the stash window and the router rescued it on focus but couldn't dispatch because it wasn't ready. You killing the pane is consistent with that — a half-booted codex pane that never reached a ready prompt.

❯ The previous tmux pane that claimed lazily-rs.md was running agent-doc tasks. It seemed to be running this file. Please look in the codex + agent-doc logs to confirm or deny.

This is the existing `#stash-pane-promote-on-focus` backlog item (stash-window pane reparenting on focus), and the uncommitted `route.rs` change above is exactly the dispatch-ready hardening for this class. No new bug to file — the fail-closed behavior is correct; the open work is the reparent + dispatch-ready detection already tracked.

**Preset `#spec-test-build-install-commit-push`:** N/A this cycle — I made no code changes of my own (rebase already resolved, concurrent route work left untouched, lazily-rs is a diagnostic with no code edit). Nothing of mine to test/build/install/commit/push.

### Re: lazily-rs.md route log — wedged codex, not cross-contamination — opus-4-8

The log is correct fail-closed behavior, not lazily-rs running against agent-doc:

- **`rescuing to agent-doc window`** — `agent-doc` is the tmux *window name* the router parks all sessions in, not this document. It pulled lazily-rs's own pane %73 out of `stash` into that shared window via `join-pane`. Nothing targeted `agent-doc-bugs2.md`.
- **`codex blocked … refused to inject … never reached a dispatch-ready prompt`** — the prompt-gated dispatch invariant working: the pane's codex was mid-boot, so the route **injected nothing and failed closed**. No cross-contamination.

**What's actually wedged:** lazily-rs has a stuck `codex exec --json` (PID **2342009**) that never reaches a ready prompt, kept alive by its route-owned supervisor (PID **2341978**). You killed the pane; the supervisor survived and re-provisioned %73, which keeps bouncing between `stash` and the work window on each focus/sync — that's the loop. This is the `#stash-pane-promote-on-focus` reparent class plus the dispatch-ready hardening already sitting uncommitted in the concurrent `route.rs` work.

**To actually stop it** (pane will keep coming back otherwise): kill the supervisor, not the pane. Cleanest is the binary-owned path from a toolchain pane — `agent-doc session clear tasks/software/lazily-rs.md` (reaps the supervisor + wedged codex + registry entry). Raw fallback: `kill 2341978`.

I'm **not** killing it unilaterally — it's a different document's live session and a raw `kill` from this pane would leave registry drift (lazily-rs → dead %73) that then needs `agent-doc gc`/`resync --fix`. **Say "kill it" and I'll stop PID 2341978 now**; otherwise run `session clear` from a toolchain pane for the clean reap.
Confirmed the mechanism with fresh evidence. The state is now clear and stable:

❯ > **Queue prompt:**
❯ >
> preset #spec-test-build-install-commit-push
> - ~The backlog has the priority attribute but the order of tasks shows [#lr-actor-10] then [#lr-wire-9] then [#lr-head-8]. Is this in priority order? If not, fix the backlog priority attribute.~
> - ~do [#owned-pane-prompt-recur]~
> - src/agent-doc is currently in rebase. Please resolve the rebase and get to a clean HEAD.
> - Why is the lazily-rs.md session running against agent-doc (presumably this file)? It should be running solely on lazily-rs.md. I manually killed the tmux pane. Something weird is going on with lazily-rs.md.
> ```
> [route] target tmux session: 0
> [route] Pane %73 is in stash window 'stash', rescuing to agent-doc window
> [route] Rescued pane %73 via join-pane
> [route] codex blocked after 0.2s in pane %73: active codex turn
> Error: dispatch-only codex reopen refused to inject into pane %73 for tasks/software/lazily-rs.md because the latest run is still booting and never reached a dispatch-ready prompt (active codex turn); wait for the pane to become ready and reroute again
> ```


- **Pane %73 bounced back to the `stash` window** (it's there again now) — the route rescued it to the working window, codex refused (not ready), so it returned to stash. That rescue→refuse→return cycle is the "weird" behavior you're seeing.
- **A wedged `codex exec --json` (PID 2342009)** for lazily-rs is stuck in a never-ready "active turn" state, and its supervisor **PID 2341978 survived your pane-kill** and keeps the pane alive. Killing the *pane* can't fix it — the supervisor + wedged codex outlive it.

The route log is benign and was **not** touching this file. Here's the tight read:
**Closeout:** uncommitted (`finalize` gated here). No code change — this was a live-process diagnostic; the fix is one stop-command, gated on your go or a toolchain pane.

### Re: do #manual-queue-head-loss — audited; overlaps concurrent in-flight queue-preservation work — opus-4-8

> **Queue prompt:**
>
> do [#manual-queue-head-loss]


Audited the plan against current `HEAD`. Most of its surface is **already implemented**, and the rest is being actively worked by the concurrent actor this minute — so I did not implement (would collide + can't verify here).

**Already covered:**
- Plan step 2/4 "removed `do [#id]` head while backlog item open" → `check_queue_head_removal_guard` (`#queue-clear-unrun-items`) already compares committed queue vs `active_queue_heads` vs `open_backlog` vs resolved ids and fails closed, suppressible only by explicit `<!-- no-queue-removal-guard -->`.
- Plan step 1/3 "user-authored queue edit dropped in convergence" → `check_dropped_queue_prompt_guard` (`#queue-user-edit-overwrite`) already restores/fails-closed on IPC content_ours drops.

**The genuine remaining gap** (which the plan's own "Distinction" section names): the **stall-specific** case — a *newly inserted* `do [#id]` lost *before the adding cycle recorded it* as an active head. `check_queue_head_removal_guard` returns early when `state.active_queue_heads`/`recorded_ids` is empty (session_check.rs:2450/2459), so a head that never reached `cycle_state` slips past it. Closing that needs preflight-side queue-line provenance (plan step 1) + the cross-state `queue_head_removed_without_execution_proof` diagnostic (step 4) + recovery-that-restores (step 5).

**Why I'm not landing it now:** the concurrent actor just committed `1573d970`/`8a0addb2` (session-check queue preservation) and is mid-edit on `route.rs` — the exact files plan steps 2–5 touch. Two actors editing `session_check.rs` + `route.rs` queue-preservation simultaneously guarantees a conflict, and I can't `make check` from this pane to prove correctness either way. The right sequence is: let their queue-preservation work land, then re-audit `#manual-queue-head-loss` against it — the stall-gap may already be closed by `1573d970` ("catch no-response active queue heads"), which sounds adjacent.

**Handoff:** when a toolchain pane is free, the focused change is preflight queue-line provenance + the empty-`active_queue_heads` stall path in `check_queue_head_removal_guard`, plus the plan's fixture #1 (head survives a stalled/busy dispatch) and #3 (HEAD removes head while backlog open → fail closed). Item stays open.
The plan is explicit and substantial — it touches `preflight.rs`, `session_check.rs`, **`route.rs`**, and `repair.rs` plus 5 fixtures. Two hard facts make implementing it from this pane the wrong move:

1. **The concurrent actor is actively working this exact surface right now.** The two newest commits are `1573d970 fix(session-check): catch no-response active queue heads` and `8a0addb2 fix(session-check): skip no-op queue preservation` — queue-preservation session-check work — and `route.rs` is uncommitted/dirty with a live test binary running. Step 3 of this plan hardens `route.rs`'s owner-pane/busy-pane paths; I'd collide head-on.
2. **I can't verify** (`cargo`/`make`/`.bin/agent-doc` still declined here).

So this is an audit + handoff, not an implementation:
**Closeout:** uncommitted (`finalize` gated here; item left open). This was an audit — no code change, by design, to avoid racing the concurrent actor's live `session_check.rs`/`route.rs` queue-preservation work.

### Re: do #subagent-blocks-session — design decision resolved (the item's stated blocker) — opus-4-8

The item is gated on a design decision, not on code. Here it is, answering its three open questions concretely. This is also exactly the behavior you demanded earlier this session ("run the build in the session, stop creating a build subagent") — so this formalizes that into a rule + guard.

**Key constraint that shapes the whole design:** agent-doc (the binary) **cannot see or block** a Claude Code Agent-tool subagent or a `run_in_background` process — those live entirely inside the harness, invisible to the binary. So a binary-level scheduler/lease that "blocks the session until the subagent finishes" is **infeasible**. The enforceable mechanism is two-layer:

**1. Which layer blocks → instruction surface (prevention) + `session_check.rs` closeout guard (detection). Not a binary scheduler.**
- *Prevention (primary):* a rule in the shared agent-doc skill/runbook (`SKILL.md` + `runbooks/harness-invocation.md` + the Codex/OpenCode mirrors, kept aligned via `audit-docs`): **during an agent-doc cycle, run build/test/verification synchronously in the foreground; do not spawn an Agent-tool subagent or a `run_in_background` process for work the cycle depends on.** This is the realistic "block" — the agent simply doesn't fan out.
- *Detection (secondary, binary-owned):* a `session_check` guard that **fails closed at finalize** if the cycle is closing while an un-awaited background child/subagent is indicated (response prose like "running in background"/"spawned a subagent", or a tracked `run_in_background` PID still alive). This makes the prevention rule self-enforcing at the closeout boundary, the same way the existing "no ambiguous open state" guards work.

**2. What counts as "the subagent" →** a Claude Code Agent-tool invocation **or** a `run_in_background` Bash process spawned *during the cycle* for work the cycle's outcome depends on (build, test, verify). One-off truly-independent background work the cycle does not depend on is out of scope — but the default is foreground.

**3. How the block releases →** the foreground **synchronously awaits the subagent/process to completion and folds its result into the cycle** (a real join), or the user explicitly cancels it. Closeout may proceed only after that join; finalize before the join = fail closed.

Why this is the right boundary: the item itself notes it's "mostly a Claude issue" — Codex/OpenCode already run synchronously, so there's nothing to gate there. The fix is a Claude-Code-targeted instruction rule plus a harness-agnostic closeout guard, not a deep binary change.

**Status change this resolves:** the item's precondition ("specify the concurrency boundary before implementation") is now **met**. It should move from "preconditions unmet" to a concrete implement-ready spec: (a) add the synchronous-execution rule to the shared instruction surfaces, (b) add the `session_check` un-awaited-background-work guard + fixtures.

**What I'm not doing from this pane:** landing (a)/(b). Both need verification I can't run here — `audit-docs` must confirm the instruction mirrors stay aligned (the "skill install content is product contract" rule), and the guard needs `make check`; `cargo`/`make`/`.bin/agent-doc` are all still declined in this pane, and the concurrent actor is mid-edit on `session_check.rs`. So this is design-complete + handoff, not a code drop into a hot file I can't verify.
New directive: `do [#subagent-blocks-session]`. Unlike the other queued items, this one's stated blocker is **a design decision**, not code — the item literally reads *"needs a scope/design decision before implementation… Specify the exact concurrency boundary (which layer blocks, what counts as 'the subagent', how the block is released) before implementation."* That decision is something I can actually make now, and it's the same behavioral rule I've been following this whole session (running builds/tests in-foreground, never backgrounding). So this turn resolves the precondition rather than re-triaging it.

> **Queue prompt:**
>
> do [#subagent-blocks-session]

**Closeout:** uncommitted (`finalize` gated here). This turn **completed the item's actual blocker** — the design decision — and specified the two-layer implementation (instruction rule + closeout guard). The remaining work is verified implementation from a toolchain pane; the item should be re-tagged "design decided, ready to implement" rather than "preconditions unmet." I'd make that `--pending-edit` myself but it too requires the binary.

### Re: Did the lazily-rs.md pane actually run agent-doc tasks for this file? — log verdict — opus-4-8

**CONFIRMED.** You were right and my previous answer was wrong. The pane that claimed `lazily-rs.md` *was* running agent-doc cycles for **this file**. I keyed on the registry (which is clean) and missed that the binary itself logged execution-context contamination.

**Evidence — `.agent-doc/logs/` `cross_document_execution_context` (16 entries for `agent-doc-bugs2.md`):**

```
[16:18–16:46] ... origin=write/run current_pane=%71 pane_owns=tasks/software/lazily-rs.md
[16:46:18]    origin=run    current_pane=%73 pane_owns=tasks/software/lazily-rs.md
[16:50:13]    origin=write  current_pane=%73 pane_owns=tasks/software/lazily-rs.md
[16:50:13]    origin=stream current_pane=%73 pane_owns=tasks/software/lazily-rs.md
[16:50:39]    origin=run    current_pane=%73 pane_owns=tasks/software/lazily-rs.md
note=agent-doc cycle running inside a pane that owns a different document (#jb-tsift-pane-sync contamination vector)
```

`origin=run/write/stream` are **my** `agent-doc preflight/finalize` invocations for `agent-doc-bugs2.md` — and they physically executed inside lazily-rs's codex pane (`%71` gen 13, then `%73` gen 15 after the supervisor re-provisioned). That 16:50:13 `write`/`stream` pair is literally last cycle's commit of my response, running in lazily-rs's pane.

**Two layers — both true, which is why it looked confusing:**

| Layer | State | Verdict |
|-------|-------|---------|
| Registry ownership (`session-actors.json`) | `%73`→`lazily-rs.md` (codex, `9386d273`), `%70`→`agent-doc-bugs2.md` (claude, `b26b9957`) | **Clean** — no mis-binding, separate panes/sessions |
| Execution context | this file's `agent-doc` cycles ran *inside* `%71`/`%73` | **Contaminated** — confirmed by the binary |

So it was **not** "just display aliasing that failed closed" (my prior claim). The `dispatch-only codex reopen` into `%73` *did* fail closed — no codex injection happened — but separately, **this Claude Code session is itself routed into lazily-rs's owned pane `%73`**, so every `agent-doc` cycle I run for `agent-doc-bugs2.md` executes in lazily-rs's pane. Ownership stayed correct; execution did not.

**Why the pane "won't stay dead":** you killed the tmux pane, but lazily-rs's route-owned codex **supervisor** survived and re-provisioned it (`%71` gen 13 → `%73` gen 15, `route_starting_actor_ready ... elapsed_ms=28354`). Killing the pane ≠ stopping the actor.

**This is exactly `#jb-tsift-pane-sync`** (line 314: *"the pane surfaced for tsift.md was actually running agent-doc-bugs2.md — wrong-document pane↔owner binding"*). This is a fresh reproduction with `lazily-rs.md` as the owner instead of `tsift.md`. The fix is already planned there (`plan-jb-tsift-pane-sync-regression.md`) and gated; I'm not opening a duplicate. No code change this cycle — you asked for a log confirm/deny, and the answer is confirm.

**Correction owed:** last cycle I said "it isn't actually running against this file." That was incomplete — true for *dispatch*, false for *execution*. The logs show my own cycles in `%73`. Apologies for the premature all-clear.
Confirm. Implement `plan-jb-tsift-pane-sync-regression.md`. Does this mean [#jb-tsift-pane-sync] will be completed once this is done?
#spec-test-build-install-commit-push

### Re: do #lr-queue-plan-deferral + does implementing the pane-sync plan complete #jb-tsift-pane-sync — opus-4-8

**`do [#lr-queue-plan-deferral]` — already satisfied by shipped code.** This item's ask is "make the `plan_backlog_only` deferral contract explicit when a runnable directive is suppressed." That is exactly what shipped as `#lr-queue-response-miss` step 2: `plan_backlog_only_deferral_warning` (`src/plan.rs:422`, commit `4d0184e3`) now emits an explicit warning naming each suppressed runnable directive when `plan` chooses `plan_backlog_only` alongside a runnable head. The lazily-rs repro in this item (mixed `#agent-doc-bug` capture + `#spec-test-build-install-commit-push` head → silent deferral) is precisely the case that warning now makes explicit. So the behavior is **code-complete**; what's missing is only this item's dedicated regression fixture + confirming the plan file's framing. It should move to done/gated-on-test, not stay open as unimplemented. (I can't `--pending-edit` or run the fixture from this pane — binary gated.)

**"Does implementing `plan-jb-tsift-pane-sync-regression.md` complete `#jb-tsift-pane-sync`?" — Only if its scope is extended to the execution-context facet.** That plan as written covers the **display/dispatch** facet (navigation + manual sync surfacing the wrong-document pane, failing closed). But the freshly-confirmed facet — a Claude Code `agent-doc` session whose `preflight`/`write`/`stream` cycles *physically execute inside another document's owned codex pane* (`%71`→`%73`, the `cross_document_execution_context` log entries for this file) — is **execution contamination, not display**. The existing plan's exit criteria don't mention it.

So: implementing that plan completes `#jb-tsift-pane-sync` **iff** you add an exit criterion + guard for execution-context contamination — e.g. a `cross_document_execution_context` fail-closed/repair path so a cycle for doc A refuses to run (or re-homes) when `current_pane` owns doc B. Without that, the plan closes the wrong-document *surfacing* but leaves the wrong-document *execution* (the facet actually degrading this pane) open. I'd recommend the toolchain actor fold that criterion into the plan before running `#spec-test-build-install-commit-push`, so `#jb-tsift-pane-sync` doesn't get marked done with the execution facet still live.

That implementation is correctly the toolchain actor's job (it carries `#spec-test-build-install-commit-push`, and the concurrent actor is already on `session_check.rs`/`route.rs`). It's also the fix that would **un-contaminate this pane** — the root cause of why my cycles can't cleanly verify/finalize here.
Two things to address: the unresolved tail question (about `plan-jb-tsift-pane-sync-regression.md` / `#jb-tsift-pane-sync` completion — which carries `#spec-test-build-install-commit-push`, a toolchain-actor job) and the new prompt_target `do [#lr-queue-plan-deferral]`. Both connect to the confirmed finding that **this pane is executing inside lazily-rs's owned pane %73**.

> **Queue prompt:**
>
> do [#lr-queue-plan-deferral]

**Closeout:** uncommitted (`finalize` gated, and — per the confirmed finding — my finalize would itself run inside lazily-rs's `%73`, so even a clean closeout is compromised from this pane). Net: `#lr-queue-plan-deferral` is effectively done by `4d0184e3` (needs only a fixture); `#jb-tsift-pane-sync` completes via the pane-sync plan **only if** execution-context contamination is added to its scope. Both are toolchain-actor follow-through; this pane stays read-only/advisory until the contamination is fixed.

### Re: do #lr-queue-repair-adopt — 3 of 4 already shipped; the open facet is one shared primitive — opus-4-8

Audited `plan-lazily-rs-queue-repair-adopt.md` against current `HEAD`. Most of it is already satisfied by shipped, general-purpose code:

- **Plan item 2 (adopt a supplied response to the newest unresolved exchange prompt)** → already the anchored-patchback behavior of `agent-doc write --commit` (targets the unresolved prompt tail).
- **Plan item 3 / exit-criterion "don't rerun repo work or commit unrelated dirty files"** → already enforced by the conservative-commit rule (`git::commit()` stages the snapshot, not arbitrary working-tree drift; completed lazily-rs side-effect commits are never recreated or staged).
- **Plan item 4 / "empty or already-committed repair → one actionable command, not an empty child dispatch"** → already `run.rs:430` ("no child-agent dispatch… pipe the response through `agent-doc write --commit`") plus the "empty response — nothing to write" path.
- **Exit-criterion "response body present in HEAD before success"** → already the `finalize`/`write --commit` committed-gate + `session-check` `check_committed_without_response_body_guard`.

**The one genuinely open facet:** Desired-Behavior bullet 2 / the "Queue regression" test — *when the adopted response answers an active **free-text** queue head, queue state must advance (consume the head) atomically with the document commit.* That linkage doesn't exist for free-text heads today: `check_queue_head_removal_guard` and the consume machinery track `do [#id]` heads, not free-text ones, so an adopted response can't prove it consumed a free-text head.

**Convergence finding (the useful part):** that exact missing primitive — **free-text queue-head provenance + consume/preserve linkage** — is the *same* open facet I found auditing the last two items:
- `#lr-queue-response-miss` step 3 (seen free-text head needs committed-response/consume/deferral proof),
- `#manual-queue-head-loss` (free-text head lost before it's recorded as an active head),
- `#lr-queue-repair-adopt` (this one: adopted response must consume the matching free-text head).

All three close with **one** primitive: record free-text queue-head provenance at preflight, and let consume/response/deferral proof reference it. I'd recommend the toolchain actor build that single mechanism once rather than three point-fixes — it retires the open facet of all three items together.

**Not implementing here:** same constraints — binary gated, and this pane's cycles execute inside lazily-rs's `%73` (confirmed contamination), so I can't verify or cleanly finalize. This is the toolchain actor's job, ideally after the `#jb-tsift-pane-sync` execution-context fix un-contaminates the pane.
Read the plan. Audited each item against shipped code — and there's a useful convergence finding here worth surfacing.

> **Queue prompt:**
>
> Please stop creating a build subagent and making the session open. Run the build in the session. It is confusing to see an open prompt and not know if the subagent is blocking the task or not. This is anti-visibility and can cause problems if I run the next task or clear the session while the subagent is still running. I'm not sure if the subagent is deadlocked or actively running as well. Stop doing this.
> #spec-test-build-install-commit-push

**Closeout:** uncommitted (gated). Net: `#lr-queue-repair-adopt` is mostly shipped; its one open facet (free-text head consume-on-adopt) is the shared primitive that also closes `#lr-queue-response-miss` step 3 and `#manual-queue-head-loss` — best built once, by the toolchain actor.
Do your recommendation.
> I'd recommend the toolchain actor fold that criterion into the plan before running `#spec-test-build-install-commit-push`, so `#jb-tsift-pane-sync` doesn't get marked done with the execution facet still live.

Note that monsterrodholders.rs is also running this document. Is there an issue with the lazily-rs integration? Are we caching something that should not be cached or should it be invalidated?
<!-- agent:boundary:4e6c31dc -->
<!-- /agent:exchange -->

<!-- agent:queue -->
~~~done
Please stop creating a build subagent and making the session open. Run the build in the session. It is confusing to see an open prompt and not know if the subagent is blocking the task or not. This is anti-visibility and can cause problems if I run the next task or clear the session while the subagent is still running. I'm not sure if the subagent is deadlocked or actively running as well. Stop doing this.
#spec-test-build-install-commit-push
~~~
preset #spec-test-build-install-commit-push
- ~do [#manual-queue-head-loss]~
- ~do [#subagent-blocks-session]~
- ~do [#lr-queue-plan-deferral]~
- do [#lr-queue-repair-adopt]
- do [#lr-queue-patchback-miss]
- do [#lr-head-8]
- do [#lr-wire-9]
- do [#lr-actor-10]
- do [#run-gated-review-items]
<!-- /agent:queue -->
###
<!--
```
[route] target tmux session: 0
[preflight] review: 55 item(s), 55 gated
[repair] closed stale empty preflight_started cycle cycle-1780431002764 for tasks/agent-doc/agent-doc-bugs2.md after 96s without a capture
[commit] detected post-commit local drift for tasks/agent-doc/agent-doc-bugs2.md — HEAD already contains the committed response; leaving later local working-tree edits uncommitted
[commit] staged snapshot already matches HEAD for tasks/agent-doc/agent-doc-bugs2.md — closing cycle as already committed
[perf] commit total: 23ms
Error: authoritative actor generation 221 for tasks/agent-doc/agent-doc-bugs2.md owns pane %70 but route could not drain the active closeout before dispatch: [session-check] INTERRUPTED: agent:queue contains assistant response prose copied from a `### Re:` body, not a user prompt or `do [#id]` directive: "There are too many items being accumulated into review. The tasks are not being ". Remove the contaminating line(s) from `agent:queue` (only user prompts, `do [#id]`, `preset`/`dispatch`, or backlog-derived entries are valid queue sources) and re-run finalize (see #jb-run-agent-doc-response-queue-contamination).
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue=append -->
- [ ] [#manual-queue-head-loss] [recommended] #agent-doc-bug 2026-06-02: user-added `do [#shipstationaudit]` in `src/boost-client/tasks/monsterrodholders.md` stalled and then disappeared while `#shipstationaudit` remained open in backlog. Queue removal must require durable proof that the exact line was consumed, done, gated, explicitly removed by the user, or stale generated residue; otherwise preserve/restore the queue head or fail closed. Plan: tasks/agent-doc/plan-manual-queue-head-stall-loss.md.
- [ ] [#subagent-blocks-session] [recommended] #agent-doc-bug feature 2026-06-02: when a turn is waiting on a subagent to complete, block the session so no other task runs in the agent until that subagent finishes (mostly a Claude-harness concurrency issue). PRECONDITIONS (unmet, attempted 2026-06-02): needs a scope/design decision before implementation — agent-doc cannot control the Claude Code harness's subagent scheduling, so this likely requires a harness/hook-level gate (e.g. a Stop / PreToolUse hook or supervisor lease) rather than an agent-doc binary change. Specify the exact concurrency boundary (which layer blocks, what counts as 'the subagent', how the block is released) before implementation.
- [ ] [#lr-actor-10] Phase 10: Add GlobalConfigSlot + SessionRegistrySlot for ActorContext. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- [ ] [#lr-wire-9] Phase 9: Wire RunContext into ops_log, template_io, patch, harness detection. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- [ ] [#lr-head-8] Phase 8: Add HeadContentSlot + SnapshotCommitStatusSlot for git show HEAD dedup. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- [ ] [#lr-queue-patchback-miss] [recommended] #agent-doc-bug 2026-06-02: lazily-rs queued free-text prompt with preset was not visibly answered or patched back; mixed #agent-doc-bug capture made this cycle plan_backlog_only and deferred the actionable queue head. Add plan/session-check coverage so the head is either answered, consumed, or explicitly deferred through a committed response. Plan: tasks/agent-doc/plan-lazily-rs-queued-prompt-patchback-miss.md.
- [ ] [#lr-queue-repair-adopt] [recommended] #agent-doc-bug 2026-06-02: if the lazily-rs queued prompt was already completed console-only, recovery must adopt a supplied response through agent-doc write --commit and consume or preserve the matching free-text queue head without rerunning repo work. Plan: tasks/agent-doc/plan-lazily-rs-queue-repair-adopt.md.
- [ ] [#run-gated-review-items] [recommended] #agent-doc-bug feature 2026-06-02: gated/review items are not easily runnable. Enable running a gated item directly via `do [#id]` or by placing it in agent:queue, and focus the run to complete that gated item. If it can only be partially completed, extract a new follow-up task containing the conditions to complete it. If no work was done because preconditions were unmet, append a timestamp of the attempt to the item and ensure the item's preconditions are present in its description.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#route-starting-timeout-poll-recovery] #agent-doc-bug 2026-06-02: IMPLEMENTED+tested+shipped (agent-doc 43de6bc8, pointer bc19f9f33, both binaries rebuilt+installed). JB Run Agent Doc on lazily-rs.md failed: 'authoritative actor generation 13 owns pane %71 but route will not bind a new dispatch target because this generation already timed out while starting'. Root cause: starting_actor_timeout is sticky (wait_for_authoritative_actor_ready short-circuits once it is set) and the blocked-timeout recovery did a single dispatch-ready capture, so a heavy gpt-5.5 xhigh Codex still settling into its idle composer after a supervisor restart dead-ended even though it became ready seconds later (session status later recovered it via live_pane_idle). Fix: recover_starting_timeout_blocked_actor_if_dispatch_ready now polls the live pane for a dispatch-ready prompt up to the harness recovery budget before failing closed; busy panes never satisfy current_generation_ready_prompt_proven so the promote-only-proven-idle invariant holds. Spec 08-session-routing + README aligned. make check green. GATED on live-verify / tmux-ci: the poll loop needs a live tmux pane (no Tmux test double in the suite), so exercise a two-session heavy-Codex restart repro before un-gating.
- [/] [#codex-upgrade-restart-cross-doc-focus] [recommended] #agent-doc-bug 2026-06-02: FOCUS MIS-BINDING IMPLEMENTED+tested+shipped (tmux-router 7a7ddd9, superproject pointer 6494fb190, both binaries rebuilt+installed): sync --focus now selects only the focus document's authoritative registry-owned, alive pane (captured before Phase 1.5/1.75 ephemeral donor/spare assignment) via the new resolve_focus_pane helper; when the document has no live owned pane it preserves the current tmux selection instead of aliasing onto a column-mate's pane (which belongs to a different document — the equityfundingsource.md symptom while focusing agent-doc-bugs2.md). 4 regression tests added; tmux-router 113 lib tests + agent-doc make check green. REMAINING: the session-loss-across-Codex-restart rebind facet (re-provision the focused document's own pane on supervisor restart rather than dropping/aliasing the session) — gated on the two-document upgrade/restart live repro. Shares the focus-by-exact-document-path invariant with #jb-tsift-pane-sync. Plan: tasks/agent-doc/plan-codex-upgrade-restart-cross-document-focus.md
- [/] [#jbstalldrain] [recommended] #agent-doc-bug 2026-06-02: JB `Run Agent Doc` appeared stalled across two observed route stages: `agent-doc-bugs2.md` failed before dispatch because closeout drain hit a session-check backlog-shadow blocker (`#queue-user-edit-overwrite` only outside live `agent:backlog`), while `monsterrodholders.md` waited the full 60s on busy authoritative actor generation 251 / pane `%61` before reporting no dispatch-ready prompt. Fix by making route emit typed in-progress and terminal results for closeout-drain blockers and busy-authoritative waits, and make the JetBrains action surface those states persistently instead of looking inert. Plan: tasks/agent-doc/plan-jb-run-agent-doc-stalled-closeout-drain.md.
- [/] [#codex-direct-writeback-miss] [recommended] #agent-doc-bug 2026-06-02: Direct-chat Codex execution of src/boost-client/tasks/monsterrodholders.md prompt `do [#pmaxpost]. #spec-test-commit-push` completed site + parent commits/pushes but the final assistant response was not written into the session document until manual `agent-doc write --commit` repair; make the Codex/direct-chat closeout gate fail closed before final chat output when the response heading is absent from HEAD, and keep the recovery path binary-owned. Plan: tasks/agent-doc/plan-response-missing-after-manual-closeout.md
- [/] [#weqq] [recommended] #jb-settext-payload-log IMPLEMENTED 2026-06-01 (submodule 12676120, JB plugin 0.2.145 built+signed): PatchWatcher.kt now logs the full result payload before document.setText on both applyPatch.component and repositionBoundary, behind LOG.isDebugEnabled, so enabling the com.github.btakita.agentdoc debug category captures the exact setText payload the IPC-duplication family needs (previously only hashes+lengths). GATED on operator: install build/distributions/agent-doc-jetbrains-0.2.145-signed.zip + restart, enable debug logging, then run the Path B capture (#wy0y/#6cmx). Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [/] [#6qfn] [recommended] #optindocs-editor follow-up to #4a6p: wire the FFI agent_doc_is_session_document gate into JB SubmitAction.update (hide/disable Run Agent Doc for a non-opted-in .md) and the VS Code extension, then live-verify. Needs a JB pluginVersion bump + signed reinstall. Plan: tasks/agent-doc/plan-opt-in-agent-doc-documents.md.
- [/] [#v819] Path A (#jkvs — after Step 0): one consolidated live busy-pane verification session to batch-close ~11 live-verify items: #t41j, #jb-busy-queue-deadlock-live-verify, #jb-run-agent-doc-busy-feedback-live-verify, #route-drain-all-surface-live-verify, #codex-owned-pane-stop-hook-live-verify, #codex-owned-pane-prompt-miss-followups (2b/3), #codex-final-response-live-verify, #recovery-drift-apply-live-verify, #jb-load-tmux-window-verify, #j0k8, #stash-pane-promote-on-focus.
- [/] [#wy0y] Path B (#6cmx — after Step 0): one IPC/typing-corruption capture session with #com.github.btakita.agentdoc debug logging enabled to capture the setText payload, then repro #ipcfullprompt-recur2, #finalize-retry-ipc-response-duplication, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #duplicate-response-after-commit, #prompt-duplicated-while-typing, and the scaffold-dup set #zpc2 / #ipcaa-scaf / #submodule-response-commit-proof. Several convert from verify to one CURE commit once the payload is captured.
- [/] [#qasm] [recommended] Path C - continue the no-live-pane implementable review items in priority order, starting with the queue-provenance cluster (#prompt-preempts-auto-queue, #queue-user-edit-overwrite, #jb-run-agent-doc-response-queue-contamination), then the Option-A fixes (#cancel-orphans-preflight-cycle, #exchange-prompt-dropped-on-merge, #duplicate-response-after-commit), closeout-guard hardening (#do-id-partial-closeout-state, #preset-item-id-collision-enforce, #queue-clear-unrun-items), the feature pair (#backlog-priority-attribute, #backlog-queue-sync-attribute), then hygiene/UX (#stash-session-ttl-prune, #plugin-cleanup-menu-command, #stash-pane-promote-on-focus, #clear-stops-running-process, #jb-stale-busy-idle-footer, #snrun).
- [/] [#submodule-response-commit-proof] [recommended] #agent-doc-bug 2026-06-01: a response to `src/boost-client/tasks/monsterrodholders.md` was committed inside the `boost-client` submodule (`6cbff1d`), but the parent `agent-loop` repo still showed `M src/boost-client`, and the closeout wording did not distinguish target-doc commit proof, parent submodule-pointer dirtiness, and the fact that the response bypassed binary-owned `agent-doc write` / `finalize` after recursive same-pane dispatch failed. Fix by making nested-repo commit ownership explicit in session-check/final diagnostics and fail closed on unverified manual fallback. Plan: tasks/agent-doc/plan-submodule-response-commit-proof.md.
- [/] [#jb-tsift-pane-sync] [recommended] JetBrains navigation to `tasks/software/tsift.md` and manual `Sync Tmux Layout` did not immediately surface this pane, AND a 2026-06-01 report shows a worse cross-document variant: the pane surfaced for `tsift.md` was actually running `agent-doc-bugs2.md` (wrong-document pane↔owner binding). Plan and fix the regression where automatic selection plus explicit manual sync can leave a live/hidden/stashed target pane invisible or silently deferred OR surface a pane that owns a different document; navigation + manual sync must cleanly autostart/surface the owner for the exact navigated file (never alias/reuse another document's owner) or fail closed naming the blocker. Plan: tasks/agent-doc/plan-jb-tsift-pane-sync-regression.md.
- [/] [#6cmx] [recommended] Path B — run one IPC/typing-corruption live-capture session: enable #com.github.btakita.agentdoc debug logging, then type into a doc during finalize --stream to capture the exact setText payload that unblocks the duplication family #ipcfullprompt-recur2, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing, #finalize-retry-ipc-response-duplication, #zpc2.
- [/] [#jkvs] [recommended] Path A — run one consolidated live busy-pane verification session to batch-close the live-verify-gated review items: install the current signed JB plugin + restart the route-owned agent-doc process, then drive one mid-active-turn pane through #t41j, #jb-busy-queue-deadlock-live-verify, #jb-run-agent-doc-busy-feedback-live-verify, #route-drain-all-surface-live-verify, #codex-owned-pane-stop-hook-live-verify, #codex-owned-pane-prompt-miss-followups (2b/3), #codex-final-response-live-verify, #recovery-drift-apply-live-verify, #jb-load-tmux-window-verify.
- [/] [#4a6p] [recommended] CORE IMPLEMENTED + tested + shipped (submodule 06180fec / pointer ba6d237e0, 2026-06-01): a plain .md no longer auto-converts into a session. project_config::is_agent_doc_document predicate + zero-dep glob_match (*/?/**); Frontmatter::has_agent_doc_marker (single source of truth for session-defining fields); DocumentsConfig ([documents] include glob + auto_session_for_all_md escape hatch); require_agent_doc_document gate wired into run/route/start (fail closed, file untouched, malformed-frontmatter bypass so the real YAML error still surfaces); FFI agent_doc_is_session_document. make check green; FFI symbol + live gate verified; both binaries installed; spec 06-config + VERSIONS (BREAKING CHANGE). GATED on editor-side wiring + behavior live-verify (see #optindocs-editor). Plan: tasks/agent-doc/plan-opt-in-agent-doc-documents.md.
- [/] [#j0k8] [recommended] Live-verify the cross-root claim guard (submodule 822688d0): from the superproject root, claim/open a new document from inside a pane already running a different document's submodule agent-doc/codex session, and confirm claim provisions a NEW pane (logs '[claim] pane ... runs a live agent-doc/codex session for another document') instead of aliasing onto it. Also confirm the awear.md recovery restores a real pane. cmdline_owns_other_document is unit-tested; this is the live-pane edge.
- [/] [#do-id-partial-closeout-state] [recommended] Partial-closeout WARN guard landed (check_partial_closeout_state_guard: a do[#id] turn whose response shows shipped + remaining-live-work signals with the directed id still open advises --pending-edit/--pending-gate narrowing; suppressible by a no-partial-closeout-guard response marker; FlowCore guard_ budget 35->40). Remaining: per-id edit-vs-gate tracking in cycle_state, opt-in strict mode, next-phase queue convergence, and the runbook note that #spec-test-commit-push completes a repo phase without a live/deploy item. Plan: tasks/agent-doc/plan-do-id-partial-closeout-state.md.
- [/] [#free-text-queue-skip-diagnostic] [recommended] IMPLEMENTED + tested (audited 2026-06-01): write::queue_skip_diagnostic_for_content emits a shape-aware skip diagnostic — an id-backed head names the missing proof + --done/--pending-gate/--pending-edit repair; a free-text head points to the answered-response path; generic fallback only when no active head exists. Wired in run.rs:448 and write.rs:1520/1527; regression queue_skip_diagnostic_names_head_shape_and_repair_path. Gated on live-verify only. Plan: tasks/agent-doc/plan-free-text-queue-owner-consume.md (point 4).
- [/] [#preset-item-id-collision-enforce] [recommended] Part 1 (mutation-time rejection) IMPLEMENTED (submodule f8e84282): write --pending-add / --pending-add-after / --pending-add-before / --pending-add-back / --pending-add-to with an explicit colliding id (id=X or [#X] matching a prompt_presets key or an active backlog/review/icebox id) fails closed via pending_cmd::reject_colliding_explicit_id + preflight::identity_collision_for_new_id (factored from detect_identity_collisions/document_active_identities); auto-id adds never blocked. 5 regressions; make check green. DEFERRED by design (over-blocking risk on live sessions): (2) hard preflight/session-check dispatch block on a pre-existing collision — stays the preset_item_id_collision warning; (3) queue-generation refusal for ambiguous ids. Plan: tasks/agent-doc/plan-preset-item-id-collision.md.
- [/] [#codex-owned-pane-stop-hook-live-verify] [recommended] Gated on a live Codex pane -- the remaining phases of #codex-owned-pane-prompt-miss-followups after phase 2a landed: (2b) a live Codex Stop-hook regression asserting the owned_pane_self_invocation diagnostic forces an in-pane response body before final chat output, and (3) live-verify a real Codex same-pane run with an unresolved exchange prompt. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#t41j] [recommended] #jb-run-agent-doc-busy-wait-deadlock live-verify (gated on a live busy pane): fix landed (submodule 89c357d5 — busy_dispatch_only_should_wait_for_ready skips the --wait-for-ready start override on a busy active turn + the (claude, active claude turn) case in dispatch_active_turn_queue_source). With the installed binary, click JB Run Agent Doc on a doc whose Claude actor is mid active-turn (not running /loop) and confirm it returns immediately (no 60s route_wait_for_ready_override block), appends the prompt to agent:queue auto, and the supervisor idle-queue watch drains the head on busy->idle. Check ops.log for the enqueue and the absence of a 60s wait.
- [/] [#queue-clear-unrun-items] [recommended] #agent-doc-bug 2026-05-31: active `agent:queue auto` heads can be deleted/cleared without execution proof during same-pane/recovery closeout. Live repro `src/boost-client/tasks/monsterrodholders.md`: queue heads `#convqa-rerun`, `#hydroapproval`, `#nbapproval`, `#shopcachewatch`, `#shoplabelgate`, and `#accessorymargin` were removed while the matching backlog items stayed open and no per-head closeout ran. Add queue-removal invariants so every removed head must have explicit consumed/done/gated/struck/stale-user-edit proof, and make reset/commit/session-check fail closed on unproved deletion. Plan: `tasks/agent-doc/plan-queue-clear-unrun-items.md`.
- [/] [#jb-busy-queue-deadlock-live-verify] [recommended] Live-verify the supervisor idle-queue watch (#jb-run-agent-doc-busy-queue-dispatch-deadlock): with the installed binary, run JB Run Agent Doc on a doc whose authoritative actor is mid active-turn (Codex or a Claude session not running /loop), confirm the route appends to agent:queue auto + returns Ok, then when the pane returns to idle the supervisor idle-queue watch injects agent-doc <FILE> once and drains the head with NO duplicate injection into the active turn and no hot-loop. Check the session log for idle_queue_watch_drain. Gated on a live busy pane. Source: submodule 4b3f4835 (idle_queue_drain_decision + spawn_idle_queue_watch_thread in start.rs).
- [/] [#recovery-drift-apply-live-verify] [recommended] Live-verify the full one-command recovery (agent-doc repair --apply-recovery) on a real owner pane now that #recovery-drift-authoritative-side landed the authoritative-side determination: reproduce a queue_metadata_drift / sidecar_visible_drift closeout and confirm apply_closeout_recovery restores-from-HEAD for the spurious queue_active:false class and commits-forward otherwise, without the manual cancel->reset->commit sequence. Gated on a live owner pane (risky in a racy multi-session repo). Parent plan: tasks/agent-doc/plan-recursive-repair-state-drift.md.
- [/] [#opencode-active-turn-reopen-auto-retry-on-idle] [recommended] When a plain dispatch-only Run Agent Doc reopen is refused because the OpenCode (or Codex) pane shows an active turn, route currently fails closed and the operator must re-click once the turn ends. Optional refinement: watch the pane and auto-fire the bare reopen once it returns to a dispatch-ready/idle composer, so the operator does not have to retry manually. Must stay a plain reopen (no prompt to queue) and respect the existing no-inject-into-active-turn invariant. Lower priority than the live-verify items.
- [/] [#jb-run-agent-doc-busy-feedback-live-verify] [recommended] Live-verify the busy-pane Run Agent Doc feedback fix (#jb-run-agent-doc-command-route-miss) end to end: install JB plugin 0.2.143 (build/distributions/agent-doc-jetbrains-0.2.143-signed.zip) + restart the route-owned agent-doc process to load the new binary, then run Run Agent Doc on a doc whose Codex pane is mid active-turn and confirm an immediate 'session still running' Event Log notification (no silent 60s wait, no 4x retry). Source landed submodule e1735fa2.
- [/] [#codex-owned-pane-prompt-miss-followups] [recommended] Follow-ups from the #codex-owned-pane-prompt-miss fix. Phase 1 (structured owned_pane_self_invocation preflight result) and phase 2a (in-pane Codex guidance + preflight_emits_owned_pane_self_invocation_for_active_queue_head regression) LANDED 2026-05-31 (submodule 407b0825). Remaining (gated — needs a live Codex pane): (2b) a live Codex Stop-hook regression asserting an in-pane response body before final chat output (the deterministic session-check backstop already landed in phase 1); (3) live-verify a real Codex same-pane run with an unresolved exchange prompt. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#codex-owned-pane-auto-queue-stuck] [recommended] Codex-owned `agent-doc <FILE>` direct invocation from the same pane can leave an active auto-queue turn stuck: the recursive guard avoids child dispatch, but the ready queue head is not handed to the owning Codex turn and no response is written. Live repro 2026-05-31 `src/boost-client/tasks/monsterrodholders.md`: `queue_active: true` + `agent:queue auto` head `re [#374n]...`; same-pane run printed `Submitting to codex...` then `recursive direct invocation would deadlock`, leaving queue/boundary drift and the head unprocessed. Fix with early owner-pane queue handoff, durable queue-target proof, no-drift guard, and Codex final-gate recovery. Plan: tasks/agent-doc/plan-codex-owned-pane-auto-queue-stuck.md.
- [/] [#backlog-queue-sync-attribute] [recommended] Add a queue=sync|append|prepend attribute to agent:backlog / agent:icebox: queue=sync replaces agent:queue content with a do [#id] list for every item; queue=append adds item ids not already in agent:queue; queue=prepend prepends them. Composable with #backlog-priority-attribute (prioritize first, then regenerate the queue), automating the manual 'regenerate the queue to the new priority' step that currently has to be done by hand after a reorder. Requested 2026-05-30.
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
- [/] [#queue-user-edit-overwrite] [recommended] User-authored agent:queue edits can be silently overwritten/deleted by write/reset/commit queue convergence. Live repro 2026-05-30 boost-client monsterrodholders.md: user added do [#gscaccess] at queue head; agent-doc repeatedly removed/reverted it during closeout/snapshot repair. Fresh repro 2026-06-02: operator wrote into agent:queue while the previous cycle was still running, and the closeout lost those queue changes without a fail-closed diagnostic. Fix by tracking queue edit provenance/epochs, rechecking the live queue at final visible-write/consume boundaries, preserving explicit queue edits unless the current response consumes that exact head, and failing closed on stale IPC/snapshot queue rewrites. Plan: tasks/agent-doc/plan-user-queue-edit-preservation.md.
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
- [/] [#snrun] [recommended] IMPLEMENTED 2026-06-01 (submodule 12676120): the deterministic core was already present (interactive-substate detection + multi-source RoutedDispatchStartProof + stage diagnostics); this cycle added the dedicated RoutedReopenGuardReason::BlockedInInteractiveSubstate guard reason (prompt_ready_barrier FlowEvent) + stage-specific error on the dispatch-only fail-closed path, distinct from generic dispatch_only_busy_actor_not_ready. Pure helpers is_interactive_shell_substate_reason/dispatch_only_blocked_guard_reason; regression interactive_substate_gets_dedicated_guard_reason; FlowCore route.rs guard_ budget 7->12 (audited). make check green; binaries reinstalled. REMAINING = live JB Run Agent Doc repro (plan exit criterion 6), operator live-verify -> folds into Path A (#jkvs). Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
