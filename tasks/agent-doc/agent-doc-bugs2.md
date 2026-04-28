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
<!-- agent:boundary:e50cbfcc -->
<!-- /agent:exchange -->

<!--
On /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md, I closed the codex + tmux session. Then a new session autostarted. I ran the document. Then the codex + tmux session autoclosed. Why? 

Why is there Codex permission prompts when `codex_args: -s danger-full-access` frontmatter is set?
-->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->
