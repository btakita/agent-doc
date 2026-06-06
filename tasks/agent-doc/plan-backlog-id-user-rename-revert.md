# Plan: agent-doc must accept a user's manual backlog-id rename (stop fighting it) + retroactive preset-collision

**Source:** agent-doc-bugs2.md queue item — "I asked to create `backlog #next-steps`... The agent added the backlog item with the id `[#next-steps]`, which collides with the prompt preset. I manually changed the id to `[#next-steps-2]`... agent-doc kept on setting the id back to `[#next-steps]` even though I changed it to `[#next-steps-2]`... at least 2 times. ... the rule of no-conflict between a backlog id and prompt preset or other id in the document still stands."

**Supersedes the memory workaround** `project_crdt_backlog_id_revert_treadmill.md` (item 6: workarounds must become binary fixes — delete that memory once this lands).

## Two parts

### Part 1 — add-time collision reject: ALREADY IMPLEMENTED (audit-confirmed)
`reject_colliding_explicit_id` (`pending_cmd.rs:190`, `#preset-item-id-collision-enforce`) already fails a `--pending-add` whose explicit `id=<id>`/`[#id]` collides with a `prompt_presets` key or an existing active `agent:backlog`/`agent:review`/`agent:icebox` id, via `preflight::identity_collision_for_new_id`. The operator's case didn't trip it because `#next-steps` was added to `prompt_presets` *after* the backlog item (their own note), so there was no collision at add-time.

Gap: **retroactive** collision — a `prompt_presets` key added later that now matches an existing backlog id is never surfaced. Add a preflight **warning** (not a hard block) when an active backlog/review id equals a `prompt_presets` key, so the ambiguity is visible.

### Part 2 — id-revert fighting: THE REAL BUG (unfixed)
When the operator manually renames a backlog item id on disk (`[#next-steps]` → `[#next-steps-2]`, body text unchanged), the binary reverts it to the original id on the next cycle, repeatedly.

Root-cause candidates (need a repro to pin):
1. **CRDT merge favors the snapshot-side id.** The item identity in `.agent-doc/crdt/<hash>.yrs` still carries `next-steps`; merging snapshot (next-steps) vs disk (next-steps-2, same text) resolves the id field to the snapshot value. Most likely.
2. **backfill/canonicalize re-derivation.** `pending::backfill` / `canonicalize_component_content` (`pending_cmd.rs:173`) re-runs id assignment; if a custom id that is not a 4–8 char base32 hash is treated as "missing", it could be reassigned. Check `find_valid_hash_id` (`pending.rs:449`) custom-id handling.
3. **backlog→queue sync reconciliation.** The `agent:backlog priority queue` sync emits `do [#next-steps]`; a mismatch between the queue head id and the renamed backlog id may trigger a reconcile that rewrites the backlog id back.

**Fix direction:** treat an operator's explicit-id rename of an existing item as authoritative — when a backlog line's leading `[#id]` changed on disk vs snapshot, the body text is otherwise stable, and the new id does not collide (`identity_collision_for_new_id` is None), adopt the new id into the snapshot/CRDT state instead of reverting. Re-key the CRDT item identity and any `do [#old]` queue reference to the new id in the same cycle.

## Investigation findings (2026-06-06, autonomous pass)

Reproduced the simple path on a temp template doc: add a backlog item (got
`[#b1hv]`), manually rename the leading id to a custom `[#b1hv-2]` on disk, then
run `agent-doc preflight` + `agent-doc write --pending-add`. **The renamed id
survived — no revert.** Two concrete results:

- **Candidate #2 (backfill / `find_valid_hash_id` re-derivation) is RULED OUT.**
  A custom, non-hash id (`b1hv-2`) is NOT treated as "missing" / reassigned by
  the pending/backfill path. The revert does not happen there.
- **`write --pending-add` does not create `.agent-doc/crdt/<hash>.yrs`.** The
  CRDT state only exists after a stream/finalize merge cycle, so the simple repro
  never exercised candidate #1. Pinning #1 vs #3 REQUIRES a git-backed doc with
  populated CRDT state and a real finalize-merge cycle (snapshot/CRDT carrying the
  old id vs disk carrying the renamed id) — the elaborate repro below, not the
  quick path.

So the live revert is in the **CRDT identity merge (#1, still most likely)** or
the **backlog→queue-sync reconcile (#3)**, both deeper than the pending layer.
Stays GATED: the fix re-keys core CRDT item identity / queue-sync, sensitive
enough to land in an attended pass with the full repro green, not unattended
during a queue drain.

## Repro (required before the fix)
1. Template doc with `agent:backlog priority queue`, add an item (gets `[#abcd]` or a custom id).
2. Manually edit the visible id to `[#abcd-2]` on disk, leave body unchanged, save.
3. Run preflight (or let the live IPC cycle run). **Bug:** id reverts to `[#abcd]`. Capture which layer rewrote it (CRDT merge / backfill / queue-sync) via `ops.log`.
4. Add the same as a deterministic test in the layer the repro implicates.

## Verification
- Renamed id survives ≥2 preflight/finalize cycles; `do [#old]` queue ref re-keys to `[#new]`.
- Retroactive preset-collision warning appears in preflight when a backlog id equals a `prompt_presets` key.
- `make check` green; new deterministic coverage for the rename-survives and re-key cases.
- Delete `project_crdt_backlog_id_revert_treadmill.md` from memory once shipped.
