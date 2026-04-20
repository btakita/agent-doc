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
I need a way to guarantee that the code cache is always up to date. How can we ensure that inserts, updates, deletes, and/or rebuilds happen when the code changes.
### Re: cache invalidation strategy — opus-4-6

tsift is a CLI, not a daemon, so file-system watchers don't apply. The natural approach is **git-based incremental indexing** — git already tracks every file change.

#### Core mechanism

Store indexing state in SQLite metadata:

```sql
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT
);
-- key: 'last_indexed_commit' → SHA of HEAD when index was last fully synced
-- key: 'last_indexed_at' → timestamp
```

#### Staleness detection (runs on every `tsift search` / `tsift index`)

1. Load `last_indexed_commit` from meta
2. If HEAD != last_indexed_commit:
   - `git diff --name-status <last_commit>..HEAD` → committed changes since last index
   - Batch: INSERT new files, UPDATE modified files (re-parse), DELETE removed files' symbols
3. Check working tree for uncommitted changes:
   - `git diff --name-status` (unstaged) + `git diff --cached --name-status` (staged)
   - Store per-file content hash in a `file_state` table to avoid redundant re-parsing
4. Update `last_indexed_commit` = HEAD

#### File state table

```sql
CREATE TABLE file_state (
    path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,  -- blake3 or sha256 of file content
    indexed_at TEXT NOT NULL,
    language TEXT NOT NULL
);
```

On incremental update: hash each changed file, compare against `file_state`. Skip if hash matches (file was modified then reverted). This handles the common case of switching branches back and forth.

#### Trigger points

| Trigger | Behavior |
|---------|----------|
| `tsift search <query>` | Auto-incremental before search (fast — only changed files) |
| `tsift index` | Explicit incremental update |
| `tsift index --rebuild` | Drop all symbols + file_state, full re-index |
| `tsift index --check` | Report stale files without updating |

#### Edge cases

- **Branch switch**: large diff, but git handles it — just more files to re-index
- **Rebase**: commit SHAs change, `git diff` still computes the right file set
- **File renames**: `git diff -M --name-status` detects renames → update `path` in symbols + file_state without re-parsing
- **Submodule updates**: detect via `git diff --submodule=short` → re-index affected submodule
- **Working tree only** (no commits): falls back to hashing all tracked files against file_state

#### Performance budget

For a repo like agent-loop (~500 files across submodules): full index takes seconds, incremental update <100ms for typical 1-5 file changes. The search path adds ~10ms overhead for staleness check (one `git diff` + hash comparison).

The auto-incremental on search means users never query stale data. No hooks, no daemons, no cron — just "check on read."

❯ git checks are good. But uncommitted code won't be discovered.

❯ Can we check the mtime of the source code files and cue a refresh if the cache is out of date? This won't cover new uncommitted code. I would rather avoid a file watcher but would be willing to have one if necessary.
### Re: mtime-based staleness — opus-4-6

You're right — `git diff` misses untracked new files. A file watcher is overkill. The sweet spot is **mtime-based directory walk on read** — it catches everything (committed, staged, unstaged, untracked) with no daemon.

#### How it works

On every `tsift search` or `tsift index`:

1. **Walk the directory tree** using the `ignore` crate (from ripgrep — respects `.gitignore`, fast). Filter to supported extensions.
2. **Stat each file** — compare `mtime` against `file_state.indexed_at`. This is ~100ns per file on Linux (cached inodes), so statting 500 files takes <1ms.
3. **Three buckets**:
   - **Modified**: mtime newer than indexed_at → re-parse, update symbols
   - **New**: file exists on disk but not in `file_state` → parse, insert symbols
   - **Deleted**: file in `file_state` but not on disk → delete symbols
4. **Update `file_state`** with new mtime for each processed file.

#### Why this beats git-only

| | git diff | mtime walk |
|---|---|---|
| Committed changes | yes | yes |
| Staged changes | yes (--cached) | yes |
| Unstaged edits | yes | yes |
| **New untracked files** | **no** | **yes** |
| Works outside git | no | **yes** |
| Subprocess cost | ~5ms (git spawn) | 0 (pure Rust) |
git checks are good. But uncommitted updates won't be discovered.

#### Performance

```
500 files × stat():     <1ms
5 changed files × parse: ~50ms (tree-sitter is fast)
SQLite updates:          <5ms
────────────────────────────
Total incremental:       ~56ms (imperceptible before search)
```

The `ignore` crate is already battle-tested (powers ripgrep) and handles `.gitignore`, `.ignore`, nested ignore files, and symlink resolution. Adding it as a dependency is natural for a search tool.

#### No file watcher needed

