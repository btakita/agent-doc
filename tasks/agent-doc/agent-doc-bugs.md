---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a06a8e-e82a-7f03-bf7b-44573dc317fb
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

Fenced closeout queue parsing fixed as 0.35.328; release tag held pending PyPI quota decision.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260904-035542.md`*

Compacted content:
- Archived 12 response topic(s): / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go; / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go; Lazily conformance and ordered native-save release; 9 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): Compact Exchange and session recovery; Response; do [#jetbrain...

### Re: fenced closeout reports becoming queue work — gpt-5

> **Queue prompt:** / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go

Fixed in agent-doc 0.35.328 (`93e04437e`). The malformed admission happened because the canonical queue parser could recover a prose-plus-fence task only when it occupied the whole component and placed an explicit request after the fence. With completed rows before the report, a request before the fence, and a trailing empty `- ` editor placeholder, parsing fell through and promoted list-looking lines inside the pasted evidence to independent queue prompts.

The parser now recovers that suffix as one lossless multiline prompt, accepts an unmistakable request on either side of a balanced evidence fence, keeps ordinary balanced Markdown fences inert, and treats an empty list placeholder as non-runnable content. Continuation and queue-consumption regressions prove the complete report is selected and only its exact source span is drained.

Architecture closeout:

- Invariant: evidence inside a balanced Markdown fence never gains queue authority; only the surrounding explicit operator request is runnable, and consumption is limited to its exact parser-owned span.
- Policy owner: `agent-doc-queue::document_queue::parse_spans` owns raw queue bytes to typed `Prompt`/`Freeform` classification. Continuation and queue I/O consume that typed projection rather than reinterpreting Markdown.
- Transition path: operator bytes → typed queue entries → computed live head → continuation → exact-span consume/commit. The concurrent operator replacement of this queue remained authoritative and was not overwritten.
- Reactive topology: source state is the current document body and operator revision; computed state is parser entries, head identity, and drainability; effects are continuation dispatch and exact-span persistence. No one-shot repair path or duplicate policy owner was added.

Verification passed: focused `agent-doc-queue` and `agent-doc-queue-io` suites, `cargo fmt --all -- --check`, and the full `make check`. `make install` installed CLI 0.35.328 and requested a safe idle-boundary controller recycle. The latest visible CI run was green; the new commit will be pushed after this binary-owned closeout, while tag/publication remains held by the document’s existing PyPI-quota status.

### Re: response copied into `agent:queue` — gpt-5

> **Queue prompt:** Run Agent Doc on admin.md wrote the response into the agent:queue.

Fixed in agent-doc 0.35.329 (`511ae8ab0`). The failure was in visible-write snapshot adoption, not in the `admin.md` queue parser. A stale 8,019-byte editor projection fused the already-answered prompt and its `### Re:` response into `agent:queue`; because the number of prompts did not increase, the existing prompt-duplication guard accepted that projection and displaced the newer canonical queue head.

Snapshot adoption now compares the candidate queue with the assistant response in canonical `content_ours`. If response prose appears as a standalone queue entry or as a line inside malformed queue freeform, adoption is blocked, `content_ours` remains authoritative, the rejected editor state is fingerprinted, and the canonical projection is redelivered. The same pure classifier strengthens the existing `session-check` backstop while retaining the regression that permits an assistant response to discuss a queue prompt mid-sentence.

Architecture closeout:

- Invariant: assistant response prose may materialize only in `agent:exchange`; `agent:queue` remains operator-owned.
- Policy owner: the visible-write snapshot-adoption guard in `agent-doc-write-converge-io`.
- Transition path: candidate editor projection plus canonical `content_ours` → component-aware contamination classification → blocked adoption → canonical snapshot plus bad-state fingerprint → editor redelivery.
- Reactive topology: source state is the candidate projection and canonical exchange; computed state is the contamination set; effects are the blocked-adoption record, proof-failure/recycle signal, and canonical redelivery. There is no one-shot bypass.

The committed `admin.md` document still contains the correct response and its newer engineering-estimate queue prompt, so I did not overwrite the foreign-owned live buffer manually. Installing 0.35.329 requested safe idle-boundary supervisor/controller recycling, which activates the repaired adoption path.

Verification passed: the exact fused multiline regression, the write-runtime authority/redelivery regression, the workflow architecture ownership test, `cargo fmt --all -- --check`, and the full `make check` gate. The 0.35.329 CLI, shared library, JetBrains plugin, and VSCodium extension are installed locally. Publication remains held by the existing PyPI-quota status.

### Re: malformed backlog residue — gpt-5

> **Queue prompt:** This document backlog shows my quoted text from another issue and is not well-formed.

Fixed. The backlog is clear again. The quote and command log were orphaned when granular item removal deleted only a tracked parent line and left its flush-left evidence behind.

