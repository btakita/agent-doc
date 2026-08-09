---
status: approved
phase: pending-implementation
owner: btakita
---

# Backlog System — Stable IDs, Checkboxes, Gated Lifecycle, Granular Ops

## Status

**Approved.** Design agreed in `tasks/agent-doc/agent-doc-bugs.md` exchange.

Terminology rule: `backlog` is the canonical user-facing name. `pending` remains
only as a legacy component/CLI/module compatibility term. New generated
commands, closeout hints, specs, and runbooks should say `--backlog-*` and
`agent:backlog`. Some older `--pending-*` spellings remain aliases while their
own migrations are active, but tracked-work completion now uses `--done` only.

## Problem

The `agent:backlog` component drifts between cycles. Historical full-replace
backlog blocks (`patch:pending`, later `replace:pending`) were lossy: text drift
and reorder both destroy identity, so `--done 3` (numeric index) is unsafe and
`--done "text"` (exact match) is fragile. Current enforcement rejects outgoing
full-replacement backlog payloads and requires granular tracked-work flags.

Three symptoms observed in practice:

1. **Drift** — items renamed slightly between cycles; the skill loses track of which line it previously saw.
2. **Silent full-replace** — the runbook encourages rewriting the whole component, so one mistake wipes out user edits that landed mid-cycle.
3. **No explicit "done" signal from the user** — the only way a human can mark an item done is to delete the bullet, which looks identical to "accidentally removed" in a diff.

## Design

Four cooperating primitives. Each solves one of the symptoms above.

### 1. Stable hash IDs

Every bullet in `agent:backlog` carries a 4-char base32 hash as a visible prefix:

```
- [ ] [#a3f2] refactor preflight commit path
- [ ] [#b1c4] fix boundary repositioning off-by-one
```

- Generated on first insert (via `--backlog-add`, legacy `--pending-add`)
  unless the caller explicitly
  provides a custom id with canonical `id=<custom> ` syntax; leading
  `[#custom] ` is accepted as compatibility input and normalized to the same
  custom id. Custom ids are non-empty ASCII alphanumeric strings with optional
  hyphens. Lazy backfill still generates IDs.
- **Mutation-time collision rejection (`#preset-item-id-collision-enforce`):** an
  **explicit** custom id (`id=<id>` / `[#id]`) passed to `--backlog-add` /
  `--backlog-add-after` / `--backlog-add-before` / `--backlog-add-back` /
  `--backlog-add-to` (or legacy `--pending-*` aliases) fails closed when it
  collides with a frontmatter
  `prompt_presets` key or an existing active `agent:backlog` / `agent:review` /
  `agent:icebox` item id, so a new ambiguous `#id` is never written. Auto-id adds
  (no explicit prefix) are never blocked. Dispatch-time enforcement on a
  *pre-existing* collision stays a preflight warning (`preset_item_id_collision`)
  rather than a hard block, to avoid over-blocking live sessions.
- **An inferred id is not an explicit one (`#baretagidcollide`):** only `id=<id>`
  and `[#id]` are explicit requests. A leading bare `#tag` is an *inference* the
  add path makes so `[operator-verify] #someid text` keeps `#someid` instead of
  deriving a slug from the tag's own words. When that inferred id is already
  active, it means the token was a classification tag rather than an id: the add
  keeps the tag in the item text and takes a generated id. It must never fail
  the turn, per the auto-id rule above. Treating the inference as an explicit
  request made a document-wide tag single-use — the first
  `--backlog-add "#agent-doc-bug ..."` claimed the id and every later one failed
  closed. The same degrade applies to `extract_inline_tag_id`.
- **Mutations validate before the response is published (`#prmergeguardpr`):**
  closeout runs the tracked-work mutation envelope twice. The first pass is a
  dry run — the identical mutation code executed against virtual document
  content inside `with_pending_write_transaction_dry_run`, with every buffered
  document/archive target discarded and no cycle state, queue projection, or
  snapshot checkpoint recorded. It runs **before** the response cell is written,
  so a rejectable flag set (an explicit id colliding per the rule above, a
  malformed `id=text` pair, a `--done` naming an absent item) fails the turn with
  nothing applied. Without it, a mutation-validation error alone could split the
  closeout transaction after the response was already durable, leaving the
  operator reading a response that claims an item is done beside a backlog that
  still shows it open, recoverable only by a manual owning-pane commit. The
  second pass applies and publishes the envelope as before. A tracked-work
  failure that survives the dry run is a delivery/projection failure, and its
  error distinguishes "response landed, mutations pending" from "whole write
  pending" so the two recovery paths are not confused.
- Stable across reorders, text edits, and cycles.
- Visible in rendered markdown (like a GitHub issue number) — no hidden state.
- Opaque: the hash is not meaningful, just unique within the component.

Hash generation: `base32(blake3(text + doc_id + monotonic_counter))[:4]`. Collision handling: on collision within the component, extend to 5 chars and retry; documented ceiling at 8 chars.

