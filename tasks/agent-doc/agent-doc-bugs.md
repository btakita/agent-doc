---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a0db52-4c0d-72d3-b2c9-0ac89aff42a0
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260927-023305.md`*

Compacted content:
- Archived 8 response topic(s): #advance-review, tmux pane auto-focus, and queue edit convergence; recovered response; #advance-review; 5 more
- Prior summary/context: prior compacted content: Archived 28 response topic(s): actionable operator verification belongs in Backlog; cross-do...

### Re: queue prompt on cargovodtrim — opus-5 · 2026-09-26T22:43-04:00

> **Queue prompt:** re [#cargovodtrim]: no live stream. Just delete from history.

Done, and the premise was even weaker than "no live stream" — the exposed value was already dead.

**No livestream artifact exists.** OBS is configured to record to `~/Videos` with `FilenameFormatting=%CCYY-%MM-%DD %hh-%mm-%ss` and has **no `service.json` in any profile**, so it was never wired to a streaming platform. The newest recording in `~/Videos` is `2026-09-01 10-15-51.mp4`; nothing exists for 2026-09-11 or 2026-09-12. Your "no live stream" is confirmed from disk.

**The exposed credential was already superseded.** I located the exposure precisely: `~/.claude/projects/-home-brian-work-btakita-agent-loop/3dd1510c-8303-48c2-9f70-5c7e55a18863.jsonl` line 494, timestamp `2026-09-12T02:57:27.170Z` — exactly the interval start this item recorded. It is the `chromium-bridge` markdown capture of the crates.io tokens page, including the one-time reveal. Comparing by SHA-256 only, never printing cleartext: the exposed token hashes to `1d9e073eda38…` while `passage btak/CARGO_REGISTRY_TOKEN` currently holds `50f79bb47a83…`. Different values, same 35-byte shape — so decision (a) from `#cargotokenwire` was taken and the exposed token was regenerated away. It could not authenticate even before this scrub.

**Scrubbed from history.** Exact-byte search over 224,762 files under `~/.claude`, `~/.codex`, `~/.config/passage`, `~/.cargo/credentials.toml` and the project `.agent-doc/` found the value in exactly three files, 8 occurrences:

| occurrences | file |
|---|---|
| 4 | `~/.claude/projects/-home-brian--claude-mem-observer-sessions/10d5cdc0-…ac.jsonl` |
| 2 | `~/.claude/projects/-home-brian-work-btakita-agent-loop-src-haiven-dev/4748560d-…39.jsonl` |
| 2 | `~/.claude/projects/-home-brian-work-btakita-agent-loop/3dd1510c-…63.jsonl` |

All three were 14–15 days stale with no live appender, so I rewrote each atomically, replacing the token with `[REDACTED-crates-io-token]` and validating every modified line still parses as JSON before the swap; original mode and mtime preserved. Post-scrub count is **0 in all three**. A second sweep for the reveal marker `You won't be able to see it again` across `~/.claude-mem` (including `claude-mem.db-wal`), `.tsift/memory.db`, `~/.codex` and the remaining transcripts found 10 files carrying the phrase and **zero** with a token-shaped run following it, so no other store kept a copy. `git log --all -S<token>` and `git grep` over the superproject found nothing, so no history rewrite was needed — nothing tracked ever held it.

`#cargovodtrim` is resolved rather than gated: there is no VOD, the credential is dead, and the cleartext is gone from every local artifact.

### Re: actionable review items into backlog — opus-5 · 2026-09-26T22:43-04:00

> **Queue prompt:** #actionable-review

Triaged all seven gated review items for an agent-executable remainder. Five filed, three left gated with the reason, and one blocker stops the **queue** half of your instruction.

