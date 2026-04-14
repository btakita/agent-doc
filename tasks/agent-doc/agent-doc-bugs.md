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
### Re: reword confirmed + tracking the multi-user follow-up (HEAD)

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
<!-- agent:boundary:d9057f66 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#25ag] Rename patch:pending → replace:pending. Parser accepts both for one release with deprecation warning; --allow-patch-pending gains --allow-replace-pending alias; AGENT_DOC_ALLOW_REPLACE_PENDING=1 becomes canonical env var; specs/pending-system.md + SKILL.md updated to new name; drop old name after one release. Queue behind current phase-3 work.
- [/] [#gqep] Release v0.32.4: bump Cargo.toml + pyproject.toml, VERSIONS.md entry for pending system, publish crates.io + GitHub Release after manual-testing gate
- [ ] [#e6z1] agent-doc: version bump + VERSIONS.md entry for gated-state feature after implementation lands (release gate — do not ship with the #eg0w bundle)
- [ ] [#14z4] Hash collision ceiling: implement 8-char fallback in pending::generate_hash if 4-char collides (spec §1)
- [ ] [#bak5] Optional: pending-done archive component for history trail (spec §3 step 3, deferred)
- [ ] [#pf01] ✅ Spec landed on 20260414/pending-gated-state: src/agent-doc/specs/pending-system.md — PendingState::Gated, [/] syntax, reaper rules, lifecycle diagram, CLI surface, state transition matrix, release discipline
- [ ] [#mgdw] agent-doc: parser — accept `[/]` checkbox char, extend PendingState enum with Gated variant in src/agent-doc/src/pending.rs; renderer emits `[/]` round-trip
- [ ] [#h1j2] agent-doc: reaper — skip Gated items in preflight reap pass; emit pending_gated_count alongside existing counts in preflight JSON output
- [ ] [#q90h] agent-doc: CLI — add `--pending-gate <id>` flag on `agent-doc write` (mirror of --pending-done); `--pending-ungate <id>` to return to `[ ]`
- [ ] [#sx35] agent-doc: tests — parser round-trip for `[/]`, state transitions ( →/→x,  →x, /→x, /→ ), reaper respects Gated, CLI flag integration
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
- [ ] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set the list once; multi-user recipients gate what senders can materialize. Ship with multi-user workspace support, not before. Gate with [/] until multi-user work begins.
- [ ] [#e4pc] [MULTI-USER FOLLOW-UP, gate next cycle] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set the list once; multi-user recipients gate what senders can materialize. Ship with multi-user workspace support, not before.
<!-- /agent:pending -->
## Assistant
