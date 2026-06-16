---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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
- Added one centralized `TmuxSubmitProfile` in `agent-doc-orchestration/src/sessions.rs`; `tmux_submit_mode_for_harness`, `tmux_submit_transform_for_harness`, and `tmux_submit_key_for_harness` delegate to it.
- Routed JB `Run Agent Doc`, JB `Clear Session Context`, queue dispatch, auto-trigger inject, and supervisor-owned tmux inject logging through that shared profile.
- Removed OpenCode as a default Kitty Return submit special case. The separate OpenCode PTY filtering for Kitty keyboard-mode permission prompts remains, because that is not the command-submit path.
- Kept owned PTY injection as `pty_cr`; the change is for tmux-bound live pane submit, not the supervisor child-PTY write boundary.
- Updated visible-draft recovery to be profile-driven, so a timed-out visible draft gets exactly one bare `Enter` retry for OpenCode too.
- Updated specs/docs/tests and the static ops-report example so the only tmux live-pane submit mode is `tmux_literal_text_enter_key`.
- `cargo test submit` passed.
- `cargo test queue_dispatch` passed.
- `cargo test -p agent-doc-orchestration submit` passed.
- `make check` passed: clippy, 4542 nextest tests, doctests, and the ignored medium sim-world budget test.
- `cargo build` passed.
- `cargo install --path .` completed and replaced `/home/brian/.cargo/bin/agent-doc`; `agent-doc --version` reports `0.34.1`.
- Latest `CI` on `main` was green: run `27588543817`, `fix(jb): add run-agent-doc proof logging`, completed successfully on 2026-06-16.
- `cargo install --path .` completed and replaced `/home/brian/.cargo/bin/agent-doc`; `agent-doc --version` reports `0.34.1`.
- `cargo build` passed.
- `make check` passed: clippy, 4542 nextest tests, doctests, and the ignored medium sim-world budget test.
- `cargo test -p agent-doc-orchestration submit` passed.
- `cargo test queue_dispatch` passed.
- `cargo test submit` passed.
- Updated specs/docs/tests and the static ops-report example so the only tmux live-pane submit mode is `tmux_literal_text_enter_key`.
- Updated visible-draft recovery to be profile-driven, so a timed-out visible draft gets exactly one bare `Enter` retry for OpenCode too.
- Kept owned PTY injection as `pty_cr`; the change is for tmux-bound live pane submit, not the supervisor child-PTY write boundary.
- Removed OpenCode as a default Kitty Return submit special case. The separate OpenCode PTY filtering for Kitty keyboard-mode permission prompts remains, because that is not the command-submit path.
- Routed JB `Run Agent Doc`, JB `Clear Session Context`, queue dispatch, auto-trigger inject, and supervisor-owned tmux inject logging through that shared profile.
- Added one centralized `TmuxSubmitProfile` in `agent-doc-orchestration/src/sessions.rs`; `tmux_submit_mode_for_harness`, `tmux_submit_transform_for_harness`, and `tmux_submit_key_for_harness` delegate to it.
- Latest GitHub Actions `CI` on `main` is green: run `27588543817`, `fix(jb): add run-agent-doc proof logging`.
- `make check` passed locally before the install: clippy/build/full test suite through the project check target.
- `cargo test -p agent-doc-orchestration stale_supervisor` passed after the change.
- This document's route-owned supervisor restart correctly refused while pane `%110` is busy with this active turn, so I did not force-interrupt it. After this turn is idle, it can recycle/restart onto the new binary; no JetBrains plugin update is needed for this fix.
- The CPC recycled onto a fresh process (`controller_generation=1263`).
- `agent-doc lib-install` installed `/home/brian/.cargo/bin/libagent_doc-0.34.1.so` and marked controllers for recycle.
- `cargo install --path .` replaced `/home/brian/.cargo/bin/agent-doc`.
- `agent-doc --version` is `0.34.1`.
- The definitive freshness check today is inode identity: `stat -Lc '%i %s %Y %n' /home/brian/.cargo/bin/agent-doc /proc/<controller_pid>/exe /proc/<supervisor_pid>/exe`. Same inode means the process is running the installed binary; different inode means recycle/restart.
- `agent-doc admin inspect <doc> --json` confirms the document actor, supervisor pid/socket/runtime state, and whether a queue pause is active.
- `agent-doc controller status` confirms the CPC is active, stable, and shows controller pid/generation.
- `agent-doc --version` confirms the installed CLI version.
- I cleared this document's stale durable queue pause with `agent-doc admin queue resume ...`; `agent-doc admin inspect tasks/agent-doc/agent-doc-bugs2.md --json` now reports `queue_control: null`.
- Regression tests now cover the exact `#qchurn ... stale host supervisor pid 2715614 ...` wording and PID extraction.
- `pause_reason_is_stale_supervisor_churn_stop` now recognizes `stale host supervisor` as the same recoverable stale-supervisor churn-stop class.
- `src/agent-doc/agent-doc-orchestration/src/project_controller/rpc.rs`
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260616-020038.md`*

Compacted content:
❯ - Archived 4 response topic(s): JB Run Agent Doc Codex boot timeout; JB Run Agent Doc Logging; Queue Repair Closeout; 1 more
❯ - Prior summary/context: prior compacted content: Archived 39 response topic(s): #compactdrift-agent; Compact Closeout Guard Follow-Up; Restor...

### Re: tmux submit profile and OpenCode Enter — gpt-5

> **Queue prompt:**
>
> :pushpin: Why are we ever using `tmux_literal_cr` and not `tmux_literal_text_enter_key`? Why do we have so much duplicate logic with the submit keys all over the codebase? Each harness' tmux_literal should be defined in a single place. No tmux_literal values spread all over the place. Why tmux_literal_kitty_return for opencode? Is tmux_literal_text_enter_key not working? #simworld


> **Queue prompt:**
>
> :pushpin: Both JB `Clear Session Context` and JB `Run Agent Doc` send a CR instead of submitting ![img_24.png](img_24.png). There is also an extra prompt that bleeds into the input ![img_25.png](img_25.png). Why is this so difficult to fix? Can we design a way to detect what is sent and how it is processed by the agent? Fix root causes. #simworld


Fixed the tmux-bound submit root cause in `src/agent-doc`: the default live-pane path no longer sends a raw CR, and OpenCode no longer has a Kitty Return submit profile.

Answer to the direct tmux question: yes, the submit path now uses the terminal-neutral shape you tested, but with literal text protection: `tmux send-keys -t <pane> -l <text>; tmux send-keys -t <pane> Enter`. The split `-l` text + named `Enter` avoids tmux interpreting prompt text as keys while still delivering the real Enter key event. Codex, Claude, OpenCode, and unknown/default harnesses all resolve through that same profile now.

What changed:
- Added one centralized `TmuxSubmitProfile` in `agent-doc-orchestration/src/sessions.rs`; `tmux_submit_mode_for_harness`, `tmux_submit_transform_for_harness`, and `tmux_submit_key_for_harness` delegate to it.
- Routed JB `Run Agent Doc`, JB `Clear Session Context`, queue dispatch, auto-trigger inject, and supervisor-owned tmux inject logging through that shared profile.
- Removed OpenCode as a default Kitty Return submit special case. The separate OpenCode PTY filtering for Kitty keyboard-mode permission prompts remains, because that is not the command-submit path.
- Kept owned PTY injection as `pty_cr`; the change is for tmux-bound live pane submit, not the supervisor child-PTY write boundary.
- Updated visible-draft recovery to be profile-driven, so a timed-out visible draft gets exactly one bare `Enter` retry for OpenCode too.
- Updated specs/docs/tests and the static ops-report example so the only tmux live-pane submit mode is `tmux_literal_text_enter_key`.

I did not add `lazily-rs` here. The duplication was removed at the policy boundary instead: a small Rust enum/function is enough, avoids adding another dependency, and keeps submit semantics greppable from one place. If we later need generated dispatch tables for more harness behavior, that is a separate design pass.

Verification:
- `cargo test submit` passed.
- `cargo test queue_dispatch` passed.
- `cargo test -p agent-doc-orchestration submit` passed.
- `make check` passed: clippy, 4542 nextest tests, doctests, and the ignored medium sim-world budget test.
- `cargo build` passed.
- `cargo install --path .` completed and replaced `/home/brian/.cargo/bin/agent-doc`; `agent-doc --version` reports `0.34.1`.
- Latest `CI` on `main` was green: run `27588543817`, `fix(jb): add run-agent-doc proof logging`, completed successfully on 2026-06-16.

Changed source paths include `agent-doc-orchestration/src/sessions.rs`, `agent-doc-orchestration/src/route/dispatch.rs`, `agent-doc-orchestration/src/flow/routed_reopen.rs`, `agent-doc-orchestration/src/start/idle_watch.rs`, `agent-doc-orchestration/src/start.rs`, `agent-doc-orchestration/src/start/supervisor_io.rs`, `src/queue_dispatch.rs`, `src/session_actor_cmd.rs`, `src/orchestrate/dispatch.rs`, `src/sim_world.rs`, `src/ops_report.rs`, `README.md`, and the tmux/session-routing supervisor specs.

Source commit/push status: not done. The `src/agent-doc` checkout already had broad unrelated dirty changes before this turn, and `cargo fmt` exposed large pre-existing formatting churn across many files. Staging whole files would include unrelated work, and a partial source commit built on the dirty tree would not be a trustworthy standalone commit. The safe next step is to isolate this patch on a clean branch or explicitly approve staging the broader dirty set.

JetBrains plugin status: no JetBrains plugin source changed, so no JB plugin update is required for this fix. The installed `agent-doc` CLI binary was updated. The route-owned supervisor for this document was still running an older binary during preflight, so it should be restarted to use the new submit policy.

### Re: OpenCode tmux Enter — gpt-5

> **Queue prompt:**
>
> :pushpin: Does agent-doc use `tmux send-keys -t target "command" Enter` to submit a prompt? Would this work with opencode? I don't want to hardcode a kitty return b/c kitty is specific to my environment...not universal. I'm concerned that it wont work on other terminals. I tried `tmux send-keys -t .1 "Hello" Enter` on opencode and it worked.


Yes. For live tmux panes, agent-doc now submits with literal text followed by a named tmux `Enter`: `tmux send-keys -t <pane> -l <command>; tmux send-keys -t <pane> Enter`. That is the terminal-neutral version of `tmux send-keys -t target "command" Enter`, and it should work with OpenCode just like your `tmux send-keys -t .1 "Hello" Enter` test.

OpenCode no longer gets `tmux_literal_kitty_return` as its submit profile. The only remaining OpenCode Kitty handling is the separate PTY filter that preserves Kitty keyboard mode for permission-prompt navigation; it is not used to submit `agent-doc <FILE>` prompts.

Verification is unchanged from the implementation closeout: `cargo test submit`, `cargo test queue_dispatch`, `cargo test -p agent-doc-orchestration submit`, `make check`, `cargo build`, and `cargo install --path .` all passed after the OpenCode Enter change.

### Re: lazily-rs for submit-key policy — gpt-5

> **Queue prompt:**
>
> :pushpin: Can we use lazily-rs to avoid duplicate logic with the submit keys all over the codebase?


I would not use `lazily-rs` for this submit-key fix.

The duplicate logic was not a laziness or caching problem; it was a policy-boundary problem. The right fix is the one now in place: a single `TmuxSubmitProfile` owns the mode, transform, key name, and visible-draft retry eligibility. Call sites ask that profile instead of spelling `tmux_literal_cr`, `tmux_literal_text_enter_key`, or any OpenCode-specific submit mode themselves.

Using `lazily-rs` here would add indirection without reducing meaningful runtime cost or behavioral branching. It may be useful later for generated or memoized harness capability tables, but command-submit semantics should stay explicit, tiny, and directly covered by tests.

### Re: supervisor freshness, stale queue pause, and Codex pane formatting — gpt-5

> **Queue prompt:**
>
> :pushpin: Issue with restart on Codex ![img_26.png](img_26.png) ![screenshot.png](../../../../../Pictures/agent-loop/screenshot.png)


I found and fixed the concrete JB `Run Agent Doc` error path from the pasted output. The existing recovery code already knew how to restart/resume on `queue_paused` when the reason said `stale supervisor pid...`, but your live reason said `stale host supervisor pid 2715614`. That wording did not match the classifier, so the controller treated the pause as terminal and JB surfaced it as an error.

Changed source path:
- `src/agent-doc/agent-doc-orchestration/src/project_controller/rpc.rs`

What changed:
- `pause_reason_is_stale_supervisor_churn_stop` now recognizes `stale host supervisor` as the same recoverable stale-supervisor churn-stop class.
- Regression tests now cover the exact `#qchurn ... stale host supervisor pid 2715614 ...` wording and PID extraction.
- I cleared this document's stale durable queue pause with `agent-doc admin queue resume ...`; `agent-doc admin inspect tasks/agent-doc/agent-doc-bugs2.md --json` now reports `queue_control: null`.

