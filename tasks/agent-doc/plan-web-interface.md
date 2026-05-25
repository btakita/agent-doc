# Plan: web interface for agent-doc (`agent-doc serve`)

**Item ID:** `#adoc-web-interface`
**Class:** new feature
**Severity:** n/a (capability)

## Goal

Add a fully-featured web interface to agent-doc. MVP is a markdown editor served from the binary against a single session document, funneling all writes through the existing `agent-doc write --commit` boundary so cycle invariants (atomic write, snapshot, commit, session-check) hold uniformly.

## Constraints

- **`No async` project rule** (`src/agent-doc/CLAUDE.md`): the binary is sequential per-run; no tokio/axum/warp. Use a sync HTTP server (`tiny_http` or hand-rolled).
- **Editor IPC stays primary**; the web path is a parallel write surface, not a replacement.
- **All writes go through `agent-doc write --commit` / `finalize` / `repair` so the cycle invariants** (capture chain, snapshot, commit boundary, session-check guards) are not bypassed.
- **Static assets embedded in the binary** so `agent-doc serve` works on a clone without a separate build pipeline.

## Phases

### Phase 1 — MVP markdown editor (this PR)

**Files:** `src/serve.rs` (new), `src/main.rs` (wire subcommand), `Cargo.toml` (add `tiny_http`)

1. `agent-doc serve <FILE> [--port 7333] [--host 127.0.0.1]` opens an HTTP server bound to localhost by default.
2. Routes:
   - `GET /` → static HTML page with a `<textarea>` markdown editor + save button.
   - `GET /doc` → current document text (read from disk under the existing doc lock).
   - `POST /save` → body is the full document text; funnels through `agent-doc write --commit` (or its in-process equivalent) so the snapshot/commit boundary applies. Returns the new HEAD SHA.
   - `GET /healthz` → 200 OK for liveness.
3. Static HTML/CSS/JS embedded as `include_str!` (no build pipeline).
4. Localhost bind by default; surface a clear warning when `--host` is set to a non-loopback address (no auth in this phase).

### Phase 2 — Live updates (SSE)

1. `GET /events` Server-Sent Events stream that emits `doc-changed` when the file mutates (from editor IPC, the CLI, or another writer).
2. Frontend re-fetches `GET /doc` on the event so the textarea reflects external edits (with a "discard local edits?" prompt when the user has unsaved changes).
3. File watching uses the existing `notify` integration if present, else a polling fallback at 250–500 ms.

### Phase 3 — Better editor surface

1. Swap the `<textarea>` for CodeMirror 6 (or similar) with markdown mode, vim/emacs keybindings opt-in, line wrap, gutter.
2. Render the exchange/backlog/queue/review components in a side panel (web components consuming preflight's JSON projections).
3. Keep edits flowing through `/save` so the cycle boundary remains the source of truth.

### Phase 4 — Multi-doc + session list

1. Root mode: `agent-doc serve` (no FILE) browses all session docs under the project.
2. `/api/sessions` returns the registered session list (reuses `sessions.json`).
3. URL routing: `/doc/<path-or-hash>` for a specific doc.

### Phase 5 — Auth + remote use

1. Bearer-token auth (token printed on `serve` start) for non-loopback binds.
2. Optional TLS via `rustls`.
3. Multi-user read-only viewer mode keyed on token scope.

### Phase 6 — Hook into `agent-doc stream` for live agent output

1. SSE stream for the active cycle's `### Re:` block as it streams from the agent (`agent-doc stream` already produces CRDT-merged output).
2. Browser renders the in-flight response with the same boundary markers the editor IPC path uses.

## Out of scope (MVP)

- Multi-user collaboration (CRDT integration for browser editors is Phase 6+).
- Authentication (Phase 5; MVP is localhost-only).
- Build pipeline / npm dependencies (MVP uses raw `<textarea>` + plain JS).

## Testing

- Phase 1: integration test in `tests/serve_integration.rs` that starts `agent-doc serve` against a temp session doc, GETs `/doc`, POSTs `/save` with new content, asserts HEAD advanced + cycle reached `committed`.
- Phase 1: `cargo test serve` for unit-level handler tests.
- Manual: open `http://127.0.0.1:7333/` in a browser, edit the markdown, save, verify the change committed via `git log` and `agent-doc session-check`.

## Open questions

- Default port? Suggest 7333 (echoes "33" boundaries elsewhere in agent-doc); easily overridable.
- Editor library for Phase 3 — CodeMirror 6 vs Monaco vs Lexical. CodeMirror is lightest.
- File watching integration: the binary already has `debounce` and IPC plumbing for editor-side mutations. Reuse vs separate file-watch in `serve.rs`?
