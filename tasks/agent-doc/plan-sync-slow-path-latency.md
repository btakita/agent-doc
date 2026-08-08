# Plan — Sync slow-path latency (`#syncslowpath`)

**Status:** Phase 0 landed 2026-08-07. Phases 1-4 open, ordered by measured cost.

## Origin

Operator, dogfooding: *"tmux pane auto-sync is fast on the first change, but subsequent
changes are laggy."*

Diagnosis came from 156 editor syncs in `/tmp/agent-doc-sync.log` over 6.5h. The sync body
is fast when the editor fast paths hit (35-100ms) and slow when they miss. `sync_lock_wait`
was **0 in every run** — this is not lock contention.

| phase | p50 | p90 | max |
|---|---|---|---|
| `ownership_proof` | 0ms | 56ms | **10,927ms** |
| `tmux_router` | 18ms | 130ms | 825ms |
| `controller_actor_lookup` | 90ms | 98ms | 147ms |
| `window_resolution` | 60ms | 75ms | 2,699ms |

The operator's "first fast, subsequent laggy" is visible inside a single sync:

```
01:05:02  exact_visible_actor_projection_reused  agent-doc-bugs2.md  pane=%25   ← 0ms
01:05:13  exact_visible_pane_reused_before_ipc   haiven-dev.md       pane=%20   ← 11s
```

## Invariant

`ownership_proof` is **fail-closed**: before routing a document to a pane, sync proves which
pane legitimately owns it, so it never creates a duplicate pane or steals another session's.
Every phase below must preserve that. Speed comes from *asking cheaper sources first* and
*bounding the expensive ones* — never from assuming ownership. A source that cannot prove a
live owner must still fall through to the next, and exhaustion must still fail closed.

## Cost model — where the slow path's time actually goes

Per file, on a miss of every editor fast path:

| work | site | measured |
|---|---|---|
| `resolve_current_document` (ownership) | `sync.rs` ownership loop | ~540-620ms |
| `resolve_current_document` (layout, via `resolve_file`) | `tmux-router` `sync_with_options` | ~540-620ms |
| `authoritative_actor_binding` RPC | `rpc.rs:1436` | 95-98ms healthy / **11s degenerate** |

Two content RPCs per file, not one. Three consecutive field runs confirm the split — each
logged exactly two `controller_actor_lookup` samples (~190ms total) against
`ownership_per_file_loop` of 1330/1444/1474ms, leaving ~1200ms of content fetch:

```
00:43:32  files=2 elapsed_ms=1330  sa_ms=1272  tmux_router=539
01:32:39  files=2 elapsed_ms=1444  sa_ms=1381  tmux_router=779
02:31:22  files=2 elapsed_ms=1474  sa_ms=1426  tmux_router=825
```

---

## Phase 0 — Binding-first ownership (`#ownershipbindingfirst`) — **LANDED**

The ownership loop fetched the whole document over editor IPC purely to read one frontmatter
field, `session:`, then passed that to the actor lookup. The binding is keyed by canonical
document path and **already carries the session id** — `rpc.rs:766-772` says so, and says
rediscovering it first "would invert editor authority for unsaved buffers and add an
avoidable content RPC".

Landed:

- `project_authoritative_actor_binding_by_file` — resolves the record with no `session_id`
  argument. Replaces the by-session variant, which is deleted (its only caller was the
  reordered branch). Preserves the safe-passive `source=local_projection` shortcut that
  `specs/07-session-tmux-commands.md:759-765` requires.
- `actor_records_by_file` in `SyncProofCache`, and `refresh_durable_registry_for_actor_record`
  extracted so consulting the binding earlier cannot silently drop the registry refresh.
- A binding hit also publishes into `pre_resolved_panes`, which short-circuits `resolve_file`
  in tmux-router — killing the **second** content RPC as well.
- Regression: `ownership_binding_first_routes_without_content_authority` (live tmux). Content
  is made unreadable; a live binding must still route the pane. Mutation-checked — disabling
  binding-first fails it.

**Semantic change to keep in view.** The live actor now wins a session-id disagreement
instead of being filtered out by the document's declared `session:`. That is the direction
`#lazily-hot-path` prescribes (the actor is hot-path authority; frontmatter can lag a
reroute), and ownership stays fail-closed because the loader still requires a live bound
pane. It is a genuine behavior change and the first thing to suspect if a wrong-pane report
follows this release.

