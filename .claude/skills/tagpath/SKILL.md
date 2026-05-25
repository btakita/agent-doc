---
description: Parse, search, lint, index, and serve tag-based identifiers across naming conventions
user-invocable: true
argument-hint: "<command> [args]"
---

# tagpath

Tag Path — parse, lint, search, index, and serve tag-based identifiers across languages. Current release: **0.18.0**.

## Invocation

```
/tagpath <command> [args]
```

Run the given tagpath command via Bash. If no command is given, show this help summary.

## Commands

### parse — Decompose an identifier into canonical tags

```sh
tagpath parse <NAME> [-c <CONVENTION>] [-f text|json]
```

Detects naming convention, extracts tags, role, shape, and namespace dimensions.

```
tagpath parse createUserProfile
# Convention: camelCase
# Tags: [create, user, profile]
# Role: factory
```

### alias — Generate convention variants of an identifier

```sh
tagpath alias <NAME> [-c <CONVENTION>] [-f text|json]
```

Produces the identifier in all 6 naming conventions (or a single target via `-c`).

### prose — Human-readable description of an identifier

```sh
tagpath prose <NAME> [-f text|json]
```

Converts identifiers to natural English using role/shape detection.

### normalize-query — Turn free text into ordered weighted tags

```sh
tagpath normalize-query <QUERY> [-f text|json]
```

Feeds search/citation tools a canonical tag sequence (used by tsift envelopes).

### family — Stable semantic family for an identifier

```sh
tagpath family <NAME> [-f text|json]
```

Returns the family id + tags + handle that group all convention/role variants together.

### compression-report — Group raw symbol rows and report savings

```sh
tagpath compression-report [INPUT_JSON | -] [-f text|json] [--example-limit N]
```

Used by tsift fixtures to measure family-preview byte/token savings vs raw rows.

### search — Semantic search across naming conventions

```sh
tagpath search <QUERY> <PATH> [--index] [-f text|json]
```

Finds identifiers matching a tag query regardless of naming convention. Pass `--index` to read `.naming/index.json` directly (instant; exits 2 with a clear message if stale/missing). Searching for `create_user` also finds `createUser`, `CreateUser`, etc.

### lint — Validate identifiers and session-doc tags

```sh
tagpath lint [PATH] [--dialect identifier|agent-doc|auto] [--fs-checks] [--rule <id>] [-f text|json]
```

- **identifier** dialect (default for code): checks identifiers against `.naming.toml` context rules.
- **agent-doc** dialect: validates session-document HTML-comment tags (`agent:exchange`, `agent:done`, `patch:*`, etc.) across 14 rules including the `archive=PATH` malformed-attr trap.
- **auto**: pick per file (.md with `<!-- agent:exchange` → agent-doc; everything else → identifier).
- `--fs-checks` enables filesystem-dependent rules (`done-archive-missing-target`, `done-id-not-in-backlog`).

Exit codes: 0 clean, 1 findings present, 2 internal error.

### extract — Extract identifiers from source files

```sh
tagpath extract <PATH> [--ast] [-f text|json|family|family-json]
```

Walks source files and extracts identifiers with location, convention, role, shape. `--ast` uses tree-sitter for context-aware extraction (14 languages).

### graph — Build tag co-occurrence graph

```sh
tagpath graph [PATH] [-q <QUERY>] [-f text|dot|json]
```

Directed graph: nodes = tags, edges = sequential pairs. `-q` filters to a subgraph.

### ontology — Load and validate `.naming/tags/` markdown

```sh
tagpath ontology [PATH] [-f text|json]
```

Reads the project's `.naming/tags/*.md` domain vocabulary; surfaces in search/index/MCP outputs.

### index — Build / check / update `.naming/index.json`

```sh
tagpath index [PATH] [--check] [--force] [--update] [--force-full] [--emit json|jsonl] [--schema-version]
```

Persistent index with stable `fam:`/`mem:` handles (sha256-derived, family-stable across member adds, member-stable across line moves). Companion bincode sidecar (`.naming/index.bincache`) makes no-op `--update` ~56x faster than full rebuild.

