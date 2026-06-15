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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260615-035227.md`*

Compacted content:
- Archived 32 response topic(s): [#r18j] supervisor auto-install; make [#r18j] live + push; [#r18j] live + proven; 29 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#0kjc] — quoted-string masking in the marker parser (shipped) — opus-4-8

> **Queue prompt:**
>
> I auto-restart occurred in equityfundingsource.md. But the restart was not successful and was stuck in the shell.
> ```
>   Opus 4.8 ctx:10% ~/work/btakit    Finished `release` profile [optimized] target(s) in 0.08s
>   ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents                               [lib-install] /home/brian/work/btakita/agent-loop/src/agent-doc/target/release/libagent_doc.so -> /home/brian/.cargo/bin/libagent_doc-0.34.1.so (symlink: libagent_doc.so)
>                          [lib-install] note: running controllers still serve the prior binary — run `agent-doc admin recycle --all-projects` (or restart sessions) to promote the new build
>                                                                     [agent-doc] supervisor auto-install succeeded; recycling onto the freshly-installed binary
>                                        [agent-doc] supervisor hot-reloading onto freshly-installed agent-doc binary (idle); preserving the live agent child via execve
>                                                [start] harness resolved: binary=claude source=frontmatter env=default
>                                                                                                                      Registered session 62fe1f41 → pane %74
>                                     Starting claude...
>                                                       [agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)
> ```
>
> I manually restarted then ran JB `Run Agent Doc`.
> ```
> resync: pruned 9 stale session(s)
> [route] target tmux session: 0
> [route] activated existing inactive agent:queue head "All \"Lender account application — incomplete\" components across all pages should be the same component...and have a button to complete the application. deploy" for tasks/professional/equityfundingsource.md (already_present=true, activated=true) before dispatch-only reopen
> Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line. receipt_id=3502
> ```


Done — agent-doc `a99a882d` (superproject pointer `fcf6d21ec`), pushed to `origin/main`.

**Fix.** Half of #0kjc was already implemented: `component::find_code_ranges` (pulldown-cmark) already masks inline code spans and fenced/indented code blocks for marker/tag extraction (`parse()`, `find_non_agent_html_comment_ranges`, `pending.rs`). The missing piece was **double-quoted prose spans** — a marker quoted by name in queue/backlog prose was scanned as a real component, opening an unmatched component and erroring the whole parse (corruption family, alongside `#lintfence`). New `component::find_quoted_ranges(doc)` returns same-line double-quoted spans (backslash escapes handled like `gate_verify::strip_quoted_spans` `#gng8`; an unterminated quote masks only the rest of its own line — fail-safe). `parse()` and `find_non_agent_html_comment_ranges` now skip any comment-open whose start byte is inside a code **or** quoted range.

**Safety.** A real `agent:queue` marker carrying a quoted attribute value is NOT masked — the comment-open precedes the quotes, so the marker start is never inside a quoted span; attribute parsing is untouched (dedicated regression test). Coverage: quoted-marker/quoted-`#id`/fenced-marker(regression)/real-marker-with-quoted-attr tests + a `find_quoted_ranges` same-line/fail-safe unit. `make check` green — **4476 passed, 0 failed**, clippy clean. Not installed mid-session (`#no-mid-session-install`); promotes on next supervisor recycle.

**⚠️ New bug observed live this cycle — compaction dropped a backlog item.** A concurrent compaction mid-finalize archived the exchange to `.agent-doc/archives/…-035227.md` (queue `preset`/`go` attrs preserved — no `#compactqattr` regression there) but **silently dropped the entire `[#0kjc]` backlog item** (pre-compact HEAD `f8a340c44` had 9 backlog items; post-compact `e4d597474` has 8, only `#0kjc` missing). Compaction must only archive the EXCHANGE and leave every other component byte-identical — dropping a backlog **item** is a worse blast-radius variant of `#compactqattr`. Filing as `#compactdropitem`. Because the item was dropped, I could not `--done` it; the completion is durable in git (`a99a882d`) and recorded here. A `--pending-add` to restore it was raced/no-op'd by the stale route-owned supervisor (pid 1654897), so I'm not fighting it further this cycle.