### 2. GFM task-list checkboxes with review lifecycle state

Every bullet carries a GFM-style checkbox that encodes a three-state lifecycle:

```
- [ ] [#a3f2] open — not started or in progress
- [/] [#b1c4] review — code-complete, awaiting release / telemetry / human review
- [x] [#c9e0] done — reaped next preflight cycle
```

**Lifecycle diagram:**

```
┌─────┐   --backlog-gate     ┌─────┐   --done     ┌─────┐
│ [ ] │ ───────────────────► │ [/] │ ───────────────────► │ [x] │ ──► (reaped)
│backlog                     │review                     │ done│
└─────┘                      └─────┘                      └─────┘
   ▲                            │
   │     --backlog-ungate       │
   └────────────────────────────┘

Direct path (no gating needed):
[ ] ──── --done ────► [x] ──► (reaped)
```

**State and container semantics:**

| State | Char | Meaning | Reaped? |
|-------|------|---------|---------|
| `Open` | `[ ]` | active work or not started | No |
| `Gated` | `[/]` | code-complete, awaiting external gate or human review | **No** |
| `Done` | `[x]` | fully complete; user or agent signals reap | Yes (next preflight) |

`agent:backlog` holds open work, `agent:review` holds gated review work, and
`agent:icebox` holds parked tracked work. Legacy documents may still have `[/]`
items inside `agent:backlog`; preflight reports `legacy_gated_in_backlog` and
`agent-doc migrate` moves those items into `agent:review`.

**Why three states instead of a prose suffix:**

- **Machine-readable.** Preflight emits `backlog_gated_count`, `review_count`, and `review_gated_count`; release workflow can query gated items programmatically.
- **Prevents accidental reap.** A `[/]` item is explicitly not reapable — releasing v0.32.5 cannot prematurely erase `#a002` just because its prose said "landed."
- **Matches observed reality.** `code-landed ≠ done` is how the agent-doc project is actually run. Encoding it in the data model stops every response cycle from re-explaining "why isn't this checked."

**Gate name is free-text prose, not structured:**

```
- [/] [#id] text — gate: v0.32.5
- [/] [#id] text — awaiting normalize_threshold_exceeded telemetry
```

Gate names live in the bullet text suffix. No parser support for a structured gate field (YAGNI — revisit only if we want programmatic queries by gate).

**Why GFM `[ ]` / `[/]` / `[x]` and not unicode checkboxes or a prose suffix:**

- Standard GFM syntax for `[ ]` / `[x]`; `[/]` is recognized by Obsidian and Logseq as "in progress" so muscle memory maps.
- Renders natively in GitHub, VS Code, JetBrains, Obsidian (though `[/]` shows as literal text in stock GFM renderers — acceptable cost, agent-doc docs live in editors).
- User can toggle in GFM-aware editors.
- Grep-friendly (`- \[/\]`, `- \[x\]`).
- Monospace-safe; unicode `☑`/`☒` break alignment in some fonts.
- Three single-char states keep visual density tight vs. two-checkbox `[x][ ]` or suffix conventions.

### 3. Lazy backfill in preflight

Preflight is the single lazy-backfill and reap path. Structural migrations that
move legacy gated backlog items into `agent:review` live in `agent-doc migrate`.

On every preflight run:

1. Scan tracked-work components (`agent:backlog` / legacy `agent:pending`,
   `agent:review`, and `agent:icebox`) for list items.
2. For each bullet:
   - Content-less bullet (no description text **and** no continuation, e.g. a
     stray `- [ ]` or an id-only `- [ ] [#hash]`) → **drop it**. Backfill must
     never mint a hash id for an empty line, because that manufactures a phantom
     tracked item whose description "disappeared" (`#icebox-empty-item-phantom-id`).
     Dropping also self-heals an already-cemented id-only empty item. A bullet
     with empty header text but a real indented continuation is **not**
     content-less and is preserved.
   - No hash prefix → generate and insert a hash.
   - No checkbox → insert `- [ ] ` before the hash.
   - Legacy `[~]` task markers are normalized to open `[ ]` while preserving
     any following `[#id]`. Backfill must never prepend a fresh `[ ] [#new]`
     ahead of an existing `[~] [#id]` line.