- `tagpath index` — full rebuild.
- `tagpath index --update` — incremental: mtime fast-path + sha256 verify, falls back to full on schema/config drift. Atomic `tmp + rename`.
- `tagpath index --check` — exit 0 fresh, 1 stale with per-entry report; no write.
- `tagpath index --emit jsonl` — NDJSON stream (`header`, `source`, `family`, `member`, `footer`).
- `tagpath index --schema-version` — prints `2` for consumer feature-detection.

### watch — Long-running NDJSON file-event stream

```sh
tagpath watch [PATH] [--once] [--no-lint] [--emit-shape full|compact]
```

Watches the project, emits one JSON object per line on stdout (`hello`, `ready`, `index_update` with `changed_handles`, `lint_finding` for agent-doc dialect, `shutdown`). 150ms debounce, single-instance lock at `.naming/watch.pid`, graceful SIGINT. `--once` does one pass + exits (good for editor save hooks).

### grammars — Runtime tree-sitter grammars (opt-in feature `dyn-grammar`)

```sh
tagpath grammars list [-f text|json]
tagpath grammars check
```

Lists configured/discovered `.so`/`.dylib`/`.dll` grammars and verifies they load (ABI check). Configure via `[grammars]` in `.naming.toml`. Requires `cargo install tagpath --features dyn-grammar`.

### mcp — Stdio JSON-RPC 2.0 server + install helpers

```sh
tagpath mcp                                     # start the stdio server
tagpath mcp install --list                      # show 5 known harnesses + config paths
tagpath mcp install --print <harness>           # emit config JSON/TOML to stdout
tagpath mcp install --apply <harness> --yes     # write to the harness's config (deep-merge)
tagpath mcp install --uninstall <harness> --yes # idempotent remove
```

Harnesses: `claude-desktop`, `claude-code`, `codex`, `opencode`, `cursor`. Project-scoped via `--project <path>` for Claude Code and Cursor; binary override via `--binary <path>`.

The server exposes **9 tools**: `parse`, `normalize_query`, `lint`, `search`, `ontology_lookup`, `indexed_project_query`, `family_by_path`, `lint_session_doc`, `index_handle`.

### init — Initialize `.naming.toml` config

```sh
tagpath init [-l <LANG>] [-p <PRESET>]
```

Generates a `.naming.toml` from a language preset (39 languages) and/or convention preset.

## Workflow

1. Run the user's command via Bash: `tagpath <command> [args]`.
2. Present the output to the user.
3. If `tagpath` is missing, suggest `cargo install tagpath`. For dynamic grammar loading add `--features dyn-grammar`. For wasm use the `@btakita/tagpath-wasm` npm package.

## Key Concepts

- Every identifier is a **path** through an ordered sequence of **tags**
- `personName` = `person_name` = `PersonName` = `person-name` → canonical `[person, name]`
- `__` separates namespace dimensions: `auth0__user__validate` → 3 dimensions
- Role detection: `create_*` (factory), `use_*` (hook), `set_*` (setter), `is_*` (predicate)
- Shape detection: `*_a` (array), `*_r` (record), `*_m` (map), `*$` (signal)
- 6 conventions: snake_case, camelCase, PascalCase, kebab-case, UPPER_SNAKE_CASE, Ada_Case
- Index handles: `fam:<sha256[0..16]>` (stable across member adds), `mem:<sha256[0..16]>` (stable across line moves, breaks on rename) — see SPEC §15

## Use Cases

- **Cross-tool symbol graph** — tsift (and other consumers) read `.naming/index.json` and cite `fam:`/`mem:` handles in envelopes. Run `tagpath index --update` before queries; subscribe to `tagpath watch` for live updates.
- **Session-doc tag enforcement** — agent-doc finalize calls `tagpath lint --dialect agent-doc` automatically; catches malformed `<!-- agent:done archive PATH -->` directives before commit.
- **MCP integration** — one command (`tagpath mcp install --apply claude-desktop --yes`) wires the 9 tools into Claude Desktop / Claude Code / Codex / OpenCode / Cursor.
- **Rename refactoring** — use `alias` (today) or the upcoming `tagpath rename` to find/rewrite all convention variants of an identifier.
- **Convention enforcement** — `tagpath lint` with `.naming.toml` for code; `--dialect agent-doc` for markdown sessions.
- **Cross-language search** — `tagpath search <query> <path> --index` for instant, index-backed results.
- **Architecture analysis** — `tagpath graph` for tag relationships; `tagpath family` for stable semantic groupings.
