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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260602-194853.md`*

Compacted content:
- Archived 2 response topic(s): #mdastapi; #mdtokens
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve the search and navigatibility of AST data across all languages including Markdown?

### Re: next steps — gpt-5

Top backlog item: `#astspansearch`.

I would prioritize the work around making AST spans first-class search artifacts, not around adding another standalone Markdown command. The current code already has indexed symbol hits with AST span metadata, Markdown symbol extraction for headings/list items/fenced code blocks, `source-read` Markdown projections, and `markdown-ast` expansion. The gap is that search still presents AST data mostly as symbol/file results plus sidecar expansion paths, instead of treating spans and graph neighborhoods as the main retrieval/navigation units.

I added the backlog in this priority order:

1. `#astspansearch` - promote AST spans to primary search artifacts across languages, including Markdown blocks.
2. `#astqueryfacets` - add cross-language query facets such as language, symbol kind, node kind, section, parent/child, heading level, list depth, and fence language.
3. `#astnavgraph` - project AST containment and adjacency edges into `graph-db` so navigation works through parent/child, sibling, section/module, call/import/route, and Markdown containment relationships.
4. `#mdembeddedcode` - parse Markdown fenced code blocks as embedded language islands and link prose sections to code symbols.
5. `#astrankingmerge` - merge symbol, span, lexical, summary, and graph-neighborhood evidence into one AST-aware ranking path.
6. `#astnavfixtures` - add multilingual fixtures and perf gates so search recall, stable handles, and expand commands stay locked across all supported languages.

Verification: ran `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `agent-doc orchestrate --from-exchange --plan`, refreshed `tsift` indexing, checked `src/tsift` status, and inspected the spec plus the relevant search/index/Markdown AST implementation paths. No code changes were made in this cycle.

### Re: #astspansearch — gpt-5

> **Queue prompt:**
>
> [#astspansearch]


Implemented `#astspansearch` and marked it done.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift` submodule pointer in the parent repo

What changed:
- Search preview symbol-family rows now include `language`, optional `end_line`, and a first-class `ast` artifact for the representative indexed symbol when the source file is readable and the index row has span columns.
- The `ast` artifact carries `artifact_kind=ast_span`, the `span-*` handle, node kind, byte/body spans, line/body-line ranges, Markdown span metadata where available, and direct `source-read`, body-window, and `symbol-read` expansion commands.
- Markdown representative spans additionally include `markdown-ast --node <span-*>` so a search result can jump directly to the selected Markdown projection node.
- Human search-budget output now prints an `ast ...` navigation line when a preview symbol has span data.
- Added focused unit coverage for generic Rust AST span artifacts and Markdown span-to-`markdown-ast` expansion.
- Updated the spec search-preview contract to document the new artifact shape.

Verification:
- `cargo test -p tsift-cli search_budget_report_` passed.
- `make check` passed: clippy, full workspace tests, doc tests, opencode plugin tests, and npm publish dry-run.
- `cargo build --workspace` passed.
- `cargo install --path . --force` passed and installed `tsift 0.1.62` to `~/.cargo/bin/tsift`.
- `cargo check -p tsift-cli` passed after reverting unrelated `Cargo.lock` drift from cargo commands.
- Live smoke check: `tsift --envelope search search_budget_report_promotes_ast_span_artifacts_for_symbols --path . --json --budget normal` emitted `report.symbols[].ast` with `span-*`, `source-read`, and `symbol-read` expansions.
- GitHub Actions: latest `CI` run `26843503622` on `main` was successful; jobs `clippy + test` and `live Convex graph acceptance` were both green. This repo has no `tmux-test` workflow/job or `make tmux-ci` target; deterministic SimWorld coverage ran inside `make check` and passed.

Commits:
- tsift submodule: `d12bf3f` (`feat: promote ast spans in search preview`) pushed to `btakita/tsift main`.
- parent repo: `64586add4` (`chore: update tsift submodule`) committed locally. Per the session-document closeout rule, I will push the parent branch after this `agent-doc finalize` commit lands.

Escalation: none.
Confidence: high.

### Re: #astspansearch closeout recovery — gpt-5

Closeout recovery note: the `#astspansearch` implementation response was already committed and the item was marked done; this write reopens the binary-owned document closeout after the IPC boundary/session-check drift. No additional code changes were made in this repair write. The remaining active queue head is `#astqueryfacets`.

### Re: #astqueryfacets — gpt-5

