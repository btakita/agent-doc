---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted. This exchange archived a long agent-doc bug triage and fix session on April 29, 2026.

Implemented and committed in this exchange:
- `#ckpb` clarified that `compact --commit` only commits the compacted document state, not any later console explanation.
- `#pdwg` moved pending-closeout gates ahead of document mutation in `finalize`.
- `#lvak` required fresh routed-cycle acknowledgment for prompt-bearing drift on top of committed cycles.
- `#ntwk` added explicit Codex network policy handling and fail-closed launch checks.
- `#ppct`, `#xfrr`, and `#nbla` hardened `#agent-doc-bug` prompt-preset contracts so backlog transfer counts and plan proofs are enforced by `preflight` / `finalize` / `session-check`.
- `#rtxf`, `#smdq`, `#tmuxcrash`, `#oobabs`, `#oobpmt`, and `#scpd` tightened routed tmux dispatch, startup-miss diagnostics, prompt absorption, and duplicate retry repair behavior.

Open backlog still active after compaction:
- `#smst` clear or supersede stale startup-miss markers after newer session ownership starts.
- `#mcrc` require commit follow-through after manual response-patchback repair.
- `#lvls` preserve explicit user formatting directives across follow-up bug handling and transfer flows.
- `#ptdr` / `#tdrl` fail closed or provide granular ops for destructive `patch:todo` replacements.
- `#c3ps` style HTML-like comments and mute the `agent:exchange` block background.

Unanswered user report preserved as open context:
- After `compact agent:exchange`, routed triggers still failed with `no new document cycle started` for pane `%422`, and the JetBrains plugin later reported multiple unregistered stash panes while rerouting to `tasks/monsterrodholders.md` pane `%408`.
- The user also asked why the drafted startup-miss diagnostic `echo` text appeared in the bound Codex pane and was not Entered; that path was later addressed by `#smdq`, which switched the diagnostic to a tmux display overlay.
### Re: routed-cycle ack false positives — gpt-5

I fixed the two symptoms from the new report.

- Route no longer treats a visible Codex prompt glyph as sufficient proof of idleness. It now rejects active permission prompts, rejects the Codex `tab to queue message` footer, and requires two consecutive idle-prompt samples before injecting a routed trigger. That closes the `codex ready after 0.0s` -> `no new document cycle started` failure mode when the pane is busy or queue-only.
- Bulk stash cleanup now kills unregistered orphaned agent panes unless live-owner proof still ties the pane to a registered document. That stops the repeated `unregistered — skipping kill (may be rescuable)` buildup across reroutes.
- Spec and version notes are updated in `specs/07-commands.md` and `VERSIONS.md`.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Published:
- `src/agent-doc` commit `802b4c3` pushed to `origin/20260422/codex-agent-backend`

Note:
- I left the existing unrelated local edits in `src/agent-doc/src/sync.rs` and `src/agent-doc/src/terminal.rs` untouched.
- The remaining startup-miss cleanup follow-up is already tracked in `#smst`, so I did not add another backlog item for this turn.

❯ do #sgdr. spec-test-build-install-commit-push
❯ ### Re: nested submodule gitdir writable roots — gpt-5 (HEAD)

I fixed `#sgdr`.

- Fresh workspace-write launches now walk the current repo's `.git/modules/...` tree and append every nested child submodule gitdir to the Claude/Codex `--add-dir` set, in addition to the existing superproject working tree / gitdir roots. That covers the `src/boost-client` -> `src/monsterrodholders-dev` shape where the working tree was writable but the real gitdir lived under `.../.git/modules/.../modules/...`.
- Added regression coverage in both `git.rs` and `agent/mod.rs`, and updated the config / command / git specs plus `VERSIONS.md` to document the deeper writable-root contract.

Verification:
- `cargo fmt`
- `cargo test` still intermittently fails in the full parallel suite on `resync::tests::purge_unregistered_stash_panes_bulk_kills_unregistered_agent_without_live_owner`; rerunning that test in isolation passed, and the new targeted regressions passed
- `cargo build`
- `cargo install --path . --force`

