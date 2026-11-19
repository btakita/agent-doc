---
description: "Interactive markdown session. TRIGGER: agent-doc <file>, or a bare `/agent-doc` / `agent-doc` (the file this tmux pane claims). Requires binary admission and committed closeout."
user-invocable: true
argument-hint: "[file]"
agent-doc-version: "0.35.583"
---

# agent-doc

Interactive document session — respond to user edits in a markdown document.

## Invocation

Submit `agent-doc <FILE>` in Grok Build. With no FILE (`/agent-doc` or `agent-doc` alone), call connected `agent_doc_admit` without `file`: the binary admits the document this tmux pane claims and returns it as `file`; use that path as FILE for the whole cycle. If it refuses (not in tmux, no claim, several claims), report its reason and stop; never guess a document. Read this skill, call connected `agent_doc_admit` for FILE, then read the document and run `agent-doc plan <FILE>`. Never recursively launch `agent-doc <FILE>` from a shell.

Grok lifecycle hooks are notifications: stdout cannot inject admission context or continue the turn. Successful MCP admission is the required checkpoint. Use connected `agent_doc_finalize` for closeout. Pass `recap`, or a non-blank `<!-- patch:recap -->` block inside `response`; a template closeout with neither is refused before capture. Format nits stay advisory. See runbooks/recap.md. If MCP tools are absent, install with `agent-doc skill install --harness grok`, then enable/trust the project MCP server in Grok. Never bypass refused admission. Never type `/clear` or `/new` into a live Grok agent-doc pane: `/clear` aliases `/new` and starts a second session in the same process.

When admit or finalize returns `queue_actions`, use those action IDs for claims, starts, heartbeats, and reported closeout. Call `agent_doc_queue_action` with FILE and the exact `next_action_id`. For a start, wait for its `spawn` hand-off, call `spawn_subagent` once using the returned preamble and `background=true`, then pass the native worker ID to the same action as `worker_id`. A start reservation returned without a spawn hand-off means a prior caller already reserved it. Bind the known worker ID when available. When a live worker owes a heartbeat, pass its exact ID after observing it alive. After collecting a native worker's result, use its report action with the exact worker ID, complete response, and prose-only or repository evidence. When an external closeout returns `execute_argv`, run that argument vector through the harness permission path and record `outcome=applied` or `outcome=failed` with its observed reason. A native spawn failure uses `outcome=failed` on the reserved start. Leave a failed receipt terminal until relevant evidence changes and continue independent rows. After native worker results or other changes, call `agent_doc_queue_action` without an ID to read current actions. Keep using the connected plan for prompt and repository scope. The following claim and start commands are for older binaries that omit `queue_actions`.

When `queue_subagent_dispatch` is non-empty, dispatch it immediately without waiting for another operator prompt. For each entry, run its `claim_command` and reserve with its `brief_command` using the printed generation, then call Grok's native `spawn_subagent` with `background=true` and the authorization preamble verbatim at the start of the worker prompt, then run its `start_command` with the returned `subagent_id`. Give repository-writing workers separate external worktrees. Use `get_command_or_subagent_output` for completion. Do not substitute `agent-doc orchestrate` while native `spawn_subagent` is available. When `queue_dispatch_acknowledgement_required` is non-empty, an earlier claim has no worker binding: do these FIRST, before new claims and before any reported-result integration. Use `brief_command` to reserve and get the preamble, spawn once, then bind its worker using `start_command`. An already reserved spawn needs the exact harness result. Use `release_command` only before a reservation, or add `--spawn-failed <reason>` after the harness confirms no launch. These fields are actionable even when `prompt_targets` is empty; never call that `no_changes` or resume the item inline.