> **Queue prompt:**
>
> [#astqueryfacets]


Implemented `#astqueryfacets` and committed it in `btakita/tsift`.

What changed:
- Added repeatable `tsift search` facet flags: `--lang`, `--kind`, `--node-kind`, `--section`, `--parent`, `--child`, `--fence-language`, `--list-depth`, and `--heading-level`.
- Wired filters into indexed symbol/AST preview search, including over-fetch before facet filtering so narrowed previews can still fill the requested limit. Preserved the existing zero-limit behavior.
- Added AST context matching for parent/child handles and names, Markdown section paths/handles, fenced-code language, list depth, and heading level.
- Echoed active filters in `report.filters` and human search-budget output; file-level lexical hits remain the fallback surface.
- Updated `SPEC.md` and added CLI/report/filter tests.

Commits:
- `btakita/tsift@02b7d8e` — `feat: add ast search facet filters`
- parent submodule pointer committed as `agent-loop@4cc3b9536` — `chore: update tsift submodule`; parent push follows this binary-owned document closeout.

Verification:
- `cargo test -p tsift-cli search_` passed.
- Final-state `make check` passed: clippy, workspace tests, SimWorld, OpenCode npm tests, and npm publish dry run.
- `cargo build --workspace` passed.
- `cargo install --path . --force` passed and installed `tsift 0.1.62`.
- Installed CLI smoke passed: `tsift --envelope search search_budget_report --path . --json --budget normal --lang rust --kind function --node-kind function_item` returned `report.filters` with `languages=[rust]`, `kinds=[function]`, and `node_kinds=[function_item]`.
- GitHub Actions CI for `btakita/tsift@02b7d8e`, run `26846278352`, passed on rerun attempt 2: `clippy + test` and `live Convex graph acceptance` both succeeded. Attempt 1 had a transient live-acceptance job failure before the acceptance step while the cache/build step still had no failed-step log.
- Tmux CI evidence: the workflow has no tmux job, `rg -n "tmux" .github Makefile packages src tests` returned no matches, and `make -n tmux-ci` reports no such target.

### Re: astnavgraph — gpt-5

> **Queue prompt:**
>
> [#astnavgraph]


Done `#astnavgraph`.

Implemented AST navigation graph rows for tsift traversal: Rust and Markdown AST spans now project as `ast_span` nodes with span metadata, source expansion commands, symbol/route/import/call connections, and sibling/parent/section containment edges. I updated `SPEC.md`, the graph projection implementation, and conformance coverage for Rust AST navigation plus Markdown section/block edges.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift/packages/tsift-cli/tests/graph_db_conformance.rs`
- parent submodule pointer `src/tsift`

Commits:
- `df40709` in `src/tsift`: `feat: project ast navigation graph rows`
- `e4f48b9fb` in parent repo: `chore: update tsift submodule`

Verification:
- `cargo test -p tsift-cli traversal_graph_projects`
- `cargo test -p tsift-cli --test graph_db_conformance graph_db_cli_covers_agent_loop_workspace_fixture_rows`
- `make check`
- `cargo install --path . --force`; `tsift --version` reports `tsift 0.1.62`
- `rg -n "tmux" .github Makefile packages src tests` returned no matches, so there was no tsift `make tmux-ci` target or tmux leg to run.
- GitHub Actions CI run `26847835290` for `df40709` passed both `live Convex graph acceptance` and `clippy + test`. The only annotations were existing workflow notices for Node.js 20 action deprecation and live Convex configuration guidance.

Confidence: high.
Escalation: none.

### Re: mdembeddedcode — gpt-5

Done `#mdembeddedcode`.

Changed paths:

- `src/tsift/SPEC.md`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift` submodule pointer in the parent repo

What changed:

- Markdown fenced code blocks now act as embedded language islands for supported fence languages: Rust, Python, TypeScript/JavaScript including JSX/TSX, Kotlin, Zig, and shell aliases.
- Markdown AST span/node metadata now carries `embedded_symbols[]`, with each extracted symbol projected back onto absolute Markdown file bytes and lines.
- Search AST facets can match from Markdown prose/block context into embedded code symbols, including child facet matching such as a Markdown `code_block` found by `--child sample`.
- Traversal graph output now materializes embedded code symbols as AST span nodes and links them both directions with the containing fence and enclosing section via `contains_embedded_symbol`, `embedded_in_fence`, `contains_embedded_code`, and `enclosing_section` edges.
- The spec documents embedded fence parsing, metadata shape, search behavior, and traversal edges.

Commits:

- tsift submodule: `acf89a0 feat: link markdown embedded code symbols` pushed to `btakita/tsift main`.
- parent repo: `8905f1a60 chore: update tsift submodule` committed locally for the submodule pointer.

Verification:

- `cargo test -p tsift-cli markdown` passed.
- `cargo test -p tsift-cli traversal_graph_projects` passed.
- `make check` passed for the full local suite.
- `cargo install --path . --force` passed; installed CLI reports `tsift 0.1.62`.
- Installed CLI smoke passed for Markdown containing a Rust fence with `fn sample() {}` and `tsift --envelope search rust --kind code_block --child sample`, returning the Markdown `code_block` with embedded symbol metadata for `sample`.
- Tmux CI review: no tmux references were found under `.github`, `Makefile`, `packages`, `src`, or `tests`; `make -n tmux-ci` reports no `tmux-ci` target, so there is no repo-local tmux CI leg to inspect or run.
- GitHub Actions CI run `26848778901` passed both jobs: `clippy + test` and `live Convex graph acceptance`. The run still carries existing annotations for Node.js 20 action deprecation and for configuring `TSIFT_LIVE_CONVEX_GRAPH_URL` on a dedicated deployment.

Confidence: high.

Escalation: none.

### Re: mdembeddedcode closeout repair — gpt-5

> **Queue prompt:**
>
> [#mdembeddedcode]


Closeout repair only: recorded the missing `--done mdembeddedcode` proof after the response commit landed, so the auto queue can advance. No code changes.

### Re: astrankingmerge — gpt-5

> **Queue prompt:**
>
> [#astrankingmerge]


Done `#astrankingmerge`.

Implemented a merged AST-aware search ranking preview in `src/tsift`:

- Added `report.ranking` and `report.ranked` to search budget reports, with stable `srnk-*` handles and ranked rows for `symbol_span`, `symbol`, and capped `lexical_file` evidence.
- Merged indexed symbol score, exact-name signal, precise AST span presence, cached summary references, and AST/traversal-neighborhood proxy evidence from parent/child handles and Markdown embedded-code symbols.
- Kept lexical file hits visible as fallback evidence while weighting them below precise symbol/span matches.
- Added human preview ranking output and evidence counts for summary refs / graph neighbors.
- Added regression coverage for precise AST spans outranking broad lexical file hits, plus summary and graph evidence appearing in ranked reasons.
- Updated `SPEC.md` to document the merged ranking path and scoring evidence.

Changed paths:

- `src/tsift/SPEC.md`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift` submodule pointer in the parent repo

Commits:

- `src/tsift`: `892b717 feat: add ast-aware search ranking preview` (pushed to `btakita/tsift main`)
- parent repo: `2c05fb374 chore: update tsift submodule`

Verification:

- `cargo test -p tsift-cli search_budget_ranked_preview` passed.
- `cargo test -p tsift-cli search_budget_report` passed.
- `cargo test -p tsift-cli search` passed.
- `make check` passed, including clippy, workspace tests, doc tests, OpenCode npm tests, and npm dry-run publish check.
- `cargo install --path . --force` passed and installed `tsift 0.1.62`.
- Installed CLI smoke passed: `tsift --envelope search "build search budget report" --path packages/tsift-cli/src/lib.rs --json --budget small` emitted `report.ranked` with `symbol_span` rows above lexical hits.
- Tmux CI evidence: `rg -n "tmux" .github Makefile packages src tests` returned no matches, and `make -n tmux-ci` reports no such target.
- GitHub Actions CI run `26849880228` passed: `clippy + test` and `live Convex graph acceptance` both green. The run still has the existing annotations for Node 20 action deprecation and missing `TSIFT_LIVE_CONVEX_GRAPH_URL` configuration.

Confidence: high. The ranking behavior is covered by focused tests, the full local suite, installed-binary smoke, and green remote CI.

Escalation: none.
<!-- agent:boundary:3cbd7dee -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#astspansearch]~
- ~[#astqueryfacets]~
- ~[#astnavgraph]~
- ~[#mdembeddedcode]~
- ~[#astrankingmerge]~
- [#astnavfixtures]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#astnavfixtures] Add multilingual AST navigation fixtures and perf gates covering Rust/Python/TS/JS/Kotlin/Zig/Bash plus Markdown headings, lists, fenced code, stable handles, ranking recall, and expand-command validity.
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