3. Reap `- [x]` bullets **only**:
   - If a completed item still lacks an id (legacy/manual form such as `- [x] shipped` or `- [x] [#] shipped`), backfill its hash first, then reap/archive that canonicalized item in the same pass. Reap must never silently drop a done line that cannot be referenced in the archive or logs.
   - Remove the line from the component.
   - `[ ]` and `[/]` are never auto-reaped. `[/]` explicitly survives forever until an operator moves it to `[x]`. No TTL on gated state — the operator owns the gate.
   - Append an archive entry to canonical `agent:done`. If the component does not
     already exist, create a visible `## Completed / Reaped` section after the
     tracked work components first, then append the entry. If the opening marker
     uses `archive=<repo-relative>.done.md`, append to that external markdown
     file instead, create it when missing, leave the local `agent:done` component
     as the routing marker, and suppress duplicate date/id/text entries on
     retry. Absolute paths, parent-directory escapes, outside-repo targets, and
     non-`.done.md` targets fail closed before active tracked work is mutated.
     Legacy `agent:backlog-done` and `agent:pending-done` components are not
     accepted as archive aliases; run `agent-doc migrate` to rewrite them to
     `agent:done`. Completed tracked work must remain grep-visible in either the
     session document or its explicit external done archive instead of
     disappearing from live tracked work without a local record.
   - Persistence invariant: the reap must land in both the working tree document and the snapshot that the commit boundary stages. If preflight cannot persist that synchronized reap safely, it must fail closed instead of continuing with completed tracked-work items still present in backlog, review, or icebox.
   - Snapshot-sync invariant (`#pending-gate-snapshot-desync`): closeout tracked-work maintenance must re-sync the snapshot's tracked-work surfaces to the working-tree document whenever they diverge — even when maintenance itself performed no reap or backfill. The historical failure was a write phase that persisted `--backlog-gate` / `--backlog-edit` / `--review-add` mutations to the document but saved a stale agent-owned `content_ours` snapshot before those mutations, so without the re-sync the snapshot lagged, the commit staged `snapshot == HEAD`, and the mutation was stranded as uncommitted post-commit drift (`--done` avoided this only because reap already triggered a snapshot rewrite). Current closeout must snapshot the verified source-of-truth tracked-work state, not a stale `content_ours` image. Reorder detection still compares the file against the **cycle-start** snapshot, not the re-synced one, so a same-cycle reorder is not masked.
- The standalone `agent-doc backlog <file> reap` command follows the same
  visibility rule for direct maintenance: it removes completed items from live
  tracked work, creates `agent:done` when needed, and appends each removed item
  there or to its explicit `archive=...done.md` target instead of silently
  deleting it.
- The standalone `agent-doc backlog <file> reopen <id> [--queue]` command is
  the only supported completed-history reversal. It atomically removes every
  same-id entry from inline or external `agent:done`, restores the newest
  archived text and continuation to open backlog state under the original id,
  and, with `--queue`, replaces any same-id struck queue history with exactly
  one live directive. Ordinary backlog add/sync remains monotonic and must not
  resurrect an archived id implicitly.
- Same-cycle resurrection invariant: once a cycle reaps a tracked `[#id]`, closeout must fail closed if that same id reappears in live tracked work before commit. Do not silently treat the stale rewrite as generic local drift.
- Same-cycle completion invariant: when preflight/repair reap a user-authored `[x]` tracked item directly from the document, that id counts as intentionally resolved for the current cycle's history-replay guards even if no explicit `--done <id>` flag was recorded. Do not restore the older `[ ]` or `[/]` history entry just because the completion came from a manual document edit.
- External archive invariant: preflight and session-check must treat IDs found
in the `agent:done archive=...done.md` target as completed-history proof for
backlog replay and as known identifiers for the coined-ID guard after inline
history is reaped. Invalid archive targets fail closed instead of being ignored.
- No-partial-reap invariant: if a completed tracked item is followed by malformed flush-left spill such as pasted command/diff transcript lines, reap/archive the whole logical block with that parent item. Do not delete only the tracked parent line and leave orphan prose behind in the live backlog.
4. Commit the rewritten component as part of the existing boundary-maintenance commit.

**Migration of existing items:** `agent-doc migrate` is deterministic only: it
moves already-explicit `[/]` items from `agent:backlog` into `agent:review` and
inserts the review block when missing. It does not auto-classify prose such as
"landed", "shipped", or "awaiting release"; those remain `[ ]` until touched
manually via `--backlog-gate` (legacy `--pending-gate`).

A doc that never gets opened again never migrates — fine, because IDs only matter when the skill/runbook is actively managing the list.

**Concurrent-open edge case:** Two sessions open the same doc before either commits backfill. Both assign different hashes to the same bullet. CRDT merge picks one. Acceptable — IDs are opaque and bullet text is unchanged.

### 4. Granular write-command surface

The skill/runbook **never** writes a full `replace:backlog`, `replace:icebox`,
or `replace:pending` block. Full-replace is
forbidden for tracked-work lists. All mutations go through explicit flags on
`agent-doc write` / `agent-doc finalize`:

