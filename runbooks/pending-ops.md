# Backlog ops — granular contract

When a template-mode document has an `<!-- agent:backlog -->` (or legacy `<!-- agent:pending -->`) component, the agent mutates it
through **granular flags** on `agent-doc write`. The optional `<!-- agent:review -->` component holds code-complete work waiting for human review. Full-replace via `<!-- replace:backlog -->`
(or `<!-- replace:pending -->`) is **forbidden** in normal response cycles — the
binary rejects those blocks with a clear error. Compatibility note: the binary may normalize one accidental
list-shaped `replace:pending` block internally before capture/replay so the cycle is not stranded,
but agents must still treat that as a recovery backstop rather than a supported authoring path. See `src/agent-doc/specs/pending-system.md`
for the full contract. `pending` remains a legacy compatibility term; new
commands and guidance should use `backlog`.

## Item shape

Backlog, review, and icebox items carry stable IDs and GFM checkboxes:

```
- [ ] [#a3f2] active item
- [x] [#b1c4] user-marked done (commit-required closeouts reap it in the same cycle; preflight / repair also clean up stale completed items)
- [/] [#c9e0] gated — skipped by reaper, waiting on external signal
1. [ ] [#d4e5] explicit top priority
```

Preflight lazy-backfills IDs and checkboxes on any item that lacks them. You do not assign
hashes yourself unless you intentionally use the `id=<custom> ` prefix on add.

If any tracked parent item in a backlog or icebox uses ordered-list style (`1.`, `2.`, `3.`),
the binary treats that component as an ordered tracked surface and renumbers all tracked parent
items sequentially in current item order on render. That keeps explicit priority lists stable
across add/reorder/done/reap operations.

Backlog and icebox sections may also contain markdown headings or blank separator
lines for organization. The granular backlog/icebox ops preserve those non-item lines in
place; they mutate only the actual task bullets.

Nested lists are supported under a backlog/icebox item as indented continuation
lines. Keep the tracked parent marker flush-left (`- ` or `1. `); the binary treats
the indented children as part of that parent item and preserves them through reorder, done,
reap, and transfer. Nested child task lines are canonicalized too: the binary inserts
missing checkboxes and nested ids shaped like `[#parentid-abcd]` so subtask bullets can
be referenced without promoting them to their own flush-left parent items.

## Granular flags

Combine any number of flags in one `agent-doc write` call:

| Flag | Purpose |
|------|---------|
| `--backlog-add "text"` | Add a new item at the beginning of the backlog. Binary assigns the hash unless the text starts with canonical `id=<custom> ` syntax. Leading `[#custom] ` is also accepted as compatibility input. Bare `[#]` and stacked prefixes such as `[#a] [#b] ...` fail closed. Repeat for multiple adds; repeated flags keep caller order at the top of the backlog. Legacy alias: `--pending-add`. |
| `--done <id>` | Mark `[x]` in tracked work (`agent:backlog` / legacy `agent:pending`, `agent:review`, or `agent:icebox`) — commit-required closeouts reap it in the same persisted cycle. Repeat for multiple ids. |
| `--backlog-edit "id=new text"` | Rewrite text, preserve hash. Repeat as needed. Legacy alias: `--pending-edit`. |
| `--backlog-clear` | Drop all backlog items. Legacy alias: `--pending-clear`. |
| `--backlog-reorder <id1,id2,...>` | Reorder by id. Missing ids keep their relative order. Legacy alias: `--pending-reorder`. |
| `--backlog-gate <id>` | Move a backlog item to `agent:review` as `[/]`. If `agent:review` is missing, the binary inserts it after the backlog. Legacy alias: `--pending-gate`. |
| `--backlog-ungate <id>` | Move a review item back to `agent:backlog` as `[ ]`. Legacy gated backlog items still ungate in place until migrated. Legacy alias: `--pending-ungate`. |
| `--icebox-add "text"` | Add a parked tracked-work item to `agent:icebox` using the same id/collision/writeback rules as backlog adds. It does not mirror into `agent:queue`. |
| `--icebox-add-after <id> "text"` / `--icebox-add-before <id> "text"` / `--icebox-add-back "text"` | Position parked icebox work explicitly without replacing the whole component. |
| `--icebox-edit "id=new text"` | Rewrite an icebox item's text, preserve hash. |
| `--icebox-clear` | Drop all icebox items. |
| `--icebox-reorder <id1,id2,...>` | Reorder icebox items by id. Missing ids keep their relative order. |
| `--review-add "text"` | Add a new `[/]` item directly to `agent:review`; usually prefer `--backlog-gate`. |
| `--review-edit "id=new text"` | Rewrite a review item's text, preserve hash. |
| `--review-resolve <id>` | Resolve a review item: remove it from `agent:review` and archive to `agent:done` (the completion path when the gated work is actually finished). |
| `--review-remove <id>` | Delete a review item by id, clearing **every** entry that shares the id — use for a stale or duplicate review entry (e.g. the identical `[/]` pair an interleaved finalize leaves behind, flagged as `preset_item_id_collision`). |

