# Plan — external done archive for `agent:done`

**Status:** Draft

## Problem

`agent:done` keeps completed/reaped backlog and icebox items visible after they
leave active tracked work. That is useful for auditability, but long-lived
documents can accumulate a large completed-work archive and make the active
session document noisy.

The system needs an opt-in way to keep completed entries in a sibling
`*.done.md` file while preserving the current invariants:

- active backlog and icebox items stay in the session document
- completed work remains grep-visible in the repo
- closeout, preflight, repair, and `session-check` can still prove an id was
  intentionally resolved
- externalization is explicit in the document instead of hidden in config

## Proposed Attribute

Use `archive=` on the `agent:done` opening marker:

```md
<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
```

Rationale:

- `archive` names the role of the external file, not just its storage type.
- It is scoped by the component name: `agent:done archive=...` reads as
  "completed-work archive lives here."
- It avoids overloading generic names such as `file` or `src`, which could mean
  source input, output target, include source, or current document path on other
  components.
- It matches existing marker grammar: space-separated `key=value` attributes
  parsed by `component::parse_attrs`.

## Semantics

- If `agent:done` has no `archive=` attribute, behavior stays unchanged: reaped
  entries append inside the local component.
- If `archive=` is present, reaped entries append to the referenced markdown
  file instead of growing the local component body.
- The local `agent:done` component should remain present as the routing marker.
  Its body should be empty or contain only a short generated pointer/comment;
  it must not carry the full completed archive.
- Paths are repo-relative by default. Absolute paths should fail closed unless
  there is already a documented project-local path policy that safely accepts
  them.
- The target file must end with `.done.md`.
- The target file must live inside the current repo.
- Missing target files may be created by the binary with a small header and an
  `agent:done-archive` marker or plain append-only markdown section, depending
  on which format is easiest to make deterministic.
- If the external append succeeds but the session document/snapshot update
  fails, the command must fail closed with enough diagnostics to repair without
  pretending the cycle completed.

## Write Shape

Prefer one entry per completed item, matching the existing inline archive
format:

```md
- 2026-05-13 [#abcd] completed item text
```

The first implementation should not compact or reorder the external file. Keep
it append-only except for deterministic duplicate suppression when retrying the
same failed closeout.

## Implementation Plan

1. Extend done-component resolution with a helper such as
   `done_archive_target(doc, component) -> Option<PathBuf>`.
2. Validate `archive=` during preflight/repair/write/finalize paths that can
   reap completed work.
3. Route `pending::archive_reaped_items` through a sink abstraction:
   inline component body or external file append.
4. Ensure snapshot/commit state records the same resolved ids whether the done
   archive was inline or external.
5. Make retries idempotent by suppressing duplicate external entries for the
   same date/id/text tuple during the active cycle.
6. Add migration/runbook guidance showing the manual conversion from inline
   archive to external archive:
   - move current `agent:done` body to `same-stem.done.md`
   - add `archive=relative/path.done.md`
   - leave the local component present and mostly empty
7. Update specs and generated instructions to document `archive=`.

## Tests

- `agent:done` without `archive=` still appends inline.
- `agent:done archive=tasks/example.done.md` appends reaped items externally
  and leaves the local component small.
- Missing `*.done.md` target is created deterministically.
- Non-`.done.md`, absolute, parent-directory escape, and outside-repo paths fail
  closed before mutating active tracked work.
- `session-check` recognizes ids archived externally as resolved.
- Retry after partial failure does not duplicate external done entries.
- `agent-doc migrate` preserves unknown safe attrs on `agent:done`, including
  `archive=`.

## Open Questions

1. Should the external file itself use a component marker, or should it remain a
   plain append-only markdown log?
2. Should `agent-doc claim` offer an option to scaffold the sibling
   `*.done.md` file, or should this remain manual opt-in?
3. Should `agent-doc compact` ignore external done archives, or include a count
   and path in compact summaries?

## Exit Criteria

1. The agreed attribute name is documented as `archive=`.
2. Completed-work reaping works with both inline and external archives.
3. Closeout guards and history replay treat external done entries as first-class
   proof that tracked ids were resolved.
4. The active session document can keep an empty `agent:done` marker while the
   sibling `*.done.md` file stores the full completed archive.
