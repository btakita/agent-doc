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

Logged three fresh log-triage follow-ups. No open backlog items.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260522-230839.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
- Trailing prompt/context: The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should ...
❯ The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?
❯ #spec-test-build-install-commit-push
### Re: compact prompt duplication — gpt-5

Line-by-line diff was the right diagnostic. This was not the same post-exchange comment cleanup race. The current failure shape came from full template `exchange` compact: it archived and summarized text after the live `agent:boundary`, so the prompt became compacted context and then reappeared as the live prompt.

I fixed full exchange compact to split at the boundary, matching the partial compact invariant: pre-boundary content is archiveable, while post-boundary prompt text remains visible working-tree drift and is excluded from the archive body, summary digest, saved snapshot, and closeout commit.

Changed paths:
- `src/agent-doc/src/compact.rs`
- `src/agent-doc/specs/07-core-commands.md`

Verification:
- `cargo test full_exchange_compact_preserves_trailing_prompt_after_boundary --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc`
- `agent-doc --version` -> `agent-doc 0.33.16`
- `make check` in `src/agent-doc`
- GitHub CI `26316547978`: `make check` and `make tmux-ci` both passed. Only annotation was the upstream Node.js 20 actions deprecation warning for `actions/checkout@v4`.

Commits and push:
- `src/agent-doc`: `db7e80b1` (`fix: preserve compact exchange prompt tail`) pushed to `origin/20260422/codex-agent-backend`.
- Root repo: `c8b85e9f8` (`agent-doc: preserve compact prompt tail`) pushed to `origin/main`.

Confidence: high for this compact-boundary duplicate class. No escalations were needed.
<!-- agent:boundary:ed046065 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
