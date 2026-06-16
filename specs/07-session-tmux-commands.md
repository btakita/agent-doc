> Extracted from [07-commands.md](07-commands.md)

# Session And Tmux Commands

This file covers the session-bound command surface: pane ownership, routing, sync/reconcile, stash handling, and tmux session selection.

## start

`agent-doc start <FILE> [--force]`

- Starts the configured harness in the current tmux pane and registers the pane as the session owner.
- Route-owned tmux autostart panes must not automatically restart the child harness after a clean exit. If the newly started child exits before or after surfacing a prompt, `start --route-owned` must surface the local restart/quit prompt instead of immediately spawning another child process.
- Route-owned reap decisions after a committed cycle may use the supervisor actor's stable `ready` state as prompt proof even when the terminal tail still contains transient renderer text. Explicit blocking prompt states, including queued drafts, permission prompts, hook-review prompts, history search, and clean-exit restart prompts, still preserve the pane.
- If another alive pane is already bound to the same document session, normal `start` must fail closed instead of reusing, restarting, or replacing it.
- That failure must print concrete tmux inspection/capture/kill commands so the user can decide which pane to keep and which pane to kill manually.
- `--force` is the only supported escape hatch for intentionally rebinding the current pane during repair work, and the registry rebind must still record supersession provenance in the session log.
- When a fresh start falls back to a new session binding because the configured session is dead, `.agent-doc/config.toml` must be updated to the new live session.
- Harness launches must auto-add writable roots for parent-repo patchback and nested submodule git metadata when needed.

## route

`agent-doc route <FILE> [--pane P] [--debounce MS]`

- Routes a harness-native reopen command into the authoritative pane for the document.
- Unless explicitly disabled with `--debounce 0`, route first waits for both filesystem mtime and the shared editor typing indicator to be idle for the debounce window (default 500ms). If the combined proof does not settle within the bounded wait, route fails before inserting frontmatter, cleaning duplicate answered prompt tails, or submitting pane input.
- Route must ask the project controller for the document's authoritative actor binding before consulting legacy supervisor-backed registry compatibility evidence. `.agent-doc/session-actors.json` is a projection, not an independent ownership input.
- Before route submits a managed or dispatch-only reopen to an actor-owned pane, it must record a controller `dispatch` attempt for the current session id, pane id, generation, and command kind. Stale session, pane, or generation requests fail closed before input is submitted.
- Existing managed reroutes must use supervisor IPC for the reopen path; they must not fall back to typing directly into a live Claude/Codex pane. If the document path disappeared from the child argv but a healthy supervisor PID still maps to the registered pane, that supervisor-owned binding is enough to keep the managed reroute on supervisor IPC even when a direct pane prompt probe cannot classify the visible prompt.
- Actor-backed reroutes may refresh `sessions.json` as a projection of the actor pane, but they must not opportunistically steal another same-file pane or re-register to a heuristic winner while the authoritative actor is healthy.
- Session-log owners, `registry_rebind` successors, and generic same-file process-tree matches are repair/diagnostic signals only; route must fail closed with explicit inspect/claim/kill guidance instead of promoting them back to authority on the normal path.
- Route must fail closed on ambiguity and list concrete follow-up commands instead of guessing.
- Routed dispatch must target an actually idle composer. Drafted user input, queue-only Codex composer states, reverse-i-search, permission prompts, or similar blockers are not safe.
- Codex reroutes always send the bare `agent-doc <FILE>` reopen. Multiline payloads or content-edited payloads are invalid.
- If unresolved prompt-bearing drift exists and the pane is busy, route may attempt one scoped `agent-doc fix <FILE>` pass and then one bounded fresh-restart recovery, but it must still fail closed if no clean dispatch path emerges.
- For live same-document panes with no new prompt-bearing drift, route may focus the pane and return success without sending a duplicate reopen.
- Fresh auto-starts and live reroutes both require a real per-document cycle acknowledgment after dispatch; accepted input alone is not sufficient.
- Route auto-start may not create a duplicate hidden fallback pane just because split/join heuristics failed. If the target session already has an `agent-doc` window but no safe registered anchor, or if `split-window` fails beside the chosen anchor, route must fail closed with tmux cleanup commands instead of creating and stashing a second pane.
- Once route has created a fresh pane for a document, that pane stays authoritative for the reroute. A concurrent geometry-only registry rebind must not hand dispatch back to an older same-session pane and make the fresh pane disposable.
- Route must never transiently register an existing pane to a different file just to probe readiness. If a candidate pane is already bound to another document, reroute fails closed instead of emitting a temporary cross-file `session_superseded` / `session_end origin=registry_rebind`.
- Actor-store binding must fail closed when the target pane has a non-closed record for another document. Cross-document cleanup is a repair-only operation, not part of normal route/start ownership transfer.
- Route progress diagnostics must be UTF-8 safe when trimming captured tmux lines for stderr/status output. Prompt/status lines containing Unicode glyphs such as `…` or `·` must never panic the binary during a live reroute.

### Startup-miss tracking

