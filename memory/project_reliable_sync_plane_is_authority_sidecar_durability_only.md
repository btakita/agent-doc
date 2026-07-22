---
name: project_reliable_sync_plane_is_authority_sidecar_durability_only
description: Operator directive — reliable-sync plane is ALWAYS the hot-path CRDT authority; sidecars demoted to background durability only
metadata: 
  node_type: memory
  type: project
  originSessionId: eaaecc53-041d-4608-9247-0a7edf679651
---

Operator directive (2026-07-12): `AGENT_DOC_RELIABLE_SYNC_AUTHORITY` must be **always true** — the reliable-sync in-memory `LivenessProjection` (plane) is the hot-path CRDT authority (`authority_for_file` → MultiReplica/GitAuthoritative, `editor_open_docs`, `editor_attach`, `#6b5h` lease). The filesystem sidecars (plugin-owner lease + `.agent-doc/live-buffer/` files) are demoted to **background durability only** — never the primary hot-path authority.

**Why:** the plane is proven cleaner than the sidecars it replaces — live verification (2026-07-12) showed the plane tracked exactly the open session docs while the sidecars accumulate 988 stale/non-session files. The plane is durable via its outbox, so it doesn't lose the on-disk durability the sidecars gave.

**How to apply:** promote the plane on the hot path (default-ON, not a default-OFF flag). Preserve the one safety invariant — a cold process (short-lived CLI / fresh controller with an empty plane) must still resolve authority: plane is **primary** in the controller (warm, fed by editor pushes); on plane cold-miss (empty plane) fall back to the sidecar as the durability backstop. Architecture: the plane global lives in `agent-doc-reliable-sync-io` (so `agent-doc-plugin-owner::authority_for_file` can read it without a dep cycle — controller-io already depends on plugin-owner). Plan: [[project_live_editor_reactive_family_initiative]] step 3 (authority flip) + step 6 (delete sidecar writers/reapers). See tasks/agent-doc/plan-sidecar-retirement-lazily-sync.md.

**3B "clean split" state (2026-07-12, all pushed):** steps 1-3 (authority flip) + step 6 (reader cutover) DONE/installed. 3B wire cutover foundation DONE: **lazily-kt/js `GraphView`** (generic read-only materialized view of native `Snapshot`/`Delta` — a cross-process consumer gets *values* not compute, so it's a **view** not a writable replica; conformance-pinned), **lazily-formal `GraphView.lean`** fold-idempotence proof, **agent-doc `AgentDocProjection`** (JB/VSCode domain inspector over `GraphView`, peer to signal-space's patchboard surface), **state-wire `wire_subscribe_to_ipc_message`** bridge. **REMAINING = the atomic no-fallback wire flip** (`[NEXT-SESSION FLIP]` 6-step handoff in the plan): flip `handle_state_subscribe` to native emit + rewrite JB/VSCode `StateGraphMirror`→`GraphView`/`AgentDocProjection` + retire old mirror + rebuild both plugins + operator eyeball. To be done in a **fresh/cleared session** (this one's context is huge). signal-space = patchboard's surface (opt-in lazily-runtime, `default=[]`), NOT agent-doc's home; agent-doc is a peer surface on lazily's `GraphView`.
