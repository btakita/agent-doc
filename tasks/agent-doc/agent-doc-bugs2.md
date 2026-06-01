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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260601-083030.md`*

Compacted content:
- Archived 3 response topic(s): JB `Run Agent Doc` dispatch latency; do [#j0k8]; do [#j0k8]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ JB `Run Agent Doc` on this document is deadlocked.
❯ #spec-test-build-install-commit-push

### Re: JB `Run Agent Doc` deadlock on this document — opus-4-8

Root-caused and fixed. This document's owner pane is **this Claude session (`%50`)**, and the deadlock is a stale-busy projection vs. the `--wait-for-ready` timeout.

#### What was happening (from `ops.log`)
```
route_wait_for_ready_override ... timeout_secs=60
route_authoritative_actor_starting_not_ready ... actor_state=busy ... prompt_ready=true last_transition_reason=live_pane_idle
route_wait_for_ready_timeout ... elapsed_ms=60119 timeout_ms=60000
route_authoritative_actor_busy_projection_repaired_by_ready_prompt ... prior_state=busy
route_dispatch_only_sent ... proof=accepted
```
The authoritative actor was projected `Busy`, but the live pane already proved a current-generation dispatch-ready prompt (`prompt_ready=true`) — a **stale busy projection**, not a real active turn. The repair that promotes Busy→Ready and dispatches (`busy_projection_repaired_by_ready_prompt`) only ran **after** the full 60s `--wait-for-ready` timeout that JB `Run Agent Doc` passes. So every click blocked ~60s before dispatching, and one path (`route_dispatch_only_authoritative_actor_busy_focus_only_not_dispatched`) fell through to focus-only with **no dispatch at all** — the deadlock.

#### Fix (submodule `fd98eb9d`)
`classify_prompt_ready_barrier` (the wait-loop barrier) now recognizes the busy-projection repair **inline**: a `Busy` actor with a proven ready prompt + dispatch eligibility resolves to `Ready` on the first poll, dispatching immediately instead of spinning to the deadline. A `Busy` projection *without* a proven ready prompt still stays fail-closed (keeps waiting, then queues) — the no-inject-into-an-active-turn invariant is preserved, since `prompt_ready` for a `Busy` actor is only ever true when the live capture matched a harness dispatch-ready prompt.

- `agent-doc-orchestration/src/flow/routed_reopen.rs` — barrier repair + `prompt_ready_barrier_repairs_busy_projection_with_proven_ready_prompt` unit test.
- `specs/08a-session-actor-contract.md` — documents the wait-barrier repair timing contract.

#### Verification
- New unit test passes; route + routed_reopen subsystem green (158 passed, 104 live-tmux ignored). Clippy `-D warnings` clean.
- `cargo build --release` + `cargo install --path .` — both binaries refreshed.
- Pushed `fd98eb9d`; CI run `26744782255` in progress.
- **Note on local `make check`:** 2 `graph::tests` (lazily-rs reactive cache invalidation) are red **locally only** — caused by uncommitted concurrent work in `src/lazily-rs/src/context.rs` (a sibling submodule with working-tree drift), not by this change and not in the route path (`a15ff6e6`: "lazily-rs is not in the pane path"). CI uses the pinned lazily-rs ref and is green; prior commits on this branch (`984e5c1e`, `6d191f1f`) passed CI with these same graph tests present. Left untouched per the concurrent-work rule.

You should now be able to click `Run Agent Doc` and have it dispatch immediately even while the owner pane was momentarily projected busy. If you still see a hang, grab `ops.log` and look for a fresh `route_authoritative_actor_busy_projection_repaired_by_ready_prompt` with a low `elapsed_ms`.

### Re: do [#j0k8] (queue continuation) — opus-4-8

> **Queue prompt:**
>
> do [#j0k8]


Gated `#j0k8` rather than executing it — this is a **live-pane verification**, not autonomously completable from here.

`#j0k8` = "live-verify the cross-root claim guard": drive a second pane already running another document's submodule `agent-doc`/codex session and confirm `claim` provisions a *new* pane instead of aliasing, plus confirm the `awear.md` recovery restores a real pane. The item itself notes the code path (`cmdline_owns_other_document`) is already **unit-tested** — only the live-pane edge remains. Reproducing a real cross-session multi-pane claim and observing the outcome needs an operator-driven tmux session, and is explicitly risky to drive autonomously in this racy multi-session repo.

So I gated it (`[/]`, code-complete / live-verify pending) to advance the queue honestly instead of marking it done without proof or spinning the auto-loop on it.

