---
name: reference_agent_doc_bugs2_done_is_archive_not_session
description: tasks/agent-doc-bugs2.done.md is the agent:done ARCHIVE sink; the live session doc is tasks/agent-doc/agent-doc-bugs2.md
metadata: 
  node_type: memory
  type: reference
  originSessionId: 7b8e3c3a-f205-4401-8c0b-27c1e79f2001
---

`tasks/agent-doc-bugs2.done.md` (title `# Agent Doc Completed Work`, a flat DONE list, no `agent:` component markers) is the **completed-work archive** written via `agent:done archive=tasks/agent-doc-bugs2.done.md`. It is NOT the interactive session document — `--pending-add` against it fails with "document has no backlog/pending component".

The **live session doc** is `tasks/agent-doc/agent-doc-bugs2.md` (note the `agent-doc/` subdir): template mode, CRDT write, with `agent:exchange` / `agent:queue` / `agent:backlog` / `agent:review` / `agent:done`.

**How to apply:** When "continue agent-doc-bugs2" is requested, preflight `tasks/agent-doc/agent-doc-bugs2.md`, not the `.done.md` at the repo-tasks root. If `ls tasks/agent-doc-bugs2*.md` only shows the `.done.md`, the real doc is one dir down. Preflight's `claims` line ("Claimed .../tasks/agent-doc/agent-doc-bugs2.md") points to the true session doc. The `#agent-doc-bug` preset = "create a plan + add to backlog of tasks/agent-doc/agent-doc-bugs2.md". Related: [[feedback_agent_doc_commit_regresses_head_to_smaller_snapshot]].
