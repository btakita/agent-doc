---
status: approved
phase: pending-implementation
owner: btakita
---

# Pending System — Stable IDs, Checkboxes, Gated Lifecycle, Granular Ops

## Status

**Approved, pending implementation.** Design agreed in `tasks/agent-doc/agent-doc-bugs.md` exchange. No code yet.

## Problem

The `agent:backlog` component drifts between cycles. Current enforcement requires a full `patch:pending` block on every response — when the skill forgets, or rewrites the wrong item, or reorders the list, downstream cycles can't tell which item was "the same bullet as last time." Full-replace is lossy: text drift and reorder both destroy identity, so `--pending-done 3` (numeric index) is unsafe and `--pending-done "text"` (exact match) is fragile.

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

- Generated on first insert (via `--pending-add`) unless the caller explicitly
  provides a custom id with canonical `id=<custom> ` syntax; leading
  `[#custom] ` is accepted as compatibility input and normalized to the same
  custom id. Custom ids are non-empty ASCII alphanumeric strings with optional
  hyphens. Lazy backfill still generates IDs.
- Stable across reorders, text edits, and cycles.
- Visible in rendered markdown (like a GitHub issue number) — no hidden state.
- Opaque: the hash is not meaningful, just unique within the component.

Hash generation: `base32(blake3(text + doc_id + monotonic_counter))[:4]`. Collision handling: on collision within the component, extend to 5 chars and retry; documented ceiling at 8 chars.

### 2. GFM task-list checkboxes with Gated lifecycle state

Every bullet carries a GFM-style checkbox that encodes a three-state lifecycle:

```
- [ ] [#a3f2] open — not started or in progress
- [/] [#b1c4] gated — code-complete, awaiting release / telemetry / verification
- [x] [#c9e0] done — reaped next preflight cycle
```

**Lifecycle diagram:**

```
┌─────┐   --pending-gate     ┌─────┐   --pending-done     ┌─────┐
│ [ ] │ ───────────────────► │ [/] │ ───────────────────► │ [x] │ ──► (reaped)
│ open│                      │gated│                      │ done│
└─────┘                      └─────┘                      └─────┘
   ▲                            │
   │     --pending-ungate       │
   └────────────────────────────┘

Direct path (no gating needed):
[ ] ──── --pending-done ────► [x] ──► (reaped)
```

**State semantics:**

| State | Char | Meaning | Reaped? |
|-------|------|---------|---------|
| `Open` | `[ ]` | active work or not started | No |
| `Gated` | `[/]` | code-complete, awaiting external gate (release, telemetry, field validation) | **No** |
| `Done` | `[x]` | fully complete; user or agent signals reap | Yes (next preflight) |

**Why three states instead of a prose suffix:**

- **Machine-readable.** Preflight can emit `pending_gated_count` alongside existing counts; release workflow can query gated items programmatically.
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

Preflight is the single upgrade path. No separate `agent-doc migrate` command.

On every preflight run:

1. Scan the `agent:backlog` component for list items.
2. For each bullet:
   - No hash prefix → generate and insert a hash.
   - No checkbox → insert `- [ ] ` before the hash.
3. Reap `- [x]` bullets **only**:
   - If a completed item still lacks an id (legacy/manual form such as `- [x] shipped` or `- [x] [#] shipped`), backfill its hash first, then reap/archive that canonicalized item in the same pass. Reap must never silently drop a done line that cannot be referenced in the archive or logs.
   - Remove the line from the component.
   - `[ ]` and `[/]` are never auto-reaped. `[/]` explicitly survives forever until an operator moves it to `[x]`. No TTL on gated state — the operator owns the gate.
   - Append an archive entry to canonical `agent:done`. If the component does not
     already exist, create a visible `## Completed / Reaped` section after the
     tracked work components first, then append the entry. Legacy
     `agent:backlog-done` and `agent:pending-done` components are not accepted
     as archive aliases; run `agent-doc migrate` to rewrite them to `agent:done`.
     Completed tracked work must remain grep-visible
     in the session document instead of disappearing from the live backlog
     without a local record.
   - Persistence invariant: the reap must land in both the working tree document and the snapshot that the commit boundary stages. If preflight cannot persist that synchronized reap safely, it must fail closed instead of continuing with completed tracked-work items still present in backlog or icebox.
