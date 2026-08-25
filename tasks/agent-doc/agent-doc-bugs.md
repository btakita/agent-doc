---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a03706-bebb-7880-a611-6cb25e59b290
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
Prior frontmatter bug fix remains complete. New work this cycle is planning only: added a concrete reliability plan for the “response patchback not committed / no patchback cycle ran” failure family. The current diagnosis is that two bugs can surface as the same symptom: (1) fresh Codex sessions can miss the initial route trigger entirely, so `preflight -> write -> commit` never starts; (2) a write/IPC + snapshot save can succeed, but the cycle can still die before `commit_*`, leaving a visible uncommitted assistant patchback in the working tree.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260824-202918.md`*

Compacted content:
- Archived 7 response topic(s): Removing AGENTS.md instructions; Extract AGENTS.md into skills & runbooks; Is #owned-pane-chat-writeback still relevant or out?; 4 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): Where else can lazily-rs be used; Where else can lazily-rs be ...

### Re: #pccv1 — gpt-5

Completed as a stale-open backlog item. Phase 1 already shipped in 0.35.127 and is present in the installed 0.35.278 code:

- Preflight re-resolves the post-maintenance document before persisting semantic operations, so queue fold-healing is visible to the durable operation log.
- The exact 16-line operator-paste regression asserts 16 individually addressable queue-item insert operations.

Verification passed:

- `cargo test -p agent-doc-turn sixteen_line_operator_paste_records_one_op_per_queue_item`
- `make check`
- Latest GitHub Actions CI run: success (`32675787507`)

No production source change was needed. `#qtypeclobber` is now unblocked and remains the next queued task.

### Re: Restart Agent on fpe.md — gpt-5

> **Queue prompt:**
>
> Restart Agent on fpe.md when switching from claude to codex restarted in claude. Note the file was not saved...only in the editor buffer. After saving, the switch seemed to work...But this is not necessarily proof of the issue.

Correct: this is useful evidence, but it is not proof of `#harnessswitchstorm`.

The `fpe.md` runtime log shows why:

- At `03:45:04Z`, the controller reported `controller_crdt_replica_refusal_storm` with 25 refusals and classified the live JetBrains plugin generation as stale.
- When Restart Agent ran, the live editor projection was unavailable (`lazily_editor_absent`), so resolution fell back to disk. The saved frontmatter still had no `agent:` value, which resolves to Claude; the log recorded `agent_restart_respec_inert ... running_harness=claude resolved_harness=claude`.
- After the document was saved, resolution saw `agent: codex` and at `03:45:37Z` recorded `agent_restart_performed ... old_harness=claude new_harness=codex`. The session is now healthy on Codex.

The required proof event, `agent_restart_deferred reason=non_authoritative_document_view`, did not occur, so `#harnessswitchverify` stays gated. The installed JetBrains package is current (`0.2.356`), but the IDE host must be restarted to load that generation before repeating the unsaved-buffer test. Separately, preflight reports that the installed `agent-doc` binary/cdylib predates committed source change `5a69fdba7`; run `make install` before the next verification.

No code change or test run was warranted for this diagnostic cycle; I updated the existing review item with the failed-proof evidence and exact retry conditions.

### Re: do [#qtypeclobber] — gpt-5

Fixed and closed. Queue maintenance no longer treats its stale preflight image as a whole-document replacement: it component-rebases the maintenance candidate over the latest Lazily canonical cut, then compare-and-swaps that exact rebased cut. The applied canonical—not the stale candidate—now drives later snapshot synchronization, so a prompt or even a few operator keystrokes published during preflight survive.

I also fenced the detached fallback: durable editor liveness now forbids a direct disk replacement, preventing JetBrains stamp changes behind an open document. The regression recreates multiple concurrent queue additions plus the exact three typed bytes `xyz`; all survive while the maintenance mutation applies.

Verification passed: the full `agent-doc-preflight-io` test suite, `make check`, release install, plugin listing, and latest GitHub Actions CI (`success`). Functional commit: `14dea7352`; required tsift instruction refresh: `eadced4b6`.

### Re: fpe.md File Cache Conflicts — gpt-5

> **Queue prompt:**
>
> do [#qtypeclobber]


The three recorded fpe.md conflicts were real, but the evidence points to a stale loaded IntelliJ plugin generation: the IDE still had fpe.md open while the old bridge reported detached authority, allowing disk changes behind the dirty buffer. This was not caused by fpe.md content.

I installed the verified binary/library and JetBrains package into every detected IDE root, including IntelliJ IDEA 2026.1. That activates the relay-side in-flight-edit guard and the new durable-liveness disk-write fence. The running IDE must still be restarted to load the new Kotlin package generation; native-library reload cannot replace that code. I did not force the restart because fpe.md has an active session/possibly unsaved buffer. Save or preserve that buffer, then restart IntelliJ IDEA 2026.1 before continuing edits in fpe.md.
<!-- agent:boundary:1b85b380 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue priority go -->
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#coder1] Phase 1 — terminal-host environment classification (CODER_* / DISPLAY / SSH_CONNECTION / tmux presence) + `agent-doc env --json`. VERIFY the real Coder env var names, the JB remote-backend probe, and vscode.env.remoteName against a live workspace before landing. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder2] Phase 2 — headless tmux bootstrap: `agent-doc tmux ensure <FILE>` (idempotent detached new-session) and rewrite ensure_inside_tmux (agent-doc-start-io/src/lib.rs:934) to auto-start + re-exec into the pane instead of bailing. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder3] Phase 3 — JetBrains IDE-hosted terminal: open/reuse an `agent-doc` tab via TerminalToolWindowManager, run the attach command, dialog fallback with a Copy action. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder4] Phase 4 — VS Code IDE-hosted terminal: window.createTerminal/sendText parity, reuse live terminals, report vscode.env.remoteName in diagnostics, update editors/plugin-parity.tsv. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder5] Phase 5 — `[terminal] host/auto_start_tmux/attach_command` config + `terminal_host:` frontmatter override + pure resolution function shared by CLI and both plugins + runbooks/coder-workspace.md. Do NOT add a `[terminal] session` key — reuse the existing project `tmux_session` binding owned by `agent-doc session set`. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder6] Phase 6 — workspace restart resilience: record tmux server identity, GC stale registry rows + orphaned .sock files on server-identity mismatch, guard AF_UNIX-incapable project-root filesystems. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder7] [operator-verify] Phase 7 — live Coder workspace validation matrix (JB Gateway cold start, VS Code Remote-SSH cold start, both IDEs attached at once, workspace stop/start, image without tmux). Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#harnessswitchverify] [operator-verify] #harnessswitchstorm remains unverified. The 2026-08-25 fpe.md attempt was invalidated by a stale live JetBrains plugin generation: ops.log showed controller_crdt_replica_refusal_storm (25 refusals), editor_absent/disk authority, agent_restart_respec_inert to Claude, then old_harness=claude -> new_harness=codex only after save. Unblocks after make install, restart the JetBrains IDE host, repeat the unsaved claude <-> codex switch, and prove exactly one spawn plus agent_restart_deferred reason=non_authoritative_document_view.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
