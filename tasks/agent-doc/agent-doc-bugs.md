---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a1545a-f42c-7aa1-b9a0-aeafeba96910
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': Fix agent-doc causes
  '#audit': 'Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.'
  '#self': agent-doc-bugs.md
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status

<!-- agent:status patch=replace sync="Track the latest published agent-doc release + publication time and latest JetBrains plugin release version + publication time." -->

📥 Queue handoff: 2 reported items awaiting coordinator integration, verification, and closeout.
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).

<!-- status:sync -->
Latest published release: [v0.35.526](https://github.com/btakita/agent-doc/releases/tag/v0.35.526), published on GitHub at 2026-10-20 04:16:57 UTC (15 assets); PyPI 0.35.526 followed at 2026-10-20 04:17:50 UTC. JetBrains packages are unchanged at 0.2.720 (classic), 0.2.721 (IDE 262), and 0.2.722 (IDE 263), because the plugin sources were byte-identical; this release re-attached them at 2026-10-20 04:16:55–56 UTC. Post-release on main: 4dd943bcf7 (the binary records blocking subagent operator requests in agent:todo) is installed locally; CI for it is still in progress. Two review items remain gated: jb262dynlauncherrecommen needs an operator decision, and pypiquotarecheck needs an external PyPI reply.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261019-133003.md`*

Compacted content:
- Archived 75 response topic(s): Queue-prompt flood and GH #282–#283; Publication status; release + publish.; 72 more
- Prior summary/context: prior compacted content: Archived 26 response topic(s): unwedging docs.md; Subagent completed: `subagent:todoidlayer`...

### Re: Subagent completed: `subagent:issue310` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/310” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=390e70281d0f89aa1af1a85da231f627470dfab2465d45ccaf097b857090d043 -->

### Re: Subagent completed: `subagent:archivepath` returned work for queue item “re: `agent:done` `archive` relative path. We made a breaking change to make the relative path based on the current document. Can we support both the project relative path behavior and document relative path behavior? What do you suggest?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=05f85d7b45ee216b5964acc88bebe229dc639ff1afd2106190e15c48d8e6a49d -->

### Re: Subagent completed: `subagent:issue309` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/309” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=1a7f76ddd345c1495af14616e8378faff3d438114540886a8098b1834f5675b8 -->

### Re: Subagent completed: `subagent:capacitycont` returned work for queue item “Codex is still getting stuck on `■ Selected model is at capacity. Please try a different model.` and agent doc is not prompting `continue`.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=dd43a96b7a024532d335452980e52256b2ad0b68f8d1d5dd2ca9132e6b8f3508 -->

### Re: Subagent completed: `subagent:issue308` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/308” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=e49ca37934cd09021af775861b3400f4d68e942a28b6a7fa83587605690a6d32 -->

### Re: Subagent completed: `subagent:extractclaim` returned work for queue item “Does `agent-doc extract` also auto-claim? See sdk-demo.md.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=3ca55e3c4dd4520beccbc1489a9a9dec3b528c89a4109f8d83e105fe528879d2 -->
> **Chat prompt (#chatprompt):** cont

> **Chat prompt (#chatprompt):** cont

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/308

### Re: GitHub issue 308 — gpt-5 · 2026-10-19T10:36-04:00

Fixed, landed on `main` in `25f61a653f`, and closed. Subagent-intent heads are now excluded from inline drain/consume unless explicitly marked inline; session-check points the coordinator back to dispatch, consume refuses an unclaimed subagent head, and authorization that conflicts with worker safety rules stays coordinator-owned.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/309

### Re: GitHub issue 309 — gpt-5 · 2026-10-19T10:36-04:00

Fixed, landed on `main` in `33d0e39b9b`, and closed. Bare `[#id]: note` queue heads now keep free-text identity, strike from an exact queue-prompt echo, and return actionable bare-consume guidance when `--id` or `--ack-id` does not apply.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/310

### Re: GitHub issue 310 — gpt-5 · 2026-10-19T10:36-04:00

Fixed, landed on `main` in `323b3ac0f9`, and closed. Free-text admission writes only to the selected queue, and loop-pass rosters prune superseded intermediate typing states while preserving live or struck-complete work.

> **Queue prompt:** re: `agent:done` `archive` relative path. We made a breaking change to make the relative path based on the current document. Can we support both the project relative path behavior and document relative path behavior? What do you suggest?

### Re: done archive path compatibility — gpt-5 · 2026-10-19T10:36-04:00

Yes. The compatible policy is now implemented in `73bcacd51f`: unprefixed `archive=path.done.md` keeps the legacy project-relative meaning, `archive=document:path.done.md` explicitly selects document-relative resolution, `archive=project:path.done.md` is explicit project-relative resolution, and bare `archive` still defaults beside the document. Every path retains inside-project and symlink-escape checks, and all consumers use the same resolver.

> **Queue prompt:** Codex is still getting stuck on `■ Selected model is at capacity. Please try a different model.` and agent doc is not prompting `continue`.

### Re: Codex capacity continuation — gpt-5 · 2026-10-19T10:36-04:00

Fixed in `d8fcd79535`. Recovery now recognizes the standalone renderer-owned capacity error even while Codex remains classified busy, submits one `continue` per visible error presentation, rearms only after the error disappears, and clears a matching stale dispatch projection before retrying.

> **Queue prompt:** Does `agent-doc extract` also auto-claim? See sdk-demo.md.

### Re: extract ownership and auto-claim — gpt-5 · 2026-10-19T10:36-04:00

No, intentionally. `agent-doc extract` performs a read-only destination ownership check but does not auto-claim or start the target: rebinding the invoking pane would orphan the still-active source session. When the extracted child should become live, use `agent-doc claim TARGET --new-pane`. Specs, all harness runbooks, CLI help, and regression coverage now state this explicitly in `a9bbf1c12e`.

### Re: integrated verification and publication — gpt-5 · 2026-10-19T10:36-04:00

The six commits were serially integrated and fast-forwarded to `origin/main` at `a9bbf1c12e`. The authoritative clean-worktree `make check` passed with producer exit `0`, zero failures, and 11,081 nextest tests passed (173 skipped); the remaining batched, doc, and frontend checks also passed. The single batch `make install` completed: binary/library installation was atomic, JetBrains packages converged, and the VS Code-compatible extension installed. GitHub CI run 39270335983 for `a9bbf1c12e` is in progress; local verification is the completion evidence for this turn.

The separate `release + publish` queue item remains with its pre-existing owner and is not consumed by this response.

### Re: recovered response

Agent-doc cycle completed successfully.

- Six worker results were integrated and recorded in [agent-doc-bugs.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md), committed as `dc0679600d`.
- GitHub issues #308–#310 were fixed and closed.
- Fixed the reported-handoff closeout regression, pushed to `origin/main` as `42bfbdc4d5`, and installed it.
- Verification passed: 11,082 full-suite tests plus editor/plugin checks.
- Final `agent-doc session-check` passed with no drainable work.
- `release + publish` remains assigned to its existing worker; no duplicate worker was started.

### Re: recovered response

Diagnostics are complete. You can unwedge it now by restarting JetBrains, then reopen `sdk-demo.md`. No need to preserve the wedged state.

Disk content and permissions are unchanged; the stale read-only flag exists only in the current IDE process.
### Re: Subagent completed: `subagent:issue315` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/315” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=6bad6ffa7a2cd16ffa164384b7669cb3f2cb96dfe2ac367284a60f53649d68fc -->

### Re: Subagent completed: `subagent:issue314` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/314” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=89bf6190406c6f6d1430b3b73f8db7c8cda9a871394138b6c0deda8a57d05557 -->

### Re: Subagent completed: `subagent:sdkdemoreadonly` returned work for queue item “sdk-demo.md is Read Only. Why? Did agent-doc make it read-only? It should not have.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=81db76ff1c619f6bfb3d628089a190947e7834e9933f5849b3a10f80a4a11985 -->
> **Chat prompt (#chatprompt):** What was the last published release and when was it published?

> **Chat prompt (#chatprompt):** continue

> **Chat prompt (#chatprompt):** cont

> **Chat prompt (#chatprompt):** continue

> **Chat prompt (#chatprompt):** have we published a recent release?

> **Chat prompt (#chatprompt):** Can you keep the agent:status up to date with the last publish release?

> **Chat prompt (#chatprompt):** Should we have a `sync="..."` attribute on `agent:status` to allow the operator to prompt the agent to keep the status in sync?

> **Chat prompt (#chatprompt):** Should we make sync additive, so the other status messages that would normally be used are also included?

> **Chat prompt (#chatprompt):** How can we make the sync use the non-default destructive mode?

> **Chat prompt (#chatprompt):** I agree with your proposal

> **Chat prompt (#chatprompt):** Do we have docs on all tags and their attributes? If not, create the docs and link to those docs in the README.md

> **Chat prompt (#chatprompt):** do these tasks in subagents

> **Chat prompt (#chatprompt):** The release is published?

> **Chat prompt (#chatprompt):** sdk-demo.md is wedged in read-only

> **Chat prompt (#chatprompt):** Can we unwedge it now or do you need it for diagnostics?

> **Chat prompt (#chatprompt):** subagent: fix: • Blocked by hook └ Recovery continuation, not a refusal or error: agent-doc Stop hook retained the exact closeout for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md under its durable keyed retry. The latest assistant text was captured into the pending/capture ledger before auto-close. The hook could not finish the required commit boundary: closeout operation is already in progress for cycle cycle-1792424550895 by captured-finalize-resume-2833522-1792439873427526237-t1 pid=2833522 role=captured_finalize_resume until=1792440173 [closeout_owner_retry_at=1792440173]. The retained capture or projection is already durable and this exact retained intent commits itself once delivery converges — this is a deferral, not a lost response. Wait for its controller-owned terminal state edge; do not use the document's current-cycle status as proof because queue continuation can advance to a different cycle. Do NOT re-send the response, force disk, or `admin recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own either: `admin reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor holding the document is not serving its replica, and then its remedy governs. The controller owns delivery, the editor-native save receipt, and the terminal commit. Do not reopen or recapture the cycle, rerun finalize/write/repair, force disk, recycle the supervisor, or resend the response. Wait for the existing controller state edge unless it explicitly reports `needs_operator`; do not send the final answer yet.

> **Queue prompt:** release + publish

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/311

> **Queue prompt:** release + publish: include #311

> **Queue prompt:** bossroom-p2p.md: Claim Tmux Pane, then Run Agent Doc, then Ctrl+c in the Claude session, then switch to agent to codex using `agent: codex`, then Restart Agent: got a prompt saying a turn was occurring. Clicked Interrupt and Restart then the followup Interrupt and Restart. Nothing happened. The Claude Code prompt was submitted and the turn started in Claude Code, not Codex. `agent: codex` was removed from the document.

> **Queue prompt:** I saw `■ Selected model is at capacity. Please try a different model.`, and a `continue`, then another `■ Selected model is at capacity. Please try a different model.`. Please confirm the behavior and work on any necessary fixes. Run Agent Doc has an error as well, presumably due to agent-doc's state not recognizing the error: [route] a turn is already running on /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md; no second trigger was sent and nothing needs recovery. The live turn owns the queue; head "release + publish" is in its scope and is answered or drained at that turn's boundary. Do not cancel or recover it (owner: [session-check] IN PROGRESS: cycle `cycle-1792424550895` remains `preflight_started` (preflight_started) while owning pane `%2` has a fresh active-turn lease — the response turn is still running, so no response capture or terminal commit exists yet. This is not abandoned closeout evidence. Do not recover or abandon this cycle, rerun finalize/write, or restart/clear the owner from another pane. Re-run `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md` only after the owning turn reaches its terminal boundary.) owner_turn=live ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/312

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/313

> **Queue prompt:** the queue id=issues gained "In sdk-demo.md, I deleted some struck queue items right after they were struck. Got an error: Agent Doc could not attach the open editor replica for sdk-demo.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.: I have since undid my changes." and lost the prompt to watch https://github.com/btakita/agent-doc/issues.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/314

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/315

> **Queue prompt:** sdk-demo.md is Read Only. Why? Did agent-doc make it read-only? It should not have.

> **Queue prompt:** release + publish including PRs #312-#315

### Re: release, synchronized status, and tag reference — gpt-5 · 2026-10-19T18:26-04:00

Released and verified. GitHub published [v0.35.525](https://github.com/btakita/agent-doc/releases/tag/v0.35.525) at 2026-10-19 22:23:24 UTC, and PyPI published `agent-doc 0.35.525` at 22:24:09 UTC. All six native archives, `SHA256SUMS`, VS Code 0.2.83, JetBrains 0.2.720/0.2.721/0.2.722 packages, Release, CI, and PyPI verification are green.

The `agent:status` opener now has a persistent `sync` instruction and this cycle refreshed it to v0.35.525. Synchronization is additive by default, preserving ordinary status text and operational projections. Use `sync-mode="replace"` for the explicit destructive mode; protected queue/restart/repair projections still survive. The user-facing tag and attribute reference is `docs/reference/tags-and-attributes.md`, linked from `README.md`; it covers built-in and generic components, common attributes, status synchronization, queue scheduling/routing attributes, backlog-to-queue synchronization, response patches, internal markers, and validation behavior. These shipped in `79cee1fde7` and `8ae7a390c9`.

### Re: integrated queue fixes — gpt-5 · 2026-10-19T18:26-04:00

Issue #311 was fixed in `0256991fe3` and closed; restart-agent retries now preserve the intended Codex target instead of falling back to the interrupted Claude session. The capacity presentation is handled by `d8fcd79535`: one `continue` is submitted per visible standalone capacity error, including while Codex still appears busy, and it rearms only after the presentation clears.

Issues #312–#315 are closed and included in v0.35.525. Claimed subagent heads are skipped by orphan drain (`38b8b4b451`); same-cycle free-text strike authority survives premature abandonment (`38bf307745`); missing operator TODO handoffs produce an advisory (`6c6061670b`); and transient editor replica loss with stale plugin bytes is held for bounded recovery instead of immediately escalating (`f86766e5be`). Deletion-only removal of settled struck queue rows is reconciled as a keyed tombstone in `e6d8ad688d`, preserving the live issue-watch prompt.

### Re: retained-closeout Stop hook — gpt-5 · 2026-10-19T18:26-04:00

Fixed by the delegated subagent, integrated as `0d9dd855c7`, and published in v0.35.525. The final closeout boundary now distinguishes an owned active `closeout_owner_retry_at` lease from an unowned error: it performs a bounded wait for the exact controller state edge and then resumes/reinspects. A durable missing response still defers to its retained owner; unrelated or unowned contention still fails closed. The reported retained cycle reached `retained_captured_projection_settled` and `captured_finalize_resume_committed`, so its captured response was not replayed.

### Re: sdk-demo read-only and extraction — gpt-5 · 2026-10-19T18:26-04:00

The stale read-only wedge is fixed in `a3607e6061` and published. Live JetBrains was dynamically upgraded to 0.2.720; both `sdk-dev.md` and `sdk-demo.md` now report fresh, attached editor replicas with no lag, so the wedged state is no longer needed for diagnostics.

The preserved transfer itself is not claimed as complete. Coordinated extraction now resumes both tabs safely, but the live-authority commit returned `projection_changed` after the nominal apply path. I stopped retrying, did not force disk or repair either live document, and added `#extract-coordinated-projection-changed` as the focused follow-up to prove exactly-once convergence.

### Re: recovered response

Fixed and published in [v0.35.525](https://github.com/btakita/agent-doc/releases/tag/v0.35.525) at **2026-10-19 22:23:24 UTC**; PyPI followed at **22:24:09 UTC**.

- Stop-hook lease contention fix: `0d9dd855c7`
- Release, CI, PyPI, native assets, and editor packages verified green.
- `sdk-demo.md` is unwedgeable/writable with fresh replicas.
- The extraction remains safely unapplied due to `projection_changed`; a focused subagent follow-up is queued.
- `agent:status` now tracks the latest release. Sync is additive by default; `sync-mode="replace"` enables destructive replacement.
- Tag and attribute documentation is in [tags-and-attributes.md](/home/brian/agent-doc-extract-queue-integ/docs/reference/tags-and-attributes.md) and linked from the README.
- Final `agent-doc session-check` passed. The retained response was not replayed.

### Re: Subagent completed: `subagent:extract_projection_fix` returned work for queue item “do [#extract-coordinated-projection-changed]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=1471c33111a78b09188676f2ff8740316af921c3bba0b125323d4ded8d71907b -->
> **Chat prompt (#chatprompt):** subagent: fix: • Blocked by hook
>   └ Session persistence continuation, not a refusal or error: agent-doc Stop hook found active session-document work
> for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md that has not crossed the binary-owned write
> boundary. Active prompt: "do [#extract-coordinated-projection-changed]". Continue THIS turn in-pane and persist with
> `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md` or `agent-doc write
> --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md`, then run `agent-doc session-check /
> home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md`. Do not send the final answer yet.

### Re: coordinated extraction projection fix — gpt-5 · 2026-10-19T19:22-04:00

Fixed, independently reviewed, integrated, installed, and pushed to `origin/main` as `dc727c4054`.

The active Stop-hook message was a correct session continuation, not a new Stop-hook defect: a successor queue head arrived after the predecessor response committed, so the hook kept this turn open rather than misattributing the old response. The Codex wire protocol represents that healthy continuation with `decision: "block"`, which the UI labels “Blocked by hook”; all 119 Stop-hook tests pass.

The actual queued defect was coordinated focused extraction changing disk without advancing controller CRDT canonical. On resume, the old canonical was restored and the next persist returned `projection_changed`. The fix adds a controller/relay detached canonical write guarded by a zero-live-replica fence under the document hub lock, advances canonical before materializing disk, rolls canonical back if disk projection fails, and makes exact replay idempotent. Two-document rollback now uses exact after-to-before canonical bases and fails closed if an editor reattaches.

Independent review approved the change with no actionable findings. Focused relay, realtime, and extraction tests passed, and the integrated authoritative `make check` passed: 11,104 primary tests, editor simulations, VS Code tests, all JetBrains builds, bootstrap checks, and the unchanged TLA proof. `make install` completed; the CLI/native library and editor packages are locally updated, JetBrains dynamically replaced the changed package, and no IDE restart is required.

The live `sdk-demo.md` replica is already healthy and writable with no projection lag. I did not force disk, repair either SDK document, or rerun the preserved live transfer. The latest published release remains v0.35.525; this extraction fix is post-release on `main`, not a newly published release.

The review noted one non-blocking coverage opportunity: fault injection for disk-persist failure and full second-document rollback. Code inspection found the rollback paths correct, so no follow-up item was required for this fix.
> **Queue prompt:** If an issue was added, also add `- release + publish`

### Re: issue-watch release condition — gpt-5 · 2026-10-19T19:25-04:00

No new issue was added in this pass, so no release-and-publish item was added.

### Re: Subagent completed: `subagent:completion_tags` returned work for queue item “Should `agent-doc-subagent-completion` tags be replaced with the response? sdk-demo.md has some lingering `agent-doc-subagent-completion` tags.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=3e549586d3501a37b9f4898a4f720c4429601be9a51f9f5f86d0d1744ac6292a -->

### Re: Subagent completed: `subagent:waypost_request_id` returned work for queue item “subagent operator request should have its own id: instead of [agent-doc]. Remove ` (#waypostrequest)`. Make the Editor Plugin syntax highlighting color code the id tag the same as in todo + backlog + queue + icebox + review.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=94268ce796cc43ac3b3334b220047511093d691c41f8f34501dd4cebc6a9c196 -->
> **Queue prompt:** If an issue was added, also add `- release + publish`

### Re: issue-watch release condition — gpt-5 · 2026-10-19T20:02-04:00

The delegated check confirmed that no issue was added by the preceding watch pass. No `release + publish` follow-up is required, and nothing was published.

> **Queue prompt:** Should `agent-doc-subagent-completion` tags be replaced with the response? sdk-demo.md has some lingering `agent-doc-subagent-completion` tags.

### Re: subagent completion feedback lifecycle — gpt-5 · 2026-10-19T20:02-04:00

Yes. A matching temporary `agent-doc-subagent-completion` cell should be retired when the coordinator's integrated response closes that queue item. It was a lifecycle defect: the claim closed but the cell continued saying integration was pending.

Fixed on `main` in `72bb77cb19` with the corrected regression in `d0b87395c3`. Queue closeout now removes only structurally binary-attested cells matching an actually consumed head; unmatched worker reports and lookalike prose remain. Retained-delivery proof also requires those retired attempt keys to be absent. The full queue-I/O library passes 210/210, and the combined `make check-fast` gate passes.

The live `sdk-demo.md` contains zero completion markers and is healthy. The five observed markers were only in a stale worktree copy, so no live SDK document was mutated.

> **Queue prompt:** subagent operator request should have its own id: instead of [agent-doc]. Remove ` (#waypostrequest)`. Make the Editor Plugin syntax highlighting color code the id tag the same as in todo + backlog + queue + icebox + review.

### Re: subagent operator-request ID and highlighting — gpt-5 · 2026-10-19T20:02-04:00

Implemented on `main` in `eb260b6b7b`. The handback header is now `[#subagent-operator-request] subagent operator request`; the generic `[agent-doc]` prefix and `(#waypostrequest)` suffix are removed.

Shared syntax classifies `[#subagent-operator-request]` as `tracked_id`, so JetBrains and VS Code use the same existing ID color path as todo, backlog, queue, icebox, and review IDs. Runtime/preflight prompts, canonical specs, all harness runbook mirrors, editor specs, and tests were updated. The focused suite passed 34/34, plugin version checks passed, and the combined integration gate passed.

The complete post-release stack is pushed through `d0b87395c3` and installed locally. This remains post-v0.35.525 work on `main`; no new release was published.
> **Chat prompt (#chatprompt):** Is agent:status up to date?

### Re: agent:status freshness — Codex

It was partially stale. The published release number and times were correct, but the managed sync block omitted the JetBrains package versions and still described the extraction blocker as open. I refreshed it with the verified v0.35.525/PyPI timestamps, JetBrains 0.2.720/0.2.721/0.2.722 asset timestamps, and the current backlog/review state.
> **Chat prompt (#chatprompt):** The drift has been around for a while. In a subagent, make status run more often by adding more checkpoints during the turn?

> **Chat prompt (#chatprompt):** implement in a subagent

### Re: periodic status-sync checkpoints — gpt-5 · 2026-10-19T20:46-04:00

Implemented in the requested subagent, integrated, pushed to `origin/main` as `6aa595da2d`, and installed locally.

Open owner turns with `agent:status sync="..."` now get a PostToolUse checkpoint after a full 60 seconds from preflight and then no more than once per true 60-second interval. The checkpoint is additive to ordinary steering, excluded from subagent tool calls, and suppressed after the response cycle is captured or closed. Its generation-keyed next-due edge is atomically claimed; ordinary not-due tool calls only read state and do not churn SQLite.

The new focused command `agent-doc status set FILE --text "<current result>"` safely refreshes only the status component during an open turn. It honors additive or replace sync mode, attributes the landed delta to the agent baseline, and does not capture, seal, append a response, mutate queues, or commit the active response cycle.

The current `agent:status` is up to date. The latest published release remains v0.35.525; this checkpoint implementation is post-release on `main`, so no newer release was published.

Verification passed: the authoritative combined `make check` ran 11,116 Rust tests with zero failures, 202 VS Code tests, cross-editor simulation, JetBrains classic/262/263 builds, Python/bootstrap checks, documentation audit, and the unchanged cached TLA proof. `make install` completed; the CLI help exposes `agent-doc status set`, and repository state is clean.

### Re: recovered response

Yes. agent-doc already installs a `PostToolUse` hook for Codex and Claude Code, which runs after each tool call during an active turn.

The proposed behavior is:

- No `interval` attribute: preflight sync only; no recurring hook reminders.
- `interval="30m"`: PostToolUse checks elapsed time and injects a sync reminder when overdue.
- Normal tool calls before the interval remain silent and token-free.
- An overdue reminder appears at the next tool boundary.

A hook cannot interrupt one continuously running eight-hour tool call; that would require timer-callback support from the harness. But during a normal long turn containing many tool calls, it can refresh status throughout the turn rather than waiting for closeout.
> **Chat prompt (#chatprompt):** The interval should default to off. There should only be the loop interval when there is a value for the interval attribute

> **Chat prompt (#chatprompt):** Perhaps sync can run with hooks? My issue was long running turns that had 1 or even 2 releases would not update agent:status until the very end of the turn. Today I had a turn last >8h.

> **Chat prompt (#chatprompt):** Can agent-doc have hooks that occur during the turn?

> **Chat prompt (#chatprompt):** I'm talking about agent-doc's lazily state. Can there be hooks or some states that can trigger sync?

> **Chat prompt (#chatprompt):** Prepare a handoff document for Claude Code. We are nearly out of tokens.

### Re: Claude Code handoff — gpt-5 · 2026-10-19T22:22-04:00

Prepared the durable handoff at [agent-doc-bugs-claude-handoff-2026-10-19.md](agent-doc-bugs-claude-handoff-2026-10-19.md).

The handoff records the intended Lazily design, exact worktrees and branch state, completed issue #316 commit and closure, the uncommitted interval implementation checkpoint, remaining tests and documentation work, serial integration order, install/push requirements, current release facts, queue claim state, and final agent-doc closeout procedure.

Issue #316 is fixed in pushed commit `188fad6374b1d1cd2f79dac9fc708b4d77a1fcfb` and the issue is closed, but that commit still awaits integration. The interval work remains safely preserved and uncommitted in `/home/brian/worktrees/agent-doc-status-sync-interval-20261019`; its subagent was stopped to avoid concurrent edits during the Claude Code transfer.

The intended behavior is now explicit: `sync` alone remains preflight-only; an explicit `interval` opts into mid-turn checks; the controller-owned Lazily graph derives and consumes the due edge; a harness hook only supplies/delivers safe-boundary observations; SQLite does not arbitrate live cadence. A single uninterrupted tool call still cannot be interrupted, but a long turn containing ordinary tool boundaries can receive the due sync prompt during the turn.

### Re: Subagent completed: `subagent:sync_interval` returned work for queue item “Should we have a `interval=...` attribute on the `agent:status` to preserve tokens?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=96766067717093fe2e973920387c81ecd43b242734cc696b9f1d0b0737d03028 -->
> **Chat prompt (#chatprompt):** continue, updating the handoff, until you run out of tokens.

> **Chat prompt (#chatprompt):** do we still have tokens?

> **Chat prompt (#chatprompt):** Still going?

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/316

> **Queue prompt:** Should we have a `interval=...` attribute on the `agent:status` to preserve tokens?

### Re: Reactive status sync and issue 316 completed — gpt-5 · 2026-10-19T23:04-04:00

Completed and pushed both queue tasks to agent-doc `main` at `9646a2b934a6eb58906394b01ec37b4e89cd2a2d`.

Issue #316 is fixed and closed. The closeout path now accepts an exact ledger-derived late reported-handoff status projection only for `WriteApplied`/`Committed` cycles, while unrelated status edits still fail closed.

Periodic status synchronization now defaults to off. `agent:status sync="..."` continues to supply the instruction at preflight; repeated in-turn reminders require an explicit `interval=...`. Durations accept unitless seconds or compound `s`/`m`/`h`/`d` values with a one-minute minimum. Bare, blank, duplicate, malformed, overflowing, sub-minute, or orphaned status intervals fail closed.

The timing decision is controller-owned Lazily state: keyed Sources retain the cycle/cadence observation and receipt, a Computed derives `status_sync_due`, and the controller atomically consumes the due edge. SQLite live arbitration was removed. Claude Code and Codex `PostToolUse` hooks only transport completed owner-tool boundary observations and deliver the reminder; they do not own cadence or write status. A single continuously running tool call cannot be interrupted, but an overdue reminder is delivered at the next supported boundary. Ordinary steering remains additive.

Verification:

- Focused status-sync tests and targeted Clippy with warnings denied passed.
- Combined `make check` passed: 11,116 Rust tests, 202 VS Code tests, cross-editor simulation, all JetBrains variants, 8 bootstrap tests, docs audit, and cached TLA proof.
- Combined `make tmux-ci` passed, including the previously failing two-IDE lane case.
- `make install` completed once; installed CLI reports `agent-doc 0.35.525`.
- Integration worktree is clean and `origin/main` contains the changes.
- GitHub CI/Book/Pages runs were queued after push; no release was published.

The durable, fully updated transfer record remains at [agent-doc-bugs-claude-handoff-2026-10-19.md](agent-doc-bugs-claude-handoff-2026-10-19.md), committed in the workspace as `a66f1714a1`.

### Re: In-turn hook behavior verified — gpt-5 · 2026-10-19T23:06-04:00

Verified all three pending hook statements against the implementation:

- `agent-doc skill install` installs the unfiltered `steering-post-tool-use` bridge for Claude Code and Codex. It runs after each completed tool call in a tracked active turn; it remains silent when there is no settled steering or due sync checkpoint.
- The implemented behavior is preflight-only `sync` by default, opt-in repeated reminders through `interval`, controller-owned Lazily due derivation and receipt consumption, additive delivery at owner tool boundaries, and agent-driven external verification plus `agent-doc status set` only when the fact changed.
- A hook cannot interrupt one continuously executing tool call. An interval may become due, but delivery waits for the next supported harness boundary. OpenCode, Grok, and Cursor use polling/command boundaries rather than `PostToolUse`.

These statements are now covered by the completed implementation and the green combined `make check` and `make tmux-ci` results recorded in the preceding response.

### Re: v0.35.526 release, and blocking operator requests now land in agent:todo — opus-5.5 · 2026-10-20T00:41-04:00

> **Queue prompt:** release + publish

**v0.35.526 is fully published.**

- **Published:** [GitHub Release](https://github.com/btakita/agent-doc/releases/tag/v0.35.526) at 2026-10-20 04:16:57 UTC with 15 assets; PyPI wheel `agent_doc-0.35.526-py3-none-any.whl` at 04:17:50 UTC.
- **Release driver:** `release summary: tag_publish_handoff=complete local_install_full=complete public_publication=complete`, exit 0.
- **What shipped:** the 10 commits since v0.35.525, including:
  - status sync `interval=` and `agent-doc status set`
  - late queue handoff status at closeout (GH #316)
  - free-text consumption across the queue graph
  - retiring integrated subagent completion feedback
  - the `[#subagent-operator-request]` tracked id
  - `agent-doc patch --id`
  - CRDT authority during focused extract
- **JetBrains:** plugins stay at 0.2.720 / 0.2.721 / 0.2.722 because their sources were byte-identical; the release re-attached them.

Getting there:

- **Stale checkout:** the `src/agent-doc` checkout was 977 commits behind, so I cut the release from a fresh worktree on `origin/main` (`/home/brian/agent-doc-release-0.35.526`).
- **First run:** release preflight stopped on a missing `editors/release-parity.tsv` row for 0.35.526. I added the row.
- **Second run:** `make check` passed, but the release gate said the repository had changed since the check. My `release.log` was an untracked file inside the worktree, and the full-check fingerprint counts untracked files. The third run kept its log outside the repository and went through.

> **Queue prompt:** In fpe.md: "What should happen to the chat messages already stuck in `pending` on STG (including the rows from the 2026-10-20 incident)": This choice should have been added into `agent:todo`.

**Confirmed and fixed. The binary now writes this todo itself.**

What happened in haiven-dev `tasks/fpe.md`:
- **23:00 (`3ad3e23c`):** a closeout presented a blocking `[#subagent-operator-request]` packet (the five STG pending-message choices) and gated `#apipendingorphans`. It passed no `--todo-add`, so `agent:todo` got nothing.
- **23:44:** the todo appeared only after you queued "Add a todo." That cycle then wrongly said the todo "already exists from last cycle."
- **Why nothing caught it:** the rule (`#wayposttodo`) existed only as guidance. Neither the GH #314 advisory nor the review-gate todo advisory covered it.

The fix (worker `subagent:packettodo`) is commit `4dd943bcf7`, fast-forwarded to main and installed locally:
- **Detection:** at closeout, the binary looks for blocking packets by structure: the marker line, then `kind:`, `blocking: true`, and `request:`. It ignores `blocking: false`, fenced, quoted, or indented examples, and the protocol template's placeholder.
- **What it writes:** if a closeout presents one and passes no `--todo-add` (and no `patch:todo`), the binary adds a todo derived from the packet, for example: `Answer the blocking subagent question for #apipendingorphans in agent:exchange: <request> (choices: withdraw, resubmit, release, leave)`. It goes in the same transaction as the rest of the closeout, so a retry or replay does not duplicate it.
- **Which id it uses:** the one id this closeout gated wins. Otherwise it uses the one id the response cites, and otherwise it mints a new id.
- **Ordering change I accepted:** `--todo-*` operations now run after the closeout's backlog adds, ungates, and gates. A todo layered on an item gated in the same closeout now names `agent:review` as its source instead of `agent:backlog`. Manual `--todo-add` ordering changes the same way; the todo operations' order among themselves does not.
- **Tests:**
  - 5 unit tests and 1 wiring test.
  - 4 integration tests: the `3ad3e23c` shape now gets `- [ ] [#apipendingorphans] … (source: agent:review …)` with no duplicate on replay; an explicit `--todo-add` is not duplicated; fenced, quoted, or non-blocking packets are ignored; with no tracked id, a new id is minted.
  - Full `make check` passed: 11126 of 11126 nextest tests, exit 0.
  - CI for `4dd943bcf7` is still in progress.
- **Release:** the fix is not in v0.35.526; it ships in the next release.
- **fpe.md:** already has its todo (`[#apipendingorphans]`, added at 23:44), so it needs no change.

The exchange is large (353 lines across 38 sections). The queue is active, so I am not stopping to compact.

### Re: Subagent completed: `subagent:gh319` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/319” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=207bfa15fe1f6ed63a2289acd96a906277a07a513e3c0933b0291ba8ef806be0 -->

### Re: Subagent completed: `subagent:gh318` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/318” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=789850c6b8adb14e513fa66ea2943da3be235ad14876bed60f0d3e9efe07f1c1 -->

### Re: Subagent completed: `subagent:busycue` returned work for queue item “route: a background orphan-drain dispatch types into a pane right after reading its busy-turn cue (route_dispatch_only_busy_active_turn_skip_wait then tmux_text_enter; from GH 319)” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=62f7da6542426f80d3a0ea794f0b101a4433e8a9487c9c8fa7ef5f09fb381bef -->

### Re: Subagent completed: `subagent:claimvis` returned work for queue item “investigate: preset head upgrade was dispatched 2 s after its claim landed (GH 319 "also seen"; likely claim-visibility timing in the controller state.db read)” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=fb2f46b1b1c141f9c3d04edb8445635d689db23d488ba246d15001abfab58d0d -->
> **Chat prompt (#chatprompt):** is PR #320 included in the release?

> **Chat prompt (#chatprompt):** should we use lazily instead of shelling to the cli to fix this?

> **Chat prompt (#chatprompt):** should we use lazily instead of shelling to the cli to fix #320?

> **Chat prompt (#chatprompt):** I would like state to be propagated...instead of relying on effects that may be incorrect

> **Chat prompt (#chatprompt):** Note that #320 may have new comments

> **Chat prompt (#chatprompt):** Instead of persisting in memory, because I use other agents, persist in the agent-doc spec

> **Chat prompt (#chatprompt):** subagent: there are 3 tmux panes

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/317

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/318

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/319

> **Queue prompt:** [#yvt3]: done

> **Queue prompt:** [#8ph6]: done

### Re: GH #317–#320 batch, v0.35.527 release, state-propagation spec — opus-5.5 · 2026-10-20T02:08-04:00

**#317, #318, #319 are fixed, merged and released in v0.35.527.** Each was done by its own subagent, on its own branch and worktree, and integrated serially on `integrate/gh317-319`. main and the tag `v0.35.527` are pushed (`e60a9e3b1d`). `make check` passed on the version-projected tree: 11139/11139. `install-full` has run: `agent-doc --version` reports 0.35.527, and the JetBrains packages were replaced live with no restart needed. All three issues closed automatically from the `Fixes #N` commits. The release driver is still waiting for the GitHub Release and PyPI uploads to go public.

- **#317 (`d94c397f4b`):** a Run Agent Doc trigger that was typed but never submitted no longer blocks Clear Session Context. When the clear withdraws the trigger and no turn-active lease appears within 1 s, it settles the oldest accepted dispatch that never started (`lease=none reason=trigger_never_submitted`), and the clear runs. A dispatch that did start, a newer queued Run, or a late lease still holds the clear. Not covered: an empty composer, where nothing was withdrawn, and the stale "turn in progress" pane title.
- **#318 (`3a8f232365`):** bolded residual-work lists (`Still stale`, `Also stale`, `Not done`, `Still open`, `Open`, `Remaining`) now count as operator asks. Asks are counted against `--todo-add` items and open todos, so one add no longer silences the advisory. Signals match on word boundaries and skip headings and fenced code, so "reply posted" no longer matches "reply post".
- **#319 (`b3ee177a78`):** the orphan drain checks claims against the head's line text, not its bare id, on both the fresh and memoized paths.
- **Release fixes:** the release needed an editor-parity row for 0.35.527, which I added. The shared `src/tmux-router` checkout is 3 commits behind (0.3.21) and has local edits that overlap upstream. Building against it would have downgraded the lockfile, so the release worktree uses its own sibling tmux-router at origin/main (0.3.22). The shared checkout was not touched.
- **Follow-up from #319: the background drain no longer types into a running turn** (`479418fe3c`, branch `fix/busycue-20261020`). Every dispatch now records whether it came from the operator or from a background drain. A background dispatch against a busy or waiting actor defers (`route_automated_dispatch_deferred_live_turn`, exit 75) and retries after backoff. Operator Run Agent Doc steering into a live turn is unchanged.
- **Follow-up from #319: claim timing** (`d9f4aa0031`, branch `fix/claimvis-20261020`). The orphan-drain sweep read claims once per pass. It now re-reads them before taking the dispatch lease and again in the route worker. I could not confirm that this caused the `upgrade` case, because no log from that session exists on this machine.
- **Both follow-ups are merged into `integrate/v0.35.528`,** which is local only. I fixed one clash between them in a test fixture. `make check-fast` passed: 9304/9304. They will be persisted when v0.35.528 ships.
- **#320 is not in v0.35.527.** It is an issue, not a PR, and it was filed at 05:34Z, after that batch was cut. A subagent is fixing it now on `fix/gh320-20261020`, and it will ship immediately as v0.35.528 together with the two follow-ups.

**Lazily instead of the CLI, for #320.** #320 has two faults.

1. **The move is never published.** The automatic capture already uses the controller's Lazily path. But in Remote Dev it stops in Kotlin (`no_visible_session_documents`) when the backend's `selectedFiles` is empty, and the remote client's visible editors are thrown away. The fix is to publish that remote-client state on the Lazily path instead.
2. **The merge keeps the old order** (`RemoteLayoutMemory::ordered_columns`). The Lazily path and the CLI `--col` route both reach this merge, so changing the transport alone does not fix it.

Moving Run and Sync off the `--col` CLI shell-out is a separate, larger migration. I asked for it as a follow-up suggestion rather than part of this release.

**Propagate state, don't infer it from effects.** The #320 subagent has been redirected. The plugin publishes the editor's actual layout: split columns in geometric order, with the visible and selected documents in each. tmux order is a pure derivation of that state, and the tmux effect only applies the result. It no longer reads "editor removed, then reopened" as a reorder. When the real geometry is known, the order is computed from the current state alone, with no carried-over memory. If the Remote Dev backend cannot see the client's geometry, the state is labelled `geometry=unknown` and the spec names that limit and the manual remedy; nothing guesses. If doing this properly needs more than one cycle, the subagent will send a `[#subagent-operator-request]` packet rather than fall back to a heuristic.

**Persisted in the agent-doc spec instead of memory.** I removed the memory note. `specs/13-state-backbone.md` now has a section, "State Propagation, Not Effect Inference" (`54570d2665` on `integrate/v0.35.528`, shipping with v0.35.528):
- consequences are derived from propagated state, and effects only apply them;
- events may trigger a re-publish but are not evidence of state;
- the current state alone determines the result;
- unknown state is labelled, never guessed;
- observed state is never dropped because of a local precondition;
- prefer the reactive path over CLI snapshots.

The Regression Rule gains a third question that checks for this. I also added the #317 entry that was missing from the 0.35.527 section of `VERSIONS.md`.

**#320 comments:** none so far (0 comments; last updated 05:34:38Z). The subagent will re-read the issue before it finalises the design and again before it commits.

**Tmux pane count ("There was 1 tmux pane with 2 editor panes." / "subagent: there are 3 tmux panes"):** dispatched to subagent `panecount` (`fix/panecount-20261020`). At 06:06Z window `0:0` shows 3 panes: `%2` agent-doc-bugs.md, `%44` upgrade.ad.md, `%58` sdk-demo.md. Preflight also reports `session drift: registered panes span 2 tmux sessions: 0, 2`. The subagent works read-only against ops.log, idea.log and tmux, does not touch the live layout, and follows the same state-propagation rule. It coordinates through me if its root cause overlaps #320.

**`[#yvt3]` and `[#8ph6]`:** checked off as done, as you reported.

**Still open:** #320 and the pane count (both running), and "release + publish including PR #317-#320." (v0.35.527 is out; this stays open until v0.35.528 publishes). The exchange is 421 lines, so it is due for compaction once the queue drains.

### Re: GH #320 subagent operator request: scope decision — opus-5.5 · 2026-10-20T02:21-04:00

The #320 subagent stopped with this blocking request (verbatim):

> [#subagent-operator-request] subagent operator request
> kind: question
> blocking: true
> request: For GH #320, which scope should this cycle ship, given that a Remote Dev backend cannot see the client's split geometry, and where it can be read (the JetBrains Client frontend module), it is not forwarded to the controller?
> context: Evidence from the IntelliJ `IU-263.6800.24` distribution in the Gradle cache, disassembled with JDK 25 `javap`. Files are in `/home/brian/work/btakita/agent-doc-wt/gh320/.gh320-scratch/`.
>   1. The backend has no split structure for a remote client. `com.jetbrains.rdserver.fileEditors.GuestFileEditorManager`, which is the backend's `ClientFileEditorManager` for a Remote Dev client, holds only a flat `CopyOnWriteArrayList<EditorComposite> composites` and one `MutableStateFlow<EditorComposite> selectedComposite`. It has no EditorWindow, splitter, or window id.
>   2. The protocol does not carry geometry either. The RD model `com.jetbrains.rd.ide.model.FileEditorCompositeModel` carries `fileId`, `editors`, `openWithFocus`, `visible: Boolean`, `pinned`, and `tabPresentation`. `RdFileEditorCompositeId` is `(fileId, clientId, index)`. No window or split identity or geometry crosses the wire.
>   3. `RdServerEditorTracker`, the source of our `remote_client_visible_editors` collector, is a flat `List<Editor>` of active editors per ClientId. `visible` is membership in open order, not geometry.
>   4. The geometry exists only in the JetBrains Client process. In our 262/263 modular package, `FrontendSurfaceCollector` (frontend module) builds `EditorWindowSnapshot(ordinal, selectedPath, openPaths, visiblePaths)`. The ordinal is a list index, not bounds. The backend `ControllerSurfaceSnapshotWire.snapshotRequest` then flattens every window into a single `visible` list for `editor_view_snapshot_observe`, which serves detached-surface ownership and does not feed the pane-layout Source. The classic 242–261 plugin has no frontend module at all.
>   5. A hazard in the controller that Finding 1 would expose: `automatic_editor_surface_sync_invocation` (`agent-doc-controller-io/src/project_controller/rpc.rs`, around line 4372) publishes every automatic surface layout with `column_order: Default::default()`, which is `Editor`. So once the Remote Dev capture stops deferring, the fold's memory-guessed order would reach tmux claiming to be editor geometry. The surface payload (`EditorSurface`) has no order or geometry field to say otherwise.
>   Current state: nothing is committed. Issue #320 has 0 comments as of this check. I have not yet seen `specs/13-state-backbone.md` (54570d2665, local only); for any spec text I would cite it by section name.
> choices:
> - A, ship now with honest labels and a written limit: (1) Fix Finding 1. On a Remote Dev backend the capture publishes the remote-client visible/selected state on the existing Lazily surface Source instead of deferring on the backend's empty `selectedFiles`. (2) Add a geometry/order label to the surface state: an `EditorSurface.column_order`/`geometry` field, carried through `SurfaceIntent::Sync` into the controller invocation. Local multi-window bounds publish `editor`. Remote Dev publishes `unknown` (or `retained` when the fold memory is anchored), so the controller derives order from its retained layout and never claims an order it cannot know. Older plugins default to `editor`. (3) Spec: Remote Dev pane order cannot follow a split move. The manual remedy is `agent-doc sync --col <left> --col <right>`, an `editor`-order publication that then sticks, because unknown and retained surface publications slot-fold against it. (4) Regression tests: a local `[A|B]` state followed by `[B|A]` re-derives tmux order from that state alone; the Remote Dev case stays at its labelled `unknown` order, and the manual `--col` override persists. No event heuristic. The state-propagation design for the Remote Dev case below goes to a follow-up item. Scope: this cycle.
> - B, A plus real Remote Dev geometry in 262/263: extend the 262/263 frontend collector to capture per-window bounds against the client's splitters (the same column grouping `LayoutDetector.buildColumnsFromSnapshots` uses). Bump the shared `SurfaceSnapshotContract` schema and validator. Forward the ordered columns from the backend into the pane-layout surface Source as `editor` order. The Remote Dev fold memory is used only when no frontend snapshot is present (classic, or a one-sided install), labelled `unknown`. This needs the plugin installed on both client and backend, bumps the 262 and 263 plugin versions, and needs a JetBrains Client to verify live. Scope: more than one cycle; it delays the release.
> - C, Finding 1 only: ship the Remote Dev capture fix without the order label. Not recommended, because it would let the fold's guessed order reach tmux labelled as editor geometry (hazard 5).

**Presented from the main session:** "Which #320 scope should ship?" with the options A: ship now, label unknown (recommended); B: A + real Remote Dev geometry (more than one cycle, delays the release); C: capture fix only (not recommended).

**Operator answer:** **A: ship now, label unknown.** I sent the answer back to the same worker, which is now implementing:
- The Remote Dev visible and selected state is published on the Lazily surface Source, instead of being dropped when the backend's `selectedFiles` is empty.
- The surface state carries an order label: `editor` for local split bounds, `unknown` or `retained` for Remote Dev. The controller never presents remembered order as editor geometry.
- The spec states that Remote Dev pane order cannot follow a split move, and names the manual remedy (`agent-doc sync --col <left> --col <right>`).
- Regression tests check the state-only derivation.

Real Remote Dev geometry (option B) becomes a follow-up queue item. #320 still ships in v0.35.528 once its `make check` passes.
<!-- agent:boundary:7018a289 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/317~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/318~~ — auto-struck: answered this cycle (#ftstrike)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/319~~ — auto-struck: answered this cycle (#ftstrike)
- 🚧 There was 1 tmux pane with 2 editor panes.
- 📥 route: a background orphan-drain dispatch types into a pane right after reading its busy-turn cue (route_dispatch_only_busy_active_turn_skip_wait then tmux_text_enter; from GH 319)
- 📥 investigate: preset head upgrade was dispatched 2 s after its claim landed (GH 319 "also seen"; likely claim-visibility timing in the controller state.db read)
- ~~There is — auto-struck: completed by #adoc-baseline-drift-after-user-commit (#qftbklgstrike)~~
- ~~[#yvt3]: done~~ — auto-struck: answered this cycle (#ftstrike)
- ~~[#8ph6]: done~~ — auto-struck: answered this cycle (#ftstrike)
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/320
- release + publish including PR #317-#320.
<!-- /agent:queue -->

## Queue: Issues
<!-- agent:queue id=issues loop interval=1h -->
- 🚧 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:queue -->

## Todo

<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [x] [#gq2c] After the haiven-dev supervisor recycles onto the new build, re-run Agent Doc on tasks/sdk.md: bracketed todo ids are admitted again (it currently uses bare ids as a workaround).
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#n7kq] In the sdk.md session: restore the 7 todo ids the 23:30 --todo-edit dropped (#tdclireview, #tdcontracts162, #tdpromotioncallers, #tdinspectclones, #tdfpe202, #tdadr0033, #tdadr0030) with --todo-edit "<exact text>=#tdX <same text>", or accept the ids the preflight backfill assigns
- [x] [#8ph6] In the docs.md session: check off the Google Drive re-auth todo (--todo-done httpsdocsgoogle once backfilled, or --todo-done with its exact text)
- [ ] [#t1v0] In the sdk.md pane, press Clear Session Context again: the 06:03 presses were held behind the open cycle and then dropped by the install restart. Check first that sdk.md's queue has not opened its tdue4cpuset cycle.
- [x] [#yvt3] Manually sync tmux now that the one-pane diagnostics are complete
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