The workers are a pool (`#subagentworkpool`): `queue_subagent_dispatch` already fills exactly the free slots of `queue_subagent_pool`, so spawn EVERY listed entry in the same step, never one at a time, and spawn and start every claimed head before integrating any reported (`☑️`) result. When PostToolUse or `agent-doc steering` prints `[agent-doc] reported queue rows are ready to close now (#reportedmidturn)`, complete every listed `☑️` closeout in that same step, before other long work and before this turn ends; refilling the pool does not hold an already-ready closeout until Stop. After a report, release, or freeze, refresh the connected action projection and refill every available slot before processing integration. With legacy binaries, each `queue report-complete`, `queue release`, or `queue freeze` frees a slot and prints `[queue] subagent work pool` with the next heads' exact claim / brief / start commands: run them in that order immediately, before integrating, and keep refilling as workers complete until no head waits. When the refill says a heartbeat is due, run its exact `queue claim --refresh --generation <N>` while the worker is alive; it records proof of life, but a bound worker has no runtime timeout and heartbeat age alone never loses tracking. A worker that ended without a result is frozen: run `agent-doc queue freeze <FILE> --item <item> --generation <N> --reason '<what Grok reported>'` and refill its slot. The emitted refill owns recovery. If it offers that item again, run its exact claim / brief / start commands as a fresh attempt with the new generation and `subagent_id`; never run `queue start` against the frozen owner and generation or reuse its old start command. `agent-doc queue pool <FILE>` reprints the current refill.

After a committed finalize, follow its queue-continuation fields in the same turn, admitting and completing each next actionable head. Do not end while a drainable head remains. Do not run another session-check after a successful connected finalize.
## Required cycle

This file is a routing table, not the workflow manual. For every normal turn, read [runbooks/session-cycle.md](runbooks/session-cycle.md), then load the branch runbooks it requires. Complete requested work and every authorized current-item or returned-subagent effect before the mandatory binary-owned closeout. Never manually patch the final response into the session document.

**Auto-update skill:** if the binary is newer, run `agent-doc skill install --harness grok`, re-read `.grok/skills/agent-doc/SKILL.md`, and continue this turn.

## Runbook table

| When | Read completely |
|---|---|
| Every normal cycle | [session-cycle.md](runbooks/session-cycle.md) |
| Harness invocation/update, hooks/routing/continuation, planning, orchestration, or presets | [harness-invocation.md](runbooks/harness-invocation.md), [queue-auto-loop.md](runbooks/queue-auto-loop.md), [planning-dispatch.md](runbooks/planning-dispatch.md), [model-tier-gate.md](runbooks/model-tier-gate.md), [command-synonyms.md](runbooks/command-synonyms.md), [compound-task-steering.md](runbooks/compound-task-steering.md), [preset-runbooks.md](runbooks/preset-runbooks.md) |
| Responding, steering, queue/subagent work, backlog, review, done, TODO, or mutable-decision work | [respond.md](runbooks/respond.md), [pending-ops.md](runbooks/pending-ops.md), [decisions.md](runbooks/decisions.md), [streaming-checkpoints.md](runbooks/streaming-checkpoints.md) |
| Verification/persistence, recovery, compaction/extraction, format, images, remote/tmux/manual jobs, or instruction authoring | [persist-closeout.md](runbooks/persist-closeout.md), [recap.md](runbooks/recap.md), [commit.md](runbooks/commit.md), [baseline-drift.md](runbooks/baseline-drift.md), [jb-cache-conflict.md](runbooks/jb-cache-conflict.md), [compact-exchange.md](runbooks/compact-exchange.md), [transfer-extract.md](runbooks/transfer-extract.md), [document-format.md](runbooks/document-format.md), [code-enforced-directives.md](runbooks/code-enforced-directives.md), [describe-image.md](runbooks/describe-image.md), [coder-workspace.md](runbooks/coder-workspace.md), [multi-tmux-sessions.md](runbooks/multi-tmux-sessions.md), [manual-job-packets.md](runbooks/manual-job-packets.md), [dynamic-context.md](runbooks/dynamic-context.md), [split-spec-files.md](runbooks/split-spec-files.md), [okf/index.md](okf/index.md) |

Canonical cross-tree authorization choices (`#canonical-cross-tree-choice`) use
one top-level structured choice with `Yes, I authorize <scope>` and
`No, I do not authorize <scope>`; freeform remains available, and each affected
tree carries only an inert `**Reference**: [#<canonical-request-id>]`. See
`respond.md`.

Read a required runbook once per session and re-read it only after compaction or a content change. Trust the binary's current cycle contract and emitted commands over remembered prose.