### Re: do [#compactdropitem] — opus-4-8

Fixed. Compaction now **fails closed** if it would drop a whole item from any non-exchange list component, instead of silently committing the loss.

**Root cause (confirmed in code, not the original hypothesis):** the deterministic compact rebuild (`comp.replace_content` on `exchange` only) and the post-compact CRDT refresh (`CrdtDoc::from_text(compacted).encode_state()`) are both pure round-trips — neither drops a backlog item. The live loss (pre-compact 9 → post-compact 8, `[#0kjc]` vanished with no `agent:done` entry) comes from a **concurrent stale-supervisor CRDT merge interleaving over the written file before the compact commit stages the snapshot**. A deterministic-path-only fix would not have caught it.

**Fix** (`agent-doc-orchestration/src/compact.rs`, commit `b571c32c`):
- `non_exchange_list_item_counts()` — per-component tracked item counts for every component except `exchange`.
- `assert_non_exchange_items_preserved()` — bails (exit non-zero) if any non-exchange component's item count decreased; logs `compact_dropped_non_exchange_item file=… stage=… dropped=backlog 9→8` to `ops.log`.
- Two guard points so a racing writer can't slip through:
  - `apply_compacted_document` (`stage=apply`) — guards the intended rebuild before write.
  - `run()` after re-reading the on-disk document (`stage=post_write`) — catches a concurrent CRDT merge that lands between the write and the commit, before the corrupt snapshot reaches HEAD.

`status` reconciliation (top-backlog sentence) and the `exchange` archive are still allowed; only item-count *decreases* on backlog/review/done/queue/icebox fail closed. Same blast-radius family as `#compactqattr`, worse (full-item loss).

**Verification:** `make check` green (exit 0). 4 new deterministic tests: counts backlog+review correctly; passes when only the exchange changes; fails closed with `backlog 3→2` on a dropped item; `run_component_compact` preserves backlog(3)+review(1) with no false positive. Submodule CI baseline green.

**Not yet live:** the fix is committed + release-built but **not installed**, to avoid making the finalize client newer than the already-stale supervisor (pid 1654897) mid-cycle (`#no-mid-session-install`). I'll `cargo install --path .` + recycle the supervisor right after this closeout so the guard goes live. The pre-existing uncommitted `#autorecycle-on-install` work (`lib_install.rs` + `VERSIONS.md`) was left untouched — separate concern, not bundled.

### Re: ~~do [#compactdropitem]~~ (struck) — verified live — opus-4-8

> **Queue prompt:**
>
> :pushpin: :round_pushpin: JB `Run Agent Doc` on equityfundingsource.md. This should self-heal. Not obvious why this is blocked as well.
> ```
> resync: pruned 8 stale session(s)
> [route] target tmux session: 0
> Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line. receipt_id=3457
> ```


Confirmed closed. The fail-closed compact guard is committed **and live in the installed binary** — no follow-up needed.

**Verification this cycle:**
- `b571c32c` (`compact: fail closed if compaction drops a non-exchange item`) is an ancestor of the agent-doc submodule HEAD. ✓
- Installed `~/.cargo/bin/agent-doc` is `0.34.1`, mtime `2026-06-15 00:20:58` — **after** the `b571c32c` commit (`00:17:37`); the prior cycle's promised `cargo install` landed. ✓
- `strings ~/.cargo/bin/agent-doc | grep compact_dropped_non_exchange_item` → **present**. The guard (`assert_non_exchange_items_preserved`, `apply`/`post_write` stages) is in the live binary. ✓

**On the stale cdylib:** the installed `libagent_doc.so` (built `00:14:46`, before the commit) does **not** carry the guard string — but that is a non-issue. The guard lives in `agent-doc-orchestration/src/compact.rs`, and `src/ffi.rs` exposes **no** compact entry point: JB `Compact Exchange` shells out to the `agent-doc` binary (`compact … --commit`), not the FFI. So the guard is reachable from the JB path via the binary that already has it; the cdylib does not gate compaction.