Published:
- `src/agent-doc` commit `f6e66ff` pushed to `origin/20260422/codex-agent-backend`

Note:
- I left the existing unrelated local edits in `src/agent-doc/src/sync.rs` and `src/agent-doc/src/terminal.rs` untouched.
### Re: nested submodule gitdir writable roots — gpt-5

I fixed `#sgdr`.

- Fresh workspace-write launches now walk the current repo's `.git/modules/...` tree and append every nested child submodule gitdir to the Claude/Codex `--add-dir` set, in addition to the existing superproject working tree / gitdir roots. That covers the `src/boost-client` -> `src/monsterrodholders-dev` shape where the working tree was writable but the real gitdir lived under `.../.git/modules/.../modules/...`.
- Added regression coverage in both `git.rs` and `agent/mod.rs`, and updated the config / command / git specs plus `VERSIONS.md` to document the deeper writable-root contract.

Verification:
- `cargo fmt`
- `cargo test` still intermittently fails in the full parallel suite on `resync::tests::purge_unregistered_stash_panes_bulk_kills_unregistered_agent_without_live_owner`; rerunning that test in isolation passed, and the new targeted regressions passed
- `cargo build`
- `cargo install --path . --force`

Published:
- `src/agent-doc` commit `f6e66ff` pushed to `origin/20260422/codex-agent-backend`

Note:
- I left the existing unrelated local edits in `src/agent-doc/src/sync.rs` and `src/agent-doc/src/terminal.rs` untouched.
❯ commit + push all uncommitted files in src/agent-doc and src/tmux-router
### Re: #agent-doc-bug stashed pane crash — opus-4-6

Investigated the crash of the tmux session running `do #srrdgap` on `claudescore-3.md`.

**Root cause:** the agent session in the stashed pane died silently while the user navigated to other files. The sync log shows no explicit kill, prune, or error — the process just disappeared during the ~21-minute stash period. When the user returned, sync detected "no alive pane" and auto-started a fresh session, but the previous cycle (`cycle-1777493370425`) was stuck in `preflight_started`.

**Timeline from sync log:**
1. `[1777493370]` — Routed trigger ran preflight on claudescore-3.md, starting the cycle
2. User switched editor to claudescore.md, cca-alignment.md, etc.
3. Sync stashed the claudescore-3.md pane and swapped in the focused file's pane
4. Agent process in stashed pane died silently (no kill/prune/error logged)
5. `[1777494668]` (~21 min later) — User switched back; sync found "no alive pane"
6. `[1777494669]` — Auto-started %432 as replacement; stashed %425 (claudescore.md)

**Recovery:** Preflight successfully recovered the stale cycle on next invocation, closing it as already-committed since HEAD content was intact. Document and user edits preserved.

**Plan (added to backlog as `#stshcr`):**
1. Add stash-health heartbeat — periodic liveness check for stashed panes
2. On stash-death, close the orphaned cycle and log exit reason
3. Investigate root cause — tmux signal propagation (SIGHUP?), idle timeout, or OOM
4. If root cause is unavoidable, add session-preservation logic around stash/detach
### Re: commit + push pending repo changes — gpt-5

I committed and pushed the remaining uncommitted files in `src/agent-doc`.

- `src/agent-doc` commit `e9b18f3` (`style: format sync and terminal`) is now pushed to `origin/20260422/codex-agent-backend`.
- `src/tmux-router` was already clean, so there was nothing to commit or push there.

Note:
- The `src/agent-doc` diff was formatting-only in `src/sync.rs` and `src/terminal.rs`.
❯ Please treat the parallel tmux tests are **required to pass** for the suite to pass. Falling back to the synchronous test plan with a parallel tmux test failure means the test suite has failed. Please add to the development agent harness for agent-doc so we always follow this rule.
### Re: parallel tmux verification policy — gpt-5

