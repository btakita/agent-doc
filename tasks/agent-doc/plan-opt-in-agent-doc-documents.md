# Plan: opt-in agent-doc documents (don't auto-convert every .md)

## Problem

agent-doc treats **every** `.md` as a potential session. The moment `route` /
`run` / `write` touches a `.md`, `frontmatter::ensure_session_for_file`
(`agent-doc-orchestration/src/frontmatter_io.rs:58` → `frontmatter::ensure_session`,
`agent-doc-core/src/frontmatter.rs:860`) injects `session: <uuid>` frontmatter,
and template-mode defaults then scaffold `agent:exchange` / `agent:queue` /
`agent:backlog` components on first write. The JB plugin enables **Run Agent Doc**
for any `.md` (`SubmitAction.update` keys only off `extension == "md"`).

Result: a plain notes/README `.md` gets silently hijacked into an agent-doc
session. The user wants plain `.md` files to coexist; agent-doc treatment should
be **opt-in**.

## Design — explicit opt-in gate

Define a single predicate, **`is_agent_doc_document(path, content, config)`**, true when ANY of:
1. Frontmatter carries an agent-doc field (`agent_doc_format`, `agent_doc_write`,
   `agent_doc_mode`, or an existing `session:`).
2. The project-relative path matches a configured glob in `.agent-doc/config.toml`
   (`[documents] include = ["tasks/**/*.md", "plan.md", ...]`).
3. Legacy escape hatch for current behavior: `auto_session_for_all_md = true` in
   `.agent-doc/config.toml` (default `false`).

Wire it as a fail-closed gate, not a silent converter:

- **Core**: add the predicate in `agent-doc-core` (pure: path + content + config).
- **`ensure_session_for_file`**: when the doc is not opted-in, return a typed
  `NotAnAgentDocDocument` error instead of injecting frontmatter.
- **`route` / `run` / `write`**: surface that error with a clear opt-in message:
  `"<file> is not an agent-doc document. Run `agent-doc init <file>` or add
  `agent_doc_format: template` to opt in."` — and do **not** mutate the file.
- **`agent-doc init <file>`** stays the explicit per-file opt-in that scaffolds.
- **FFI** (`ffi.rs`, Shared Foundation): export `agent_doc_is_session_document(path)`
  so editor plugins gate cheaply without reimplementing the logic.
- **JB plugin**: `SubmitAction.update` (and the other agent-doc actions) call the
  FFI gate so **Run Agent Doc** is hidden/disabled for non-opted-in `.md`. VS Code
  extension mirrors it.
- **watch daemon / autoclaim**: `discover_entries` only treats opted-in docs as
  sessions (autoclaim already keys off the registry, so it is largely safe).

## Backward compatibility

Existing session docs already carry `session:` / `agent_doc_format:`, so they
stay sessions. Only brand-new plain `.md` change behavior. Projects that want the
old "every md" behavior set `auto_session_for_all_md = true`.

## Tests

- core: `is_agent_doc_document` truth table (frontmatter field / glob / escape
  hatch / plain md).
- `ensure_session_for_file` returns `NotAnAgentDocDocument` for a plain `.md` and
  leaves the file byte-identical.
- route/run/write fail closed with the opt-in message and no file mutation.
- JB: `SubmitAction.update` disabled for a plain `.md`, enabled for an opted-in one
  (FFI gate test).

## Open questions for the user

- Preferred opt-in surface: per-file frontmatter, project-config glob, or both
  (recommended: both, frontmatter wins)?
- Should `init` auto-add a `[documents]` glob, or stay purely per-file?