How to tell whether CPC + supervisor are up to date:
- `agent-doc --version` confirms the installed CLI version.
- `agent-doc controller status` confirms the CPC is active, stable, and shows controller pid/generation.
- `agent-doc admin inspect <doc> --json` confirms the document actor, supervisor pid/socket/runtime state, and whether a queue pause is active.
- The definitive freshness check today is inode identity: `stat -Lc '%i %s %Y %n' /home/brian/.cargo/bin/agent-doc /proc/<controller_pid>/exe /proc/<supervisor_pid>/exe`. Same inode means the process is running the installed binary; different inode means recycle/restart.

Current local state:
- `agent-doc --version` is `0.34.1`.
- `cargo build` passed.
- `cargo install --path .` replaced `/home/brian/.cargo/bin/agent-doc`.
- `agent-doc lib-install` installed `/home/brian/.cargo/bin/libagent_doc-0.34.1.so` and marked controllers for recycle.
- The CPC recycled onto a fresh process (`controller_generation=1263`).
- This document's route-owned supervisor restart correctly refused while pane `%110` is busy with this active turn, so I did not force-interrupt it. After this turn is idle, it can recycle/restart onto the new binary; no JetBrains plugin update is needed for this fix.

On the formatting screenshot: direct `agent-doc admin --help` output is normal in a plain command run. The screenshot shows corruption in the route-owned Codex pane/supervisor display path: help/start output is scattered and the Codex shell appears offset. That is a separate PTY/screen-model bug, not the help text itself. I added a backlog item to capture it with logging and a reproduction instead of hand-waving it.