| Flag | Legacy alias | Behavior |
|------|--------------|----------|
| `--backlog-add "text"` | `--pending-add` | Add new item at the beginning of the backlog. Binary assigns hash and `[ ]` unless the text starts with canonical `id=<custom> ` syntax. Leading `[#custom] ` is accepted as compatibility input. When repeated in one command, all added items are inserted as one ordered batch: the first flag appears above the second, and the full batch appears above existing backlog items. |
| `--backlog-add-to <file> "text"` | `--pending-add-to` | Add a new `[ ]` item to another document's backlog. The target file must exist and contain an `agent:backlog` / legacy `agent:pending` component; missing targets fail closed instead of falling back to the current document. Repeated pairs are grouped per target and preserve caller order at the top of each target backlog. |
| `--backlog-add-after <id> "text"` | `--pending-add-after` | `#ah0s`: insert a new `[ ]` item immediately **after** an existing backlog item, by id. Repeatable `ID TEXT` pairs; chaining `--backlog-add-after A "B" --backlog-add-after B "C"` builds A->B->C deterministically (no follow-up `--backlog-reorder`). Errors if the anchor id is absent. Applied after the front-insert default so an anchor added earlier in the same cycle resolves. |
| `--backlog-add-before <id> "text"` | `--pending-add-before` | `#ah0s`: symmetric counterpart — insert immediately **before** the anchor item. |
| `--backlog-add-back "text"` | `--pending-add-back`, `--backlog-append`, `--pending-append` | `#ah0s`: insert at the **end** of the backlog list (before any trailing text), for low-priority captures that should not jump the head. Repeatable. |
| `--icebox-add "text"` | none | Add a parked tracked-work item at the beginning of `agent:icebox` using the same id, checkbox, collision, and writeback rules as backlog adds. Icebox adds do not mirror into `agent:queue`. |
| `--icebox-add-after <id> "text"` | none | Insert a parked item immediately after an existing icebox item. |
| `--icebox-add-before <id> "text"` | none | Insert a parked item immediately before an existing icebox item. |
| `--icebox-add-back "text"` | `--icebox-append` | Insert a parked item at the end of `agent:icebox`. |
| `--icebox-edit "id=new text"` | none | Rewrite parked item text, **preserve hash and state**. Multiline edits replace the item's entire continuation block; lines after the first must be indented continuation content, not new flush-left parent items. The `agent-doc icebox <file> edit <id> <text>` subcommand is the equivalent CLI form. |
| `--icebox-clear` | none | Remove all icebox items. |
| `--icebox-reorder <id1,id2,...>` | none | Reorder parked icebox items by ID. Missing IDs keep their relative order after the listed prefix. |

The backlog is a **priority-ordered pool with id-based consumption** (`--done` / `--backlog-gate` reference `#id`, never position) — not a stack or queue (FIFO execution discipline lives in `agent:queue`). So `--backlog-add` stays the cheap front-insert default for single captures, and the explicit-position flags above make ordered insertion unambiguous when position matters, instead of relying on argv direction.
| `--done <id>` | none | Mark `[x]` in tracked work (`agent:backlog` / legacy `agent:pending`, `agent:review`, or `agent:icebox`) — commit-required closeouts reap it in the same persisted cycle, while preflight / repair also clean up stale completed items. Valid from any state (`[ ]` or `[/]`). If the id is already present in canonical `agent:done` or the current cycle's resolved-id ledger, treat it as an idempotent resolution warning rather than a fatal missing-id error. |
| `--backlog-gate <id>` | `--pending-gate` | Move a backlog item to `agent:review` as `[/]` — code-complete, awaiting review/gate. Valid from `[ ]`. No-op if already in `agent:review`. Error if source is `[x]`. |
| `--backlog-ungate <id>` | `--pending-ungate` | Move an `agent:review` item back to backlog as `[ ]` — review failed, back to active. Legacy gated backlog items still ungate in place until migrated. Error if source is `[ ]` or `[x]`. |
| `--backlog-edit <id> "new text"` | `--pending-edit` | Rewrite text, **preserve hash and state**. Multiline edits replace the item's entire continuation block; lines after the first must be indented continuation content, not new flush-left parent items. |
| `--backlog-clear` | `--pending-clear` | Remove all backlog items. |
| `--backlog-reorder <id1,id2,...>` | `--pending-reorder` | Reorder backlog items by ID. Missing IDs keep their relative order after the listed prefix. |
| `--review-add "text"` | none | Add a new `[/]` item directly to `agent:review`. Rare; normal code-complete flow should use `--backlog-gate`. |
| `--review-edit <id> "new text"` | none | Rewrite text in `agent:review`, preserving hash and state. |
| `--review-resolve <id>` | none | Resolve an `agent:review` item: remove it and archive to `agent:done`. The completion path for finished gated work. Errors if no review component or no matching id. |
| `--review-remove <id>` | none | Delete an `agent:review` item by id, removing **every** entry sharing the id. For stale/duplicate review entries (e.g. the identical `[/]` pair an interleaved finalize leaves behind, flagged `preset_item_id_collision`) that cannot be deduped via an ambiguous edit-by-id. Errors if no review component or no matching id. |

