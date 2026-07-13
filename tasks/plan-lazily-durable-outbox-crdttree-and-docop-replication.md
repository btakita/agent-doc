# Plan: lazily DurableOutbox(SqliteOutbox) + CrdtTree trait → agent-doc document-op replication (never-frozen canonical)

**Handoff for Codex.** Owner started this in Claude Code; running low on tokens. This
file is self-contained — file:line refs verified 2026-07-13 against the working tree.
Original goal: **fix `live_editors == 0`** (the phantom-lease wedge that resurrects
operator-deleted content, e.g. `#sy71`). Root cause and full fix below.

## ⚠️ LIVE-INCIDENT + CURRENT STATE (2026-07-13, post-deploy) — READ FIRST

The reattach-adopt push was deployed live and caused a **runaway** on the live
equityfundingsource doc: the JB `ensureEditorText` handler fired the adopt push on EVERY
buffer-align divergence and pushed the editor's whole op-log (`encode_state` =
tombstone-bloated `Vec<TextOp>`), which fed back (adopt→canonical→editor re-align→more
tombstones→bigger push) — `state_len` 77MB→167MB in ~2 min. It **self-limited** at 08:20:18
(align settled) and caused **NO text corruption** (every adopt was `changed=false`).

**Mitigation applied (deployed):**
- `agent_doc_reliable_sync_push_full_state_adopt` (ffi.rs) is now a **hard no-op** (gated);
  gated cdylib rebuilt + `lib-install`ed → the running plugin reloads it → pushes no-op.
- The JB `ensureEditorText` adopt push was **removed** (source reverted).
- Controller was killed to break the loop; respawned clean.

**What's still valid + green (keep):** the controller/hub side — `RelayHub::adopt_editor_full_state`
(lineage-intact), `adopt_editor_full_state_for_file`, the adopt-frame encode/decode + routing,
`apply_document_op_delta` (P2), the `request_full_state` signal. Those are correct and tested;
only the *plugin trigger* was wrong.

