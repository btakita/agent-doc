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
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260529-080122.md`*

Compacted content:
- Archived 1 response topic(s): #next-steps for the three JB Run Agent Doc / Clear errors
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#ipcfullprompt-recur]. #spec-test-build-install-commit-push
### Re: do [#prompt-duplicated-while-typing] — investigation (gated, not a speculative fix) — opus-4-8

I will not ship a fabricated fix for this one. I did a full static root-cause pass and **ruled out every obvious duplicator** — the native pipeline cannot double a post-boundary prompt on its own:

| Suspected path | Verdict from code |
|----------------|-------------------|
| Native boundary reposition (`template.rs`) | **idempotent** — `remove_all_boundaries()` then re-append one marker; can't duplicate |
| Rust synthesis (`build_ipc_patches_json`, write.rs) | dedup-guarded; synthesizes *agent* content, skips if already present |
| Editor append (`applyComponentPatchNative`) | `appendPatchAlreadyPresentUtil` + `patchReplayAlreadyPresentUtil` short-circuit re-delivery |
| Disk-vs-buffer reposition source (`shouldPreferCommittedDiskContentForRepositionUtil`) | prefers disk **only** when normalized editor==disk, so it won't drop/recompute over freshly-typed post-boundary text |
| EDT apply race | `EditorApplyProof(content, modStamp)` re-checked inside `WriteCommandAction`, rejects mid-apply edits |

**Conclusion:** this is a live EDT-timing race (a patch computed against a buffer snapshot whose `normalizeExchangePrefixes` re-incorporates the typed prompt, overlapping another in-flight apply/reposition for the same cycle within a window the modStamp proof doesn't distinguish). It is **not** reproducible through the deterministic native pipeline, so I can't write a trustworthy regression test or a confirmed fix from static analysis alone.

Per the project's own gates — *no hallucinating under uncertainty*, *manual testing gate*, *verify live system after fix* — the right move is to **capture the race before fixing it**, not to ship a speculative collapse-guard that may never fire or may eat legitimate content.

**Ironically, this very turn reproduced the contention live:** my `finalize` hit `IPC timeout (2s) → CRDT merge → visible write deferred → exit 75` while you were typing `#ipcfullprompt-recur` into the buffer. Same IPC-contention-during-typing window, captured in the runtime log.

**Done this turn:**
- Ruled-out paths + sharpened hypothesis recorded in `tasks/agent-doc/plan-prompt-duplicated-while-typing.md` (commit `c1b6080b6`).
- Concrete next step set: temporary `PatchWatcher` instrumentation logging `patch_id` + pre/post post-boundary user-region on every `setText`, reproduce in IntelliJ while typing during an in-flight write-back, freeze the captured trace into a deterministic test, then implement Option A against the proven path.