Verification:
- `cargo test -p agent-doc-orchestration stale_supervisor` passed after the change.
- `make check` passed locally before the install: clippy/build/full test suite through the project check target.
- Latest GitHub Actions `CI` on `main` is green: run `27588543817`, `fix(jb): add run-agent-doc proof logging`.

Source commit/push status: not done. The `src/agent-doc` submodule already has broad unrelated dirty changes, including the same file, and a whole-file commit would include unrelated work. I left the source change in the working tree and committed this session response through `agent-doc` only.

### Re: do [#codexpaneformat] + do [#freshnessstatus] — gpt-5

> **Queue prompt:**
>
> The restart stalled. As I was typing, numerous `/clear` and restarts occurred.


Implemented both items, tested them, and installed the updated local binary.

`#codexpaneformat`: route-owned `agent-doc start` now keeps its startup/status chatter out of the child pane by default. The `[start] harness resolved`, registration, network-access, starting/restarting, and similar route-owned status lines go to the session log instead of stdout/stderr unless verbose diagnostics are enabled, so the Codex TUI starts at the top of the pane without row-offset corruption. The log still records `start_console_status ... printed=false` proof for audit.

`#freshnessstatus`: `agent-doc controller status` and `agent-doc admin inspect --json` now include a structured `freshness` object: installed binary identity, installed inode, running controller inode, optional route-owned supervisor inode, `matches_installed`, `stale`, and operator guidance. The CLI enriches `controller status` client-side too, so an older active controller still yields a freshness proof before it recycles. Plain `admin inspect` now prints `freshness=controller:<state>,supervisor:<state>`.

