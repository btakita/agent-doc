---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Added post-fix cleanup next steps after the decision-separation backlog. Top backlog item remains #dvre.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-014122.md`*

Compacted content:
- Archived 2 response topic(s): full-content IPC replacements; post-exchange HTML comment deletion
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Why is the document stale, causing the Full document IPC to run?
### Re: document staleness and full-content IPC — gpt-5

The document was not stale in the sense of a stale-response fallback causing the full-document IPC. The full-document IPC was the expected Compact Exchange path.

The relevant ops-log sequence is:

- `2026-05-22 21:41:22 EDT`: `flow=document_mutation stage=pre_write_guard outcome=completed reason=full_content_source_buffer_apply:compact_exchange`
- same second: `source=try_ipc_full_content_socket write_mode=socket_full_content ... live_exchange_edited=false prompt_text_duplicated=false`
- same second: snapshot save and `commit_success`

That means the source-buffer guard accepted the full-content replacement for a compact operation. If the source buffer had actually gone stale before delivery, this path would have logged `full_content_source_buffer_reject_stale_source_buffer:compact_exchange` plus `full_content_source_buffer_rejected`, and it would have skipped the IPC payload.

Why full-document IPC ran: `compact` calls `try_ipc_full_content_operator_mutation_from_source(...)` because compacting exchange rewrites the whole markdown document around the archived exchange summary. The editor spec explicitly requires Compact Exchange to use `fullContent` so the active editor buffer is updated through the editor API instead of getting an external-file-change dialog.

The stale-looking part happened after that commit. At `2026-05-22 21:42:03 EDT`, the snapshot was `2610` bytes and the working file was `2692` bytes because your new prompt had been added after the compact commit. `commit` classified that as `post_commit_user_follow_up`, and this preflight then diffed exactly that new prompt against the last committed snapshot. So the current drift is normal unanswered-user-prompt drift, not evidence that the compact IPC replacement was stale.

Code points I checked:

- `src/agent-doc/src/compact.rs`: compact sends `try_ipc_full_content_operator_mutation_from_source`.
- `src/agent-doc/src/write.rs`: full-content IPC checks the source buffer before delivery and rejects stale source buffers.
- `src/agent-doc/src/flow/document_mutation.rs`: the decision is either `apply` or `reject_stale_source_buffer`.
- `src/agent-doc/editors/SPEC.md`: Compact Exchange is supposed to emit a `fullContent` IPC payload.

Verification: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, ops-log inspection, and source inspection. No code changes were made.
❯ So the issue is triggered by `Compact Exchange`?
### Re: Compact Exchange trigger — gpt-5

Yes, but with an important distinction.

`Compact Exchange` triggered the full-document IPC path. That part is intentional: compacting the exchange rewrites the document around the archived summary, so the binary sends a `fullContent` payload to the editor instead of a component append patch.

What I do not see in the logs is evidence that Compact Exchange triggered a stale replacement or the duplicate/corrupt prompt issue in that specific cycle. The compact full-content write logged `full_content_source_buffer_apply:compact_exchange`, `socket_full_content`, `live_exchange_edited=false`, and `prompt_text_duplicated=false`, followed by `commit_success`. If it had been stale, the expected log would be `full_content_source_buffer_reject_stale_source_buffer:compact_exchange` / `full_content_source_buffer_rejected`, and the IPC payload would have been skipped.

So the accurate framing is:

- Trigger for full-document IPC: yes, `Compact Exchange`.
- Proven trigger for stale/corrupt duplicate content: not from the evidence I checked.
- Risk surface worth testing: yes. Because compact uses whole-document replacement, any plugin-side proof/ack bug there has a larger blast radius than a normal exchange append.

The follow-up prompt after compact was normal post-commit drift: the snapshot stayed at the compact commit, then your new question appeared in the working document and preflight picked it up as the next prompt.

Verification: `agent-doc preflight` and `agent-doc plan`; reused the previous source/log inspection for the compact full-content path. No code changes were made.
❯ What is triggering the full-document IPC?
### Re: full-document IPC trigger — gpt-5

In the observed case, the trigger is the compact closeout path:

`agent-doc compact ...` -> `apply_compacted_document(...)` -> `try_ipc_full_content_operator_mutation_from_source(...)` -> `try_ipc_full_content_with_mode(..., FullContentIpcMode::OperatorMutation, ...)` -> IPC payload with `fullContent`.

That is why the ops log labels it as:

