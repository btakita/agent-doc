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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260602-144937.md`*

Compacted content:
- Archived 10 response topic(s): next steps for read/write AST support; #astspans; #writerefs; 7 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #mdastcontract — gpt-5

> **Queue prompt:**
>
> [#mdsectionspans]


> **Queue prompt:**
>
> [#mdastcontract]


Implemented the Markdown semantic edit language contract for `tsift edit-intents`.

What changed:
- Added canonical Markdown language contract support: `.md` / `.mdx` resolve to `markdown`, aliases include `markdown`, `md`, and `mdx`, parser binding is `graph::Lang::Markdown`, and formatter policy is `None`.
- Split semantic edit contract metadata into recognized intent kinds vs apply-supported intent kinds.
- Added recognized Markdown intents: `rename_heading`, `replace_section_body`, `insert_section`, `move_section`, `insert_list_item`, and `rewrite_code_fence`.
- Kept Markdown apply support intentionally empty for this queue item, so dry-runs recognize/validate Markdown intents but report `unsupported` with `apply_supported=false`; `--apply` fails closed without mutating Markdown files.
- Updated `SPEC.md`, `CLAUDE.md`, and `VERSIONS.md` to document the dry-run-only Markdown contract state.
- Added compiled CLI regressions for Markdown heading dry-run metadata/span resolution and no-mutation apply refusal.

Commits created:
- `70301c8` in `src/tsift`: `feat: add markdown edit intent contract`
- `47c1ee4bc` in the superproject: `agent-doc(tsift): update submodule pointer`

Verification:
- `cargo test -p tsift-cli semantic_edit_language_contracts_resolve_current_executor_surface` passed.
- `cargo test -p tsift-cli edit_intents_markdown` passed.
- `make check` passed with exit code 0 via digest artifact `tart-c68dee9489` after rerunning the full suite. The first full run exposed a concurrency-sensitive existing lock-fallback test failure; that test passed isolated and passed in the final full-suite rerun.
- `cargo install --path .` passed; installed binary reports `tsift 0.1.62`.
- Latest GitHub Actions `CI` run inspected: `26800161158` completed successfully, with `clippy + test` and `live Convex graph acceptance` jobs green.
- No `tmux-test` job, `tmux-ci` target, or tmux-test reference exists in this tsift repo/workflow, so there was no separate tmux CI leg to inspect.

Push is pending until after this binary-owned document closeout, per the agent-doc commit/push ordering.
<!-- agent:boundary:48e1306b -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#mdastcontract]~
- ~[#mdsectionspans]~
- [#mdsectionedits]
- [#mdblockedits]
- [#mdverify]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#mdsectionspans] Markdown heading/section/list/code-fence stable spans: hierarchy metadata, section boundaries, and source-read/edit target refs.
- [ ] [#mdsectionedits] Markdown section edit executor: rename_heading, replace_section_body, insert_section, and move_section with tree-sitter validation.
- [ ] [#mdblockedits] Markdown block edit executor: insert_list_item and rewrite_code_fence while preserving syntax and refusing ambiguous targets.
- [ ] [#mdverify] Markdown semantic-write verification coverage: dry-run/apply no-mutation tests, temp-worktree verify gates, source-read impact summaries, and refusal regressions.
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
