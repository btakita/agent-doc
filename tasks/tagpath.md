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
