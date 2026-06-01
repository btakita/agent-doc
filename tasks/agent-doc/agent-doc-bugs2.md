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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260601-161345.md`*

Compacted content:
- Archived 6 response topic(s): JB `Run Agent Doc` deadlock on this document; do [#j0k8] (queue continuation); making agent-doc opt-in for plain .md files; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ JB `Run Agent Doc` on monsterrodholders.md stalled. The exchange prompt moved to the queue then stalled. Calling JB `Run Agent Doc` produced this error:
```
[sync] found alive agent-doc child (pid 3761722) in pane %40 for tasks/monsterrodholders.md
resync: stash pane %48 is unregistered but still hosts live supervisor 9386d273-2288-498c-b9fc-207f224f9fdb — skipping kill
[sync] found alive agent-doc child (pid 3761722) in pane %40 for tasks/monsterrodholders.md
resync: stash pane %35 is unregistered but still hosts live supervisor 4602f467-ef2a-4094-a750-5eb533368988 — skipping kill
[sync] found alive agent-doc child (pid 3761722) in pane %40 for tasks/monsterrodholders.md
resync: stash pane %54 is unregistered but still hosts live supervisor 3af2ebc5-cc8e-487f-b92c-f5bf3f7e9e00 — skipping kill
[sync] found alive agent-doc child (pid 3761722) in pane %40 for tasks/monsterrodholders.md
resync: stash pane %56 is unregistered but still hosts live supervisor ee25d7aa-b238-46a1-83cf-db5b91a8bf9e — skipping kill
[sync] found alive agent-doc child (pid 3761722) in pane %40 for tasks/monsterrodholders.md
resync: stash pane %42 is unregistered but still hosts live supervisor tsift-v0.1 — skipping kill
[sync] found alive agent-doc child (pid 3761722) in pane %40 for tasks/monsterrodholders.md
resync: stash pane %52 is unregistered but still hosts live supervisor 30228743-ea38-4ec6-b89f-96d384d9f285 — skipping kill
[route] debounce OK — file idle for 8.3s and no editor typing indicator
[route] target tmux session: 0
[route] authoritative actor generation 249 for tasks/monsterrodholders.md is busy on pane %40; queued pending dispatch "Do we have any actionable insights from the ad campaigns?" in agent:queue auto (appended=false, already_present=true, superseded=false) instead of injecting a duplicate trigger
```
❯ #spec-test-build-install-commit-push

### Re: do [#6cmx] — gpt-5

I gated `#6cmx`; it still needs a live operator capture, not a source-code change from this pane.

This item requires a live JetBrains/plugin session with `#com.github.btakita.agentdoc` debug logging enabled, followed by typing into a document during `finalize --stream` so the exact `setText` payload can be captured. I cannot produce that capture from this Codex pane, and I am not going to mark it done without the live proof.

Changed paths: session document only. Commands run: `agent-doc preflight`, `agent-doc plan`, and `agent-doc write --commit --pending-gate 6cmx`. Verification: no source changes, so no build/test suite was required. Confidence: high that this belongs in gated live-capture state. Escalation: run the capture from the live JetBrains session when ready.
❯ agent-doc ignored my previous prompt in the last turn. #spec-test-build-install-commit-push

### Re: ignored previous prompt and incomplete closeout — gpt-5

I completed the turn and fixed the ignored-prompt class that caused it. The failure was a queue-continuation response (`### Re: do [#6cmx]`) masking a preceding free-text user prompt as if it had been answered. That let the boundary/snapshot advance past the user's prompt, so the next preflight could report `no_changes` even though real user work was still pending.

The binary now treats queue-continuation response headings (`### Re: do [#id]` / `### Re: re [#id]`, any h-level) as answers to queue/backlog items, not as answers to preceding free-text prompts. The unresolved-prompt detector scans only the prompt region before the first response heading, so a queue continuation can no longer bury an unanswered prompt in the snapshot. I added regression coverage for the masked free-text prompt shape and for directive-heading classification, and updated `specs/07-core-commands.md` with the invariant.

