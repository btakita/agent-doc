---
agent_doc_session: tsift-v0.1
resume: 765c7ea4-1760-4ae9-8ffa-b4daf4861a5b
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260730-034115.md`*

Compacted content:
- Archived 1 response topic(s): integrate ast-grep for search + refactor + editing
- Prior summary/context: prior compacted content: Archived 4 response topic(s): Compact Exchange stale-sidecar convergence; rollback-proof, se...

### Re: do [#astgrepstructural] — opus-5

Done. `tsift edit-intents` has a `structural_rewrite` intent kind, landed as `tsift@503c2df` and superproject `c2e7fe7b5`.

```json
{ "kind": "structural_rewrite", "file": "src/scan.rs",
  "pattern": "$A.unwrap()", "replacement": "$A.expect(\"scan\")" }
```

**Why this was worth a kind of its own rather than a flag on `ast-grep rewrite`.** `tsift ast-grep rewrite --apply` writes straight to the working tree — no reindex, no impact report, no temp worktree to fail in. That is the right shape for an exploratory codemod and the wrong shape for one you intend to land. Routing the same engine through the edit-intents planner means a pattern-driven codemod now earns exactly the proof a `rename_symbol` does: patch proposal, bounded diff preview, formatter policy, `expected_content_hash` conflict detection, `--verify` in a detached temp worktree with reindex plus `impact`, and batch rollback. None of that is re-specified for this kind; it is the same path.

**It is the only intent kind with no resolved symbol.** The other kinds all start from a name — an index row or a handle that already resolved to one symbol. A refactor usually starts from a *shape* instead, which is the gap the whole ast-grep surface exists to close. So the target is `file` + `pattern`, and `symbol`/`target_handle` are unused.

**Promoted across every executor at once** — Rust, Python, TypeScript, TSX, JavaScript, JSX, Markdown — because the selection and mutation logic are language-independent: the grammar is a parameter, not a code path. What *is* language-specific (parser validation, formatter policy) already lives in the executor contract. Language resolution goes through the contract `id` rather than a second parallel match, so a newly registered executor cannot advertise structural support it has no grammar for.

**Three things fail closed**, each with a test behind it:

- `pattern` is required by `structural_rewrite` and **refused on every other kind**. A pattern silently ignored by a symbol-resolved intent is not representable.
- Input *and* rewritten buffer are both reparsed with the executor grammar. The rewrite template is raw text, so that output reparse is the only thing standing between a template typo and a syntactically broken file.
- A pattern that matched nothing, and a template that reproduces every match, are **refusals** — not empty plans. The standalone `ast-grep rewrite` surface merely flags the latter `unchanged`; here a plan that cannot mutate must not be applyable.

**Verification.** 9 unit tests (multi-match, non-Rust executor, multibyte source, both degenerate refusals, output-reparse refusal, validation symmetry, and a drift guard asserting every registered executor resolves an ast-grep grammar) and 4 CLI integration tests (apply rewrites all 3 call sites and leaves the definition alone; dry run reports a diff and writes nothing; no-match refusal; `--verify` reaches `temp_applied_total=1` with the real tree byte-identical). `make check` exit 0, 0 failures, all 13 ran.

I mutation-checked rather than trusting green: dropping the no-match refusal, dropping the output reparse, and disabling the dispatch hook each failed the tests that claim to cover them — the last one failed all four integration tests, which is what tells me they reach ast-grep and are not just asserting the kind is accepted.

Live smoke on the installed 0.1.78 binary, not just the suite: `$A.unwrap()` → `$A.expect("smoke")` verified in a temp worktree with the real tree untouched, and all three refusals reproduce with their intended messages.

### Re: ast-grep as the core, and more languages — opus-5

> **Queue prompt:**
>
> do [#astgrepstructural]


> **Queue prompt:** Should we integrate ast-grep to support code search + refactor + editing code across many different languages? Should asg-grep be the core to convert code to ast or should we stick with the existing tsift ast bindings? I need to add more languages.

Taking the three parts separately, because they have different answers.

**Integrate it — done.** `tsift ast-grep search|rewrite|languages` shipped in 0.1.78, and as of this cycle it also reaches the semantic write path through `structural_rewrite` above.

**Do *not* make ast-grep the core.** Stick with the existing tsift AST bindings, for a concrete reason rather than a conservative one: `ast-grep-core` is a *matcher* over tree-sitter, not an indexer. It has no symbol extraction, no tag queries, no call edges, no persistence — everything `tsift index`, `graph`, and `explain` are built out of. Rebuilding those on top of it would mean writing the same extraction layer again with less control.

The usual argument for consolidating — "one parse, not two" — does not apply here. I checked during the ast-grep integration: `ast-grep-core` and `ast-grep-language` 0.44 resolve to **tree-sitter 0.26 and the same grammar crates tsift already compiles**. There is no duplicate tree-sitter, no duplicate grammar linkage, and no second parse to eliminate. They already share the substrate; the split is only in what each layer does with it.

So the right model is three surfaces over one grammar set, not one core: `search` answers *where is this concept* (index-backed), `edit-intents` answers *change this resolved symbol* (name-resolved), `ast-grep` answers *where does code have this shape* (pattern-matched). `structural_rewrite` is the seam where the third feeds the second.

**More languages splits into two jobs with very different costs — this is the part worth deciding before doing.**

`ast-grep-language` ships 28 grammars. tsift compiles 8 `lang-*` features today (rust, python, typescript, javascript, kotlin, zig, bash, markdown), and `zig` has no ast-grep grammar at all, so structural patterns cover 7.

- **Structural-only languages are nearly free.** One feature line plus three match arms in `tsift-astgrep/src/lang.rs` per language. That buys `ast-grep search`/`rewrite` and nothing else. The unclaimed set is roughly 19: go, java, c, cpp, c-sharp, dart, swift, ruby, php, scala, lua, elixir, haskell, nix, solidity, css, html, json, yaml, hcl.
- **Indexed languages are expensive.** `tsift-graph`/`tsift-search` need per-language tag queries and symbol extraction before a language can be searched, graphed, or made a semantic-edit executor. That is real per-language work, not a feature flag.

The cheap tier is worth taking soon and is directly useful to you: **go, cpp, c-sharp, dart, java** would give structural search and codemods across most of the lazily binding set, which is exactly the kind of "same change in nine repos" problem patterns are good at. I filed that as `#astgreplangs`.

