---
agent_doc_session: ab1adf12-6fc0-430d-99d7-37a0d33e500c
agent: codex
agent_doc_format: template
agent_doc_write: crdt
enable_tool_search: true
---

# Agent Doc GitHub issue watcher

This dedicated scheduled session watches `btakita/agent-doc` for newly opened
GitHub issues. It files work into
`tasks/agent-doc/agent-doc-bugs.md`; it does not run that document's immediate
`id=main` queue on the watch cadence.

## Exchange

<!-- agent:exchange -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue id=issues interval=1h loop -->
- Inspect the open issues at https://github.com/btakita/agent-doc/issues/. Exclude issues #40 and #45. For each issue not already represented in `tasks/agent-doc/agent-doc-bugs.md`, add one backlog item with the stable id `gh-issue-<number>` and mirror it into that document's `id=main` queue as `do [#gh-issue-<number>]`. Deduplicate by the tuple `(btakita, agent-doc, issue number)` across the target document's backlog, done items, and every queue block. Use only binary-owned cross-document backlog/queue mutations; if an exact `id=main` mutation cannot be proven, fail closed and report the missing primitive instead of editing another queue.
<!-- /agent:queue -->