- Same-cycle resurrection invariant: once a cycle reaps a tracked `[#id]`, closeout must fail closed if that same id reappears in the live `agent:backlog` or `agent:icebox` before commit. Do not silently treat the stale rewrite as generic local drift.
- Same-cycle completion invariant: when preflight/repair reap a user-authored `[x]` tracked item directly from the document, that id counts as intentionally resolved for the current cycle's history-replay guards even if no explicit `--pending-done <id>` flag was recorded. Do not restore the older `[ ]` or `[/]` history entry just because the completion came from a manual document edit.
- No-partial-reap invariant: if a completed tracked item is followed by malformed flush-left spill such as pasted command/diff transcript lines, reap/archive the whole logical block with that parent item. Do not delete only the tracked parent line and leave orphan prose behind in the live backlog.
4. Commit the rewritten component as part of the existing boundary-maintenance commit.

**Migration of existing items:** No auto-migration. Existing `[ ]` items with "✅ landed" / "shipped" / "awaiting release" prose stay as-is until touched manually. Auto-classifying prose is fuzzy (what counts?); blast radius is unclear. With only a handful of real gated items in-tree, retagging by hand via `--pending-gate` is faster and safer than writing a heuristic.

A doc that never gets opened again never migrates — fine, because IDs only matter when the skill/runbook is actively managing the list.

**Concurrent-open edge case:** Two sessions open the same doc before either commits backfill. Both assign different hashes to the same bullet. CRDT merge picks one. Acceptable — IDs are opaque and bullet text is unchanged.

### 4. Granular write-command surface

The skill/runbook **never** writes a `replace:pending` (or the deprecated `patch:pending`) block. Full-replace is forbidden. All mutations go through explicit flags on `agent-doc write`:

| Flag | Behavior |
|------|----------|
| `--pending-add "text"` | Add new item at the beginning of the list. Binary assigns hash and `[ ]` unless the text starts with canonical `id=<custom> ` syntax. Leading `[#custom] ` is accepted as compatibility input. When repeated in one command, all added items are inserted as one ordered batch: the first flag appears above the second, and the full batch appears above existing backlog items. |
| `--pending-add-to <file> "text"` | Add a new `[ ]` item to another document's backlog. The target file must exist and contain an `agent:backlog` / legacy `agent:pending` component; missing targets fail closed instead of falling back to the current document. Repeated pairs are grouped per target and preserve caller order at the top of each target backlog. |
| `--pending-done <id>` | Mark `[x]` in tracked work (`agent:backlog` / legacy `agent:pending` or `agent:icebox`) — commit-required closeouts reap it in the same persisted cycle, while preflight / repair also clean up stale completed items. Valid from any state (`[ ]` or `[/]`). If the id is already present in canonical `agent:done` or the current cycle's resolved-id ledger, treat it as an idempotent resolution warning rather than a fatal missing-id error. |
| `--pending-gate <id>` | Mark `[/]` — code-complete, awaiting gate. Valid from `[ ]`. No-op (logged) if already `[/]`. Error if source is `[x]`. |
| `--pending-ungate <id>` | Return `[/]` → `[ ]` — gate failed, back to active. Error if source is `[ ]` or `[x]`. |
| `--pending-edit <id> "new text"` | Rewrite text, **preserve hash and state**. Multiline edits replace the item's entire continuation block; lines after the first must be indented continuation content, not new flush-left parent items. |
| `--pending-clear` | Remove all items. |
| `--pending-reorder <id1,id2,...>` | Reorder items by ID. Missing IDs keep their relative order after the listed prefix. |

For every id-based pending flag except `--pending-add`, the binary normalizes
the id by trimming whitespace, stripping one optional leading `#`, and
lowercasing before lookup. `--pending-done 4qja` and `--pending-done #4QJA`
must therefore resolve the same tracked item.

**Plan-backed item rule:** when a pending bullet depends on a dedicated plan
document, the operator must create that plan file before adding the pending
item, and the pending text must include the concrete plan-file path. The
backlog entry is the durable pointer; it should not require later archaeology to
discover which `plan-*.md` file was intended.

**State transition matrix:**

