---
session: 4602f467-ef2a-4094-a750-5eb533368988
resume: null
agent: null
model: null
branch: null
---

## Project Summary

**tagpath** — Tag Path naming system. Parse, lint, and search tag-based identifiers across languages.

- **Repo:** https://github.com/btakita/tagpath (submodule in agent-loop, symlinked from `~/work/btakita/tagpath`)
- **Crate:** [`tagpath` v0.1.0](https://crates.io/crates/tagpath)
- **Naming:** Chosen "Tag Path" over Tag Vector, Graph Tag, Semantic Tag, DAG Tag
- **Config format:** TOML (`.naming.toml`)
- **39 language presets:** C, C++, C#, Clojure, Common Lisp, Crystal, CSS, D, Dart, Elixir, Erlang, F#, Gleam, Go, Haskell, Java, JavaScript, Julia, Kotlin, Lua, Nim, Objective-C, OCaml, Odin, Perl, PHP, Python, R, Racket, Ruby, Rust, Scala, Scheme, Shell, SQL, Swift, TypeScript, V, Zig
- **Docs:** README.md, AGENTS.md (symlinked as CLAUDE.md), SPEC.md

### Key Concepts

- Every identifier is a **path** through an ordered sequence of **tags**
- `personName` (camelCase) = `person_name` (snake_case) = `PersonName` (PascalCase) → canonical `[person, name]`
- `__` separates namespace dimensions: `auth0__user__validate` → 3 dimensions
- Role detection: `create_*` (factory), `use_*` (hook), `set_*` (setter), `is_*` (predicate)
- Shape detection: `*_a` (array), `*_r` (record), `*_m` (map), `*$` (signal)
- Per-context convention mapping in `.naming.toml` (e.g., Zig: camelCase functions, snake_case variables, PascalCase types)
- English prose as a variant (planned): tags ↔ natural language translation

## Phases

### Phase 1 — Parse, detect, semantic equivalence, CLI ✅

- [x] Config schema (`.naming.toml` with `extends`)
- [x] Name parser — convention detection, tokenization, tag decomposition
- [x] `tagpath parse` — CLI command with text/JSON output
- [x] `tagpath init` — generate config from language/convention preset
- [x] Language presets — 39 languages (far exceeded the original 3)
- [x] Convention presets — `immutable-tag.toml`
- [x] 12 tests passing
- [x] YAML → TOML migration complete

### Phase 2 — tree-sitter integration, lint, extract

- [ ] tree-sitter integration — parse source files to extract identifiers
- [ ] `tagpath lint` — validate identifiers against `.naming.toml` rules
- [ ] `tagpath extract` — list all identifiers from source files
- [ ] Config inheritance — resolve `extends` chains
- [ ] Fix dead_code warnings (NamingConfig structs unused until lint loads configs)

### Phase 3 — Graph building, search, alias, prose

- [ ] `tagpath graph` — build tag relationship graph from codebase (petgraph)
- [ ] `tagpath search` — semantic tag search across codebase
- [ ] `tagpath alias` — convention translation (`snake_case ↔ camelCase ↔ PascalCase`)
- [ ] Prose convention — translate between tags and natural language
- [ ] Publish to crates.io, PyPI wrapper, GitHub Releases

## Assistant

**Phase 1 is complete.** All original goals met and exceeded:

- Parser works with 5 conventions + mixed convention support
- 12 tests covering detection, tokenization, semantic equivalence, roles, shapes, namespaces
- 39 language presets (originally planned 3)
- TOML config format (migrated from YAML)
- CLI with `parse`, `init`, and stub `lint` commands

Document compacted — removed the iterative design/naming discussions (preserved as git history). Ready for Phase 2 when you want to start.

## User

commit, version bump, and release

## Assistant

Released **tagpath v0.1.0**:

- **crates.io:** https://crates.io/crates/tagpath (`cargo install tagpath`)
- **GitHub Release:** https://github.com/btakita/tagpath/releases/tag/v0.1.0
- **Git tag:** `v0.1.0` pushed

12/12 tests pass. Published all three targets (crates.io, GitHub Release, git tag).

## User

Phase 2 looks good. What is the utility of tree-sitter integration?
Can we do a search for a "tag" across different languages.
Such as Python <-> Typescript.

## Assistant

### Tree-sitter utility

Tree-sitter gives **structured AST parsing** — it distinguishes function names, variables, types, imports, and string literals. Without it, `tagpath lint` can't know whether `user_name` is a variable (should be snake_case in Python) or a class (should be PascalCase). Tree-sitter provides the **context** that `.naming.toml` rules need.

### Cross-language tag search — yes, the killer feature

The canonical tag list is language-agnostic:

```
Python:      validate_user_name()  → [validate, user, name]
TypeScript:  validateUserName()    → [validate, user, name]
Go:          ValidateUserName()    → [validate, user, name]
CSS:         .validate-user-name   → [validate, user, name]
SQL:         validate_user_name    → [validate, user, name]
```

`tagpath search "user_name"` finds all five — semantic search, not textual. `grep` would need 5 different patterns.

### Phase 2 split proposal

- **2a:** Regex extraction + cross-language search (fast to build, immediately useful)
- **2b:** tree-sitter for context-aware lint (heavier, enables full `.naming.toml` validation)

Cross-language search doesn't require tree-sitter — regex extraction gets 80% accuracy. Tree-sitter adds the context layer needed for lint rules.

## User

Can we also better integrate "meaning" with coding agents such as Claude Code using the tree sitter integration?

## Assistant

### Coding agents + tree-sitter = semantic code understanding

Yes — this is one of the most compelling use cases. Today, when Claude Code (or any coding agent) sees your codebase, it reads text. Tree-sitter integration with tagpath would give agents a **semantic vocabulary layer**:

**1. Intent-preserving refactoring**

An agent could say "rename the `user` concept across all files" and tagpath would find every identifier containing the `user` tag regardless of convention — `userName`, `user_name`, `UserName`, `get-user-data` — and rename them consistently per-language convention.

**2. Cross-language symbol resolution**

When an agent works on a full-stack project (Python backend + TypeScript frontend), tagpath provides a **shared semantic index**. The agent understands that `fetchUserProfile` in TypeScript and `fetch_user_profile` in Python are the same concept, enabling coherent cross-boundary changes.

**3. Convention enforcement in generated code**

When Claude Code generates new code, tagpath's `.naming.toml` tells it exactly what conventions to use per context. Instead of guessing from examples, the agent reads the config: "functions are camelCase, types are PascalCase, constants are SCREAMING_SNAKE_CASE."

**4. Semantic search for agent context**

Agents have limited context windows. `tagpath search "auth validate"` returns semantically relevant identifiers across the whole codebase — better than grep for building the right context window for a task.

### Integration model

The natural integration is as an **MCP tool** — tagpath exposes `parse`, `search`, `lint` as tools that Claude Code can call during a session. The agent uses tagpath to understand naming patterns before writing code, and to validate its output after.

This strengthens the Phase 2 split:
- **2a:** regex extraction + cross-language search (immediately useful for agents)
- **2b:** tree-sitter + lint (context-aware validation, richer agent integration)

Want to start building Phase 2a?

## User

Can I have a domain ontology like with the philosophy repo for each tag in the tag path?

Let's implement Phase 2.

## Assistant

### Tag ontology — yes, powerful idea

The philosophy repo structure maps directly to tags. Each tag in a tag path (like `user`, `validate`, `auth`) could have a definition file:

```
.naming/tags/
├── user.md        → "An authenticated identity in the system"
├── validate.md    → "Verify correctness against rules"
├── auth.md        → "Authentication and authorization boundary"
└── profile.md     → "User-facing data representation"
```

Following the philosophy pattern, each tag definition could include:

- **Ontology** — what the tag means in this domain
- **Relations** — links to related tags (`user` → `auth`, `profile`, `account`)
- **Conventions** — how this tag appears in code (`user_name`, `UserService`, `is_user_active`)
- **Examples** — concrete identifiers using this tag across the codebase

This creates a **domain vocabulary** — the codebase's own ontology. An agent reads `.naming/tags/user.md` and understands not just the naming convention but the *meaning* of `user` in this project. Two projects might both have a `user` tag but define it differently.

This fits naturally as a Phase 3 feature (graph building) since the tag definitions form a directed graph like the philosophy nodes. For Phase 2, we build the extraction and search foundation that the ontology layer will sit on top of.

### Phase 2 implementation plan

Based on the current codebase (parser, config, lint stub), here's the Phase 2 breakdown:

**Phase 2a — `tagpath extract` + cross-language search**
1. Regex-based identifier extraction from source files (walk directories, match identifiers per language)
2. `tagpath extract <path>` — list all identifiers with their parsed tag paths
3. `tagpath search <tags> <path>` — find identifiers containing given tags across all conventions
4. Config inheritance — resolve `extends` chains in `.naming.toml`

**Phase 2b — tree-sitter + lint**
5. tree-sitter integration — context-aware extraction (function vs variable vs type)
6. `tagpath lint` — validate identifiers against `.naming.toml` rules
7. Fix dead_code warnings (structs become used)

I'll start with Phase 2a. Want me to proceed, or adjust the plan first?

## User

Yes. Start with 2a. Does this include indexing search results or does that come later...like in phase 3?

## Assistant

### Indexing in Phase 2a — lightweight, scan-per-run

Phase 2a includes a **scan-time index** — when `tagpath extract` walks a directory, it builds an in-memory map of `tag → [(file, line, identifier, convention)]`. The `tagpath search` command uses this same extraction pipeline. It's computed fresh each run (no persistent cache).

What Phase 2a does **not** include:
- **Persistent index** (saved to disk for instant queries) — Phase 3
- **Graph relationships** between tags — Phase 3
- **Incremental updates** (only re-index changed files) — Phase 3

Think of Phase 2a search as `grep` but semantic — it scans on every run. Phase 3 adds `tagpath index` to build and persist a `.naming/index.json` that makes search instant and enables graph queries.

Building Phase 2a now — `extract` module + `search` module + CLI commands.

## User

Is Phase 2a complete? If yes, implement Phase 2b.

## Assistant

### Phase 2a — done

- 18/18 tests pass (12 original + 6 new)
- `tagpath extract <path> [--format text|json]` — regex-based extraction with tag decomposition
- `tagpath search <query> <path> [--format text|json]` — semantic tag search (AND semantics)
- New modules: `src/extract/mod.rs`, `src/search/mod.rs`
- New deps: `walkdir`, `regex`

### Phase 2b — building now

Adding tree-sitter integration for context-aware extraction and lint:

1. **tree-sitter deps** — `tree-sitter 0.26` + grammar crates for Rust, Python, JS, TS, Go, C, C++
2. **AST-aware extraction** — enhance extract to tag identifiers with their AST context (function, variable, type, import)
3. **`tagpath lint`** — validate identifiers against `.naming.toml` rules using AST context
4. **Config loading** — wire up `NamingConfig` for lint rules (fixes dead_code warnings)

## User

Can we have tests for cross language matching?

## Assistant

### Phase 2b — complete

**37/37 tests pass.** Phase 2b delivered:

- **tree-sitter integration** — 8 language grammars (Rust, Python, JS, TS, TSX, Go, C, C++)
- **`tagpath extract --ast`** — AST-aware extraction with context (function, variable, type, constant, import, parameter, field)
- **`tagpath lint`** — validates identifiers against `.naming.toml` context rules, suggests fixes
- **Config loading** wired up, dead_code warnings resolved

### Cross-language matching tests — added

Two new tests in `src/search/mod.rs` that prove the core thesis:

**`test_cross_language_search`** — Creates the same concept (`validate_user`) in 5 languages:
- Python: `validate_user` (snake_case)
- TypeScript: `validateUser` (camelCase)
- Go: `ValidateUser` (PascalCase)
- Rust: `validate_user` (snake_case)
- CSS: `validate-user` (kebab-case)

Searching with ANY convention (`validate_user`, `validateUser`, or `ValidateUser`) finds ALL of them. One query, all conventions.

**`test_cross_language_partial_tag_match`** — Searching for just `"user"` finds:
- Python: `create_user_profile`
- TypeScript: `fetchUserData`
- Go: `GetUserByID`
- CSS: `user-avatar`

But does NOT find `fetchPostData` (no `user` tag). Semantic search, not textual.

## User

version bump and release

Let's also setup github actions. Similar to agent-doc and corky.

## Assistant

### Released tagpath v0.2.0

- **crates.io:** https://crates.io/crates/tagpath (`cargo install tagpath`)
- **GitHub Release:** https://github.com/btakita/tagpath/releases/tag/v0.2.0
- **Git tag:** `v0.2.0` pushed
- **37/37 tests pass** — Phase 2 complete (extract, search, tree-sitter, lint, cross-language matching)

### GitHub Actions — set up

Two workflows added, matching the agent-doc/corky pattern:

**CI** (`.github/workflows/ci.yml`) — runs on push/PR to main:
- `cargo clippy -- -D warnings`
- `cargo test`

**Release** (`.github/workflows/release.yml`) — runs on `v*` tag push:
- 5-target build matrix: x86_64-linux, aarch64-linux (cross), x86_64-macOS, aarch64-macOS, x86_64-windows
- Packages binaries as `.tar.gz` (Unix) / `.zip` (Windows)
- Creates GitHub Release with all platform artifacts

Simpler than agent-doc/corky — no `instruction-files` dependency to clone, no `vendored-openssl`, no PyPI. Tree-sitter grammars compile C code via `cc` crate, which works natively on all targets and via `cross` for aarch64-linux.

CI is running now: https://github.com/btakita/tagpath/actions

## User

Does the crates release include binaries to all of the platforms? Should we have a wasm release as well?

## Assistant

### Crates.io vs GitHub Release

**Crates.io does not distribute prebuilt binaries** — it distributes source code. Users compile with `cargo install tagpath`. The **GitHub Release** is where prebuilt binaries go, and the release workflow builds for all 5 platforms. However, v0.2.0 was tagged *before* the workflow existed, so its GitHub Release only has the manually-uploaded binary. Future tags will get all 5 automatically. (We can re-tag v0.2.0 to backfill, or wait for v0.3.0.)

### WASM — yes, compelling

WASM would enable:
- **Browser playground** — try tagpath in the browser (parse, search demos)
- **npm package** — `@btakita/tagpath` for JS/TS projects
- **Editor extensions** — VS Code could embed WASM tagpath directly
- **Lightweight MCP tool** — agents embed WASM instead of spawning a binary

**The catch:** tree-sitter compiles C code via FFI. The core parser, regex extract, and search are pure Rust and compile to WASM cleanly. Tree-sitter (lint + AST extract) would need to stay native-only.

**Recommended approach:**
1. Add a `wasm` feature flag that excludes tree-sitter deps
2. Core parser + regex extract + search work in WASM (the most useful parts)
3. Tree-sitter stays native-only (lint + AST extract)
4. Add `wasm32-unknown-unknown` to release workflow
5. Publish via `wasm-pack` as npm package

This fits as Phase 2.5 or early Phase 3. Want me to implement it?

## User

That's ok for now. Not a priority. Document in future tasks.