Expected effect on the three runs above: ~1400ms → ~200ms for `ownership_proof`, plus the
`tmux_router` 539-825ms tail.

---

## Phase 1 — Bound the cross-root fallback RPC

**The 11-second stall.** `haiven-dev.md` hits the in-memory fast path 131 of 138 syncs. In
the other 7 the binding is absent and sync calls `resolve_cross_root_document_pane`
(`sync.rs`) → `authoritative_actor_binding` → `request_controller` wrapped in
`retry_controller_transport_drop`. Production `CONTROLLER_RPC_TIMEOUT` is **5s**
(`project_controller.rs:88`) and the wrapper retries once: 5 + 5 + overhead = the **11s**
measured twice (01:05:13, 01:22:07).

An interactive tab switch must never inherit a 10s budget. `ownership_proof`'s own budget is
750ms.

- Give the interactive ownership path a **deadline parameter** (250-500ms) rather than the
  ambient `CONTROLLER_RPC_TIMEOUT`. Background/recovery callers keep 5s.
- On deadline, fall through to registry / `last_layout.json` for a last-known pane and log an
  explicit `cross_root_binding_deadline_exceeded`. Fail-closed is preserved: an unproven pane
  is still not routed, it is just not *waited* for.
- Do not retry inside the interactive path. A transport drop there should degrade to the next
  source, not double the budget.

Regression: a fake controller that never answers must leave the sync inside its ownership
budget, and must not route an unproven pane.

## Phase 2 — Cross-sync binding cache

`SyncProofCache` lives for **one sync run**. A file whose in-memory binding is missing
re-pays the full lookup on every subsequent sync, so a degraded nested controller is an
11s tax per tab switch rather than once.

- Process-wide cache keyed `(owner_root, canonical_file)` with a short TTL and generation
  invalidation, sitting under both `authoritative_actor_binding` call sites.
- Invalidate on: generation advance, pane death, controller recycle.
- Negative results cached too, with a shorter TTL — that is what stops the repeated timeout.

Prior art in-repo: `37d4d0d1d` did exactly this for the `/proc` ownership walk
("share /proc ownership walk across syncs via process-wide TTL cache").

## Phase 3 — Carry cross-root bindings in the editor projection

`reactive_actor_bindings` excludes cross-root entries by construction (`sync.rs`, the
"Cross-root pane ownership stays with the nested controller" comment). That is why a
cross-root document can only ever reach the *second* fast path, and why it is the file that
stalls.

- Let the parent controller retain the last **controller-proven** cross-root binding it
  observed, generation-fenced, and include it in the projection it passes to sync.
- This does not move ownership authority — the nested controller still owns provisioning and
  the durable registry. It caches a proven fact the parent already received.
- Target: the 131/138 in-memory hit rate for cross-root files becomes ~100%, and Phases 1-2
  become the rare fallback rather than the routine one.

## Phase 4 — Redundant cross-root round-trip

When `exact_visible_projection && is_cross_root` reaches `resolve_actor_pane_after_content`,
the pre-IPC reuse branch **already** called `resolve_cross_root_document_pane` for that file
and got nothing. The second call re-pays the same RPC — including its timeout — for a miss
already observed this run. Phase 0 documented this; Phase 2's negative caching makes it
cheap, but the call should be removed outright once that lands.

## Phase 5 — `window_resolution` outlier

p50 60ms, p90 75ms, but **max 2,699ms** (21:40:46), and it alternates 2ms/60ms between two
sync sources in bursts. Not yet diagnosed — the 60ms floor on one of the two sources looks
like a fixed wait, matching the flat 97ms profile `controller_actor_lookup` shows. Worth one
measurement pass before deciding whether it needs work.

## Verification

- `make check` (8674 tests) for every phase.
- Live-tmux regressions run with `cargo test -p agent-doc-sync-io -- --ignored`. **Note:** 14
  of these fail on the current dev machine *at baseline*, unrelated to this work — diff the
  failure set against a clean checkout before attributing any of them to a change here.
- Field proof is the sync log, not a unit test: after each phase, confirm
  `ownership_proof` p90 and max in `/tmp/agent-doc-sync.log` across a session of real tab
  switches. The phase meters already emit everything needed.