- `full_content_source_buffer_apply:compact_exchange`
- `source=try_ipc_full_content_socket`
- `write_mode=socket_full_content`
- `patches=0 unmatched_len=0`

The `patches=0` part is expected for this path because it is not sending a template component patch. It is sending an entire replacement document as `fullContent`.

There are other full-document IPC callers in the codebase, but they are different paths:

- response fallback repair: `try_ipc_full_content(...)` with `FullContentIpcMode::ResponseFallback`
- normalization fallback redelivery: `redeliver_normalization_fallback_to_editor(...)`
- IPC duplicate-response repair redelivery: `redeliver_ipc_dedupe_to_editor(...)`

Those are not what the compact log showed. For this document's recent full-document IPC, the concrete trigger was Compact Exchange's operator mutation path. Normal `finalize` responses after your follow-up questions are going through component patch IPC (`try_ipc_socket` or file IPC), not the full-document compact path.

Verification: `agent-doc preflight`, `agent-doc plan`, source grep for `try_ipc_full_content_operator_mutation_from_source`, and ops-log grep for `full_content_source_buffer_apply:compact_exchange`. No code changes were made.
❯ What full-document IPC path is causing the duplicate corrupt document & the duplicate prompt issues?
### Re: duplicate/corrupt full-document IPC path — gpt-5

The full-document IPC path tied to the duplicate-prompt/corrupt-document class is not Compact Exchange. It is the repair redelivery path that runs after a normal component IPC closeout detects bad post-apply content.

There are two concrete repair callers:

- Duplicate prompt repair: `dedupe_ipc_snapshot_content(...)` detects duplicate prompt residue, logs `ipc_prompt_duplicate_repaired`, then calls `redeliver_ipc_dedupe_to_editor(...)`.
- Prefix/sidecar divergence repair: `verify_sidecar_normalization(...)` fails, the binary builds a `normalized_content_ours_fallback(...)`, then calls `redeliver_normalization_fallback_to_editor(...)`.

Both redelivery functions call `try_ipc_full_content(...)`, which uses `FullContentIpcMode::ResponseFallback` and sends a whole-document `fullContent` payload. That is the full-document IPC path I would treat as relevant to the duplicate/corrupt issue.

The logs support that distinction. On `2026-05-15 10:07:24 EDT`, this document logged `ipc_prompt_duplicate_repaired` immediately followed by `ipc_dedupe_redelivered_editor`. There are also many older `sidecar_normalization_fallback_redelivered_editor` entries for this document starting around `2026-05-09 10:23:13 EDT`. Those are response-fallback full-document repairs, not compact operator mutations.

Compact Exchange also uses full-document IPC, but it logs as `full_content_source_buffer_apply:compact_exchange` and `write_mode=socket_full_content` with `patches=0`. In the recent compact cycle, that path reported `live_exchange_edited=false` and `prompt_text_duplicated=false`, so it is not the evidence trail for the duplicate prompt issue.

So the answer is: the suspect full-document IPC path is `redeliver_ipc_dedupe_to_editor(...)` / `redeliver_normalization_fallback_to_editor(...)` -> `try_ipc_full_content(...)` in `ResponseFallback` mode. The normal trigger is a component IPC apply that produces duplicate prompt residue or sidecar normalization divergence; the full-document IPC is then used as a repair/redelivery mechanism.

Verification: `agent-doc preflight`, `agent-doc plan`, ops-log grep for `ipc_prompt_duplicate_repaired`, `ipc_dedupe_redelivered_editor`, and `sidecar_normalization_fallback_redelivered_editor`, plus source inspection around the redelivery callers. No code changes were made.
❯ Why are `redeliver_ipc_dedupe_to_editor` and `redeliver_normalization_fallback_to_editor` being too eagerly triggered?
### Re: over-eager repair redelivery — gpt-5

They are too eager because the current write path treats any IPC snapshot repair as proof that the live editor must be repaired with a full-document replacement.

For `redeliver_normalization_fallback_to_editor`, the trigger is strict sidecar verification. `normalize_prefix_lines` is derived from every line the binary prefixed in `content_ours`. After the plugin applies the component patch, `verify_sidecar_normalization(...)` requires the ack-content sidecar to contain the required count of `❯ <target>` occurrences. If one occurrence is missing because of plugin ordering, boundary position, assistant/user-region classification, repeated prompt text, or editor whitespace behavior, the binary falls back to `content_ours`, repairs disk, and unconditionally calls `redeliver_normalization_fallback_to_editor(...)`. The plugin spec even calls out one known failure shape: if normalization happens before boundary cleanup, the prompt can be skipped and the binary will fall back.

