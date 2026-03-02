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
- **Crate:** `tagpath` (not yet published to crates.io)
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