**Remaining queue is the same shape:** `#jkvs` (Path A — consolidated live busy-pane session), `#qasm` (Path C — no-live-pane implementable review items), `#6cmx` (Path B — IPC/typing-corruption live-capture). `#qasm` is the one batch that's actually implementable without a live pane — say the word and I'll work that one. The two `Path A/B` items need a live session you drive.

To live-verify `#j0k8` when you're ready: open a fresh doc from inside a pane already running another document's submodule session and watch the log for `[claim] pane ... runs a live agent-doc/codex session for another document` + a new pane being provisioned.
re #j0k8, I may want to have a md file that is not an agent doc. But the agent-doc template auto-generates. How can we make this opt-in?

### Re: making agent-doc opt-in for plain .md files — opus-4-8

Good catch — today **every `.md` is auto-treated as a session**, and there's no opt-in gate. Here's exactly where it happens and how to make it opt-in.

#### Why it auto-generates today
`route` / `run` / `write` unconditionally call `frontmatter::ensure_session_for_file` (`agent-doc-orchestration/src/frontmatter_io.rs:58` → `ensure_session`, `agent-doc-core/src/frontmatter.rs:860`). For a `.md` with no frontmatter it **injects `session: <uuid>`**, and template-mode defaults then scaffold `agent:exchange` / `agent:queue` / `agent:backlog` on first write. The JB plugin compounds it: `SubmitAction.update` enables **Run Agent Doc** for *any* file whose extension is `md`. So one stray Run (or a watch/sync touch) converts a notes/README file into a session.

#### Opt-in design (proposed — plan written)
A single fail-closed predicate, `is_agent_doc_document(path, content, config)`, true when **any** of:
1. **Frontmatter marker** — `agent_doc_format` / `agent_doc_write` / `agent_doc_mode` / an existing `session:`.
2. **Project-config glob** — `.agent-doc/config.toml` `[documents] include = ["tasks/**/*.md", "plan.md"]`.
3. **Escape hatch** — `auto_session_for_all_md = true` (default `false`) restores today's behavior.

Then:
- `ensure_session_for_file` returns a typed `NotAnAgentDocDocument` error for a non-opted-in `.md` instead of injecting frontmatter; `route`/`run`/`write` **fail closed without mutating the file**, printing: *"`<file>` is not an agent-doc document. Run `agent-doc init <file>` or add `agent_doc_format: template` to opt in."*
- `agent-doc init <file>` stays the explicit per-file opt-in (scaffolds).
- New FFI `agent_doc_is_session_document(path)` (Shared Foundation) lets the JB/VS Code plugins gate `SubmitAction.update`, so **Run Agent Doc** doesn't even appear on a plain `.md`.

**Back-compat:** existing docs already carry `session:`/`agent_doc_format:`, so they stay sessions — only new plain `.md` change behavior.

Full plan: `tasks/agent-doc/plan-opt-in-agent-doc-documents.md`; queued as a `[recommended]` backlog item.

#### Two choices I need from you before implementing
1. **Opt-in surface:** per-file frontmatter only, project-config glob only, or **both** (my recommendation: both, frontmatter wins)?
2. Should `agent-doc init` also offer to add a `[documents]` glob, or stay purely per-file?

Say the word (with `#spec-test-build-install-commit-push` if you want it shipped this cycle) and I'll implement against the plan.
❯ A new session was created. But there was a JB error
```
Claim failed (exit 1): [claim] auto-scaffolding empty file: /home/brian/work/btakita/agent-loop/tasks/recruit/test.md [commit] repositioned boundary in snapshot [commit] skipping working-tree boundary reposition — IPC listener active [perf] commit.staging (hash_object+update-index): 10ms [perf] commit.git_commit: 10ms [main 0899cd3b7] agent-doc(test): 2026-06-01 05:05:05 [commit] IPC reposition boundary signal sent [commit] VCS refresh signal written [perf] commit total: 22ms Error: pane %18 is in tmux session '1' but project session is '0'; switch to the configured session or pass --force
```

### Re: JB claim error — auto-scaffold + cross-session reject — opus-4-8

