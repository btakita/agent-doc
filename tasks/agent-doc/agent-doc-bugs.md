---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Compacted exchange summary (2026-04-14)

### Shipped to main

- **Pending system Phase 1-3** — branch `20260414/pending-gated-state` ff-merged to main (`69b5475`). Three commits: Phase 1 data model (`PendingState::{Open, Gated, Done}`, `[/]` parse/render, `backfill_never_inserts_gated` invariant), sweep commit (Phase 2 gate/ungate matrix + `--pending-gate`/`--pending-ungate` CLI + supervisor scaffolding), Phase 3 (inverted `enforce_no_patch_pending` default — now rejects unconditionally unless `AGENT_DOC_ALLOW_PATCH_PENDING=1`, emits `pending_gated_count` in preflight JSON).
- **Supervisor Phase 1 progress** — `supervisor/cwd.rs` (priority chain, 11 tests) and `supervisor/pty.rs` (portable-pty alloc/spawn/IO, 6 tests, deterministic env invariant locked). `#a17b` done. Still TODO: `resize.rs`, `state.rs`, `ipc.rs`, `start.rs` wire-up.
- **`src/instruction-files/LIBRARY_CONTEXT_POLICY.md`** committed as canonical spec for library agent context. Library sweep deferred until the audit check drives it mechanically.

### Build + install state

- `cargo build --release` clean, `cargo install --path .` replaced `~/.cargo/bin/agent-doc`. Binary reports `agent-doc 0.32.3` — no version bump yet, this is a pre-release testing install on top of the shipped tag. **Not published.**
- Test baseline: 576 pass / 45 fail. 43 failures are pre-existing tmux env dependencies (`route::*`, `resync::*`). 2 non-tmux failures (`supervisor::cwd::document_parent_fallback_when_no_project_root`, `preflight::preflight_sweep_commits_other_tracked_docs`) are test-isolation issues (nested `agent-loop/.git` contamination); both present on pre-merge HEAD — **not regressions**.

### Document calibrated for live test

Three pending items transitioned `[ ] → [/]` (preflight confirms `pending_gated_count: 3`):

- **`#a002`** — safety rail landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- **`#gqep`** — release v0.32.4, awaiting manual-testing gate.
- **`#mchh`** — JetBrains plugin reinstall + session-picker verify, awaiting rebuilt zip.

### Key design decisions locked

- **`validate_transition` matrix** (`src/pending.rs`) is single source of truth for `Open/Gated/Done × Gate/Ungate/MarkDone`. Gate-on-Gated and MarkDone-on-Done are idempotent **no-ops**, not errors, so the skill can retry safely. Gate-on-Done is an error (no time-travel resurrection). Ungate requires a `[/]` source strictly.
- **Apply order in Write handler**: `clear → add → edit → gate → ungate → done → reorder`. Gate before done means `--pending-gate X --pending-done X` in one call lands `[x]` cleanly (`Gated → Done`, valid).
- **Why reject `patch:pending` by default** (answered): full-replace on a list the user concurrently edits enables silent-data-loss via (1) concurrent-edit clobber, (2) hash instability breaking reap invariant, (3) intent laundering (greppability loss). Granular flags preserve all three.
- **Rename `patch:pending` → `replace:pending`** approved and queued as `#25ag`. Migration: dual-accept one release with deprecation warning; `--allow-patch-pending` gains `--allow-replace-pending` alias; `AGENT_DOC_ALLOW_REPLACE_PENDING=1` becomes canonical. Lands **after** v0.32.4 ships.
- **pty.rs invariants**: caller supplies canonicalized cwd, env is complete (no parent inherit — enforces deterministic-env spec invariant), slave dropped immediately after spawn, I/O threads exit on EOF. Env passthrough policy for `start.rs` wire-up is a queued decision (option a: enumerate exact vars needed, or b: opt-in `inherit_parent_env` escape hatch).

### Open questions parked

1. **`op_done` + `validate_transition`** — should `op_done` use the matrix too (to error on `Done → Done` double-mark), or stay unconditional? Cheap to add, slightly less forgiving for retry loops.
2. **`--pending-done` on `[/]` edge case** — expected to transition `[/] → [x]` cleanly (completion overrides gating); behavior not yet verified. If it errors, queue behind `#a002`.
3. **v0.32.4 release sequencing** — hold for `#25ag` (rename) and ship together, or release pending enforcement standalone? Recommendation on record: hold for rename.
4. **skills-janitor install** — not yet run; requires interactive `/plugin` command at the terminal. Fork + `src/skills-janitor/` submodule plan captured for post-install.

### Branch / workspace state

- `main` at `69b5475` (three commits ahead of origin).
- `20260414/pending-gated-state` redundant, still points at HEAD (safe to delete after origin push).
- Submodule `src/agent-doc` has working-tree clean; `.bin/agent-doc` symlink points at the fresh release binary.

❯ reorder agent:pending to have the tasks related to pending moved to the top
### Re: Pending reordered — pending-system items at top

Ten items moved to the head of the list via `--pending-reorder`, ordered from "active next" to "shipped tracker":

1. `#25ag` — Rename `patch:pending` → `replace:pending` (next queued task)
2. `#gqep` — Release v0.32.4 (gated, awaiting manual-testing gate)
3. `#e6z1` — Version bump + VERSIONS.md entry for gated-state (release gate)
4. `#14z4` — Hash collision ceiling: 8-char fallback in `generate_hash`
5. `#bak5` — Optional: pending-done archive component (spec §3 step 3, deferred)
6. `#pf01` — ✅ Spec landed: `specs/pending-system.md`
7. `#mgdw` — ✅ Parser `[/]` (Phase 1 shipped)
8. `#h1j2` — ✅ Reaper skip + `pending_gated_count` (Phase 3 shipped)
9. `#q90h` — ✅ CLI `--pending-gate`/`--pending-ungate` (Phase 2 shipped)
10. `#sx35` — ✅ Tests: parser round-trip + state transitions + CLI integration

All other items retain their original relative order beneath this group — `--pending-reorder` only moves the listed ids; unlisted ids fall through in place.

**Reap question:** `#pf01`, `#mgdw`, `#h1j2`, `#q90h`, `#sx35` are all shipped work now tracked only as history. If you want a cleaner list, I can mark them `--pending-done` next cycle and preflight will reap them. Keeping them for now since you asked for reorder, not reap — but say the word and they're gone. `#14z4` (hash collision ceiling) and `#bak5` (archive component) are genuinely future work and should stay.

**`pending_reordered` guard:** the spec says not to reorder again for at least one cycle after the user does. I just reordered per your explicit instruction — next cycle will respect `pending_reordered: true` if preflight reports it and leave the order alone unless you re-ask.

❯ In src/session-share/tasks/claudescore.md, I asked agent-doc to recalibrate. But `- [ ] [#ahmt] [/]` was rendered. Why?
### Re: Reproduced — `- [ ] [#ahmt] [/] ...` is the skill misusing `--pending-add`, not a renderer bug

Reproduced in a scratch doc in ~5 seconds:

```bash
$ agent-doc write bugtest.md --pending-add "[/] test item"
$ cat bugtest.md
- [ ] [#3sxg] [/] test item
```

Same pattern. Here's what happened.

**Root cause.** `--pending-add` writes the text **literally** — it does not parse the body for state markers. State is controlled by the checkbox char (`[ ]` / `[/]` / `[x]`), and `--pending-add` always creates items in the `Open` state (`[ ]`). The `[/]` in the argument became part of the item body, not a state transition. The renderer faithfully emitted what was stored: `- [ ] [#hash] [/] text`. From the renderer's POV this is correct — `[ ]` is the state, `[/] text` is the body.

**Why the other skill session wrote this.** Looking at the claudescore item text — "testing gated-state pending feature in live claudescore doc — verify [/] round-trip..." — the skill was trying to create a **gated** item in one shot. It reached for `--pending-add "[/] ..."` as a way to encode the state in the text, because there's no atomic add-as-gated path today:

- `--pending-add` creates `Open` only.
- `--pending-gate <id>` requires knowing the hash id.
- `--pending-add "X"` does **not** emit the assigned hash to stdout — the skill has no way to learn the new id within the same call. So `--pending-add "X" --pending-gate $NEW_ID` can't be composed atomically.

The skill got stuck and improvised by jamming `[/]` into the body. Wrong answer, but the hole is real.

**Three ways to fix this, each fixing a different slice of the problem:**

1. **Emit the assigned hash to stdout on `--pending-add`.** One line per add: `pending_added: abcd`. Lets the skill do `HASH=$(agent-doc write ... --pending-add "X" | grep pending_added | awk '{print $2}')` and then `agent-doc write ... --pending-gate $HASH` as a two-call flow. Minimal surface change; closes the "can't learn the id" gap.
2. **Add `--pending-add-gated "text"` as a convenience flag.** Directly creates `- [/] [#hash] text`. One-call atomic. Mirrors the existing `--pending-add` surface. Preferred if we think gated-at-birth is a common-enough case (it is for "this item is blocked on X from day one" scenarios, which is exactly what the claudescore test was).
3. **Reject ambiguous text in `--pending-add`.** If the text starts with `[/]`, `[x]`, or `[ ]` (with or without a leading space), refuse with an error telling the caller to use the proper flag. Defensive — stops the silent footgun the claudescore doc tripped.

All three are compatible and probably all worth doing. Queued as one pending item (`--pending-add-gated` flag + hash-to-stdout + reject-ambiguous-text bundle) since they cluster as one ergonomics fix.

**Immediate fix for the claudescore doc.** Two CLI calls:

