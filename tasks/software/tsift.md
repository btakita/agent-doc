---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-205847.md`*

Compacted content:
- Archived 8 response topic(s): token savings, graph neighborhoods, and claude-mem; first-party memory crate/plugin; tsift-memory crate/plugin; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Can we replace claude-mem with tsift-memory? Can we migrate all claude-mem data into tsift-memory?

### Re: claude-mem and tsift-memory — gpt-5

Yes, but there are two meanings of "replace".

For retrieval/reuse, tsift can already stand in front of `claude-mem`. The current spec and code project `claude-mem` rows into graph-db as read-only semantic/session/source/vector-handle rows, and `tsift memory status` sees the existing `~/.claude-mem/claude-mem.db` plus adjacent Chroma directory. That means existing memory can seed tsift graph/semantic queries without mutating `claude-mem`.

For replacement as the owner of live memory capture, not completely yet. `tsift-memory` owns `.tsift/memory.db`, schema versioning, budget guards, hook contracts, import commands, and deduped event storage. I only found writes through `tsift memory init` and `tsift memory import-claude-mem`; I did not find automatic agent-doc/Claude closeout hooks writing live prompt/tool/summary/session-check events into `tsift-memory` yet. That wiring is the main remaining replacement step.

Migration is available for the supported `claude-mem` memory tables. Dry-run evidence against this workspace:

- `tsift memory status /home/brian/work/btakita/agent-loop --json`: target `.tsift/memory.db` is not initialized; `claude-mem` DB is readable; Chroma is present.
- `tsift memory import-claude-mem /home/brian/work/btakita/agent-loop --limit 40000 --json`: plans 50,447 imported events from `observations`, `session_summaries`, and `user_prompts`.
- The default import is capped at 1,000 rows per table, so a full supported import needs an explicit high `--limit`.

Caveat: "all `claude-mem` data" is not true yet if it means every byte/table. `pending_messages` is inspected and currently has 6,541 rows, but the importer does not convert it. Chroma vectors are preserved as external handles/read-only graph rows; they are not copied into `.tsift/memory.db`.

Practical answer: use `tsift-memory` as the new first-party memory substrate, migrate the supported `claude-mem` rows now, and keep the old `claude-mem` DB as a read-only compatibility source until pending-message import and live capture hooks are added. I did not apply the import.

Proof: changed paths: none by hand; document response/backlog only through `agent-doc finalize`. Verification commands: `tsift memory status ... --json`, `tsift memory import-claude-mem ... --json`, and full dry-run with `--limit 40000`. Escalation: none. Confidence: high for current supported migration behavior, medium for full replacement until the live hook and pending-message gaps are closed.
#next-steps
<!-- agent:boundary:a2e0390b -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsiftmemhooks] [recommended] Wire agent-doc/Claude closeout capture into tsift-memory, then switch live capture/retrieval call sites away from direct claude-mem ownership.
- [ ] [#tsiftmempending] [recommended] Extend claude-mem migration to handle pending_messages or explicitly exclude it from the full-replacement contract, with tests.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
