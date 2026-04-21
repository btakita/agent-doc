# Pending ops — granular contract

When a template-mode document has an `<!-- agent:pending -->` component, the agent mutates it
through **granular flags** on `agent-doc write`. Full-replace via `<!-- replace:pending -->`
(or the deprecated `<!-- patch:pending -->`) is **forbidden** in normal response cycles — the
binary rejects those blocks with a clear error. See `src/agent-doc/specs/pending-system.md`
for the full contract.

## Item shape

Pending items carry stable 4-char hash IDs and GFM checkboxes:

```
- [ ] [#a3f2] active item
- [x] [#b1c4] user-marked done (preflight reaps next cycle)
- [/] [#c9e0] gated — skipped by reaper, waiting on external signal
```

Preflight lazy-backfills IDs and checkboxes on any item that lacks them. You do not assign
hashes yourself — the binary does.

## Granular flags

Combine any number of flags in one `agent-doc write` call:

| Flag | Purpose |
|------|---------|
| `--pending-add "text"` | Append a new item. Binary assigns the hash. Repeat for multiple adds. |
| `--pending-done <id>` | Mark `[x]` — preflight reaps next cycle. Repeat for multiple ids. |
| `--pending-edit "id=new text"` | Rewrite text, preserve hash. Repeat as needed. |
| `--pending-clear` | Drop all items. |
| `--pending-reorder <id1,id2,...>` | Reorder by id. Missing ids keep their relative order. |
| `--pending-gate <id>` | Transition to `[/]` gated state. Reaper skips gated items. |
| `--pending-ungate <id>` | Return `[/]` to `[ ]`. |

## `pending_reordered` flag

If preflight returns `pending_reordered: true`, the user just expressed a priority by
reordering items. **Do NOT reorder this cycle** — respect the user's intent for at least
one cycle.

## What to decide each cycle

- Items completed during this response → `--pending-done <id>`
- New items discovered → `--pending-add "text"`
- **Agent-proposed forward actions** → `--pending-add "text"` for each concrete option.
  Any response ending with a forward-looking question ("Ready to X?", "Should we A or
  B first?", "Shall I capture Y as a spec?") MUST `--pending-add` each concrete
  next-step option in the same cycle. The proposal dies if the user doesn't reply
  immediately; capturing it preserves continuity across cycles.
- Reword an existing item → `--pending-edit "id=new text"`
- Reprioritize (only when `pending_reordered` is NOT true) → `--pending-reorder`
- Block an item on external signal → `--pending-gate <id>`

## Example — multi-flag cycle

Add one, mark two done, reword another:

```bash
cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline> --stream --origin skill \
  --pending-add "integration test for --pending-reorder" \
  --pending-done a3f2 --pending-done b1c4 \
  --pending-edit "c9e0=refactor preflight: use single exit point"
<response body — patch:exchange allowed, replace:pending forbidden>
RESPONSE
```

## Escape hatch

`--allow-replace-pending` (hidden flag, or `AGENT_DOC_ALLOW_REPLACE_PENDING=1`) permits
`<!-- replace:pending -->` blocks. Only use during compaction, migration, or tests. Never in
a normal response cycle.

`--allow-patch-pending` and `<!-- patch:pending -->` are accepted as **deprecated aliases**
for one release (tracked as #25ag) — the parser emits a stderr deprecation warning.