Finalize also auto-dedupes identical same-id review entries during closeout maintenance, so an interleaved-finalize duplicate collapses without an explicit flag; distinct items that merely share an id are preserved so the ambiguity warning still surfaces.

**Complete over gate — keep `agent:review` small (target < 10).** The default
outcome of a turn is `--done`: finish the implementation, tests, build/install,
and verification this cycle and reap the item. `agent:review` (gated `[/]`) and
`--backlog-gate` / `--review-add` are for **exceptional** work only — genuinely
blocked on something this turn cannot do (a required live editor/pane verify, an
external approval, a CI/billing outage) — and the item text must name exactly
what unblocks it. Do not gate to "track for later" what you could finish now, to
record a hypothesis, or to avoid effort; that is what produces an unreadable
50-item review backlog. Real-but-unblocked follow-up goes to `agent:backlog` as
an **actionable** item, never to `agent:review`. When a gated review item's
blocking condition is stale or already satisfied, resolve it: `--done <id>` if
the work is in fact complete, otherwise `--backlog-ungate <id>` (or capture a
fresh actionable backlog item) and `--done` the stale review entry so it leaves
`agent:review`. Prefer automated completion detection (a log/state check the
binary can evaluate) over a human-gated review item wherever the signal exists.

`--done <id>` is the tracked-work completion flag. New guidance, plans, and
recovery hints must not emit removed completion-alias spellings.

Use `--icebox-add*`, `--icebox-edit`, `--icebox-clear`, and
`--icebox-reorder` for parked future work. Do not rewrite `agent:icebox` with
`replace:icebox` in normal response cycles; icebox follows the same
operator-preserving tracked-work mutation discipline as backlog, except parked
icebox items do not mirror into `agent:queue`.

`review_done_guard` is an optional frontmatter/project guard. Default `off`
preserves existing behavior. With `warn`, `--done <id>` emits a warning when the
item is still in backlog or icebox instead of `agent:review`. With `strict` (or
alias `error`), that `--done` fails until the cycle first gates the item through
`--backlog-gate <id>`.

Completed/reaped items are archived under canonical `agent:done`. To keep a
long-running session document small, use
`<!-- agent:done archive=path/to/session.done.md -->`; the target must be
repo-relative, stay inside the repo, and end with `.done.md`. Legacy
`agent:backlog-done` and `agent:pending-done` components are not accepted as
archive aliases; run `agent-doc migrate` to rewrite them.

## Custom IDs

When you need a stable human-chosen identifier, start the add text with
canonical `id=<custom> ` syntax:

```bash
agent-doc backlog plan.md add "id=spec1 write rollout spec"
agent-doc write plan.md --backlog-add "id=fix42 add regression test"
```

Rules:
- `custom` is a non-empty ASCII alphanumeric string; hyphens are allowed.
- `id=#spec1 ...` is also accepted; the leading `#` is stripped.
- Leading `[#spec1] ...` is accepted as compatibility input and normalized to the
  same custom id, but `id=<custom> ` remains the preferred form for agents.
- Bare `[#] ...` is invalid in active add-time paths. Omit it to get a generated
  id, or use `id=<custom> ...` / `[#custom] ...` for an explicit id.
- Stacked leading prefixes such as `[#spec1] [#spec2] ...` or
  `id=spec1 [#spec2] ...` are invalid. Use exactly one leading custom-id prefix.
- The custom id must be unique within active backlog, review, and icebox work.
- The remainder after the prefix becomes the item text.

## Reorder flag

If preflight returns `backlog_reordered: true`, the user just expressed a
priority by reordering items. **Do NOT reorder this cycle** -- respect the
user's intent for at least one cycle.

## Default ordering

New backlog items go at the **beginning** of the list. When adding multiple new
items in one cycle, preserve the order you presented them in so the first
recommended next step stays first.

