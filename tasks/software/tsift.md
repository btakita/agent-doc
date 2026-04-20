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
What would supporting any tree-sitter programming language look like? Would it mean the binary would have all of the languages? Can we have a plugin architecture instead? Would a plugin architecture mean support a separate build via gh actions for each language? What would be the lowest impact + modular way to approach this distribution architecture?
### Re: distribution architecture for multi-language support — opus-4-6

Three approaches, from simplest to most modular:

#### 1. Cargo features (lowest impact — recommended start)

Each grammar is a compile-time feature flag:

```toml
[features]
default = ["lang-rust", "lang-python", "lang-typescript"]
lang-rust = ["dep:tree-sitter-rust"]
lang-python = ["dep:tree-sitter-python"]
lang-typescript = ["dep:tree-sitter-typescript"]
lang-javascript = ["dep:tree-sitter-javascript"]
lang-go = ["dep:tree-sitter-go"]
all-languages = ["lang-rust", "lang-python", "lang-typescript", "lang-javascript", "lang-go"]
```

- **Binary size**: ~200KB-1MB per grammar. Even 20 languages stays under 20MB.
- **GH Actions**: single workflow, one "full" binary with `--all-features`. No per-language matrix needed.
- **User choice**: `cargo install tsift` gets defaults (Rust/Python/TS). `cargo install tsift --all-features` gets everything. `--no-default-features --features lang-python` for minimal.
- **Adding a language**: grammar crate dep + feature flag + `.scm` query file + `Language` enum variant. No architecture changes.

#### 2. Dynamic grammar loading (true plugin architecture)

tree-sitter supports loading `.so`/`.dylib` grammars at runtime via `Language::from_path()`:

```bash
tsift lang install python    # downloads prebuilt grammar .so
tsift lang list              # shows installed grammars
tsift lang remove go         # cleanup
```

- **Pro**: tiny core binary (~2MB), add languages without recompiling
- **Con**: platform-specific shared libs (linux/mac/windows x86/arm), separate grammar build infra, `tsift lang install` needs a grammar registry or GitHub releases per grammar
- **GH Actions**: needs a matrix build (per-language x per-platform) for grammar .so files

#### 3. Hybrid (pragmatic long-term)

Ship common languages compiled in (Cargo features), dynamic loading for extras. Users get Rust/Python/TS/JS out of the box; `tsift lang install haskell` for niche languages without rebuilding.

**Recommendation**: Start with **Cargo features**. It's one Cargo.toml change, zero runtime infrastructure, and GH Actions builds exactly one binary. The dynamic loading path is a natural evolution if/when the grammar count makes binary size a concern (probably 30+ languages). The feature-flag design is forward-compatible — the `Language` enum and query files work identically regardless of how the grammar is loaded.

❯ Create a spec or plan for this architectural evolution. Support languages that I use right now.
❯ - Rust
❯ - Python
❯ - Typescript
❯ - Javascript
❯ - Kotlin
❯ - Zig
❯ - Bash/Zsh/Shell
❯ - Markdown
### Re: multi-language spec — opus-4-6

Updated `src/tsift/SPEC.md` with a full "Multi-Language Architecture" section. All 8 grammar crates confirmed on crates.io — no git deps needed:

| Language | Crate | Version |
|----------|-------|---------|
| Rust | `tree-sitter-rust` | crates.io |
| Python | `tree-sitter-python` | crates.io |
| TypeScript/TSX | `tree-sitter-typescript` | crates.io |
| JavaScript/JSX | `tree-sitter-javascript` | crates.io |
| Kotlin | `tree-sitter-kotlin` | 0.3.8 (community, MIT) |
| Zig | `tree-sitter-zig` | 1.1.2 (tree-sitter-grammars org) |
| Bash/Zsh/Shell | `tree-sitter-bash` | 0.25.1 (official tree-sitter org) |
| Markdown | `tree-sitter-md` | 0.5.3 (two-grammar split: block + inline) |

The spec covers: Cargo feature flags (all 8 default-enabled), `Language` enum with `#[cfg(feature)]` gating, per-language symbol extraction tables, SQLite schema with `language` column, module structure (`src/lang/*.rs`), and future dynamic loading path.

Development phases updated: Rust + Python queries first, then remaining languages, then graph + federation.

do #ast1 with spec and tests. commit + push
### Re: #ast1 done — opus-4-6

Implemented and pushed `83bf239`:

- **Cargo.toml**: `tree-sitter` 0.25 core + 8 grammar crates as optional deps behind `lang-*` feature flags (all default-enabled)
- **Grammar crates**: `tree-sitter-rust`, `tree-sitter-python`, `tree-sitter-typescript`, `tree-sitter-javascript`, `tree-sitter-kotlin-ng` (used `-ng` — original `tree-sitter-kotlin` 0.3.x pulls tree-sitter 0.20, ABI-incompatible), `tree-sitter-zig`, `tree-sitter-bash`, `tree-sitter-md`
- **`src/lang/mod.rs`**: `Lang` enum with `#[cfg(feature)]` gating, `from_extension()` dispatch (17 extensions), `tree_sitter_language()` grammar loading, `name()`, `all()`
- **12 new tests**: grammar-creates-parser for all 10 variants, extension dispatch coverage, unknown extension returns None, plus per-language snippet parsing (Rust, Python, TypeScript, TSX, JavaScript, Kotlin, Zig, Bash, Markdown)
- **SPEC.md**: full multi-language architecture section

All 39 tests pass, clippy clean. Single `tree-sitter-language v0.1.7` ABI — no version splits.
<!-- agent:boundary:1dc3f24e -->
<!-- agent:boundary:d56c51ec -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] [#ast1] Add tree-sitter core + 8 grammar crates behind Cargo feature flags (rust, python, typescript, javascript, kotlin, zig, bash, markdown)
- [ ] [#ast2] Implement Language enum with extension dispatch + per-language .scm query patterns
- [ ] [#ast3] SQLite symbols table — name, kind, language, signature, file, line, end_line, parent_module, visibility
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
- [ ] [#xksz] Write .scm query patterns for Kotlin (fun, class, interface, object, data class, sealed class, enum class)
- [ ] [#881c] Write .scm query patterns for Zig (fn, struct, enum, union, const)
- [ ] [#ajg7] Write .scm query patterns for Bash (function definitions, alias)
- [ ] [#hscz] Write .scm query patterns for Markdown (headings h1-h6, fenced code blocks with language tag)
<!-- /agent:pending -->