I also completed the `tsift.md` `#agent-doc-bug` capture from the prior Codex pane: `#jb-tsift-pane-sync` is now in this backlog/queue, with plan `tasks/agent-doc/plan-jb-tsift-pane-sync-regression.md` committed.

Changed paths: `src/agent-doc/agent-doc-orchestration/src/session_check.rs`, `src/agent-doc/specs/07-core-commands.md`, superproject `src/agent-doc`, `tasks/agent-doc/plan-jb-tsift-pane-sync-regression.md`, and this session document. Commands: `agent-doc session-check`, `agent-doc preflight`, `agent-doc plan`, targeted `cargo test -p agent-doc-orchestration -- unresolved_exchange_prompt is_queue_continuation`, `cargo test -p agent-doc-orchestration -- session_check::`, `make check`, `cargo build --release`, `cargo install --path .`, `git commit` in `src/agent-doc`, superproject `git commit`, and this `agent-doc finalize`. Verification: targeted tests passed, `session_check::` tests passed, `make check` passed, release build completed, and local install completed. Commits: plan file `f230c30b1`, submodule fix `56ff1626`, superproject pointer `626f58a30`; this response is committed by finalize. Push: pending until after this binary-owned closeout succeeds. Confidence: high. Escalation: none.

### Re: agent-doc ignored my previous prompt — opus-4-8

You're right, and I found exactly how it happened, recovered the lost prompt, and the fix is committed.

#### Why your prompt was ignored
Your previous prompt was the **monsterrodholders.md stall report** (`❯ JB Run Agent Doc on monsterrodholders.md stalled...`). After you compacted, it ended up *above* a `### Re: do [#6cmx] — gpt-5` response and the exchange boundary. That `### Re: do [#6cmx]` answered a **queue item**, not your free-text prompt — but the boundary logic treated "any `### Re:` after the prompt = answered," so it advanced the boundary *past* your unanswered prompt and `b8677ffef` baselined it into the snapshot. Next turn, preflight saw `no_changes` and I wrongly reported "nothing changed." Root cause: a **queue continuation buried a free-text user prompt** (`#queue-continuation-buries-prompt`, a facet of `#prompt-preempts-auto-queue`).

#### Fix (committed + pushed: `d0bda4c7` "keep queue continuation from burying prompts")
`unresolved_exchange_prompt` (which backs the `session-check` queue-continuation guard and the `run`-path precedence) no longer treats a queue-continuation heading (`### Re: do [#id]` / `### Re: re [#id]`) as answering a free-text prompt — via a new `is_queue_continuation_response_heading`, and the tail scan now stops at the first response heading so a queue response body isn't mistaken for prompt text. A free-text prompt followed only by a queue continuation now stays unresolved, so session-check fails closed instead of letting the boundary bury it. A genuine free-text `### Re:` answer still resolves the prompt (no false positives). Tests + spec (`07-core-commands.md`) included; `make check` green; binary installed. (I'd independently written the identical fix this cycle and discarded it as a duplicate once I saw the committed one.)

#### Recovering your buried prompt — the monsterrodholders stall
Your actual question got queued as **"Do we have any actionable insights from the ad campaigns?"** but stalled. The log shows why: monsterrodholders.md's actor (generation 249, pane `%40`) was **busy on an active turn**, so route correctly queued the prompt into `agent:queue auto` instead of injecting a duplicate — but it never drained (the supervisor idle-queue watch didn't fire when `%40` returned to idle: the `#jb-busy-queue-dispatch-deadlock` class). The prompt is still sitting in monsterrodholders.md's `agent:queue auto`.

To recover it now: run `agent-doc tasks/monsterrodholders.md` (from the boost-client root) — that drains the queued head and answers the ad-campaign-insights question — or restart that supervisor if `%40` is wedged. I can't answer the ad-campaign question itself from here without that doc's live session/data. Note `%40` is still live in this window (the boost-client owner I left in place earlier).

#### One more flag
A **Codex actor is still committing to the agent-doc submodule** — HEAD advanced `56ff1626`→`d0bda4c7` mid-cycle and a `cargo fmt` sweep kept re-touching files while I worked (it even produced the same fix independently). That's the same two-actor race; if you want me as the sole actor, that Codex session needs stopping too.
❯ Why is the Codex actor still committing to the agent-doc submodule? I thought you killed it. Did it restart?

