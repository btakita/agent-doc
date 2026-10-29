---
description: "Interactive markdown session. TRIGGER: agent-doc <file>, or a bare `/agent-doc` / `agent-doc` (the file this tmux pane claims). Requires binary admission and committed closeout."
user-invocable: true
argument-hint: "[file]"
agent-doc-version: "0.35.560"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Invocation

Submit `agent-doc <FILE>` in Grok Build. With no FILE (`/agent-doc` or `agent-doc` alone), call connected `agent_doc_admit` without `file`: the binary admits the document this tmux pane claims and returns it as `file`; use that path as FILE for the whole cycle. If it refuses (not in tmux, no claim, several claims), report its reason and stop; never guess a document. Read this skill, call connected `agent_doc_admit` for FILE, then read the document and run `agent-doc plan <FILE>`. Never recursively launch `agent-doc <FILE>` from a shell.

Grok's UserPromptSubmit hook cannot inject admission context or continue the turn: an allowing prompt hook's stdout is discarded. Successful MCP admission is the required checkpoint. Use connected `agent_doc_finalize` for closeout. If MCP tools are absent, install with `agent-doc skill install --harness grok`, then enable/trust the project MCP server in Grok. Never bypass refused admission.

While this Grok session owns the document, the project PostToolUse hook (`agent-doc hook steering-post-tool-use` in `.grok/hooks/agent-doc-steering.json`) pushes settled operator steering from the live editor replica into the running turn: new queue rows, claimed-row strikes, and claim-forwarded edits. That is the same real-time steering Claude Code and Codex receive through PostToolUse and OpenCode receives by polling `agent-doc steering <FILE>` while its turn runs. The Read tool sees disk, which trails the editor replica; do not trust it for queue state. On that additionalContext, re-call connected `agent_doc_plan` for FILE in the same turn and dispatch every new `queue_subagent_dispatch` row without waiting for another user message. `agent-doc steering <FILE>` remains the monitor path and OpenCode's in-turn delivery; do not ignore hook additionalContext in favor of a poll.

The connected plan is the queue-lifecycle contract. When `queue_subagent_dispatch` is non-empty, dispatch it immediately without waiting for another operator prompt. For each entry, run its `claim_command`, call Grok's native `spawn_subagent` with `background=true` and the authorization preamble verbatim at the start of the worker prompt, then run its `start_command` with the returned `subagent_id`. Give repository-writing workers separate external worktrees. Use `get_command_or_subagent_output` for completion. Do not substitute `agent-doc orchestrate` while native `spawn_subagent` is available. When `queue_dispatch_acknowledgement_required` is non-empty, an earlier claim has no worker binding: run its exact `start_command` if spawn succeeded, or its `release_command` to requeue if no worker was spawned. These fields are actionable even when `prompt_targets` is empty; never call that `no_changes` or resume the item inline.

After a committed finalize, follow its queue-continuation fields in the same turn, admitting and completing each next actionable head. Do not end while a drainable head remains. Do not run another session-check after a successful connected finalize.
## Required cycle

This file is a routing table, not the workflow manual. For every normal turn, read [runbooks/session-cycle.md](runbooks/session-cycle.md), then load the branch runbooks it requires. Complete requested work before the mandatory binary-owned closeout. Never manually patch the final response into the session document.

**Auto-update skill:** if the binary is newer, run `agent-doc skill install --harness grok`, re-read `.grok/skills/agent-doc/SKILL.md`, and continue this turn.

## Runbook table

| When | Read completely |
|---|---|
| Every normal cycle | [session-cycle.md](runbooks/session-cycle.md) |
| Harness invocation/update, hooks/routing/continuation, planning, orchestration, or presets | [harness-invocation.md](runbooks/harness-invocation.md), [queue-auto-loop.md](runbooks/queue-auto-loop.md), [planning-dispatch.md](runbooks/planning-dispatch.md), [model-tier-gate.md](runbooks/model-tier-gate.md), [command-synonyms.md](runbooks/command-synonyms.md), [compound-task-steering.md](runbooks/compound-task-steering.md), [preset-runbooks.md](runbooks/preset-runbooks.md) |
| Responding, steering, queue/subagent work, backlog, review, done, or TODO mutations | [respond.md](runbooks/respond.md), [pending-ops.md](runbooks/pending-ops.md), [streaming-checkpoints.md](runbooks/streaming-checkpoints.md) |
| Verification/persistence, recovery, compaction/extraction, format, images, remote/tmux/manual jobs, or instruction authoring | [persist-closeout.md](runbooks/persist-closeout.md), [commit.md](runbooks/commit.md), [baseline-drift.md](runbooks/baseline-drift.md), [jb-cache-conflict.md](runbooks/jb-cache-conflict.md), [compact-exchange.md](runbooks/compact-exchange.md), [transfer-extract.md](runbooks/transfer-extract.md), [document-format.md](runbooks/document-format.md), [code-enforced-directives.md](runbooks/code-enforced-directives.md), [describe-image.md](runbooks/describe-image.md), [coder-workspace.md](runbooks/coder-workspace.md), [multi-tmux-sessions.md](runbooks/multi-tmux-sessions.md), [manual-job-packets.md](runbooks/manual-job-packets.md), [dynamic-context.md](runbooks/dynamic-context.md), [split-spec-files.md](runbooks/split-spec-files.md), [okf/index.md](okf/index.md) |

Read a required runbook once per session and re-read it only after compaction or a content change. Trust the binary's current cycle contract and emitted commands over remembered prose.