**Why gated, not done:** no code fix landed — a safe fix needs the live capture above (instrumentation requires a running IntelliJ session I can't drive headless). `#prompt-duplicated-while-typing` is gated so the queue won't re-attempt it as a fresh `do`. `#ipcfullprompt-recur` (your new backlog item, left open) is the closely-related full-document-IPC-corruption recurrence — same IPC-during-typing root family; both should be tackled together with the instrumentation-first approach.
❯ do [#exchange-prompt-dropped-on-merge]. #spec-test-build-install-commit-push
### Re: do [#ipcfullprompt-recur] — Option 2 + instrumentation (gated) — opus-4-8

Did Option 2 with the additional logging you asked for. Since the corruption isn't reliably reproducible, I instrumented every whole-buffer mutation/adoption seam so the **next** real occurrence is captured automatically — rather than ship a speculative fix.

**Audit first (binary guards already comprehensive).** Both IPC transports already route through fail-closed guards — `guard_ipc_snapshot_adoption_against_live_prompt_drift` + `..._against_prompt_duplication` at the socket *and* file adoption paths in `write.rs`. They demonstrably work (they're why my earlier turn deferred + exited 75 instead of corrupting). The gap was **observability of the paths that proceed**.

**Instrumentation landed:**
- **Binary (`write.rs`):** new `log_ipc_snapshot_adoption_allowed` after the guards on both transports. Previously only *blocked* adoptions logged; *allowed* ones were silent. Now every adoption emits `ipc_snapshot_adoption_blocked` **or** `ipc_snapshot_adoption_allowed` with `snap_source`, snapshot/content_ours len+hash, and an independent `drift_recheck`/`dup_growth_recheck`. A non-benign re-check on an *allowed* line is a smoking gun for a guard-coverage gap.
- **JetBrains (`PatchWatcher.kt`, v0.2.135):** `documentMutationDiagnosticUtil` logged at every editor-visible `setText` (`applyPatch.component`, `repositionBoundary`) — op, patch_id, transport, pre/post fingerprint, mod-stamp, idle, and the **post-boundary user-region length pre vs post** with `user_region_changed`. That flag is the direct prompt-duplication/deletion signal.

**Verification:** `make check` green (incl. FlowCore token-budget gate); new Rust tests (`ipc_snapshot_adoption_allowed_*`) and Kotlin tests (`DocumentMutationDiagnosticTest`) pass; both binaries rebuilt; JB plugin v0.2.135 built + signed.

**How to capture the packet now** (next time it happens in IntelliJ): `idea.log` shows `[patch-watcher] document-mutation … user_region_changed=true` for the offending `setText`, and `.agent-doc/logs/ops.log` shows the paired `ipc_snapshot_adoption_(allowed|blocked)` line with the content shape. That pinpoints transport + payload → drives the SimWorld regression + real fix.

**Committed + pushed:** submodule `be0d5297` → `origin/20260422/codex-agent-backend`; superproject `cf64e89e6` (pointer + plan) → `origin/main`. **Install the v0.2.135 JB plugin** (`editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.135-signed.zip`) to get the editor-side diagnostics live.

**Why gated, not done:** this is instrumentation, not a behavioral fix — acceptance needs a captured packet + the resulting SimWorld regression. Reproduce once with the new logging and share `idea.log` + `ops.log`, and I'll land the real fix against the proven path.
❯ I upgraded the JB plugin.

### Re: I upgraded the JB plugin — diagnostics are live — opus-4-8

Good — v0.2.135 means the editor-side instrumentation is now active, and a responsive IPC plugin should also clear the editor-buffer-vs-disk closeout blocker we kept hitting (this cycle committing cleanly is the proof).

**What's now capturing**, with no further action from you:
- **Editor (`idea.log`):** every visible `setText` logs `[patch-watcher] document-mutation op=… patch_id=… transport=… pre=… post=… user_region_changed=…`. `user_region_changed=true` on a `repositionBoundary`/`applyPatch.component` line is the direct prompt-duplication/deletion signal.
- **Binary (`.agent-doc/logs/ops.log`):** every IPC snapshot adoption now logs `ipc_snapshot_adoption_allowed` or `ipc_snapshot_adoption_blocked` with `snap_source` + len/hash + `drift_recheck`/`dup_growth_recheck`.

**When `#ipcfullprompt-recur` (or the prompt-duplication sibling) next fires:** grab the `[patch-watcher] document-mutation … user_region_changed=true` line from `idea.log` plus the paired `ipc_snapshot_adoption_*` line from `ops.log` for the same patch_id, and send them over. That pins the transport + payload, and I'll land the real fix + a frozen SimWorld regression against the proven path.

To confirm the editor diagnostics are wired, you can enable debug logging for the category if needed: **Help → Diagnostic Tools → Debug Log Settings →** add `#com.github.btakita.agentdoc`. The `document-mutation` lines log at INFO so they should appear in `idea.log` regardless.

Both gated items (`#ipcfullprompt-recur`, `#prompt-duplicated-while-typing`) stay parked until a captured packet arrives.
❯ agent-doc on monsterrodholders.md still ate the prompt "go".
❯ #spec-test-build-install-commit-push

### Re: agent-doc ate the prompt "go" — root cause found + fixed — opus-4-8

The instrumentation paid off — it caught the real cause, and it was **not** where any of the plans suspected (not a PatchWatcher `setText`, not the IPC snapshot guard — those fired and recovered correctly). idea.log had the smoking gun:

```
[route] removed duplicate answered prompt tail after exchange boundary in tasks/monsterrodholders.md
```

**Root cause (binary route path, not the editor).** `route.rs::scrub_duplicate_prompt_comments_for_route` → `template::remove_duplicate_answered_exchange_prompt_tail` deletes a post-boundary prompt when it text-matches an earlier answered prompt. And `normalize_duplicate_exchange_prompt_line` **strips the `❯` marker** before comparing — so your freshly-typed `go` (unprefixed) matched a previously-answered `❯ go` and was silently deleted. `go`/`yes`/`continue` are exactly the short prompts that recur, so it fired easily. The binary IPC guards never even saw it; the prompt was gone before dispatch.

**Fix.** `duplicate_exchange_tail_prompt_lines` now requires every tail line to carry the `❯ ` answered-form marker before it can be scrubbed as replay residue. That marker is added only by the answer/normalize cycle — never by a user typing a fresh prompt — so it's the ownership proof the plan demanded. A bare unprefixed post-boundary prompt is now **always preserved and dispatched**. Worst case for genuine residue that somehow lost its marker: it gets re-dispatched (recoverable), never deleted (data loss).

**Verification (deterministic — no live repro needed for this one):**
- `make check` green. New tests: core `remove_duplicate_answered_tail_preserves_unprefixed_live_prompt` (the `go` regression) + `..._scrubs_prefixed_replay_residue`; route `route_preserves_unprefixed_live_prompt_matching_an_answered_prompt`; updated the route/preflight/normalize fixtures to model real answered-form (prefixed) residue.
- `cargo install` + `cargo build --release` — both binaries rebuilt, so the fix is live for new route dispatches.

**Committed + pushed:** submodule `bb99c4f0` → `origin/20260422/codex-agent-backend`; superproject `ad1286552` (pointer + plan) → `origin/main`.

Marking `#ipcfullprompt-recur` **done** — this was the data-loss cause and it's fixed with a regression test that will catch any recurrence. The v0.2.135 diagnostics stay in place to catch any *other* whole-buffer facet (stray `###`, stuck capture), which the binary guards already fail-closed + recover. If `go` still vanishes after this, that diagnostic pair will tell us immediately — but the regression test says it won't.
<!-- agent:boundary:89c7df16 -->
<!-- /agent:exchange -->
###
<!--
JB `Run Agent Doc` error
```
[route] debounce OK — file idle for 0.5s and typing indicator idle
[route] target tmux session: 0
Error: authoritative actor generation 204 for tasks/agent-doc/agent-doc-bugs2.md owns pane %8 but route could not drain the active closeout before dispatch: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. Rebuild sidecars without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
```
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

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#prompt-duplicated-while-typing]~
- do [#duplicate-response-after-commit]
- do [#adoc-queue-ipc-buffer-divergence]
- do [#queue-strike-on-halt]
- do [#jb-stale-busy-idle-footer]
- do [#pending-add-append-not-prepend]
- do [#next-steps-backlog-capture]
- do [#run-agent-doc-stale-busy-replay]
- do [#jb-busy-queue-ux-feedback]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#exchange-prompt-dropped-on-merge] User-authored exchange prompt (observed: "go") silently dropped during an IPC content_ours merge while the IDE held ipc.sock — never reaches snapshot/HEAD, assistant never responds, next preflight reports no_changes. Same drift family as #adoc-queue-ipc-buffer-divergence / #duplicate-response-after-commit but the victim is user input. Repro 2026-05-29 on src/boost-client/tasks/monsterrodholders.md (dropped the "go" authorizing #gfwconnect; assistant replied "nothing changed" for two cycles). Plan: tasks/agent-doc/plan-exchange-prompt-dropped-on-merge.md (recommend Option A — union user prompt lines / fail closed + add dropped_prompt_guard to session-check).
- [ ] [#duplicate-response-after-commit] Late-IPC reposition signal re-inserts already-committed response body into working tree, wrapped in duplicate boundary markers. Symptom: next preflight interrupts with "direct response patchback without agent-doc cycle"; agent-doc write --commit returns "empty response — nothing to write"; only manual Edit-based dedup recovers. Repro: tasks/dreamable/separation.md cycle after commit 231eafc83 on 2026-05-28. Plan: tasks/agent-doc/plan-duplicate-response-after-commit.md (recommend Option A — both detect over-application in session-check and make IPC reposition idempotent).
- [ ] [#adoc-queue-ipc-buffer-divergence] Root cause #4 FIXED (submodule 729f7bc2): response-replay / stale-cycle hash matching now neutralizes the agent:queue component (tag + body) and the queue_active frontmatter, so queue-maintenance churn no longer wedges a committed cycle as stuck_captured_cycle. REMAINING #2 (live IDE buffer re-adds auto / diverges from committed inactive snapshot): blocker found — ipc_socket::send_patch carries component CONTENT only and cannot converge a queue opening-tag attribute (auto) or the queue_active frontmatter; try_ipc_full_content is disabled and rejects template docs, so no IPC seam exists yet. Needs a plugin protocol extension (PatchWatcher.kt + FFI) to converge queue tag + frontmatter to the editor buffer, plus a deterministic live-IPC-listener repro harness (SimWorld). Plan: tasks/agent-doc/plan-queue-ipc-drift.md.
- [ ] [#queue-strike-on-halt] Queue head consumed on halt/refusal response. write.rs:1233-1242 + response_explicitly_targets_queue_head (write.rs:2481) treat any `### Re: <head-id> <suffix>` heading as completion signal even when body explicitly says "backlog left intact" + passes no --done/--pending-gate. Repro: tasks/agent-doc/open-code.md cycle 619af4d0b struck [#d97v] after halt response. Plan: tasks/agent-doc/plan-queue-strike-on-halt-response.md (recommend Option A — require explicit completion signal).
- [ ] [#jb-stale-busy-idle-footer] Three JB Run Agent Doc / Clear Session Context reports queue-instead-of-dispatch or block Clear while the live pane footer shows a Claude Code idle composer: monsterrodholders.md (actor gen 233, pane %19), equityfundingsource.md (gen 35, pane %3), tsift.md (pane %21, footer 22.4K (11%) ctrl+p commands). The tsift.md case is the strongest stale-busy candidate — Clear was blocked though the footer indicates idle. Fix: route.rs / session_actor_cmd.rs busy projection should treat the Claude Code idle-composer footer (ctrl+p commands / context % used) as idle direct evidence to repair a stale busy actor/lease projection before queuing or blocking, mirroring the existing OpenCode context % used idle-composer rule. Relates to #run-agent-doc-stale-busy-replay and #snrun (plan tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md).
- [ ] [#pending-add-append-not-prepend] `--pending-add` appends new items below the previous CLI arg within the same finalize cycle, contradicting SKILL.md "Pending capture rule" ("Put new items at the beginning of agent:backlog"). Repro: tasks/agent-doc/open-code.md commit f94e7851c — agent added wave-4, wave-3, wave-2, wave-1 expecting wave-1 on top per SKILL.md; actual backlog was wave-4 on top (pure CLI-arg order). User flagged it as bug; agent had to issue --pending-reorder. Plan: tasks/agent-doc/plan-pending-add-prepend-semantics.md (recommend Option A — strict prepend, drop the implicit adjacency heuristic).
- [ ] [#next-steps-backlog-capture] Agent responses containing #next-steps recommendations do not auto-add items to backlog when prompt preset is present. Agent must manually call `--pending-add` but the planning output `pending_mutations: expect_add` is not enforced by session-check `pending_done_guard`.
- [ ] [#run-agent-doc-stale-busy-replay] Follow-up to the #snrun stale-busy-projection fix (submodule 89c99c79): the dispatch-only busy-vs-stale repair is covered by unit tests on the pure decision functions; add a live JB Run Agent Doc replay regression (SimWorld or tmux-ci) exercising a stale busy actor projection + proven ready prompt and asserting dispatch instead of agent:queue auto enqueue. Also still open in #snrun: distinguishing rescued-stash accepted-only proof misses. Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
- [ ] [#jb-busy-queue-ux-feedback] [recommended] When the authoritative actor is genuinely busy, JB Run Agent Doc correctly queues the dispatch in agent:queue auto instead of injecting a duplicate trigger (monsterrodholders.md, equityfundingsource.md reports), but the plugin surfaces the route/resync log dump as an error. Present queued-behind-a-busy-turn as an informational notification distinct from real failures so expected busy-queuing does not read as a plugin error.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#prompt-duplicated-while-typing] User prompt duplicated in the live editor while typing — in-progress prompt text appears twice in the buffer during/after typing, before any new agent-doc cycle. Distinct from #duplicate-response-after-commit (response body) and #adoc-queue-ipc-drift (queue/snapshot loop). Suspected: PatchWatcher.kt normalize+setText (lines 774/801) double-counts post-boundary keystrokes against the try_ipc() boundary-aware synthesized exchange patch payload; await_idle debounce narrows but does not close the race. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md (recommend Option A — single-source post-boundary user region + idempotent apply).
- [/] [#queue-auto-no-continue] Queue auto trigger does not continue after finalize session-check warning about pending shadow items. Steps to reproduce: (1) Session document with queue_active=true, queue_trigger=auto, and multiple queued items. (2) finalize --stream completes cycle but session-check issues pending shadow warning for items outside live agent:backlog. (3) Queue consumes head item but auto trigger does not fire for next item. (4) agent-doc write --commit with --done reaps the item, queue reports 9 remaining but auto continues stop. Expected: queue auto should continue to next item after successful closeout. Actual: queue stops after first item. Session: tasks/software/tsift.md. agent-doc version: 0.34.0. The pending shadow warnings about items appearing both inside and outside agent:backlog may be blocking the auto-continue path.
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