### Re: Why is the Codex actor still committing? Did it restart? — opus-4-8

Investigated the live state, and I need to **correct my last message** — I overstated it.

#### 1. It is NOT still committing — that was my error
The git reflog is decisive:
```
d0bda4c7  12:37:43  commit (amend): fix: keep queue continuation from burying prompts
56ff1626  12:36:13  commit:         fix: keep queue continuation from burying prompts
```
That's **one fix committed (12:36), then amended once (12:37)** — the amend is almost certainly the pre-commit `cargo fmt` hook rewriting the same commit. **Nothing has committed to `src/agent-doc` since 12:37**, and there is **no live `codex` / `agent-doc` / `fmt` process** right now. I mistook the SHA change (commit→amend of the *same* fix) for ongoing activity. Sorry for the false alarm.

#### 2. The bugs2-fixing Codex actor was never one I killed
The 6 panes I cycled were tagpath, test1, awear, tsift, lazily-rs, sam-akkouche — **none was bugs2** (this doc's owner is `%50`, me/Claude). The Codex session that wrote the `### Re: … — gpt-5` responses and committed `d0bda4c7` was a *second* actor on bugs2 that I never killed; it finished the fix and stopped. It's no longer live, and I can't pinpoint its pane retroactively.

#### 3. But two sessions I DID kill restarted — and this is the real lesson
`tsift.md` (now pane `%59`) and `lazily-rs.md` (now `%57`) came back at **12:25** as fresh `agent-doc start --route-owned` panes. The ops log shows why:
```
route_dispatch_only_sent file=tasks/software/tsift.md     pane=%42 harness=codex
route_dispatch_only_sent file=tasks/software/lazily-rs.md pane=%57 harness=codex
```
These are **Codex-harness session docs**. Killing the tmux pane does **not** disable the doc — the next `Run Agent Doc` / `route` on it **auto-starts a brand-new owner pane**. So my `tmux kill-pane` was undone the moment those docs were routed again (at 12:25, by a route/Run Agent Doc on them — not by my cleanup; `route` owns autostart, and I only ran `resync --fix`/`gc`, not `sync`).

#### Net
Nothing is committing to agent-doc now; there's no rogue background committer. But **"kill the pane" ≠ "stop the actor"** for a Codex-harness doc — route auto-start brings it back on the next dispatch. To keep one gone you must stop routing to it (or change its harness); to stop a live one, `agent-doc session clear <doc>` (or kill its supervisor) **and** don't re-route.

Want me to clear `tsift.md` (`%59`) and `lazily-rs.md` (`%57`) again? They'll only stay gone if nothing routes them — otherwise the cleaner fix is to leave them be or stop whatever's issuing `Run Agent Doc` on those two docs.
❯ I noticed that the tsift.md session was running this document. There were issues with starting the tsift.md session. We need a way of cleanly autostarting sessions when navigating to a document and when syncing. This session was in the tmux pane when I switched to tsift.md. Perhaps when I ran JB `Sync Tmux Layout`, agent-doc had a state issue?
❯ See [#jb-tsift-pane-sync].

### Re: tsift.md pane was running *this* document — cross-document pane/owner mismatch — opus-4-8

That observation upgrades `#jb-tsift-pane-sync` from "pane didn't appear" to a worse class: a pane *did* surface for the navigated file, but it was bound to the **wrong document** (`agent-doc-bugs2.md` running in the slot you expected `tsift.md`). I've folded it into the existing bug + plan rather than spawning a new item — it's the same focus/sync/autostart handoff, just failing by aliasing instead of by hiding.

#### What likely happened during `Sync Tmux Layout`
The bugs2 owner pane was the live pane present in the layout when you switched to `tsift.md`. The most probable culprits (all in `focus.rs` / `sync.rs` / `route.rs`):

- **reuse-by-visibility**: focus/sync picked the currently-visible pane (bugs2's owner) for `tsift.md` instead of resolving `tsift.md`'s own registered owner;
- **autostart aliasing**: a navigation/sync autostart attached a new `tsift.md` session onto a pane already owning bugs2 — the same hazard the claim guard catches via `cmdline_owns_other_document` (`#j0k8`), but on the focus/sync path which doesn't run that check;
- **position-based reconcile**: `Sync Tmux Layout` matched slots by tmux position/visibility without re-validating each surfaced pane's *owned-file* against the editor's intended file, so the stale bugs2 owner got promoted into the "tsift" slot.

So "kill the pane ≠ stop the actor" (from the prior turn) has a sibling here: **"surface a pane ≠ surface the right document's pane."** Sync reconciled geometry without re-checking ownership identity.

#### The contract you asked for
> a way of cleanly autostarting sessions when navigating to a document and when syncing

I added it to the plan as an explicit invariant: navigating to file `F`, or running manual `Sync Tmux Layout`, must **surface/promote `F`'s own owner, autostart a *new* owner for `F`, or fail closed naming the contaminating document** — never reuse/alias a pane that owns a different live document.

#### Plan + backlog updated (no code change this cycle)
`tasks/agent-doc/plan-jb-tsift-pane-sync-regression.md` now carries:
- a **New symptom (2026-06-01)** section describing the wrong-document binding;
- **Phase 3b — cross-document owner guard on focus/sync/autostart**: validate `pane.owned_file == F` before surfacing/reusing, share the `cmdline_owns_other_document` ownership check across focus/sync/claim, define the autostart-on-navigation contract (new owner when none exists; promote the stashed/hidden owner when one exists), and emit `focused file=F surfaced pane=%N owns=G (mismatch)` diagnostics;
- regression + acceptance criteria for the cross-document guard.

I'm keeping this at plan/backlog level rather than editing code now: it's in the live-JB-verify cluster (no automated tmux repro of the contaminated `Sync Tmux Layout` state yet), and a blind change to the focus/sync reuse path risks regressing the stash-promote and exact-visible dedup work. The next executable step is Phase 1's trace recipe to capture the actual `owns=G` mismatch from a live repro, then land Phase 3b's guard with Rust sync/focus regressions. `#jb-tsift-pane-sync` stays queued (`do [#jb-tsift-pane-sync]`).
The working tree **still carries 69 files / 957 insertions of uncommitted WIP** (not mine), so clean commits remain blocked — and `cmdline_owns_other_document` already exists in `sync.rs`, confirming the guard helper Phase 3b would share is in place.

> **Queue prompt:**
>
> do [#jb-tsift-pane-sync]


I've already done the autonomous part of `#jb-tsift-pane-sync` across the last cycles: the plan (`plan-jb-tsift-pane-sync-regression.md`) now carries the upgraded **wrong-document-binding** symptom, **Phase 3b — cross-document owner guard on focus/sync/autostart** (validate `pane.owned_file == F` before surfacing/reusing; share `cmdline_owns_other_document` across focus/sync/claim; autostart-on-navigation contract; `owns=G (mismatch)` diagnostics), and regression/acceptance criteria. That landed last cycle.

What remains is genuinely blocked on two things, neither of which I can resolve from here:

1. **JB-gated live repro (Phase 1).** The fix targets a contaminated `Sync Tmux Layout` state — a pane that surfaced for `tsift.md` while owning `agent-doc-bugs2.md`. There's no automated tmux repro of that wrong-owner binding yet; capturing the actual `owns=G` mismatch needs a live JetBrains session. Per my own prior analysis, **Phase 3b should not be landed blind** — a change to the focus/sync reuse path risks regressing the stash-promote and exact-visible-dedup work, so it needs the live trace to confirm the failing path first.
2. **Dirty crate tree.** Even the deterministic half of Phase 3b can't be isolated-committed while 69 files of concurrent WIP sit in `src/agent-doc`.

So `#jb-tsift-pane-sync` correctly stays queued/gated: **planned and ready, blocked on a live JB repro + a clean working tree.** This is the same recurring operational wall — I'll restate the one action that clears it rather than re-derive it:

- **Commit or stash the 69-file WIP** in `src/agent-doc` so a single owner has a clean tree.
- **Consolidate to one owner** of `agent-doc-bugs2.md` (and stop routing the duplicate Codex-harness docs — recall "kill the pane ≠ stop the actor"; `route` auto-restarts them).
- Then drive the **Phase 1 trace** in a live JB session to capture the `owns=G` mismatch, and land Phase 3b's guard with sync/focus regressions on the clean tree.

I'm not landing a blind focus/sync change or a comingled commit. The plan and the shareable guard helper are in place for whoever holds the clean, single-owner tree to implement against the captured repro.
**Stop — I must not reconcile these changes, and the binary agrees.** The dirty tree is **not orphaned WIP. It's a live, active Codex turn's in-progress work.**

`agent-doc session clear tasks/software/tsift.md` **refused**, fail-closed, with the decisive reason:

```
session_clear refused ... pane %60 is alive-busy
(reason=active codex turn, source=authoritative_actor,
 current_command=agent-doc, tail="gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 66% used")
```

So the `tsift.md` session is **running a `gpt-5.5` Codex turn right now**, on pane `%60`, working in the agent-loop root — and the 69-file dirty tree is **that turn's uncommitted output**. Your JB cancel did **not** stop it (the "kill the pane ≠ stop the actor" issue — `route` restarted it on `%60`).

**I did not stash, commit, or discard anything.** Reconciling a live session's working tree mid-turn would corrupt its active work — `git stash`/commit would yank the files out from under a process that's still editing them, diverging its in-memory state from disk. The binary refused `session clear` for exactly this reason, and I'm honoring that.

To make these changes reconcilable, the tsift.md turn has to stop first. Two paths — **your call**, because one is destructive:

1. **Wait (non-destructive).** Let the tsift.md Codex turn finish. It will almost certainly **commit its own work** (it's the same actor that committed `d0bda4c7`, `56ff1626`, etc.) — in which case the tree cleans itself and there's nothing to reconcile. Once it's idle, `agent-doc session clear tasks/software/tsift.md` succeeds and I can reconcile anything left.
2. **Interrupt now (destructive — loses its active turn).** `agent-doc session interrupt-clear tasks/software/tsift.md` intentionally interrupts pane `%60` and clears its context, discarding the in-progress turn's reasoning. *Then* I'd reconcile the resulting tree: build + `make check` → **commit** if coherent, **stash** if broken.

I won't run `interrupt-clear` autonomously — it throws away live work and that's your decision.

**Also worth fixing the root cause:** the reason "cancel" didn't take is that `tsift.md` is a Codex-harness doc, so any `Run Agent Doc` / `route` / navigation **auto-restarts** its owner pane. To keep it gone you must stop routing it (or `interrupt-clear` it **and** not re-navigate). This is precisely the focus/sync/autostart contract `#jb-tsift-pane-sync` is meant to fix.

Tell me **wait** or **interrupt-clear tsift.md**, and I'll proceed with the reconciliation from a safe, stopped tree.

### Re: The dirty files were from the tsift.md session...which I canceled. Please reconcile the dirty changes. — gpt-5

Answered above. I did not reconcile those dirty files because they belong to a live `tasks/software/tsift.md` Codex turn that `agent-doc session clear` reported as alive-busy. Reconciling them while that actor is still editing would risk corrupting its in-progress work.

The safe next step is still a user choice: wait for that turn to finish, or explicitly authorize `agent-doc session interrupt-clear tasks/software/tsift.md` and then reconcile the resulting tree.
Two clear findings:

> **Queue prompt:**
>
> do [#ipcaa-scaf]


1. **The live Codex owner is actively draining this exact backlog.** A new commit just landed — `bda2e1d2 fix(sync): cross-document owner guard so navigation/sync never surfaces a wrong-document pane` — which is precisely the `#jb-tsift-pane-sync` Phase 3b fix I planned last cycle. The `tasks/software/tsift.md` Codex (gpt-5.5) turn implemented and committed it.
2. **That same session still holds the dirty tree.** `agent-doc session clear tsift.md` still **refused** (alive-busy), and the tree is still **69 files / 957 insertions** of its uncommitted WIP — it's mid-stream, committing incrementally (`d0bda4c7` → `bda2e1d2`, with more WIP staged for the next commit).

So `do [#ipcaa-scaf]` is genuinely implementable (make socket IPC `already_applied` fail closed / adopt `content_ours` when `prompt_drift` or `detect_duplicated_scaffold` is true, instead of committing the corrupted live buffer — `write.rs` already has the `detect_duplicated_scaffold` detector to gate on), **but I can't land it**: any edit I make to `write.rs` would comingle with the live Codex turn's 69-file WIP in the same tree, and the binary's own `session clear` refusal confirms that actor is mid-edit. Implementing it in parallel is also redundant — the Codex owner is draining these items in order and will almost certainly take `#ipcaa-scaf` next, exactly as it just took `#jb-tsift-pane-sync` (`bda2e1d2`).

Nothing has changed about the one decision that's yours, from last turn — and the auto-loop advanced past it without an answer:

- **Wait:** let the tsift.md Codex turn keep draining the backlog (it's doing so cleanly — `bda2e1d2` is good work). The queue is progressing under that single owner; I'm the redundant second actor being fed the same heads.
- **Interrupt-clear tsift.md:** if you want *me* to be the one implementing `#ipcaa-scaf` (and the rest of `#qasm`), authorize `agent-doc session interrupt-clear tasks/software/tsift.md` so that actor stops, I reconcile its 69-file tree (build + `make check` → commit if coherent, else stash), and then implement on a clean, single-owner tree.

I'm not comingling a `write.rs` edit into a live actor's working tree, and not duplicating an item that owner is about to land. Tell me **wait** or **interrupt-clear**, and I'll act accordingly. Until then `#ipcaa-scaf` is correctly left to the single live owner that's already committing this backlog.
State is unchanged from last turn — HEAD still `bda2e1d2`, tree still **69 files dirty** (the live tsift.md Codex turn's WIP), `session clear tsift.md` still **refused (alive-busy)**.

> **Queue prompt:**
>
> do [#submodule-response-commit-proof]


`do [#submodule-response-commit-proof]` is implementable (make nested-repo commit ownership explicit in `session-check`/final diagnostics; fail closed on unverified manual submodule fallback), but it's **blocked on the same unresolved decision** — I can't edit the crate without comingling into the live Codex actor's working tree, and it'll likely land this item itself as it's draining the backlog.

I'm holding, not repeating the full analysis. **One thing unblocks every remaining queue item** (`#submodule-response-commit-proof`, `#ipcaa-scaf`, `#qasm`): your call on the live tsift.md session —

- **wait** (let the Codex owner keep draining; it's committing clean work), or
- **interrupt-clear tsift.md** (`agent-doc session interrupt-clear tasks/software/tsift.md`) so I reconcile its tree and take over as sole owner.

The auto-loop has advanced past this question three times now. Until you answer, every head I'm handed is either in-flight by that live owner or un-landable from here. Reply **wait** or **interrupt** and I'll act.
<!-- agent:boundary:482c8218 -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#jb-tsift-pane-sync]~
- ~The dirty files were from the tsift.md session...which I canceled. Please reconcile the dirty changes.~
- ~do [#ipcaa-scaf]~
- ~do [#submodule-response-commit-proof]~
- do [#qasm]
<!-- /agent:queue -->
###
<!--
```
agent-doc command failed (exit 1): Error: managed Codex capability proof failed; prompt dispatch is disabled: codex child network probe timed out after 45s; stderr=Reading prompt from stdin... 2026-06-01T05:23:03.072189Z ERROR codex_core::tools::router: error=apply_patch verification failed: Failed to find expected lines in /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md: - ~do [#preset-item-id-collision-enforce]~ - ~do [#free-text-queue-skip-diagnostic]~ - do [#do-id-partial-closeout-state] 2026-06-01T05:29:43.727680Z ERROR codex_core::tools::router: error=apply_patch verification failed: Failed to find expected lines in /home/brian/work/btakita/agent-loop/src/agent-doc/agent-doc-orchestration/src/write.rs: } fn should_consume_queue_prompt_for_diff_content( 2026-06-01T05:30:40.447726Z ERROR codex_core::tools::router: error=apply_patch verification failed: Failed to find expected lines in /home/brian/work/btakita/agent-loop/src/agent-doc/tests/finalize_integration.rs: ); } #[test] fn write_commit_empty_stdin_repair_preserves_active_auto_queue_and_owned_scratch_comment() {
```
---
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
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

## Backlog

<!-- agent:backlog priority queue=prepend -->
- [ ] [#qasm] [recommended] Path C - continue the no-live-pane implementable review items in priority order, starting with the queue-provenance cluster (#prompt-preempts-auto-queue, #queue-user-edit-overwrite, #jb-run-agent-doc-response-queue-contamination), then the Option-A fixes (#cancel-orphans-preflight-cycle, #exchange-prompt-dropped-on-merge, #duplicate-response-after-commit), closeout-guard hardening (#do-id-partial-closeout-state, #preset-item-id-collision-enforce, #queue-clear-unrun-items), the feature pair (#backlog-priority-attribute, #backlog-queue-sync-attribute), then hygiene/UX (#stash-session-ttl-prune, #plugin-cleanup-menu-command, #stash-pane-promote-on-focus, #clear-stops-running-process, #jb-stale-busy-idle-footer, #snrun).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#submodule-response-commit-proof] [recommended] #agent-doc-bug 2026-06-01: a response to `src/boost-client/tasks/monsterrodholders.md` was committed inside the `boost-client` submodule (`6cbff1d`), but the parent `agent-loop` repo still showed `M src/boost-client`, and the closeout wording did not distinguish target-doc commit proof, parent submodule-pointer dirtiness, and the fact that the response bypassed binary-owned `agent-doc write` / `finalize` after recursive same-pane dispatch failed. Fix by making nested-repo commit ownership explicit in session-check/final diagnostics and fail closed on unverified manual fallback. Plan: tasks/agent-doc/plan-submodule-response-commit-proof.md.
- [/] [#ipcaa-scaf] [recommended] #agent-doc-bug 2026-06-01: socket IPC already_applied accepted a prompt-drifted, scaffold-duplicated live buffer for tasks/software/lazily-rs.md (prompt_drift=true, snap_source=file_read), then committed a structurally invalid document with duplicated Queue/Backlog/Review/Done scaffold and partial typed prompt fragments. Fix already_applied snapshot adoption to fail closed or use content_ours when prompt drift or duplicated scaffold is detected. Plan: tasks/agent-doc/plan-ipc-already-applied-scaffold-drift.md.
- [/] [#jb-tsift-pane-sync] [recommended] JetBrains navigation to `tasks/software/tsift.md` and manual `Sync Tmux Layout` did not immediately surface this pane, AND a 2026-06-01 report shows a worse cross-document variant: the pane surfaced for `tsift.md` was actually running `agent-doc-bugs2.md` (wrong-document pane↔owner binding). Plan and fix the regression where automatic selection plus explicit manual sync can leave a live/hidden/stashed target pane invisible or silently deferred OR surface a pane that owns a different document; navigation + manual sync must cleanly autostart/surface the owner for the exact navigated file (never alias/reuse another document's owner) or fail closed naming the blocker. Plan: tasks/agent-doc/plan-jb-tsift-pane-sync-regression.md.
- [/] [#6cmx] [recommended] Path B — run one IPC/typing-corruption live-capture session: enable #com.github.btakita.agentdoc debug logging, then type into a doc during finalize --stream to capture the exact setText payload that unblocks the duplication family #ipcfullprompt-recur2, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing, #finalize-retry-ipc-response-duplication, #zpc2.
- [/] [#jkvs] [recommended] Path A — run one consolidated live busy-pane verification session to batch-close the live-verify-gated review items: install the current signed JB plugin + restart the route-owned agent-doc process, then drive one mid-active-turn pane through #t41j, #jb-busy-queue-deadlock-live-verify, #jb-run-agent-doc-busy-feedback-live-verify, #route-drain-all-surface-live-verify, #codex-owned-pane-stop-hook-live-verify, #codex-owned-pane-prompt-miss-followups (2b/3), #codex-final-response-live-verify, #recovery-drift-apply-live-verify, #jb-load-tmux-window-verify.
- [/] [#4a6p] [recommended] Make agent-doc documents opt-in so a plain .md is not auto-converted into a session. Add is_agent_doc_document(path, content, config) gate (frontmatter field OR .agent-doc config [documents] glob OR auto_session_for_all_md escape hatch); ensure_session_for_file/route/run/write fail closed with an opt-in message instead of injecting session: frontmatter; FFI agent_doc_is_session_document for JB/VSCode SubmitAction.update gating. Plan: tasks/agent-doc/plan-opt-in-agent-doc-documents.md.
- [/] [#j0k8] [recommended] Live-verify the cross-root claim guard (submodule 822688d0): from the superproject root, claim/open a new document from inside a pane already running a different document's submodule agent-doc/codex session, and confirm claim provisions a NEW pane (logs '[claim] pane ... runs a live agent-doc/codex session for another document') instead of aliasing onto it. Also confirm the awear.md recovery restores a real pane. cmdline_owns_other_document is unit-tested; this is the live-pane edge.
- [/] [#do-id-partial-closeout-state] [recommended] Partial-closeout WARN guard landed (check_partial_closeout_state_guard: a do[#id] turn whose response shows shipped + remaining-live-work signals with the directed id still open advises --pending-edit/--pending-gate narrowing; suppressible by a no-partial-closeout-guard response marker; FlowCore guard_ budget 35->40). Remaining: per-id edit-vs-gate tracking in cycle_state, opt-in strict mode, next-phase queue convergence, and the runbook note that #spec-test-commit-push completes a repo phase without a live/deploy item. Plan: tasks/agent-doc/plan-do-id-partial-closeout-state.md.
- [/] [#free-text-queue-skip-diagnostic] [recommended] Polish from #free-text-queue-owner-consume plan point 4: when queue consumption is skipped, the closeout still prints the generic '[queue] skipped consumption because the active prompt did not target the queue head'. For an id-backed head with no completion flag, name the missing proof source and the supported repair (--done/--pending-gate/--pending-edit <id>); for a free-text head edge case, point to the answered-response path. Replace the single generic line at write.rs ~1460/1469 with a shape-aware diagnostic. Plan: tasks/agent-doc/plan-free-text-queue-owner-consume.md (point 4).
- [/] [#preset-item-id-collision-enforce] [recommended] Escalate #preset-item-id-collision from a preflight warning to hard enforcement (the riskier half, deferred to design carefully against over-blocking live sessions): (1) mutation-time rejection — write --pending-add / --pending-add-after / etc. with an explicit custom id (id=X or [#X]) that collides with a prompt_presets key or an existing active item id fails closed; (2) fail-closed dispatch — preflight/session-check block dispatch on an existing collision with the rename repair hint, surfaced by route/JB as a document-repair error not a generic agent failure; (3) queue generation refuses to emit do [#id] for an ambiguous id. The identity registry + detection + preflight warning + 3 regressions already landed (preflight::detect_identity_collisions). Plan: tasks/agent-doc/plan-preset-item-id-collision.md.
- [/] [#codex-owned-pane-stop-hook-live-verify] [recommended] Gated on a live Codex pane -- the remaining phases of #codex-owned-pane-prompt-miss-followups after phase 2a landed: (2b) a live Codex Stop-hook regression asserting the owned_pane_self_invocation diagnostic forces an in-pane response body before final chat output, and (3) live-verify a real Codex same-pane run with an unresolved exchange prompt. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#zpc2] #ipc-corruption-live-repro-lazily-rs [recommended] Live repro of IPC scaffold duplication on tasks/software/lazily-rs.md while typing during finalize --stream. Full Backlog/Queue/Review duplicated plus two escaped exchange-close markers. Partial user typing embedded in wrong positions. Matches detect_duplicated_scaffold(). Plans: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md, tasks/agent-doc/plan-prompt-duplicated-while-typing.md.
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
