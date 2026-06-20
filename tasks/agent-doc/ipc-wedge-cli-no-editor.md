# #agent-doc-bug — Pure-CLI session wedges every finalize on editor-IPC `no_ack`

**Filed:** 2026-06-20 (live repro). Standalone report — the canonical `agent-doc-bugs2.md`
could not be written from this session (foreign-owned by pane %125; see "Capture blocked" below).
Fold this into `agent-doc-bugs2.md` backlog from the owning pane (%125), or `agent-doc claim` it first.

## Summary

A pure-CLI `agent-doc` session (Claude Code pane, **no live JetBrains editor**) wedges on
**every** `finalize`: the editor-IPC write returns `ack status: error`, the file-IPC fallback
writes a patch that no editor ever applies (`no_ack`), and the cycle fail-closes with
`refusing direct disk write … editor IPC listener active`. Only `finalize --force-disk`
succeeds. It never self-heals.

## Live repro (this session)

- Doc: `tasks/professional/equityfundingsource.md`, driven from Claude Code **pane %158**.
- **No JetBrains IDE process running** (`ps` for idea/jetbrains/java → none).
- Sole agent-doc daemon: the controller — `agent-doc controller serve`, **pid 2014438**, binary
  **0.34.27** (matches the finalize client; mtime 05:41:00, controller bootstrap 05:41:06 → NOT stale-binary).
- `admin list`: the doc actor is `pane=%158 (alive) … ready` with an active editor-IPC listener,
  but there is **no editor endpoint behind it** (no plugin attached in a CLI-only session).

Per-cycle symptom (3 cycles this session, all identical):
```
[write] socket IPC failed: IPC ack status error: {"type":"ack","status":"error"} — falling back to file IPC
[write] IPC timeout (2s) — leaving patch for editor retry; refusing direct document write
[write] IPC proof insufficient … source=file_ipc … invariant=no_ack recovery=retry_without_disk_write
Error: editor IPC did not prove the write … refusing direct document write
```
Recovery each time: `finalize --force-disk` → `Stream patches applied … (CRDT)` → committed +
`session-check` ok. Commits `3b16bae56`, `3d6d89b57`, `5f4f7bafc` all landed correctly.
Granular `write --pending-add` also intermittently failed first with `reason=send_failed` before succeeding.

## Why it never self-heals

The actor is **not orphaned** (pane alive) and the binary is **not stale** (controller on 0.34.27),
so none of the existing recovery triggers fire: not M1b orphan-reap (#ipc-drift), not drain-lease
expiry (#supselfheal), not stale-binary recycle (#wd40 / #staleloop). The disk-write guard stays
armed for the whole session with no editor behind the listener.

## Capture blocked (further evidence)

Attempting to file this very bug into `agent-doc-bugs2.md` from pane %158 failed two ways:
1. controller-mediated `write --pending-add` → `refused direct disk write … editor IPC listener active (reason=send_failed)` (the same wedge).
2. `write --force-disk` → `pane ownership mismatch: session b26b9957… owned by pane %125, current pane is %158. Use agent-doc claim to reclaim.`

So the IPC wedge plus the foreign-ownership guard together **prevent capturing the IPC wedge via the
normal path** — hence this standalone file.

## Side effects observed live

- **#rt83 flood**: `ops.log` shows `queue_dispatch_skipped file=…/agent-doc-bugs2.md harness=claude
  reason=queue_control_paused` at **~2/sec for hours** (frontmatter `queue: start` vs controller
  `queue_control_paused` mismatch driving a busy-loop). Plus an orphaned
  `.agent-doc/patches/save-document.signal` patch for that doc.
- **6 orphaned dead-pane bindings** (`admin detect`): spoton, spoton2, virtuousai,
  monsterrodholders, lazily-rs, speeding-ticket actors `ready` with dead panes.

## Relationship to existing items

Extends **#cyh0 / #fcczero** (zero session-doc disk writes / disk-write guard),
**#wd40** (stale-binary auto-recycle gap), **#rt83** (dispatch flood), **#editorbufwin** (content_ours
recovery). **New facet:** CLI-only session with no JB editor where the controller advertises an
editor-IPC listener with nothing behind it → first-finalize `no_ack` wedge every cycle.

## Fix direction

- Detect "editor-IPC listener registered but **no live editor endpoint**" for a CLI-only actor and
  route `finalize` to the controller-host disk write (the legitimate writer when no plugin is attached)
  instead of fail-closing to `no_ack`.
- Or: do not arm the editor-IPC disk-write guard when no plugin editor is attached to the actor.
- Supervisor-core. Needs an editor-less live repro + SimWorld coverage. `execution_scope=plan_backlog_only`
  for this `#agent-doc-bug` capture — no agent-doc code changed.