**The queue half is not reachable from the CLI on this document.** `&lt;!-- agent:backlog --&gt;` here carries no `queue` attribute, and every mirror path is gated on it: `--backlog-queue-placement` is documented as acting only "when the backlog carries a `queue` attribute", and `agent-doc queue sync` is "sync from backlog items with `queue` attribute". I verified rather than assumed — on a scratch copy of this document, two `--backlog-add` items landed in `agent:backlog` and mirrored **nothing** into `agent:queue`. No command can add the attribute either: `backlog` has no `set-attr`, `write` exposes no marker flag, `patch` replaces component *content*, and `migrate` only *strips* deprecated attributes. The only route today is hand-editing the marker, which is exactly what I must not do here — `agent-doc reliable-sync-status` shows this document **live under IntelliJ pid 3129637 / plugin 0.2.428**, so a raw disk edit becomes a disk candidate the agent is forbidden to resolve.

So: the five items are in `agent:backlog`. To mirror them into the queue, add ` priority queue` to the backlog marker in the editor you already have open — matching `tasks/software/lazily.md`, `tasks/software/corky.md` and `tasks/brookebrodack-dev.md` — then `agent-doc queue sync`. The missing command is filed as `#bkqattrcli`, top of backlog, since it blocks this exact instruction every time it runs.

**Filed, top-down:**

- `#bkqattrcli` — the gap above: `agent-doc backlog set-attr` / `unset-attr` through the editor-converging write path, plus a preflight warning when a capture lands items the queue cannot mirror.
- `#hswspawncount` — `#harnessswitchverify` wants "one spawn per authoritative switch … no refusal or respawn storm". Four scenarios already exist (`src/sim_world.rs:6556` persists the record so post-restart dispatch does not defer, `:6663` accepted handoff then idle-watch fresh restart, `:6769` paused supervisor holds for resume with no restart, `:6856` disabled restart bails explicitly) but **none asserts a spawn count across a multi-switch sequence**, which is the actual invariant. Deterministic coverage shrinks that review item to the unsaved-editor eyeball.
- `#hswdisposabledoc` — that same item names its own unblocker as "provide the disposable document path". That is setup work, not operator work: scaffold the throwaway document with `agent: codex` frontmatter and a one-screen script naming the exact edit and the ops.log receipts to expect.
- `#splicesimcov` — `#activateinstalledjetbrai` is down to the captured-edit splice proof. `tests/cross_editor_simworld.rs` holds only three tests and none touches the splice path; per `plan-captured-editor-splice-recovery.md` the native side is proven only through the ABI test. A cross-editor SimWorld scenario for a captured edit rebased across an independently advanced canonical response closes the deterministic half.
- `#pypiquotarecheck` — `#pypiquotafix`'s precondition is **met**: the thin wheel is live (0.35.383, 35,389 bytes, 2026-09-13T17:29:35). Its figures were stale, so I refreshed them in place — 185 releases / 9.99 GiB total, and the 34 oldest (0.35.0 → 0.35.112) hold **1.63 GiB**, not ~1.75 GiB. `pypi/support#12214` is still open with a single comment, the github-actions bot auto-reply from 2026-09-10T22:46:21Z. If that 25 GiB grant lands the whole P1 browser task disappears, so rechecking it is cheap agent work.

**Left gated, with cause:** `#tmuxfocussyncverify` (fix landed in c4b62e733, controller focus to `%215` already succeeded — only the live tab/split click remains), `#grokmcpverify` (external quota/subscription), `#coder7` (needs a reachable Coder workspace plus auth).

**One finding outside this document:** `corky` will not start — `corky: /usr/lib/libonnxruntime.so.1: version 'VERS_1.27.1' not found (required by corky)`, while the system ships `libonnxruntime.so.1.29.0`. It surfaced because the YouTube lookup was the only remaining locator path while triaging `#cargovodtrim`. Filed into `tasks/software/corky.md` as `#ckyonnx`, whose backlog does carry `priority queue`, so it mirrors into its queue there.

No repo code changed this cycle, so there is no spec/test update and no build/install to run — the preset's remaining half is the commit and push, which this closeout owns.

### Re: pypiquotafix complete — opus-5 · 2026-09-26T22:58-04:00