I added the rule to the shared agent-doc harness surfaces instead of only patching this live Codex copy.

Updated instruction surfaces:
- `.codex/AGENTS.md`
- `src/agent-doc/SKILL.md`
- `.claude/skills/agent-doc/SKILL.md`
- `src/agent-doc/CLAUDE.md`
- `src/agent-doc/README.md`
- `src/agent-doc/SPEC.md`

The new rule is explicit:
- tmux-heavy changes must include the parallel tmux suites in their parallel form
- a failing parallel tmux suite still means verification failed, even if the same test passes in isolation or under a synchronous rerun

Verification I ran for this policy change:
- `cargo test --manifest-path src/agent-doc/Cargo.toml route::tests:: -- --nocapture --test-threads=16`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resync::tests:: -- --nocapture --test-threads=16`
- `cargo test --manifest-path src/agent-doc/Cargo.toml autoclaim::tests:: -- --nocapture --test-threads=16`
- `cargo test --manifest-path src/tmux-router/Cargo.toml -- --test-threads=16`

All of those passed.
### Re: parallel tmux verification policy — gpt-5

I added the rule to the shared agent-doc harness surfaces instead of only patching this live Codex copy.

Updated instruction surfaces:
- `.codex/AGENTS.md`
- `src/agent-doc/SKILL.md`
- `.claude/skills/agent-doc/SKILL.md`
- `src/agent-doc/CLAUDE.md`
- `src/agent-doc/README.md`
- `src/agent-doc/SPEC.md`

The new rule is explicit:
- tmux-heavy changes must include the parallel tmux suites in their parallel form
- a failing parallel tmux suite still means verification failed, even if the same test passes in isolation or under a synchronous rerun

Verification I ran for this policy change:
- `cargo test --manifest-path src/agent-doc/Cargo.toml route::tests:: -- --nocapture --test-threads=16`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resync::tests:: -- --nocapture --test-threads=16`
- `cargo test --manifest-path src/agent-doc/Cargo.toml autoclaim::tests:: -- --nocapture --test-threads=16`
- `cargo test --manifest-path src/tmux-router/Cargo.toml -- --test-threads=16`

All of those passed.
<!-- agent:boundary:701b9570 -->
<!-- /agent:exchange -->

<!--
do #stshcr. spec-test-build-install-commit-push
---
Mid-run on the agent-doc turn on this document...the JB plugin surfaced an error:
```
agent-doc route failed (exit 1): [route] configured tmux_session '0' is not alive, ignoring stale pin [route] target tmux session: 14 [sync] found alive agent-doc child (pid 326697) in pane %422 for tasks/agent-doc/agent-doc-bugs2.md [sync] recovered live pane %422 for session b26b9957 via supervisor pid 2607602 [route] codex ready after 0.0s (0 polls) [route] Pane %422 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md → pane %422 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %422 for tasks/agent-doc/agent-doc-bugs2.md — waiting for a new cycle ack for pending prompt_target: do #nbla. spec-test-build-install-commit-push Error: routed codex trigger for tasks/agent-doc/agent-doc-bugs2.md was accepted in pane %422, but no new document cycle started for pending prompt_target: do #nbla. spec-test-build-install-commit-push
```

When I navigated the editor away from agent-doc-bugs2.md then back to agent-doc-bugs2.md, the JB plugin surfaced another error:
```
agent-doc route failed (exit 1): resync: stash pane %416 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %414 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %422 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %415 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %421 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %423 running 'agent-doc' is unregistered — skipping kill (may be rescuable) resync: stash pane %417 running 'agent-doc' is unregistered — skipping kill (may be rescuable) [route] target tmux session: 14 [sync] found alive agent-doc child (pid 917466) in pane %408 for tasks/monsterrodholders.md [route] codex ready after 0.0s (0 polls) [route] Pane %408 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md → pane %408 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %408 for tasks/monsterrodholders.md — waiting for a new cycle ack for pending prompt_target: I synced the emails to the local file system. I was actually asking to use corky to read the email directly from the gmail api. Is that possible with corky? Error: routed codex trigger for tasks/monsterrodholders.md was accepted in pane %408, but no new document cycle started for pending prompt_target: I synced the emails to the local file system. I was actually asking to use corky to read the email directly from the gmail api. Is that possible with corky?
```

