---
name: reference_cdylib_reload_strands_attached_document_replica
description: "A mid-session `make install` hot-reloads the cdylib and strands the Lazily replica of an attached document — closeout then fails at prevalidate with missing_replica / editor_attached_model_missing and reload-lib does NOT clear it; only an editor-side re-attach does"
metadata:
  node_type: memory
  type: reference
---

## 2026-09-28 (later): the self-heal below assumes the handoff RUNS — check that first

The self-heal is real, but it can only fire if a new generation actually loads.
A second failure wears the same `missing_replica` face and never heals, on any
timescale: **the generation handoff itself fails closed**, so the IDE keeps
executing the old cdylib and no re-registration is ever attempted.

Separate them from the IDE log, not from ops.log:

```bash
grep -a "NativeReloadCoordinator" ~/.cache/JetBrains/*/log/idea.log | tail
# self-heal case:  "[native] replica restart converged"
# wedge case:      "[native] reload failed closed; an IPC listener did not terminate"  (repeats forever)
ls -lt /tmp/agent-doc-native-<IDE_PID>/    # newest .so OLDER than your make install == wedged
ss -xlp | grep "ipc-<IDE_PID>.sock"        # TWO LISTEN rows for one path == duplicate generations
```

Cause: `socket_path` is keyed by pid, so every generation in one IDE shares a
name; a new listener unlinked the live one and rebound it. `wake_listener`
then reached the wrong listener, the evicted one never saw its shutdown token,
and `quiesceAllForNativeReload` refused the handoff — permanently. Fixed in
`36eaa1357` (refuse to evict a serving peer), `23f81e9ac` (prove *serving* with
a round trip — a bare connect succeeds into a dead listener's backlog) and
`3b06dcc28` (non-blocking accept, so shutdown needs no wake).

**The fix cannot deploy itself into an already-wedged IDE** — the loaded code is
the broken code. That process needs one restart; after it, the guards hold.
Downstream on 2026-09-28: retained writes that never settle on fpe.md /
monsterrodholders.md / sdk.md, a doubled `agent-doc: persisting` banner (one per
resident generation), File Cache Conflict, and stale-supervisor warnings — all
one cause. See [[reference_ipc_build_mismatch_was_a_clock_not_a_code_difference]].

## 2026-09-28: SELF-HEALS NOW — the operator tab reopen below is obsolete

Everything in this file that says the clearing action is editor-side and
operator-gated was true and is no longer. Measured on `agent-doc-bugs.md` after a
`make install` reload:

```
13:25:19-20  editor_replica_reregister_attempt x3   reregister=definitively_refused:1
13:25:21     realtime_doc_resolve_missing_replica_terminal_rebuild_failed
13:25:22     realtime_doc_resolve authority=disk          reason=editor_absent     diverged=false
13:25:25     realtime_doc_resolve authority=editor_buffer reason=crdt_relay_current diverged=false
```

Three seconds, three documents, no operator action, `diverged=false` on both
sides and zero `disk_read_refused` after. Do not hand the operator a tab reopen
for this any more; wait one resolve and re-check.

**The strand never needed a new recovery edge — it needed the refusal to be
legible.** `EditorReplicaStrand`'s demotion edge already covered an endpoint that
answers and refuses. What blocked it was the IPC build identity: while it was a
build-script wall clock (see
[[reference_ipc_build_mismatch_was_a_clock_not_a_code_difference]]) client and
listener never matched, so every re-registration died in a handshake mismatch
that the running binary classified as NON-definitive, and the latch had nothing
to demote on. Content-derived ids (`88b21fea6`) plus `9e398b6ab` — an exhausted
build mismatch IS a definitive answer — made it legible. That is why this file
recorded the strand as operator-gated for months: the symptom was the replica,
the cause was the identity.

**Diagnostic that settles it in one command:**

```bash
grep -a editor_replica_reregister_attempt .agent-doc/logs/ops.log | grep -o 'reregister=[a-z_:0-9]*' | sort | uniq -c
```

`definitively_refused:N` means the exit exists and the document will recover on
its own. `not_delivered` or `failed:` means it will not — that is silence, which
this design deliberately never demotes on, and it is the shape to investigate.

A third shape, an endpoint that ACCEPTS every request and never serves, is
covered by `582822a9c` but has never been observed here; it is a state-machine
completion, not a measured failure.

**FIXED in 0.35.384 (`#reloadgateperprocess`).** The reload planner now groups
all project endpoints and document evidence by editor PID before making one
native-generation decision. Exact reliable-sync registrations retain
`(pid, document)` attribution; when that projection is empty, live PID sockets
inherit otherwise-unattributed supervisor documents so the safety gate remains
closed. A supervisor-discovered project root is added even when the controller
process scan missed it, and `reload_library_project_root_recovered`,
`reload_library_status_failed`, and
`reload_library_process_endpoint_unavailable` make every former silent bail
diagnosable. The pending idle-boundary publisher re-enters the same global
planner rather than a project-local bypass.

The original failure was measured 2026-09-12 18:25 and 18:42 with open
`cycle-1789236397866`: `lib-install` reported `0 deferred mid-cycle` both times,
and after 0.35.373 neither `reload_library_deferred_cycle_open` nor
`reload_library_no_document_candidates` reached this project's ops.log even
though the IDE reloaded (`crdt_replica_deregister` 18:26:01). The underlying
rule remains: a cdylib belongs to an editor process, so every document served by
that process must vote before its generation is retired. A *stalled* open cycle
does not hold it; an unreadable cycle projection does.

**The plugin-side half is fixed in agent-doc 0.35.385 / JetBrains plugin
0.2.384 (`#jbreloadreregconverge`).** The old coordinator captured resume state
only after every manager worker terminated; one busy lane therefore suppressed
checkpointing for every document, while disposal still dropped all replicas.
It also released the application reload gate after scheduling a fire-and-forget
open-tab sweep. The handoff now checkpoints every attached forwarder on its
serialized document lane before shutdown or unload; any missing checkpoint
retains the old generation and live managers. After a successful generation
swap, the coordinator waits for every current Markdown tab to register and logs
an exact converged or failed-path report before releasing the gate.

**2026-09-27 21:09 variant: the strand can land POST-capture, and then
`reload-lib` deadlocks against the open cycle.** Same project, same document,
one `make install` at 21:08 (three cdylibs mapped in idea pid 2435532:
`libagent_doc-1790536729489.so`, `-1790542028715.so`, `-1790557728283.so`).
Two things differ from the 2026-09-11 record:

- **The failure was post-capture, not pre-capture.** `respond` got as far as
  reaping the tracked-work mutation (`[pending] completed and reaped 1 item(s)
  atomically`) and retained a 4939-byte response; `session-check` then reported
  `INTERRUPTED: cycle ... is still response_captured`, and
  `agent-doc repair <FILE>` found and re-applied the orphaned response only to
  fail at the same `editor_attached_model_missing`. So the "no capture is
  retained, re-send once the replica is back" note above is **not** universal —
  check `session-check` for `response_captured` before assuming a re-send is
  duplicate-safe. Here `repair` is the correct resume, and it is idempotent.
- **`admin reload-lib` cannot break the cycle it is blocked by.** It reported
  `typed reload delivered to 0/1 editor endpoints ... (1 deferred mid-cycle)`.
  The open cycle defers the reload; the stranded replica keeps the cycle open.
  That is a closed loop with no in-band exit — do not retry it.

Also note the plugin registers at the *correct* version through all of this
(`[editor] live jetbrains plugin 0.2.431 registered (expected 0.2.431)`), so a
healthy plugin-version line is not evidence the document replica survived.

Everything below is the original 2026-09-11 forensic record, and it remains the
live recovery procedure.

---

**The post-install finalize race is real, and it is not version skew.**
[[feedback_no_mid_session_install_against_live_supervisor]] recorded that
`#supselfheal` mitigates the supervisor/client skew but noted its proof covered
only the *recycle*, never a *post-install finalize*. Observed 2026-09-11 23:36 on
`tasks/agent-doc/agent-doc-bugs.md`, one minute after the session's **second**
`make install`: the closeout failed and never recovered.

**Signature.** `agent-doc respond` fails at `prevalidate_tracked_work_response`
and `session-check` at `session_check_response_replay_dedup_current`, both with:

```
editor is still attached for <FILE>; missing_replica recovery exhausted
and disk read authority is refused
```

ops.log, in order: `crdt_current_text_unavailable ... authority=multi_replica
reason=missing_replica` (for two different `process_pid`s), then
`realtime_doc_resolve_missing_replica_terminal_rebuild_failed ... editor authority
stayed in editor_attached_model_missing after a bounded Lazily-current observation
request ... recovery=retry_without_disk_write ... operator_action=none`, then
`realtime_doc_resolve_disk_read_refused ... invariant=attached_editor_never_descends_to_disk`.

**The disk refusal is CORRECT — do not force-disk.** The defect is that the
promised asynchronous re-registration never converges.

**Smoking gun: check `/proc/<idea-pid>/map_files/` for TWO cdylibs.**

```bash
ls -l /proc/$(pgrep -x idea)/map_files/ | grep -o "libagent_doc[^ ]*" | sort -u
# libagent_doc-1789182099748.so   <- 23:01 install
# libagent_doc-1789184163251.so   <- 23:36 install
```

The replica lived in the instance the reload replaced. The epoch in the filename
is a unix ms timestamp, so it dates each install directly.

**Why this looks like a different bug class.** The plugin jar is CURRENT — the
running IDE had `agent-doc-jetbrains-0.2.382.jar` mapped and 0.2.382 was the
staged version — so the usual `stale_plugin` preflight warning never fires. Verify
with `ls -l /proc/<pid>/map_files/ | grep -o "agent-doc-jetbrains[^ ]*"` rather
than assuming a stale plugin.

**What does NOT clear it** (all tried, in this order):
- `agent-doc admin reload-lib` — the documented recovery for
  `editor_attached_model_missing` in
  [[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]] — reported
  `cdylib v0.35.367 typed reload delivered to 1/3 editor endpoints across 9
  projects (0 restart required, 3 unavailable)` and changed nothing. Repeating the
  reload cannot restore a replica for an already-loaded document; the reload is
  what dropped it.
- `agent-doc admin detect` — "No fleet anomalies detected". Its silence is not
  evidence of health.
- Waiting ~2 min and re-checking twice; the idle watch keeps polling
  `controller_crdt_current_text ... status=editor_attached_model_missing`.
- Retrying `respond` (the diagnostic's own `recovery=retry_without_disk_write`).

**Scope is per document, not per project.** `tasks/software/existence-lang.md` in
the same project resolved normally throughout (`authority=editor_buffer
diverged=false`), so a healthy sibling document is not evidence the wedge cleared.

**Every failure is PRE-capture**, at `prevalidate_tracked_work_response` — so no
response is retained and no retry can duplicate one. That is the opposite of the
retained-write cases in [[reference_agent_doc_recovery_index]]: here the
"do NOT re-send / reload-lib / recycle" clause does not apply, because there is no
capture being awaited. Keep the composed response in a file so a context compaction
cannot lose it, then re-send once the replica is back.

**Clearing action is editor-side and operator-gated:** in IntelliJ, close and
reopen the document tab (or save it, or restart the IDE) so the plugin
re-registers the replica against the current cdylib; then re-invoke
`/agent-doc <FILE>`.

**How to apply:** the ordering is a trap — `#deploy-just-do-it` mandates a
`make install` after every fix, and that install can block the closeout of the
very session that made the fix. Prefer finishing the document closeout FIRST and
installing after, or expect to hand the operator a tab reopen. The product fix
(filed as `#installstrandsreplica`) is for a cdylib reload to re-register every
attached document's replica into the new instance before dropping the old one, or
to defer the reload while an attached document has an open cycle — the supervisor
already has `supervisor_recycle_deferred_cycle_open` for exactly that shape.

---

## 2026-09-20 correction: TWO `.so` is NOT sufficient proof, and bare `missing_replica` is background noise

Both discriminators above were re-measured and are weaker than this file implied.
Do not open an investigation on either one alone.

**`crdt_current_text_unavailable ... reason=missing_replica` is benign transient
churn.** Measured on agent-loop 2026-09-20: **988 occurrences across 7 documents**
(agent-doc-bugs, existence-lang, tsift, lazily, agent-loop, chromium-bridge,
equityfundingsource), 04:16Z-21:05Z, **including hours with no install at all**.
Every sampled one is immediately followed by a successful
`crdt_commit_barrier ready=true visible_delivery_projected=true` and a
full-length `crdt_current_text`, with replicas pulling normally (210
`crdt_replica_pull`, generations advancing 7→8→9). It resolves on the next
attempt. Filed as `#missingreplicanoise` because the benign and terminal cases
are logged identically.

The terminal signature is the FOLLOW-ON, never the bare line:
`realtime_doc_resolve_missing_replica_terminal_rebuild_failed` +
`editor_attached_model_missing` surviving a bounded observation +
`realtime_doc_resolve_disk_read_refused`. Grep for those, not for
`missing_replica`.

**TWO mapped `libagent_doc-<epoch>.so` also does not prove a strand.** Same day,
IDEA PID 1023909 mapped two (16:07:57 and 16:42:13 installs) while every document
in the project converged fine. An old generation stays mapped until the JVM
unmaps it; that is ordinary. The two-`.so` check is a *consistency hint*, not the
finding.

## Better probe: the mapped jar's inode, not its version string

For "is the running IDE on the staged plugin", version strings and mtime
arithmetic both lie, because `make install` reuses the same filename and
**version number** for different bytes. Resolve what the JVM actually mapped:

```bash
for f in /proc/<idea-pid>/map_files/*; do readlink "$f"; done | grep agent-doc-jetbrains
# → .../agent-doc-jetbrains-0.2.388.jar (deleted)
```

`(deleted)` means the install unlinked the inode the JVM is still executing: the
IDE runs superseded bytes under an identical version string, so every
version-string check reports "activated" and is wrong.

**Check `(deleted)` FIRST and short-circuit — inode numbers are reused.**
Measured 2026-09-20 across two installs on this machine: the replacement jar came
back on the SAME inode 76585058 the unlinked one had. An inode-only comparison
returns "current" for a process demonstrably running unlinked bytes. Only fall
back to comparing the mapped inode against the on-disk jar when the link does
*not* carry the suffix. Measured 2026-09-20: mapped
jar `(deleted)`, on-disk inode 76585058 written 16:42:24, both labelled 0.2.388.
Filed as `#pluginbyteidentity`; it makes plugin-activation review items
machine-checkable instead of operator-gated.


## `agent-doc repair --apply-recovery` can contradict `session-check`

2026-09-20: `session-check` reported `INTERRUPTED: document still contains
completed tracked item(s) after closeout: #pluginbyteidentity. Re-run
preflight/repair so the reap is persisted`, and `agent-doc repair
--apply-recovery` on that same document answered `is clean — no recovery
needed`. Repair's drift classifier does not recognise an unreaped `[x]` backlog
item, so the remedy session-check names does not implement it. The response had
committed fine; only the archive move was outstanding. Do not read repair's
"clean" as proof the interruption is spurious, and do not loop past the
interruption — preflight is the half of that remedy that actually owns the reap.

**REFINEMENT 2026-09-26 23:38 — the strand hits SIBLING documents, not the one you
closed out.** Following this file's own ordering ("close out the document FIRST,
install after"), `make install` + `agent-doc admin recycle` ran only after
`tasks/agent-doc/agent-doc-bugs.md` reached `committed` with a drained queue. That
document came through clean. Three OTHER attached documents did not:

```
[03:38:18Z] realtime_doc_resolve_missing_replica_terminal_rebuild_failed file=tasks/software/agent-loop.md      source=prepare_start_runtime pass=bypassed_no_content_identity reason=missing_replica
[03:38:18Z] realtime_doc_resolve_disk_read_refused                       file=tasks/software/agent-loop.md      editor_open=true recovery=upstream_replica_refresh_exhausted invariant=attached_editor_never_descends_to_disk
[03:38:18Z] ... file=tasks/software/chromium-bridge.md   (same pair)
[03:38:19Z] ... file=tasks/software/lazily.md            (same pair)
```

`source=prepare_start_runtime` is the tell: the strand lands on whichever attached
documents have their start-runtime prepared *after* the reload, which is every
other open session document — including one owned by a different live session
(`lazily.md`). So closing out your own document protects your own closeout and
nothing else.

**Practical consequence:** after a mid-session install, grep ops.log for
`terminal_rebuild_failed` and report the FILE LIST, not just "a replica was
stranded" — each of those tabs needs reopening, and one of them may belong to a
pane you do not own. Counting bare `missing_replica` (139 occurrences here) or
`editor_attached_model_missing` (112) still tells you nothing; only the
`terminal_rebuild_failed` + `disk_read_refused` pair does, and there were exactly
3 of each.
