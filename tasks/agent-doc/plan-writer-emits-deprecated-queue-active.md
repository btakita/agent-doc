# Plan: writer emits deprecated `queue_active:` instead of canonical `queue: start/stop`

**Source:** agent-doc-bugs2.md backlog `#writer-emits-deprecated-queue-active-false`.

## Problem

The frontmatter writer still emits the deprecated `queue_active: true/false`
form instead of the canonical `queue: start/stop`. Both parse, so this is a
canonicalization debt, not a functional break.

## Prior attempt (2026-06-05, reverted)

A `write()`-level migrate-on-write in `frontmatter.rs` was tried and reverted. It
reaches ONLY the parse+rewrite paths (`set_session_id`, `set_pipeline_state`,
`merge_queue_state` — confirmed working + tested). It does NOT reach:

- `finalize`
- preflight queue-clear
- closeout recovery
- `codex_hook` Stop-repair

…which PRESERVE the frontmatter line verbatim (component-patch) or set
`queue_active` directly (`preflight.rs` ~3695/3827), bypassing `write()`.

## Scope (why this is gated, not unattended)

`queue_active:` is emitted/handled across 10+ modules (`preflight.rs`, `git.rs`,
`closeout.rs`, `repair.rs`, `queue.rs`, `queue_continuation.rs`, `start.rs`,
`ipc_socket.rs`, `session_accretion.rs`, plus tests). A complete fix is one of:

1. **Commit-boundary normalization** — the preflight commit rewrites
   `queue_active: X` → `queue: start/stop` once, centrally (preferred: single
   site, all writers flow through it).
2. **Migrate each verbatim/direct-set site** — more surface, more risk.

Either way, ~15 deprecated-form test assertions across
`run_integration` / `finalize_integration` / `preflight` / `closeout` /
`codex_hook` must move to the canonical form (only for the paths actually
migrated, to avoid asserting a form a path doesn't emit).

This touches the **commit/finalize boundary** — the same path currently fragile
under live editor-buffer races — and was already reverted once. It must land in
an **attended pass**: implement option 1, run the full suite, and confirm no
deprecated-form assertion is left asserting a now-canonical path (and vice
versa). Not appropriate for an unattended queue drain.

## Verification

- Writer emits `queue: start/stop` from all closeout paths (finalize, preflight
  queue-clear, closeout recovery, codex_hook Stop-repair), not just `write()`.
- `queue_active:` no longer appears in freshly written frontmatter.
- `make check` green with the ~15 assertions migrated to canonical form.