For `redeliver_ipc_dedupe_to_editor`, the trigger is broader: `dedupe_ipc_snapshot_content(...)` runs duplicate response-block cleanup, post-exchange duplicate prompt comment cleanup, prompt-count reconciliation against the pre-write exchange, and prefix-variant tail cleanup. If any of those changes the ack sidecar content, the caller treats it as `dedupe_repair=true`, repairs disk, and unconditionally calls `redeliver_ipc_dedupe_to_editor(...)`.

That is the over-eager design problem: the code collapses several different states into one boolean, `repair_disk` / `dedupe_repair`, and then uses that boolean to perform a visible full-document redelivery. It does not first prove that the editor buffer still contains the bad duplicate/prefix state, that the bad state is not just an ack-sidecar/snapshot artifact, or that a narrower component/prefix repair would be insufficient.

The logs match that. On `2026-05-09 10:23:13 EDT`, the sequence was `normalize_user_prompts applied=1` -> `sidecar_normalization_fallback` -> `sidecar_normalization_fallback_redelivered_editor`. On `2026-05-15 10:07:24 EDT`, the sequence was `ipc_prompt_duplicate_repaired` -> `ipc_snapshot_deduped` -> `ipc_dedupe_redelivered_editor`, while the later diagnostic for that write still reported `live_exchange_edited=false`. That points to an IPC/sidecar reconciliation path deciding to redeliver the whole document, not to a proven active live-typing conflict at redelivery time.

So the likely root cause is not that `redeliver_*` itself has complex logic; it has almost none. The eagerness is upstream: snapshot/sidecar repair predicates are too coarse, and any repair delta is promoted to a full-document editor repair.

What should change: separate the decisions. Save a repaired snapshot when needed, but only full-content redeliver when a fresh editor-buffer proof shows the visible buffer still contains the exact bad state. Otherwise skip editor redelivery or send a narrow normalization/reposition patch.

Verification: `agent-doc preflight`, `agent-doc plan`, source inspection of `verify_sidecar_normalization`, `dedupe_ipc_snapshot_content`, and both redelivery call sites, plus ops-log inspection around the May 9 and May 15 repair sequences. No code changes were made.
❯ What are the #next-steps to "separate the decisions"?
### Re: next steps for separating repair decisions — gpt-5

The top backlog item is `#dvre`.

The practical split is: snapshot/disk repair can happen when `verify_sidecar_normalization(...)` or `dedupe_ipc_snapshot_content(...)` proves the ACK/snapshot needs cleanup, but editor redelivery should require a second, fresh proof that the visible editor buffer still contains the exact bad state.

Concrete next steps:

1. Keep `#dvre` first: gate `redeliver_ipc_dedupe_to_editor(...)` and `redeliver_normalization_fallback_to_editor(...)` so a repaired snapshot does not automatically become a full-document editor replacement.
2. Replace the current boolean repair result with a typed repair decision carrying the repaired content, repair reason, bad-state fingerprint, and explicit `redeliver_editor` decision.
3. Add fresh editor-buffer proof before full-content redelivery. The proof should match the exact duplicate prompt or missing-prefix state, not just an ACK sidecar delta.
4. Add logging at the decision boundary: patch id, snapshot source, repair reason, before/current hashes, proof source, duplicate prompt count, required/observed prefix counts, and whether redelivery was skipped or sent.
5. Add unit and SimWorld coverage for both branches: snapshot-only repair when the bad state is only in the sidecar/snapshot path, and full-content redelivery only when the visible editor still contains the bad state.
6. After that, make narrow normalization/reposition patching the preferred prefix repair and keep full-content `ResponseFallback` as the last resort.

I added those follow-up items to the backlog in priority order. No code changes were made in this cycle.

Verification: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, and source inspection of the repair/redelivery call sites in `src/agent-doc/src/write.rs`.
❯ Are there #next-steps to clean up the many failed attempts to fix the duplicate corrupt document & the duplicate prompt issues after we "separate the decisions"? Please prioritize after the "separate the decisions" fixes.
### Re: cleanup after repair decision split — gpt-5

Yes. The top backlog item remains `#dvre`; the cleanup work should come after the decision-separation fixes because otherwise cleanup risks deleting the only evidence and guardrails around the bug.

I would prioritize the cleanup sequence this way:

