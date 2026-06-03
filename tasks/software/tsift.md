---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: opencode
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-182739.md`*

Compacted content:
- Archived 3 response topic(s): #libmod5 SearchBudget extraction; #libmod5b Phase 5b: Extract ConflictMatrix + ContextPack + SessionReviewBudget; #libmod5b completion acknowledged
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #opencode-read — opencode

> **Queue prompt:**
>
> do [#opencode-read]


Implemented `tsift-source-read` as a new OpenCode command shortcut that provides AST-aware source code reading via `tsift --envelope source-read`.

**Changes:**
- `src/tsift/packages/tsift-index/src/init.rs`: Added `tsift-source-read` to `OPENCODE_COMMANDS` array with instruction body directing agents to prefer `tsift --envelope source-read` over the raw Read tool for source code files
- `src/tsift/packages/opencode-tsift/commands/tsift-source-read.md`: Created matching npm package command file
- `src/tsift/packages/opencode-tsift/test/install.test.js`: Updated expected command count from 10 to 11

**Verification:**
- `cargo build` succeeds
- All 4 `opencode` tests pass including npm parity test
- `make check` passes (build, test, lint, opencode-plugin-test)
- `tsift init --opencode` installs `tsift-source-read.md` into `.opencode/commands/`
- `tsift --envelope source-read` returns bounded source window with AST symbol metadata, line previews, and expansion commands

### Re: #opencode-grep — opencode

> **Queue prompt:**
>
> do [#opencode-grep]


Implemented `tsift-search` as a new OpenCode command shortcut that provides AST-aware content search via `tsift --envelope search`.

**Changes:**
- `src/tsift/packages/tsift-index/src/init.rs`: Added `tsift-search` to `OPENCODE_COMMANDS` array, directing agents to prefer `tsift --envelope search` over grep/rg for content search in indexed projects
- `src/tsift/packages/opencode-tsift/commands/tsift-search.md`: Created matching npm package command file
- `src/tsift/packages/opencode-tsift/test/install.test.js`: Updated expected command count from 11 to 12

**Verification:**
- `cargo build` succeeds
- All 4 `opencode` tests pass including npm parity test
- `make check` passes
- `tsift init --opencode` installs `tsift-search.md` into `.opencode/commands/`
- `tsift --envelope search` returns ranked hits with symbol families, file previews, AST-aware scoring, and scale guards

### Re: #codex-read — opencode

> **Queue prompt:**
>
> do [#codex-read]


Added `source-read` and `symbol-read` as first-class commands in the tsift code-navigation instructions that all harnesses (Codex, OpenCode, Claude Code) consume via AGENTS.md/CLAUDE.md.

**Changes:**
- `src/tsift/packages/tsift-index/src/init.rs`: Updated `versioned_section()` to list `tsift --envelope source-read` and `tsift --envelope symbol-read` as preferred alternatives to cat/head for source code reading, before the existing search and explain entries
- Ran `tsift init` to update AGENTS.md in the agent-loop project with the new instructions

**How Codex adopts the AST API:**
- The `tsift init --codex` hook already rewrites shell-based file reads (cat/head/tail/sed) to `tsift --envelope source-read` automatically via `tsift rewrite --run`
- The updated code-navigation instructions now explicitly tell Codex agents to prefer `tsift --envelope source-read` over raw file reads for source code
- Combined with the rewrite hook, Codex agents get AST-aware reading both implicitly (via hook rewrites) and explicitly (via instruction guidance)

**Verification:**
- `cargo build` succeeds
- All 41 `tsift-index` tests pass
- `make check` passes
- `tsift init` updates AGENTS.md with the new `source-read` and `symbol-read` entries

**Queue drained.** All three queue items completed: `#opencode-read`, `#opencode-grep`, `#codex-read`.
<!-- agent:boundary:3569c4d2 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#surreal-warm] Warm-start SurrealDB store: reuse SurrealdbGraphStore across backend-eval runs instead of recreating; measure real-workload median with persistent store
- [ ] [#surreal-delta] Delta projection refresh: implement replace_projection_rows_delta() with row-hash diffing matching SQLite's replace_projection_with_version() pattern
- [ ] [#surreal-multiproc] Multi-process lock behavior: write conformance test with N concurrent reader processes + 1 writer against same SurrealKV path; document lock contention, stale-read behavior, and crash recovery (SPEC.md:304 promotion gate)
- [ ] [#surreal-rt-pool] Shared tokio runtime: accept external Runtime reference or use lazy static instead of per-store Runtime::new()
- [ ] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
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