```bash
# Strip the [/] prefix from the body text (keeps hash intact)
agent-doc write tasks/claudescore.md --pending-edit "ahmt=testing gated-state pending feature in live claudescore doc — verify [/] round-trip, reaper skip, preflight pending_gated_count. Tracks agent-doc v0.32.4 bundle (#gqep, #mgdw, #h1j2, #q90h, #sx35). See tasks/agent-doc/agent-doc-bugs.md"

# Then gate it for real
agent-doc write tasks/claudescore.md --pending-gate ahmt
```

Want me to run those against `src/session-share/tasks/claudescore.md` now, or leave it for that doc's own session?
❯ do it
### Re: Blocked on pane ownership — claudescore doc is claimed by pane %2

Ran the edit, got:

```
Error: pane ownership mismatch: session 143cbb97-6295-45d1-add5-9c19e9c713bb owned by pane %2, current pane is %3. Use `agent-doc claim` to reclaim.
```

This pane (%3) is the agent-doc-bugs session. The claudescore doc is currently claimed by pane %2 — presumably a live session in another tmux window. The pending-system contract intentionally blocks cross-pane writes so two agents can't fight over the same document's pending list mid-cycle.

Three ways forward, you pick:

1. **Run it from the claudescore pane.** Switch to %2 and issue the two commands there — that session already holds the claim and the write will succeed instantly. Cleanest option since the update then shows up in that session's next preflight diff naturally.
2. **Let me reclaim from this pane.** `agent-doc claim src/session-share/tasks/claudescore.md` would transfer ownership to %3, I'd run edit+gate, then I'd release or you'd re-claim from %2. Works but disrupts whatever %2 is doing — if that session is mid-cycle, it'll see "pane ownership lost" on its next action.
3. **Defer.** Leave the item as-is; next time the claudescore session runs, its own skill can notice the stray `[/]` in the body and fix it. The rendered item is ugly but not broken — it's `state=Open, body="[/] ..."`, reaper and gate count work correctly, just reads weird.

My recommendation: **option 1** if pane %2 is still live, **option 3** otherwise. Option 2 is available but I don't want to take it without a direct "yes, reclaim." Let me know which — or just do it from the other pane and I'll verify here on the next cycle.

❯ Can we add a way to send a task to another pane? Would transfer work?

**No pending change this cycle** — nothing to add/remove until I know which path you want.
### Re: send a task to another pane

Short answer: **`transfer` won't do it** — it moves component content between documents, not tasks between panes. Panes are bound to documents via claims, so "send to another pane" really means "write into the document that other pane owns, in a way its next cycle will see."

The existing primitives that come closest:

1. **`agent-doc notify <target-doc> "<message>"`** — appends a blockquote into the target doc's `agent:exchange`. The pane claiming that doc sees it as a new user turn on its next preflight. Supports `--source` and `--affects` for re-eval directives. This is the real "send a task" path.
2. **`agent-doc pending <target-doc> --add "text"`** (or `agent-doc write ... --pending-add`) — drops a task into the target doc's `agent:pending`. Less intrusive than `notify` — it doesn't interrupt an ongoing exchange, just queues work.
3. **`transfer` / `extract`** — bulk content movement between docs (e.g., hoist an exchange entry out of a scratch doc into a tracked one). Not a task-dispatch mechanism.

What's missing for a clean "dispatch" UX:
- No single command like `agent-doc send <target-doc> "<task>"` that combines notify + focus + optional wake (tmux send-keys to trigger a cycle immediately).
- No cross-pane ack — the sender doesn't know when the receiver picked it up unless it polls the target doc's git log.
- The receiving pane only picks the message up on its **next** user-triggered cycle (editor save, explicit `/agent-doc`). There's no push — unless you `focus` the pane and have a watch running.

**Possible new command** (pending candidate): `agent-doc dispatch <target-doc> "<task>" [--wake]` — wraps `notify` + `focus` + optional `tmux send-keys` to the receiver pane to kick a cycle. Cleaner than remembering the notify + focus dance.

Want me to add `dispatch` as a pending item, or start with just documenting `notify` as the canonical cross-pane task-send mechanism?

❯ What is `agent-doc notify` used for normally?
### Re: what is `agent-doc notify` normally for?

The designed-for use case is **cross-document re-evaluation signals** — one session telling another "something you depend on changed, take another look." Not primarily a task-dispatch mechanism (even though it can be repurposed as one).

**What it does** (from `src/notify.rs`):
- Appends a blockquote to the target document's `agent:exchange` component:
  ```
  > **[NOTIFY from <source>]** (2026-04-14T04:09:18Z)
  > <message>
  > Re-evaluate: <affects>
  ```
- Inserts **before** the boundary marker if one exists (so it reads as new user input on the next cycle, not as stale history).
- Atomic write + snapshot update + optional git commit (same write path as a normal response).

**The flags reveal the intent:**
- `--source <doc-or-session>` — who's sending the signal (e.g. `src/agent-doc/tasks/agent-doc-feat.md`). Lets the receiver trace provenance.
- `--affects <sections>` — emits a `Re-evaluate: <sections>` line, which is the **directive** to the receiving agent: "the following sections of your doc may be stale now." That's the semantic core — it's a change-propagation primitive, not a generic message.

**Canonical scenario:** agent-doc session A edits a shared spec or a dependency that session B's doc references. A runs `agent-doc notify path/to/B.md "Spec X changed: Y is now required" --source path/to/A.md --affects "architecture,pending"`. Next time B's pane runs a cycle, the blockquote is in the diff as a `[user+]` turn, and B re-evaluates the flagged sections.

**Why it's close-but-not-ideal for "send a task to another pane":**
- ✅ Cross-pane reachable — works for any claimed document regardless of which pane owns it.
- ✅ Shows up as user input on the next cycle.
- ❌ No wake — receiver sees it only when its pane naturally runs (editor save, explicit `/agent-doc`, watch daemon). No push.
- ❌ Semantically framed as "re-evaluate," not "do this." Works fine for task-dispatch, but the `[NOTIFY]` header + `Re-evaluate:` line read weird for imperative tasks.
- ❌ No ack path back to sender.

So `notify` is the right **primitive**, but a `dispatch` wrapper (notify + focus + optional tmux send-keys for wake) would be a cleaner ergonomic layer for "send a task." Still waiting on your call: queue `dispatch` as pending, or document `notify` as the canonical cross-pane mechanism and leave it there?
❯ The proposed `dispatch` would swap the tmux pane even if the target document is closed or not viewable in the editor?
### Re: would `dispatch` swap away the active pane?

Good catch — yes, as I loosely described it (`notify + focus + tmux send-keys`), it would, and that's the wrong default. `agent-doc focus` resolves the pane claiming the target doc and calls `tmux select-pane` on it, which yanks the visible pane in the current window regardless of whether the editor is showing the doc. For a background dispatch ("hey B, do this task when you get a chance"), stealing focus from whatever I'm currently looking at is user-hostile.

Rethinking the design along the axis you pointed at — **focus should not be the default**. Three tiers make more sense:

**Tier 1 — fire-and-forget (default):** `agent-doc dispatch <target-doc> "<task>"`
- Same as `notify` but with imperative framing (`[DISPATCH from ...]` header, no `Re-evaluate:` line).
- No tmux interaction at all. No pane swap, no send-keys, no focus.
- Receiver picks it up on its next natural cycle — editor save, explicit `/agent-doc`, or watch daemon tick.
- Safe to run from anywhere, any time, including when the target doc isn't open in the editor.

**Tier 2 — wake without stealing focus:** `agent-doc dispatch ... --wake`
- Does the notify, then sends a command to the **target's claimed tmux pane** via `tmux send-keys -t <pane> '/agent-doc <target-doc>' Enter`. That triggers the receiver to run a cycle immediately.
- Critically: `send-keys` routes by pane ID — it does **not** require the pane to be the active one, and it does **not** change which pane is visible. The receiver runs in the background pane; my current pane stays put.
- Caveat: if the target doc is **not claimed by any tmux pane**, `--wake` is a no-op (or an error). Fire-and-forget still works because the notify is just a file mutation — it doesn't need a live pane.

**Tier 3 — interactive handoff:** `agent-doc dispatch ... --focus`
- Opt-in only. Does notify + wake + `tmux select-pane`. Use when I actually want to hand control over (e.g., "stop what I'm doing, go watch B work on this").
- This is the only tier that touches visibility. Never default.

**On "document closed/not viewable in editor":**
- For Tier 1, totally irrelevant — the notify just writes to the file on disk. The receiver's next cycle will see it whether or not the doc is open in the editor.
- For Tier 2, "viewable in editor" still doesn't matter, but "claimed by a tmux pane" does. If nothing owns the doc, there's no pane to `send-keys` to. Could auto-start a headless receiver pane (`agent-doc start`) but that's scope creep — error out and tell the user to claim or start first is fine.
- For Tier 3, editor visibility still doesn't matter (it's a tmux-level focus, not an editor-level one), but it's reasonable for `--focus` to also trigger the editor plugin to open the target doc. That's plugin-side work though, not dispatch-side.

So the revised proposal: `dispatch` defaults to pure fire-and-forget (safe, no side effects on my current view), `--wake` adds background trigger without disruption, `--focus` is the only flag that changes visible pane state.

Still want me to queue `dispatch` (with this tiered design) as a pending item, or park this in the agent-doc-feat doc as a full proposal first?
❯ Just do a notify. Add an option for notify to add a task.
### Re: extend `notify` with a task option

Confirmed. Queued as pending. Proposed shape:

```
agent-doc notify <target-doc> "<message>" --pending-add
agent-doc notify <target-doc> "<message>" --pending-add --source <src> --affects <sections>
```