- When route/startup acknowledgment times out, the binary records `.agent-doc/state/startup-miss/<doc-hash>.json` with pane/session provenance and shows a visible diagnostic in tmux.
- On the next route/start/sync/session-check path, the tool must distinguish between a stale startup-miss marker and a still-stranded owner. A same-pane marker may only be cleared once newer session-log provenance proves a later open run on that same pane. If the document is now registered to a different pane/session, the old marker is stale and must clear from the current file owner's later `session_start` provenance instead of staying tied to the superseded `session_id`.
- Successful cycle acknowledgment clears the startup-miss marker.
- `route --dispatch-only` still uses a one-shot reopen instead of the managed acceptance/cycle-ack path, but for any existing live managed session that one-shot reopen must submit directly through the resolved pane's tmux input path instead of routing back through supervisor IPC or writing raw bytes into the child PTY. Editor callers add `--plain-trigger` and a longer `--wait-for-ready` budget, which makes the one-shot reopen the plain `agent-doc <FILE>` prompt even when the document's harness normally uses slash commands while allowing slow supervisor startup to settle. That live-pane submit must use the same centralized tmux submit profile as file-scoped `session clear`: Codex, Claude, OpenCode, and default harnesses send hex-encoded text plus a carriage-return byte in one tmux `send-keys -H` operation. It must reuse the same bounded repair/restart checks before injecting into an existing pane, and it must honor the authoritative actor readiness guard: prompt-bearing reroutes may not inject into `starting` or dispatch-only `busy` actors until the current generation reaches a dispatch-ready prompt (`prompt_ready=true`) and refreshes to `ready`. A tracked Codex `/clear` or missing managed capability proof may still force a fresh restart on the managed non-dispatch route, but dispatch-only editor reroutes must keep the plain reopen on the authoritative live-session boundary once that readiness guard passes.
- Every direct-pane submit must emit redacted `tmux_input_event` diagnostics for the payload/key transform and a `route_submit_observation` after route's capture/proof checks. The observation records pane id, harness, phase, elapsed time, whether the trigger was still visible in captured tmux content, and capture length/hash; it never logs the prompt text. If capture still shows the trigger after the acceptance window, route also logs `route_submit_issue issue=prompt_not_submitted`. If the trigger leaves the input surface but no dispatch-start proof appears where required, route logs `route_submit_issue issue=accepted_without_dispatch_start_proof`. For forensic recovery, route must also write redacted terminal snapshots under `.agent-doc/logs/route-submit/` and log `route_pane_snapshot ... snapshot_path=...` when it detects a pre-existing drafted trigger, a still-visible trigger after submit/resubmit polling, or accepted pane delivery without required dispatch-start proof.
- When Codex hook tracking is visible for a dispatch-only reroute, a live-pane or supervisor-backed bare reopen is not allowed to return silent success on acceptance alone. If the command leaves the input surface but Codex never records a routed submission proof within the bounded wait, route must fail once with a stage-specific "accepted but unproven" error instead of looking non-responsive, and the `route_submit_issue` line must make that condition visible to log review. This proof gate applies after any dispatch-only submit helper, including ready authoritative actor reroutes that do not enter the startup prompt gate.
- When a prompt-bearing `route --dispatch-only` targets an authoritative actor pane whose runtime still reports `starting`, route must wait one bounded readiness window for that same actor generation to report `ready` with dispatch-ready prompt proof, then fail closed with a single wait/retry diagnostic if it remains unready. Repeated reroutes for the same pane and generation must reuse that saved timeout state and may log coalesced wait attempts, but must not emit another `route_authoritative_actor_starting_not_ready` timeout until the actor generation or pane changes, or the same generation clears through `ready`, `closed`, or `blocked`. If a route-owned `starting_actor_timeout` block later shows a dispatch-ready prompt for the same pane and generation, route must clear the saved timeout, promote the actor to `ready`, and continue through the normal prompt-gated submit path. No supervisor IPC queue, direct-pane submit, split churn, pane kill, or same-file ownership re-election may occur while the actor is still `starting`.
- When the controller record and healthy supervisor runtime still report an authoritative actor as `starting`, route may promote the controller lifecycle to `ready` only after the resolved actor pane for the current generation shows a harness-specific dispatch-ready prompt (`prompt_ready=true`). After that promotion, dispatch-only uses the same direct-pane submit boundary as other ready authoritative actor reroutes. Before that promotion, the only user-facing recovery is to wait for the prompt and rerun `agent-doc <FILE>`, or restart the stuck owner with `agent-doc start <FILE>`. Timeout diagnostics must be typed and include elapsed time, pane id, generation, actor state, supervisor health/runtime state, prompt-ready status, and last lifecycle transition.
- When a prompt-bearing `route --dispatch-only` targets an authoritative actor pane whose runtime reports `waiting_input` because the supervisor is sitting at its clean-exit restart prompt, route must request one fresh supervisor restart and then resume the same bare reopen flow instead of fail-closing without a submit. `Run Agent Doc` should not strand the pane at `Press Enter to restart...` with no routed `agent-doc <FILE>` follow-up.
- When `route --dispatch-only` has a live authoritative actor pane but the supervisor socket no longer reports a healthy runtime or actor state, route must keep that pane as the recovery target when it still matches the current registered/live owner binding. It logs `route_dispatch_only_authoritative_degraded_direct_pane` with supervisor health and runtime actor state, records the controller dispatch attempt, and then uses the normal direct-pane readiness, blocker, and proof checks before submitting. It must still fail closed if the pane is not dispatch-ready, or if hook-backed Codex / pane-state-backed OpenCode delivery remains accepted-only instead of producing dispatch-start proof.
- Editor `Run Agent Doc` callers may retry the route command only for the authoritative-actor startup guard. Dispatch-only latest-run boot timeouts such as `latest run is still booting ... (timed_out)` already spent the binary ready-wait window and must surface the persisted route diagnostic immediately instead of silently re-waiting the full ready timeout per attempt. Active-turn busy blockers must notify immediately as still-running, and protected-input blockers such as shell history search must remain persistent failures.
- The explicit editor `--wait-for-ready` budget applies to every pre-submit dispatch-only readiness wait that is trying to prove a starting/latest-run pane has become dispatch-ready, including the session-log `latest run is still booting` probe. It must not fall back to the short Codex dispatch-only default while the user-visible JetBrains command asked for a longer startup-readiness window.
- If a prompt-bearing dispatch-only reroute reaches a live pane that reports a plain active Codex or OpenCode turn before input is submitted, route must queue that rerun in plain `agent:queue`, sync the snapshot, strip any legacy `auto` from the touched queue tag, and print a `queued pending dispatch ... in active agent:queue` diagnostic instead of surfacing a persistent editor route failure. This queue path is only for active-turn blockers; hook review prompts, queued drafts, permission prompts, shell history search, and clean-exit restart prompts still fail closed with their specific recovery action.
- The supervisor idle-queue watch owns the later drain payload for that queued dispatch. Claude/OpenCode drains inject the normal slash-command harness reopen, and Codex drains inject the bare `agent-doc <FILE>` reopen so editor-triggered queue continuation goes through the same harness-native entrypoint as a manual Codex `Run Agent Doc`.
- Drain ownership is single-owner at any instant (#kp5z / #qflood). When a self-driving harness loop is the active drain owner, the supervisor idle-queue watch must defer instead of also injecting `agent-doc <FILE>`, or the two owners flood the live harness input queue with duplicate triggers. The tie-break is a short-TTL drain-owner lease (`.agent-doc/drain-owner/<pathhash>.json`, owner `claude_loop`): the Claude Code `/loop` auto-loop refreshes it via `agent-doc drain-claim <FILE>` just before re-invoking `/loop`; while the lease is fresh the watch returns `SkipSelfDrivingLoopOwner` and logs `idle_queue_drain_decision decision=SkipSelfDrivingLoopOwner owner=… lease_age=…`. A stale or absent lease hands ownership back to the supervisor, so non-`/loop` harnesses (e.g. the Codex `Stop`-hook loop, which never writes a lease) keep getting supervisor drive. The TTL is short so a crashed loop returns ownership quickly; the lease self-expires and never needs manual release.
- Route queue enqueue must be resilient to an unparseable existing `agent:queue` body. When an earlier corruption has merged free-text / log-dump content into the queue component (`#jb-run-agent-doc-response-queue-contamination`), `queue::parse` rejects it, but the route must not propagate that as a fatal `route queue dispatch: failed to parse existing agent:queue` error that bricks every reroute for the document. Instead it preserves the existing body verbatim, appends the new pending dispatch as a well-formed entry beneath it (idempotently — a dispatch already present in the raw body is not duplicated), and records a `route_queue_dispatch_unparseable_preserved` ops-log diagnostic. The queue corruption is left for separate repair rather than silently dropped.
- When any busy-pane dispatch-only path finds the pane still busy after auto-fix, route must attempt interrupt recovery for OpenCode as well as Codex. For OpenCode, the interrupt sequence sends Escape twice and then waits for an idle prompt. This handles the post-`Clear Session Context` scenario where the agent-doc child process is still running and OpenCode is showing a progress bar.
- File-scoped `agent-doc session clear <FILE>` must resolve a direct-pane submit target in the same order that routed reopen prefers an existing live session binding: authoritative actor pane first, then a current live-owner pane for the same file, then the registry pane. Only when none of those panes is directly addressable on the default tmux server may clear fall back to supervisor IPC inject.
- File-scoped `agent-doc session clear <FILE>` must not run the starting-actor readiness guard, but its operator guard is owned by FlowCore `operator_clear`: clear may submit `/clear` after an idle prompt, clean-exit proof, no live pane evidence, or an otherwise unclassified busy projection with no protected input and no explicit busy cue. A live `agent-doc`/harness wrapper process is not blocking evidence by itself. Protected prompt input such as a permission prompt, queued draft, shell history search, or drafted user text remains a distinct refusal reason; explicit busy cues such as an active Codex turn, hook-review prompt, or help/usage screen fail closed with typed UX data and a pointer to `agent-doc session interrupt-clear <FILE>`. Operators use `interrupt-clear` for intentional destructive discard.
- After a successful clear delivery, `agent-doc session clear <FILE>` must reclaim any orphaned open preflight cycle the cleared turn left behind so a following `Run Agent Doc` is not wedged by a stale open cycle (`#clear-stops-running-process`). It reuses the same reclaim semantics as explicit run-cancel (`#cancel-orphans-preflight-cycle`): an empty `preflight_started` cycle is abandoned, while a cycle that already captured a response is protected and left intact. The reclaim is best-effort — a failure is logged as a warning and never fails the clear, which already delivered — and records a `session_clear_cycle_reclaim` ops-log diagnostic with the reclaim outcome.
- File-scoped `agent-doc session clear <FILE>` and `route --dispatch-only <FILE>` must each record their delivery branch in ops-log, including whether the command crossed the live pane's tmux submit boundary or supervisor IPC inject. Dispatch-only route logs must also record both `proof` and `proof_scope`: attempts without dispatch-start proof are `proof=accepted proof_scope=accepted_only`; Codex hook proof may record `proof=consumed` or `proof=submitted`, and OpenCode pane-state proof records `proof=pane_state_changed`, each with `proof_scope=dispatch_start`.
- Dispatch-only OpenCode and hook-backed Codex reroutes must not report routed delivery as successful on pane-input acceptance alone. OpenCode proof requires the routed trigger to leave the composer and the pane to leave idle chrome within the OpenCode redraw budget. When proof remains `accepted` / `accepted_only`, route records `route_dispatch_only_submit_unproven` and fails closed; only dispatch-start proof may be described as successful delivery. Claude Code dispatch-only routes may still use accepted-only delivery because no dispatch-start hook or pane-state proof is currently available for that harness.
- Direct-pane reroute telemetry must keep pane-input acceptance latency distinct from harness dispatch-start proof. A direct submit whose pane-input disappearance is not observed within the tmux/control-mode acceptance window must not be logged as a submit timeout when later proof shows the routed prompt was consumed, submitted, or moved the OpenCode pane out of idle state; in that case `route_latency phase=direct_pane_submit` uses `outcome=acceptance_unobserved_dispatch_proven`, while `phase=dispatch_start_proof` carries the dispatch-start proof.
- Supervisor-owned inject coverage must include at least one real socket-backed tmux regression, not only mocked writer tests, so the live IPC listener proves it can hand submitted text off to a real pane through tmux.
- The tmux integration is a hybrid architecture, not a tmux replacement. `tmux-router` owns the common policy: control mode is the lifecycle/event backend, `pipe-pane` is the preferred live output stream when a file/process sink is needed, and tmux `send-keys` remains the visible-pane submit fallback. Managed supervisors own the child PTY and maintain an `alacritty_terminal` screen model for readiness/help/protected-prompt detection, so those checks do not depend on tmux `capture-pane`; owned PTY input is scoped to managed Claude/OpenCode supervisors where byte-exact child input is required and must not become the default layout or pane-management backend.
- If dispatch-only has to inspect a starting-pane reroute, route must spend one bounded recovery window looking for the same actor generation to become `ready` or for a newer same-file startup handoff before it surfaces a `still booting` refusal. A same-file successor pane may be followed; a cross-file rebind must still fail closed. The refusal must happen before tmux or supervisor input is submitted.

### Live-child ack rules

- A live `agent-doc` child or healthy supervisor is not itself proof that the rerouted prompt started a new cycle.
- For Codex reroutes, hook-backed submission proof is preferred before the later cycle-start health check.
- For dispatch-only Codex reroutes with visible hook tracking, accepted tmux delivery without routed submission proof is a terminal accepted-but-unproven error. The command output must not also print an optimistic fallback/success line before that error.
- If dispatch-only route refuses a Codex pane with `route_dispatch_only_blocked reason=codex hook review prompt`, the user-facing error must name the hook-review approval gate and tell the operator to open `/hooks`, approve or disable the pending hook change, wait for the idle composer, and rerun the dispatch-only route or editor Run Agent Doc action. A generic "restore an idle prompt" hint is insufficient for this blocker.
- Claude Code and OpenCode do not currently expose the Codex-style prompt-submit hook. Claude dispatch-only route must therefore label accepted pane delivery as accepted-only instead of implying consumed/submitted proof parity. OpenCode can satisfy the same proof requirement only through pane-state transition proof (`proof=pane_state_changed proof_scope=dispatch_start`).
- Frontmatter-only metadata churn must not count as prompt-bearing work that blocks or requires reroute acknowledgment.

## claim

`agent-doc claim <FILE> [--position left|right|top|bottom] [--window W] [--pane P]`

- Claims a document for a tmux pane that is already running the harness.
- The command must enforce the one-live-pane-per-document binding invariant. If the requested pane already belongs to another alive document session, `claim` provisions a new pane instead of commandeering the old one.
- The binding invariant is enforced **across project/submodule roots**, not just against the calling root's `sessions.json`. The calling root's registry only records documents rooted under it, so a pane owned by a document rooted in another project/submodule (for example a submodule `agent-doc start --route-owned <other>.md` Codex session) would not appear as a registry conflict. `claim` must therefore also inspect the requested pane's live process tree: if it runs an agent-doc/codex owner session for a different document, `claim` provisions a new pane instead of commandeering it. Without this, a new document claimed from inside another document's live pane aliases onto that pane (registries can even disagree on the new document's session id) and no real pane for the new document ever appears.
- Cross-session claim is invalid unless the configured project session is stale or the user forced the claim explicitly.
- On a cross-session **Reject**, `claim` emits a stable machine-readable marker line to stderr **before** the human bail text so editor plugins (JetBrains "Claim for Tmux Pane", VS Code claim command) can branch on it and render a choice dialog (Force claim / Switch project session / Cancel) instead of surfacing the raw exit-1 message. The marker shape is `[claim] cross-session-reject pane_id=<id> pane_session=<session> configured=<session>` (`CROSS_SESSION_REJECT_MARKER` + stable `pane_id`/`pane_session`/`configured` field order, formatted by `cross_session_reject_marker`). The human bail message (`pane <id> is in tmux session '<session>' but project session is '<session>'; switch to the configured session or pass --force`) is preserved unchanged for terminal use. The JetBrains (`ClaimAction.parseCrossSessionReject`) and VS Code (`crossSession.parseCrossSessionReject`) plugins parse this marker from the merged claim output and present a **Force Claim / Switch Project Session / Cancel** dialog: Force Claim re-runs `claim --force`; Switch Project Session runs `session set <pane_session>` then re-claims; Cancel leaves the file unclaimed. Behavioral verification of the live dialog is gated on a live IntelliJ/VS Code session.
- Pane validation runs **before** any file mutation (`#claim-validate-before-scaffold`). `claim` resolves the pane and applies the cross-session guard before the empty-file auto-scaffold writes/commits the document. A rejected cross-session claim must leave the target file byte-identical (no scaffold, no `agent-doc(<doc>)` commit) — earlier the auto-scaffold ran first, so an empty `.md` opened in a pane on the wrong tmux session was scaffolded and committed and *then* the claim failed with "pane %N is in tmux session '1' but project session is '0'", converting a file the operator never opted into.
- For new template documents, `claim` scaffolds the default `status` and `exchange` components and saves a baseline snapshot with empty exchange content.

## focus

`agent-doc focus <FILE> [--pane P] [--blocking|--synchronous]` focuses the pane that currently owns the document session.

- When `.agent-doc/session-actors.json` has a live local actor projection for
  the document session, focus must select that actor-owned pane even if
  `sessions.json` still points at an older projection.
- Focus must not launch or wait on the project controller actor-binding RPC; it
  is the editor fast path and must stay independent of slower reconciliation.
- Focus may use `sessions.json` only as a fallback binding helper when no live
  local actor projection exists.
- Live-owner precedence: a pane resolved from the local actor projection or the
  `sessions.json` registry only proves the pane is alive, not that it still owns
  the document. After a reroute / fresh-restart the session can move to a new
  pane while the old pane stays alive with a dead owner, which previously left
  editor navigation focusing the stale pane (it appeared to "not swap"). Before
  selecting, focus reconciles the candidate against the document's provable live
  owner (`sync::find_live_owner_pane`); when a different pane provably owns the
  document right now, focus selects that live owner and lets resync repair the
  registry. The owner-resolution stays quiet (no per-hit stderr) on the navigation
  fast path, and the happy path is unchanged because the resolver returns the
  candidate when it is already the owner.
- Cross-document owner guard (`#jb-tsift-pane-sync`): both owner-resolution
  paths must never surface or reuse a pane that is actually running a *different*
  document's agent-doc/codex session — the normal navigation / sync / autostart
  path (`sync::find_normal_path_owner_pane*`, used by route/start/sync) **and**
  the heuristic recovery resolver (`sync::find_live_owner_pane*`) that `focus.rs`
  navigation and resync recovery use. The focus path was the operator's exact
  repro: navigating from `agent-doc-bugs2.md` to `tsift.md` resolved the owner
  through `find_live_owner_pane_quiet`, which is *not*
  `find_normal_path_owner_pane*`, so the normal-path guard alone left the
  contaminating pane reachable. Stale registry provenance or a process-tree match
  could otherwise return the currently-visible pane (for example one owning
  `agent-doc-bugs2.md`) as the owner for the navigated file (for example
  `tsift.md`), binding two documents to one pane. Each resolver rejects a
  candidate when `sync::pane_runs_other_document_owner` proves it owns another
  document, returns `None`, and the caller cold-starts a correct owner for the
  navigated file. A candidate that already owns the navigated file, or a bare
  non-owner pane, is preserved (`cmdline_owns_other_document`), so the happy path
  and legitimate reuse are unchanged. This is the focus/sync sibling of the
  `claim` one-live-pane-per-document guard.
- Focus must also recover instead of failing closed when the registered pane is
  dead, or no registry entry exists, but the document is still served by a live
  owner in another pane.
- Default focus defers stash promotion (`#jb-nav-3pane-promote-swap`): `agent-doc
  focus <FILE>` selects the resolved pane when it is already visible, but skips
  additive `join-pane` promotion when the pane is parked in a `stash` window.
  Stash reparenting is left to the debounced `sync` reconcile that follows
  editor navigation. Without this, the focus-path promote and the reconcile race
  on a 1-in/1-out tab switch: the promote joins the incoming pane while the
  reconcile (operating on a stale snapshot, or unable to stash a busy outgoing
  pane) does not remove the displaced pane, growing the `agent-doc` window to an
  extra pane (the "3 panes for a 2-column editor" symptom). Older callers may
  still pass `--no-stash-promote`, but it is a deprecated compatibility no-op
  because fast no-promotion focus is the default. Keep this aligned in
  `focus.rs`, the editor `buildFocusCommand`, and this spec.
- Blocking focus (`#stash-pane-promote-on-focus`): `agent-doc focus <FILE>
  --blocking` (alias `--synchronous`) preserves the previous standalone focus
  behavior for manual/operator flows that explicitly want to surface a stashed
  pane before the command returns. Once blocking focus has resolved the pane, if
  that pane is parked in a `stash` window it best-effort reparents the pane into
  the session's `agent-doc` window before selection, not merely selecting it in
  place inside the stash. Promotion is single-pane scoped
  (`sync::promote_pane_to_agent_doc_window`): a failed move is logged and focus
  still selects the pane in place. It does not run full stash consolidation.
  Keep this aligned in `focus.rs`, `sync.rs`, and this spec.
- Editor automatic tab-to-pane sync must not use `focus` as a substitute for
  passive `sync --no-autostart`; beyond the single-pane stash promotion above,
  focus does not own full stash consolidation, safe passive replacement, or
  preserved-layout reselect proof.

## layout

`agent-doc layout <FILE>... [--split h|v] [--window W]`

- Mirrors editor split layout in tmux by rejoining the wanted panes into a target window and preserving non-session panes.

## resync

`agent-doc resync [FILE] [--fix]`

- Prunes dead panes from the registry, reaps idle stash windows, and reports orphaned windows.
- In scoped mode, it must limit repair to the target document and fail closed if ownership is still ambiguous.
- `--fix` may deregister wrong-process panes, move wrong-window panes into stash, and either kill or relocate wrong-session panes, but only when no stronger live-owner proof keeps the current pane authoritative.
- Stash cleanup must preserve recoverable agent panes that still prove ownership of a registered document or still host a live supervisor socket.

## fix

`agent-doc fix [FILE] [--session <name>]`

- Canonical repair surface; document-scoped form shares the `resync --fix` implementation but limits mutations to the target document.
- When a `FILE` is given, `fix` first finishes any unfinished agent-doc turn on that document (`#jb-fix-document-finish-turn`): it runs the deterministic repair path until `session-check` is clean or no further progress is made — recovery can need a second pass (commit a stranded response, then persist the reap) — and only then reconciles tmux routing. Best-effort: a document that stays interrupted falls through to the routing fix with a warning rather than aborting. The JB `Fix Document` action wraps this command, so it finishes the turn without leaving the editor.

## sync

`agent-doc sync [--col <FILES>,...] [--window W] [--focus FILE] [--no-autostart] [--exact-visible]`

- Declaratively mirrors editor layout into tmux columns.
- Files with session ids are managed even when their current registry entry was pruned; `claim` is the only command that creates a new session id.
- Sync must synthesize a per-run tmux-router registry from each visible file's own nearest `.agent-doc` root instead of forcing all files through the caller's current root.
- When `.agent-doc/session-actors.json` has a live authoritative record for a
  visible document, sync must treat that actor-owned pane as the owner-of-record
  and refresh `sessions.json` only as a projection of that binding.
- An alive pane is not reusable solely because the pane id exists; normal sync may reuse the live authoritative actor pane returned by the controller, then supervisor-backed registry compatibility evidence for that specific document.
- When ownership falls back to legacy associated-pane evidence (`session-log`, `registry_rebind`, generic same-file process tree), sync must fail closed and require explicit claim/repair instead of choosing a winner automatically.
- When ownership proof weakens but the alive pane still contains protected Codex drafted input or still appears as the newest open pane in the session log, sync must fail closed for that file instead of fabricating `registered_pane_missing`.
- If two visible files point at the same pane, sync must either find one decisive owner or drop the duplicate from the synthetic registry so tmux-router cannot alias both files onto one pane.
- Once a live pane is reserved for one file during the pass, later files in the same pass must treat it as unavailable.
- The auto-start pre-sync pass must make at most one pane decision per document. The same document may appear in more than one requested column (column memory, focus + column overlap, repeated layout requests), but its candidate set is deduped by path (first-seen order) before resolution. Without this, a document requested twice can cold-start a second pane on its second occurrence — before the first freshly-provisioned pane has recorded a registry / session-log binding that the second occurrence's lookup or `find_associated_panes` could see — producing the duplicate-editor-pane regression ("3 tmux panes with 2 editor panes"). This complements `find_associated_panes` (which dedups against already-discoverable live panes) by closing the same-run, not-yet-discoverable window.
- If a registered pane is stashed, sync must rescue it back into the visible `agent-doc` window rather than treating the stash copy as disposable.
- Protected outgoing on a 1-in/1-out reconcile preserves layout (`#jb-nav-3pane-promote-swap`): when the reconciler's SWAP fast path detects exactly one pane to attach and one to detach, and the outgoing pane is protected (busy / `protect_pane`), it must NOT fall through to ATTACH (join the incoming) while DETACH skips the protected outgoing pane — that grows the window to N+1 panes. Instead it preserves the current layout, leaves the incoming pane in its stash, and defers; the caller's deferred-retry resurfaces it once the busy pane frees. Keep this aligned in the `tmux-router` reconciler (`sync.rs`) and this spec.
- Manual/full sync is also the operator repair surface behind editor
  `Sync Tmux Layout`: before reconciliation it runs `repair_layout` for the
  inferred target session, so the visible layout is repaired to `0:agent-doc`,
  `1:stash`, and adjacent overflow stash windows before panes are reconciled.
- Passive `sync --no-autostart` keeps layout repair explicit and does not run
  `repair_layout`; automatic editor sync should not rename/move stash windows
  while it is only trying to follow a selection event.
- Because opening/selecting a document fires only the passive `sync
  --no-autostart` path, a freshly-opened document whose tmux pane is not already
  up is **not** autoloaded (by design — the passive path avoids attach-first
  pane growth). The editor exposes an explicit on-demand loader, `Load Tmux
  Window` (JB action `AgentDoc.LoadTmuxWindow`), which runs the autostart
  layout-sync path (`sync` with autostart, i.e. without `--no-autostart`) for
  the focused document so the pane is created and the harness started via the
  normal `route` auto-start. The action must not invoke `agent-doc start`
  directly (that runs the harness as a blocking restart-loop child unsuitable
  for a plugin thread).
- If an editor supplies `--window W`, `W` must already be an `agent-doc` window for the target tmux session. When the named session has no visible `agent-doc` window, normal sync must fail closed and preserve layout instead of reconciling remembered docs onto an arbitrary non-`agent-doc` window.
- Post-sync registry updates must fail closed if tmux-router reports a
  geometry-only pane assignment that disagrees with a still-live authoritative
  actor pane for that document.
- When a missing pane coincides with an open `preflight_started`,
  `response_captured`, or `write_applied` cycle, normal sync must fail closed
  and preserve the current tmux layout until an explicit repair surface closes
  that state. `agent-doc repair <FILE>` and `agent-doc session doctor <FILE>
  --repair` own that recovery boundary.
- When tmux-router DETACH would evict an unwanted pane whose owning document
  still has an open `preflight_started`, `response_captured`, or
  `write_applied` cycle, sync must log the open-cycle state and allow
  non-destructive stashing. The pane and closeout process must stay alive, but
  the visible editor projection should not grow solely because another document
  is mid-closeout.
- If a requested projection is missing from the visible `agent-doc` window and
  a visible pane owns an open closeout cycle, both manual sync and passive
  `--no-autostart` sync must satisfy the requested projection without waiting
  for that closeout owner. Sync may displace unrelated unwanted visible panes
  and may stash the open-cycle pane; blocking a different document's sync is
  not acceptable.
- Test coverage for those pure ownership/cardinality decisions should live in
  deterministic `SimWorld` traces. Keep real tmux coverage only for the
  minimal smoke surface that proves pane/window movement, tmux-router detach,
  and focus selection still work against a live server; duplicate manual vs
  passive ownership variants do not both need default-suite tmux coverage after
  matching simulator traces exist.
- Sync serialization is bounded: a contended `.agent-doc/sync.lock` may delay a
  later editor-triggered sync only up to the lock wait budget, after which the
  later sync logs the contention and continues rather than starving selection,
  ownership proof, or auto-start handling behind an orphaned process. Sync
  latency logs must include the `sync_lock_wait` phase so contention is not
  hidden inside the safe-passive total. Safe-passive contention diagnostics must
  classify redundant editor selection churn with `coalesced=skipped_stale` while
  preserving the retry marker consumed by editor plugins. When the lock is still
  held by stale orphaned `agent-doc sync` processes for the same lock file, sync
  reaps those lock owners and retries acquisition before reporting contention.
- In safe-passive editor mode, `sync --no-autostart --focus <file>` may perform
  a lock-free pre-lock handoff before acquiring the bounded
  `.agent-doc/sync.lock`: select a live local actor projection immediately, or
  try a nonblocking skip-wait pane provision when no local actor record exists.
  If that lock is contended, sync returns the normal retry marker without
  further tmux reconciliation or post-lock focus changes. Once the lock is held
  and the actor binding matches the document session with an alive pane, sync
  selects that pane before prune, ownership proof, or tmux-router
  reconciliation.
- Sync latency diagnostics must name the phase that crossed budget. The
  top-level phases include window resolution, prune, ownership proof,
  tmux-router reconcile, and safe-passive total; prune must also emit subphase
  timings for registry pruning, tmux window/pane metadata fetch, stash-window
  cleanup, stash-pane cleanup, and retained-dead non-stash cleanup. Controller
  actor lookup and projection refresh during ownership proof must be logged as
  separate phases so cold/stale controller cost is not confused with
  tmux-router work.
- A single sync cycle should reuse stable ownership facts it has already
  proven. Repeated checks for the same document/session/pane during
  pre-reconcile ownership proof, synthetic tmux-router registry construction,
  and post-router `sessions.json` projection must share a per-cycle proof cache
  instead of re-querying controller actor bindings or supervisor/live-owner
  heuristics for unchanged selection state.
- Safe-passive editor sync must skip expensive stash-window and stash-pane
  cleanup before tmux-router reconciliation, including the first pass of a
  changed selection. Registry pruning and retained-dead non-stash cleanup still
  run, but `prune_stash_windows` and `prune_stash_panes` are skipped subphases
  so the router can detach extra visible panes before orphaned stash scans spend
  the safe-passive budget.
- Editor plugins may deduplicate repeated automatic selection/layout states, but
  a real markdown selection event must still reach the safe-passive
  reconciliation path even when its visible/focused signature matches the last
  applied state. The immediate `agent-doc focus` fast path cannot create a
  missing actor/supervisor; the follow-up `sync --no-autostart` pass owns that
  safe cold-start so later `session clear`/status commands do not fail with
  `stage=missing_actor` after simply navigating to the document.
- Safe-passive editor sync should not prove whether live unregistered agent
  panes in stash are still owned. Live agent-pane ownership proof and
  kill-or-preserve decisions belong to full sync/repair paths.
- Safe-passive focus-only editor sync must not turn a visible split into a
  one-pane `agent-doc` window. If the editor event provides only the focused
  markdown file, sync must expand that one-column projection from
  `last_layout.json`; when no saved layout exists, it must derive the sibling
  projection from the registered panes already visible in the target
  `agent-doc` window. If the focused file is already in the remembered or
  visible projection, sync must select that column rather than replacing the
  currently active tmux pane; active-pane inference is only for true same-side
  replacements.
- Automatic editor plugins that have captured the full visible markdown
  projection must pass `--exact-visible` with `--no-autostart`. In that mode a
  single provided `--col` is authoritative and must not expand from remembered
  layout state, so switching the editor away from a document like
  `tasks/software/corky.md` cannot reintroduce its stale pane as a sibling.
- Ordinary sync/preflight/finalize recovery paths must never kill a tmux pane. When sync observes a dead pane during missing-pane repair, it may capture diagnostics and keep the dead pane retained for manual inspection, but only explicit repair surfaces such as `fix` / `resync --fix` may escalate to pane-kill cleanup.
- `resync --fix` orphan-agent cleanup must preserve non-stash panes that are registered, live-owner-proven, or supervisor-backed in their pane-local project root even when they are absent from the current project's registry.
- Recent repeated `missing_pane` recoveries, unresolved startup-miss state, or a `registry_rebind` closeout whose recorded successor pane is still alive and rooted to the same document all block passive `--no-autostart` cold-start.
- If any visible file stays blocked under passive `--no-autostart`, sync must preserve the current visible tmux layout and warn instead of reconciling the remaining foreign pane set into a new authoritative layout. This includes the live mixed-root replay shape where `tasks/agent-doc/agent-doc-bugs2.md` shares the visible `agent-doc` window with `src/session-share/tasks/claudescore-3.md`; a blocked sibling file must not let the remaining visible pane set collapse into a new authoritative layout.
- A preserve-layout return that successfully reselects the requested already-visible focus pane must print `[sync] safe_passive_layout_preserved_reselected_focus ...` to command output, not only to `/tmp/agent-doc-sync.log`, so editor plugins can mark that focus handoff as applied.

### Sync-specific invariants

- `provision_pane` is the sync-specific pane-creation path. It chooses split direction by column position and does not block on prompt readiness.
- When sync creates new panes it should prefer splitting in the visible `agent-doc` window, not beside a stash pane when a visible anchor exists.
- Post-sync registration must fail closed if one pane would be mirrored back into the registry for multiple documents.
- Cross-session stash rescue is intentionally non-destructive: if a live stashed pane belongs to another tmux session, preserve it in place and report the mismatch instead of moving or killing it.
- Retained-dead pane cleanup regressions must drive the pane from a confirmed idle shell state before sending the exit command; split-pane shell startup is asynchronous under parallel tmux test load, so verification must fail closed on an unready shell instead of assuming the pane already accepted input.

## repair_layout

`repair_layout` is the low-level primitive behind explicit doctor repair.

- `agent-doc session doctor <FILE> --repair` and other repair-oriented commands
  may call it to consolidate duplicate stash windows, recreate the `agent-doc`
  window from stash when needed, and normalize window indices so `agent-doc`
  remains window `0` with stash windows immediately after it.
- The durable repaired window shape is `0:agent-doc`, `1:stash`, and, only when
  stash panes cannot be joined into the primary stash window, `2:stash`,
  `3:stash`, etc. All `stash-*` aliases are treated as stash windows for
  discovery but must be renamed back to `stash` during repair.
- `agent-doc preflight <FILE>` may also call it for the narrow base-index
  compliance case after the pre-diff layout check reports missing window index
  `0`; preflight rechecks layout afterward so its JSON describes the remaining
  state instead of the pre-repair state.
- Full/manual `agent-doc sync` must call the same file-scoped doctor repair
  path used by `agent-doc session doctor <FILE> --repair` before tmux-router
  reconciliation when a focused/session document is available. This includes
  editor invocations scoped to a currently selected `stash` window: sync repairs
  the session back to `0:agent-doc`, `1:stash`, ... first, then resolves the
  repaired `agent-doc` window for layout reconciliation.
- Passive `agent-doc sync --no-autostart` resolves the target session/window
  without invoking doctor repair; if stash/window drift is detected, sync
  warns and leaves the destructive or heuristic layout repair for an explicit
  repair command.

## session

`agent-doc session` shows the configured project tmux session.

`agent-doc session set <name>` updates config and migrates the `agent-doc` and `stash` windows when possible.

**Superseded-session close.** Once `session set <name>` makes a new session
canonical, the old session is closed and dropped from the model rather than left
spanning two tmux sessions. After the window migration, `set` calls
`resync::close_superseded_session(old)`:

- If tmux already auto-destroyed the old session (no windows remained after the
  migration), it is reported as already closed.
- Otherwise the old session is closed (`tmux kill-session`) **only** when it is a
  pure agent-doc orphan: every remaining window is agent-doc-managed (`agent-doc`,
  `stash`, `stash-*`) **and** no pane runs a live agent process. The registry is
  then pruned of the now-dead panes.
- A session that still holds any unmanaged user window or a live agent is
  preserved (and logged), so superseding the canonical session never destroys
  unrelated work.

**Auto-resync superseded close (`#canonical-session-close-autodetect`).** The same
superseded-session close also runs on the auto-resync drift path, with no
configured canonical session. When preflight detects session drift twice
consecutively (registered panes span more than one tmux session) it runs
`resync --fix`; afterward, if panes still span multiple sessions, it closes the
superseded ones around the **canonical** session. The canonical-determination
rule is the **active agent-doc window session**: `resync::canonical_session_for_document`
resolves the session of the registered pane for the current document that runs a
live agent-doc supervisor. Every other drift session is passed to
`resync::close_superseded_drift_sessions`, which applies the same safe
`close_superseded_session` treatment per session — so a session with a live agent
or an unmanaged user window is still preserved. When no canonical agent-doc
session resolves, all sessions are preserved and the condition is logged. The
step is best effort and never blocks a cycle.

`agent-doc session clear` with no file still clears the configured tmux-session
pin and returns the project to auto-detect mode.

### Actor session operator commands

The same `session` namespace now also exposes the operator-facing
single-owner actor controls:

- `agent-doc session status <FILE>` prints the authoritative actor record,
  registry projection, supervisor runtime state, startup-miss marker, and
  latest session-log summary for the document. It must fetch the actor record,
  supervisor lease, recent operator/dispatch attempt, and projection drift
  diagnostics from the project controller rather than treating
  `session-actors.json` as an authority. It must also query tmux directly and
  classify the resolved pane as `alive-idle`, `alive-busy`, `closed-clean`, or
  `projection-stale`, including the pane current command and recent output tail
  so operators can distinguish an idle live harness from a turn that is still
  running.
- `agent-doc session history <FILE>` prints the actor/session transition
  history from the controller's durable `actor_transitions` store. Legacy
  session-log filtering is only a compatibility fallback when no controller
  transitions exist.
- `agent-doc session debug [FILE] [--human]` dumps the state of **every** actor
  in a project for investigating state drift across documents
  (`#closeout-recovery-state-machine`). It loads the whole actor store via
  `session_actor::load_all_records_in` (the debug API) and, for each record,
  cross-references the document's cycle phase (`cycle_state::load`) and closeout
  recovery classification (`flow::closeout::classify_closeout_recovery_state` +
  `recovery_command`). Output is a clean JSON array on stdout by default (each
  entry is the serialized actor record plus `document_id`, `cycle_phase`,
  `recovery_state`, and `recovery_command`); `--human` prints a per-actor summary
  line instead. With no `FILE`, it scopes to the current working directory's
  project root; with a `FILE`, to that document's project root. Diagnostic-only:
  it never mutates actor, cycle, or document state.
- `agent-doc session attach <FILE> --pane %123` performs an explicit
  authoritative handoff onto the requested pane through the controller,
  creating a new generation and refreshing the registry projection from that
  result. The registry helper used after controller acceptance must be
  projection-only and must not create another actor transition.
- `agent-doc session restart <FILE> [--fresh] [--force]` requests an actor-owned
  supervisor restart through IPC instead of relying on route-side restart
  heuristics. Before contacting the supervisor, it must record a controller
  operator-command acceptance or fail with the rejected stage. If direct tmux
  evidence classifies the resolved pane as `alive-busy`, restart must fail
  closed before mutating the session, except when the pane is visibly at the
  harness clean-exit restart prompt. The refusal must point to `--force` as the
  explicit discard path, and must surface a `busy_proof` field carrying the
  concrete active-turn line (the interrupt/working-spinner cue such as
`Working (Xs · esc to interrupt)`) when one is present, in addition to the raw
pane `tail`. The raw tail alone is the ambiguous composer/permission footer
(e.g. `⏵⏵ bypass permissions on …`), which shows in both idle and busy states
and reads as a false refusal; `busy_proof` makes the busy classification
self-evident. A `starting` actor generation is also a restart guard:
bare restart may proceed only after the pane shows a dispatch-ready prompt or
that clean-exit restart prompt, and the document has not changed after the
last committed response cycle. `--force` bypasses that starting guard and may
interrupt a busy live pane before requesting the supervisor restart. `--fresh`
uses a fresh child launch instead of resume/continue args, but the replacement
child must still re-submit the document's `agent-doc <FILE>` trigger after its
new prompt appears.
- `agent-doc session clear <FILE>` injects the harness-native clear-context
command into the authoritative session through the same canonical
  single-line submit command used by routed reopen and queued slash-command
  dispatch. Claude Code and Codex use `/clear`; OpenCode has no `/clear`
  command, so its clear-context equivalent is `/new` (`session_new`,
  "Create a new session") — submitting `/clear` to OpenCode is a no-op
  (#opencode-clear-uses-new). When the authoritative pane is alive on the default tmux server,
  the command must submit directly through that pane's tmux input path;
  otherwise it may fall back to supervisor IPC delivery. That supervisor
  fallback uses the gate-exempt `Clear` IPC control method rather than the gated
  `Inject` method, so clearing a session whose managed capability proof failed
  succeeds without `kill -9` instead of failing with `prompt dispatch is
  disabled` (`#codex-capability-proof-unrecoverable`). For Codex, it still
  records the clear prompt state so the next reroute can reapply the original
  launch contract. Before contacting tmux or the supervisor, it must record a
  controller operator-command acceptance or fail with the rejected stage. Clear
  is a session-context operation only: it must not rewrite the session markdown,
  save a snapshot, or delete live text below an `agent:boundary` marker.
  is an explicit operator action and must not fail solely because direct tmux
  evidence classifies the resolved pane as `alive-busy` or because the current
  pane command is an agent wrapper; ordinary active/status panes are allowed
  through the same `/clear` submit path. Clear may fail closed when the live pane
  contains protected prompt input such as a permission prompt, queued draft,
  shell search, or drafted user prompt, and may block explicit busy cues such as
  an active Codex turn, hook-review prompt, or help/usage screen. The
  protected-input check must infer the actual harness from the pane's
  `pane_current_command`
  when that command is an unambiguous harness binary (`codex`, `opencode`,
  `claude`, `bun`), falling back to the document's configured harness only when
  the command is ambiguous (`node`, `agent-doc`, shell). This prevents false
  positives when the pane runs a different harness than the document specifies.
  Protected-input and explicit-busy refusals must record their reason in the ops
  log and tell the operator to use
  `agent-doc session interrupt-clear <FILE>` for an explicit discard.
  `interrupt-clear` owns harness-specific interrupt keys, waits for direct
  idle/closed evidence, and then retries the normal clear path. The Codex
  interrupt keys are state-scoped: `C-g` is sent only when the live pane is in a
  shell `reverse-i-search` / history-search state (where it aborts the search),
  because in the normal Codex TUI composer `C-g` opens the external editor
  (`$EDITOR`, e.g. nvim) instead of interrupting; any other Codex state (idle
  composer, active turn) receives only `Escape` + `C-c`
  (`#codex-interrupt-clear-ctrl-g-opens-editor`). The same gating applies to the
  `session restart-supervisor <FILE> --force` busy-pane interrupt and to the
  `route` busy-existing-pane reroute interrupt
  (`attempt_busy_existing_pane_interrupt_recovery`, `#codex-route-busy-ctrl-g-opens-editor`):
  the reroute sends `C-g` only when the pane's authoritative busy `blocker_reason`
  is a shell `reverse-i-search` / history-search, or a fresh whole-capture
  re-classification proves that state (the readiness wait can report a timeout
  with no latched reason, and the search line can sit above trailing blank pane
  rows, out of the last-few-lines window the normal busy classifier inspects);
  any other busy state goes straight to `Escape` + `C-c` and logs
  `route_busy_existing_pane_interrupt_skipped_ctrl_g ... reason=not_shell_search`. If the interrupt
  opens a Vim/Neovim editor prompt in the managed pane, the discard path must
  attempt one forced editor quit before continuing the idle/closed wait; if the
  pane still does not settle, the ops event and error must name the final
  blocking live-pane state, evidence source, prompt-ready value, last observed
  command, and recent pane tail, then give an exact manual recovery action
  instead of leaving the operator with a generic busy timeout.
  `agent-doc session interrupt-clear <FILE> --force` is the explicit
  destructive hatch for a wedged owner that cannot settle: it marks the
  authoritative actor closed when possible, removes the document's sessions.json
  projection under the registry lock, writes the clear cooldown, signals the
  supervisor/child PIDs when known, kills the bound tmux pane when it is still
  alive, removes the supervisor socket, and reclaims any empty orphaned
  preflight cycle. This force path is intentionally separate from ordinary
  `session clear` and normal `interrupt-clear`, and its ops-log summary must
  report which cleanup steps actually ran.
  For Codex panes, a capture
  that shows only Codex status/footer chrome such as the model/cwd/context line,
  with no prompt input or busy cue, is direct idle evidence for operator
  clear/status; Codex idle placeholder prompts such as
  `› Ask Codex to do anything`, `› Explain this codebase`, and safe generated
  placeholders ending in `in @filename`, `for @filename`, or
  `on my current changes` are also prompt-ready evidence when the pane does not
  show an active `Working (... esc to interrupt)` cue. These idle forms must
  override stale actor/supervisor busy projection even though route dispatch
  still requires a real dispatch-ready prompt before injecting a reopen. A
  `closed` actor generation must still accept this explicit clear operator
  command, because closed only blocks duplicate reopen dispatch; it must not
  prevent clearing the live harness context before the next run.
- A `starting` actor generation is not an ordinary active/status pane for
  operator clear. Even after controller acceptance, clear must fail closed
  before direct tmux submit or supervisor inject unless the resolved pane shows
  a dispatch-ready composer and the document still matches the last committed
  cycle hash. A clean-exit restart prompt is not a clear target; operators must
  restart or wait for the composer before clearing.
- Any tmux-bound command submit in this surface (`route --dispatch-only`,
  file-scoped `session clear`, queued slash-command dispatch, supervisor-owned
  reopen inject) must normalize trailing line endings once and use exactly one
  centralized tmux submit profile operation. Codex, Claude, OpenCode, and
  default harnesses send hex-encoded text plus a carriage-return byte in one tmux `send-keys -H` operation. These
  paths must not layer follow-up synthetic submit-key retries on top of the first
  submit unless the bounded visible-draft recovery predicate fires.
- Every tmux-bound input path must produce structured `tmux_input_event`
  diagnostics naming the source, destination, transform, key, byte count, and
  harness when known. Text payloads must be hashed rather than logged raw, so
  route, queue-dispatch, supervisor IPC, auto-trigger, and stdin-forwarding
  regressions can assert the actual delivery path without depending on manual
  pane inspection.
- `agent-doc session doctor <FILE> [--repair]` reports actor/registry/supervisor
  drift plus controller projection and recent command-stage failures in one
  read-only summary, with `--repair` explicitly escalating into the destructive
  repair path before re-checking status.

### Shared session resolution

The session-targeting precedence is shared across start/route/sync/session-aware helpers:

1. Explicit non-empty context session from `sync --window`
2. Live project `.agent-doc/config.toml` `tmux_session`
3. Current tmux session
4. Harness fallback only for start/route when no live tmux session exists

Current tmux session resolution must target the caller's `TMUX_PANE` owner pane
when it is available. Bare `display-message -p "#{session_name}"` may follow
another attached client's selected session and must only be used as a fallback.
Windowless sync must prefer the live project pin over the caller pane's tmux session.

## controller

`agent-doc controller status [--project-root ROOT] [--ensure]` reports the
project-local controller bootstrap as JSON. Without `--ensure`, status is
read-only: it attempts the controller socket and, if inactive, reports the
last persisted bootstrap state from `.agent-doc/controller-state.json` without
launching a process. With `--ensure`, the command runs the lazy
connect-or-launch path before printing status.

Status JSON includes a `control_plane` object that makes the single-process
runtime boundary explicit. It reports `process_model =
project_scoped_single_process`, `external_boundary = controller_ipc`,
`.agent-doc/state.db` as state authority, compatibility projections as
non-authoritative output, and role snapshots for the dispatch actor, store
actor, per-document session actors, supervisor adapters, and projection
workers. Role snapshots include `owned_items` plus category counts for the
durable SQLite families the role owns, including actor records, lifecycle
transitions, dispatch receipts, queue heads, document cycles, projection
diagnostics, admin operations, crash-recovery markers, and layout state. Active
controllers report the live runtime shape; inactive status reports the durable
offline shape from SQLite without launching a process.
Status JSON also includes a `freshness` object with the installed agent-doc
binary identity, the installed inode when available, the controller running
inode proof, and operator guidance. A process is marked stale only when the
running inode and installed inode are both known and differ; unknown inode proof
is reported as `unknown` guidance, never as a blocking failure.
Per-document session status surfaces projection diagnostics with the projection
name, source generation, intended projection hash, retry status, timestamp, and
message so projection lag/failure is inspectable without treating the sidecar as
authoritative.
For active controllers, the session-actor role snapshot reports the live memory
authority (`actor_records`) plus the current map backend and write-through
marker; this distinguishes the in-process actor map from the durable SQLite
store while keeping the store counts visible.

The controller owns project-level bootstrap identity and the live actor
authority used by route/start/sync:

- socket: `.agent-doc/controller.sock`
- launch lock: `.agent-doc/locks/controller-launch.lock`
- bootstrap state: `.agent-doc/controller-state.json`

Lazy startup must connect first, acquire the launch lock only when needed,
verify that the active controller was started from the same agent-doc binary
identity as the caller, recheck the socket after acquiring the lock, and then
launch the detached controller only when needed. If an active controller is
missing that binary identity or reports a different path/version/size/mtime
stamp, the client must shut it down and relaunch before returning a client
connection. This keeps local installs or rebuilds from leaving a stale
controller process that rejects newly-added controller RPCs as unknown
commands. Detached launch must also tolerate the caller's `current_exe()` path
being removed by a local install or rebuild: the client should skip that stale
path and fall back to the invoked command or `agent-doc` on `PATH` before
reporting a launch failure. The persisted bootstrap records `project_root`,
`socket_path`, `launch_mode`, `bootstrap_epoch`, `pid`, and the startup binary
identity.
Controller request and response reads are bounded: a stalled client connection
must close with a timeout diagnostic instead of monopolizing the server, and a
client waiting for a response must fail closed rather than blocking
indefinitely. `status --ensure` may use the connect-or-launch path only as a
readiness check; it must close that stream before issuing the status RPC.
Sync must measure controller actor-binding calls as `controller_actor_lookup`
and any sessions.json projection update as `projection_refresh`; over-budget
entries should point at the controller phase instead of only reporting broad
ownership-proof latency.
Controller actor-binding responses must distinguish `bound` from `not_found`;
an absent actor is a typed no-op for safe-passive focus, while a malformed
controller response is a protocol error with the raw envelope in diagnostics.
Safe-passive pre-lock focus, post-lock focus, and safe-passive document binding
must use the local actor projection when it is live, avoiding a controller RPC
on the editor fast path. If no local actor record exists, the pre-lock focus
path may try nonblocking skip-wait provisioning and must return without waiting
when a startup lock is already held. Document binding logs
`controller_actor_lookup_skipped ... source=local_projection` and records an
immediate `controller_actor_lookup` success sample when that fast path resolves.
If sync must fall back to the controller actor-binding call, that result must be
stored in the per-sync proof cache so later controller actor lookup, ownership
proof, and synthetic registry construction do not pay for the same lookup again.
The broad `window_resolution` bucket covers target session/window resolution
only; pre-lock and post-lock focus are timed separately as
`prelock_actor_focus` and `postlock_actor_focus`.

`agent-doc start` creates owner generations through the controller. `route`
and `sync` read actor bindings through the controller before consulting
supervisor-backed registry compatibility evidence. `session-actors.json`,
tmux-router state, and session logs remain projections or diagnostic inputs.
Destructive or heuristic recovery belongs behind explicit operator repair flows.
