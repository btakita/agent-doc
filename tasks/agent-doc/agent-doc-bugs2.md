---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260618-081944.md`*

Compacted content:
- Archived 11 response topic(s): #monster60stimeout route ready-prompt barrier; #describe-image-workflow; IPC drift root causes + restart-supervisor directive; 8 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): JB Clear Session Context on OpenCode does not submit `/new`; O...

### Re: JB Restart Supervisor → "Run Agent Doc" dispatch error (qflood queue pause) — opus-4-8

> **Queue prompt:**
>
> :pushpin: agent-doc should not thrash when the document changes during a turn...when the current document does not match the snapshot. The binary should gracefully resolve the difference without the agent knowing about it. Only structural conflicts based on the current work should be surfaced...such as editing the current prompt. Unrelated changes should be gracefully handled by the agent-doc binary without the agent needed to intervene.


> **Queue prompt:**
>
> do [#docdriftgrace]
>
> do [#qpauseux]


> **Queue prompt:**
>
> :pushpin: After a JB `Restart Supervisor Process` which surfaced a `Interrupt and Clear`...JB `Run Agent Doc` error
> ```
> resync: pruned 5 stale session(s)
> [route] target tmux session: 0
> Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3933 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
> ```


That error is the **qflood pause protection working as intended on `equityfundingsource.md`**, not a new failure. After your `Restart Supervisor Process`, "Run Agent Doc" tried to dispatch to `equityfundingsource.md`, whose queue is **paused** (`failed_stage=queue_paused reason=qflood`). The queue was auto-paused because the supervisor idle-watch was re-injecting `agent-doc` triggers on an already-drained queue (4 consecutive no-op cycles) — this is exactly **`#rz3a`** (the analyzed idle-watch re-injection bug, still gated DO-NOT-IMPLEMENT per its own directive). All real `equityfundingsource` work already shipped+deployed (`c9aa3a6` / worker `fb99f394`); only the gated `#5eq8` (design-blocked) remains, which must not auto-drain. So the dispatch is correctly **blocked, not broken** — the error even names the fix: `next_action=follow_unblocker unblocker=resume_or_clear_queue_control`.