| From \ Op | `--pending-done` | `--pending-gate` | `--pending-ungate` |
|-----------|------------------|------------------|--------------------|
| `[ ]` Open  | → `[x]` Done     | → `[/]` Gated    | error              |
| `[/]` Gated | → `[x]` Done     | no-op (log)      | → `[ ]` Open       |
| `[x]` Done  | no-op (log)      | error            | error              |

**Explicitly rejected:** `--pending-replace`. Every transformation it could express is a sequence of `add` / `done` / `edit` / `reorder`, and the sequence preserves IDs where replace would churn them. Adding it would re-enable the full-replace pattern this spec is trying to eliminate.

### 5. Reorder detection in preflight

Preflight becomes reorder-aware:

1. Extract ordered `[#id]` list from snapshot's pending component.
2. Extract same from current document.
3. If ID set unchanged but order differs → **user reordered**. Preflight rewrites the snapshot to match, commits, and emits `pending_reordered: true` in the JSON output.
4. If the ID set also changed → apply adds/removes (lazy backfill + `[x]` reap) first, then compare remaining IDs' order.
5. When `pending_reordered: true`, the skill MUST NOT reorder the component in the current cycle — user intent wins for at least one cycle.

### 6. Enforcement

Invert the current rule:

- **Before:** missing `patch:pending` → error.
- **After:** presence of `replace:pending` (or the deprecated `patch:pending`) in the outgoing write payload → error.

`agent-doc write` validates this at parse time. The only way to mutate pending is via the granular flags.

**#25ag rename (v0.32.4):** The block syntax was renamed from `patch:pending` to `replace:pending`. The `replace:` prefix signals full-replacement semantics explicitly and is the canonical form. Dual-accept is in effect for one release: `patch:pending`, `--allow-patch-pending`, and `AGENT_DOC_ALLOW_PATCH_PENDING=1` still work but emit a deprecation warning on stderr. Canonical names: `replace:pending` + `--allow-replace-pending` + `AGENT_DOC_ALLOW_REPLACE_PENDING=1`. Next release removes the deprecated names.