1. First finish `#dvre`, `#rdec`, `#eprf`, `#rlog`, `#rtst`, and `#nfix` so the system has a clear repair/redelivery contract, proof, logs, and tests.
2. Then audit the prior fixes and classify them as real invariants, fallbacks, logging, or obsolete workarounds.
3. Consolidate the duplicate-prompt cleanup paths so there is one canonical place deciding what counts as duplicate prompt residue and what repair is legal.
4. Update the editor specs to remove stale requirements that every normalization fallback must be pushed back via full-document `fullContent`.
5. Rewrite tests that currently encode the old eager-redelivery behavior, keeping regression coverage but changing the expected behavior to typed decisions and proof-gated redelivery.
6. Remove redundant workaround branches only after the new tests and logs prove the narrower behavior is stable.
7. Finally, reduce temporary transition logging while keeping high-signal corruption diagnostics.

I added those cleanup items after the existing “separate the decisions” backlog items, preserving priority order. No code changes were made in this cycle.

Verification: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, and source/spec scan for the duplicate-prompt, normalization fallback, full-content, and reposition paths.
<!-- agent:boundary:5aa8b980 -->
<!-- /agent:exchange -->
###
<!--
---
html comment fix
---
Please fix. This has taken >50 turns and you still fail to fix this issue. FIX THE DANM ISSUE. Please add logging so you can detect when this issue occurs. I'm talking about the duplicate corrupt document & the duplicate prompt issue. It happened as I was typing this prompt. WTF are you doing? Not fixing the issue apparently. Just wasting tokens and time. #spec-test-build-install-commit-push
---
Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.
---
#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#dvre] [recommended] Gate response-fallback full-document redelivery: after IPC dedupe or sidecar-normalization repair, require fresh editor-buffer proof that the visible buffer still contains the exact bad state before calling redeliver_ipc_dedupe_to_editor or redeliver_normalization_fallback_to_editor; otherwise save the repaired snapshot or send a narrow normalization/reposition patch.
- [ ] [#rdec] [recommended] Split the IPC repair outcome in src/agent-doc/src/write.rs into typed decisions: repaired snapshot content, disk-repair reason, editor bad-state fingerprint, and explicit redeliver_editor=true|false.
- [ ] [#eprf] [recommended] Add fresh editor-buffer proof before response-fallback full-document redelivery, so redeliver_ipc_dedupe_to_editor and redeliver_normalization_fallback_to_editor only run when the visible buffer still contains the exact duplicate/prefix-divergent state.
- [ ] [#rlog] [recommended] Add structured ops-log records for repair/redelivery decisions: patch_id, snap_source, repair_reason, before/current hashes, proof source, duplicate prompt count, required/observed prefix counts, and redeliver=true|false.
- [ ] [#rtst] [recommended] Add deterministic unit and SimWorld coverage for ack-sidecar-only repair, real visible duplicate repair, sidecar normalization divergence, stale source-buffer skip, and post-commit user follow-up drift.
- [ ] [#nfix] [recommended] Replace eligible prefix-normalization response fallback redeliveries with a narrow normalization/reposition patch before using fullContent as a last resort.
- [ ] [#caud] [recommended] After the repair/redelivery split lands, audit the duplicate-prompt and prefix-normalization fixes added during the failed attempts and classify each as invariant, fallback, logging, or obsolete workaround.
- [ ] [#cuni] [recommended] Consolidate duplicate-prompt cleanup into one canonical repair/invariant path instead of scattered cleanup helpers for post-exchange comments, prompt lines against before-content, prefix variants, and duplicate response blocks.
- [ ] [#cspe] [recommended] Update src/agent-doc/editors/PLUGIN-SPEC.md and src/agent-doc/editors/SPEC.md to describe the new snapshot-only repair versus editor-redelivery contract, removing stale language that says every normalization fallback is sent back through fullContent.
- [ ] [#ctes] [recommended] Rewrite tests that encode old over-eager fullContent redelivery behavior so they assert the new typed decisions, skip-redelivery branch, narrow-repair branch, and last-resort fullContent branch.
- [ ] [#crem] [recommended] Remove redundant workaround branches and compatibility shims made unnecessary by the typed repair decision, keeping only guards that still protect committed snapshots, active typing, or externally visible editor state.
- [ ] [#cobs] [recommended] Add temporary observability cleanup criteria: once the new repair/redelivery logs prove stable across local runs and tmux/CI, reduce noisy transition logs while preserving high-signal corruption diagnostics.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