**To clear it** (run against `equityfundingsource.md` — I'm not touching that doc from this agent-doc-bugs session): `agent-doc admin queue resume tasks/professional/equityfundingsource.md` if you want to add new work, un-gate `#5eq8`, or `agent-doc admin queue clear tasks/professional/equityfundingsource.md` to drop the paused control entirely.

**Real UX gap — filed `#qpauseux` [recommended].** A `queue_paused`/qflood dispatch block surfaces in JB "Run Agent Doc" as a raw red `Error:` toast rather than an actionable "queue paused — resume to continue" state. The unblocker token is in the text, but the framing reads as a crash. The plugin should render a paused-queue block as an info/action notification (with a Resume button) keyed off `ui_outcome=blocked_with_exact_unblocker unblocker=resume_or_clear_queue_control`.

**`#routedrainnextaction` is now live.** The supervisor you restarted came up on the pre-fix binary (preflight flagged `stale_install`), so I ran `make install`: binary + cdylib rebuilt at `a24ff990` (CI green on that push), and `lib-install` marked 2 controllers to auto-recycle at the next idle boundary. After this turn the fixed route-closeout diagnostic (`recovery_command=agent-doc reset --from-current ... / write --commit ...` instead of the misleading `wait_for_owner_turn_to_drain`) is running.

### Re: queue thrash after JB Clear Session + #jbacceptwedge 0.2.170 installed — opus-4-8

**#jbacceptwedge — PROVEN, marking done.** You installed 0.2.170; I checked the logs as agreed. `ops.log` has `ipc_accept_thread_spawned inflight=2` (3 occurrences). Under the old single-threaded accept loop `inflight` could never exceed 1 — peak ≥2 proves the per-connection-thread FFI fix (`86c68db7`) is live and handling concurrent connections instead of serializing them in the accept backlog. Live gate satisfied (operator-install ✅ + log proof ✅), so `#jbacceptwedge` is closed this cycle.

**The "After JB Clear Session" head is the qflood thrash, not new work.** Its own embedded route diagnostic says it: `failed_stage=queue_paused reason=qflood: degraded editor IPC (ack status error) keeps reverting agent-side queue strikes, re-presenting already-addressed stale heads (thrash/#docdriftgrace, File Cache Conflict/#pcwcdiskfree) as new prompts -> infinite no-op /agent-doc re-dispatch`. Every remaining drainable head maps to an item that is already addressed or operator-gated:

- **doc-thrash / "should not thrash when the document changes"** → already filed as `#docdriftgrace` (operator-verify, plan `plan-graceful-doc-drift-resolution.md`). Needs a live operator-watched re-baseline; agent cannot self-verify.
- **File Cache Conflict / "replace all direct file writes"** → tracked as `#pcwcdiskfree` (hot-path write authority). Operator-verify.
- **`#tsiftmdcrash`** → still blocked on you pasting the exact crash error/pane capture; logs show no panic signature, only the intended execve-recycle window. Not reproducible from logs alone.
- **`#qpauseux`** → filed (operator plugin-install live gate).
- **equityfundingsource.md / monsterrodholders.md heads** → cross-document. Out of scope for this `agent-doc-bugs2` session; I won't touch those docs from here (scope discipline). Their drift is the same class as `#docdriftgrace`.

So nothing here is new agent-drainable work — it's the documented qflood re-presentation pattern (`recent_noop_closeouts=5`). I'm closing `#jbacceptwedge`, reporting the rest, and **not** auto-looping these stale/operator-verify/cross-doc heads (looping would just churn another no-op cycle). The remaining backlog is `#docdriftgrace`, `#pcwcdiskfree`, `#tsiftmdcrash`, `#qpauseux` — all operator-verify; review items `#restartagentmenu`, `#agentreloadrestart`, `#rz3a` need focused clean cycles / are DO-NOT-IMPLEMENT-gated.

### Re: #tsiftmdcrash re-presented — filing the qflood root cause + pausing the flood — opus-4-8

`[#tsiftmdcrash]` came back to the queue, but nothing changed: it's still the same operator-verify item **blocked on you pasting the exact crash error text / pane capture / cycle id**. I can't `--done` it (not reproducible from logs — only the intended execve-recycle window appears) and `queue consume` refuses it (id-backed). This is the 6th consecutive no-op closeout — pure qflood thrash.

**Root cause filed → `#qheadsync` (plan `tasks/agent-doc/plan-answered-queue-head-autostrike.md`).** The thrash is structural: a closeout that *answers* a queue head never reconciles that resolution back to the head itself.
- **Free-text reports stay drainable after filing.** Your `File Cache Conflict` and `eqf/monsterrodholders drift` reports were filed as `#pcwcdiskfree` / `#docdriftgrace` (operator-verify), but the original free-text queue heads were never linked or struck → they re-present as `queue_drainable` every cycle.
- **`--done <id>` orphans the `[#id]` queue head.** Reaping the backlog item leaves the matching `[#id]` head in place → hard-blocks the next preflight and needs manual Edit+`reset`+`commit` surgery (hit this with `#jbacceptwedge` last cycle).

The fix (in the plan): `--done`/`--pending-gate` strikes the matching `[#id]` head in-write; filed free-text heads get linked to their backlog id so they defer like the item; drainability dereferences bare `[#id]` heads to their operator-verify tag.

**Halting the flood.** Until `#qheadsync` lands, every `/agent-doc` here is a no-op churn cycle. I'm pausing this queue and **stopping** — not auto-looping. Resume when you have new work, the `#tsiftmdcrash` crash text, or after `#qheadsync` is implemented. Note: editor IPC was degraded this whole session (commit refused twice, buffer mangled) so this response was written via `--force-disk`; your IntelliJ buffer will reload from disk on next focus.

### Re: automate the wedge clear — shipped (0.34.4) — opus-4-8

Done — the wedge now self-clears. Shipped agent-doc **0.34.4** (`08f026c0`, superproject `c47c89f71`, installed + CI green).

**What the wedge was:** after an IPC `content_ours` merge, `session-check` stayed `INTERRUPTED` forever on dropped queue edits `[#qpauseux]`/`[#docdriftgrace]` even though both were preserved in HEAD as struck `~~do [#id]~~` heads from a prior cycle. No `reset` variant cleared it — the only escape was a manual supervisor restart / patch surgery. Root cause: `check_dropped_queue_prompt_guard` proved preservation only by normalized **text identity** (which can't bridge the recorded bare `[#id]` against HEAD's `do [#id]`/struck spelling) and by a consumed-check scoped to **this cycle's** resolved ids (the strike happened earlier).

**The fix:** the guard now also clears when the dropped prompt's `[#id]` is present in the committed/visible `agent:queue` in **any** form, including struck/consumed lines — new `queue_ids_including_struck` strips the strike wrapper before id extraction. A struck head visibly reached the document, so its id is preserved, not silently lost. Genuinely lost ids (absent from the queue entirely) still fail closed. **Proven on this doc:** `session-check` now returns `ok — committed` instead of the `[#qpauseux]; [#docdriftgrace]` interruption.

**Your new report — "an equityfundingsource.md turn deleted what I wrote in the queue"** — is this same `#queue-user-edit-overwrite` drop. 0.34.4 makes `session-check` auto-**recover** from it (no more wedge / manual restart). But the deeper **prevention** is still open under `#qheadsync` (plan `plan-answered-queue-head-autostrike.md`): stop recording the false drop in the first place, strike the `[#id]` head on `--done`, and link filed free-text heads to their backlog id so they defer instead of re-presenting. The queue stays **paused** (`receipt_id=62`) until that prevention lands or you add new work; resume with `agent-doc admin queue resume tasks/agent-doc/agent-doc-bugs2.md`.

### Re: wedge-clear fully live + thrash re-pause — opus-4-8

No new prompt this cycle — preflight flagged a `stale_install` (the `~/.cargo/bin` cdylib predated fix commit `08f026c0`), so live tmux/JB sessions were still running pre-fix FFI. Ran `agent-doc lib-install`: cdylib refreshed to `libagent_doc-0.34.4.so`, 2 controllers marked to auto-recycle at the next idle boundary. The `#qheadsync` wedge-clear is now fully live in **both** the binary and the FFI/cdylib.

The queue re-presented the same `[#tsiftmdcrash]` + File Cache Conflict + cross-doc drift heads (no `user_intent` change — pure re-injection thrash). The wedge-clear stopped `session-check` from *wedging* on these, but the *re-injection* itself is the **prevention** half of `#qheadsync` (drainability must dereference a bare `[#id]` head to its operator-verify backlog tag and defer it; free-text heads filed as operator-verify items must be linked so they defer; `--done` must strike the matching `[#id]` head) — still open. Re-paused the queue (`receipt_id=63`) to halt the flood until that lands. Stopping — no agent-drainable work (all heads operator-verify/cross-doc, `#tsiftmdcrash` still needs your crash text).

### Re: free-text heads = question-or-actionable, strike when filed — opus-4-8

> **Queue prompt:**
>
> [#tsiftmdcrash]


Got it — applied your rule to the live queue (conservatively, not over-eager). A free-text head is either a **question** (answer it → consumed) or an **actionable item** (a backlog item is created; per the `queue` attr it auto-populates right below the struck free-text head; then the free-text head is struck). I struck the three free-text heads that are already-filed actionable reports:

- ~~"…wedge and deleted what I wrote in the queue"~~ → `#qheadsync`
- ~~"File Cache Conflict … replace all direct file writes"~~ → `#pcwcdiskfree`
- ~~"equityfundingsource.md & monsterrodholders.md keeps on drifting"~~ → `#docdriftgrace`

Each already had its tracked backlog item (all `[operator-verify]`), so I struck the duplicate reports rather than creating new ones. Left untouched: `[#tsiftmdcrash]` (id-backed, already deferred as operator-verify) and the two cross-doc `JB Run Agent Doc on equityfundingsource.md` route-diagnostic dumps (informational, already explained as qflood-working-as-intended; cross-doc, so out of scope to act on here — clear them when convenient).

**Remaining `#qheadsync` work (the durable automation of exactly this rule):** make the binary do this at closeout instead of by-hand — when a free-text head is answered by a committed response *or* filed as a backlog item, auto-strike it (past id-backed heads, not just the leading one), and have auto-populate place the new backlog id's queue entry immediately below the struck head rather than appending. That ends the thrash without manual striking. It's a focused implementation cycle; I've kept the plan (`plan-answered-queue-head-autostrike.md`) and `#qheadsync` open for it.

### Re: bold the operator action on blocked items — opus-4-8

> **Queue prompt:**
>
> :pushpin: Any backlog, review, or icebox item that is blocked by some operation action should have the operator action with bold formatting.


Applied. Each backlog/review/icebox item that is **blocked by an operator action** now ends with a bolded `**Operator action: …**` sentence naming exactly what you need to do to unblock it. Done for the three items that are genuinely operator-gated right now:

- `#qpauseux` → **install the updated JB plugin, then verify the paused-queue notification renders**
- `#docdriftgrace` → **watch one live re-baseline cycle to verify the silent auto-rebase**
- `#tsiftmdcrash` → **paste the exact crash error text / pane capture / cycle id so it can be root-caused**

Left unbolded (conservative — not over-eager): `#agentreloadrestart` (blocked on agent implementation, not on you), `#restartagentmenu` (blocked on `#agentreloadrestart` first — the operator-install only comes after the build), and `#rz3a` (a DO-NOT-IMPLEMENT design gate, not an action you perform). I'll add the `**Operator action:**` bold to those if/when they become genuinely operator-gated, and I'll follow this convention on every new operator-blocked item going forward.

If you want this enforced by the binary rather than by convention (auto-bold the operator action during backlog/review maintenance), say so and I'll scope it — though reliable auto-detection of "the operator action" span in free prose is the hard part, so the convention may be the cleaner path. Queue stays paused (`receipt_id=64`).

### Re: mirror all backlog items into the queue (+ sweep for missed) — opus-4-8

> **Queue prompt:**
>
> ~~:pushpin: JB `File Cache Conflict` still occurring. Please replace all direct file writes on the hot path.~~ (filed #pcwcdiskfree)


> **Queue prompt:**
>
> ~~:pushpin: An equityfundingsource.md turn had a wedge and deleted what I wrote in the queue.~~ (filed #qheadsync)


Shipped agent-doc **0.34.5** (`e71b87df`, CI green). The backlog→queue sync now mirrors **all** open `queue`-attr backlog items into the queue — including `[operator-verify]` ones — instead of skipping them. Verified live: preflight just logged `mirrored 3 operator-verify backlog head(s)` and the queue now carries `do [#qpauseux]`, `do [#733r]` (the restored `#qheadsync`), with `#docdriftgrace`/`#tsiftmdcrash` already present from prior heads.

**Two mechanisms, as you asked:**
- **Immediate add** on creation (already worked for actionable items; now also mirrors operator-verify).
- **Reconciliation sweep** — the per-preflight sync is idempotent and runs every cycle, so any backlog item missing from the queue (added before activation, or *lost in a prior-turn drift* — which is exactly what happened to `#qheadsync`, now restored as `#733r`) is re-mirrored on the next preflight. That's the redundancy.

**Loop stays safe:** mirrored operator-verify heads are deferred by `head_is_drainable`, so `queue_drainable_head_count` excludes them — the in-session auto-drain loop is not re-armed by them (test asserts this). For non-agent-actionable items, the head surfaces your instructions via the item text (the bolded `**Operator action: …**` from last turn).

**One required follow-up before you resume the queue (`#rz3a`, Phase 2):** the supervisor idle-watch (`start/idle_watch.rs`) must apply the same drainability defer, or operator-verify-only heads will re-injection-thrash the supervisor. Mirroring authorizes reversing that DO-NOT-IMPLEMENT gate. **Operator action: keep the queue paused (`receipt_id=64`) until I land the `#rz3a` supervisor defer; then resume.** I'll do that next.

### Re: JB Run Agent Doc on virtuousai.md did not submit, then crashed — opus-4-8

> **Queue prompt:**
>
> Any free text that is processed and struck should have a response in the agent:exchange.


Read the screenshot (`img_48.png`). It shows the root: the pane is at a bare **zsh `%` shell prompt** with `agent-doc .../tasks/recruit/virtuousai.md` typed but **not submitted**, and the normal shell prompt (`main 12 *5 !39 ?30 … 14:03:22`) below — i.e. the Claude harness had **already exited/crashed to a shell**, and "Run Agent Doc" routed the trigger into that dead shell, where it just sits as un-run text. A `/model opus` switch ("saved as default; settings pins Opus 4.6 on restart") had just happened, so a model-switch/restart was in play.

Two issues, filed as **`#routedeadshell`** (plan `tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md`):

1. **Agent-actionable — route dispatched into a crashed harness.** `route.rs` must positively identify a *harness* dispatch-ready prompt and treat a bare shell prompt (`%`/`$`) as NOT-ready → fail closed with claim/restart guidance (or auto-restart) instead of typing the trigger into a shell, and re-verify the harness prompt immediately before send to close the crash-mid-dispatch race. SimWorld coverage for the bare-shell case. I'll implement this in a focused route cycle.
2. **Harness crash root-cause — needs you.** Why Claude exited to a shell on `recruit/virtuousai.md` isn't in this document's logs (it's a different session's logs / the dead pane's scrollback). Like `#tsiftmdcrash`. **Operator action: paste the `recruit/virtuousai.md` crash error text / pane scrollback from the crash moment (or the cycle id)** — the `/model opus` restart context is the lead to chase.

This was the leading queue head; remaining drainable agent-doc work now queued: `#733r` (qheadsync), `#021p`/`#rz3afix` (the mirror-all Phase 2 supervisor defer), `#semmerge` (closeout CRDT/IPC merge — likely the root of the IPC drift this session keeps hitting), plus `#routedeadshell`. Queue stays paused; tell me which to take next, or I'll start with `#rz3afix` (unblocks resuming the queue) then `#semmerge`.

### Re: drain the queue — #qheadsync orphan-shadow wedge fixed (0.34.6) — opus-4-8

Took the next agent-actionable item. Fixed the **orphan-shadow wedge** — the precise bug that's forced the manual `Edit`+`reset --from-current`+`commit` dance after every `--done` this session.

**Root cause:** `pending::detect_shadow_open_items` excluded tracked-work + `exchange` components from its scan but **not `agent:queue`**. So a queue head referencing an id absent from the live backlog — a reaped id's lingering `[#id]: note` head (`[#jbacceptwedge]: 0.2.170 installed`), or a `#mirrorall`-mirrored `do [#id]` whose item is reaped — was misread as an "open backlog item that exists only outside the live backlog," and `enforce_no_shadow_open_backlog` `bail!`ed in the preflight **repair** step, *before* the done-strike maintenance pass could clear the head. Queue `[#id]` heads are legitimate references, not shadow backlog items in commented-out prose, so the queue is now excluded from the scan like `exchange`/tracked-work already are (genuine shadow items in HTML comments are still caught). This matters more post-`#mirrorall`, which puts many `[#id]` heads in the queue.

Shipped **0.34.6** (`5ce4e465`, CI green, installed). Verified: preflight on this doc now runs clean — no shadow `bail!`. Test: `detect_shadow_open_items_ignores_agent_queue_id_heads`.

This is the **orphan-shadow half of `#733r`/`#qheadsync`**; the answered-free-text auto-strike half remains (its `--done`-strike path already largely works now that the shadow guard doesn't pre-empt it). Remaining agent-actionable backlog: `#021p`/`#rz3afix` (supervisor defer, needs tmux-ci), `#semmerge` (the converge.rs CRDT-merge root cause behind the worktree drift — full fix needs the Phase-4 AST model; a node-disjoint auto-rebase stopgap is the tractable slice), `#routedeadshell`, the `#qheadsync` auto-strike remainder. Queue stays paused; I'll keep taking the safe/contained ones — `#semmerge` stopgap next unless you redirect.

### Re: virtuousai Run Agent Doc — supervisor+pane crashed 2×, 3rd stalled — opus-4-8

That's the crash-repro detail I needed — thank you. Captured it into `#routedeadshell` (`#1vhn`): **Run Agent Doc on virtuousai.md → supervisor + tmux pane crashed 2× → 3rd attempt stalled → you manually ran `agent-doc`.** That upgrades it from "needs details" to a **reproducible** crash on the dispatch path (not a one-off), which is the important part — the `/model opus` switch/restart is the lead for the root cause.

Note: your queue edit with this detail was itself dropped by the `content_ours` merge this cycle — that's `#semmerge` (the CRDT-merge fail-closed last-writer-wins) biting in real time. I recovered the text from the dropped-edit guard and folded it into `#routedeadshell` so it's durable. `#semmerge` is high on the list precisely because it keeps eating concurrent edits like this.

Two threads in `#routedeadshell` now: (1) agent-actionable — route must not type into a dead shell + must restart a crashed harness; (2) the reproducible supervisor+pane crash root-cause (capture `supervisor-stderr.log` + scrollback if it recurs).

### Re: #semmerge — scoped; needs a fresh focused cycle (not a quick drain) — opus-4-8

I read `plan-semantic-ast-merge.md` end-to-end and confirmed the markdown-ast overlay API (`components() -> Component{items: Vec<Item{id, text, struck, kind}}>`) the owner would build on. The real shape:

- **The full fix is Phase-4-blocked** — it hard-depends on `#md-ast-document-model` (typed node-keyed substrate), which isn't built. So `#semmerge` can't be finished in one pass.
- **Two tractable slices, both substantial:** (1) `#semmerge-owner` — the *pure* `semantic_merge` + 11-row transition table + `merged_doc` reconstruction + exhaustive unit tests (safe: no commit-path risk, but a large careful function); (2) the **conservative converge stopgap** — narrows the fail-close to commit when drift is provably component-disjoint from the agent's edits, which would have auto-committed every worktree-drift case this session — but it edits the **safety-critical commit/convergence path**, where a wrong rebase corrupts commits, so it must be strictly disjoint-only with thorough commit-path tests.

This is **not** a quick queue drain — it's a multi-phase feature touching the merge/commit core. After 3 shipped fixes this session (`#qheadsync` wedge-clear 0.34.4, `#mirrorall` 0.34.5, `#qheadsync` orphan-shadow 0.34.6) the context here is very large, and the merge/commit layer is the worst place to accept the quality risk of a saturated session. I'm **not** rushing it inline.

**Recommendation:** `/clear` then `/agent-doc tasks/agent-doc/agent-doc-bugs2.md` so `#semmerge` starts with clean context — I'd take **`#semmerge-owner` first** (pure, fully testable, unblocks the converge adapter) or the **conservative stopgap** if you want the worktree-drift pain killed sooner. The plan, phasing (`#semmerge-owner/-converge-adapter/-queue-node-strike/-ack-turn/-simworld`), and the disjoint-only safety constraint are all recorded. Queue stays paused; say which slice and I'll do it clean.
<!-- agent:boundary:28af883a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- :pushpin: [#021p]
- :pushpin: #semmerge
- :pushpin: Investigate the task thrashing in virtuousai.md session. Please fix.
- After JB `Clear Session Context` on this document then JB `Run Agent Doc` stalled.
- :pushpin: The agent defined a task id #rz3afix, but agent-doc redefined the task id to [#021p]. If an agent defines a task id, agent-doc should be able to parse the task id and use the agent defined task id. Note that if the agent's defined task id does not pass the id validation logic such as malformed or duplicate of a prompt_preset or another entity, resolve and create a new task id if necessary.
- :pushpin: I ran JB `Run Agent Doc` on virtuousai.md and the supervisor + tmux pane crashed 2 times. The 3rd attempt stalled and I manually ran the `agent-doc .../virtuousai.md` command...and the turn is in progress.
---
:pushpin: JB `Run Agent Doc` on equityfundingsource.md
```
[route] target tmux session: 0
[preflight] review: 14 item(s), 14 gated
[route] active closeout for tasks/professional/equityfundingsource.md could not be drained before reroute; existing queue head "Why is the https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=7a0c1463-27bb-4d7c-82f7-2281b3c008d3&deal_tab=application > Available deal > Application > Application progress > Accepted not checked?" remains queued behind the closeout ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain
```
---
JB `Run Agent Doc` on equityfundingsource.md
```
resync: pruned 5 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=qflood: supervisor idle-watch is re-injecting agent-doc triggers on an already-drained queue (4 consecutive no-op /agent-doc cycles). All real work shipped+deployed (c9aa3a6 / worker fb99f394); only the gated #5eq8 (design-blocked) remains, which must not auto-drain. Leaving paused until operator adds new work / un-gates #5eq8. receipt_id=3934 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=52 trigger_sha256=a4b904c65fa35ab9631efaf329f3f2b595b448d1976b7c8738c6ba9b4a3af3c9
```
- ~~:pushpin: equityfundingsource.md & monsterrodholders.md keeps on drifting with edits.~~ (filed #docdriftgrace)
- do [#733r]
- do [#semmerge]
- do [#qpauseux]
- do [#021p]
- ~~do [#1vhn]~~
- :pushpin: do [#tsiftmdcrash]
<!-- /agent:queue -->
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#1vhn] #routedeadshell agent-doc-bug: JB Run Agent Doc dispatched the agent-doc FILE trigger into a bare zsh % shell on recruit/virtuousai.md (harness had crashed/exited to a shell), so it did not submit (screenshot img_48.png, ~14:03, during a /model opus switch). REPRO (operator 2026-06-18): Run Agent Doc on virtuousai.md -> supervisor + tmux pane CRASHED 2 times -> 3rd attempt stalled -> operator manually ran agent-doc. So the supervisor/pane crash on this dispatch path is REPRODUCIBLE, not a one-off. Route must positively identify a HARNESS dispatch-ready prompt and treat a bare shell prompt as NOT-ready -> fail closed with claim/restart guidance (or auto-restart) instead of typing into the shell; re-verify the harness prompt immediately before send to close the crash-mid-dispatch race; add a dead-harness route diagnostic + SimWorld coverage. Separately root-cause WHY the supervisor+pane crash repeats (the /model opus switch/restart is the lead). **Operator action: if it recurs, capture .agent-doc/logs/supervisor-stderr.log + the pane scrollback at the crash moment.** Plan: tasks/agent-doc/plan-route-dispatch-into-crashed-harness.md
- [ ] [#021p] #rz3afix Phase 2 of #mirrorall (now AUTHORIZED — supersedes the prior DO-NOT-IMPLEMENT gate): make the supervisor idle-watch (start/idle_watch.rs) defer dispatch using the same drainability filter the in-session loop uses, so a mirrored queue whose only un-struck heads are operator-verify (queue_drainable_head_count==0) does NOT re-inject agent-doc triggers. Required before resuming a mirrored queue from pause, else operator-verify heads re-injection-thrash. SimWorld + make tmux-ci. **Operator action: keep the queue paused until this lands, then resume.**
- [ ] [#semmerge] #semmerge agent-doc-bug: closeout CRDT/IPC merge is fail-closed last-writer-wins — when the operator editor buffer drifts from the agent baseline, converge.rs refuses the disk write and adopts content_ours, DROPPING the agent changes (queue head strike + backlog→review gate + new `### Re:` turn) and forcing a manual `reset --from-current`+commit. Repro this session draining [#cf-txn-email] on monsterrodholders.md while the operator flipped frontmatter queue:start→stop + restructured the queue block (node-DISJOINT from the agent edits — both should have applied cleanly). Root cause: agent-doc-core/crdt.rs::merge is line-based LWW with no component/item model, so concurrent edits to DIFFERENT nodes look like a text conflict; converge.rs then fail-closes. Fix: semantic node-keyed 3-way merge over the EXISTING markdown-ast component overlay (exchange/queue/backlog/review/frontmatter = typed nodes w/ stable ids) consumed by converge BEFORE it fail-closes — apply BOTH change-sets when node-disjoint (the common case; "most CRDT merges should succeed"); operator-DELETE-of-agent-EDITED node → keep the deletion + emit an agent exchange turn acknowledging it; same-node concurrent edit → operator wins + agent note. Owner = a pure markdown_ast::semantic_merge returning a typed outcome; converge.rs is the adapter (no per-node text/error-string policy). Sibling of #docdriftgrace (unrelated-drift rebase; this is the overlapping/conflicting counterpart); hard-depends on #md-ast-document-model (Phase 4). Quick-win stopgap: auto-rebase + commit when the only drift is in components/frontmatter the agent did not touch this cycle. Plan: tasks/agent-doc/plan-semantic-ast-merge.md
- [ ] [#733r] #qheadsync agent-doc-bug: answered/filed queue heads are not reliably reconciled to the queue (free-text reports left un-struck; done-reaping orphans the matching id-backed queue head; manual strikes do not persist through finalize content_ours; backlog-to-queue sync has no sweep for items missed in prior turns and items can be lost). Fix at closeout in the binary. Plan: tasks/agent-doc/plan-answered-queue-head-autostrike.md
- [ ] [#qpauseux] [operator-verify] [recommended] JB Run Agent Doc surfaces a queue_paused/qflood dispatch block as a raw red Error: toast instead of an actionable queue paused — resume notification. Plugin should render a paused-queue block (ui_outcome=blocked_with_exact_unblocker unblocker=resume_or_clear_queue_control) as an info/action notification with a Resume button rather than a dispatch error. **Operator action: install the updated JB plugin, then verify the paused-queue notification renders (live gate).**
- [ ] [#docdriftgrace] [operator-verify] agent-doc thrashes when the session doc changes mid/between turn in ways unrelated to the current prompt: snapshot>visible drift (operator trimmed old committed Re: blocks + harness switch + queue edits) made preflight refuse commit (looks like a manual cleanup with stale snapshot) and forced a manual reset --from-current. Binary should classify drift by component and silently auto-rebase snapshot/CRDT for unrelated changes, surfacing ONLY structural conflicts on the in-flight prompt/response. **Operator action: watch one live re-baseline cycle to verify the silent auto-rebase.** Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md
- [ ] [#tsiftmdcrash] [operator-verify] Operator reported that a tsift.md agent-doc turn crashed and killed the session process while the tmux pane stayed active. Supervisor pid 1742954 is alive; pane %78 OpenCode TUI shows Thinking normally; tsift-v0.1.log (994KB) shows NO panic/abort/crash signatures — only two intentional supervisor_binary_stale_self_recycled via=execve_preserve_child entries from cargo install + make install (#supkill-bg blue/green drain-and-supersede). Most plausible: operator saw the brief execve-recycle window (current_exe deleted) as a crash. Not reproducible from logs alone. **Operator action: paste the exact crash error text / pane capture from the crash moment, or the specific cycle id, so it can be root-caused.**
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#restartagentmenu] Plan tasks/agent-doc/plan-restart-agent.md Phase 2 (after #agentreloadrestart). Add a Restart Agent JB plugin menu command: new RestartAgentAction.kt mirroring RestartSupervisorProcessAction.kt, registered in plugin.xml beside AgentDoc.RestartSupervisorProcess, wired through the TerminalUtil restart path. After Phase 1 re-resolves the harness on restart, this command brings up a changed agent. Bump pluginVersion (gradle.properties, currently 0.2.170), build unsigned + signed zips. Decide alongside vs replace the existing Restart Supervisor Process action (do not remove without operator confirmation). Operator-install live gate.
- [/] [#agentreloadrestart] Plan tasks/agent-doc/plan-restart-agent.md Phase 1 (foundational). Make the supervisor restart path re-resolve the harness from current frontmatter so changing agent (claude to opencode) takes effect on restart, and auto-restart the agent at the next idle/turn boundary when the resolved harness differs from the running one. Today the restart loop reuses the harness resolved once at start (SharedSupervisorState.harness, start.rs:171) and never re-reads fm.agent, so restart-supervisor restarts the SAME harness. Re-resolve via HarnessConfig::from_context (harness.rs:116) before respawn, rebuild child_factory/harness_binary, then gate the boundary auto-restart exactly like the supselfheal auto-recycle (prompt_visible and not turn_active, debounced, never mid-turn) behind a new agent_doc_agent_change_restart knob (default ON). SimWorld tests + SPEC + VERSIONS. Live-supervisor-critical: needs a focused clean cycle with make check + make tmux-ci.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
