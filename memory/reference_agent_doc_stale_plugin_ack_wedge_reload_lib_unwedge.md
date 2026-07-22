---
name: reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge
description: "Stale-plugin delivery-ACK wedge (finalize/write loops on delivery_ack_pending) unwedges with `agent-doc admin reload-lib` — no force-disk"
metadata: 
  node_type: memory
  type: reference
  originSessionId: a7358cf0-df47-4b0e-8602-f1f39ae8d0e2
---

Symptom: `agent-doc preflight/finalize/write --commit` repeatedly prints
`[write] Waiting for typing/CRDT versions to settle (reason=delivery_ack_pending)`
and fails closed at ~8000ms with `serialized_atomic_write` / `pending_write` /
`repair_partial_captured_response`, message: "canonical response remains retained
in CRDT + Lazily state ... editor reconnect continues asynchronously (no
force-disk or operator recovery required)". `session-check` shows the cycle stuck
at `write_applied` (response landed, no terminal commit). Preflight also warns
`stale_plugin` (live JB plugin older than the binary's shipped version, e.g.
0.2.256 < 0.2.257) and often `stale_install`.

Root cause: the live editor plugin's native/IPC half is stale and never sends the
delivery ACK finalize fails-closed on. The response IS safe — it reaches the
reliable-sync plane ("response cell already present via realtime backbone
(r:...)") and even flushes to the working tree — only the terminal git commit is
blocked.

Fix (non-destructive, do NOT force-disk): run `agent-doc admin reload-lib` to
force the cdylib reload broadcast (#cdylib-reload-broadcast) to editor plugins,
then re-run `agent-doc write --commit <FILE> --baseline-file <preflight baseline>`
with the same `<!-- patch:exchange -->` response. The refreshed native path ACKs,
git_commit runs, and `session-check` reaches `ok — cycle is committed`. Confirmed
2026-07-15 on tasks/recruit/haiven.md (committed d9576a84e after 3 wedged
attempts + reload-lib).

Prefer this over the force-disk path in
[[reference_agent_doc_stale_plugin_heartbeat_forcedisk_recovery]] when the plane
already holds the response and only the ACK is wedged. Escalate to `make install`
(src/agent-doc) + `agent-doc admin recycle` only if reload-lib does not unwedge
(#staleloop-recycle-restart). Retry the editor/CRDT path, never a direct disk
write. [[project_reliable_sync_plane_is_authority_sidecar_durability_only]]

Refinement (2026-07-15, tasks/recruit/haiven.md, binary+cdylib BOTH current at
0.34.129 — so NOT a stale-binary case): reload-lib alone did NOT unwedge, and
`agent-doc admin recycle` only *deferred* ("marked to recycle at next idle
boundary") because the shared lazy `controller serve` was busy serving a
concurrent session and never hit an idle boundary. What actually cleared it was
killing the lazy `controller serve` pid (`pgrep -af "agent-doc controller
serve"`, then `kill <pid>` — it relaunches lazily on the next command, per
[[reference_agent_doc_dangling_queue_head_resurrect_baseline_overlay_and_controller_model_wedge]]).
After the kill, the retry's socket IPC reported `already_applied` ("response
already in live buffer") and git_commit ran. So `make install` is NOT the right
escalation when binary/cdylib are current — force the deferred recycle by killing
the lazy controller pid instead. The real editor client here was the route-owned
`agent-doc start --route-owned ... <FILE>` pid, not a JB IDE. Also: a
transactional `--backlog-add` whose #id a prior wedged attempt already applied on
the CRDT plane fails the whole write with `pending add: refusing to add item with
explicit id`; re-run the commit with `--no-followups` (item already captured).

Persistent-wedge escalation (2026-07-15, haiven.md, later same session): sometimes
the controller-kill recovery does NOT clear it — the wedge is the *editor replica*
process itself (here `agent-doc start --route-owned ... <FILE>`, pid 174155), not
the controller. Killing the shared `controller serve` pid repeatedly, `admin
recycle` (deferred to idle boundary), `admin reload-lib`, and 10s waits all failed
to make that replica ACK; each retry returned `delivery_ack_pending` again. In
that state the response + all backlog mutations (`--backlog-add`/`--done`) still
land on the CRDT+Lazily plane and the working-tree file (verify with `grep`), so
only the session-doc git commit is deferred. **Correct terminal action: STOP —
do NOT force-disk and do NOT kill the operator's live editor pane unilaterally.**
Report honestly that any *separate* git work (e.g. a submodule commit + gitlink
bump) is already committed and safe, that the session-doc response is retained on
the authority plane, and offer to restart the stale editor-replica pid (the clean
fix that lands it) or let it reconnect asynchronously. Repeatedly killing the
shared controller also risks a concurrent session in the same superproject —
another reason to stop rather than keep escalating. [[project_reliable_sync_plane_is_authority_sidecar_durability_only]]

Zero-replica variant + clean write --commit escape (2026-07-15, haiven.md,
committed 81b666458): when `finalize --stream` wedges on `delivery_ack_pending` AND
`session-check` (or a later preflight) reports "route-owned editor authority for
<FILE> has zero registered relay replicas; the serving supervisor/editor bridge is
stale", the editor bridge is fully DISCONNECTED (no replica exists to ACK), not
merely a stale native path — reload-lib cannot help and `admin recycle` only
defers. Clean escape: after killing the lazy `controller serve` pid, re-run
`agent-doc preflight <FILE>` — because finalize already flushed the visible
`### Re:` body to the working tree, preflight now classifies it as
`direct_response_patchback` and prints the exact recovery `agent-doc write
--commit <FILE>` (bare, NO stdin, NO `--baseline-file` — it detects the
already-present response, "[repair] Response already present ... skipping apply",
absorbs it through the snapshot/commit boundary). That ran git_commit and
auto-requested `editor_replica_reregister` in one shot where reload-lib failed.
Ordered ladder: reload-lib → (binary/cdylib current) kill controller pid →
`preflight` → bare `write --commit`. Only STOP (paragraph above) if even
`write --commit` keeps returning `delivery_ack_pending`.

Truncated-reactive-model + force-disk-duplication saga (2026-07-15, haiven.md,
final commit b636a7379): a stale JB plugin (0.2.257 < shipped 0.2.258) served a
TRUNCATED reactive model — `preflight` printed `current sourced from lazily
reactive model` with `doc_len=3462` while the on-disk file was the real 97949
bytes (verify disk with `wc -c` + `git diff HEAD` — disk is authoritative, the
3462 view is the stale plugin, NOT content loss). Do NOT finalize `--stream`
against a truncated reactive view (clobber risk). Fix that was decisive: the
OPERATOR restarts the JB IDE/plugin — ops.log then shows `crdt_current_text ...
len=97949 ... live_editors=1 delivery_converged=true` and the truncation clears.
Two more wedges followed: (a) `preflight` hung at `step 2: commit` doing a
`free_text_queue_strike` of a PHANTOM queue head (`/goal Implement backlog
item(s): #<id>`) resurrecting from `.agent-doc/baselines/<h>.overlay.yrs` even
though the on-disk queue block was empty — the controller lookup for the strike
timed out repeatedly. (b) Killing the controller to clear it DISCONNECTED the
route-owned editor replica (`agent-doc start --route-owned ... <FILE>`), so the
next finalize returned `deferred write ... the editor owns the document but no
relay replica is registered; disk was not written; recovery=await_editor_replica
_no_disk_write_then_retry_finalize` (it correctly did NOT clobber). **Key new
hazard: operator-authorized `--force-disk` DURING an editor-reconnect window
DUPLICATES the response** — the reconnecting replica wrote concurrently
(`[write] File was modified during force-disk`, `[commit] file is N bytes larger
than snapshot - possible out-of-band write`), the commit landed the answer TWICE
in HEAD, and `session-check` INTERRUPTED reporting the (already-answered) prompts
as unresolved with boundary drift. **Recovery for the force-disk duplicate: do
NOT force-disk again** (session-check says so explicitly). Once the editor has
reconnected (`preflight` completes cleanly again), run ONE more NORMAL `--stream`
finalize with a short boundary-advancing note — because `--stream` merges against
the LIVE editor buffer (which already reconciled the dup to a single copy), the
new commit's tree collapses HEAD back to one answer and advances the boundary
past the prompts. Confirmed clean: live buffer == HEAD, single answer, working
tree clean. Lesson: prefer OPERATOR JB restart over force-disk whenever the
editor is merely reconnecting; force-disk is safe only when NO editor replica is
racing the write.

Controller-fully-down + force-disk-duplicate manual repair (2026-07-15, haiven.md,
clean commit e9cd2b651): after many delivery-ACK wedges, repeated controller kills
left the project controller FULLY DOWN — `finalize`/`write --commit` returned
`failed to connect to project controller: Connection refused (os error 111)` or
`could not capture initial editor authority ... timed out after 0.8s`, and a stale
0-byte `.agent-doc/controller.sock` blocked lazy relaunch. Recovery: `rm -f
.agent-doc/controller.sock`, then start a fresh controller (`setsid agent-doc
controller serve --project-root <root> --launch-mode lazy &`), confirm with
`agent-doc read <FILE>`. force-disk THEN committed but (editor still holding a
buffer bloated by the earlier stacked attempts) duplicated the response — HEAD got
2-3 `### Re:` blocks plus a jumbled partial operator prompt. Since force-disk had
committed with a CLEAN working tree, the reliable fix was **manual**: Edit the
session doc down to ONE response, preserve any operator prompt text once (never
drop it — put an unanswered one AFTER the `agent:boundary` marker so it stays a
pending prompt), `git commit` directly, then **`agent-doc reset --from-current
<FILE>`** to rebuild the snapshot/CRDT/overlay from the now-clean file so the
wedged editor replica cannot reconnect and re-clobber HEAD with its stale bloated
buffer. Verified stable (grep `^### Re:` == 1, working tree clean) after a short
wait. `reset --from-current` is the resync tool after any manual session-doc
repair. Broader lesson from this whole session: once force-disk has duplicated
once, STOP retrying agent-doc paths — do the manual collapse + reset instead of
piling on more finalizes (each one stacks another racing response cell).

`editor_attached_model_missing` variant (2026-07-18/19,
tasks/professional/equityfundingsource.md): a DIFFERENT symptom that responds to
the same reload-lib fix. Preflight fails with `resolve current document ...
Caused by: editor_attached_model_missing: editor authority unavailable; Lazily
still reports the editor open, so disk is not consulted as a fallback` — the
plugin holds an *attachment* for the path but no CRDT model behind it. The JB
plugin version MATCHES the binary's expected version (0.2.278 == 0.2.278), so the
usual `stale_plugin` warning never fires and this looks like a different class of
bug. Two graded severities: (a) mild — preflight succeeds but every cycle warns
`queue_maintenance_not_persisted` and silently discards the recomputed queue text
(backlog stays intact; only queue prose goes stale); (b) hard — preflight itself
exits 1 and no cycle can start. `agent-doc admin recycle` does NOT clear either
severity even when it lands successfully (the `supervisor_binary_stale` warning
disappears afterward, proving the recycle worked and the wedge is elsewhere).
`agent-doc admin reload-lib` DID clear the hard variant immediately —
`[admin] reload-lib: cdylib typed reload delivered to 1/1 editor endpoints` and
the next preflight returned clean with `committed: true`. So for
`editor_attached_model_missing`, go straight to reload-lib and do NOT bother with
recycle first. Never force-disk here: disk is correctly being refused as
non-authoritative while the editor claims the file. Also seen alongside, and
harmless: `queue consume: queue_active is true but document queue has no prompt
to consume` when the queue drains to empty while still flagged active — the
response and backlog mutations commit before that step, and session-check still
reports ok.