Docs/version notes were updated in `specs/07-session-tmux-commands.md`, `specs/08b-single-process-control-plane.md`, and `VERSIONS.md`.

Verification:
- `cargo fmt --check`
- focused tests: `controller_process_freshness_classifies_inode_identity`, `controller_status_reports_startup_binary_identity`, `controller_queue_control_rejects_stale_generation_and_blocks_dispatch_when_paused`, `route_owned_start_status_logs_without_printing_by_default`
- `cargo check -p agent-doc-orchestration`
- `make check` passed: 4544 tests, 238 skipped; doctests and medium simulation passed
- `cargo build`
- `cargo install --path .`
- latest GitHub Actions CI check: `CI` completed success on `main` at 2026-06-16T01:47:40Z, run `27588543817`

Live proof after install: `agent-doc controller status --project-root /home/brian/work/btakita/agent-loop --ensure` reports the controller fresh by inode. A warmed `agent-doc admin inspect tasks/agent-doc/agent-doc-bugs2.md --json` now reports the current controller fresh and the route-owned supervisor for this document stale by inode (`route_owned_supervisor.stale=true`), which is the operator-facing freshness proof this item needed.

Source commit/push caveat: I did not commit or push the agent-doc source changes because `src/agent-doc` was already broadly dirty before this cycle, including files I had to touch. Staging whole files would have included unrelated pre-existing changes.

