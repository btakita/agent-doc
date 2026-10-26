---
description: "Interactive markdown session for OpenCode. TRIGGER: user invokes /agent-doc <file> command, or a bare `/agent-doc` (the file this tmux pane claims). Requires a markdown session document, installed CLI, and write+commit every cycle."
user-invocable: true
argument-hint: "[file]"
agent-doc-version: "0.35.554"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Invocation

```
/agent-doc <FILE>
/agent-doc
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`). With no FILE, run `agent-doc resolve-pane-document` first and use the path it prints as FILE for the whole cycle. If it exits nonzero (not in tmux, no claim, several claims), report its reason and stop; never guess a document.

**Note:** Slash commands (`/agent-doc`) are installed as an OpenCode command via `agent-doc skill install --harness opencode`. Other harnesses receive the document path directly.
## Required cycle

This file is a routing table, not the workflow manual. For every normal turn, read [runbooks/session-cycle.md](runbooks/session-cycle.md), then load the branch runbooks it requires. Complete requested work before the mandatory binary-owned closeout. Never manually patch the final response into the session document.

**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`; if newer, follow the OpenCode update procedure in [runbooks/harness-invocation.md](runbooks/harness-invocation.md), then continue as directed.

## Runbook table

| When | Read completely |
|---|---|
| Every normal cycle | [session-cycle.md](runbooks/session-cycle.md) |
| Harness invocation/update, hooks/routing/continuation, planning, orchestration, or presets | [harness-invocation.md](runbooks/harness-invocation.md), [queue-auto-loop.md](runbooks/queue-auto-loop.md), [planning-dispatch.md](runbooks/planning-dispatch.md), [model-tier-gate.md](runbooks/model-tier-gate.md), [command-synonyms.md](runbooks/command-synonyms.md), [compound-task-steering.md](runbooks/compound-task-steering.md), [preset-runbooks.md](runbooks/preset-runbooks.md) |
| Responding, steering, queue/subagent work, backlog, review, done, or TODO mutations | [respond.md](runbooks/respond.md), [pending-ops.md](runbooks/pending-ops.md), [streaming-checkpoints.md](runbooks/streaming-checkpoints.md) |
| Verification/persistence, recovery, compaction/extraction, format, images, remote/tmux/manual jobs, or instruction authoring | [persist-closeout.md](runbooks/persist-closeout.md), [commit.md](runbooks/commit.md), [baseline-drift.md](runbooks/baseline-drift.md), [jb-cache-conflict.md](runbooks/jb-cache-conflict.md), [compact-exchange.md](runbooks/compact-exchange.md), [transfer-extract.md](runbooks/transfer-extract.md), [document-format.md](runbooks/document-format.md), [code-enforced-directives.md](runbooks/code-enforced-directives.md), [describe-image.md](runbooks/describe-image.md), [coder-workspace.md](runbooks/coder-workspace.md), [multi-tmux-sessions.md](runbooks/multi-tmux-sessions.md), [manual-job-packets.md](runbooks/manual-job-packets.md), [dynamic-context.md](runbooks/dynamic-context.md), [split-spec-files.md](runbooks/split-spec-files.md), [okf/index.md](okf/index.md) |

Read a required runbook once per session and re-read it only after compaction or a content change. Trust the binary's current cycle contract and emitted commands over remembered prose.
