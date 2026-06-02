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

### Re: #mdsectionspans — gpt-5

Implemented the Markdown stable span layer for read/write planning.

What changed:
- Markdown indexing now creates full-section heading spans: a heading extends through nested content until the next same-or-higher heading, with body byte/line ranges preserved separately.
- Markdown list items are indexed as `list_item` symbols, and fenced code blocks expose code-body byte ranges plus their fence language.
- `source-read`, `symbol-read`, and `edit-intents` span JSON now include Markdown metadata: `heading_level`, `section_path`, `section_handle`, `list_depth`, and `fence_language` when applicable.
- Full-projection backend-eval watermarks now continue to exclude agent-doc/session Markdown from the full-project cache key, so richer Markdown symbols do not make task-doc edits invalidate full-projection caches.
- The graph-db API paging regression test no longer assumes a specific global `mentions` edge ordering now that Markdown emits more symbol rows.
- Updated `SPEC.md`, `CLAUDE.md`, and `VERSIONS.md` for the new Markdown span contract.

Commits created:
- `90e18f7` in `src/tsift`: `feat: add markdown section spans`
- `051707461` in the superproject: `agent-doc(tsift): update submodule pointer`

Verification:
- `cargo test -p tsift-graph markdown` passed.
- `cargo test -p tsift-cli markdown` passed.
- `cargo test -p tsift-cli graph_db_api_queries_sqlite_neighborhood_and_schema` passed after updating the order-sensitive assertion.
- Full `make check` passed with exit code 0 via digest artifact `tart-7fb46342af`.
- `cargo install --path .` passed via digest artifact `tart-29733f2bf2`; installed binary reports `tsift 0.1.62`.
- Latest GitHub Actions `CI` run inspected: `26800161158` completed successfully. Jobs `clippy + test` and `live Convex graph acceptance` are both green.
- No `tmux-test` job, `tmux-ci` target, or tmux-test reference exists in this tsift repo/workflow, so there was no separate tmux CI leg to inspect.

`#mdsectionedits` remains the next queued implementation item; I did not mark it complete or gated in this cycle.

### Re: #mdsectionedits deferred — gpt-5

No implementation was started for `#mdsectionedits` in this cycle. It remains the next queued backlog item; this closeout only repairs the binary-owned boundary after completing and reaping `#mdsectionspans`.

### Re: #mdsectionedits — gpt-5

> **Queue prompt:**
>
> [#mdsectionedits]


Implemented and committed `#mdsectionedits`.

What changed:
- `edit-intents --apply` now supports Markdown section intents: `rename_heading`, `replace_section_body`, `insert_section`, and `move_section`.
- Markdown section apply re-parses the current buffer for each intent, validates output through tree-sitter Markdown, and writes through the existing atomic edit/rollback path with no formatter.
- `move_section` accepts `destination_symbol` plus optional `position` (`before`/`after`); `insert_section` can append or insert before/after a target heading.
- Block intents `insert_list_item` and `rewrite_code_fence` remain recognized but unsupported for the next queued block-executor item.

Committed:
- `src/tsift` submodule: `07cf25f feat: add markdown section edit executor`
- Superproject pointer: `db68ce4e8 agent-doc(tsift): update submodule pointer`

Verification:
- `cargo test -p tsift-cli markdown` passed.
- `cargo test -p tsift-cli edit_intents` passed.
- `cargo test -p tsift-graph markdown` passed.
- `cargo test -p tsift-cli semantic_edit_language_contracts_resolve_current_executor_surface` passed.
- `make check` passed via digest artifact `tart-4b54040c57`.
- `cargo install --path .` passed via digest artifact `tart-830d792d19`; installed binary reports `tsift 0.1.62`.
- Latest GitHub Actions CI run `26800161158` is green: `clippy + test` and `live Convex graph acceptance` both succeeded.
- No tmux CI target/workflow was found in this repo (`rg "tmux|tmux-test|tmux-ci" .github Makefile packages src tests` returned no matches).

Note: I also ran `cargo fmt --check`; it reports pre-existing rustfmt drift in unrelated files, so I restored that unrelated formatting churn and kept this commit scoped to the Markdown executor/docs/tests.

### Re: #mdblockedits — gpt-5

> **Queue prompt:**
>
> [#mdblockedits]


Implemented and committed `#mdblockedits`.

What changed:
- `edit-intents --apply` now supports Markdown block intents: `insert_list_item` and `rewrite_code_fence`.
- `insert_list_item` requires a unique indexed list-item target, preserves the target marker and indentation, and supports `position` (`before`/`after`).
- `rewrite_code_fence` requires a unique fenced-code target, rewrites only the fence body, refuses replacement text that includes fence markers, and preserves the existing fence syntax/language.
- Apply refusal diagnostics now include the unsupported plan message, so ambiguous Markdown list/code-fence targets report the exact ambiguity before mutation.

Committed:
- `src/tsift` submodule: `7a02458 feat: add markdown block edit executor`
- Superproject pointer: `c1438ba2c agent-doc(tsift): update submodule pointer`

Verification:
- `cargo test -p tsift-cli edit_intents_apply_mutates_markdown_block_intents` passed.
- `cargo test -p tsift-cli edit_intents_markdown_block_apply_refuses_ambiguous_targets_without_mutating` passed.
- `cargo test -p tsift-cli markdown` passed.
- `cargo test -p tsift-cli edit_intents` passed.
- `cargo test -p tsift-graph markdown` passed.
- `cargo test -p tsift-cli semantic_edit_language_contracts_resolve_current_executor_surface` passed.
- `make check` passed via digest artifact `tart-d349462e1a`.
- `cargo install --path .` passed via digest artifact `tart-830d792d19`; installed binary reports `tsift 0.1.62`.
- Latest GitHub Actions CI before this new push, run `26830983751`, was green: `clippy + test` and `live Convex graph acceptance` both succeeded.
- No tmux CI target/workflow was found in this repo (`rg "tmux|tmux-test|tmux-ci" .github Makefile packages src tests` returned no matches).

`#mdverify` remains the next queued Markdown item.
<!-- agent:boundary:e3236188 -->
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
- ~[#mdsectionedits]~
- ~[#mdblockedits]~
- [#mdverify]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