Semantics:
- **Without `--pending-add`** (current behavior): append blockquote to target's `agent:exchange` — change-propagation signal.
- **With `--pending-add`**: also (or instead of?) add `<message>` as a new item in target's `agent:pending` via the same code path as `agent-doc write --pending-add`. Hash ID auto-assigned by the binary.
- `--source` annotation: prefix the pending text with `[from <source>]` so the receiver can trace provenance when it picks up the item.

Open sub-questions I'll resolve during implementation (or flag back to you if there's ambiguity):

1. **Exchange + pending, or pending-only?** Two reasonable readings of "add an option for notify to add a task":
   - (a) `--pending-add` is additive — still appends the blockquote to `agent:exchange` *and* adds a pending item. Receiver sees both an inbox message and a queued task.
   - (b) `--pending-add` switches behavior — skip the exchange blockquote, only add to pending. Quieter, doesn't interrupt the receiver's current conversation.
   I'll default to **(b)** — the exchange blockquote would be redundant noise when the task is what matters, and the sender can always run notify twice if they want both.
❯ option b

2. **What if the target has no `agent:pending` component?** Error out with a clear message (`target document has no agent:pending component`). Don't silently fall back to exchange.
❯ Would adding a agent:pending to the target violate user expectation? I would be ok with it. But what is the standard thought on this applied to agent-doc?
3. **Commit behavior:** same as current `notify` — commit by default, `--no-commit` to skip.

Added to pending. Implementation happens when that item gets scheduled — nothing further this cycle unless you want to refine the design first.
### Re: option b + auto-create pending?

Locked in **(b)** for `notify --pending-add` — pending-only, no exchange blockquote.

On auto-creating `agent:pending` in the target: the standard thought in agent-doc is that **components are opt-in surfaces controlled by the document author**. A doc without `agent:pending` hasn't enrolled in the pending workflow — it has no reaper, no granular flags wired in the author's mental model, and the author may have deliberately kept the doc as a plain exchange. Silently materializing the component on someone else's document is the same class of overreach as a sender deciding to add `agent:status` or `agent:architecture` because they felt like it.

The conservative default lines up with how the rest of the system behaves:

- `agent-doc write --pending-*` on a local doc errors if `agent:pending` is absent — it doesn't auto-create.
- Component creation is an authoring action (`agent-doc component add`, template scaffolding), not a side effect of a write.
- Receivers get to decide what surfaces live in their doc.

So the principled default for `notify --pending-add` is: **error if the target lacks `agent:pending`**, with the error message telling the sender exactly how to fix it (`ask the recipient to run agent-doc component add pending`, or similar).

Escape hatch for your "I'd be OK with it" case: an explicit **`--create-pending`** flag on `notify` that opts the sender into materializing the component on the target. Off by default, on when you know the recipient won't mind. That keeps the principle intact while giving the pragmatic path you want for your own docs.

Want me to add that `--create-pending` flag to the pending item, or keep it strict-only for v1?

❯ Right now, I'm the author of the sending + receiving documents.
❯ We may support multi-user agent-doc workspaces in the future.
### Re: single-author now, multi-user later

That flips the default. When you own both ends, "violating author expectation" is vacuous — you *are* the author on both sides. The conservative default exists to protect a stranger's document; there's no stranger yet.

**Revised recommendation for v1:** auto-create `agent:pending` on the target when absent. Ergonomic, zero friction, matches today's reality.

Forward-compat path for when multi-user lands:

1. **Invert the flag.** Ship with `--no-create-pending` (opt-out) instead of `--create-pending` (opt-in). Default = create. Users who want strict behavior have an explicit knob.
2. **Recipient-side policy later.** When multi-user workspaces arrive, add a recipient config (e.g., `.agent-doc/config.toml` → `receive.auto_create_components = ["pending"]` or `= false`). Recipient decides what senders can materialize on their docs. This moves the consent from sender-flag to recipient-config, which is where it belongs in a multi-user world.
3. **Migration is a no-op for you.** Since you're the recipient of your own docs today, flipping the recipient default to strict later wouldn't break anything — you'd just add `auto_create_components = ["pending"]` to your workspace config and keep working.

Net: ship auto-create as default now, with `--no-create-pending` as the escape hatch. Plan the recipient-policy TOML knob as a multi-user follow-up, not v1.

Updating the pending item to reflect this — want me to reword it to "notify: auto-create agent:pending on target (with --no-create-pending opt-out)"?
❯ Yes.
❯ How can we track **Recipient-side policy later.**? This may happen weeks from now.
### Re: reword confirmed + tracking the multi-user follow-up

Reworded `#txex` to match the new design: auto-create default, `--no-create-pending` opt-out, pending-only behavior.

For the **recipient-side policy** work — that's exactly what `[/]` gated state is designed for. Gated items:

- Are **not reaped** by preflight, so they persist indefinitely without cluttering active work.
- Show up in `pending_gated_count` in preflight JSON, so you can see at a glance how many long-term trackers exist.
- Transition `[/] → [ ]` via `--pending-ungate <id>` when you're ready to schedule the work, then through the normal `[ ] → [x]` active lifecycle.
- Survive pending reorders and stay visible in the tail of the list as a roadmap marker.

**What I just did:** added the multi-user follow-up as a new pending item. I tried `--pending-gate "last"` to gate it in the same call, but that alias isn't implemented — the binary rejected it (`pending gate: no item with id [#last]`). So the item landed as an active `[ ]` for now. Next cycle I'll read the fresh hash from preflight and run `--pending-gate <hash>` to flip it to `[/]`. I tagged the item text with `[MULTI-USER FOLLOW-UP, gate next cycle]` so it's visually distinct until the gate lands.

**Small feature idea** (not adding to pending unless you want): `--pending-gate last` and `--pending-gate N` (gate the Nth-most-recent add) would be a clean ergonomic improvement for this exact workflow. Currently you have to do a two-cycle dance to add-and-gate.

**Alternative tracking mechanisms** if pending-gated feels wrong for long-horizon work:

- **tasks/software/agent-doc.md or a dedicated roadmap doc** — narrative tracker, not lifecycle-managed. Good for architecture notes, weak for "remind me when X lands."
- **A `[MULTI-USER]` tag prefix convention** — search by tag instead of state. Simpler but no first-class reap protection.
- **GitHub issue on agent-doc** — external tracker, good if you want it visible to collaborators and linked from code.

Gated-pending wins for single-user workspaces because it keeps the tracker adjacent to the code it describes and inherits lifecycle machinery. If you later want GitHub visibility, we can mirror gated items → issues via a sync command.
❯ Gated-pending with a gh issue.

❯ In src/session-share/tasks/claudescore.md, my last prompt where the entire line is `#zj6s` did not get a `❯ ` prefix in the patchback.
### Re: gated-pending + gh issue, and the prefix bug

**Done on the multi-user follow-up:**

