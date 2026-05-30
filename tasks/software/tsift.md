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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-212202.md`*

Compacted content:
- Archived 3 response topic(s): claude-mem and tsift-memory; tsift-memory initialization; tsift-memory status repair
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

❯ Archived 3 response topic(s): claude-mem and tsift-memory; tsift-memory initialization; tsift-memory status repair

### Re: archived response summary — gpt-5

Normalized the added archive line as a document-state signal. The three compacted topics are already recorded in the session summary, so that line does not add a new source-code directive by itself.

Preflight also warned that the post-exchange HTML comment mentioning `#next-steps` is non-executable while it remains outside `agent:exchange` or `agent:queue`.

Proof:
- Changed paths: session document only, through `agent-doc finalize`.
- Commands: `agent-doc --version`; `tsift status`; `tsift status --fix`; `agent-doc preflight tasks/software/tsift.md`; `agent-doc plan tasks/software/tsift.md`.
- Verification: preflight and plan completed; no source-code change was required for this prompt target.
- Confidence: high.
- Escalation: none.
<!-- agent:boundary:c762c1cc -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
dispatch #spec-test-build-install-commit-push
- do [#tsiftmeminit]
- do [#tsiftmemhooks]
- do [#tsiftmempending]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsiftmeminit] Initialize .tsift/memory.db for this workspace, run the supported claude-mem import with explicit --limit/--apply, refresh graph-db, and verify imported counts before deleting or disabling the old claude-mem source.
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