> **Queue prompt:**
>
> do [#4a6p]


Fixed the immediate bug. Your error had two parts; the damaging one is an ordering bug.

#### What the error showed
```
[claim] auto-scaffolding empty file: tasks/recruit/test.md
[main 0899cd3b7] agent-doc(test): ...            ← already committed!
Error: pane %18 is in tmux session '1' but project session is '0'; switch to the configured session or pass --force
```
`claim` **scaffolded and committed** the empty `test.md` *before* validating the pane. Then the cross-session guard rejected (your JB pane `%18` was in tmux session `1`, but the project session is `0`). So a file you never opted into got converted to a session and committed, and the claim still failed.

#### Fix (`#claim-validate-before-scaffold`, submodule `7e07f6d1`)
Moved pane resolution + the cross-session guard **ahead of** the empty-file auto-scaffold in `claim.rs` (pane validation needs only the pane id + configured session). A rejected cross-session claim now **fails closed and leaves the file byte-identical** — no scaffold, no `agent-doc(<doc>)` commit. This honors the existing `// Pane validated — now safe to modify files` invariant, which the auto-scaffold had been jumping ahead of. Documented in `specs/07-session-tmux-commands.md`.

- Verified: `make check` fully green (the earlier local `graph::tests` red from concurrent lazily-rs drift has cleared); claim tests pass (16). `cargo build --release` + `cargo install` refreshed both binaries. Pushed `7e07f6d1`.

#### About the cross-session reject itself
That guard is intentional — `claim` refuses to bind a pane that lives in a *different* tmux session than the project's configured one (here pane in `1`, project `0`), unless you pass `--force` or switch the pane to session `0`. The fix above just ensures a refused claim no longer mutates the file first. If you frequently work across tmux sessions and want claim to adopt the pane's session automatically, that's a separate policy change — tell me and I'll scope it.

#### This is one slice of `#4a6p` (opt-in) — gated pending your input
The auto-scaffold-on-reject was a symptom of the broader "every `.md` is auto-converted" issue you raised. The full opt-in feature (`#4a6p`, plan `tasks/agent-doc/plan-opt-in-agent-doc-documents.md`) is still **blocked on your two answers** from last cycle, so I gated it rather than auto-draining a design-blocked item:
1. Opt-in surface — per-file frontmatter, project-config glob, or **both** (my recommendation)?
2. Should `agent-doc init` also offer to add a `[documents]` glob, or stay per-file?

Answer those (add `#spec-test-build-install-commit-push` to ship) and I'll ungate `#4a6p` and implement the full gate.

(FYI: the exchange is getting large — 6 cycles in 30 min. Say the word if you want me to `agent-doc compact` it.)
<!-- agent:boundary:a28992c1 -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- ~do [#4a6p]~
- ~do [#t3m5]. Apply all 3 optimizations.~
- ~do [#j0k8]~
- do [#jkvs]
- do [#qasm]
- do [#6cmx]
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
- [ ] [#jkvs] [recommended] Path A — run one consolidated live busy-pane verification session to batch-close the live-verify-gated review items: install the current signed JB plugin + restart the route-owned agent-doc process, then drive one mid-active-turn pane through #t41j, #jb-busy-queue-deadlock-live-verify, #jb-run-agent-doc-busy-feedback-live-verify, #route-drain-all-surface-live-verify, #codex-owned-pane-stop-hook-live-verify, #codex-owned-pane-prompt-miss-followups (2b/3), #codex-final-response-live-verify, #recovery-drift-apply-live-verify, #jb-load-tmux-window-verify.
- [ ] [#qasm] [recommended] Path C — work the no-live-pane implementable review items in priority order: #free-text-queue-skip-diagnostic, #queue-auto-no-continue, then the queue-provenance cluster (#prompt-preempts-auto-queue, #queue-user-edit-overwrite, #jb-run-agent-doc-response-queue-contamination), the Option-A fixes (#cancel-orphans-preflight-cycle, #exchange-prompt-dropped-on-merge, #duplicate-response-after-commit), closeout-guard hardening (#do-id-partial-closeout-state, #preset-item-id-collision-enforce, #queue-clear-unrun-items), the feature pair (#backlog-priority-attribute, #backlog-queue-sync-attribute), then hygiene/UX (#stash-session-ttl-prune, #plugin-cleanup-menu-command, #stash-pane-promote-on-focus, #clear-stops-running-process, #jb-stale-busy-idle-footer, #snrun).
- [ ] [#6cmx] [recommended] Path B — run one IPC/typing-corruption live-capture session: enable #com.github.btakita.agentdoc debug logging, then type into a doc during finalize --stream to capture the exact setText payload that unblocks the duplication family #ipcfullprompt-recur2, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing, #finalize-retry-ipc-response-duplication, #zpc2.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
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