> **Queue prompt:** complete [#pypiquotafix]: logged in

Done. All 34 authorized releases are deleted and PyPI's own settings page now reports **8.4 GiB of the 10.0 GiB limit**, down from 9.99 GiB — the project was within 10 MiB of its ceiling.

**Session verified before touching anything.** `chromium-bridge` reached Chrome 153.0.8010.37, tab 1 was `pypi.org/manage/projects/` with `#user-indicator` reading `btak` and the full project list rendered, so you were genuinely authenticated, not sitting on the `/account/confirm-login/` device prompt in tab 2.

**Boundary re-derived from PyPI rather than trusted from the item.** The 34 oldest are `0.35.0` … `0.35.112` holding **1.63 GiB** — confirmed against the JSON API, matching the corrected figure from the last cycle rather than the original ~1.75 GiB. One label correction: the item said "all pre-2026-08-02", but `0.35.112` uploaded at `2026-08-02T17:12:38`, so it is *on* that date, ~17 hours past the stated cutoff. The version boundary was the explicit authorization, so I deleted through `0.35.112` as named and flag the date wording as the inaccurate half.

**Executed through the real form, not by clicking 34 modals.** The release page carries a dedicated whole-release form — POST to `/manage/project/agent-doc/release/<v>/` with `csrf_token` + `confirm_delete_version` — distinct from the per-file `confirm_project_name` forms above it. I deleted `0.35.0` alone first to prove the flow before batching, so a misread would have cost one release rather than 34. The remaining 33 ran sequentially with a per-request CSRF token freshly parsed from each release page and a 500 ms gap: **33/33 `ok`, zero failures**, every response redirecting to `/manage/project/agent-doc/releases/`, 99 s total.

**Verified from the authenticated page, not the cached API.** The management releases page lists **151** releases — exactly 185 − 34 — with **zero survivors** of the 34 and `0.35.113` as the oldest remaining, precisely the next version past the boundary. Worth recording: `pypi.org/pypi/agent-doc/json` was still serving a stale CDN view claiming 27 of the 34 were present. Do not verify a PyPI deletion from that endpoint.

**What the remaining 8.36 GiB is made of**, since it decides whether this recurs:

| bucket | releases | size |
|---|---|---|
| `>= 0.35.383` (thin wheels) | 20 | **0.7 MiB** |
| `< 0.35.383` (fat wheels) | 131 | **8.36 GiB** |

Growth is effectively over — 20 releases since the thin-wheel cutover cost 0.7 MiB combined, against ~68 MiB apiece before it. So the 1.6 GiB of headroom is not a countdown, it is slack. If a ceiling is ever hit again the next clean tranche is `0.35.113` … `0.35.152`, another **2.10 GiB**, but nothing forces that now.

`#pypiquotafix` is resolved. I reframed `#pypiquotarecheck` rather than closing it: the deletion it was hedging against has happened, so its remaining value is making sure `pypi/support#12214` ends in a limit raise instead of a second mass deletion.

No repo code changed, so there is no spec/test update and no build/install — the preset's remaining half is the commit and push this closeout owns.

### Re: tmux focus did not auto-sync — opus-5 · 2026-09-26T23:18-04:00

> **Queue prompt:** [#tmuxfocussyncverify]: did not auto-sync

Confirmed from the controller side, and the review item was narrowed on bad evidence. `#tmuxfocussyncverify` is out of review and back in the backlog as actionable.

**Your report is reproduced in the log.** Selection-driven focus sync is normally busy: across 2026-09-24 → 27 there were **854 `pane_layout_projection` events, 736 of them not within 20 s of any `controller_editor_route_started`** — so the overwhelming majority were driven by editor selection, not by an explicit route action. Hourly counts ran 4–18 continuously. Then:

| | |
|---|---|
| last **selection**-driven projection | `2026-09-27T02:25:42Z` |
| last projection of any kind | `02:33:26Z`, route-driven (`controller_editor_route_started` → projection → `controller_editor_route_layout_converged`) |
| since then | **zero** `pane_layout_projection`, `pane_layout_survey_timing`, `pane_layout_projection_cancelled`, `focus_*`, or `controller_automatic_focus_suppressed` |

That is ~53 minutes of total silence covering the window you tested in. Not a logged refusal — silence.

**Three candidate causes ruled out:**
- **Not a stale pane→window binding.** `%231` is recorded `window=@0` and `tmux display-message` puts it in `@0` = `0:0(agent-doc) active=1`. `session doctor` reports no issues, and I grepped for `is stale` rather than anchoring on the `doctor:` header.
- **Not a dead or unloaded plugin.** `CrdtReplicaForwarder` / `CrdtReplicaManager` are still forwarding deltas for this document as recently as 23:01:13, and both `EditorFocusSyncListener` and `TmuxPaneFocusSync` are installed from `PluginLifecycleListener` on project open.
- **Not a controller replacement.** The `controller_lifecycle` lines at 02:33:23/24 are ready↔busy transitions at the same `generation=157`, same pane, around the `/clear`; no recycle, no new socket.

**Why it could not be root-caused further — and that is the bug I fixed.** `handle_editor_surface_observe` did this:

```rust
let (accepted, mut receipt) = runtime.editor_surface_graph.observe(…);
if accepted { /* Sync → publish, Focus → select-pane, Idle → {} */ }
```

Nothing was logged on ingress, and nothing at all was logged when the graph **rejected** the observation. So "the plugin published and the graph fenced it" and "the plugin never published" produce byte-identical logs: silence. The only logging branch was `Focus` + `desktop_editor_inactive`. Worse, the plugin's own decisions go through `EditorTabSyncListener.log()`, which is `LOG.debug` — absent from `idea.log` at its default level — so neither side leaves a trace. A persistent silent stall is exactly what a stale `(client_id, generation, sequence)` identity would produce, and `forgetEditorSurface`/`forgetEditorFocus` fire from four sites in the plugin including a "pre-publish surface root retirement **deferred**" path.

**Landed** (`75a112ec4`, superproject `449f40a64`): every observation now emits, unconditionally and ahead of any intent handling,

```
controller_editor_surface_observed client=<id> generation=<n> sequence=<n> accepted=<bool> intent=<sync|focus|idle>
```

and every retirement emits `controller_editor_surface_forgotten client=… generation=… client_family=… forgotten_clients=…`, because a stray or early forget is the leading cause of a graph that rejects everything after it.

The regression drives **both** branches through the real handlers — observe, then retire the generation through `handle_editor_surface_forget`, then re-observe — and asserts the logged `accepted=` equals the graph's own verdict rather than a constant. I mutation-checked it two ways: hardcoding `accepted=true` reddens it (`the receipt must report the graph's real verdict, not a constant`), and deleting the ingress log reddens it (`every observation must leave one ingress receipt`); restored, it is green. The mutated run also printed the real behaviour — the re-observation after retirement is `accepted=false intent=idle`.

`make check`: **9808 tests run, 9808 passed, 259 skipped, exit 0**, zero `result: FAILED`, zero `error:` lines, exit status captured outside any pipe. Latest CI run 36277005969 is green. Two caveats I will not paper over: that tree also carried another session's uncommitted edits to `agent-doc-commit-io`, `project_controller.rs`, `agent-doc-queue` and `src/plugin_activation.rs`, so the green is not purely mine; and because those edits sit in the same `rpc.rs` I touched, I committed a HEAD-based blob carrying only my hunks (150 insertions, 0 deletions) rather than `--only`, which would have swept their work in. Their changes are still in the worktree, untouched.

**What this does not do: it does not fix the sync.** It makes the next click diagnostic instead of silent. The honest remaining sequence is install the new binary, click once, and read which of the two states the receipt names — a publish that arrives and is fenced, or no publish at all. So `#tmuxfocussyncverify` is now an actionable backlog item carrying that exact procedure, not a review gate.

**And the gate should never have been narrowed to "only the live click eyeball remains."** That line rested on "direct controller focus to pane %215 succeeded" — a controller/route-path success. The log shows the route path and the selection path are different producers, and the last projection before the silence was route-driven. Proving the route path never proved the plugin→controller selection path.

**One ordering note on the install.** I am not installing mid-drain. A mid-session `make install` strands the attached document's Lazily replica and wedges closeout at `prevalidate_tracked_work_response` with `missing_replica`, which `admin reload-lib` does not clear — only reopening the editor tab does, and this document is attached to live IntelliJ pid 3129637. The recorded ordering is close out first, install after, so the install and recycle land once the queue drains rather than in the middle of it; the receipt is useless until then anyway, since reading it requires a click after the new binary is running.

### Re: validate activateinstalledjetbrai from logs — opus-5 · 2026-09-26T23:31-04:00

> **Queue prompt:** [#activateinstalledjetbrai]: Please validate from logs

It cannot be validated from logs, and the reason is worth more than the answer: **the evidence class this gate needs has never been produced.** Not inconclusive — absent. Two defects fell out, one of which I fixed.

**The splice-recovery path itself is alive and healthy.** `source=captured-local-splice-recovery` appears **15 times** across 2026-09-24 → 27, on two real documents (14 on this one, 1 on `tasks/software/lazily.md`), every one `status=current authority=cp_model`, none failing. The canonical text was demonstrably advancing between recoveries — the 2026-09-26T00:11:48→52 burst runs `text_len` 42885 → 42885 → 42892 → 42900 with three distinct hashes.

**But none of those carried an operator edit.** Every one of the **317** `editor_op_epoch_closed` events in that window reads `cause=non_operator_projection action=cleared`. Not one operator-authored epoch, against **216** `editor_open` events.

**And the capture ledger is completely dormant:**

| marker | role | count |
|---|---|---|
| `editor_op_recorded` | one-op FFI producer | **0** |
| `editor_ops_recorded` | batch FFI producer (JetBrains) | **0** |
| `editor_op_record_failed` | one-op producer failure | **0** |
| `editor_ops_record_failed` | batch producer failure | **0** |
| `editor_ops_for_base accepted=true` | merge consumer receipt | **0** |

Zero successes *and* zero failures means `TypingTracker.kt:527` is never reached — the FFI is not being called and then failing, it is not being called. That matters because you typed into this attached document repeatedly today; the four queue heads I have been draining this session are your edits, in IntelliJ, on a document with a live registration. Those edits arrive as `crdt_document_op_delta_ingested` deltas, but they are never recorded as captured *ops*.

So the gate is blocked behind something upstream of itself: you cannot prove operator-captured local edits survive an independent canonical advance while operator edits are not captured at all. That is why this is out of review and back in the backlog, with the dormant ledger filed ahead of it as `#opcapturedormant`.

**The defect I fixed is that the verifier could not have told you any of this.** `agent-doc verify-op-capture` demanded `editor_op_recorded` — the marker written by the *one-op* FFI `agent_doc_record_editor_op`. The shipped JetBrains plugin records **only** through `agent_doc_record_editor_ops_json` (`TypingTracker.kt:527`), whose success marker is `editor_ops_recorded count=<n> base=<hash> transaction=batch`. So the verifier was a **permanent false negative on every real JetBrains session**, no matter how much had actually been captured, and its failure text named a marker no JetBrains session can emit. I nearly read its `missing editor_op_recorded marker` as the answer to your question; it was the wrong instrument reporting on itself.

Landed in `6b0eabc0c` (superproject `b4f5800da`): either producer proves capture, both failure markers count as failures, and when neither producer is present the diagnostic **names both** so a genuinely dormant ledger is never again misread as the wrong-marker bug. The locally built binary now says exactly that against this document:

```
missing editor-op producer marker for doc=agent-doc-bugs … — expected either
`editor_op_recorded` (one-op FFI, #qnodemerge4wire) or
`editor_ops_recorded ... transaction=batch` (batch FFI, #qbasehashmemo, the path
the JetBrains TypingTracker uses)
```

Three regressions: the batch producer is accepted, a batch failure marker is not silently ignored, and the both-producers diagnostic is asserted. Mutation-checked by making the batch matcher unreachable — two of the three redden. `make check`: **9811 tests run, 9811 passed, 259 skipped, exit 0**, zero `result: FAILED`, zero `error:` lines, status captured outside any pipe. Same caveat as the previous cycle: the tree carried another session's uncommitted edits, but neither file in this commit is one they touched, so `--only` was safe here.

**One connection worth flagging.** This is the second consecutive head whose root cause is a plugin→controller publish path that is silent rather than refusing: editor-surface observations logged nothing on ingress, and editor-op capture logs neither success nor failure. Both now have receipts. If the next click produces an `accepted=false` on one and still nothing on the other, that asymmetry is itself the lead.

Exchange is large (129 lines, 4 sections, warn-level accretion) — noting it rather than stalling the drain to compact.
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~#actionable-review~~
- ~~complete [#cargovodtrim]: no live stream. Just delete from history.~~
- ~~complete [#pypiquotafix]: logged in~~
- ~~[#tmuxfocussyncverify]: did not auto-sync~~
- ~~[#activateinstalledjetbrai]: Please validate from logs~~
- [#harnessswitchverify]: validate from logs
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#tmuxfocussyncverify] [agent-doc] [operator-verify] Root-cause the editor-selection tmux focus sync from the new ingress receipt, then fix it. DISPROVEN 2026-09-26 by the operator ("did not auto-sync"), so this is no longer a verification gate. Confirmed from ops.log: selection-driven focus sync produced 854 pane_layout_projection events 2026-09-24..27, 736 of them NOT within 20s of any controller_editor_route_started (4-18/hour continuously); the last selection-driven one was 2026-09-27T02:25:42Z and the last of any kind was 02:33:26Z (route-driven), after which there are ZERO pane_layout_projection, pane_layout_survey_timing, pane_layout_projection_cancelled, focus_*, or controller_automatic_focus_suppressed events for ~53 minutes covering the operator test window. Ruled out: stale pane->window binding (%231 recorded @0, tmux puts it in @0 = 0:0(agent-doc) active=1, session doctor clean, grepped for `is stale` not the `doctor:` header); dead plugin (CrdtReplicaForwarder still forwarding at 23:01:13; EditorFocusSyncListener and TmuxPaneFocusSync both installed from PluginLifecycleListener); controller replacement (02:33:23/24 controller_lifecycle lines are ready<->busy at the same generation=157, no recycle). Leading hypothesis is a stale (client_id, generation, sequence) identity making editor_surface_graph.observe reject every later fact — the plugin calls forgetEditorSurface/forgetEditorFocus from four sites including a "pre-publish surface root retirement deferred" path. NEXT STEPS, in order: (1) install the binary carrying 75a112ec4 and recycle — deferred out of this drain because a mid-session install strands the attached document replica ([[reference_cdylib_reload_strands_attached_document_replica]]) and this document is attached to live IntelliJ pid 3129637; (2) have the operator click once between two visible session documents; (3) read `controller_editor_surface_observed ... accepted=` in ops.log — `accepted=false` means the publish arrives and the surface graph fences it, so fix the fence/retirement; NO line at all means the plugin never publishes, so raise EditorTabSyncListener.log from LOG.debug (invisible in idea.log at default level) and instrument the emit path. Also fix the narrowing error that produced this stall: the old gate text claimed "only the live tab/split click eyeball remains" on the strength of "direct controller focus to pane %215 succeeded", but route-path and selection-path are different producers and proving the former never proved the latter. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [ ] [#opcapturedormant] [agent-doc] [P1] The editor op-capture ledger records NOTHING: across 2026-09-24..27 this project logged 216 editor_open and 317 editor_op_epoch_closed events — every single one cause=non_operator_projection action=cleared — with ZERO editor_op_recorded, ZERO editor_ops_recorded, ZERO editor_op_record_failed and ZERO editor_ops_record_failed. Zero successes AND zero failures means editors/jetbrains/.../TypingTracker.kt:527 (`lib.agent_doc_record_editor_ops_json`) is never reached, not that it is failing. This is not a no-typing artifact: the operator typed four separate queue heads into tasks/agent-doc/agent-doc-bugs.md in IntelliJ on 2026-09-26/27 while it held a live registration (pid 3129637, plugin 0.2.428), and those edits arrived as crdt_document_op_delta_ingested deltas without ever being recorded as captured ops. Consequence: every captured-local-splice-recovery invocation (15 in the same window, all status=current, on agent-doc-bugs.md and tasks/software/lazily.md) ran against agent-driven projections only, so no operator-captured edit has ever exercised the splice path in production, and #activateinstalledjetbrai cannot be proven while this holds. NEXT: instrument or read the TypingTracker guard chain to find why the batch record call is not reached (candidate causes: the op epoch being closed as non_operator_projection before the operator burst is flushed, a base-hash memo miss, or the tracker not being installed for the document); note that a bare absence is now diagnosable because verify-op-capture names both producers as of 6b0eabc0c. Blocks #activateinstalledjetbrai and #splicesimcov.
- [ ] [#activateinstalledjetbrai] [agent-doc] [operator-verify] Prove captured local editor edits recover across an independently advanced canonical response — BLOCKED ON #opcapturedormant, and no longer validatable from logs. Asked to validate from logs 2026-09-26 and the answer is that the evidence class is ABSENT, not inconclusive: the splice-recovery path itself is healthy (15 captured-local-splice-recovery invocations 2026-09-24..27, all status=current authority=cp_model, with the canonical text demonstrably advancing between them — the 2026-09-26T00:11:48..52 burst runs text_len 42885 -> 42885 -> 42892 -> 42900 across three distinct hashes), but all 317 editor_op_epoch_closed events in that window are cause=non_operator_projection, so not one of those recoveries carried an operator-authored edit. Order of work: (1) fix #opcapturedormant so operator edits are recorded as ops at all; (2) re-run `agent-doc verify-op-capture <FILE>` — it now accepts the batch producer the JetBrains TypingTracker actually drives (6b0eabc0c), so a pass is meaningful and a failure names both producers; (3) only then does the disposable-document live proof have anything to prove. Do NOT re-narrow this gate to "just needs the live check" until step 1 lands. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [ ] [#bkqattrcli] [agent-doc] No command can add a component attribute, so "into backlog + queue" is unreachable from the CLI on any document whose `agent:backlog` marker lacks `queue`. Verified 2026-09-26 on a scratch copy of tasks/agent-doc/agent-doc-bugs.md: two `--backlog-add` items landed in agent:backlog and mirrored NOTHING into agent:queue. `--backlog-queue-placement` is documented as acting only "when the backlog carries a `queue` attribute" and `agent-doc queue sync` is "sync from backlog items with `queue` attribute", so both are gated on it; `agent-doc backlog` has no set-attr, `write` exposes no marker flag, `patch` replaces component CONTENT only, and `migrate` only STRIPS deprecated attributes. The one remaining route is hand-editing the marker, which is unavailable precisely when it matters: reliable-sync-status shows this document live under IntelliJ pid 3129637 / plugin 0.2.428, and a raw disk edit under a live editor becomes a disk candidate the agent must never resolve. Add `agent-doc backlog set-attr <FILE> <attr>` / `unset-attr` (accepting at least `queue` and `priority`) routed through the same editor-converging write path as `backlog add` — the marker span is already modelled as Component.open_start/open_end with parsed attrs in agent-doc-element/src/element.rs (parse_attrs at :736), so this is a marker-span mutation class rather than a content patch. Also emit a preflight warning when a capture cycle adds backlog items that the queue cannot mirror, so the "+ queue" half of an actionable-review-style instruction fails loudly instead of silently landing half.
- [ ] [#hswspawncount] [agent-doc] Add a SimWorld scenario asserting exactly ONE supervisor spawn per authoritative harness switch across a codex -> claude -> codex sequence, with no refusal and no respawn storm — the deterministic half of the #harnessswitchverify review gate. Grounding (src/agent-doc, 2026-09-26): four scenarios already cover adjacent behaviour — src/sim_world.rs:6556 route_sim_harness_switch_persists_record_so_post_restart_dispatch_does_not_defer, :6663 route_sim_harness_switch_accepts_handoff_then_idle_watch_drives_fresh_restart, :6769 route_sim_harness_switch_paused_supervisor_holds_for_resume_no_restart, :6856 route_sim_harness_switch_disabled_restart_bails_explicitly_no_silent_proceed — but NONE asserts a spawn COUNT over a multi-switch sequence, which is the actual invariant the review item names. Landing this shrinks #harnessswitchverify to the unsaved-editor eyeball instead of asking the operator to prove a countable invariant by hand. Plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [ ] [#hswdisposabledoc] [agent-doc] Create the disposable session document that #harnessswitchverify names as its own unblocker ("provide the disposable document path plus perform the real unsaved editor edit"). That setup is agent work, not operator work: scaffold a throwaway document with `agent-doc init`, `agent: codex` frontmatter and a trivial exchange, then write a one-screen operator script naming the exact frontmatter line to edit unsaved, the codex -> claude -> codex order, and the ops.log receipts to expect (route_harness_switch_handoff_accepted, the supervisor spawn receipt). Leaves the operator only the live editor edit. Pairs with #hswspawncount.
- [ ] [#splicesimcov] [agent-doc] Add a cross-editor SimWorld scenario for captured-edit splice recovery across an INDEPENDENTLY ADVANCED canonical response — the single remaining proof in the #activateinstalledjetbrai review gate. Grounding (2026-09-26): tests/cross_editor_simworld.rs holds only three tests (cargo_target_debug_dir_honors_default_relative_and_absolute_targets, cross_editor_plugin_protocol_harnesses_peer_through_real_agent_doc_controller, native_plugin_harnesses_peer_through_real_agent_doc_controller) and none exercises the splice path; per tasks/agent-doc/plan-captured-editor-splice-recovery.md the native side is proven only through the ABI test plus one manual installed-native call. Cover the plugin-side sequence the plan describes: observe the controller cut, plan before replacing membership, CAS-register that cut, forward bounded splices, and assert the translated offsets land while an unrelated response change is in flight. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [ ] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision so the next PyPI ceiling is raised rather than cleared by another mass deletion. Context changed 2026-09-26: #pypiquotafix is DONE — the 34 oldest releases (0.35.0 through 0.35.112, 1.63 GiB) are deleted and PyPI settings now reports 8.4 GiB of the 10.0 GiB limit, so there is no urgency. #12214 is still open with only the github-actions bot auto-reply from 2026-09-10T22:46:21Z. Growth is already bounded: the 20 releases at or after the 0.35.383 thin-wheel cutover total 0.7 MiB, against roughly 68 MiB apiece before it, so the remaining 8.36 GiB is 131 historical fat wheels and is not growing. If the grant lands, record it and drop this item. If it is refused or stalls past the next ceiling, the next clean tranche is 0.35.113 through 0.35.152 (another 2.10 GiB) — but do not delete anything until a ceiling is actually in reach. Verify deletions from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json, which served a stale CDN view claiming 27 of the 34 deleted releases were still present.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#harnessswitchverify] [operator-verify] Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal or respawn storm. PREMISE CORRECTED 2026-09-26T14:48-04:00: the recorded pid 1023909 / staged-0.2.388 / CLI-0.35.400 triple is entirely dead. Live IDEA is pid 3129637 running plugin 0.2.427 (mapped and linked, hot-loaded, no restart pending); installed CLI is 0.35.418. A codex -> claude switch was made in this document today, but on a live rather than disposable session and with no supervisor spawn receipt in ops.log, so it is not the proof. Unblocks: provide the disposable document path and perform the real unsaved editor edit — persisted CLI writes cannot substitute. Plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