Closeout pending-maintenance (commit-required `finalize` / `write --commit`) also auto-dedupes **identical** same-id review entries (same id, state, gate type, text, continuation) to a single representative; distinct items sharing an id are left intact so the `preset_item_id_collision` ambiguity warning still surfaces.

For every id-based backlog flag except `--backlog-add`, the binary normalizes
the id by trimming whitespace, stripping one optional leading `#`, and
lowercasing before lookup. `--done 4qja` and `--done #4QJA`
must therefore resolve the same tracked item.
`--done` is the only tracked-work completion flag; generated plans and closeout
guidance must not emit removed completion-alias spellings.

`review_done_guard` is a frontmatter/project guard for review-then-done
projects. Default `off` keeps direct backlog-to-done closeouts valid. `warn`
prints a warning when `--done <id>` resolves an item outside `agent:review`;
`strict` (alias `error`) fails that mutation until the same cycle first runs
`--backlog-gate <id>` (legacy alias: `--pending-gate <id>`).

**Plan-backed item rule:** when a backlog bullet depends on a dedicated plan
document, the operator must create that plan file before adding the backlog
item, and the backlog text must include the concrete plan-file path. The
backlog entry is the durable pointer; it should not require later archaeology to
discover which `plan-*.md` file was intended.

**Preset target authority:** prompt-preset text such as `#agent-doc-bug` may
name a cross-document backlog target. When the target path starts with a known
project directory name, such as
`agent-loop/tasks/agent-doc/agent-doc-bugs2.md`, preflight resolves it against
that project root even if the current session document lives in a nested
project with its own `.agent-doc` tree. If the path could match multiple
existing task trees, or if a project-prefixed target does not exist, preflight
fails closed before any backlog item can be written. Preflight output includes
the resolved `explicit_backlog_targets` paths for operator verification.

**Declaration-chain order:** when multiple prompt-bearing changes in one cycle
each invoke `#agent-doc-bug`, `agent-doc plan` treats them as one ordered batch.
The `pending_mutations` entries and repeated `--backlog-add-to` placeholders
must follow declaration order so the first declared bug remains above later
bugs after insertion. The internal field name remains `pending_mutations` for
compatibility; generated command placeholders are canonical `--backlog-add-to`.
If an agent intentionally changes priority, the closeout
must say so explicitly instead of relying on reversed insertion side effects.

**State transition matrix:**

| From \ Op | `--done` | `--backlog-gate` | `--backlog-ungate` |
|-----------|------------------|------------------|--------------------|
| `[ ]` Open  | → `[x]` Done     | → `[/]` Gated    | error              |
| `[/]` Gated | → `[x]` Done     | no-op (log)      | → `[ ]` Open       |
| `[x]` Done  | no-op (log)      | error            | error              |

**Explicitly rejected:** `--backlog-replace` / `--pending-replace`. Every transformation it could express is a sequence of `add` / `done` / `edit` / `reorder`, and the sequence preserves IDs where replace would churn them. Adding it would re-enable the full-replace pattern this spec is trying to eliminate.

### 5. Reorder detection in preflight

Preflight becomes reorder-aware:

1. Extract ordered `[#id]` list from the snapshot's backlog component.
2. Extract same from current document.
3. If ID set unchanged but order differs → **user reordered**. Preflight rewrites the snapshot to match, commits, and emits `backlog_reordered: true` in the JSON output.
4. If the ID set also changed → apply adds/removes (lazy backfill + `[x]` reap) first, then compare remaining IDs' order.
5. When `backlog_reordered: true`, the skill MUST NOT reorder the component in the current cycle — user intent wins for at least one cycle.

### 6. Enforcement

Invert the current rule:

- **Before:** outgoing full-replacement backlog payloads were tolerated during
  migration.
- **After:** presence of `replace:pending` in the outgoing write payload → error.

`agent-doc write` validates this at parse time. The only way to mutate backlog is via the granular flags.

**#25ag rename (v0.32.4):** The block syntax was renamed from `patch:pending` to `replace:pending`. The `replace:` prefix signals full-replacement semantics explicitly and is the canonical form. The migration window has ended: `patch:pending`, `--allow-patch-pending`, and `AGENT_DOC_ALLOW_PATCH_PENDING=1` are no longer accepted. Canonical names: `replace:pending` + `--allow-replace-pending` + `AGENT_DOC_ALLOW_REPLACE_PENDING=1`.

