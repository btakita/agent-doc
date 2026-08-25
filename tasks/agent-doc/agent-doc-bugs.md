---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a03734-3b32-7d33-acc6-84fa83826114
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

### Re: do [#coder1] — gpt-5

Implemented Phase 1 terminal-host detection and released it in agent-doc 0.35.279.

- Added a pure `TerminalHostEnv` classifier in `agent-doc-config`. It reports Coder workspace presence, an authoritative remote-IDE observation, display/SSH availability, tmux installation and server state, then resolves `ide`, `external`, or `none` with an explicit reason.
- Added `agent-doc env` and `agent-doc env --json`; tmux is probed once at the command boundary and the classifier remains deterministic and unit-testable.
- Coder detection uses the current agent's explicit `CODER=true` and workspace identity variables, while deliberately ignoring `CODER_AGENT_TOKEN`. The variable names were checked against Coder's current agent source: https://raw.githubusercontent.com/coder/coder/main/agent/agent.go
- VS Code is bridged through `vscode.env.remoteName`, whose value is extension-defined and absent without a remote extension host: https://code.visualstudio.com/api/references/vscode-api
- JetBrains is bridged through the current `IdeProductMode.isBackend` API, not a guessed environment flag: https://plugins.jetbrains.com/docs/intellij/api-notable-list-2025.html and https://plugins.jetbrains.com/docs/intellij/split-mode-and-remote-development.html
- Covered laptop display, Coder plus VS Code Remote, SSH, missing tmux, and token-only false-positive cases. The CLI integration test also validates the JSON schema.
- On this host, `agent-doc env --json` resolves `external`: display present, tmux installed/running, no Coder or remote-IDE signal. A live Coder workspace was not available here, so the exact Coder signals were verified against the authoritative source and exercised deterministically in tests.

### Re: Compact Exchange and File Cache Conflict — gpt-5

Fixed the shared authority race behind both failures and released it in agent-doc 0.35.279.

The File Cache Conflict was caused by an editor replica registering and allocating its routed CRDT model before the separately scheduled reliable-sync `Open` frame arrived. Controller/write adapters looked only at durable liveness during that gap, falsely classified the document as editor-absent, wrote to disk, and detached authority while the JetBrains buffer still held unpublished text. Compact Exchange then encountered the stranded lazy-delivery projection and emitted an inapplicable response-cycle recovery message.

- All controller, write, and convergence paths now use the relay-owned authority predicate: either a routed allocated CRDT model or durable reliable-sync liveness proves editor attachment. A registered replica therefore bridges the delayed-`Open` window and disk fallback is forbidden.
- Added regressions for process-scoped replica registration across delayed liveness and for the write path never selecting disk during that gap.
- Compact pre-write projection refusals now use the central retained-projection ownership model. Their remedy says to retry the same mutation after the editor observes the base and explicitly forbids `commit`, `write --commit`, or forced disk recovery when no response-cycle write owns the projection.
- Added compact and ownership-guard regressions, plus updated the realtime and command specifications.
- `make check` passes in full, `git diff --check` passes, version sync passes, and 0.35.279 is installed locally. Commit `d9a596599` is on `main`; tag `v0.35.279` is published. GitHub CI and Release were triggered.

For `contracts.md`, the original Compact Exchange target remains safely retained by continuation `compact-projection-966ee7b4087795d9d5f0669817d206a37f284349ffdd4dbe6f8d277d9bd44f77-f5ac0a17-c77a-406b-824b-a87ab52e516a`; the corrected command returned that same continuation without rewriting. The editor is currently detached, so settlement awaits editor reconnection. I did not retry the compact or force a disk write, preserving the editor-owned content and avoiding another File Cache Conflict.

### Re: do [#coder2] — gpt-5

Implemented Phase 2 headless tmux bootstrap.

- Added `agent-doc tmux ensure <FILE> [--session NAME] [--json]`. It reuses a live registered document target before applying the cold-start precedence `--session` → project `tmux_session` → `0`, creates detached sessions idempotently, and reports the session, pane, attach command, creation state, resolution source, and registered document pane.
- Changed CLI `agent-doc start` outside tmux to ensure the session, provision a pane when needed, and re-exec the original start request inside tmux while preserving force, resume/fresh, route ownership, and reap-policy flags. Hosts without tmux now use the shared terminal-host classifier's workspace-image guidance.
- Preserved the single-owner invariant: an exact live registry match is reused and does not start a second supervisor unless `--force` was explicitly requested.
- Documented the command and bootstrap contract in `specs/07-core-commands.md`.

Verification passed:

- `cargo test -p agent-doc-start-io` — 11 passed.
- `cargo test --test test_cli test_cli_start_outside_tmux_bootstraps_a_detached_session -- --nocapture` — passed.
- `cargo test --test test_cli test_cli_tmux_ensure_help_exposes_headless_bootstrap_contract -- --nocapture` — passed.
- Isolated live invocation called `tmux ensure` twice and returned the same pane with `created=true` then `created=false`.
- Rustfmt check passed for all changed Rust production files.

### Re: do [#coder3] — gpt-5

Implemented Phase 3 JetBrains IDE-hosted tmux.

- Extended the `tmux ensure` receipt with authoritative attached-client state. A live registered external session with an attached client remains a no-op, while a detached or newly-created session is eligible for IDE attachment.
- Added the optional JetBrains Terminal integration. It opens one `agent-doc` tab with `TerminalToolWindowManager`, reuses and focuses a live tab, and executes the binary-provided attach command.
- Wired both `Run Agent Doc` and autostarting `Sync Tmux Layout` through the same headless ensure operation before controller dispatch. Run preparation participates in the existing per-document command state, so repeated clicks coalesce instead of racing session creation.
- Declared `org.jetbrains.plugins.terminal` as an optional bundled dependency. A missing/disabled Terminal plugin or terminal API failure produces a notification containing the exact attach command and a Copy action; route dispatch remains available.
- Added pure receipt/host-decision tests and documented the integration contract.

Verification passed:

- `make check` — 9,047 passed, 253 skipped; docs and formal models passed.
- `./gradlew test` — full JetBrains plugin suite passed.
- `./gradlew test buildPlugin` — plugin packaging passed.
- `cargo test -p agent-doc-start-io` — 11 passed.
- Live CLI receipt for the current external session reported `attached=true`, `created=false`, and the registered pane, proving the IDE no-op branch receives the required authority fact.

Committed as `bec955e6e feat: host tmux in JetBrains terminal`. Live GUI tab behavior remains for the dedicated `#coder7` operator-verification phase.
<!-- agent:boundary:1b85b380 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue priority go -->
- ~~do [#coder1]~~
- ~~do [#coder2]~~
- ~~do [#coder3]~~
- do [#coder4]
- do [#coder5]
- do [#coder6]
- do [#coder7]
<!-- /agent:queue -->

<!-- agent:backlog -->
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
