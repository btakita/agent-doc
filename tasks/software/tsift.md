---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260602-015206.md`*

Compacted content:
- Archived 8 response topic(s): SurrealDB parity; SurrealDB performance; SurrealDB optimization potential; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ - Can tsift be a token-efficient replacement for the Read tool for agents by providing lookups, the ast of the specified abstraction, and other navigation/search capabilities?
❯ - Can tsift be a token-efficient replacement for the Write tool by providing an ast interface to modify the code?

### Re: tsift as Read/Write substrate — gpt-5

Yes for a large part of `Read`, but I would frame it as a token-efficient read/navigation front end rather than a total replacement. The current surface already does the important token-saving work: `search`, `explain`, and `graph` locate relevant symbols and relationships; `source-read` returns bounded source windows with intersecting symbol refs, summary refs, and expansion commands; `context-pack`, `diff-digest`, `session-review`, and digest-runner replace raw reads of transcripts, diffs, logs, and test output. That lets agents widen context progressively instead of opening whole files.

The missing piece is that current `source-read` is still a source-window reader with index annotations, not a true exported AST subtree for a selected abstraction. A stronger `Read` replacement would add a `symbol-read` / `ast-read` surface that returns the selected symbol's signature, body range, child declarations, outgoing calls, inbound callers, imports, summaries, and expansion handles under an explicit token budget. With that, raw `Read` becomes the fallback for exact full-file inspection, unsupported languages, binary/non-text assets, generated files, or index-freshness uncertainty.

For `Write`: not yet. `tsift edit` is useful, but it is currently an atomic old/new string replacement batch with uniqueness checks, staging, backup, and rollback. That can replace some repetitive patch applications, but it is not an AST mutation interface. A real `Write` replacement would need tree-sitter-backed edit intents such as rename symbol, replace function body, insert import, add method, move declaration, update call signature, and rewrite call sites, plus formatting, dry-run diffs, conflict detection against current file content, and per-language conformance tests.

So the practical answer is: `tsift` is plausibly a strong replacement for most exploratory `Read` usage, and should probably become the default first step before raw `Read`. It is only a partial `Write` helper today. The strategic direction is sound, but the write side needs semantic edit operations before it can be treated as a tool replacement instead of a convenience wrapper.
❯ What are #next-steps for replacing Read usage with tsift?
❯ What are #next-steps for replacing Write usage with tsift?

### Re: next steps for tsift Read/Write replacement — gpt-5

Top backlog item: `#readast`.

For replacing `Read`, the priority is to make `tsift` return complete symbol-centered packets, not just annotated source windows. That means adding `symbol-read` / `ast-read`, then routing broad raw file/session/diff/log/test reads through `tsift rewrite --run` or harness hooks, with raw `Read` reserved for exact fallback cases.

For replacing `Write`, the next step is specification first: define the semantic edit intent schema before implementation. Then build an AST-backed executor that can apply those intents with dry-run diffs, formatting, conflict checks, rollback, and language fixtures. Until that exists, `tsift edit` remains useful for atomic string replacements but should not be treated as a general Write replacement.

I added the follow-up backlog in priority order: `#readast`, `#readrewrite`, `#writeintents`, `#writeexecutor`.
<!-- agent:boundary:861846f9 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- [#readast]
- [#readrewrite]
- [#writeintents]
- [#writeexecutor]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#readast] Add a `tsift symbol-read` / `tsift ast-read` surface that returns a token-budgeted symbol packet: signature, body range, child declarations, imports, callers/callees, summaries, and expansion commands.
- [ ] [#readrewrite] Route broad Read-style file opens through `tsift rewrite --run` / hooks for source windows, session docs, diffs, logs, and tests; keep raw Read as the explicit fallback.
- [ ] [#writeintents] Specify semantic write intents for AST-backed edits: rename symbol, replace function body, insert import, add method, update call signature, move declaration, and rewrite call sites.
- [ ] [#writeexecutor] Implement the AST-backed write executor with dry-run diffs, formatting, conflict detection against current file content, rollback, and per-language conformance tests.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