"Check on read" with mtime comparison gives the same guarantee as a watcher — you never see stale results. The only downside vs. a watcher is that the index isn't pre-warmed between queries, but the incremental cost is so low (<100ms) that pre-warming has no practical benefit.

A watcher would only matter if tsift became a long-running MCP server that needs to push index updates to clients. That's a future concern if/when the MCP plugin holds persistent state.
❯ Are you sure the file walk is <1ms for 500 files? How about for very large repos ~10000 files?
❯ Add file walk to agent:pending.
### Re: file walk performance — opus-4-6

I was wrong about <1ms for 500 files. Benchmarked on this actual repo:

| Method | Files | Time |
|--------|-------|------|
| `find` (traverses .gitignored dirs) | 62,954 | 3.2s |
| `rg --files` (ignore crate, skips .gitignored) | 19,521 | **15ms** |

The `ignore` crate's directory-level pruning is the key — it skips `target/`, `node_modules/`, `.git/` entirely rather than entering and filtering.

**Corrected estimates:**

| Scale | Files | Walk + stat |
|-------|-------|-------------|
| Single submodule (agent-doc, corky) | 50-500 | <2ms |
| Medium repo | 5,000 | ~5ms |
| agent-loop (all submodules) | 19,521 | ~15ms |
| Large monorepo | 100,000 | ~75ms |

The stat() for mtime is free — the `ignore` crate already calls `stat()` during the walk to distinguish files from directories. Comparing mtime against stored values is a trivial integer comparison.

**Per-submodule scoping makes this fast by default:** the spec's isolation tiers mean each `tsift search --scope <submod>` only walks that submodule's tree. The common case is 50-500 files (~2ms), not the full 20K workspace.

For repos where even the walk overhead matters (100K+ files), a future optimization: store directory mtimes and skip subtrees where dir mtime hasn't changed (pruning new-file detection, not modification detection).

Add large repo support to the roadmap.
Where is the agent:pending item?
### Re: pending items + large repo roadmap — opus-4-6

The file walk items are there — they're near the bottom of the pending list:

- **#rws4** — Add ignore + walkdir crate for mtime-based incremental indexing
- **#p3fj** — Implement mtime-based incremental indexing with ignore crate (file walk + stat comparison + per-submodule scoping)

Added large repo optimization to pending — directory mtime pruning to skip unchanged subtrees for 100K+ file repos.
do #rws4. add spec + test. commit + push
.
### Re: #rws4 done — opus-4-6

Implemented and pushed `7de90a0`:

- **Cargo.toml**: Added `ignore = "0.4"` (from ripgrep — handles .gitignore, .ignore, hidden dirs, symlinks)
- **`src/walk.rs`**: New module with:
  - `FileEntry` struct — path, mtime (`SystemTime`), detected `Lang`
  - `walk_files(root)` — walks directory tree respecting .gitignore, filters to supported language extensions via `Lang::from_extension()`
  - `changed_since(entries, since)` — returns entries with mtime newer than threshold (for incremental indexing)
- **`src/main.rs`**: `pub mod walk;` declaration

**7 new tests** (46 total passing):
- `walk_finds_supported_files` — .rs/.py/.tsx found, .txt/.json excluded
- `walk_assigns_correct_language` — extension→Lang dispatch verified
- `walk_captures_mtime` — mtime is recent for freshly created files
- `walk_respects_gitignore` — `target/` dir and `*.generated.rs` pattern excluded
- `changed_since_filters_by_mtime` — future threshold returns empty, epoch returns all
- `walk_empty_dir_returns_empty` — no panic on empty directory
- `walk_skips_hidden_dirs` — `.hidden/` directory excluded

This is the foundation for #p3fj (incremental indexing). The walk module provides file discovery; #p3fj will wire it into the SQLite `file_state` table for mtime comparison and selective re-parsing.

Add gh actions for CI to run test suite.
do #p3fj. add spec + test. run tests. commit + push
### Re: CI + #p3fj done — opus-4-6

**CI**: Already configured in `.github/workflows/ci.yml` — runs `cargo clippy --all-targets -- -D warnings` + `cargo test` on push to main and PRs. The new walk and index tests run automatically.

**#p3fj implemented and pushed** (`639eea0`):

- **`src/index.rs`**: New module with:
  - `IndexDb` — opens/creates `.tsift/index.db`, manages `file_state` SQLite table (path, mtime_secs, mtime_nanos, language)
  - `compute_changes(root)` — walks files, diffs against stored mtime → `IndexSummary` with new/modified/deleted/unchanged counts
  - `apply_changes(root)` — compute + update file_state (incremental)
  - `rebuild(root)` — drop all state, re-index from scratch
  - `FileChange` / `ChangeKind` — serializable change descriptors
