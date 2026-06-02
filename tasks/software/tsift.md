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
<!-- agent:boundary:331029d4 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#astspansearch] Promote AST spans to first-class search artifacts across languages: index/query symbol spans, Markdown headings/list items/code blocks, node_kind, byte/body spans, and return source-read/symbol-read/markdown-ast expand commands from search hits.
- [ ] [#astqueryfacets] Add cross-language AST query facets and filters: --lang, --kind, --node-kind, --section, --parent, --child, and Markdown-specific fence-language/list-depth/heading-level facets.
- [ ] [#astnavgraph] Project AST navigation edges into graph-db: contains, parent/child, previous/next sibling, enclosing module/section, call/import/route, and Markdown section/block containment edges with stable handles.
- [ ] [#mdembeddedcode] Treat Markdown fenced code blocks as embedded language islands: parse supported fence languages, link fence AST nodes to extracted symbols, and let search navigate from prose section to embedded code and back.
- [ ] [#astrankingmerge] Merge symbol, span, lexical, summary, and graph-neighborhood evidence into one AST-aware ranking path so file-level lexical matches do not drown out precise span hits.
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
