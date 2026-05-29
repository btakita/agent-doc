# Compile Baseline — `agent-doc` 3-layer architecture

Tracks the build-profile payoff of the `agent-doc-orchestration` extraction
(`#bz6s` / `#adoc-orchestration-crate`). See
`tasks/agent-doc/plan-agent-doc-orchestration-extraction.md`.

## Layering

```
agent-doc (CLI shell)  →  agent-doc-orchestration  →  agent-doc-core
  main.rs + commands       cluster/sessions/route/      pure data layer
  + ffi cdylib             git/sync/supervisor/...      (published 0.1.0)
```

## Dependency-tree footprint (after Wave 5)

| Crate | Resolved deps (incl. transitive) | Notes |
|-------|----------------------------------|-------|
| `agent-doc-core` | 93 | pure data layer; cold-builds ~9.87s / 74 crates (original `#adcr` baseline) |
| `agent-doc-orchestration` | 340 | holds the heavy tree (ureq/notify/rusqlite/interprocess/portable-pty/alacritty_terminal/tmux-router/zip/yrs/…) |
| `agent-doc` (CLI shell) | **24 direct deps** (was 36 pre-extraction) | heavy deps reach it only transitively through orchestration |

## CLI-shell incremental rebuild (the dev-loop win)

With `agent-doc-orchestration` already compiled (separate, cached compilation
unit), editing the CLI shell recompiles only the thin shell crate:

| Change | Rebuild (debug) |
|--------|-----------------|
| touch `src/main.rs` | ~1.5s |
| touch `src/lib.rs` | ~1.5s |

Pre-extraction, the same edit recompiled within the monolithic ~154K-LOC crate
(plan-recorded full build: 129s / 266 crates). The orchestration cluster
(~44K+ LOC and the entire heavy dependency tree) is now isolated behind a crate
boundary, so shell-only edits no longer pay for it.

## Acceptance criteria status (#bz6s)

1. ✅ `agent-doc-orchestration` compiles standalone on `agent-doc-core`.
2. ✅ CLI shell builds **without** the heavy deps as **direct** deps — confirmed
   via `cargo tree -p agent-doc -e no-dev --depth 1` (none of
   `rusqlite`/`alacritty_terminal`/`portable-pty`/`interprocess`/`signal-hook`/
   `tagpath`/`yrs`/`htmd`/`regex`/`serde_yaml`/`fs2` appear). They reach the
   shell only transitively through orchestration.
3. ✅ CLI-shell rebuild measured and recorded here (~1.5s incremental).
4. ✅ `make check` green across all three crates (modulo the pre-existing
   live-tmux environmental test `codex_bare_run_inside_owning_pane`).
5. ✅ `nm -D libagent_doc.so` exports the full FFI surface (39 `agent_doc_*`
   symbols); `ffi.rs` stays in the CLI shell lib.

## Method

```sh
cargo build                       # warm everything
touch src/main.rs && cargo build  # shell-only rebuild
cargo tree -p agent-doc -e no-dev --depth 1   # direct-dep audit
```