### Re: JB Run Agent Doc EFS auto-sync proof — gpt-5

> **Queue prompt:**
>
> JB `Run Agent Doc` on equityfundingsource.md after a fresh auto-sync did not submit the prompt.


I checked the `equityfundingsource.md` route path against the live logs and the current `src/agent-doc` source. The fix for this queue head is already landed and pushed on `main` in `src/agent-doc`: `f5844c53` (`fix(jb): surface codex boot timeouts`) and `f27a388b` (`fix(jb): add run-agent-doc proof logging`). `src/agent-doc` is even with `origin/main`.

The useful log evidence is that the fresh auto-sync route did not fail to submit silently. For `equityfundingsource.md`, the first fresh route sent the literal text plus Enter to pane `%101`, then recorded `route_dispatch_only_sent` with `submit_mode=tmux_literal_text_enter_key`, `proof=consumed`, and `proof_scope=dispatch_start`. Later retries were accepted by the pane but did not produce dispatch-start proof within the wait window, and those are now surfaced as `accepted_without_dispatch_start_proof` / `route_dispatch_only_submit_unproven` instead of being reported as a clean success. That is the behavioral distinction the JetBrains proof logging fix needed.

Changed source already present in the pushed commits:

- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt`
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt`
- `editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/SubmitActionTest.kt`
- `editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/TerminalUtilTest.kt`
- `agent-doc-orchestration/src/project_controller.rs`
- `agent-doc-orchestration/src/project_controller/rpc.rs`
- `editors/jetbrains/SPEC.md`
- `specs/07-session-tmux-commands.md`
- `specs/08b-single-process-control-plane.md`

Verification:

