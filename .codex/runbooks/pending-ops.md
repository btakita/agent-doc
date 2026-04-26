# Pending ops — granular contract

When a template-mode document has an `<!-- agent:backlog -->` (or legacy `<!-- agent:pending -->`) component, the agent mutates it
through **granular flags** on `agent-doc write`. Full-replace via `<!-- replace:backlog -->`
(or the deprecated `<!-- patch:pending -->` / `<!-- replace:pending -->`) is **forbidden** in normal response cycles — the
binary rejects those blocks with a clear error. See `src/agent-doc/specs/pending-system.md`
for the full contract.

## Item shape

Pending items carry stable IDs and GFM checkboxes:

```
- [ ] [#a3f2] active item
- [x] [#b1c4] user-marked done (preflight reaps next cycle)
- [/] [#c9e0] gated — skipped by reaper, waiting on external signal
```

Preflight lazy-backfills IDs and checkboxes on any item that lacks them. You do not assign
hashes yourself unless you intentionally use the `id=<custom> ` prefix on add.

## Granular flags

Combine any number of flags in one `agent-doc write` call:

| Flag | Purpose |
|------|---------|
| `--pending-add "text"` | Add a new item at the beginning of the list. Binary assigns the hash unless the text starts with canonical `id=<custom> ` syntax. Leading `[#custom] ` is also accepted as compatibility input. Repeat for multiple adds. |
| `--pending-done <id>` | Mark `[x]` — preflight reaps next cycle. Repeat for multiple ids. |
| `--pending-edit "id=new text"` | Rewrite text, preserve hash. Repeat as needed. |
| `--pending-clear` | Drop all items. |
| `--pending-reorder <id1,id2,...>` | Reorder by id. Missing ids keep their relative order. |
| `--pending-gate <id>` | Transition to `[/]` gated state. Reaper skips gated items. |
| `--pending-ungate <id>` | Return `[/]` to `[ ]`. |

## Custom IDs

When you need a stable human-chosen identifier, start the add text with
canonical `id=<custom> ` syntax:

```bash
agent-doc pending plan.md add "id=spec1 write rollout spec"
agent-doc write plan.md --pending-add "id=fix42 add regression test"
```

Rules:
- `custom` is 1-8 ASCII alphanumeric characters.
- `id=#spec1 ...` is also accepted; the leading `#` is stripped.
- Leading `[#spec1] ...` is accepted as compatibility input and normalized to the
  same custom id, but `id=<custom> ` remains the preferred form for agents.
- The custom id must be unique within the pending component.
- The remainder after the prefix becomes the item text.

## `pending_reordered` flag

If preflight returns `pending_reordered: true`, the user just expressed a priority by
reordering items. **Do NOT reorder this cycle** — respect the user's intent for at least
one cycle.

## Default ordering

New pending items go at the **beginning** of the list. When adding multiple new
items in one cycle, preserve the order you presented them in so the first
recommended next step stays first.

Exception: if you are later adding a follow-on step from an ordered batch that
is already partially represented in pending, place the new item next to its
predecessor rather than prepending it above earlier steps. The practical pattern
is:

```bash
agent-doc write plan.md \
  --pending-add "id=step3 [recommended] Add step 3" \
  --pending-reorder gkke,9pw9,step3
```

That keeps the ordered batch stable when Step 1 / Step 2 already exist and you
are only promoting Step 3 in a later cycle. If the predecessor is not already in
pending, fall back to the normal front-insertion rule.

## Plan-backed pending items

If a pending item points at a dedicated plan document, create the plan file
first, then add the pending item in the same cycle and include that exact plan
file path in the item text. Do not create a vague pending bullet like "write
the plan" and only later decide which file it refers to.

Preferred shape:

```bash
agent-doc write plan.md \
  --pending-add "id=spec2 [recommended] Draft follow-up rollout plan in tasks/agent-doc/plan-spec2-rollout.md"
```

That keeps the backlog self-describing: the pending line already tells the next
cycle which concrete plan file exists and should be opened.

## What to decide each cycle

- Items completed during this response → `--pending-done <id>`
- New items discovered → `--pending-add "text"`
- **Agent-proposed forward actions** → `--pending-add "text"` for each concrete
  follow-up that should be tracked across cycles.
- **Unaccepted recommendations** → `--pending-add "[recommended] text"` so the
  item is visibly provisional until the user opts in.
- **Existing `do #id` work that completed this cycle** → `--pending-done <id>` in
  the same closeout command. If the item is code-complete but blocked on an
  external gate, prefer `--pending-gate <id>` instead of leaving it silently open.
- Any response ending with a forward-looking question ("Ready to X?", "Should we A or
  B first?", "Shall I capture Y as a spec?") MUST capture each concrete next-step
  option in the same cycle unless the options are explicitly mutually exclusive and
  still awaiting user choice. The proposal dies if the user doesn't reply immediately;
  capturing it preserves continuity across cycles.
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
