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

Resolved #qfixture; auto queue drained.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-172749.md`*

Compacted content:
- Archived 5 response topic(s): duplicate IPC/prompt corruption; fix elaboration; documentation index and process diagram; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The full-document IPC corruption occurred in equityfundingsource.md. I noticed that only the Codex sessions have this issue. root.md using a Claude Code session does not have this issue. Is the Codex path different for this behavior? If yes, can we just standardize on the Claude Code path across all agent sessions?
❯ #spec-test-build-install-commit-push
### Re: Codex closeout path — gpt-5

Yes. The Codex path is different at invocation, routing, and backend capture: Codex uses a plain `agent-doc <FILE>` prompt, Codex hooks, Codex JSONL parsing, and Codex-specific resume/capability behavior. It should not standardize on the Claude Code slash command or Skill-tool path because that path is not available to Codex.

The part that should be standardized is the response persistence path. Once any harness has final assistant text, it must go through the same binary-owned `finalize` / `write --commit` closeout, component-patch write pipeline, snapshot commit, and `session-check` boundary. I updated the specs to make that explicit and added a regression test that locks the invariant down.

Changed paths:
- `src/agent-doc/specs/codex-support.md`
- `src/agent-doc/specs/05-agent-backend.md`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/tests/test_cli.rs`

Verification:
- `cargo test test_codex_shared_closeout_spec_invariants` passed.
- `make check` passed from the workspace root, including `instruction-files`, `src/agent-doc`, and `src/corky`.
- Latest GitHub Actions CI run `26367673444` is green; its `Run make tmux-ci` step also succeeded.
- `make install` passed for `src/agent-doc` and installed `agent-doc v0.33.16` plus `libagent_doc-0.33.16.so` locally.
- `cargo run --quiet -- audit-docs` is blocked by pre-existing root `AGENTS.md` managed-surface drift from the tsift code-navigation block. I preserved that user/worktree drift and added a recommended backlog item for it.

Commit:
- `src/agent-doc`: `8a5058bf spec: clarify codex closeout boundary`
- parent repo: `794dd6c15 chore: update agent-doc submodule`

Push:
- Queued after this session-document closeout so the response commit is included, per the agent-doc commit ordering rule.

Escalation: none required.
Confidence: high for the spec/test change; the remaining audit-docs issue is isolated to the pre-existing managed instruction-surface drift described above.
❯ Why does agent-doc Codex have the full-document IPC corruption issue but not agent-doc Claude Code?
### Re: Codex IPC corruption — gpt-5

The difference is not that Codex should have a separate document-write model. It is that Codex has an extra recovery/input surface before the shared closeout layer, and Claude Code normally does not hit that surface.

Claude Code enters through the slash-command/Skill flow: `/agent-doc <FILE>` produces an explicit response payload, and the session document is mutated by `agent-doc finalize` or `agent-doc write --commit`. Its bridge is mainly a post-write hook/VCS signal. It does not need to capture the final assistant answer out of a generic harness stop event and replay that text.

Codex enters through a plain `agent-doc <FILE>` prompt plus repo-local Codex hooks. The Codex `Stop` hook is a backstop for cases where Codex is about to end while an agent-doc cycle is still open. That hook receives `last_assistant_message`; if that value is a clean single assistant closeout, it can be replayed through repair/finalize. If it is instead a full transcript, full `agent:exchange` dump, prompt lines, or multiple response blocks, replaying it is exactly the full-document corruption class.

So the root cause is the Codex-only Stop-hook/direct-chat recovery path trusting or replaying a broader payload than the normal assistant closeout shape. Claude Code avoids the observed failure because its usual path already has the response body separated from the session transcript before persistence. The right standardization is therefore:

- keep harness-specific launch/routing/capability handling for Codex vs Claude Code;
- require every harness to hand only final assistant response text to the shared closeout layer;
- block Codex Stop-hook replay unless `last_assistant_message` validates as one response or one `patch:exchange` closeout;
- keep full-content editor IPC disabled, including stale fallback files and recovery paths.

This is now reflected in the specs I inspected: `src/agent-doc/specs/codex-support.md`, `src/agent-doc/specs/05-agent-backend.md`, `src/agent-doc/specs/07-closeout-commands.md`, and `src/agent-doc/specs/09-git-integration.md`. The implementation also has `src/agent-doc/src/replay_guard.rs`, which is the right enforcement boundary: ambiguous transcript-shaped payloads should be blocked, not auto-extracted.

Changed paths: none; this was an explanation-only cycle.
Commands: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `rg`, `sed`.
Verification: no code changes, so I did not run the test suite.
Escalation: none.
Confidence: high on the architectural cause; any remaining question is whether every legacy Codex recovery path now consistently calls the replay guard before saving pending replay.
❯ Does Codex support hooks similar to Claude Code to avoid the extra step? Does Codex support `/agent-doc` commands and would we be able to avoid having to use the path causing the full-document IPC issues?
### Re: Codex hooks and slash commands — gpt-5

Yes, Codex has hooks here, but they are not the same escape hatch as Claude Code's `/agent-doc` Skill path.

What exists in the installed Codex path:

- `.codex/config.toml` enables `features.hooks = true`.
- `.codex/hooks.json` wires `UserPromptSubmit` to `agent-doc hook codex-user-prompt-submit`.
- `.codex/hooks.json` wires `Stop` to `agent-doc hook codex-stop`.
- The current `codex --help` exposes hook trust controls such as `--dangerously-bypass-hook-trust`, which confirms hooks are a real Codex integration surface in this install.

What does not exist is project-defined slash command parity with Claude Code. The installed Codex runbook and `.codex/AGENTS.md` both say not to type `/agent-doc`; Codex reserves leading slash commands for its own built-ins and rejects project-defined `/agent-doc`. The Codex-native entrypoint is still the normal message form:

`agent-doc <FILE>`

So, can we avoid the path that caused full-document IPC issues? Yes, but not by switching Codex to `/agent-doc`. The safer boundary is:

- use `UserPromptSubmit` only to track the active document and same-message directive body;
- keep the agent's real response going through explicit `agent-doc finalize` / `agent-doc write --commit` with `patch:exchange`;
- treat the Codex `Stop` hook as a fail-closed backstop, not the primary write path;
- make `Stop` replay only validated single-response payloads, and block transcript/full-exchange/full-document payloads before they can touch pending replay.

That means the bad path is not "Codex hooks" in general. It is specifically Stop-hook/direct-chat recovery replaying too much text. The UserPromptSubmit hook is useful and should stay. The Stop hook should either be conservative replay-only-after-validation, or even stricter: block and tell the agent to run `finalize` when the payload shape is not provably one assistant closeout.

Changed paths: none; this was an explanation-only cycle.
Commands: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `codex --help`, `sed`, `rg`.
Verification: no code changes, so I did not run tests.
Escalation: none.
Confidence: high for the installed Codex behavior in this workspace; lower only for future Codex CLI versions if OpenAI later adds project-defined slash-command support.
❯ What are the #next-steps for the recommended path?
### Re: recommended Codex path next steps — gpt-5

The recommended path is to keep Codex-native invocation, keep Codex hooks, and remove the unsafe part of the recovery surface rather than trying to emulate Claude Code slash commands.

Next steps, in priority order:

1. Make `tasks/agent-doc/plan-cancelled-stop-hook-exchange-replay.md` the implementation plan for the recommended path. The core change is: `UserPromptSubmit` may track active document/directive context, but `Stop` must only replay payloads that validate as one assistant closeout or one `patch:exchange` response.
2. Audit every Codex Stop/direct-chat recovery entrypoint before `repair::save_pending(...)`. If an entrypoint can save `last_assistant_message`, it must call the replay guard first and block transcript/full-exchange/full-document shapes.
3. Add regressions around both sides of the boundary: valid single-response Stop payload succeeds; full `agent:exchange`, prompt-line transcript, multiple `### Re:` headings, and full-document dumps are blocked and saved only as diagnostics.
4. Keep explicit `finalize` / `write --commit` as the primary success path in the Codex instructions. The Stop hook should be a fail-closed backstop, not the normal writer.
5. After the fix, run targeted Stop-hook tests, broader `src/agent-doc` tests, `make check`, and the tmux/CI leg before closing the implementation item.

