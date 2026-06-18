# Plan — wedged file-IPC transport produces non-provenance (false-committed) closeout with no durable fallback

## Symptom

During a `boost-client/tasks/monsterrodholders.md` session (2026-06-18, agent-doc 0.34.3 CLI
against a stale `0.1.0` route-owned supervisor), every `finalize` / `write --commit` of a valid
assistant response failed to land in HEAD:

```
[write] IPC timeout (2s) — leaving patch for editor retry; refusing direct document write
[write] IPC proof insufficient ... source=file_ipc ... invariant=no_ack recovery=retry_without_disk_write timeout_secs=2
[write] editor IPC did not prove the write — refusing direct document write; retry after the editor applies the queued patch
```

Yet a prior cycle was internally recorded as reaching state `committed` while its body was **absent
from HEAD** ("non-provenance through the wedged transport"):

```
Cycle cycle-XXXX reached state `committed` but the captured response body (... state `committed`)
is not present in HEAD for tasks/monsterrodholders.md.
```

## Root cause (observed)

1. **All registered editors were dead.** `.agent-doc/patches/` held 14 `jetbrains-<pid>-<uuid>.json`
   registrations whose PIDs were every one dead (`kill -0` fails). With zero live editor buffers,
   file-IPC `no_ack` is structurally guaranteed — there is no consumer that can ever ack — so the
   `retry_without_disk_write` recovery loops forever and the response can never be committed.
2. **No live-editor liveness gate before choosing file-IPC.** The write path selects `source=file_ipc`
   because the plugin "looks installed" (registration files / config present) without verifying that
   any registered editor PID is still alive. When none are alive, it should fall back to a durable
   direct disk write instead of refusing.
3. **False-committed provenance.** A capture was marked `committed` in session/controller state while
   its body never reached HEAD, so a later `preflight` both (a) refused to re-close it and (b) reported
   an already-committed cycle whose content is missing — a provenance gap that blocks all subsequent
   closeouts until manually cleared.
4. **Stale supervisor amplifier.** The route-owned host supervisor was mapping a stale `0.1.0` binary;
   `admin recycle` only fires at an idle boundary, and `session restart-supervisor` re-launched the
   same stale binary, so the wedge persisted across restarts while the session stayed busy.

## Fix directions

- Before selecting `file_ipc`, probe registered editor liveness (PID alive + recent heartbeat). If no
  live editor consumes the doc, route to a durable direct disk write (the `--force-disk` path) instead
  of `no_ack` → `retry_without_disk_write` forever. Treat "all editors dead" as not-installed.
- Never record `committed` until the body is provably in HEAD (or in a proven file-IPC ack). The
  `committed` transition must be gated on durable-write proof, not on patch-queued.
- Garbage-collect dead-PID `jetbrains-*` registration files (and on supervisor recycle) so routing
  stops chasing dead consumers.
- `session restart-supervisor` for a stale-binary owner should re-exec against the freshly installed
  binary path, not re-launch the stale target.
- Provide a first-class "clear stuck `response_captured` cycle" recovery that does not require manual
  patch-file deletion + `reset --from-current`.

## Manual recovery used this session

1. Confirmed all 14 `jetbrains-*` patch PIDs dead; removed them + `save-document.signal` + the orphaned
   queued `.json`.
2. `agent-doc admin reap` the orphaned `mrh-servers.md` dead-pane binding; `admin detect` → clean.
3. `agent-doc reset --from-current tasks/monsterrodholders.md`.
4. Landed the response and the pending document edits via `agent-doc write --commit --force-disk`
   (durable disk write; no live editor to conflict).