- `cargo test -p agent-doc-orchestration dispatch_only` passed: 32 passed, 0 failed, 13 ignored.
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests com.github.btakita.agentdoc.SubmitActionTest --tests com.github.btakita.agentdoc.TerminalUtilTest` passed.
- `gh run list --workflow CI --limit 1` shows the latest CI run for `fix(jb): add run-agent-doc proof logging` completed successfully on `main`.
- Latest GitHub Actions `CI` on `main` is green: run `27588543817`, completed successfully on 2026-06-16T01:47:40Z.
- `agent-doc --version` reports `agent-doc 0.34.1` at `/home/brian/.cargo/bin/agent-doc`
- `agent-doc lib-install`
- `cargo install --path .`
- `cargo build`
- `make check`
- `cargo test -p agent-doc-orchestration supervisor_auto_install`
- `cargo test -p agent-doc-orchestration dogfood_crate_root_rejects_unrelated_superproject_docs`
- `git diff --check` on the touched paths
- `cargo fmt --check`
- `VERSIONS.md`
- `specs/08b-single-process-control-plane.md`
- `agent-doc-core/src/project_config.rs`
- `agent-doc-core/src/frontmatter.rs`
- `agent-doc-orchestration/src/project_controller.rs`
- `agent-doc-orchestration/src/start/decisions.rs`
- `agent-doc-orchestration/src/start/idle_watch.rs`
- `agent-doc-orchestration/src/project_controller/rpc.rs`

JetBrains plugin note: this fix does require the JetBrains plugin update. The checked-in plugin version is now `0.2.167`, and the local signed artifact exists at `src/agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.167-signed.zip`.

No new source changes were made in this closeout turn; this cycle verified the already-pushed fix and closed the live EFS auto-sync queue item with proof.

### Re: Only agent-doc dogfooding sessions should auto-build + install — gpt-5

Fixed the auto-install scope bug. The live logs confirmed the failure mode: route-owned supervisors for `tasks/professional/equityfundingsource.md`, `tasks/software/lazily-rs.md`, and other sibling project docs were resolving `src/agent-doc` and running `supervisor_auto_install_started ... crate_root=src/agent-doc`. That was wrong.

Root cause: `dogfood_agent_doc_crate_root` only checked whether the current superproject contained an `agent-doc` crate. Since this repo has `src/agent-doc`, every session document under the superproject could inherit dogfood build/install behavior. The resolver now also proves the served document is an agent-doc dogfood session document: inside the agent-doc source checkout, under `tasks/agent-doc/`, or one of the legacy agent-doc task docs. Sibling docs such as `tasks/professional/equityfundingsource.md` and `tasks/software/lazily-rs.md` now resolve no auto-install crate root even if the env/frontmatter/project auto-install knob is truthy.

Changed paths:
❯ - `agent-doc-orchestration/src/project_controller/rpc.rs`
❯ - `agent-doc-orchestration/src/start/idle_watch.rs`
❯ - `agent-doc-orchestration/src/start/decisions.rs`
❯ - `agent-doc-orchestration/src/project_controller.rs`
❯ - `agent-doc-core/src/frontmatter.rs`
❯ - `agent-doc-core/src/project_config.rs`
❯ - `specs/08b-single-process-control-plane.md`
❯ - `VERSIONS.md`

Verification:
❯ - `cargo fmt --check`
❯ - `git diff --check` on the touched paths
❯ - `cargo test -p agent-doc-orchestration dogfood_crate_root_rejects_unrelated_superproject_docs`
❯ - `cargo test -p agent-doc-orchestration supervisor_auto_install`
❯ - `make check`
❯ - `cargo build`
❯ - `cargo install --path .`
❯ - `agent-doc lib-install`
❯ - `agent-doc --version` reports `agent-doc 0.34.1` at `/home/brian/.cargo/bin/agent-doc`
❯ - Latest GitHub Actions `CI` on `main` is green: run `27588543817`, completed successfully on 2026-06-16T01:47:40Z.

`agent-doc lib-install` installed `/home/brian/.cargo/bin/libagent_doc-0.34.1.so` and marked 2 controllers for idle recycle. No escalation was needed. Source commit/push was not done because `src/agent-doc` already has broad unrelated dirty changes, including some of the same files; staging whole files would mix this fix with pre-existing work.

### Re: commit + push uncommitted agent-doc files — gpt-5

> **Queue prompt:**
>
> commit + push uncommitted agent-doc files


Committed and pushed the uncommitted `src/agent-doc` submodule files.

Commit/push proof:
- `src/agent-doc`: committed `cdbc1a78` (`chore: checkpoint agent-doc updates`) and pushed `main` to `github.com:btakita/agent-doc.git`.
- Parent repo: committed the `src/agent-doc` submodule pointer as `37bee5bcc` (`agent-doc: checkpoint submodule updates`) and pushed `main` to `github.com:btakita/agent-loop.git`.

Verification carried into the commit:
- `cargo fmt --check`
- `git diff --check`
- `cargo test -p agent-doc-orchestration dogfood_crate_root_rejects_unrelated_superproject_docs`
- `cargo test -p agent-doc-orchestration supervisor_auto_install`
- `make check`
- `cargo build`
- `cargo install --path .`
- `agent-doc lib-install`

Current state:
- `src/agent-doc` is clean and even with `origin/main`.
- Parent repo is even with `origin/main` for committed work, but still has unrelated dirty files and untracked work outside this queue head.
- Latest `src/agent-doc` GitHub Actions `CI` for `cdbc1a78` is in progress: run `27593485048`, started 2026-06-16T04:08:16Z. Per the closeout rules, I recorded it as pending instead of blocking on `gh run watch`.

No escalation was needed.
<!-- agent:boundary:ced8e25e -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~JB `Run Agent Doc` on equityfundingsource.md after a fresh auto-sync did not submit the prompt.~~
- ~~commit + push uncommitted agent-doc files~~
---
Typing into the queue during the turn cause the conversation to be interrupted. ![img_30.png](img_30.png). A pre-finalize response was uncommitted causing the response to be treated as a user prompt ![img_31.png](img_31.png). After restart, JB `Run Agent Doc` error.

```
[route] target tmux session: 0
Error: authoritative actor generation 438 for tasks/agent-doc/agent-doc-bugs2.md owns pane %110 but route could not drain the active closeout before dispatch: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. Rebuild sidecars without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
```
---
- Why not just shell out `tmux send-keys "Command" Enter`? Is the JB `Run Agent Doc` not-submiting an issue or is it the fact that logic is still spread all over the codebase and we are hitting the various conditions that use this spaghetti logic?
- :pushpin: Only agent-doc dogfooding sessions should auto-build + install. The equityfundingsource.md session should not auto-install ![img_29.png](img_29.png)
- :pushpin: How can I tell if the supervisor + CPC are up to date? Why is the formatting all messed up? ![2026-06-15_22-56.png](../../../../../Pictures/agent-loop/2026-06-15_22-56.png) Also I'm seeing some shells from the supervisor are 2 lines too low. See where the `agent-doc admin --help` starts in the screenshot.
---
:pushpin: After restarting a session in the terminal, JB `Run Agent Doc` errors. It should not error.
```
resync: pruned 3 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=#qchurn no-op churn: go-mode re-injecting :pushpin: [#qchurn] each idle boundary; all 16 heads are clean-session/operator-verify and undrainable under stale host supervisor pid 2715614; zero drainable work. Pausing to stop the flood until restart onto fresh binary. receipt_id=3649 blocked_head_bytes=132 blocked_head_sha256=ae121dad65e5b00a6c504db706985aee4e8d6ed00988ba4244f51196952739d7 trigger_bytes=44 trigger_sha256=a55a6572fd4dc952bc476bd516d3a7801b1e63409715f063b4b7351ea112ade5
```
---
- :pushpin: agent-doc start to codex misformatted ![img_27.png](img_27.png) ![img_28.png](img_28.png)
- :pushpin: Complete the `[operator-verify]` and live verification using the logs to prove/disprove the intended behavior.
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
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
- [ ] [#lazilyqrestore] [operator-verify] Restore/account for lost queue content in `tasks/software/lazily-rs.md`: that document is foreign-owned by another pane, so capture the intended lost queue heads from the owning lazily-rs session/doc history/snapshots/sidecars, restore them as backlog items there, or record explicit user-removal proof. Related #queue-clear-unrun-items/#manual-queue-head-loss/#queue-user-edit-overwrite.
- [ ] [#recyclerestart-verify] [operator-verify] #recyclerestart live proof + promotion: Q1 (recycle ENOENT / restart) + Q2 (FFI self-heal sync guard, supersede-newer-generation) are committed (agent-doc 0b31e90c, fa6a230a) but NOT promoted onto the live stale supervisor — the img_23 stall is the pre-fix binary. Recycle/restart the supervisor onto the new build (agent-doc admin recycle --all-projects, or restart), then confirm: kill-pane + JB Sync Tmux Layout no longer reports Sync deferred, and a real idle/next-queue recycle restarts/re-clears the agent session. Cannot be agent-done. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof — two-live-IDE CRDT broadcast convergence: with the installed build, edit the same managed doc in JB and VS Code at once and confirm both editor buffers receive the broadcast patch and converge (no clobber). Implementation landed agent-doc cddad87b (id/deliver/merge, SimWorld-covered, CI green). Cannot be agent-done — needs two live editors. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#1j8q] [operator-verify] #suprecyclelock live proof: drive a real next_queue_item supervisor self-recycle (go-mode queue drain over a freshly-installed binary) while a sibling/concurrent launcher holds the shared project-root controller-launch.lock; confirm the execve'd start no longer surfaces "controller launch already in progress ... (os error 11)" on the pane and ops.log shows the contended waiter adopting the published controller instead. Fixed in agent-doc-orchestration project_controller.rs LaunchLock::acquire_blocking + connect_or_launch.
- [ ] [#anw0verify] [operator-verify] #anw0 redirect live proof: install the new agent-doc binary (cargo build --release + cargo install + lib-install) then drive a real recycle / restart-supervisor race so a dispatcher holding a superseded generation hits the new path. Confirm ops.log shows dispatch_stale_generation_redirect prior_generation=N next_generation=N+1 then dispatch_retry_after_stale_generation and a clean re-dispatch (no terminal generation-N-is-closed). Code+SimWorld landed agent-doc 21a70cdd / superproject 412355fa3. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#qflood2] [operator-verify] qflood regression: JB Run Agent Doc on efs coalesced as exit-1 (dual-drain owner, gen 66 already in flight). Needs live multi-process verify; if it still collides, coalesce should report deduped-success not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
- [ ] [#qt32] [operator-verify] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [ ] [#fcc0-live-verify] [operator-verify] #fcc0 live JB 0-dialog proof: with the current agent-doc binary/cdylib installed and the route-owned supervisor recycled onto it, drive a live JetBrains queue/template/compact write path and confirm ops.log shows editor IPC convergence (for example queue_consume_writeback ... transport=editor_ipc) and zero JetBrains File Cache Conflict dialogs. Cannot be agent-done; requires live IDE observation.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
