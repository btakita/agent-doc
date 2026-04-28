---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: investigated missing patchbacks and uncommitted write cycles in this document and related src/session-share sessions; root causes were traced across fresh Codex route-start acknowledgment, write/commit-boundary drift, stale HEAD/boundary preflight churn, pending and backlog mutation capture, submodule writable-root gaps, tmux pane routing and rescue instability, and repair when the response was already present in the document. Multiple src/agent-doc fixes were implemented and pushed on origin/20260422/codex-agent-backend, including fail-closed startup/session-check/commit handling, strict session write-commit, pending-done and bypass guards, backlog alias deprecation, tmux duplicate-start/swap/sync-scope fixes, and repair adoption of already-present responses; a broad commit-all request was blocked on scope, then narrowed and src/agent-doc was verified with cargo test, cargo build, and cargo install --path . --force and pushed as 0e18871. No unresolved exchange prompts remained at compaction time.

❯ `agent-doc backlog` should detect an id in the text when a backlog item is sent...such as `[#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, and JSONL-backed fixtures so future benchmark drift is attributable`...which currenly results in:

```
- [ ] [#sgzy] [#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, and JSONL-backed fixtures so future benchmark drift is attributable
```

❯ Instead the backlog item should be:
```
- [ ] [#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, 
v and JSONL-backed fixtures so future benchmark drift is attributable
```
### Re: backlog custom id detection — gpt-5

Implemented in `src/agent-doc`.

`agent-doc` now preserves explicit alphanumeric backlog IDs even when they are longer than 8 characters, so inputs like `[#sdig2matrix] ...` no longer get re-backfilled into `[#generated] [#sdig2matrix] ...`.

I updated the pending/backlog parser, the imperative diff normalizer, the backlog docs/spec, and added regressions for:
- direct `backlog add`
- `write --pending-add`
- `replace:pending` normalization / backfill preserving long custom IDs

Verification completed in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

❯ On /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md, I closed the codex + tmux session. Then a new session autostarted. I ran the document. Then the codex + tmux session autoclosed. Why? spec-test-build-install-commit-push
### Re: codex + tmux session autoclosed after a document run — gpt-5

I traced this from the real `claudescore-3` session log instead of guessing.

What happened:
- the pane was not killed by tmux cleanup
- the supervisor log shows a normal successful turn closeout: `codex_exit code=0`
- immediately after that, the same `agent-doc start` supervisor logged `user_quit` and then `session_end`

That is the actual bug. `codex exec` exits `0` after an ordinary completed turn, but `agent-doc start` was still treating every clean exit like the Claude flow: drop into the local `Enter` / `q` restart prompt. So a successful Codex document run was landing on a quit boundary instead of staying attached as a persistent session.

I fixed that in `src/agent-doc` and pushed `0d00a1a` on `origin/20260422/codex-agent-backend`.

What changed:
- added a harness-specific clean-exit policy in `src/harness.rs`
- Claude still prompts on clean exit
- Codex now auto-restarts in resume mode after a clean `codex exec` turn, so the persistent `agent-doc start` session stays attached
- updated `src/start.rs` plus the supervisor and Codex support specs
- added unit coverage for the clean-exit resolution

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

So the direct answer to “why did it autoclosed?” is: because the Codex harness was still using the clean-exit prompt path meant for Claude, and your successful turn hit that quit path instead of auto-resuming.
<!-- agent:boundary:c0144e25 -->
<!-- /agent:exchange -->

<!--
Why is there Codex permission prompts when `codex_args: -s danger-full-access` frontmatter is set?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#pbc2] `claudescore-3.md` patchback not committed — the latest response patchback landed in `src/session-share/tasks/claudescore-3.md`, but no owning `write --commit` / `finalize` closeout committed the session-share repo for that document update. Track this as the older commit-boundary / patchback-ownership failure family, distinct from the post-turn closeout-bypass case in `tasks/agent-doc/agent-doc-bugs3.md`. Plan: `tasks/agent-doc/plan-claudescore-3-patchback-commit-boundary.md`
- [ ] [#reap1] Completed `agent:backlog` items not reaped — template docs can still contain `- [x]` backlog entries after a later normal `agent-doc` cycle, even though the pending system contract says done items should be reaped on the next preflight pass. Track this separately from missing `--pending-done` closeout bugs: this family starts after the item is already `[x]` in the document. Plan: `tasks/agent-doc/plan-backlog-done-reap.md`
<!-- /agent:backlog -->