The second tier — giving those languages semantic-edit executors so `structural_rewrite` and the symbol-resolved kinds reach them — is `#astgrepexec`, and it depends on the graph/search side, so it should not be started until the first lands.

I have not started either; they are queued in priority order, not gated. Say the word on `#astgreplangs` and I will take it — it is mostly mechanical, and the drift guard I added this cycle will catch any executor that gets registered without a grammar.
<!-- agent:boundary:5f4dbb23 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- Please extract tsift:code-navigation section into a runbook. If AGENTS.md + CLAUDE.md are used and CLAUDE.md uses `@AGENTS.md`, please do not add redundant instructions.
- release + publish
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#astgreplangs] Widen tsift-astgrep to the unclaimed ast-grep grammars, structural-only (feature line + 3 match arms each): go, cpp, c-sharp, dart, java first for lazily-binding codemods, then swift/ruby/php/scala/lua/elixir/haskell/nix/solidity/css/html/json/yaml/hcl. No tsift-graph/tsift-search work — index support is #astgrepexec's dependency, not this item's
- [ ] [#astgrepexec] Register semantic-edit executors for languages that have an ast-grep grammar but no executor (kotlin and bash today, plus whatever #astgreplangs adds) so structural_rewrite and the symbol-resolved intent kinds reach them. Blocked on per-language tag queries + symbol extraction in tsift-graph/tsift-search
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