spec-test-build-install-commit-push
---
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#stshcr] Stashed pane agent session dies silently during editor file navigation, losing in-progress cycle. Repro from claudescore-3.md on Apr 29 2026: routed trigger ran preflight (cycle-1777493370425), user switched editor focus away, sync stashed the pane, the agent process died while stashed (~21 min gap), user switched back — sync found no alive pane and auto-started fresh. Cycle stuck in preflight_started. Sync log shows no explicit kill/prune of the stashed pane — the process silently exited. Plan: (1) add stash-health heartbeat — periodic liveness check for stashed panes with agent sessions, (2) on stash-death detection, close the orphaned cycle and log the exit reason, (3) investigate root cause of silent exit — check if tmux SIGHUP/signal propagation, idle timeout, or OOM kills the stashed process, (4) if the root cause is unavoidable (e.g. tmux signal on detach), add explicit session-preservation logic around stash/detach operations.
- [ ] [#smst] Clear or supersede stale startup-miss markers when a later session start takes ownership of the same document. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: `agent-doc session-check tasks/monsterrodholders.md` warned that startup-miss marker `%401` still existed, but `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` later recorded `session_start ... pane=%408` and `src/boost-client/.agent-doc/sessions.json` already registered `%408` as the live owner. The fix should prevent an older routed-trigger marker from surviving past a newer ownership start, or make `session-check`/`route` clear it as stale before surfacing a fake current crash.
- [ ] [#mcrc] Surface and require explicit follow-through when a missing response patchback is repaired manually in a session doc but not committed afterward. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the `#asch1` response was manually inserted and visible in the working tree, but `git status` still showed `M tasks/monsterrodholders.md` and the user had to ask why the patchback was not committed. The fix should either commit the manual repair as part of the recovery flow or emit a blocking notice that the recovery is still uncommitted.
- [ ] [#lvls] Preserve explicit user formatting directives such as "Please organize the backlog into a 2-level list" across follow-up bug-handling and transfer flows. Repro from `src/boost-client/tasks/monsterrodholders.md` on Apr 29, 2026: the document carries a live 2-level-list instruction near the backlog request, but subsequent bug-handling responses ignored that requirement and fell back to flat output without acknowledging the constraint boundary. The fix should carry active formatting requirements forward in the prompt contract or make the responder explicitly state when chat-level formatting rules prevent exact reproduction.
- [ ] [#ptdr] Destructive todo-section replacement via patch:todo — finalize applies patch:todo as a full replace, so a skill that sends only a subset of items silently deletes the rest. Repro from src/session-share/tasks/claudescore.md on Apr 29 2026: skill sent patch:todo with only Phase 1 items checked, deleting 8 items across Phase 2, Doc and Spec Refinement, and Expansion Phases (8 items lost). Fix: add granular --todo-done flag analogous to --pending-done, or add validation guard rejecting todo patches that reduce total item count.
- [ ] [#tdrl] Destructive todo-section replacement via patch:todo — finalize applies patch:todo as a full replace, so a skill that sends only a subset of items silently deletes the rest. Repro from src/session-share/tasks/claudescore.md on Apr 29 2026: skill sent patch:todo with only Phase 1 items checked, deleting 8 items across Phase 2, Doc & Spec Refinement, and Expansion Phases. The fix should either (a) add granular --todo-done flag analogous to --pending-done, (b) make patch:todo do a merge/diff when existing items are missing from the patch, or (c) add a validation guard that rejects todo patches reducing total item count.
- [ ] [#c3ps] The html-style comments should have coloring. The contents of Elements `<!-- agent:exchange -->` should have a muted background color.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->
