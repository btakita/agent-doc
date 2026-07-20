---
agent_doc_session: tsift-v0.1
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
queue: stop
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260720-044434.md`*

Compacted content:
- Archived 4 response topic(s): Compact Exchange stale-sidecar convergence; rollback-proof, self-reconnecting agent-doc closeout; rollback-proof, self-reconnecting agent-doc closeout; 1 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): session compaction; release + publish; #015t; 2 more

### Re: integrate ast-grep for search + refactor + editing — opus-4-8

Done and pushed. New `tsift ast-grep` command, backed by a new `tsift-astgrep` package.

**Why a new surface instead of extending an existing one:** `tsift search` is index-backed and answers "where is this concept." `tsift edit-intents` is symbol-resolved and answers "change this resolved symbol." Neither expresses "every place code has this *shape*" — which is where a refactor actually starts. ast-grep patterns fill that gap, and the same engine drives both search and rewrite so they can't drift apart.

**Commands**

| Command | Behavior |
|---------|----------|
| `tsift ast-grep search '<pattern>'` | path, 1-based line/column, byte range, matched text, `$A` captures |
| `tsift ast-grep rewrite '<pattern>' '<replacement>'` | codemod; **previews** unless `--apply` |
| `tsift ast-grep languages` | grammars compiled into this build |

Flags: `--path` (repeatable, default `.`), `--lang`, `--no-ignore`, `--json`, plus the standard `--max-items`/`--max-bytes`/`--budget` trio. Envelope output uses `tool: "ast-grep"`.

**Dependency fit turned out clean.** `ast-grep-core`/`ast-grep-language` 0.44 resolve to tree-sitter 0.26 and the same grammar crates tsift already compiles — I verified in a scratch crate before touching the repo. No duplicate tree-sitter, no duplicate grammar linkage. The `lang-*` features fan out to `tsift-astgrep/lang-*` the same way they already do for `tsift-graph`/`tsift-search`.

**Safety contracts, each with a test behind it**

- `rewrite` never writes without `--apply`, and `--apply` is *refused* under a preview budget — a capped scan that also wrote would land a partial codemod and report it as complete.
- Edits apply back-to-front. Front-to-back would invalidate every later byte offset by the accumulated length delta; the growing-replacement test is what proves it.
- `AstGrep::replace` only edits the first match. The multi-match test guards against that one-shot path sneaking back in.
- Identity/no-op rewrites are dropped rather than counted as completed work.
- Language resolution is gated per *compiled* grammar. `SupportLang` declares every variant unconditionally, so passing a variant whose grammar is disabled is a latent panic — the gated enum makes it an up-front failure that names the supported set.
- An explicitly named unsupported file is an error (you asserted intent about that file); one found by *walking* is skipped (a tree has many file types).

**Two limits worth knowing before you rely on it**

- **Macro bodies are opaque.** tree-sitter parses Rust macro arguments as `token_tree`, so `AstGrepLang::from_name($X)` matches a real call but *not* the same call inside `assert_eq!(...)`. I hit this during smoke testing and initially read it as a bug in my walker. It's a grammar property — patterns aimed at macro-heavy code will under-report.
- **No Zig.** `ast-grep-language` ships no Zig grammar, so `lang-zig` intentionally forwards nothing. Zig stays indexable by tsift but not structurally matchable.

**Verification**

- 16 unit tests (`tsift-astgrep`) + 12 CLI integration tests (`packages/tsift-cli/tests/ast_grep.rs`) + 1 root re-export test.
- `make check` green: exit 0, 65 suites, no failures, clippy clean.
- Live smoke on real source after `cargo install`: `$A.unwrap()` → 10 hits in `scan.rs`; `Some(AstGrepLang::$V)` → 16 hits with 8 distinct captures.
- Latest CI run on `main` was green before these commits.

**Landed:** `tsift@18e3cac`, superproject `bd00a38d3` (submodule bump + skill command-reference entry). Spec: `specs/structural-patterns.md`, indexed from `SPEC.md`; `VERSIONS.md` Unreleased updated.

One thing I did *not* do: wire structural patterns into `edit-intents` as a new intent kind. That's a real follow-up — it would let a codemod go through the existing temp-worktree `--verify` path — but it's a distinct design decision about intent schema, not a gap in this work. Say the word and I'll take it.
<!-- agent:boundary:50f347d1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
- Please integrate ast-grep to support code search + refactor + editing code.
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#astgrepstructural] Add an ast-grep structural intent kind to `tsift edit-intents` so a pattern-driven codemod runs through the existing temp-worktree `--verify` path (spec: src/tsift/specs/structural-patterns.md)
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