**Controller-side root-cause fix — DONE + tested (2026-07-13, post-incident):**
`RelayHub::adopt_editor_full_state` now has a **self-echo guard**: if the incoming state's
visible text already equals the canonical, it is a hard no-op — NO replace (so the canonical
never accretes the editor's tombstone op-log), NO broadcast, NO rebootstrap. This breaks the
controller half of the feedback loop (a `changed=false` adopt can't fan back). Test
`adopt_editor_full_state_self_echo_is_a_no_op_no_broadcast` (bloated-but-same-text state →
no-op, no broadcast) passing. This alone would have stopped the runaway. **SimWorld gate DONE:**
`repeated_same_text_adopts_converge_without_growth_feedback_loop_gate` reproduces the runaway
(20 rounds of a churning editor whose op-log grows each align while its text stays the same)
and proves the guard makes it converge — canonical never grows, never broadcasts. Passing.
So the pre-re-enable gate the plan required is met on the controller side.

**Correct re-implementation (before re-enabling the FFI push — needs a controlled re-deploy,
NOT auto-run after the incident):**
1. Scope the push to a GENUINE reattach (register-after-failure in `CrdtReplicaForwarder`), NOT
   every `ensureEditorText` align — a one-shot on reattach, not a per-divergence stream.
2. Make it BOUNDED — on reattach with no other live members, rebuild the canonical from the
   editor's TEXT (small) not the full op-log; or cap/skip when `encode_state` is oversized. The
   op-log full-state push is O(tombstones) and unbounded on a churned doc.
3. Also fix the feedback: an adopt that lands `changed=false` must NOT trigger an editor re-align
   that re-pushes. Add a self-echo guard (don't re-adopt a state the canonical already equals).
Re-enable the ffi.rs push only after a SimWorld test reproduces the feedback loop and proves the
one-shot/bounded version converges without growth.

## ✅ ACUTE-WEDGE FIX IMPLEMENTED + BUILT (2026-07-13) — plugin trigger DISABLED pending redesign (see incident above)

Every layer of the reattach acute-wedge fix (`#reattach-adopt`) is implemented, compiling, and
built; the Rust side is `make check` green (7520 tests); the JetBrains plugin fully builds
(**signed zip `agent-doc-jetbrains-0.2.241-signed.zip`** ready). See the `#reattach-adopt`
section below for every file/symbol. The ONLY thing left is the **live install + verify** on the
operator's running Idea session, which is inherently disruptive + partly manual (IDEA GUI), so it
is deliberately NOT auto-run:
1. `make install-full` (agent-doc binary: controller hub signal + adopt routing) + `agent-doc
   lib-install` (FFI cdylib) — **restarts/reloads the live controller** (disrupts the active doc).
2. IDEA GUI (manual): Settings → Plugins → Install Plugin from Disk → the 0.2.241 signed zip;
   uninstall-first→restart→install→restart since classes changed structurally (new methods).
3. Restart the controller (or auto-recycle picks up the new cdylib).
4. **Live-verify**: induce the phantom lease (controller recycle → `live_editors==0`), delete a
   drifted line, confirm it stays deleted; watch ops.log for
   `crdt_editor_full_state_adopted ... recovery=reattach_drop_drift` and the plugin's
   `[reattach-adopt]`. Then mirror the plugin handler in VS Code/lazily-js.

## ⏸ WHERE CLAUDE GOT CUT OFF (2026-07-13) — Codex starts here

**Big correction:** there is **NO yrs/TextCrdt substrate split** (I misread stale comments;
see Finding 1). The relay canonical is already `lazily::TextCrdt` and already exposes the
document-op delta surface. **So P2 was wireable and I wired the controller side:**

- ✅ **P2a** hub fold `RelayHub::apply_document_op_delta` (`crdt_relay.rs`) + test
  `apply_document_op_delta_feeds_canonical_with_no_live_editors` (passing) — feeds the
  canonical at `live_editors==0`.
- ✅ **P2b** relay-io wrapper `apply_document_op_delta_for_file` (`agent-doc-crdt-relay-io/src/lib.rs`)
  + controller RPC routing in `handle_reliable_sync` (`agent-doc-controller-io/src/project_controller/rpc.rs`):
  a document-op frame with `request.file` folds into the relay canonical. Inert until a
  plugin pushes such frames. **Clippy clean + full `make check` GREEN (7518 tests pass).**
  Uncommitted in the working tree (agent-doc submodule) for review/commit.

**Acute-wedge hub-side fix — DONE + tested this session (`#reattach-adopt`):**
`RelayHub::adopt_editor_full_state(&mut self, full_state: &[u8])` (`crdt_relay.rs`) rebuilds
the canonical **from the editor's authoritative full lazily state** (`ReplicaState::encode_state`
= `Vec<TextOp>`, editor's `OpId`s), REPLACING the drifted canonical — drops drift (`#sy71`)
while keeping lineage so later editor deltas merge (no dup). Test
`adopt_editor_full_state_drops_canonical_drift_lineage_intact` (drifted canonical + editor
full-state lacking the drift → drift dropped, later delta merges clean) — passing. Wrapper
`adopt_editor_full_state_for_file` in `agent-doc-crdt-relay-io/src/lib.rs`, compiles.
**This solves the lineage trap** (it does NOT mint fresh ids; it adopts the editor's ops).

**Rust handshake side — DONE + green this session:**
- Adopt frame: `document_op::encode_full_state_adopt_frame` / `decode_full_state_adopt_frame`
  on `DOCUMENT_OP_ADOPT_NODE = NodeId(u64::MAX-2)` (distinct from the fold `DOCUMENT_OP_NODE`);
  test `adopt_and_fold_frames_round_trip_and_route_distinctly` — passing.
- Controller routing: `handle_reliable_sync` (rpc.rs) checks the adopt frame FIRST → routes to
  `adopt_editor_full_state_for_file` (replace); else the fold path → `apply_document_op_delta_for_file`
  (union-merge). Clippy clean. Inert until the FFI sends an adopt frame.

**Reattach handshake — ALL Rust + FFI + JNA DONE this session; ONLY the plugin handler +
build/install/live-verify remain.** Regression-safe additive design (interim union-merge kept;
adopt corrects the drift when it lands):
1. ✅ **Hub asks for full state on reattach**: `relay_replica_update_for_file`
   (crdt-relay-io:1169 `reattached` branch) emits `signal_crdt_replica_event(file, "request_full_state", 0)`.
   Additive — the existing union-merge stays, so no regression; the adopt replaces the drifted
   canonical when it arrives. Compiles + `make check` green.
2. ✅ **Push-with-file RPC client**: `push_reliable_sync_frame_for_file` (rpc.rs) — sets
   `request.file` so the adopt frame routes to the canonical (liveness push is hash-keyed, no file).
3. ✅ **FFI adopt-push export**: `agent_doc_reliable_sync_push_full_state_adopt(project_root,
   file_path, full_state_json)` in `src/ffi.rs` — wraps the editor's `encode_state` JSON in an adopt
   envelope (`document_op::encode_full_state_adopt_envelope`) and pushes via (2). Compiles.
4. ✅ **JNA binding**: `agent_doc_reliable_sync_push_full_state_adopt` in `NativeLib.kt`.
5. ✅ **Plugin handler — IMPLEMENTED + COMPILES this session (JetBrains; `compileKotlin` exit 0):**
   In `CrdtReplicaForwarder.ensureEditorText` (which already `applyLocal`-replaces the replica
   with the editor's document text → drift-free — and only runs when `current != editorText`,
   exactly the reattach/drift case), after `node.encodeState()` push it as an adopt frame:
   `transport.pushFullStateAdopt(filePath, String(update, UTF_8))`. Added `pushFullStateAdopt`
   to the `ReplicaTransport` interface (default no-op) + `CpcSocketReplicaTransport` override
   calling `AgentDocLib.get().agent_doc_reliable_sync_push_full_state_adopt(projectRoot, filePath,
   fullStateJson)`. This is the clean fit: encode_state there is drift-free, so the adopt carries
   the operator's authoritative text (incl. their delete of the drifted line) → controller
   REPLACES the drifted canonical. REMAINING: `./gradlew buildPlugin` / `make bump-plugin`,
   `agent-doc lib-install` (FFI cdylib), install plugin, restart controller, **live-verify in
   Idea** (watch ops.log for `crdt_editor_full_state_adopted ... recovery=reattach_drop_drift`).
   Mirror in VS Code/lazily-js. (VS Code + the durable-outbox durability of the adopt push are
   the only non-JetBrains follow-ups.)

   Original approach notes (superseded by the ensureEditorText fit above): `CrdtReplicaManager`
   POLLS remote drains (`drainRemoteUpdates`, triggered by DocumentListener edits / file-open /
   the `#crdt-drain-idle-quiet` idle poll) and does **NOT** read crdt-replica-event *reasons* —
   so the `request_full_state` signal alone won't trigger a push. Two ways to bridge it (pick one):
   - **(A, recommended — fits the architecture, no controller signal needed) plugin-side reattach
     detect:** in `CrdtReplicaForwarder`, when `register` SUCCEEDS after a prior failure/null
     (`registerFailureCounts`/`lastRegisterError` show a reattach), push the editor's full state:
     get `NativeLib.agent_doc_replica_encode_state(clientId, outLen)` (already done at
     `CrdtReplicaForwarder.kt:518`) → `NativeLib.agent_doc_reliable_sync_push_full_state_adopt(
     projectRoot, filePath, fullStateJson)` (JNA binding added). This is the editor detecting its
     own reattach — clean, and the hub's `request_full_state` signal becomes an optional backstop.
   - **(B) signal-consume:** add event-reason consumption to `CrdtReplicaManager` (read the event
     file JSON's `"reason"`; the controller writes it — `signal_crdt_replica_event` body has
     `"reason"`) and on `request_full_state` do the same encode_state→push. More plumbing.
   Mirror in VS Code/lazily-js. The controller adopt routing + FFI + JNA are all in place, so the
   plugin side is just: on reattach, encode_state → `agent_doc_reliable_sync_push_full_state_adopt`.
   **CRITICAL caveat (verified in `CrdtReplicaForwarder.register` :61-90):** on reattach
   `register()` re-bootstraps the FFI replica FROM the canonical (`node.open(ack.bootstrap)`),
   which carries the drift. So a bare `encode_state` right after reattach may INCLUDE the drift →
   adopting it is a no-op (re-adopts the drift). The plugin MUST first make the replica hold the
   editor's authoritative document text (the operator's buffer, which lacks the drift) BEFORE
   encoding+pushing — i.e. run `ensureEditorText`/`replace_all` to the IntelliJ `Document` text so
   the replica == editor buffer (drift-free), THEN `encode_state` → push adopt. This is the lazily
   embodiment of the owner directive "prefer the editor buffer": the editor's *document text* is
   the authority the adopt frame must carry. Getting this exact requires the live editor to verify
   (induce reattach, delete a drifted line, confirm the pushed adopt frame carries the deletion).
6. ⬜ **Build/install/verify**: FFI cdylib via `agent-doc lib-install`; JB plugin via
   `make bump-plugin` (see `editors/jetbrains/CLAUDE.md` — uninstall→restart→install→restart on
   structural change); restart the controller (`agent-doc admin recycle` or kill the
   `controller serve` pid). Then **live-verify in Idea**: induce the phantom lease
   (`live_editors==0`, e.g. after a controller recycle) + delete a drifted line → confirm it
   stays deleted (no `#sy71` resurrection) and nothing duplicates. Watch ops.log for
   `crdt_editor_full_state_adopted ... recovery=reattach_drop_drift`.

**Original wedge analysis (kept for reference):**
Phantom-heal `relay_replica_update_for_file` (`agent-doc-crdt-relay-io/src/lib.rs:1132`)
re-registers a dropped replica by **seeding the fresh member from the recovered `.yrs`
canonical** (`with_hub_seeded_from_file`) and then **union-merging** the editor's update.
If that recovered canonical carries drift (a line the operator already deleted, e.g.
`#sy71`) and the editor's incremental update has no counterpart *delete* op, the union
**keeps the drift → resurrection**. That is the acute wedge (steady-state is handled — the
heal fires: `crdt_replica_reattach_on_update` observed live 06:34:06).

**Fix (owner directive): on reattach, re-baseline the canonical to authoritative content in
priority `editor buffer → disk → committed HEAD`, and do it ALL via lazily on the hotpath —
NO live-buffer sidecar read** (`read_live_buffer_snapshot`/`durable_buffer_state` sidecar is
telemetry, not authority; owner: "it should be all lazily on the hotpath").
- **Editor-buffer tier = the editor's lazily replica state, not the sidecar file.** The
  catch: the editor's `update` param is *incremental against the (drifted) canonical SV*, so
  it does NOT carry a delete for drift the editor never had. To make the editor authoritative
  you need its **full lazily state** on reattach — either the editor ships a full-state
  bootstrap (`delta_since(∅)`) or the hub requests a snapshot (lazily `ResyncCoordinator`
  gap→snapshot). Then re-baseline the canonical to that.
- **LINEAGE CAVEAT (do not ship the naive version):** `hub.apply_disk_change(text)` /
  `TextCrdt::from_str(text)` **mint fresh `OpId`s**, so union-merging them with the editor's
  lazily ops (different lineage) **duplicates**. Disk/HEAD tiers therefore only work as a
  clean baseline when there is NO live editor lazily lineage to preserve; when there is, the
  editor's lazily state must be adopted lineage-intact (its `OpId`s), not rebuilt from text.
  This is the same lineage trap that made text-level reconciliation re-add `#sy71` (Finding:
  a text union of "canonical has sy71" + "buffer lacks sy71" re-adds sy71).
- Implementation site: the `reattached` branch in `relay_replica_update_for_file`
  (crdt-relay-io:1155-1162). Keep it dep-clean (crdt-relay-io deps `agent-doc-fs` + git via a
  small helper; the editor lazily state comes through the existing FFI update/resync path, not
  a new sidecar reader). Test headless: a hub with a drifted canonical + a reattaching editor
  whose lazily state lacks the drift → assert the drift is dropped and nothing duplicates.
- **DO NOT use `pending_rebootstrap` for this** (verified `crdt_relay.rs:903/938/958-968`):
  it pushes the **canonical → editors** (editors REPLACE their buffer with `rebootstrap_text()`
  = the canonical), which is the WRONG direction and would *overwrite the operator's unsaved
  edits*. `apply_disk_change(disk)` rebuilds the canonical from disk (drops drift ✓) but also
  flags editors to rebootstrap-to-disk (loses unsaved edits ✗) — so it's only safe when the
  editor has no unsaved edits (disk == editor buffer, `no_unsaved_operator_edits`).
- **The correct fix needs editor → canonical adoption, lineage-intact:** on reattach the
  editor/FFI must re-announce its **full** lazily state (`ReplicaState::diff(∅)` = its whole
  `Vec<TextOp>` with the editor's `OpId`s), and the hub adopts that AS the canonical (replace
  the drifted one, keep the editor's lineage). That is a small **FFI/plugin** change (editor
  ships a full-state frame on reattach, or the hub sets a "next update is authoritative
  full-state" flag and requests it) + the hub-side adopt. Needs the live Idea/plugin to verify
  — this is the one piece that genuinely can't be finished purely headless. Reuse the
  disk/HEAD tiers only as the baseline when `no_unsaved_operator_edits` (safe, lineage-moot).

**Codex's remaining work, in order:**
1. **P2c — plugin push** (JetBrains/lazily-kt + VS Code/lazily-js): bootstrap a replica from
   the canonical snapshot, send `delta_since` frames as `document_op` frames over the
   reliable-sync RPC **with `file` set** (so P2b's routing folds them). Back the push with the
   durable outbox (Foundation B). This is what actually starts feeding the canonical live.
2. **P3 — authority flip**: stop serving the frozen canonical. In
   `agent-doc-document-realtime-io/src/lib.rs` (~1479–1541) the `live_editors==0`
   `KeepEditorAuthority` branch returns the (now continuously-fed) canonical `text` — once
   P2c feeds it, that's correct; verify + drop the write-path `live_count()==0` demotion
   (`agent-doc-crdt-relay-io/src/lib.rs:1291`) if redundant. **Live-verify in Idea** that the
   `#sy71` repro can't reproduce.
3. **Foundations** (can run in parallel, needed by P2c's durability): CrdtTree (now optional
   cohesion), **SqliteOutbox as `Outbox<S: OutboxStore>`** across the 5 repos + releases.
4. **Cleanup**: rename stale `.yrs` filenames / "yrs" comments in `crdt_relay.rs`/`crdt_sync.rs`.

Everything below is the full detail. Foundations A/B and releases are unchanged.

## TL;DR of the fix

`live_editors == 0` has two halves:
- **Detection** — a connected editor with a dropped CRDT replica registration reads as
  headless. **DONE** (P0): `observe_editor_open` is plane-primary.
- **Content (root)** — when `live_editors == 0`, the resolve path serves the **relay
  canonical**, which is frozen-stale because no replica fed it; the operator's edits sat
  in a passive sidecar. Text-level reconciliation CANNOT fix this (a union merge of
  "canonical has sy71" + "buffer lacks sy71" re-adds sy71 — you need the operator's
  *delete op*). **Only document-op replication fixes it**: the connected plugin
  continuously, durably replicates its CRDT ops into the canonical.

The foundation for that is two lazily primitives the owner has directed: a durable
**`SqliteOutbox`** (`DurableOutbox` impl) and a **`CrdtTree`** trait (to unify the two
CRDT substrates — see "Substrate split" below). Build those in lazily first, then wire
agent-doc P2/P3/P4.

## DONE and verified (do not redo)

- **Immediate incident:** `#sy71` purged from all authority layers, converged to the
  operator's authored queue, committed `40fac79c3` (in `tasks/professional/equityfundingsource.md`), verified stable.
- **P0 detection:** `agent-doc/agent-doc-document-realtime-io/src/lib.rs` `observe_editor_open`
  is now plane-primary — reads `agent_doc_reliable_sync_io::plane_editor_live_for_path`
  (lazily `OrSet` liveness plane) first, cold-miss falls back to the `editor_open_docs`
  registry + `plugin-owner` lease. `make check` green.
- **P1 mechanism (SimWorld-proven, not yet wired live):**
  `agent-doc/agent-doc-reliable-sync-io/src/document_op.rs` — `encode/decode_document_op_frame`
  (a `Vec<lazily::TextOp>` carried as a `CrdtSync` frame on `DOCUMENT_OP_NODE = NodeId(u64::MAX-1)`,
  mirrors `liveness.rs`), and `CanonicalFold` (folds frames into a `TextCrdt`). 4 tests pass,
  incl. `canonical_converges_to_editor_across_disconnect_reconnect_zero_loss` (real
  `SyncDriver` + disconnect + reconnect replay + recycle rebuild, zero loss). `CrdtDoc`
  gained `version_vector`/`delta_since`/`apply_delta` (`agent-doc/agent-doc-merge/src/crdt.rs`).
  Full `make check` green (7516 tests). Cargo change: `agent-doc-reliable-sync-io/Cargo.toml`
  lazily features now include `"serde"`.
- **P2a — hub-level canonical ingest (DONE + tested, this session):**
  `RelayHub::apply_document_op_delta(&mut self, delta: &[u8]) -> Result<BroadcastPacket>`
  in `agent-doc-document-realtime/src/crdt_relay.rs` — folds a document-op delta
  (`serde_json` `Vec<TextOp>`) straight into `self.canonical` **without a registered
  member**, broadcasting the delta to live members. Fixes the freeze root: `relay_update`
  only reaches the canonical via a live member, so `live_editors==0` starved it. Test
  `apply_document_op_delta_feeds_canonical_with_no_live_editors` (same file) proves the
  canonical converges to the operator's edit at `live_count()==0` and is idempotent —
  passing. Wrapper `apply_document_op_delta_for_file(file, delta)` added to
  `agent-doc-crdt-relay-io/src/lib.rs` (next to `adopt_authoritative_text_for_file`),
  compiles. **Not yet run through full `make check`** — do that next.

## Key architectural findings (verified — these gate the design)

1. **~~Substrate split~~ CORRECTED 2026-07-13 — there is NO split.** Earlier notes
   claimed the relay hub was yrs-based; that was a misread of **stale "yrs" comments +
   the `.yrs` filename**. The relay `ReplicaState` is defined in
   `agent-doc/agent-doc-merge/src/crdt_sync.rs:40` and is **already `lazily::TextCrdt`**
   (`use lazily::{TextCrdt, TextOp, TextVersionVector}`; field `text: RefCell<TextCrdt>`;
   doc: *"Backed by a lazily TextCrdt (replacing the former Yjs Doc)"*). There is **no
   yrs dependency** in agent-doc. Its delta wire format IS the `document_op` frame body:
   `state_vector()` = `serde_json(version_vector)`, `diff(sv)` = `serde_json(Vec<TextOp>)`
   = `delta_since`, `apply_update(bytes)` = decode `Vec<TextOp>` + `apply_delta`. So the
   relay canonical already has the exact delta-ingest surface. **"prefer lazily's crdt
   over yrs" is already done.** `CrdtTree` (Foundation A) is now an *optional cohesion
   refactor*, NOT a blocker for P2. Residual cleanup: rename the stale `.yrs` filenames /
   "yrs" comments in `crdt_relay.rs`/`crdt_sync.rs` to lazily terms.
2. **Relay canonical mutation is whole-text CAS**, not delta-apply:
   `crdt_relay.rs` `apply_canonical_replace(expected_current, content)` (~line 550) —
   deletes all + inserts `content`. To feed the canonical with replicated *ops*, add a
   delta-apply on `ReplicaState` (after substrate unification, `apply_delta(ops)`).
3. **Liveness plane already exists and is authoritative** (`AGENT_DOC_RELIABLE_SYNC_AUTHORITY`
   default-on): `agent-doc/agent-doc-reliable-sync-io/src/{plane.rs,liveness.rs,push.rs}`
   — `ControllerLivenessPlane` (OrSet open-set), `plane_editor_live_for_path`,
   `LivenessPushEndpoint` (durable, backpressured via `DurableOutbox`), `dual_run_enabled`.
   **The document-op plane must mirror this rollout exactly** (shadow dual-run → parity
   oracle → authority flip → retire sidecar).
4. **Controller RPC ingest point:** `agent-doc/agent-doc-controller-io/src/project_controller/rpc.rs`
   `handle_reliable_sync` (dispatched at `"reliable_sync" =>` ~line 6952) decodes the
   envelope and `plane.ingest`s liveness frames. Document-op frames must route here too
   (decode via `document_op::decode_document_op_frame` → fold into the relay canonical).
5. **Decision sites still gating on frozen canonical** (fix in P3): the `live_editors == 0`
   resolve branch `agent-doc-document-realtime-io/src/lib.rs` (~1479–1541, `KeepEditorAuthority`
   returns the frozen `text`), and the write-path guard `agent-doc-crdt-relay-io/src/lib.rs:1291`
   (`hub.live_count() == 0` → disk authority).
6. **RelayCell + Merge algebra** (`lazily-rs/src/{merge.rs,relay.rs,relay_transport.rs}`)
   is the principled substrate: `MergePolicy`/`CrdtJoin`/`Context::merge_cell` for a
   **reactive** canonical (authority reads observe it, free idempotent dedup), and
   `RelayCell`+`BackpressurePolicy`+`Transport` for the backpressured egress. Adopt in P3.
   Note: `TextCrdt`/`CrdtDoc` are NOT `CellCrdt` yet — `CrdtTree` (below) should carry the
   join so `CrdtJoin`/`MergeCell` work over it.

## Foundation A — `CrdtTree` trait in lazily (do first; unblocks substrate unification)

Goal: one lazily CRDT abstraction the relay hub, snapshot layer, and document-op frames
all share, so `CrdtDoc` implements it and the yrs/TextCrdt split disappears.

- **lazily-rs** (`src/`): define `pub trait CrdtTree` — a lossless, mergeable document
  tree over the existing `TextCrdt` + distributed clock (see the `lossless-tree` feature,
  `#lzlosstree`, already present). Surface: `version_vector()`, `delta_since(&vv) -> Delta`,
  `apply_delta(&Delta) -> bool`, `text()/value()`, `merge_from(&Self) -> bool` (align with
  the existing `CellCrdt` trait, `src/crdt.rs:211`, so `CrdtJoin`/`MergeCell` compose).
  Provide a blanket or concrete impl for `TextCrdt`. Add law tests (commutative,
  associative, idempotent; delta round-trip; snapshot = `delta_since(&empty)`).
- **agent-doc-merge**: make `CrdtDoc` implement `CrdtTree` (it already wraps `TextCrdt`;
  the new methods added in P1 are the seam).
- **lazily-spec / lazily-formal**: spec the `CrdtTree` contract + conformance fixtures;
  formal model the three algebraic laws.
- **lazily-kt / lazily-js**: implement `CrdtTree` over their `TextCrdt` equivalents (kt/js
  reached parity — mirror the rs surface).
- Features table + release/publish (see "Release" below).

## Foundation B — `SqliteOutbox` as a lazily `DurableOutbox` feature (do in parallel with A)

The impl already exists at `agent-doc/agent-doc-sqlite/src/reliable_sync_outbox.rs`
(implements `lazily::DurableOutbox`: `append`/`ack_through`/`replay_from`/`retained_epochs`;
two SQLite tables `reliable_sync_outbox(document_hash,epoch,frame_json)` +
`reliable_sync_outbox_cursor(document_hash,acked_through)`; per-`document_hash` channel;
infallible trait methods log to stderr). **Extract it into lazily, per binding, behind a
feature** — because the plugin push (kt/js) needs durable crash-replay and must not
re-derive it.

**Naming/factoring decision (settled 2026-07-13):** Keep the name `DurableOutbox` — do
NOT rename to `DurableRelay`. `RelayCell` (in-proc backpressure/coalesce) and
`DurableOutbox` (durable retain-until-acked replay log) are orthogonal and compose
(`RelayCell → SyncDriver → DurableOutbox`); "outbox" is the precise transactional-outbox
term (the outbox doesn't forward — `SyncDriver` does). **Split protocol from storage via
a generic**, so the ack-prune/replay/cursor protocol is written+law-tested ONCE and each
binding ships a tiny storage impl:

```rust
pub trait DurableOutbox { /* unchanged — SyncDriver's contract, src/reliable_sync.rs:151 */ }
pub trait OutboxStore {                       // dumb key-range CRUD, no protocol
    fn put(&mut self, epoch: u64, frame: &[u8]);
    fn delete_through(&mut self, epoch: u64);            // prune ≤ cursor
    fn scan_after(&self, cursor: u64) -> Vec<(u64, Vec<u8>)>;  // ascending
    fn load_cursor(&self) -> u64;
    fn save_cursor(&mut self, epoch: u64);
}
pub struct Outbox<S: OutboxStore> { store: S, acked_through: u64 }
impl<S: OutboxStore> DurableOutbox for Outbox<S> { /* the one tested impl */ }
pub type InMemoryOutbox = Outbox<InMemoryStore>;   // refactor existing to this
pub type SqliteOutbox   = Outbox<SqliteStore>;     // behind `durable-sqlite`
```
Use `OutboxStore`/`SqliteStore` (not `SqliteDurability`/`DurableRelay<SqliteDurability>`):
durability is the *property* the outbox provides; the pluggable thing is the *store*.
If a genuinely durable backpressure buffer is later wanted, that's `DurableRelayCell` =
`RelayCell` composed *with* an `Outbox`, not a rename.

- **lazily-rs**: add optional `rusqlite` dep + feature
  `durable-sqlite = ["ipc", "dep:serde_json", "dep:rusqlite"]` (needs `IpcMessage: Serialize`
  → `ipc`/`serde`, and `serde_json`). Add `src/outbox.rs` with the `OutboxStore` trait +
  `Outbox<S>` (moves the protocol out of today's `InMemoryOutbox`) and law-tests
  (append-before-send, retain-until-ack, `scan_after(cursor)` ascending, prune ≤cursor,
  crash-recovery via reload cursor). `SqliteStore` (in `src/durable_sqlite.rs`) is the
  only SQL — port it from `agent-doc-sqlite/src/reliable_sync_outbox.rs` (the two tables
  `reliable_sync_outbox`/`reliable_sync_outbox_cursor`). Refactor `InMemoryOutbox` to
  `Outbox<InMemoryStore>`. Keep the `DurableOutbox` trait signature so `SyncDriver` is
  untouched.
- **agent-doc-sqlite**: retire its `SqliteOutbox` → re-export `lazily::SqliteOutbox`
  (= `Outbox<SqliteStore>`), or keep a thin adapter opening it at agent-doc's DB path.
  Update `agent-doc-sqlite/src/lib.rs` and callers (`grep -rl SqliteOutbox agent-doc-*/src`).
- **lazily-kt**: `RoomStore: OutboxStore` (Room/SQLite) behind a feature; reuse the shared `Outbox` protocol.
- **lazily-js**: `IndexedDbStore: OutboxStore` behind a feature (browser has no SQLite); reuse `Outbox`.
- **lazily-spec / lazily-formal**: spec the durable-outbox contract (durability, at-least-once,
  cursor monotonicity, prune-safety) + conformance; formal model the properties.
- Features table + release/publish.
- **Do NOT** put rusqlite in lazily's default features (WASM/browser can't link it).

## agent-doc wiring (after A+B land) — strangler, per-subsystem, behind dual-run

- **P2 (shadow):** in `handle_reliable_sync` (rpc.rs), also decode `document_op` frames and
  fold into the relay canonical via a new `ReplicaState::apply_delta` (needs Foundation A
  substrate unification). Run in shadow beside `apply_canonical_replace`; log divergence
  (mirror liveness `dual_run`). Add a controller-level SimWorld test: simulated plugin
  frames → canonical converges regardless of `live_count()`.
- **Plugin push:** JetBrains (Kotlin, lazily-kt) + VS Code (TS, lazily-js) push endpoints:
  bootstrap replica from canonical snapshot (shared OpIds), send `delta_since` over the
  reliable-sync RPC (`push_reliable_sync_liveness` sibling), backed by the durable outbox.
  Thin plugins — durability/backpressure stay in shared FFI/Rust.
- **P3 (authority flip):** canonical served from the doc-op plane; remove the frozen-canonical
  serve in the `live_editors == 0` resolve branch and the write-path `live_count()==0` guard.
  Adopt `MergeCell`/`RelayCell` (finding 6). **Done-bar:** the `#sy71` repro cannot reproduce
  in a live session with the editor open (needs live verify — owner will watch in Idea).
- **P4:** retire the plugin-owner lease cold-miss + live-buffer sidecar reconcile (durability
  only). `audit-docs` + `make check` + tmux CI green.

## Release / publish (per repo — see memory refs)

- **crates.io** (lazily-rs): OIDC trusted publishing configured for most crates; new crates
  need registration. Fallback token: `CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)"`.
  Bump versions lockstep; update the release.yml publish lists.
- **lazily-kt** → GitHub Packages on `v*` tag — **immutable**: check the registry before
  tagging (409 kills publish). Bundle conformance fixtures into `src/test/resources/`.
- **lazily-js** → npm (btakita 2FA/passkey; watch the OIDC E404-on-PUT if trusted-publishing
  isn't configured on the pkg).
- **lazily-spec/formal**: version + conformance fixtures; update BENCHMARKS.md / features
  parity table with the new `CrdtTree` and `durable-sqlite` rows.
- Bump superproject gitlinks after each submodule release.

## Guardrails (owner's standing request — apply them)

Strangler not big-bang (cut over per-subsystem behind the dual-run seam; delete old paths).
Spike-before-commit for unknown corners. Conviction gates as code-enforced checks, not
comments. **Behavioral done-bar**: one normal live session with zero recovery interventions
and no `#sy71`-class resurrection — not structural completeness. Cap refactor time so
revenue work isn't hostage. Foundations A+B are independently shippable and reversible;
don't flip agent-doc authority (P3) until the parity oracle + dual-run prove zero divergence.

## Verify commands

- lazily-rs: `cargo test -p lazily --features durable-sqlite` / `--features lossless-tree`.
- agent-doc: `make check` (from `src/agent-doc`); `gh run list --workflow CI --limit 1`;
`make tmux-ci` for the tmux leg.
- Existing plan (superseded/absorbed here): `src/agent-doc/tasks/agent-doc/plan-document-op-replication-never-frozen-canonical.md`.

## Codex full-parity closeout (2026-07-13)

The stale-handle cursor regression is fixed at both layers of the durable-outbox contract:

- `OutboxStore.save_cursor(epoch)` now persists `max(stored_cursor, epoch)` atomically.
- Shared outbox wrappers refresh the persisted cursor before acknowledgement, replay, and
  retention decisions, so a long-lived stale handle cannot regress or reason from an old cursor.
- The canonical conformance fixture now opens two handles, advances one to epoch 9, attempts
  to save epoch 3 through the stale handle, reopens the store, and requires epoch 9.
- `lazily-formal` proves the corresponding `stale_save_cannot_regress` property.

Full CrdtTree and durable-outbox parity was implemented and verified across every maintained
implementation:

| Implementation | Version | Persistent store | Release state |
| --- | --- | --- | --- |
| Rust | 0.38.1 | SQLite | tagged, pushed, and published to crates.io |
| Python | 0.28.1 | SQLite | tagged and pushed; package built; PyPI token required to upload |
| Kotlin | 0.27.1 | Room/SQLite | tagged, pushed, and published to GitHub Packages |
| JavaScript | 0.17.1 | IndexedDB | tagged and pushed; npm trusted-publisher/passkey action required |
| Dart | 0.17.1 | append-only file | tagged, pushed, and published to pub.dev |
| Zig | 0.22.1 | file | tagged and pushed |
| Go | 0.13.1 | file | tagged and pushed |
| C++ | 0.16.1 | file | tagged and pushed |

`lazily-spec` 0.32.1 is the canonical contract and fixture release. `lazily-formal` 0.32.1
contains the explicit stale-save theorem. A final clean-checkout parity pass reran every
maintained repository's native full gate on 2026-07-13:

- spec: `make check` (120 schema/conformance tests, the 9-job Lean model, and sibling
  coverage synchronization); formal: `make check` (38 Lean jobs);
- Rust: `make check`, including every feature/conformance/formal/network/benchmark target,
  plus `cargo publish --dry-run --locked`;
- Python: `make check` plus `uv build`; Kotlin: `make check` plus `./gradlew assemble`;
- JavaScript: `make check` plus `npm pack --dry-run`; Dart: `dart analyze && dart test`
  (364 tests) plus `dart pub publish --dry-run`;
- Zig: `make check`; Go: `make check && make race`; C++: `make check` (20 CTest targets).

Every gate and package dry-run passed. The canonical outbox-store fixture has an identical
SHA-256 (`23f7080e...02d61e1`) in all eight mirrored bindings; Rust consumes the canonical
spec fixture directly. All ten repositories (spec, formal, and eight bindings) finish clean
and synchronized with `origin/main`. Latest configured CI is green for spec, Rust, Kotlin,
Dart, Go, and C++; formal and Zig have no workflow. Python's repository workflow is manual
and its last run is green. JavaScript's tagged Release built and signed provenance, then npm
returned the known scope authorization 404; the local full gate and tarball dry-run are green.

The agent-doc integration landed initially in commits `0ea39300` and `8b34046b` at version
0.34.87: document replication is durable and feedback-safe, the lazily-backed SQLite outbox
is wired through the dependency graph, and the publication dependency cycle is broken.
The P3 authority flip is enabled by default, and the 0.34.88 P4 closeout adds a
commit-before-ACK receiver journal, monotone receive cursors, controller-start/cold-reader
rehydration, and plugin startup enumeration. Default authority and write-convergence reads
no longer consult plugin-owner leases or demote a durably open editor merely because relay
membership is zero; the explicit authority-off environment switch remains as rollback.

The named stale `.yrs` cleanup is also complete in the intended lazily-backed
`crdt_relay.rs` / `crdt_sync.rs` files. Remaining `.yrs` references belong to separate
legacy/overlay state formats with an explicit `.seqcrdt` migration plan and are not renamed
without that compatibility migration.

Remaining release operations are external credential/rate gates, not parity gaps: PyPI needs
an upload token, npm needs the package's trusted-publisher/passkey configuration corrected,
and crates.io is rate-limiting the incremental first publication of agent-doc's decomposed
crates. The idempotent `make publish-crate` resumes from the first unpublished crate.