The uncommitted `VERSIONS.md` + `src/lib_install.rs` are the pre-existing `#autorecycle-on-install` work — left untouched, separate concern (not bundled), same as the prior cycle.
<!-- agent:boundary:4058bfde -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
~~~done
I auto-restart occurred in equityfundingsource.md. But the restart was not successful and was stuck in the shell.
```
  Opus 4.8 ctx:10% ~/work/btakit    Finished `release` profile [optimized] target(s) in 0.08s
  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents                               [lib-install] /home/brian/work/btakita/agent-loop/src/agent-doc/target/release/libagent_doc.so -> /home/brian/.cargo/bin/libagent_doc-0.34.1.so (symlink: libagent_doc.so)
                         [lib-install] note: running controllers still serve the prior binary — run `agent-doc admin recycle --all-projects` (or restart sessions) to promote the new build
                                                                    [agent-doc] supervisor auto-install succeeded; recycling onto the freshly-installed binary
                                       [agent-doc] supervisor hot-reloading onto freshly-installed agent-doc binary (idle); preserving the live agent child via execve
                                               [start] harness resolved: binary=claude source=frontmatter env=default
                                                                                                                     Registered session 62fe1f41 → pane %74
                                    Starting claude...
                                                      [agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)
```

I manually restarted then ran JB `Run Agent Doc`.
```
resync: pruned 9 stale session(s)
[route] target tmux session: 0
[route] activated existing inactive agent:queue head "All \"Lender account application — incomplete\" components across all pages should be the same component...and have a button to complete the application. deploy" for tasks/professional/equityfundingsource.md (already_present=true, activated=true) before dispatch-only reopen
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line. receipt_id=3502
```
~~~
- ~~do [#compactdropitem]~~
---
:pushpin: :round_pushpin: JB `Run Agent Doc` on equityfundingsource.md. This should self-heal. Not obvious why this is blocked as well.
```
resync: pruned 8 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line. receipt_id=3457
```
---
:pushpin: JB `Run Agent Doc` should self-heal and complete the dispatch. There was no `- #advance-review` line. Seems to be stuck in an old state.
```
[sync] recovered live pane %25 for session b26b9957 via supervisor pid 1562136
[sync] recovered live pane %57 for session 62fe1f41 via supervisor pid 2378152
resync: stash pane %15 is unregistered but still hosts live supervisor c94cd9fd-71b6-4588-aab5-d53d9c9887f3 — skipping kill
[sync] recovered live pane %25 for session b26b9957 via supervisor pid 1562136
[sync] recovered live pane %57 for session 62fe1f41 via supervisor pid 2378152
resync: stash pane %55 is unregistered but still hosts live supervisor 3d9ff8e4-bafd-4e1f-b48d-550d3c976904 — skipping kill
[sync] recovered live pane %25 for session b26b9957 via supervisor pid 1562136
[sync] recovered live pane %57 for session 62fe1f41 via supervisor pid 2378152
resync: stash pane %50 is unregistered but still hosts live supervisor tsift-v0.1 — skipping kill
[route] target tmux session: 0
[route] activated existing inactive agent:queue head "The new Deal Pipeline kanban is missing the completed application + accepted application from the intake section. deploy" for tasks/professional/equityfundingsource.md (already_present=true, activated=true) before dispatch-only reopen
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line. receipt_id=3460
```
---
:pushpin: :round_pushpin: - :pushpin: Ensure the markdown AST is properly handling strings (quoted text) and code blocks (backtick or triple backtick). Tags within the blocks should be treated as the content, not as part of the document tags.
---
:pushpin: JB `Compact Exchange` on this document
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
:pushpin: JB ` /home/brian/.cargo/bin/agent-doc start --route-owned tasks/professional/equityfundingsource.md`
```
~/work/btakita/agent-loop main ⇡20 *5 !41 ?11                                                     agent-loop 23:02:05
                                                                                                                      ❯ /home/brian/.cargo/bin/agent-doc start --route-owned tasks/professional/equityfundingsource.md
[start] harness resolved: binary=claude source=frontmatter env=default
                                                                      Registered session 62fe1f41 → pane %33
                                                                                                            Error: project controller command `start_session` failed: controller failed to start session actor for /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md
```
---
:pushpin: Spurrious post-compaction in monsterrodholders.md deleted all attributes in `agent:queue` including `preset` and `priority`. Too much blast radius.
:round_pushpin: This document has `agent:backlog priority queue`...but the backlog items are not being added to the `agent:queue`.
---
- ~~:pushpin: [#compactqattr]~~
- :pushpin: [#dupcontent]
---
:pushpin: - do [#dfrefresh-retire]
- do [#1j8q]
---
:round_pushpin: do [#suprehotreload]
- :pushpin: do [#ar27-majors]
- :pushpin: do [#dupcontent]
- :pushpin: do [#compactqattr]
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
- [ ] [#recyclerestart-verify] [operator-verify] #recyclerestart live proof + promotion: Q1 (recycle ENOENT / restart) + Q2 (FFI self-heal sync guard, supersede-newer-generation) are committed (agent-doc 0b31e90c, fa6a230a) but NOT promoted onto the live stale supervisor — the img_23 stall is the pre-fix binary. Recycle/restart the supervisor onto the new build (agent-doc admin recycle --all-projects, or restart), then confirm: kill-pane + JB Sync Tmux Layout no longer reports Sync deferred, and a real idle/next-queue recycle restarts/re-clears the agent session. Cannot be agent-done. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof — two-live-IDE CRDT broadcast convergence: with the installed build, edit the same managed doc in JB and VS Code at once and confirm both editor buffers receive the broadcast patch and converge (no clobber). Implementation landed agent-doc cddad87b (id/deliver/merge, SimWorld-covered, CI green). Cannot be agent-done — needs two live editors. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
- [ ] [#ar27-majors] [clean-session] Major-version dependency bumps held back by Cargo.toml constraints in src/agent-doc (each needs a focused API-migration review, NOT a blanket bump): instruction-files 0.2->0.3, notify 7->8, portable-pty 0.8->0.9, pulldown-cmark 0.12->0.13, rusqlite 0.34->0.40, sha2 0.10->0.11, signal-hook 0.3->0.4, similar 2->3, toml 0.8->1.1, ureq 2->3, yrs 0.21->0.27 (CRDT core, highest risk), zip 2->8, generic-array 0.14.7->0.14.9. Do per-crate with its own make check; yrs/rusqlite/toml/zip are the breaking ones. Follow-up from #ar27 (compatible cargo update landed d0f5475d). CLEAN session (rebuild/reinstall re-stales the live supervisor).
- [ ] [#cleandrainsup-verify] [operator-verify] #cleandrainsup live proof: after the supervisor recycles onto the 1f8c9f63 build, confirm ops.log shows idle_queue_watch_context_reset reason=#cleandrainsup followed by a drain of a [clean-session] head (one fresh agent per item). Plan: tasks/agent-doc/plan-cleandrainsup-supervisor-drains-clean-session.md
- [ ] [#dfrefresh-retire] [clean-session] Retire the dogfood-supervisor-refresh runbook (runbooks/dogfood-supervisor-refresh.md + BUNDLED_RUNBOOKS entry + SKILL.md catalog line + 4 harness mirrors) once #jbrestale (JB Run Agent Doc auto-restarts a stale route-owned supervisor / red-green switchover) AND #recyclerestart (PCP recycle restarts the harness session) both land and prove the binary self-heals a stale supervisor without the manual make+install+restart bootstrap. The runbook is an explicit stopgap for the bootstrap gap (running supervisor predates its own self-recycle fix + is the live agent parent). Acceptance: with both landed, a stale-binary session auto-promotes onto the new build at the next idle/dispatch boundary; delete the runbook + dependent references in one cycle and re-run audit-docs. CLEAN session (skill/runbook surface).
- [ ] [#1j8q] [operator-verify] #suprecyclelock live proof: drive a real next_queue_item supervisor self-recycle (go-mode queue drain over a freshly-installed binary) while a sibling/concurrent launcher holds the shared project-root controller-launch.lock; confirm the execve'd start no longer surfaces "controller launch already in progress ... (os error 11)" on the pane and ops.log shows the contended waiter adopting the published controller instead. Fixed in agent-doc-orchestration project_controller.rs LaunchLock::acquire_blocking + connect_or_launch.
- [ ] [#dupcontent] [clean-session] Duplicate-content / malformed-attribute document corruption when finalize adopts a stale-supervisor `content_ours` snapshot. LIVE 2026-06-14 (monsterrodholders.md, stale route-owned supervisor pid1328300 started 02:15:48 < binary mtime 02:28:10, session-check `#fcc0`): `finalize --stream` hit `mid-turn cycle rotation ... #adoc-compact-during-turn-response-loss` then `live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar content_ours_len=40827 > candidate 39216` then `snapshot from content_ours`. The adopted buffer was CORRUPT and written to disk: 273 lines vs 181 in clean HEAD, TWO `agent:queue` blocks (held one at ~88 PLUS a stale re-armed `preset="#spec-test-commit-push" priority go` block at ~181) and a truncated marker `<!-- agent:queue tasks=" -->`. The lint-gate caught `empty-attr-value` only AFTER the corrupt content already landed on disk (interrupt, no commit; HEAD stayed clean). Repair: `git checkout HEAD -- <file>` + `reset --from-current --preserve-session`. ROOT CAUSE (hypothesis): `content_ours`/`ack_content_sidecar` buffer is adopted verbatim as the snapshot base without structural validation, so a bad prior CRDT merge (duplicate singleton component blocks + attribute split across a merge boundary) is taken as truth and written before the lint-gate runs; the known-stale supervisor is still trusted as an IPC content source. FIX: (1) structurally validate before adopting any content_ours snapshot — reject/repair >1 open marker for a singleton component (exchange/status/queue/backlog/review/icebox/done) or a malformed/unterminated attribute, falling back to candidate/HEAD; (2) fail closed when the serving supervisor is already classified `supervisor_binary_stale` (#fcc0) — do not ingest its buffer; (3) run the structural/lint check on the candidate BEFORE the disk write, not after; (4) de-dup singleton blocks on parse instead of concatenating. CLEAN session (snapshot-adoption / converge write path). Related #fcc0/#compactqattr/#adoc-compact-during-turn-response-loss. Plan: tasks/agent-doc/plan-content-ours-duplicate-corruption.md
- [ ] [#compactqattr] [clean-session] Compaction wipes agent:queue inline attributes (preset, priority, likely go) from the opening marker — operator observed in monsterrodholders.md: post-compaction deleted all agent:queue attributes. Too much blast radius. Compaction should only archive/truncate the EXCHANGE component and must leave every other component opening marker byte-identical. HYPOTHESIS: the post-compact CRDT refresh (compact.rs, CRDT state refreshed from post-compact content) re-serializes component opening tags from parsed structure and drops inline attrs. FIX: preserve the verbatim opening marker (all inline attributes) for every non-archived component across compact + the post-compact CRDT refresh; add coverage that compacting a doc whose agent:queue carries preset/priority/go leaves that marker unchanged. CLEAN session (compaction/CRDT path).
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#aazp] [operator-verify] suprecyclestall live-verify (agent-doc aafa6510, installed+release-built): restart a stale session via session restart-supervisor FILE --force so it runs the new binary, then drive an idle/next-queue-item recycle. Confirm ops.log shows supervisor_binary_stale_self_recycled ... candidates=[...] then EITHER a clean execve hot-reload OR supervisor_reexec_failed fallback=continue_current_binary with per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Subset of the broader recyclerestart sync-block work. Cannot be agent-done: needs a live editor + real recycle.
- [/] [#fcc0] [TOP] [clean-session] Zero JB File Cache Conflict dialogs. Route all document-mutating disk writes through one shared converge_document_or_disk() behind the listener-active gate (editor_ipc primary + disk_fallback); resolve the ipcdrift/stuckhandoff2 wedge that forces fallbacks. CLEAN session required (live write path); 0-dialogs proof operator-drive live JB. Plan: tasks/agent-doc/plan-zero-file-cache-conflict.md
- [/] [#4myd] [operator-verify] #suprecyclestall live-verify: with the new binary installed (cargo install + cargo build --release done, pushed agent-doc aafa6510), restart a stale session and drive an idle/next-queue-item recycle. Confirm ops.log shows `supervisor_binary_stale_self_recycled ... candidates=[...]` then EITHER a clean execve hot-reload OR `supervisor_reexec_failed fallback=continue_current_binary` with the per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Cannot be agent-done: needs a live editor + real recycle.
- [/] [#anw0verify] [operator-verify] #anw0 redirect live proof: install the new agent-doc binary (cargo build --release + cargo install + lib-install) then drive a real recycle / restart-supervisor race so a dispatcher holding a superseded generation hits the new path. Confirm ops.log shows dispatch_stale_generation_redirect prior_generation=N next_generation=N+1 then dispatch_retry_after_stale_generation and a clean re-dispatch (no terminal generation-N-is-closed). Code+SimWorld landed agent-doc 21a70cdd / superproject 412355fa3. Cannot be agent-done: needs a live editor + real recycle.
- [/] [#qdedup] [clean-session] Between-queue-turn trigger enqueue must de-duplicate: successive between-turn enqueues concatenated into one garbled trigger (`/clear /agent-doc <FILE>/agent-doc <FILE>`). Operator: stop the drain once a turn completes, insert into the inactive agent between turns, enqueue UNIQUE (one /clear, one /agent-doc <FILE>) landing once in the PCP/Supervisor between-turn stage. Compose next trigger as a deduped set, gate strictly to the inter-turn boundary, proof line between_turn_enqueue deduped=N. Related #qflood2/#qchurn/#clearcontresume. CLEAN session. Plan: tasks/agent-doc/plan-between-turn-trigger-dedup.md
- [/] [#clearexchstale] [clean-session] JB Clear Exchange clears the exchange but the content comes back (from a stale HEAD/snapshot replay); same session, a deleted+retyped prompt also revived. Same disk-vs-buffer convergence family as #fcc0: a redeliver/replay path re-applies HEAD/snapshot over the cleared buffer, and clear does not advance the durable merge base. FIX: route clear through converge_document_or_disk + advance merge base; gate redeliver on fresh editor-buffer proof (#dvre/#eprf). CLEAN session. Plan: tasks/agent-doc/plan-jb-clear-exchange-stale-head-revival.md
- [/] [#qpresetstrike] [clean-session] Preset-token queue head (a #name from prompt_presets, e.g. #advance-review) is unstrikeable by any agent path AND a spent preset head fails closed instead of auto-repairing. `queue consume` rejects it as id-backed (leading #), `--done`/`--pending-gate` reject it as not a backlog id, and `finalize` keeps the head — so it wedges queue_active=true and re-serves every /agent-doc invocation. LIVE-REPRO 2026-06-13 (efs): operator pressed JB Run Agent Doc expecting auto-repair; dispatch returned 'failed_stage=queue_paused reason=advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the - #advance-review line' (receipt 3426). The pause itself is correct (do not re-run a spent preset) but it fails CLOSED and prescribes a MANUAL line deletion instead of auto-striking the spent head — same fail-closed-instead-of-auto-repair family as #jbrestale. FIX: (1) classify a #-head against prompt_presets AND backlog/icebox before routing; a preset head should strike like a free-text head via consume + the finalize heuristic, while do[#id] heads stay on the --done path. (2) On a spent-preset queue_paused, JB Run / dispatch must auto-strike the spent preset head and re-dispatch the remaining queue ONCE (or cleanly close if empty), not surface a terminal error requiring manual line deletion. Workaround: strike ~~#name~~ + drop go + reset --from-current --preserve-session + commit. Relates to #qchurn/#goqueuestall/#jbrestale. Plan: tasks/agent-doc/plan-preset-queue-head-unstrikeable.md
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