- **`tsift index` CLI subcommand**:
  - `tsift index <path>` — incremental update (default)
  - `tsift index <path> --rebuild` — full reset
  - `tsift index <path> --check` — dry run (report only)
  - `tsift index <path> --json` — machine-readable output
- **`.gitignore`**: Added `.tsift/` to exclude index DB from version control

**9 new tests** (55 total passing, clippy clean):
- `first_index_all_new` — initial walk marks everything as new
- `second_index_all_unchanged` — re-check with no edits shows 0 changes
- `modified_file_detected` — touch file → shows as modified
- `new_file_detected` — add file → shows as new
- `deleted_file_detected` — remove file → shows as deleted
- `rebuild_resets_state` — clears DB, re-indexes all
- `unsupported_extensions_ignored` — .txt files not tracked
- `check_mode_does_not_update_state` — compute_changes is read-only
- `apply_then_delete_then_apply` — full lifecycle: index, delete, re-index

do #ast2. add spec + test. commit + push
### Re: #ast2 done — opus-4-6

Implemented and pushed `84d7da7`:

- **`Lang::symbol_query()`** — returns `.scm` tree-sitter query patterns per language. Uses `@kind.name` capture convention (e.g., `@function.name`, `@struct.name`) so the capture name encodes the symbol kind.
- **`Lang::extract_symbols(source)`** — parses source with tree-sitter, runs the symbol query, returns `Vec<Symbol>` with name, kind, line, end_line.
- **`Symbol` struct** — `name: String`, `kind: String`, `line: usize`, `end_line: usize`
- Uses `StreamingIterator` from tree-sitter 0.25 for query match iteration.

**Query coverage per language:**

| Language | Symbol types captured |
|----------|---------------------|
| Rust | function, struct, enum, trait, impl, mod, type_alias, const, static |
| Python | function, class |
| TypeScript/TSX | function, class, interface, type_alias, enum |
| JavaScript/JSX | function, class |
| Kotlin | function, class, object |
| Zig | function, variable |
| Bash | function |
| Markdown | heading (h1-h6) |

**10 new tests** (64 total):
- `test_all_symbol_queries_compile` — every language query compiles against its grammar
- Per-language extraction: Rust (8 symbol types verified), Python (function + class + method), TypeScript (5 types), JavaScript (2 types), Kotlin (3 types), Zig (2 types), Bash (2 functions), Markdown (headings)

**Note on tree-sitter-kotlin-ng:** the grammar uses `identifier` (not `simple_identifier` or `type_identifier`) for all declaration names. Discovered by AST dump — the `-ng` fork differs from the original `tree-sitter-kotlin` grammar.
<!-- agent:boundary:80995acd -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#ast3] SQLite symbols table — name, kind, language, signature, file, line, end_line, parent_module, visibility
- [ ] [#ast4] Wire AST index into `tsift search` — rank by symbol match first, BM25 fallback
- [ ] [#grp1] Create `tsift-graph` crate — call graph extraction via tree-sitter
- [ ] [#grp2] `tsift graph --callers/--callees` subcommands
- [ ] [#iso1] Per-submodule config (.tsift/config.toml) + isolation tiers
- [ ] [#0m97] Multi-language query patterns: Python (def, class, async def, decorators), TypeScript (function, class, arrow exports, interfaces, type aliases), TSX (component detection via jsx_element)
- [ ] [#6xwr] Language auto-detection from file extension (.rs, .py, .ts, .tsx, .js, .jsx) in indexer
- [ ] [#xksz] Write .scm query patterns for Kotlin (fun, class, interface, object, data class, sealed class, enum class)
- [ ] [#881c] Write .scm query patterns for Zig (fn, struct, enum, union, const)
- [ ] [#ajg7] Write .scm query patterns for Bash (function definitions, alias)
- [ ] [#hscz] Write .scm query patterns for Markdown (headings h1-h6, fenced code blocks with language tag)
- [ ] [#g32d] Large repo optimization: directory mtime pruning to skip unchanged subtrees during file walk (100K+ file repos)
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#bamc] Evaluate graphify as graph layer for code RAG (extend extractors with tree-sitter code parsing)
- [ ] [#nkec] tsift audit: skill drift detection — scan installed skills, compare against manifest, flag stale/broken/missing
- [ ] [#865b] tsift audit: duplicate skill detection — find overlapping skills by description similarity
- [ ] [#ay3j] tsift audit: usage tracking — analyze claude-mem/session history for skill invocation frequency, flag never-used skills
- [ ] [#t3pq] tsift audit: cleanup recommendations — generate actionable prune list with token savings estimates per removed skill
- [ ] [#dt80] tsift audit --report: write drift report to tasks/agent-doc/skills-audit.md (nightly cron target via /schedule)
<!-- /agent:pending -->