**Thread-safety fix (#envvar1):** The CLI dispatcher no longer uses `unsafe { env::set_var() }` to propagate `--allow-replace-pending` and `--pending-add` state to downstream write functions. A `WriteFlags` struct is threaded explicitly through the call chain. Env var reads are retained as a backwards-compat fallback for external scripts that set them before invoking `agent-doc write`.

**Component attribute deprecation (v0.33.15):** The `patch=replace` (and legacy `mode=replace`) attribute on `<!-- agent:backlog -->` / `<!-- agent:pending -->` opening tags is deprecated. The backlog component defaults to `replace` mode via the built-in default in `template::default_mode()`, and the binary owns all backlog mutations through `--pending-*` flags — making the inline attribute redundant. Existing documents are normalized automatically: the write path strips `patch=` and `mode=` from backlog component tags and emits a deprecation warning. New scaffolds omit the attribute.

## Schema — fully-migrated example

```markdown
<!-- agent:backlog -->
### Active
- [ ] [#a3f2] implement --pending-reorder
- [ ] [#b1c4] rewrite runbook to forbid replace:pending

### Gated
- [/] [#eg0w] per-file CommitLock + freshness gate — gate: v0.32.5 release
- [/] [#a002] normalize_user_prompts safety rail — awaiting large-drift telemetry trip

### Done
- [x] [#c9e0] fix boundary repositioning
<!-- /agent:backlog -->
```

After next preflight: `#c9e0` is reaped, `- [x]` line is removed, commit rolls forward. `#eg0w` and `#a002` remain untouched — they stay `[/]` until an operator explicitly promotes them to `[x]`.

**Header preservation:** `agent:backlog` and `agent:icebox` may contain ordinary markdown headings or blank lines between item groups for organization. Granular pending mutations (`add`, `done`, `edit`, `clear`, `reorder`, `gate`, `ungate`, reap/backfill) must preserve those non-item lines in place. Reordering operates on the item slots only; it does not delete or auto-synthesize headings.

**Ordered parent-item support:** A backlog or icebox may use flush-left ordered parent entries (`1. ...`, `2. ...`) instead of unordered `- ...` bullets. When any tracked parent entry in a component uses ordered style, the binary canonicalizes all tracked parent entries in that component as a single sequential ordered list in current item order. Adds, reorders, done/reap transitions, and selective transfers therefore renumber tracked parents instead of preserving stale ordinals.

**Nested checklist support:** A tracked backlog/icebox item is the flush-left tracked parent line (`- ...` or `1. ...`) plus any following indented continuation lines (nested lists, dependency notes, indented paragraphs, etc.) up to the next flush-left tracked item or other non-indented structural content. Those continuation lines move with the parent item during reorder/transfer, survive backfill/edit/done transitions, and are reaped together with the parent when it reaches `[x]`.

Indented child task lines are canonicalized when they look like list items: the binary inserts missing checkboxes and nested ids shaped like `[#parentid-abcd]`, where `parentid` is the owning flush-left tracked item's id and `abcd` is a generated suffix. Those nested ids stay subordinate to the parent continuation block — they are not independent reorder/done targets unless the child is promoted to its own flush-left tracked entry.

## Implementation plan

1. **Rust — commands** (`src/write.rs`, `src/pending.rs`):
   - `--pending-add <text>` (supports canonical `id=<custom> ` syntax and compatibility `[#custom] ` input)
   - `--pending-add-to <file> <text>` for explicit cross-document backlog targets
   - `--pending-done <id>`
   - `--pending-gate <id>` (new — Gated lifecycle)
   - `--pending-ungate <id>` (new — Gated lifecycle)
   - `--pending-edit <id> <text>`
   - `--pending-clear`
   - `--pending-reorder <ids>`
   - `PendingState` enum: `Open | Gated | Done`. Parser accepts `[ ] | [/] | [x]`; renderer emits the reverse.
   - Hash generation helper in `src/pending.rs`.
   - State transition validation (see matrix above) enforced at the `pending_cmd` layer.
   - Enforcement: reject `replace:pending` (and deprecated `patch:pending`) blocks in parsed patches.

2. **Rust — preflight** (`src/preflight.rs`):
   - Lazy backfill: assign missing hash IDs.
   - Lazy backfill: insert missing `[ ]` checkboxes (never `[/]` — gated state is always explicit).
   - Reap: remove `- [x]` lines only. `[/]` is skipped unconditionally.
   - Reorder detect: diff ID order, emit `pending_reordered` flag in JSON.
   - Emit `pending_gated_count` alongside existing counts in preflight JSON.

3. **Skill — runbook** (`.claude/skills/agent-doc/SKILL.md` §1b):
   - Document `--pending-gate` / `--pending-ungate` alongside existing granular flags.
   - Guidance: when agent lands code that cannot ship immediately (awaiting release, telemetry, field validation), call `--pending-gate <id>` instead of leaving `[ ]` with prose "awaiting X".
   - Respect `pending_reordered: true` — do not reorder this cycle.
   - For plan-backed work, create the plan file first and include its path in
     the pending item text in the same cycle.

4. **Release discipline:**
   - `Gated` lifecycle ships in its own release (**v0.32.6**), NOT bundled with the `#eg0w` CommitLock release (v0.32.5). Reason: the CommitLock fix gets clean field validation first; mixing gated-state into the same release confuses the field-test signal.
   - Post-v0.32.6 release, retag `#a002`, `#64mb`, `#eg0w` to `[/]` by hand — nice closed loop validating the new workflow on its own implementation.

5. **Feedback memory:**
   - Update `feedback_pending_patch_every_cycle.md` to reflect the new invariant.
   - Or replace with `feedback_pending_granular_ops.md`.

## Migration

No explicit migration command. First preflight after upgrade lazy-backfills. Docs stuck on the old schema self-heal on next open.

## Open questions

- Should `--pending-done` reap immediately, or only mark `[x]` and let preflight reap on the next cycle? Default: mark-only (preflight is the single reap path). Add `--reap` flag later if needed.
- `agent:done` is the canonical completed/reaped archive component. `agent:backlog-done` and `agent:pending-done` are not accepted as aliases; migration rewrites both to the canonical marker.
- Should the hash prefix be rendered-visible or hidden behind a comment? Visible — transparency wins, and grep-ability is too useful to lose.
- Reorder flag scope: one cycle, or persistent until user signals otherwise? One cycle — simpler invariant, lower risk of the skill getting stuck refusing to reorder.
