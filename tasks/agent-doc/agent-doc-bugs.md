---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a068ac-ce18-7510-95e9-161237197ac8
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

Issue #47 fixed on main as 0.35.303; release tag held pending PyPI quota decision.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260903-190913.md`*

Compacted content:
- Archived 8 response topic(s): Compact Exchange and session recovery; Response; do [#jetbrainsrunroute-z09d]; 5 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): Automatic stale-supervisor recycle; Response; do [#fixclaimtmu...

### Re: / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go — gpt-5

Fixed and recovered. Your queue edits did not cause the first native-save rejection: the editor accepted and projected the response at 15:02:40, the first save was rejected at 15:03:48, and your next observed typing began at 15:05:04. The later edits correctly became the newer canonical revision; they exposed the ordering defect, but they were not the original cause.

The wedge had two agent-doc causes. First, the controller's `AwaitingConvergence` retained-write state derived no persistence effect, so a rejected advisory save had no owned retry path. Second, JetBrains handled the native save outside the per-document CRDT worker, allowing it to race accepted local/remote update work. Making the controller wait synchronously would have introduced an update-RPC/save-RPC cycle, so the fix uses a non-authoritative controller egress worker and keeps document authority in the existing reactive graph.

The reusable policy is now in Lazily as `LatestDurableProjectionCore<K,V>`: monotone desired epochs, per-key latest-value conflation, one flight, retryable failure, exact durable acknowledgement, stale-receipt safety, and reconnect generation fencing. `lazily-spec` v0.38.0 (1c388a5) and `lazily-formal` v0.38.0 (74d0f76) were released first. Conformant `lazily-rs` v0.57.0 (c4a760b) was then released to crates.io with core, reactive, thread-safe, and async surfaces and canonical fixture replay.

agent-doc now queues native save behind all earlier accepted operations on JetBrains' per-document FIFO, enters the EDT only from that lane, validates the exact visible/replica/disk revision, and logs a typed reason for every rejection. The controller retains the latest desired revision through retry/supersession and advances only on the matching durable receipt. This is a command/effect driver in the Controller, not a second authority actor.

Released and installed agent-doc v0.35.324 at 1772efe18 with JetBrains 0.2.373. Verification passed: registry-backed Rust checks, 9,192 workspace tests, model checks, `make check`, `make tmux-ci`, and signed JetBrains build. Normal `agent-doc session-check` recovered `src/haiven-dev/tasks/backend.md` to `committed (commit_success)`; doctor confirms editor convergence, Lazily/CP equals disk, and no failure markers remain. No response resend, force-disk write, or destructive recovery was used, and the newer operator text was preserved. The remaining focused queue head was durably handed to the supervisor for clear-and-continue. Restarting IntelliJ activates the installed 0.2.373 Kotlin package; the controller-side retained retry is already live.
### Re: / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go — gpt-5

> **Queue prompt:** The document closeout is deferred, not lost. agent-doc respond reports editor_projection_pending — the editor

The backend closeout described by this head is now recovered and committed through the normal retained-delivery/session-check path, with the later queue typing preserved and no force-disk fallback. The detailed cause, chronology, implementation, release, and verification evidence are in the preceding response.

Release-audit correction: `lazily-rs` 0.57.0 follows the canonical spec fixture, but a newly detected mismatch in `lazily-formal` 0.38.0 retained a duplicate desired value after claim. Remaining binding releases are paused. The formal model will be corrected and released first, followed by a fresh conformant Rust patch release and an agent-doc dependency patch release, so the final provenance order is genuinely spec/formal before Rust before agent-doc. Session accretion is noted; the active queue is being drained rather than stalled for compaction.

### Re: Lazily conformance and ordered native-save release — gpt-5

Completed in the required order. The initial contract shipped as lazily-spec v0.38.0, the corrected Lean model shipped as lazily-formal v0.38.1 (`1a1aa153`), and only then did conformant lazily-rs v0.57.1 (`162319d7`) publish to crates.io. agent-doc now consumes lazily-rs 0.57.1.

The original native-save rejection was not caused by your updates: the first rejection occurred at 15:03:48, while the next observed user typing began at 15:05:04. The updates exposed the second ordering flaw, but did not cause the first failure. The agent-doc causes were (1) `AwaitingConvergence` derived no owned persistence retry and (2) JetBrains native save ran immediately on the EDT outside the per-document operation lane, so it could overtake accepted CRDT updates.

The fix uses one domain authority and one effect driver. The Controller retains the latest desired durable revision and emits commands; a non-authoritative egress worker performs blocking save I/O. JetBrains queues native save behind every previously accepted update on the same per-document FIFO, then enters the EDT and checks the exact visible/replica/disk revision. A save rejection is typed and retryable. This avoids a second command actor with competing state while preserving physical command order.

Lazily now supplies that reusable policy as `LatestDurableProjectionCore`: per-key monotone desired epochs, latest-value conflation, one in-flight claim, retryable failure, exact durable acknowledgement, stale-receipt safety, and reconnect generation fencing. Core/reactive/thread-safe/async surfaces exist where the binding supports those execution models; GDScript correctly exposes core plus synchronous reactive only.

The released bindings are Rust 0.57.1, Python 0.40.0, Kotlin 0.41.2, JavaScript 0.32.0, Dart 0.30.0, Zig 0.34.1, Go 0.26.0, C++ 0.28.0, C# 0.5.0, and GDScript 0.1.0; React 0.2.0 wraps the JavaScript implementation. Every native binding replays the canonical 22-step/two-scenario fixture across its supported surfaces. lazily-spec v0.38.1 (`d05f958e`) records the primitive as shipped in all ten binding columns, and its coverage guard validated 477 shipped marks against 10/10 ledgers. The audit caught and corrected Kotlin's missing `egress` ledger scope before the final spec release.

agent-doc itself was released and installed as v0.35.327 (`4ee1cc948`). Beyond the ordered save path and retained retry, v0.35.325 added exact-span recovered-queue consumption. Dogfooding then exposed two recovery-instruction bugs: a committed residue must first open a repair cycle with `preflight`, and the response-free strike must close with `agent-doc commit`, not response-materializing `write --commit`. Those fixes shipped in 0.35.326/0.35.327. Full verification passed: 9,193 Rust tests, 254 skipped, all formal models, documentation/version audits, eight live tmux tests, PyPI publication, and the six-platform binary release.

The wedged session is recovered through the normal path and reports `committed (commit_success)`. No response was resubmitted, no force-disk write was used, and the later user text was preserved. Safe supervisor recycle is queued for the next idle boundary. IntelliJ still reports live plugin 0.2.372; restart the IDE to activate the already installed 0.2.373 package.

One distribution-only blocker remains: React v0.2.0 has a green CI run and GitHub release, but its first npm publication cannot occur until npm credentials or a trusted publisher are configured. Rust's crates.io package is live; its failed OIDC automation attempt is likewise only release-workflow debt because the authenticated manual publish succeeded.
<!-- agent:boundary:65937e75 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
Please fix agent-doc causes of turn closeout not completing in backend.md. I was typing in the queue while the turn was closing:
```
  Verified by reproducing in a clean worktree at 628bf4ac with the submodule at its own pin 2cd516e, then running the
  preflight workflow's steps in order at both heads — task tidy + git diff --exit-code clean, go test ./... 25 packages
- / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go
  build, all exit 0 — then confirmed on GitHub's runners. Pushed with --force-with-lease; temp worktree removed.
  Two things need you:
  1. ~~The document closeout is deferred, not lost. agent-doc respond reports editor_projection_pending — the editor~~
     acknowledged the canonical content (df8a7991…) but hasn't projected its native save to disk. The retained intent
     1788462227612561795-1-df8a7991… commits itself once delivery converges; the binary explicitly forbids re-sending,
     forcing disk, or recycling, so I stopped after six session-check polls over ~2 minutes, all still INTERRUPTED. A
     save in the editor should converge it. Contributing factor worth knowing: the lint gate blocked the first two
     attempts on a pre-existing stray go attribute on <!-- agent:backlog priority go --> (queue-only, ignored, also
     flagged by preflight), and removing it required a disk edit under the live buffer — which is plausibly what the
     editor is now reconciling.
  2. ⏭️ #stalewtclean bit again. I couldn't rebase #413 to drop its now-redundant harness hunk because
     feat/studio-exposure-vocabulary is still checked out by ~/.claude/jobs/93c87c28/tmp/be-studio. Not urgent — git
     drops that hunk automatically (a trial rebase reported dropping … patch contents already upstream), and #413/#416
     stay green and MERGEABLE meanwhile.
```
<!-- /agent:queue -->

<!-- agent:backlog -->
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
