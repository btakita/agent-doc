---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
❯ Session established tsift as a local MIT-licensed Rust CLI replacing WozCode. Implemented: tsift search (sift library direct integration, #92bz), tsift edit batch (#mwhg), tsift sql, tsift route model-tier routing (#6jmq), CI workflow (#e15f). /tsift skill installed with TRIGGER/SKIP + runbooks/search-strategies.md. WozCode fully uninstalled (settings.json, plugins, hooks). Spinner verbs added: sifting, panning, winnowing, filtering, prospecting, sieving, distilling, unearthing, tsifting. 9 unit tests passing. Repo: btakita/tsift (private). Pending: release workflow + tsift audit subcommands (skill drift, duplicate detection, usage tracking, cleanup recommendations, nightly report).
❯ Move the spec that was transferred tasks/software/agent-loop.md into a spec file.
Note I just moved this file to tasks/software/tsift.md
### Re: spec extraction + file move — opus-4-6

Extracted the full "Spec: AST-Aware Code RAG" section to `src/tsift/SPEC.md`. The session doc now links to it instead of inlining the spec.

Acknowledged the file move to `tasks/software/tsift.md`.
We should support any tree-sitter supported programming language. I have immediate needs for python and typescript with React support.
### Re: multi-language tree-sitter support — opus-4-6

tree-sitter grammars are separate crates. For your immediate needs:

| Crate | Covers | Key extractions |
|-------|--------|-----------------|
| `tree-sitter-rust` | `.rs` | `fn`, `struct`, `enum`, `trait`, `impl` blocks |
| `tree-sitter-python` | `.py` | `def`, `async def`, `class`, decorators |
| `tree-sitter-typescript` | `.ts`, `.tsx` | `function`, `class`, `interface`, `type`, arrow exports, React components (JSX) |
| `tree-sitter-javascript` | `.js`, `.jsx` | Same as TS minus type constructs |

**Architecture for multi-language:**

1. **Language enum** — `Rust | Python | TypeScript | Tsx | JavaScript | Jsx` with auto-detection from file extension
2. **Per-language query files** — tree-sitter's `.scm` query format, one per language. These define what node types to extract as symbols (e.g., `(function_definition name: (identifier) @name)` for Python)
3. **Unified symbol table** — `symbols` SQLite table gets a `language TEXT` column; everything else (name, kind, signature, file, line) is language-agnostic
4. **Pluggable grammars** — adding a new language = grammar crate + query file + enum variant. No core changes needed

The TypeScript crate handles both `.ts` and `.tsx` via separate grammar entry points (`tree_sitter_typescript::LANGUAGE_TYPESCRIPT` vs `LANGUAGE_TSX`), so React/JSX support comes for free.

I'll update SPEC.md to reflect this. Updated pending items to replace Rust-only with multi-language approach.
We should support any tree-sitter supported programming language. I have immediate needs for python and typescript with React support.
<!-- agent:boundary:2119603a -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#ast1] Add tree-sitter + language grammars (rust, python, typescript/tsx) to Cargo.toml
- [ ] [#ast2] Implement tsift index --ast with multi-language extraction (Language enum + per-language query patterns)
- [ ] [#ast3] SQLite `symbols` table schema — name, kind, signature, file, line, parent_module
- [ ] [#ast4] Wire AST index into `tsift search` — rank by symbol match first, BM25 fallback
- [ ] [#grp1] Create `tsift-graph` crate — call graph extraction via tree-sitter
- [ ] [#grp2] `tsift graph --callers/--callees` subcommands
- [ ] [#iso1] Per-submodule config (.tsift/config.toml) + isolation tiers
- [ ] [#bamc] Evaluate graphify as graph layer for code RAG (extend extractors with tree-sitter code parsing)
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#nkec] tsift audit: skill drift detection — scan installed skills, compare against manifest, flag stale/broken/missing
- [ ] [#865b] tsift audit: duplicate skill detection — find overlapping skills by description similarity
- [ ] [#ay3j] tsift audit: usage tracking — analyze claude-mem/session history for skill invocation frequency, flag never-used skills
- [ ] [#t3pq] tsift audit: cleanup recommendations — generate actionable prune list with token savings estimates per removed skill
- [ ] [#dt80] tsift audit --report: write drift report to tasks/agent-doc/skills-audit.md (nightly cron target via /schedule)
- [ ] [#0m97] Multi-language query patterns: Python (def, class, async def, decorators), TypeScript (function, class, arrow exports, interfaces, type aliases), TSX (component detection via jsx_element)
- [ ] [#6xwr] Language auto-detection from file extension (.rs, .py, .ts, .tsx, .js, .jsx) in indexer
<!-- /agent:pending -->