Exception: if you are later adding a follow-on step from an ordered batch that
is already partially represented in backlog, place the new item next to its
predecessor rather than prepending it above earlier steps. `#ah0s` makes this
first-class -- prefer the explicit-position flags over a prepend +
`--backlog-reorder`:

```bash
# Insert directly after an existing item (no reorder pass needed):
agent-doc write plan.md --backlog-add-after step2 "id=step3 [recommended] Add step 3"

# Chain to build an ordered sub-sequence A -> B -> C deterministically:
agent-doc write plan.md \
  --backlog-add-after stepA "id=stepB Add B" \
  --backlog-add-after stepB "id=stepC Add C"

# Low-priority capture that should not jump the head (tail insert):
agent-doc write plan.md --backlog-add-back "[recommended] Nice-to-have cleanup"
```

`--backlog-add` stays the cheap front-insert default;
`--backlog-add-after <id>`, `--backlog-add-before <id>`, and
`--backlog-add-back` (alias `--backlog-append`; legacy alias
`--pending-append`) set position explicitly. The backlog is a priority-ordered
pool with id-based consumption (not a stack/queue), so position is author
intent, not argv order. The older prepend + `--backlog-reorder` pattern still
works:

```bash
agent-doc write plan.md \
  --backlog-add "id=step3 [recommended] Add step 3" \
  --backlog-reorder gkke,9pw9,step3
```

That keeps the ordered batch stable when Step 1 / Step 2 already exist and you
are only promoting Step 3 in a later cycle. If the predecessor is not already in
backlog, fall back to the normal front-insertion rule.

## Plan-backed backlog items

If a backlog item points at a dedicated plan document, create the plan file
first, then add the backlog item in the same cycle and include that exact plan
file path in the item text. Do not create a vague backlog bullet like "write
the plan" and only later decide which file it refers to.

Preferred shape:

```bash
agent-doc write plan.md \
  --backlog-add "id=spec2 [recommended] Draft follow-up rollout plan in tasks/agent-doc/plan-spec2-rollout.md"
```

That keeps the backlog self-describing: the backlog line already tells the next
cycle which concrete plan file exists and should be opened.

For multi-phase implementation plans, prefer one flush-left backlog item per
actionable phase with a stable custom id (for example `#crdtrespfx1`,
`#crdtrespfx2`) instead of one parent id that gets repeatedly
`--backlog-gate`d after partial progress. The parent plan file can remain the
overview, but queue entries and closeouts should target the concrete phase ids.
Use `--backlog-gate` when that specific phase is code-complete and waiting on
review or an external signal, not merely because later phases in the same plan
remain open.

If the document is being used collaboratively, treat that cross-document read as
a security boundary. Shared docs should carry both
`agent_doc_collaboration: shared` and an auditable
`agent_doc_security_review: <review-id>` before a `do #id` cycle follows a
plan path into another `.md` file.

## What to decide each cycle

- Items completed during this response → `--done <id>`
- New items discovered → `--backlog-add "text"`
- **Agent-proposed forward actions** → `--backlog-add "text"` for each concrete
  follow-up that should be tracked across cycles.
- **Unaccepted recommendations** → `--backlog-add "[recommended] text"` so the
  item is visibly provisional until the user opts in.
- **Existing `do #id` work that completed this cycle** → `--done <id>` in
  the same closeout command whether the item lived in the live backlog or the
  icebox. Session-doc closeouts now fail before commit when a response clearly
  completes `#id` but omits the matching done mutation. If the item is
  code-complete but blocked on an external gate, prefer
  `--backlog-gate <id>` instead of leaving it silently open.
- **`do [#id]` / `do #id` directives are closeout-gated regardless of response
  wording** (`#do-id-closeout-open-backlog`). When the prompt directive names a
  tracked id that is open in `agent:backlog` at preflight, the cycle records an
  `expect_done_or_gate` obligation for that id. A successful closeout — even one
  that only clears the queue or updates status without a "completed #id" heading
  — fails closed if the id is still `[ ]` in `agent:backlog` and no `--done`,
  `--backlog-gate`, or kept-open edit (`--backlog-edit "id=... (stays open: ...)"`)
  was recorded this cycle. Resolve every `do [#id]` target with exactly one
  lifecycle outcome: `--done <id>`, `--backlog-gate <id>` (code-complete,
  awaiting review/external validation), an explicit kept-open edit, or set
  `pending_done_guard: off` for the document when the item must stay open.