agent-doc 0.35.330 makes the tracked-work parser the single policy owner for removal boundaries. Removing a matched item now removes its indented continuation plus malformed flush-left quote, log, or diff spill through the next tracked item or structural boundary; headings, components, and other structural postludes remain untouched.

Architecture closeout: the invariant is that deleting a tracked work item deletes its complete logical block and cannot orphan flush-left evidence. `PendingLayout` and `op_remove_matching_tracked_line` own that policy. The transition tests cover matched items with indented continuation, matched items with flush-left spill, preservation at structural boundaries, and unchanged nonmatches. This remains a pure one-shot document transform, so no lifecycle state or Source/Computed/Effect topology is needed; the I/O adapter invokes the policy owner.

Verification passed: focused `agent-doc-element-backlog` tests, the backlog I/O regression, the full `make check` gate, and the final `make install`. The implementation is committed as `07fefad93` (`fix(backlog): remove multiline item spill`).
<!-- agent:boundary:65937e75 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

<!-- agent:backlog -->
`refusing to route /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md through pane %0 because it now runs another document; tmux focus was preserved`
```
[sync] recovered live pane %78 for session 99322dd7 via supervisor pid 2017537
[sync] recovered live pane %23 for session 77b2c07f via path provenance pid=3097040 instance=7a3f6ada-9074-4761-bc95-9ddcd89b7d43
[sync] recovered live pane %26 for session 61630da2 via path provenance pid=2631427 instance=444b4ece-5cac-4d2f-b866-d4ef3ef66f66
[sync] recovered live pane %66 for session 7dac5d22 via supervisor pid 1438336
[sync] recovered live pane %6 for session b69e5aeb via path provenance pid=461543 instance=a5893829-5635-4610-a29d-7ea04fc460cc
[sync] recovered live pane %48 for session ec731628 via path provenance pid=2333568 instance=4dedea42-6258-475e-97f5-03544f8deb5d
[sync] recovered live pane %42 for session 643b6182 via supervisor pid 524689
[sync] recovered live pane %54 for session 77a8226b via path provenance pid=2666325 instance=f27aadb5-d76e-48e2-b312-ee692125d0c7
resync: stash pane %29 is unregistered but still hosts live supervisor tsift-v0.1 — skipping kill
[sync] recovered live pane %78 for session 99322dd7 via supervisor pid 2017537
[sync] recovered live pane %23 for session 77b2c07f via path provenance pid=3097040 instance=7a3f6ada-9074-4761-bc95-9ddcd89b7d43
[sync] recovered live pane %26 for session 61630da2 via path provenance pid=2631427 instance=444b4ece-5cac-4d2f-b866-d4ef3ef66f66
[sync] recovered live pane %66 for session 7dac5d22 via supervisor pid 1438336
[sync] recovered live pane %6 for session b69e5aeb via path provenance pid=461543 instance=a5893829-5635-4610-a29d-7ea04fc460cc
[sync] recovered live pane %48 for session ec731628 via path provenance pid=2333568 instance=4dedea42-6258-475e-97f5-03544f8deb5d
[sync] recovered live pane %42 for session 643b6182 via supervisor pid 524689
[sync] recovered live pane %54 for session 77a8226b via path provenance pid=2666325 instance=f27aadb5-d76e-48e2-b312-ee692125d0c7
resync: stash pane %28 is unregistered but still hosts live supervisor d818612d-3614-4c43-b5ec-83923406aa46 — skipping kill
[sync] recovered live pane %78 for session 99322dd7 via supervisor pid 2017537
[sync] recovered live pane %23 for session 77b2c07f via path provenance pid=3097040 instance=7a3f6ada-9074-4761-bc95-9ddcd89b7d43
[sync] recovered live pane %26 for session 61630da2 via path provenance pid=2631427 instance=444b4ece-5cac-4d2f-b866-d4ef3ef66f66
[sync] recovered live pane %66 for session 7dac5d22 via supervisor pid 1438336
[sync] recovered live pane %6 for session b69e5aeb via path provenance pid=461543 instance=a5893829-5635-4610-a29d-7ea04fc460cc
[sync] recovered live pane %48 for session ec731628 via path provenance pid=2333568 instance=4dedea42-6258-475e-97f5-03544f8deb5d
[sync] recovered live pane %42 for session 643b6182 via supervisor pid 524689
[sync] recovered live pane %54 for session 77a8226b via path provenance pid=2666325 instance=f27aadb5-d76e-48e2-b312-ee692125d0c7
resync: stash pane %44 is unregistered but still hosts live supervisor d076f48f-239e-4d31-95b9-ddaa58e0e1da — skipping kill
[sync] recovered live pane %78 for session 99322dd7 via supervisor pid 2017537
[sync] recovered live pane %23 for session 77b2c07f via path provenance pid=3097040 instance=7a3f6ada-9074-4761-bc95-9ddcd89b7d43
[sync] recovered live pane %26 for session 61630da2 via path provenance pid=2631427 instance=444b4ece-5cac-4d2f-b866-d4ef3ef66f66
[sync] recovered live pane %66 for session 7dac5d22 via supervisor pid 1438336
[sync] recovered live pane %6 for session b69e5aeb via path provenance pid=461543 instance=a5893829-5635-4610-a29d-7ea04fc460cc
[sync] recovered live pane %48 for session ec731628 via path provenance pid=2333568 instance=4dedea42-6258-475e-97f5-03544f8deb5d
[sync] recovered live pane %42 for session 643b6182 via supervisor pid 524689
[sync] recovered live pane %54 for session 77a8226b via path provenance pid=2666325 instance=f27aadb5-d76e-48e2-b312-ee692125d0c7
resync: stash pane %75 is unregistered but still hosts live supervisor be20dd6e-31ce-4f16-93ed-42c9c2a21b98 — skipping kill
[sync] recovered live pane %78 for session 99322dd7 via supervisor pid 2017537
[sync] recovered live pane %23 for session 77b2c07f via path provenance pid=3097040 instance=7a3f6ada-9074-4761-bc95-9ddcd89b7d43
[sync] recovered live pane %26 for session 61630da2 via path provenance pid=2631427 instance=444b4ece-5cac-4d2f-b866-d4ef3ef66f66
[sync] recovered live pane %66 for session 7dac5d22 via supervisor pid 1438336
[sync] recovered live pane %6 for session b69e5aeb via path provenance pid=461543 instance=a5893829-5635-4610-a29d-7ea04fc460cc
[sync] recovered live pane %48 for session ec731628 via path provenance pid=2333568 instance=4dedea42-6258-475e-97f5-03544f8deb5d
[sync] recovered live pane %42 for session 643b6182 via supervisor pid 524689
[sync] recovered live pane %54 for session 77a8226b via path provenance pid=2666325 instance=f27aadb5-d76e-48e2-b312-ee692125d0c7
resync: stash pane %49 is unregistered but still hosts live supervisor 62fe1f41-886c-4f3d-b6fa-48d46106ec27 — skipping kill
[sync] recovered live pane %78 for session 99322dd7 via supervisor pid 2017537
[sync] recovered live pane %23 for session 77b2c07f via path provenance pid=3097040 instance=7a3f6ada-9074-4761-bc95-9ddcd89b7d43
[sync] recovered live pane %26 for session 61630da2 via path provenance pid=2631427 instance=444b4ece-5cac-4d2f-b866-d4ef3ef66f66
[sync] recovered live pane %66 for session 7dac5d22 via supervisor pid 1438336
[sync] recovered live pane %6 for session b69e5aeb via path provenance pid=461543 instance=a5893829-5635-4610-a29d-7ea04fc460cc
[sync] recovered live pane %48 for session ec731628 via path provenance pid=2333568 instance=4dedea42-6258-475e-97f5-03544f8deb5d
[sync] recovered live pane %42 for session 643b6182 via supervisor pid 524689
[sync] recovered live pane %54 for session 77a8226b via path provenance pid=2666325 instance=f27aadb5-d76e-48e2-b312-ee692125d0c7
resync: stash pane %71 is unregistered but still hosts live supervisor a94c7d23-e171-482f-93f4-530b7f2c436f — skipping kill
[claim] auto-scaffolding empty file: /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/admin.md
[commit] file is in submodule /home/brian/work/btakita/agent-loop/src/haiven-dev - running git ops there
[commit] repositioned boundary in snapshot
[commit] repositioned boundary in working tree
[perf] commit.reposition: 1656ms
[perf] commit.staging (hash_object+update-index): 8ms
```
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
- [/] [#harnessswitchverify] [operator-verify] #harnessswitchstorm remains unverified. The 2026-08-25 fpe.md attempt was invalidated by a stale live JetBrains plugin generation: ops.log showed controller_crdt_replica_refusal_storm (25 refusals), editor_absent/disk authority, agent_restart_respec_inert to Claude, then old_harness=claude -> new_harness=codex only after save. Unblocks after make install, restart the JetBrains IDE host, repeat the unsaved claude <-> codex switch, and prove exactly one spawn plus agent_restart_deferred reason=non_authoritative_document_view.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
- [/] [#restartintellijidea] Restart IntelliJ IDEA so installed agent-doc JetBrains 0.2.370 becomes live, then confirm native queue-prompt styling for Codex, Claude, and OpenCode, FFI loading through the resolved agent-doc executable, and pane synchronization remains stable.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