- Created [btakita/agent-doc#40](https://github.com/btakita/agent-doc/issues/40) with full context, TOML sketch, and the "do not ship before" note.
- Marked the duplicate `#e4pc` done (it'll reap next cycle) — that was stray fallout from the failed `--pending-gate "last"` call last round, which partially landed before erroring.
- Edited `#ha3h` to include the issue link and tightened the wording.
- Gated `#ha3h` to `[/]` — preflight should report four gated items next cycle (was 3) and stop touching it.

The gh issue gives external visibility; the gated-pending keeps the tracker adjacent to the code. When multi-user lands, you ungate `#ha3h`, close the issue with a link to the implementing PR, and the loop closes.

### Re: prefix bug on hash-only user-prompt lines

Nice catch. Filed as a new pending item. Working theory:

`normalize_user_prompts_in_exchange` (src/agent-doc/src/write.rs) rebuilds added lines within the exchange component as user-prompt prefixed so the next cycle sees them as user prompts. There's almost certainly a short-circuit that skips lines that look like agent metadata — checkbox items, hash ids in brackets, boundary markers, patch blocks. A line that is literally just `#zj6s` (a bare hash) probably trips the "looks like an id reference" branch and gets passed through unprefixed.

Fix candidates:

1. **Scope the skip to actual metadata shapes.** A line matching `^[-*] \[[ x/]\] \[#[a-z0-9]+\]` is a pending item and should stay as-is. A bare `^#[a-z0-9]+$` line is almost certainly a user prompt referencing a pending id — prefix it.
2. **Anchor the skip to the component.** Only skip prefix-rewrite inside `agent:pending`, `agent:boundary:*`, and patch/component open/close comments. Inside `agent:exchange`, every added line gets prefixed, full stop.

Option 2 is the cleaner invariant — the component tells us the context, so don't second-guess the content. I'll pick it up when that item gets scheduled. If you want me to investigate right now instead of queuing, say the word and I'll pull up `write.rs` and pinpoint the branch.

❯ do Option 2

### Re: do Option 2

Done. Landed the cleaner invariant in `normalize_user_prompts_in_exchange` (`src/agent-doc/src/write.rs`):

- Dropped the content-based skips (`starts_with('#')`, `starts_with('"')`).
- Tightened the HTML-comment skip to `<!--` (still excludes component/patch/boundary markers).
- Inside `agent:exchange`, every added line in the user region now gets the `❯ ` prefix. Structural markers and fenced code blocks remain excluded.

Tests:

- New regression: `normalize_user_prompts_hash_ref_prefixed` — bare `#zj6s` gets `❯ ` (the #vnxg repro).
- Updated `normalize_user_prompts_heading_in_user_region_prefixed` and `normalize_user_prompts_quoted_string_prefixed` to match the new invariant — user-typed headings and quoted lines now get the prefix.
- Full `cargo test --bin agent-doc write::` suite: 62 passed, 0 failed.

Pending item `#vnxg` marked done.
❯ do #25ag
### Re: #25ag landed — `patch:pending` → `replace:pending` with dual-accept

Implementation complete in `src/agent-doc`. Version bumped to **0.32.4** (`Cargo.toml` + `pyproject.toml`). All pending-system tests green (24/24), write suite green (62/62), clippy clean, release binary built. Pre-existing `resync::tests` / `route::tests` failures are tmux-environment flakes unrelated to this change.

**Parser change (`src/template.rs::parse_patches`):** Added a second prefix branch — the scanner now recognizes `<!-- replace:pending -->...<!-- /replace:pending -->` as a full synonym for the old `<!-- patch:pending -->` form. Scope is tight: `replace:` is only accepted when the component name is exactly `pending`. Other `replace:*` names fall through as unmatched rather than silently broadening the grammar to every component. The close marker tracks the open prefix (`</patch:pending>` closes a `patch:` open; `</replace:pending>` closes a `replace:` open) so mixed open/close pairs don't accidentally match. When the parser sees `patch:pending`, it emits a stderr deprecation warning **per block** so callers find and update their sites during the dual-accept window.

**Enforcement rename (`src/write.rs`):** `enforce_no_patch_pending` → `enforce_no_replace_pending` (call sites updated at lines 823, 925, 1315, 1499). The function checks both env vars — either `AGENT_DOC_ALLOW_REPLACE_PENDING=1` (canonical) or `AGENT_DOC_ALLOW_PATCH_PENDING=1` (deprecated alias) authorizes the escape hatch. Error message now says `ERR: replace:pending block forbidden — use --pending-add/done/edit/clear/reorder.` Enforcement still triggers on `name == "pending"`, so both block forms hit the same gate regardless of which prefix opened them.

**CLI flag rename (`src/main.rs`):** `--allow-patch-pending` → `--allow-replace-pending` via clap `alias`. Struct field renamed `allow_patch_pending` → `allow_replace_pending`. The CLI sets **both** env vars when the flag is passed, so any library-layer code still reading only the old env name keeps working through the migration window. Hidden flag stays hidden.

**Tests added/renamed (`tests/pending_integration.rs`):**

1. `write_rejects_replace_pending_block` — default-reject on canonical form.
2. `write_rejects_legacy_patch_pending_block` — default-reject on deprecated form, **asserts stderr contains `deprecated`** (pins the warning emission).
3. `write_allows_replace_pending_with_escape_hatch` — happy path: canonical form + canonical flag.
4. `write_allows_legacy_patch_pending_with_legacy_flag` — legacy form + legacy flag routed via clap alias, asserts deprecation warning still emitted.
5. `write_allows_replace_pending_with_legacy_env_var` — library-layer compat: canonical block + `AGENT_DOC_ALLOW_PATCH_PENDING=1` (old env var) still authorizes.
6. `write_rejects_replace_pending_via_library_default` — phase-3 inversion intact for library callers, both env vars unset.

All six pass in isolation (`cargo test --test pending_integration -- --exact ...`). The renamed tests replace the two old `write_{allows,rejects}_patch_pending_*` cases; the legacy tests exercise the deprecation-warning path that didn't exist before.

**Docs updated:**

- `VERSIONS.md` — 0.32.4 entry with full rationale, test list, and a **"next release removes dual-accept"** call-out so the migration window is finite and visible.
- `specs/pending-system.md` — `§4 Granular write-command surface` and `§6 Enforcement` now mention both forms; added a **#25ag rename paragraph** documenting canonical names, deprecated aliases, and the dual-accept policy.
- `.claude/skills/agent-doc/SKILL.md` — `§1b Update pending` references `<!-- replace:pending -->` as canonical; escape hatch paragraph explains the deprecation status of `--allow-patch-pending` / `patch:pending`.

**What's NOT in this change (deliberate):**

- `src/agent-doc/SKILL.md` (the bundled skill) still has the *old Phase 2* guidance ("every response MUST include a patch:pending block") which is now outright inverted by Phase 3 — touching it is a separate skill-rewrite job, not part of #25ag. Filed as follow-up below.
❯ We need to do this before the release
- No release (crates.io / PyPI / GitHub Release). Per release checklist, `cargo install --path .` + live manual-testing gate must run first, and that's your call.
- `--pending-add` still writes text literally — the `[/]` state-marker footgun (`#9v7h`) is a separate queued item.

**Invariants verified:**

- Default-reject holds for both forms — you cannot land a `replace:pending` or `patch:pending` block without an explicit opt-in.
- Granular-flag path (`--pending-add/done/edit/clear/reorder/gate/ungate`) is unchanged — the dual-accept is strictly additive at the parser layer.
- Hash-id stability is untouched — no code path assigns or churns ids during this change.
- Parser scope: the new `replace:` branch only matches `replace:pending`. `<!-- replace:status -->` or any other `replace:*` falls through as unmatched text, which keeps the grammar surface small.

**Follow-ups queued (not this cycle):**

1. `src/agent-doc/SKILL.md` rewrite to match Phase 3 + #25ag — the bundled skill doc is drifted.
2. Next-release removal of `patch:pending` parsing + `--allow-patch-pending` alias + `AGENT_DOC_ALLOW_PATCH_PENDING` env var.
3. Sweep of any other `patch:pending` references I may have missed in submodule task docs / runbooks (this change only touched the `src/agent-doc` crate + the canonical skill file).

Marking `#25ag` done via `--pending-done 25ag` in this write cycle. Preflight will reap it next cycle.

Pending item `#25ag` marked done.

### Re: Bundled skill rewrite is a release gate

Acknowledged — promoting the `src/agent-doc/SKILL.md` rewrite from "follow-up" to **release gate for #gqep**. You're right: shipping v0.32.4 with the bundled skill still saying "every response MUST include a patch:pending block" would be actively harmful — Phase 3 inverts that rule (granular ops only, `replace:pending` forbidden in normal cycles), and any agent loading the bundled skill from crates.io/PyPI would immediately violate the contract.

**What the rewrite needs to cover** (mirroring `.claude/skills/agent-doc/SKILL.md` §1b):

1. **Delete the old Phase 2 mandate** — no more "every cycle must emit `<!-- patch:pending -->`". That block type is deprecated.
2. **Add the granular-ops contract** — `--pending-add`, `--pending-done <id>`, `--pending-edit "id=text"`, `--pending-clear`, `--pending-reorder`. Explain hash IDs + GFM checkboxes + lazy backfill.
3. **Document `pending_reordered` semantics** — skip reorder for one cycle after user expresses priority.
4. **Escape hatch paragraph** — `--allow-replace-pending` + `AGENT_DOC_ALLOW_REPLACE_PENDING=1`, only for compaction/migration/tests; `--allow-patch-pending` + `patch:pending` as deprecated aliases for one release with stderr warning.
5. **Cycle example** — the multi-flag `agent-doc write` invocation showing add + done + edit in one call.
❯ Move detailed instructions into a runbook.
**Gating changes:**

- Added new pending item for the rewrite (release gate, queued ahead of #gqep).
- Rewrote #gqep to explicitly mark "BLOCKED on bundled SKILL.md rewrite + manual-testing gate" so nothing accidentally triggers publish.

**What I'm NOT doing yet:** the rewrite itself. It's a substantive edit to `src/agent-doc/SKILL.md` (~a few hundred lines touched), and per process discipline I'd rather land it as its own focused change with a diff you can review, not fold it into this cycle. Say "do the bundled skill rewrite" (or similar) and I'll execute it next.
❯ do the bundled skill rewrite
### Re: Bundled skill rewrite — landed (runbook extraction)

Done. Followed your "move detailed instructions into a runbook" guidance — `src/agent-doc/SKILL.md` §1b is now a tight summary that links out, and the full contract lives in a new runbook.

**Changes:**

1. **New file** `src/agent-doc/runbooks/pending-ops.md` — full granular-ops contract:
   - Item shape (hash IDs, GFM checkboxes, `[/]` gated syntax)
   - All 7 flags in a table (`--pending-add`, `--pending-done`, `--pending-edit`, `--pending-clear`, `--pending-reorder`, `--pending-gate`, `--pending-ungate`)
   - `pending_reordered` semantics
   - Multi-flag cycle example
   - Escape hatch paragraph (`--allow-replace-pending` + `AGENT_DOC_ALLOW_REPLACE_PENDING=1`, and the one-release deprecation alias for `--allow-patch-pending` / `patch:pending`)

2. **`src/agent-doc/SKILL.md` §1b rewritten** — replaced the inverted Phase 2 mandate ("every response MUST include a `<!-- patch:pending -->` block") with a 3-paragraph summary:
   - Granular flags are canonical; `replace:pending` / `patch:pending` forbidden in normal cycles
   - Preflight lazy-backfills hashes + checkboxes; `pending_reordered` skip-reorder rule
   - Link to `runbooks/pending-ops.md` for the full contract and `specs/pending-system.md` for the spec

3. **Runbooks section** — added pending-ops entry alongside compact-exchange and transfer/extract.

**What I deliberately did NOT touch in this cycle:**

- `agent-doc-version` in frontmatter (still `"0.32.0"`) — bumping it triggers the auto-update path on next skill invocation, which is a release-adjacent concern. Your call whether to land that in the same v0.32.4 bump or separately.
- The preflight JSON example in §0 — it's missing `pending_reordered`, `pending_gated_count`, `diff_type`, `annotated_diff`, `env`, `layout_issues`, `pending_callbacks`. Drift but not load-bearing for the Phase 3 correctness fix. Filing as follow-up.
- Release itself. #gqep is still blocked on the manual-testing gate — I edited the text to reflect that the bundled SKILL.md rewrite sub-dependency has landed, but the release itself is still your call.

**Release-gate status for #gqep:** bundled SKILL.md rewrite ✅ landed. Remaining gate: `cargo install --path .` + manual-testing confirmation in a live session before crates.io/PyPI/GitHub Release publish.
❯ One more issue. Both of these files do not have the uncommitted removal of ` (HEAD)` in the header. Why?
❯ - src/boost-client/tasks/monsterrodholders.md
❯ - src/session-share/tasks/claudescore.md
$(cat /tmp/agent-doc-resp-$$.md)
❯ The patchback did not occur in this doc. Why?

### Re: Why didn't the patchback occur last cycle?

Skill-author bug on my end, not a binary bug. My previous cycle used a **quoted heredoc** to pipe the response into `agent-doc write`:

```
cat <<'RESPONSE' | agent-doc write ...
$(cat /tmp/agent-doc-resp-$$.md)
RESPONSE
```

The single-quoted delimiter disables all shell expansion inside the heredoc, so `$(cat ...)` was passed through as a literal 32-byte string instead of being replaced with the file's contents. `agent-doc write` saw one line of raw (unwrapped) content, synthesized a boundary, and appended the literal `$(cat ...)` string into `&lt;!-- agent:exchange --&gt;`. That's the stray line sitting above your follow-up.

So the patchback **did** occur — it just wrote the wrong content. The real response (the `### Re:` analysis of why the two swept files lack the working-tree `(HEAD)` removal) never reached the binary at all.

**Fix:** either use an unquoted heredoc so `$(...)` expands, or — simpler and what I'm using now — pass the response file to `agent-doc write` via stdin redirection from a real file (`< /tmp/resp.md`) with no command substitution involved.

I'm re-delivering the original answer inline:

---

Both swept files **do** now have the uncommitted `(HEAD)` removal in the working tree — the sweep this preflight runs is what creates it. From `git diff` against the index:

`src/boost-client/tasks/monsterrodholders.md`:
```
-## Pending / Not Built (HEAD)
+## Pending / Not Built
```

`src/session-share/tasks/claudescore.md`:
```
-### Expansion Phases (Future) (HEAD)
+### Expansion Phases (Future)
```

Plus a stale `<!-- agent:boundary:... -->` marker being dropped at the old position.

❯ Make this ` (HEAD)` removal logic deterministic in the agent-doc binary + ffi plugin (JB plugin only if necessary) if possible.

**Why it looks invisible until the sweep runs:** sweep commit and working-tree boundary reposition are decoupled.

- `reposition_boundary_in_snapshot` (git.rs:593) only updates the snapshot file under `.agent-doc/snapshots/`. Its doc-comment spells it out: *"The plugin handles working-tree boundary reposition via the `reposition_boundary: true` IPC flag sent during `agent-doc write`."*
- So a file's working-tree heading only gains/loses `(HEAD)` when (a) `agent-doc write` runs on it and the open editor plugin receives an IPC ping, or (b) the next `agent-doc commit` stages the snapshot into the index, at which point `git diff` shows the lag between index (new boundary position) and working tree (old boundary position).
- Sweep-committed docs never go through `agent-doc write` — they're foreign docs preflight happens to notice dirty. So (a) is skipped, and (b) is what just produced the visible diff.

**The residual gap:** without an editor actively holding those files open and wired to the plugin, the working tree continues to lag until someone opens them or another write cycle targets them. Two candidate fixes, now in pending:

1. **Have `reposition_boundary_in_snapshot` also rewrite the working tree** when no active run is in progress. It already has the canonical path and the pending-run guard. Cheapest, but sweep commits would start touching working trees of unrelated files — potentially surprising for editors that don't have the plugin loaded.
2. **Emit an IPC `reposition_boundary` notification** from the sweep path, matching what `agent-doc write` does. Cleaner separation but requires a running plugin to take effect.

#1 looks like the lower-friction fix and matches what the existing comment already implies the snapshot path "should maybe" do. #2 is the purist answer. Let me know which way you want it, or if you'd rather hold off.
❯ reposition_boundary_in_snapshot seems fine.

### Re: Deterministic `(HEAD)` removal in the binary

Going with option #1 — extending `reposition_boundary_in_snapshot` (git.rs:593) to also rewrite the working tree with `template::reposition_boundary_to_end` + `strip_head_markers`, gated on the existing active-run guard (`pending_path_for`). Sweep-committed foreign docs have no active run, so they'll get the deterministic rewrite; in-flight runs are skipped so concurrent IPC/plugin writes don't race.

Implementing now in the agent-doc binary only — no FFI/plugin changes needed since the binary is the canonical source of truth. I'll leave `try_ipc_reposition_boundary` in place as a hint to open editors, but the on-disk state will be correct regardless of whether the plugin is loaded.

### Re: Implemented — deterministic working-tree boundary reposition

**Change:** `reposition_boundary_in_snapshot` (git.rs:593) now rewrites both the snapshot AND the working tree under the same active-run guard.

**Diff** (src/agent-doc/src/git.rs):
- Snapshot path unchanged: `template::reposition_boundary_to_end` still drops stale `<!-- agent:boundary:... -->` markers and inserts a single fresh one at the end of exchange
- Working-tree path added: reads the file, applies `reposition_boundary_to_end` + `strip_head_markers`, atomic-writes if the content differs
- Both transformations preserve user edits — they only touch boundary lines and ` (HEAD)` heading suffixes
- Returns `true` if either write changed content (so the existing `try_ipc_reposition_boundary` hint still fires)

**Why this is safe:**
- `pending_path_for` guard is unchanged — active `agent-doc write` runs still own the transition via IPC, preventing races
- Outside an active run, the binary becomes the canonical source of truth regardless of whether a plugin is loaded — fixes the foreign sweep-committed doc case directly
- No-op writes are skipped (`cleaned != working` check), so IDEs with open editors only see "externally modified" when something actually needed fixing
- No FFI/plugin changes required — the skill requirement "+ ffi plugin (JB plugin only if necessary)" reduced to zero plugin work

**Verification:**
- `cargo clippy` clean
- 274 lib tests pass
- 7 existing `reposition_boundary*` tests pass (underlying `template::reposition_boundary_to_end` and `strip_head_markers` primitives are already unit-tested)
- `cargo install --path .` + `cargo build --release` rebuilt both the global binary and `.bin/agent-doc` symlink target
- No new test added for the git.rs function itself — it's a thin composition of already-covered primitives behind the pending-path guard
❯ do #e6z1

### Re: #e6z1 — can't execute, upstream implementation not landed

`#e6z1` is the release-gate task: *"version bump + VERSIONS.md entry for gated-state feature **after implementation lands**"*. The gated-state implementation items are all still open:

- `#pf01` spec ✅ landed
- `#mgdw` parser — not started
- `#h1j2` reaper — not started
- `#q90h` CLI `--pending-gate` / `--pending-ungate` — not started
- `#sx35` tests — not started

Bumping the version + writing a VERSIONS.md entry now would ship a release that advertises the feature without any of the parser/reaper/CLI/test code actually present. That's the exact failure mode the gate was written to prevent.

**What I can do instead, pick one:**

1. **Execute `#mgdw` → `#h1j2` → `#q90h` → `#sx35` as a bundle**, then come back to `#e6z1` for the version bump. Roughly a parser extension + a preflight field + two CLI flags + a test sweep. This is the path that actually discharges `#e6z1`.
2. **Re-scope `#e6z1`** if you meant something narrower (e.g. "draft the VERSIONS.md entry now as a stub so it's ready when impl lands") — tell me and I'll edit the pending text.
3. **Override the gate** — explicitly say "ship the version bump now without the feature code" and I'll do it, but I'd push back hard because the bundled skill (`#hrmt`) is also a release gate for `#gqep` and you'd be stacking half-ready gates.

My read: you probably want option 1. Confirm and I'll start on `#mgdw` (parser + `PendingState::Gated` + `[/]` round-trip) as the first slice.
❯ do option 1

### Re: option 1 — the bundle was already shipped; only the VERSIONS.md entry was missing

Walked the tree before writing any code and found everything already landed across three commits:

- `c53b09b` — **Phase 1:** `PendingState` enum (`Open` / `Gated` / `Done`), `PendingOp` enum (`Gate` / `Ungate` / `MarkDone`), `validate_transition` state matrix, parser round-trip for `[/]`, `box_char` / `from_box_char` primitives in `src/pending.rs`
- `fa31f78` — **Phase 2:** CLI flags wired — `--pending-gate <id>` / `--pending-ungate <id>` on `agent-doc write` (main.rs:437–442, 1098–1101), gate/ungate ordering before done in main.rs:1084, reaper skips `Gated` items in preflight
- `69b5475` — **Phase 3:** `pending_gated_count` field in preflight JSON (preflight.rs:196, 512, 706), plus the earlier `patch:pending` → `replace:pending` enforcement inversion

Which means `#mgdw` / `#h1j2` / `#q90h` / `#sx35` were already discharged — I was about to re-implement work that's already in `main`.

**Verification run (fresh, just now):**
- `cargo test --lib -q` — 274 passed
- `cargo test --test pending_integration -q` — 24 passed (covers `write_pending_gate_open_to_gated`, `write_pending_gate_idempotent_on_gated`, `write_pending_gate_done_errors`, `write_pending_gate_then_done_in_one_call`, `preflight_emits_pending_gated_count`, `preflight_omits_pending_gated_count_when_zero`)
- Live signal from this very cycle's preflight: `pending: 4 gated item(s)` + `"pending_gated_count": 4` in the JSON — the feature is running in production against this exact document

### Re: #e6z1 — VERSIONS.md entry landed, no version bump needed

`0.32.4` is **not yet published** (`cargo search agent-doc` shows `0.32.3`; `git tag -l 'v0.32.*'` stops at `v0.32.3`). So `#e6z1` collapses to *"append the gated-state feature notes to the in-progress 0.32.4 section"* — no `Cargo.toml` / `pyproject.toml` bump required.

**Change to VERSIONS.md:**
- New top entry under `## 0.32.4`, above the existing `patch:pending → replace:pending` bullet
- Cites the five pending-ids (#pf01, #mgdw, #h1j2, #q90h, #sx35) so git-blame ↔ tasklist is recoverable
- Documents: the `[/]` canonical form, state-transition matrix + rationale for illegal/no-op cells, CLI flag combinability (gate-before-done ordering), preflight's `pending_gated_count` signal (emitted when ≥1, omitted at zero), reaper skip semantics, the concrete test names that cover each path, and the original rationale for why `[/]` exists at all (release-gated tasks had no lexical distinction from active work)
- `Cargo.toml` / `pyproject.toml` untouched — both still at `0.32.4`

**Marking done this cycle:** `#mgdw`, `#h1j2`, `#q90h`, `#sx35`, `#e6z1`. All five were the gated-state bundle + its release-gate meta-item, and all five are discharged.

**What's still on the release path to `#gqep` (v0.32.4 publish):**
1. `#hrmt` — rewrite bundled `src/agent-doc/SKILL.md` to invert Phase 2 pending guidance (remove "every response MUST include a patch:pending block", replace with granular-ops contract). Still the hard release gate.
2. Manual-testing gate — per `feedback_testing_before_release.md`, pause for your live-session confirmation before `cargo publish` / `maturin publish` / `gh release create`.
3. `cargo install --path .` + `cargo build --release` after publish so `.bin/agent-doc` and `~/.cargo/bin/agent-doc` both track the published build.

Ready to start on `#hrmt` next if you want to keep the release train moving, or stop here so you can eyeball the VERSIONS.md prose first.

❯ src/boost-client/tasks/monsterrodholders.md and src/session-share/tasks/claudescore.md is still not having the ` (HEAD)` in uncommitted changes during a patchback. Note that both of these agent docs are in a submodule. Sanity check, the pwd for claudescore.md is incorrect. It should be src/session-share. monsterrodholders.md pwd should be src/boost-client:
```
! pwd
  ⎿  /home/brian/work/btakita/agent-loop
```

### Re: Submodule docs missing `(HEAD)` + wrong pwd

Two separate bugs, both rooted in the same asymmetry: commit logic routes through the submodule repo, but peripheral helpers still resolve against the superproject.

**Bug 1 — `(HEAD)` marker missing in submodule docs.**

Traced it in `src/agent-doc/src/git.rs`:

- `commit()` calls `narrow_to_submodule(super_root, resolved)` at git.rs:309 → git ops run inside the submodule. Good.
- `add_head_marker(snap, file)` at git.rs:404 then calls `show_head(file)` to diff against HEAD.
- `show_head()` at git.rs:1051 uses `resolve_to_git_root(file)` which, for absolute paths, **prefers the superproject** (git.rs:117-119). So it runs `git show HEAD:src/session-share/tasks/claudescore.md` from the *superproject* root.
- Superproject can't `git show HEAD:<submodule-tracked-path>` — the gitlink blocks path traversal. `output.status.success() == false` → `show_head` returns `Ok(None)`.
- Back in `add_head_marker`, `head_content = None` → falls through to the "mark last heading only" branch (git.rs:859). In theory that still stamps `(HEAD)` on the tail heading, but it also skips the "re-apply from HEAD" safety at git.rs:864 that handles *no-new-heading* cycles. Any boundary-only or pending-only cycle on a submodule doc gets no marker at all — the snapshot stages clean, commit succeeds, working tree strips, gutter goes flat.

Fix: `show_head` (and any other HEAD-lookup helper in this file) needs to route via `narrow_to_submodule` the same way `commit()` does. Concretely, after `resolve_to_git_root`, narrow the `git_root` to the submodule and strip the rel_path against *that* root before `git show HEAD:...`.

**Bug 2 — Pane pwd is super root, not submodule root.**

When `agent-doc start` / `agent-doc route` spawns the tmux pane for a submodule doc, the `-c <cwd>` passed to `tmux new-window`/`new-session` is `std::env::current_dir()` (see route.rs uses at :644 and all the test-site :1038+ call sites). Whatever directory the caller happened to be in leaks into the pane, which for `/agent-doc` invocations is almost always the agent-loop super root.

Fix: add a `resolve_pane_cwd(file)` helper that does `narrow_to_submodule(super_root, resolved).0` and returns that (falling back to `super_root` for non-submodule files), then route start.rs / route.rs spawn paths through it. Gives `src/session-share` for claudescore.md, `src/boost-client` for monsterrodholders.md, and doesn't change behavior for top-level docs.

Both fixes are additive and independently testable — I can land them as a pair or split.

Added to pending. Not touching code yet pending your go-ahead.
❯ fix both right now

### Re: Both fixes landed — awaiting live verification

Implemented in `src/agent-doc`:

**Fix 1 — `show_head()` narrows to submodule** (`src/git.rs`)
- After `resolve_to_git_root(file)`, pipe through `narrow_to_submodule(&super_root, &resolved)` so `git show HEAD:<rel>` runs from the submodule's own repo.
- `rel_path` is now stripped against the narrowed `git_root`, so the HEAD lookup succeeds for submodule-tracked docs and `add_head_marker` gets real HEAD content instead of falling through to the "mark last heading only" branch.

**Fix 2 — `resolve_pane_cwd()` helper** (`src/git.rs` + `src/route.rs`)
- New pub helper: `resolve_pane_cwd(file: &Path) -> PathBuf`. Returns the submodule git toplevel for submodule-hosted docs, super-root for top-level docs, `current_dir()` only as last-resort fallback.
- `route::auto_start_in_session` now uses `crate::git::resolve_pane_cwd(file)` instead of `std::env::current_dir()` at route.rs:644. All four downstream `tmux.split_window` / `tmux.auto_start` call sites (671/686/707/719) pick it up automatically via the single `cwd` var.

**Verification:**
- `cargo build` — clean
- `cargo test --lib -- --skip tmux` — 274 passed
- `cargo clippy --all-targets` — no new warnings (only pre-existing noise in `diff.rs`, `write.rs`, `sync.rs` etc.)
- `cargo install --path .` + `cargo build --release` — `~/.cargo/bin/agent-doc` and `.bin/agent-doc` both updated to the new build

**Manual testing gate (per feedback_testing_before_release.md):**
I'm stopping here for you to live-verify in a claudescore / monsterrodholders session before any release motion. To test:

1. Open `src/session-share/tasks/claudescore.md` (or `src/boost-client/tasks/monsterrodholders.md`) via `/agent-doc`.
2. Confirm the spawned Claude pane's `! pwd` reports the submodule root (`…/src/session-share`), not `…/agent-loop`.
3. Run a normal response cycle and check that the new `### Re:` heading shows the blue `(HEAD)` gutter boundary in the editor diff markers.
4. Kill the pane, re-open, run a boundary-only cycle (pending-only change, no new heading) — confirm the `(HEAD)` marker on the prior heading persists via the re-apply-from-HEAD path that show_head now actually reaches.

If all four pass, I'll mark the pending items done and queue the v0.32.4 release. If anything misbehaves, flag it and I'll iterate before touching the gate.
❯ Looks good.
❯ One issue. There is too much commit noise in src/session-share/tasks/claudescore.md
❯ It appears there are 2 commits on the agent:boundary id per turn. Please propose a way to at least reduce to 1 commit. Even better if we can roll the agent:boundary move into the patchback commit.
### Re: boundary commit noise — proposal

Confirmed from `git log tasks/claudescore.md`:

```
ec56aa6 boundary 574921c7 → 221a1a69   15:26:47
1f6b618 boundary 221a1a69 → 7ca2aae4   15:26:47
```

Two commits per turn, identical timestamp, each rotating the boundary id. Root cause is the commit/reposition ordering in `src/agent-doc/src/git.rs` (~L520–567):

1. Stage snapshot (old boundary id) + `git commit` — **commit A**
2. Strip `(HEAD)` from snapshot
3. `reposition_boundary_in_snapshot()` — rewrites snapshot + working tree with a **new** boundary id

Step 3 lands *uncommitted*. The next turn's preflight commit picks it up as **commit B**, separate from that turn's actual response. So every turn commits the previous turn's boundary rotation alongside its own content.

**Proposal — fold reposition into the same commit (preferred):**

Reorder `commit_file()` so reposition happens **before** staging, and the committed blob carries both the response and the new boundary id atomically:

```
1. load snapshot (response + old boundary)
2. reposition_boundary → snapshot now has new boundary id
3. inject (HEAD) markers → staged blob
4. hash-object + update-index + git commit      ← single commit, new boundary id
5. strip (HEAD) from snapshot
6. working tree: reposition boundary + strip (HEAD)
```

Result:
- Committed content already has the fresh boundary id → nothing left over for next cycle
- Next turn's preflight commit is a genuine no-op when only the boundary rotated
- Blue `(HEAD)` gutter mechanism unchanged — still relies on `(HEAD)` in staged content vs. stripped working tree

**Risk / edge cases to validate:**

- The active-run guard (`pending_path_for`) in `reposition_boundary_in_snapshot` currently skips reposition mid-run. Moving reposition before commit means we need to preserve that guard at the new call site — if a run is active, commit the old boundary id and let the in-flight write's IPC path handle it (unchanged behavior).
- Submodule pointer update (`update_parent_submodule_pointer`) runs after the submodule commit — still fires once per real commit, so no extra parent commits.
- Preflight's "gap commit" on the next turn will now see a clean state; the `file is N bytes larger than snapshot` warning should disappear for boundary-only gaps.

**Fallback (simpler, if the reorder is risky):** detect boundary-only diffs in the preflight commit path and skip them — `git commit` becomes a no-op when the only staged change is a single-line boundary marker rewrite. Cheaper to implement but leaves a small window where the working tree has an uncommitted boundary id.

Let me know which direction you want and I'll open the patch. Preference would be the reorder — it's the structurally correct fix and makes the ordering match the intent ("one commit per turn, atomic").

❯ do the reorder
### Re: reorder done

Implemented in `src/agent-doc/src/git.rs`:

1. Added a **pre-commit** reposition block right after the snapshot-bootstrap step (before staging). Calls `reposition_boundary_in_snapshot(file)` and then reloads `snapshot_content` from disk so the staged blob carries the fresh boundary id.
2. Removed the **post-commit** reposition block. The IPC `try_ipc_reposition_boundary(file)` call is preserved so the plugin's in-memory Document buffer still picks up the new boundary without a disk reload.
3. The active-run guard (`pending_path_for`) inside `reposition_boundary_in_snapshot` still applies — if a concurrent `agent-doc write` is in flight, reposition is skipped and the IPC path owns the transition. Unchanged for that case.

**Verification:**
- `cargo build --release` → clean
- `cargo test --release reposition` → 7 passed, 0 failed (all boundary-lifecycle tests)
- Next turn will test the live fix on this document — expect **one** commit per turn instead of two.

If you want extra confidence before shipping, I can also:
- Add a regression test that asserts "two consecutive commits produce no boundary-only delta between them"
- Run `make check` (clippy + full test suite)

Let me know if you want either before I queue a release.
❯ do [#14z4] [#bak5]
### Re: #14z4 + #bak5 done

Both landed in `src/agent-doc/src/pending.rs` and `src/preflight.rs`.

**#14z4 — Hash collision ceiling (8-char fallback)**

- `generate_hash_n(text, doc_id, counter, width)` — variable-width hash, `width` clamped to `[4, 8]`. Width-4 output is bit-identical to the pre-#14z4 formula (existing docs don't churn on re-backfill — verified by a new test).
- `generate_hash(text, doc_id, counter)` kept as a thin width-4 wrapper with `#[allow(dead_code)]` so the backward-compat API + tests continue to work.
- `assign_unique_hash(text, doc_id, taken)` — collision-escalation loop. Starts at width 4, budgets 4 retries per width, widens by 1 on exhaustion, ceiling at width 8 per spec §1. At width 8 (40 bits of entropy) it keeps spinning counter forever — a further collision is effectively impossible.
- Both `backfill` and `op_add` now call `assign_unique_hash` instead of the open-coded counter loop.
- Parser already accepted up to 8 chars (`is_valid_hash_id` check) — no parser change needed.
- **Tests:** 5 new unit tests — `generate_hash_n_width4_matches_generate_hash`, `generate_hash_n_widths_have_correct_length`, `generate_hash_n_wider_extends_shorter`, `assign_unique_hash_extends_on_collision`, `assign_unique_hash_widens_when_counter_exhausted_at_width4`, and a stress test `backfill_assigns_collision_free_ids_under_pressure` (50 items, all unique, all within 4..=8 chars).

**#bak5 — pending-done archive component**

- `pending::reap_with_items` — new function that returns `(new_body, Vec<PendingItem>)` so callers can access the full reaped item (including text), not just the id. `pending::reap` is now a thin wrapper that drops the text.
- `preflight::archive_pending_done(content, removed)` — new helper. Parses the content, looks for an `agent:pending-done` component, and if present, appends entries in format `- YYYY-MM-DD [#id] text` (ISO date, hash preserved for grep-compat, text verbatim). Silent no-op when the archive component is absent — per spec §3 step 3 we never auto-create it.
- `run_pending_maintenance` now calls `reap_with_items` and, if reap produced entries, invokes `archive_pending_done` before writing back. Re-parses the post-reap content because component offsets shift after the first `replace_content`.
- Date comes from the `date` command (not `chrono`) — matches `git.rs::chrono_timestamp`, keeps agent-doc's no-chrono dependency policy. Falls back to `"unknown-date"` on command failure.

**Verification**
- `cargo build --release` → clean, 0 warnings
- `cargo test --release --bin agent-doc -- pending::tests reposition git::tests` → **70/70 pass**
- Full test suite has pre-existing failures in `resync::tests` (missing tmux in test env, 45 failures) and one `preflight_sweep_commits_other_tracked_docs` failure — I confirmed both are reproducible on a `git stash` of my changes, so they're not regressions.
- `cargo install --path .` → binary updated at `~/.cargo/bin/`

**What's next**
- The two items are marked `--pending-done` on this write (`14z4`, `bak5`) — preflight will reap them next cycle.
- Spec §3 step 3 is no longer "deferred" — the archive component now works when present. An `agent:pending-done` component can be added to any doc that wants history; docs without it behave unchanged.
- Release candidates for v0.32.5: #eg0w (CommitLock), #14z4 (hash ceiling), #bak5 (archive), and the pre-commit reposition reorder from this session's earlier work.

Let me know if you want me to update `VERSIONS.md` + `specs/pending-system.md` (flip `(Optional, deferred)` to `(shipped)`) or queue a release.

❯ One of my last patchbacks to claudescore.md resulted wth `### Re: #nahq — checkpoint 1: news + spec + ontology landed` having an uncommitted removal of ` (HEAD)` even though the section was from a previous commit.
❯ I did an inline prompt in the section.

### Re: (HEAD) stripped from older section on inline prompt

Logged as pending. Quick read on the suspect path:

- `commit.reposition` runs on every preflight step 2, both in snapshot and working tree (`[commit] repositioned boundary in ...`).
- If reposition strips `(HEAD)` from *any* heading that is not the newly-computed boundary, then an inline prompt inside an older `### Re:` — which happens to still carry `(HEAD)` in the working tree because the prior cycle committed it that way — will show a phantom diff: the committed blob has ` (HEAD)`, the rewritten working tree does not.
- That matches your symptom exactly: section is from a previous commit, yet git gutter shows an uncommitted removal of ` (HEAD)`.

Fix direction (not implementing yet, want your call):
1. Reposition should only touch the *previous* boundary heading and the *new* boundary heading — never a third heading.
2. Or: reposition should be a no-op on the working tree when the only change is an inline user edit that does not move the boundary. Detect via "boundary heading unchanged across old/new snapshot".
3. Add a regression test: doc with two `### Re:` sections, the older one carries `(HEAD)`, user edits inside the older one, run preflight, assert working tree still has `(HEAD)` on the older heading.

Want me to dig into `reposition_boundary` and propose a patch, or just leave it as #pending for a dedicated pass?
❯ Do you recommend 1 or 2
❯ I a regression test in addition

### Re: 1 vs 2 + regression test

**Recommend option 2** as the primary fix, with option 1 as a follow-up if needed.

Why 2 over 1:
- **Narrower invariant**: "if the boundary heading is unchanged, don't touch the working tree" is a cheap, local check — compute the new boundary, compare against the old boundary's identity (heading text + component), early-return if equal. No risk of rewriting unrelated headings.
- **Option 1 requires knowing "previous boundary" reliably** across snapshot + working tree, which is exactly the state that drifts when the user is editing. Option 2 sidesteps that by making the no-op condition a *property of the new computation*, not a diff against remembered state.
- **Option 2 also fixes the phantom-diff class generally** — any inline edit that doesn't move the boundary becomes a true no-op in reposition, which is the correct semantics: reposition is a boundary-migration operation, not a working-tree normalizer.
- Option 1 is still worth doing eventually as a defense-in-depth: even when reposition *does* run, it should only rewrite the two headings it actually needs to touch. But it's a bigger change and the bug you hit is already closed by 2.

**Regression test** (yes, in addition):

```rust
// src/agent-doc/tests/reposition_boundary_preserves_older_head.rs
#[test]
fn inline_edit_in_older_section_preserves_head_marker() {
    let doc = indoc! {"
        ---
        agent_doc_mode: template
        ---
        &lt;!-- agent:exchange --&gt;
        ### Re: #older — already committed (HEAD)
        old content
        ### Re: #newer — current boundary
        new content
        &lt;!-- /agent:exchange --&gt;
    "};
    // commit, then inline-edit inside the #older section
    let edited = doc.replace("old content", "old content\nuser note");
    // run preflight (or reposition_boundary directly)
    let result = run_preflight(&edited);
    // assert: #older still carries " (HEAD)" in working tree
    assert!(result.working_tree.contains("### Re: #older — already committed (HEAD)"));
    // assert: #newer did NOT gain (HEAD)
    assert!(!result.working_tree.contains("### Re: #newer — current boundary (HEAD)"));
}
```

Minimal scaffolding — whatever `reposition_boundary` unit-test harness already exists is probably the right home. The key assertions are (a) `(HEAD)` preserved on the older heading, (b) no phantom migration to the newer heading, (c) working-tree byte-equal to input modulo the user's edit.

Want me to implement option 2 + this test now, or keep it as pending for a dedicated pass?

❯ The issue I ran into is that `### Re: #older — already committed (HEAD)` was committed when the boundary moved. The uncommitted `### Re: #older — already committed` is convenient for the user to see and navigate to the beginning of the patchback.

❯ If the boundary moves, presumably due to the user running the next agent-doc command...should the commit strip the ` (HEAD)`?
❯ I believe if the commit strips ` (HEAD)`, then the issue would also be fixed.

### Re: strip `(HEAD)` from staged blob — yes, this is the right fix (HEAD)

You're right, and this is cleaner than both options 1 and 2. Let me restate the invariant:

**`(HEAD)` is a working-tree-only marker. It must never appear in the committed blob.**

Under that invariant:
- HEAD blob of the doc: clean — no `(HEAD)` on any heading.
- Working tree: exactly one heading carries `(HEAD)` — the current boundary.
- Git gutter: the boundary heading is always blue (uncommitted modification of a committed line), which is exactly the navigation affordance you want.
- Moving the boundary: strip `(HEAD)` from the old boundary in the working tree, add it to the new one. The commit stages the clean content, so the old boundary's committed form already matches the new working-tree form (both clean) → no phantom diff.

Why this subsumes options 1 and 2:
- The bug you hit exists because the old boundary was committed *with* `(HEAD)` on it, so when reposition later stripped it, git saw a removal. If `(HEAD)` is never in the blob, there's nothing to remove against — the phantom diff can't exist.
- Reposition in the working tree is still fine (it's how you move the visual marker), but it no longer needs any "don't touch other headings" guard — the commit path has already normalized the blob.

Concrete change, roughly:
1. In `agent-doc commit` staging: before `git hash-object` / `update-index`, run the staged content through a `strip_head_markers` pass that rewrites any `### ... (HEAD)` heading to `### ...`. Stage the stripped bytes, not the working-tree bytes.
2. Leave the working tree alone (don't `git checkout` it). Reposition in the working tree continues to move `(HEAD)` to the new boundary heading.
3. Result: stripped content in blob, `(HEAD)` on current boundary in working tree, old boundary now clean in both (no diff).

This also means the "reposition in working tree" step can stop touching older headings entirely — since the blob is clean, the only heading that needs modification on a boundary move is the *new* boundary (add `(HEAD)`) and the *old* boundary (strip `(HEAD)`), which is the narrow scope option 1 wanted but gets it for free.

Regression test shifts to assert: after commit, `git show HEAD:<file>` contains zero occurrences of `(HEAD)`, and the working tree contains exactly one.

Want me to implement:
- `strip_head_markers` helper + call it in `agent-doc commit` staging,
- drop any reposition-rewrites-working-tree-for-old-headings logic that's now unnecessary,
- add the regression test above?

I'd replace the `#pending` item you added earlier with this approach — same bug, cleaner fix.
<!-- agent:boundary:0422facb -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release. Bundled SKILL.md rewrite ✅ landed — still BLOCKED on manual-testing gate.
- [ ] [#pf01] ✅ Spec landed on 20260414/pending-gated-state: src/agent-doc/specs/pending-system.md — PendingState::Gated, [/] syntax, reaper rules, lifecycle diagram, CLI surface, state transition matrix, release discipline
- [ ] [#64mb] Investigate double commit_staging race — root cause: concurrent preflights racing step 2 vs step 2d sweep; mid-flight (HEAD)-strip rewrites working tree between them
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [ ] [#xzz5] Preflight slash-command parser: absolute paths like /home/... get captured as commands. Require [a-z][a-z0-9:_-]* after the slash and reject tokens containing a second /.
- [ ] [#a011] End-of-cycle write invariant — warn if `preflight_diff_start` without `ipc_write_attempt`.
- [ ] [#a012] Expose `agent-doc session-check <FILE>` — nonzero if last ops.log entry is `preflight_diff_start`.
- [ ] [#a014] Session drift: `registered panes span 2 tmux sessions: 1, 4` — resync if recurring.
- [ ] [#a010] PostToolUse hook for agent-doc write (layer 3, optional).
- [/] [#mchh] Install rebuilt agent-doc-jetbrains plugin + verify Refresh Environment session picker end-to-end
- [ ] [#4sh0] Add planned-move logging to sync.rs reconciler so ClaimAction→SyncLayout pane swaps are debuggable
- [ ] [#tw4a] claim: cd into document's .agent-doc root (nearest git repo) when claiming a pane. Fixes relative-path ambiguity and makes git ops naturally target the correct repo.
- [ ] [#8jzg] claim --isolate: spawn a fresh Claude Code process in the submodule root so CLAUDE.md, memory, and skills load scoped to that repo. Only real fix for cross-project context leakage (mid-session cd does not reload context).
- [ ] [#cce5] response headers: include model attribution in '### Re:' sections (e.g. '### Re: topic — opus-4-6'). Source: agent-doc binary records spawn model; skill reads and appends to boundary-synthesized headers or patch:exchange headers
- [ ] [#a013] SPEC.md breakup — flat `specs/*.md` mechanical extraction.
- [ ] [#a004] Drop Option B (skill-side submodule workaround) — A handles it deterministically; B redundant.
- [ ] [#jg0d] supervisor/resize.rs: SIGWINCH unix path via signal-hook; Windows ConPTY stub for phase 1
- [ ] [#b486] supervisor/state.rs: crash classifier, ring buffer, Healthy->Transient->Flapping->Halted state machine
- [ ] [#40ct] supervisor/ipc.rs: Unix-domain socket accept loop, reuse length-prefixed JSON frame from ipc_socket.rs
- [ ] [#vnp0] supervisor: wire into start.rs (replace existing restart loop, remove #[allow(dead_code)])
- [ ] [#6ae3] supervisor/pty.rs: wire PtySession into start.rs (phase 1 wire-up stage) — replaces the current Command::status inherit-tty path
- [ ] [#zp02] supervisor/env.rs: wire into start.rs — replace the inline fm.env expansion loop in start.rs:189 with EnvSpec::from_frontmatter(&fm).resolve()?; feed the result into PtySpawnConfig.env once state.rs restart loop is in place
- [ ] [#f7d5] supervisor: smoke test — real claude under supervisor in a tmux pane, verify restart + inject + CWD invariant end-to-end
- [ ] [#eg0w] ✅ Shipped: per-file CommitLock in git::commit + <5s freshness gate in preflight sweep (awaiting release)
- [ ] [#kdy5] Implement instruction-files audit check: every library AGENTS.md has a ## Library Context Policy section with resolvable link to LIBRARY_CONTEXT_POLICY.md. Wire into make precommit.
- [ ] [#1376] After audit check lands: sweep all libraries under src/ to add the Library Context Policy stub section. Each submodule commit separately, then parent pointer bumps.
- [ ] [#breh] rust-package skill: write the Library Context Policy stub into new library AGENTS.md at scaffold time.
- [ ] [#3j0e] Install skills-janitor as Claude Code plugin (run /plugin install khendzel/skills-janitor at terminal). User plans to fork eventually; this is the interim path.
- [ ] [#vdyj] After fork: move forked skills-janitor under src/ as submodule, wire audit into make precommit, and optionally add nightly cron via schedule skill writing drift reports to tasks/agent-doc/skills-audit.md.
- [ ] [#9v7h] agent-doc: --pending-add should reject text starting with '[/]' / '[x]' / '[ ]' (ambiguous state marker). Add --pending-add-gated flag and emit assigned hash to stdout so skill can compose add+gate atomically without text-embedding.
- [ ] [#txex] notify: add --pending-add flag — auto-create agent:pending on target when absent, --no-create-pending to opt out. Pending-only (no exchange blockquote). Errors become no-ops under auto-create.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#hrmt] Rewrite src/agent-doc/SKILL.md (bundled skill) to invert Phase 2 pending guidance: remove 'every response MUST include a patch:pending block', replace with granular-ops contract from .claude/skills/agent-doc/SKILL.md §1b. RELEASE GATE for #gqep — v0.32.4 cannot ship with inverted bundled-skill guidance.
- [ ] [#e46w] Sweep-commit path: working-tree boundary lags until next write or editor re-open. Fix: have reposition_boundary_in_snapshot also rewrite the working-tree file when no active run is in progress (git.rs:593). Alternative: emit IPC reposition_boundary notification to open plugins.
- [ ] [#f8j3] Sweep-commit working-tree lag: reposition_boundary_in_snapshot (git.rs:593) only updates snapshot; working tree boundary only moves via IPC from agent-doc write. Fix: also rewrite working tree in snapshot reposition path, or emit IPC ping to open plugins.
- [ ] [#y4da] Deterministic boundary reposition in working tree (git.rs reposition_boundary_in_snapshot extends to disk write with active-run guard)
- [ ] [#zvmd] Submodule (HEAD) marker: fix show_head() to route via narrow_to_submodule — superproject rev-parse can't show submodule-tracked files, so add_head_marker falls through to the 'no HEAD → mark last heading only' fallback, which drops (HEAD) when the exchange component has no ### Re: heading yet or when the fallback runs during a boundary-only cycle
- [ ] [#np8p] Submodule pane cwd: agent-doc start/route spawns tmux panes with std::env::current_dir() instead of the nearest git toplevel for the target document — add a resolve_pane_cwd(file) helper that returns narrow_to_submodule(super_root, resolved).0 so submodule docs get a pwd inside their own submodule
- [ ] [#dsng] (HEAD) marker wrongly stripped from prior-cycle section on inline prompt: #nahq — checkpoint 1 section in claudescore.md showed uncommitted removal of ' (HEAD)' after inline-prompt edit, even though the section was committed in a previous cycle. Repro: edit inline inside an older ### Re: section that still carries (HEAD). Expected: (HEAD) preserved on sections not being responded to this cycle; only the current boundary section gains/loses it. Suspect: reposition_boundary pass rewrites the working tree and strips (HEAD) from any heading not matching current boundary, instead of only the one that previously held it.
<!-- /agent:pending -->
## Assistant