I added the top backlog item for this path as `#cstop`, ahead of the existing recommended items.

Changed paths: session document backlog only.
Commands: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `tsift status --fix`, `sed`, `rg`.
Verification: no code changes, so I did not run tests.
Escalation: none.
Confidence: high; the remaining implementation work is scoped to enforcing the replay guard consistently and proving it with regressions.
<!-- agent:boundary:063b2879 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#1mm8]
- do [#hg0n]
- do [#te1d]
- do [#pmz2]
- do [#f4xr]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#cstop] [recommended] Implement the conservative Codex Stop-hook replay path in tasks/agent-doc/plan-cancelled-stop-hook-exchange-replay.md: keep UserPromptSubmit tracking, require explicit finalize/write primary closeout, and block transcript/full-exchange payloads before pending replay.
- [ ] [#e86b] [recommended] Resolve pre-existing root AGENTS.md managed instruction-surface drift from the tsift code-navigation block so `cargo run --quiet -- audit-docs` can pass.
- [ ] [#rpatch] Fix response patchback recovery so stale-cycle closeout cannot report already committed while a valid visible response patchback remains uncommitted; implement tasks/agent-doc/plan-response-patchback-uncommitted.md.
- [ ] [#qresidue] [recommended] Fix inactive agent:queue residue being read as active response context; implement tasks/agent-doc/plan-drained-non-auto-queue-cleanup.md.
- [ ] [#qinactive] [recommended] Prevent edits inside inactive agent:queue blocks, such as do [#gdbpropscan], from becoming prompt targets or cross-document #agent-doc-bug backlog captures; implement tasks/agent-doc/plan-inactive-queue-residue-prompt-target.md.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