- **"Review-gated" is not "blocked but still actionable"**
  (`#blocked-closeout-followup-capture`). `--backlog-gate <id>` is for work that
  is *implementation-complete* and only waiting on review/external validation —
  no further agent execution step is needed. If a `do [#id]` cycle instead
  reports the target is blocked / still needs future action (e.g. "next steps to
  complete", "must remove/expire …", "awaiting approval before …") and gates the
  id out of `agent:backlog`, closeout fails closed: the document then explains
  the blocker while the active backlog no longer drives the remaining work.
  Resolve it one of these ways:
  - keep the same id open with the narrowed next action —
    `--backlog-edit "<id>=<remaining next step>"`;
  - split a new actionable follow-up —
    `--backlog-add-after <id> "<new-id>=<concrete next step>"` (or any
    `--backlog-add*`);
  - for a genuine review-only gate, state it explicitly in the response: an
    "no additional backlog follow-up is needed because …" phrase satisfies the
    guard, as does a `<!-- no-blocked-followup-guard -->` marker.
- If the completed ids are also contiguous head entries in an active
  `agent:queue`, repeating `--done <id>` for each completed id lets closeout
  consume that done-backed queue batch in the same commit. The queue still stops
  before the first unresolved queued prompt.
- **Multiple free-text heads answered in one cycle → `agent-doc queue consume <FILE> --count <N>` (`#multi-head-consume-one-per-finalize`).** The free-text strike consumes only ONE head per finalize (the head current at that cycle's preflight). When a single cycle answers several free-text heads — most often because the operator added queue items mid-turn and your response addressed all of them — the trailing answered heads stay queued and re-serve on the next auto-loop, producing duplicate-response churn. Drain them explicitly with `queue consume --count <N>` (you assert the leading N free-text heads are answered, the same explicit contract `--done <id>` gives an id-backed head), then close out normally. The count is one atomic editor-authority transaction: agent-doc plans the leading free-text prefix once and writes once, stopping before an id-backed head; it does not re-resolve the same head between asynchronous ACKs. There is intentionally no automatic head→response matcher: an answered free-text head the operator added this cycle is structurally identical to an unanswered one they added, so auto-striking would risk deleting a genuinely unanswered prompt. **Prefer prevention:** in a normal drain, answer only the current head per cycle and let the auto-loop drain subsequent heads one at a time; reach for `queue consume` only after an unavoidable batch answer. `queue consume` is scoped to free-text heads — a leading id-backed (`do [#id]` / `[#id]` / `#id` / `#preset`) head bails with `--done` guidance so it is never desynced from its backlog item.
- Any response ending with a forward-looking question ("Ready to X?", "Should we A or
  B first?", "Shall I capture Y as a spec?") MUST capture each concrete next-step
  option in the same cycle unless the options are explicitly mutually exclusive and
  still awaiting user choice. The proposal dies if the user doesn't reply immediately;
  capturing it preserves continuity across cycles.
- Reword an existing item → `--backlog-edit "id=new text"`
- Reprioritize (only when `backlog_reordered` is NOT true) → `--backlog-reorder`
- Block an item on external signal → `--backlog-gate <id>`

## Example — multi-flag cycle

Add one, mark two done, reword another:

```bash
cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline> --stream --origin skill \
  --backlog-add "integration test for --backlog-reorder" \
  --done a3f2 --done b1c4 \
  --backlog-edit "c9e0=refactor preflight: use single exit point"
<response body -- patch:exchange allowed, replace:pending forbidden>
RESPONSE
```

## Review triage (`#review-list-query`)

When a long-running document's `agent:review` grows unmanageable, triage it
token-efficiently instead of reading the whole component:

- `agent-doc review list <FILE>` — one compact line per gated item: `#id [gate-type]
  summary #tags`, plus a `→ NEXT: …` line when the item carries a `NEXT:` annotation.
- Filters: `--gate-type <t>`, `--tag <#foo>` (bare `foo` accepted), `--has-next`
  (only actionable, NEXT-annotated items), `--no-next` (the stale set to split or
  drop), `--json` (structured output for programmatic triage).
- `agent-doc review ungate-tasks <FILE>` — drive gated items back into the backlog
  pipeline by adding one ungate follow-up task per gated review item (idempotent).

Annotate gated items with a token-efficient `[<gate-type|blocked:reason>] <summary>.
NEXT: (1)… (2)…` so `review list --has-next` surfaces exactly what is actionable.

## Escape hatch

`--allow-replace-pending` (hidden flag, or `AGENT_DOC_ALLOW_REPLACE_PENDING=1`) permits
`<!-- replace:pending -->` blocks. Only use during compaction, migration, or tests. Never in
a normal response cycle.