**Thread-safety fix (#envvar1):** The CLI dispatcher no longer uses `unsafe { env::set_var() }` to propagate `--allow-replace-pending` and backlog-add state to downstream write functions. A `WriteFlags` struct is threaded explicitly through the call chain. Env var reads are retained as a backwards-compat fallback for external scripts that set them before invoking `agent-doc write`.

**Component attribute deprecation (v0.33.15):** The `patch=replace` (and legacy `mode=replace`) attribute on `<!-- agent:backlog -->` / `<!-- agent:pending -->` opening tags is deprecated. The backlog component defaults to `replace` mode via the built-in default in `template::default_mode()`, and the binary owns all backlog mutations through `--backlog-*` flags (legacy `--pending-*` aliases) — making the inline attribute redundant. Existing documents are normalized automatically: the write path strips `patch=` and `mode=` from backlog component tags and emits a deprecation warning. New scaffolds omit the attribute.

## Schema — fully-migrated example

```markdown
<!-- agent:backlog -->
### Active
- [ ] [#a3f2] implement --backlog-reorder
- [ ] [#b1c4] rewrite runbook to forbid replace:pending

### Gated
- [/] [#eg0w] per-file CommitLock + freshness gate — gate: v0.32.5 release
- [/] [#a002] normalize_user_prompts safety rail — awaiting large-drift telemetry trip

### Done
- [x] [#c9e0] fix boundary repositioning
<!-- /agent:backlog -->
```

After next preflight: `#c9e0` is reaped, `- [x]` line is removed, commit rolls forward. `#eg0w` and `#a002` remain untouched — they stay `[/]` until an operator explicitly promotes them to `[x]`.

**Header preservation:** `agent:backlog` and `agent:icebox` may contain ordinary markdown headings or blank lines between item groups for organization. Granular tracked-work mutations (`add`, `done`, `edit`, `clear`, `reorder`, `gate`, `ungate`, reap/backfill) must preserve those non-item lines in place. Reordering operates on the item slots only; it does not delete or auto-synthesize headings.

**Ordered parent-item support:** A backlog or icebox may use flush-left ordered parent entries (`1. ...`, `2. ...`) instead of unordered `- ...` bullets. When any tracked parent entry in a component uses ordered style, the binary canonicalizes all tracked parent entries in that component as a single sequential ordered list in current item order. Adds, reorders, done/reap transitions, and selective transfers therefore renumber tracked parents instead of preserving stale ordinals.

**Priority ordering (`#backlog-priority-attribute`):** A backlog or icebox marker may carry a bare `priority` attribute (`<!-- agent:backlog priority -->`). When present, `run_pending_maintenance` stable-sorts that component's tracked items each cycle by their per-item `priority=<1..9>` token (`1` = highest, sorts first; `9` = lowest numbered). An item with no valid `priority=` token ranks below every numbered item and keeps its authored relative order under the stable sort. The token may appear anywhere in the item text (e.g. `- [ ] [#id] priority=2 do the thing`). The sort preserves non-item segments (headings, blank lines) at their positions and is idempotent. Paired with the backlog→queue sync `queue` attribute (see `specs/07-orchestration-commands.md`), a `priority`-sorted backlog yields a prioritized `agent:queue`; if the queue-tagged backlog source also carries `priority`, preflight immediately applies the queue priority/auto-DAG recompute to synced prompts and annotates automatically promoted prompts with `:round_pushpin:`. `agent:icebox priority` still sorts parked work, but `agent:icebox queue` does not auto-populate the active queue. A `priority` attribute on the `agent:queue` marker additionally stable-sorts the queue's `do [#id]` prompts by their source item's priority (covering append-built or manually edited queues). Pure logic: `pending::sort_by_priority` / `pending::item_priority_rank` and `queue::sort_prompts_by_priority`.

**Same-cycle actionable backlog queue sync (`#pendingaddqueuesync`, `#backlogqueuepopulation`):** `--backlog-add*` and `--backlog-ungate` mutations are applied during finalize/write, after preflight's ordinary backlog→queue sync has already run. Successful mutations record normalized ids in `cycle_state.pending_actionable_ids` (separate from `pending_added_ids`, which remains new-add/ops-proof evidence). In an active go-mode queue whose `agent:backlog` carries a recognized `queue` attribute, closeout insert-only mirrors exactly those actionable ids into `agent:queue` after current-head consumption and before commit. The new block is ordered by hard `after=#id` dependencies, then priority; existing queue bytes remain unchanged. Done/already-queued/deferred ids are skipped, and plain persisted-active queues without go retain the amplification guard. Mutation scoping is deliberate: reconciling every open backlog id here would resurrect unrelated queue entries the operator deleted.

**Manual priority pin (`#queue-manual-priority-override`):** Any queue item may be prefixed with a pin marker (`- __prioritized__ do [#id]` or `- __prioritized__ <free text>`). Under a `priority`-attributed `agent:queue`, `queue::sort_prompts_by_priority` treats an **operator** pin as **position-locked** (`#queue-operator-pin-position-lock`): the `priority` attribute never moves an operator-pinned prompt — it stays at the exact slot where the operator placed it, and the unpinned / agent-pinned prompts reorder *around* it (filling only the slots not held by an operator pin). This is the operator's stated requirement: "if I add an item to a certain position with a `:pushpin:`, it remains there." A pin is sticky — it persists across the per-turn recompute because it lives in the document text — and is released simply by deleting the marker, after which the item rejoins the rank-ordered remainder. If an operator manually moves an existing live prompt to a new position in a priority queue, preflight treats that movement as an authored priority override and annotates the moved prompt with the canonical operator pin `:pushpin:` before the priority/auto-DAG recompute (`queue::annotate_operator_priority_reorders`), locking it at that new position. The `sort_prompts_by_dag` path — active only when `after=#id` edges exist — applies the same position-lock (`#queue-operator-pin-position-lock-dag`): operator pins keep their document slots while the movable (agent-pin/unpinned) prompts reorder around them to satisfy the edges, and the blocker-outranks-pin exception still holds (if anchoring would violate a dependency, the dependency wins and the plain dependency topo is used).

**Two pin tiers (`#queue-agent-vs-operator-pin-tier`):** the marker is a markdown-emphasis wrap of the word `prioritized`, so it renders distinctly in the editor and is released by deleting it. The two emphasis tiers have different effects on the `priority` recompute:

- **operator** pin (position-locked) — markdown **strong** emphasis on `pin`/`prioritized` (`**pin**`, `__pin__`, `**prioritized**`, `__prioritized__`), the `:pin:` / `:pushpin:` shortcodes, or the literal 📌 emoji. The `priority` attribute never moves it; it is the operator's explicit "freeze here" marker.
- **agent** pin (floats above unpinned, never above an operator pin's slot) — markdown *italic* emphasis on `pin`/`prioritized` (`*pin*`, `_pin_`, `*prioritized*`, `_prioritized_`), the `:round_pushpin:` shortcode, or the literal 📍 emoji. The agent uses it to surface auto-promoted prompts; it sorts ahead of the unpinned tail among the slots not held by an operator pin.

Both asterisk and underscore spellings of each emphasis level are accepted (markdown treats them identically), and the pushpin shortcodes/emoji alias the operator / agent markers, so toggling spelling does not change the tier.

**Auto-dag dependency ordering (`#queue-auto-dag-priority`):** an item may declare hard dependencies with an `after=#id` token in its text (repeatable, comma lists accepted: `after=#a,#b`), meaning the named ids must be ordered *before* this item. When any dependency edge is resolvable among the queue prompts, `queue::sort_prompts_by_dag` orders the queue by a priority-weighted topological sort (Kahn's algorithm). **Operator pins are position-locked here too (`#queue-operator-pin-position-lock-dag`)**, matching `sort_prompts_by_priority`: an operator pin keeps its exact document slot while the movable (agent-pin/unpinned) prompts are topologically ordered — agent pin before unpinned, then priority rank, then document order — and fill the remaining slots around the anchored pins. **A blocker still outranks a pin** (the operator's stated exception): if anchoring an operator pin to its slot would violate a dependency edge, the binary falls back to the plain dependency topo so the dependency is never broken. An edge-free queue is ordered exactly as the plain pin+priority sort (`sort_prompts_by_priority`). A dependency cycle is broken by emitting the remaining prompts in priority order (never dropped). Dependencies declared on a backlog item propagate to its synced `do [#id]` queue prompt, and inline `after=` tokens on a queue prompt are also honored. Pure logic: `queue::sort_prompts_by_dag`, `pending::item_after_deps` / `pending::active_item_after_deps`. Plan: `tasks/agent-doc/plan-queue-auto-dag-priority.md`. Deferred: a rich graph-visualization projection and incremental (non-recompute) DAG maintenance. The sort key is `(tier, rank)` with `tier` ∈ {0 operator, 1 agent, 2 unpinned}; rank only orders the unpinned tail. The agent may add an italic pin to items it prioritizes; the operator overrides by deleting it or adding their own strong pin. Open question deferred by the operator: whether an agent pin may be placed *above* an operator pin — for now, no. Marker constants + detectors: `queue::PRIORITIZED_MARKERS` / `queue::AGENT_PRIORITIZED_MARKERS`, `queue::is_prioritized` / `queue::is_agent_prioritized`.

**Nested queue execution (`#queuenest`, `#f1s3`):** the queue's top level remains an unordered list. Indented `-` children form unordered groups, while sibling `1.`, `2.`, ... children compile at ingress into the same canonical dependency projection used by explicit `after=` metadata. Ordered group subtrees connect their leaf frontiers: every entry leaf in a later sibling depends on every terminal leaf in the preceding sibling, while group labels remain non-executable and require no ids. The parser and renderer preserve indentation and the exact authored ordered marker, operator reordering re-derives the compiled edges, and priority/DAG maintenance must not flatten structured list shape.

**Scheduling precondition (`#backlog-not-before`):** an open backlog/icebox item may carry a `not-before=YYYY-MM-DD` token in its text declaring an earliest eligibility date. While the system clock's UTC date is *before* that threshold the item is **held out of the backlog→queue sync** (`pending::active_item_ids`, `active_item_priorities`, and `active_enqueue_item_ids` all exclude it), so a `queue`-attributed backlog does not enqueue work scheduled for the future — an explicit `:inbox_tray:`/`/enqueue` marker does not override an unmet date. On or after the threshold date the item becomes eligible and syncs like any other open item. The item stays a normal open `[ ]` entry in the backlog the whole time (it is a soft schedule, not a `[/]` gate, and it is not auto-gated). The token must start at a word boundary and parse as a strict `YYYY-MM-DD`; a malformed value is ignored (treated as no precondition rather than silently held). Day arithmetic uses a proleptic-Gregorian `days_from_civil` so no date dependency is required. Pure logic: `pending::item_not_before_day` / `pending::item_precondition_unmet` / `pending::today_civil_day`.

**Nested checklist support:** A tracked backlog/icebox item is the flush-left tracked parent line (`- ...` or `1. ...`) plus any following indented continuation lines (nested lists, dependency notes, indented paragraphs, etc.) up to the next flush-left tracked item or other non-indented structural content. Those continuation lines move with the parent item during reorder/transfer, survive backfill/edit/done transitions, and are reaped together with the parent when it reaches `[x]`.

Indented child task lines are canonicalized when they look like list items: the binary inserts missing checkboxes and nested ids shaped like `[#parentid-abcd]`, where `parentid` is the owning flush-left tracked item's id and `abcd` is a generated suffix. Those nested ids stay subordinate to the parent continuation block — they are not independent reorder/done targets unless the child is promoted to its own flush-left tracked entry.

## Implementation plan

1. **Rust — commands** (`src/write.rs`, `src/pending.rs`):
   - `--backlog-add <text>` (supports canonical `id=<custom> ` syntax and compatibility `[#custom] ` input)
   - `--backlog-add-to <file> <text>` for explicit cross-document backlog targets
   - `--done <id>`
   - `--backlog-gate <id>` (Gated lifecycle; legacy `--pending-gate`)
   - `--backlog-ungate <id>` (Gated lifecycle; legacy `--pending-ungate`)
   - `--backlog-edit <id> <text>`
   - `--backlog-clear`
   - `--backlog-reorder <ids>`
   - `PendingState` enum: `Open | Gated | Done`. Parser accepts `[ ] | [/] | [x]`; renderer emits the reverse.
   - Hash generation helper in `src/pending.rs`.
   - State transition validation (see matrix above) enforced at the `backlog_cmd` layer.
   - Enforcement: reject `replace:pending` blocks in parsed patches.

2. **Rust — preflight** (`src/preflight.rs`):
   - Lazy backfill: assign missing hash IDs.
   - Lazy backfill: insert missing `[ ]` checkboxes (never `[/]` — gated state is always explicit).
   - Reap: remove `- [x]` lines only. `[/]` is skipped unconditionally.
   - Reorder detect: diff ID order, emit `backlog_reordered` flag in JSON.
   - Emit `backlog_gated_count` alongside existing counts in preflight JSON.

3. **Skill — runbook** (`.claude/skills/agent-doc/SKILL.md` §1b):
   - Document `--backlog-gate` / `--backlog-ungate` alongside existing granular flags.
   - Guidance: when agent lands code that cannot ship immediately (awaiting release, telemetry, field validation), call `--backlog-gate <id>` instead of leaving `[ ]` with prose "awaiting X".
   - Respect `backlog_reordered: true` — do not reorder this cycle.
   - For plan-backed work, create the plan file first and include its path in
     the backlog item text in the same cycle.

4. **Release discipline:**
   - `Gated` lifecycle ships in its own release (**v0.32.6**), NOT bundled with the `#eg0w` CommitLock release (v0.32.5). Reason: the CommitLock fix gets clean field validation first; mixing gated-state into the same release confuses the field-test signal.
   - Post-v0.32.6 release, retag `#a002`, `#64mb`, `#eg0w` to `[/]` by hand — nice closed loop validating the new workflow on its own implementation.

5. **Feedback memory:**
   - Update `feedback_pending_patch_every_cycle.md` to reflect the new invariant.
   - Or replace with `feedback_pending_granular_ops.md`.

## Migration

No explicit migration command. First preflight after upgrade lazy-backfills. Docs stuck on the old schema self-heal on next open.

## Open questions

- Should `--done` reap immediately, or only mark `[x]` and let preflight reap on the next cycle? Default: mark-only (preflight is the single reap path). Add `--reap` flag later if needed.
- `agent:done` is the canonical completed/reaped archive component. `agent:backlog-done` and `agent:pending-done` are not accepted as aliases; migration rewrites both to the canonical marker.
- Should the hash prefix be rendered-visible or hidden behind a comment? Visible — transparency wins, and grep-ability is too useful to lose.
- Reorder flag scope: one cycle, or persistent until user signals otherwise? One cycle — simpler invariant, lower risk of the skill getting stuck refusing to reorder.
