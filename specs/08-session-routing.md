> Extracted from SPEC.md — see [index](../SPEC.md)

# Session Routing

## Registry

The controller registry in `.agent-doc/state.db` maps each canonical document
identity to its owning tmux pane, session UUID, supervisor generation, harness,
and lifecycle state. Registry mutations and actor transitions share one SQLite
transaction; no registry JSON is emitted or read. Multiple documents can still
map to the same pane, and the optional window binding supports window-scoped
`claim` and `layout` routing.

The broader single-owner session-actor contract lives in
[08a-session-actor-contract.md](08a-session-actor-contract.md), and the target
single-process control-plane contract lives in
[08b-single-process-control-plane.md](08b-single-process-control-plane.md). In
The controller registry is the binding authority, while the session log carries
append-only ownership-generation provenance.

## Use Cases

| # | Scenario | Command | What Happens |
|---|---|---|---|
| U1 | First session for a document | `agent-doc start plan.md` | Creates tmux pane, launches Claude, registers pane |
| U2 | Submit from JetBrains plugin | Plugin `Ctrl+Shift+Alt+A` | Calls `agent-doc route <file>` → sends to registered pane |
| U3 | Submit from Claude Code | `/agent-doc plan.md` | Skill invocation — diff, respond, write back |
| U4 | Claim file for current session | `/agent-doc claim plan.md` | Skill delegates to `agent-doc claim` → updates the controller binding |
| U5 | Claim after manual Claude start | `/agent-doc claim plan.md` | Fixes stale pane mapping without restarting |
| U6 | Claim multiple files | `/agent-doc claim a.md` then `/agent-doc claim b.md` | Both files route to same pane |
| U7 | Re-claim after reboot | `/agent-doc claim plan.md` | Overrides old pane mapping (last-call-wins) |
| U8 | Pane dies, plugin submits | Plugin `Ctrl+Shift+Alt+A` | `route` detects dead pane → auto-start cascade |
| U9 | Install skill in new project | `agent-doc skill install` | Writes bundled SKILL.md to `.claude/skills/agent-doc/` |
| U10 | Check skill version after upgrade | `agent-doc skill check` | Reports "up to date" or "outdated" |
| U11 | Permission prompt from plugin | PromptPoller polls `prompt --all` | Shows bottom bar with numbered hotkeys in IDE |
| U12 | Claim notification in session | Skill reads `.agent-doc/claims.log` | Prints claim records, truncates log |
| U13 | Clean up dead pane mappings | `agent-doc resync` | Removes stale controller registry rows |

## Claim Semantics

`claim` binds a document to a **tmux pane**, not a Claude session. The pane is the routing target — `route` sends keystrokes to the pane. Claude sessions come and go (restart, resume), but the pane persists. If Claude restarts on the same pane, routing still works without re-claiming.

`claim --new-pane` explicitly provisions a fresh pane in the project's authoritative tmux session and binds the document there. This path never resolves, replaces, or migrates the current pane/window. Normal claim retains last-call-wins for an unoccupied target, while an occupied target already provisions rather than commandeering it.

Last-call-wins: any `claim` overwrites the previous mapping for that document's session UUID.

**Session identity ownership:** A frontmatter session UUID names one document,
not whichever registry row happens to be visited first. The first durable
`DocumentSessionIdentityObserved` event owns the identity; ordering is the
controller's global event sequence with canonical document key as a
deterministic tie-breaker. Start publishes the observation and projects this
fact before pane lookup:

| Projected claim | Start transition |
| --- | --- |
| Unclaimed | Keep the document UUID and register it |
| Owned by this canonical document | Keep the UUID and reuse/rebind this document's actor |
| Owned by another canonical document | Assign this copied document a fresh UUID through document authority, then continue routing |

Registration independently rejects the third state while holding the registry
lock. This is the race fence: no legacy route may create parallel ownership
after projection. For identities created before the typed event existed, the
current registry owner seeds exactly one compatibility observation; all later
decisions use durable event order. Supervisor recycling and registry pruning
therefore cannot change the owner. Session lookup uses that same owner rule, so
an old duplicate cannot select an arbitrary sibling supervisor while it is
being repaired.

**Canonical same-document claim reuse:** `claim` must judge "is this pane already mine?" by canonical document identity, not by an incidental caller path. The registry key is a canonical file path, not a session UUID. Re-claiming the same live pane for the same document, including submodule-relative `entry.file` shapes, must remain idempotent and must not provision a duplicate pane.

**Cross-session claim guard:** `claim` may bind to a pane in the operator's current tmux session even when the configured project `tmux_session` points at another still-live session. This is a live-session override, not a config rewrite: the configured value remains unchanged, and it is still the fallback when the current session is not the active `agent-doc` session. Other cross-session claims are invalid unless the configured project session is stale or the user explicitly passes `--force`.

**Fresh-start stale-session rebind:** When `start` falls through to a fresh pane, finds that the configured project session is dead, and therefore registers the document in the caller's current live tmux session instead, it must persist that new session back to `.agent-doc/config.toml`. That keeps later `route` / `claim` resolution aligned with the new binding instead of repeatedly targeting the dead session name.

**Prompt-bearing routed admission projection:** When `route` dispatches to an already-running pane and the document already has prompt-bearing user drift on top of a closed cycle, pane-input acceptance is not sufficient proof of success. Route subscribes to the Project Controller's reactive per-document state and requires a newer projected cycle (`preflight_started` or later) before returning success; if the baseline was already `committed`, that means a newer cycle id, not a same-cycle `commit_already_current` mutation. It does not poll cycle sidecars. Without the newer projection it fails closed instead of silently leaving the prompt stranded in the document. This guard keys off document-body prompt drift only; frontmatter-only metadata edits such as `agent: codex` must not require the admission projection.

**Queued rerun preemption (`#jb-run-preempt-autoloop-priority`):** When `route --dispatch-only` cannot inject a prompt-bearing editor rerun because the authoritative actor is busy and it records that rerun in `agent:queue`, the rerun is inserted **ahead of queued active-loop items** so a manual `Run Agent Doc` preempts the loop instead of landing at the tail. Route-owned queue writes must never add `auto`, and must strip a legacy `auto` attribute from any queue tag they touch while preserving other attributes. Those queue writes are editor-visible document mutations: with a JetBrains/VS Code listener active they must converge through component/frontmatter editor IPC (`route_dispatch_queue_writeback ... transport=editor_ipc`) and fail closed if delivery is unproven, instead of writing directly to disk behind the IDE and manufacturing a File Cache Conflict. The priority insert lands after any leading queue directive (preset / start fence) and must not supersede a lone active-loop prompt — preempt preserves the queued item (manual prompt first, then the queued item) rather than replacing it, which avoids the data loss the legacy single-prompt supersede risked. The legacy tail-append + lone-prompt supersede behavior remains available to non-priority (`priority=false`) `enqueue_route_dispatch_prompt` callers and is covered by its own tests.

**Turn-in-progress status on the queued path (`#claude-busy-status-during-active-turn`):** when that busy-actor path queues the prompt and returns success, route must surface a visible turn-in-progress status on the owned pane (tmux `display-message`) naming that the live session is busy and the Run Agent Doc was queued and will run when the current turn finishes — explicitly *not* asking the operator to rerun (the prompt already auto-queued). This is status-surfacing only (no hard block): it closes the gap where the queued path returned silently and the session looked idle while a turn was in flight. Hard busy-lease enforcement remains gated on the `#subagent-blocks-session` scope decision.

**Route-submit projection:** Before route submits text through a managed, dispatch-only, or direct-pane path, it must publish `RouteSubmitStarted` through the project controller with the document hash, pane id, harness, reason, and submit epoch. Successful completion publishes `RouteSubmitSettled`; accepted-without-dispatch-start proof publishes `RouteSubmitBlocked` for the bounded blocked window. Idle-watch, supervisor queue drains, context-reset clears, pending queue context-clear projections, and orphan visible `/clear` / `/new` drafts must consult the route projection instead of `.agent-doc/route-in-flight` or `.agent-doc/route-submit-blocked` marker files.

**Plain Run pass-through:** An explicit editor
`route --dispatch-only --plain-trigger` is real-time steering, not prompt-bearing
work. After target and startup safety checks, route sends the normalized bare
trigger plus one submit key exactly once and returns when tmux accepts that
transport operation. It must not wait for pane-capture acceptance,
dispatch-start proof, closeout recovery, or send additional submit keys. This
rule supersedes the older busy-actor focus-only/refusal behavior for a plain
trigger; prompt-aware routes retain their queueing, proof, and recovery
requirements.

**Authoritative actor dispatch:** When the durable actor store has a healthy authoritative record for the document generation, route must treat that actor pane as the dispatch target and send the bare reopen through supervisor IPC. Harness compatibility at this boundary is checked against the actor store's canonical harness ids (`claude-code`, `codex`, `default`), not raw supervisor binary labels, so a live `claude` supervisor must accept an actor record stored as `claude-code` instead of failing closed on the alias. A cross-harness mismatch is a conflict only when the recorded pane still has a healthy live supervisor and a non-closed actor state; dead panes, closed actors, and unreachable supervisor records are stale authority and must fall through to fresh start/rebind instead of blocking an intentional harness switch such as Codex to Claude. Supervisor-owned command injection must use the same claimed-pane tmux submit path as real operator input instead of writing reopen bytes directly into the child PTY. The IPC payload still carries one canonical single-line submit command, but the actual delivery boundary is the centralized agent-doc tmux submit profile through the authoritative pane so `Run Agent Doc`, `Clear Session Context`, queue-dispatch, and auto-trigger share one real terminal submit method instead of distinct PTY-write semantics. Existing managed-session reroutes must not fall back to direct tmux `send-keys`; they either dispatch through the supervisor socket or fail/recover explicitly. If process argv no longer exposes the document path, a healthy supervisor PID that maps to the registered pane is still a supervisor-owned managed binding; direct pane prompt classification must not downgrade that path to focus-only no-op. The controller `actor_binding` response is typed: `bound` includes the actor record, `not_found` is an explicit absent-binding result, and `ok:true` without data is a controller protocol error rather than a routing fallback. When route does have to type into a non-authoritative fallback pane, the submit contract stays the same: one profile-selected operation, using normalized text plus a named `Enter` key in one tmux `send-keys -t <pane> <text> Enter` operation for Codex, Claude, OpenCode, and default harnesses. JetBrains editor-triggered routes must pass the plugin's durable attempt id into the `agent-doc route` process; route submit observations, route submit issues, route latency, pane snapshots, input-delivery diagnostics, and bounded Enter re-submit proof lines must include that id when present, so a live failure can be correlated from the click ledger to the binary's exact `tmux_text_enter` / `key=Enter` proof without removing tmux-router from pane/session reconciliation. When there is prompt-bearing reroute work and the authoritative actor is still `starting`, route must first wait for the current actor generation to refresh to `ready` with dispatch-ready prompt proof and then fail closed if it remains unready; no managed, dispatch-only, or direct-pane path may inject into a still-starting actor. That fail-closed diagnostic must say to wait and rerun `agent-doc <FILE>`, name `agent-doc start <FILE>` as the stuck-owner recovery, and persist the last observed pane id, generation, actor state, supervisor health/runtime state, prompt-ready status, elapsed wait, and lifecycle transition. A `busy` actor may still accept one supervisor-owned queued reopen for managed reroutes, but dispatch-only reroutes must first observe the current generation return to `ready`; otherwise they fail closed instead of using a direct pane submit during bootstrap or an active turn. Direct pane evidence repairs a stale busy projection (`#snrun`): when a dispatch-only reroute finds the authoritative actor projected `busy` but the live pane proves a dispatch-ready prompt in the current generation, the busy/lease projection is stale — the actor is not actually mid-turn — so route promotes it to `ready` and dispatches via direct pane submit instead of queuing the prompt into `agent:queue`. A `busy` projection without that proven ready prompt is left untouched and still fails closed (queues), consistent with the direct-evidence rule that idle direct evidence repairs stale busy while busy direct evidence stays fail-closed. `waiting_input`, `blocked`, and `closed` remain hard stop states for duplicate reopen injection. A plain dispatch-only reopen with no prompt-bearing work (an editor `Run Agent Doc` reopen) against a `busy` actor classifies as focus-only, but it must fail closed with the busy-not-ready diagnostic after focusing the pane instead of returning focus-only success — otherwise the editor caller reports a routed run even though nothing was submitted, and the operator sees no feedback after the full ready-wait timeout (`#jb-run-agent-doc-command-route-miss`). The narrow exception is an already-startable inactive `agent:queue` head (marker-side `go`/`start`, a start fence, or legacy `auto`): route sets `queue_active: true`, syncs the snapshot, logs `route_dispatch_only_busy_existing_queue_deferred`, and strips legacy `auto` from the touched tag. Unattended continuation after closeout still requires explicit `go`; `queue_active: true` alone is not enough. Plain inactive queues without an explicit start/go trigger stay inert. A second deferral case (`#jb-busy-reopen-auto-drain-when-idle`) covers a document whose queue is **already actively looping**: when there is no inactive head to activate but `queue_continuation::detect` reports a live go-mode continuation (the running `agent:queue` loop is still draining this document), a bare dispatch-only reopen must report **deferred success** (logging `route_dispatch_only_busy_active_auto_loop_deferred`) rather than the busy-not-ready refusal — the operator's `Run Agent Doc` against a self-driving loop catches a brief inter-iteration gap and would otherwise see an error for a session that is in fact making progress; the active loop continues the document, so there is nothing to inject. This deferral only fires on a proven active continuation; a busy actor with a drained/inactive queue still fails closed with the busy refusal. The fail-closed shape logs `route_dispatch_only_authoritative_actor_busy_focus_only_not_dispatched` and carries the same `the authoritative actor is busy did not return to a dispatch-ready prompt` message the editor classifies as a "session still running" notification, surfaced immediately rather than after silent retries (a still-booting `starting` pane stays the only silently-retried shape). The busy ready-wait itself is gated on the live pane (`#jb-run-agent-doc-busy-active-turn-stall`): when a bare dispatch-only reopen with no prompt/queue fallback targets an actor whose pane proves a genuine active turn (a working spinner or `esc to interrupt` busy-proof line), route skips the bounded ready-wait entirely and refuses immediately, logging `route_dispatch_only_busy_active_turn_skip_wait`, because a multi-minute turn cannot reach a dispatch-ready prompt inside that budget and waiting only produces a silent stall before the inevitable refusal. The refusal is then worded as a busy active turn (naming the cue) instead of the misleading cold-start "after waiting Ns" phrasing. A `busy` projection *without* a live active-turn cue (transient or stale) still serves the bounded ready-wait so a turn about to finish is still picked up. After any IPC or auto-trigger dispatch marks the actor `busy`, the next visible idle harness prompt for that same child must drive the supervisor actor state back to `ready`; this ready transition cannot be limited to the first prompt observed after process spawn. In that state, route may refresh the transactional controller registry to match the actor pane, but it must not re-elect ownership from live-owner heuristics, same-file associated panes, or opportunistic registry rebinds.

**Foreground editor-route layout boundary:** `Run Agent Doc` carries the
editor's complete `--col` / `--focus` projection. Before publication, the
controller canonicalizes editor-relative paths. If focus is absent from the
columns, it may materialize focus only into one unique explicit empty-column
placeholder. The controller then publishes exact-visible desired state and
waits for tmux convergence before dispatching. A missing layout, zero or
multiple candidate empty slots, or a routed document outside the converged
columns fails closed before desired-state publication or dispatch.

Claude artifact UI must be distinguished by stable shape rather than session-owned text. A bare `⧉ <label>` chip is an attachment on an otherwise idle composer and is skipped while locating the real dispatch-ready prompt; the label is arbitrary. The active picker is blocked only when `Enter to open` and a `claude.ai/code/artifact/...` URL are both visible.

**Owned ready/busy conflict:** When a managed owned pane reaches an internally ready state (`actor=ready`, supervisor runtime actor ready, controller lease ready) but the pane probe still reports `alive-busy` / `prompt_ready=false` from a recoverable stale queued-draft cue, route-owned completion and supervisor idle-queue dispatch must treat that as a bounded ready/busy conflict rather than an unbounded keep-alive. After the same four-poll debounce used by stale-busy idle repair, they emit `owned_pane_ready_busy_conflict` and continue route-owned reap/liveness or queue dispatch as appropriate. Active turn cues, permission prompts, hook-review prompts, shell-search prompts, help screens, and clean-exit prompts remain hard blockers. `agent-doc session status` prints this conflict with a bounded reconcile/clear hint so the operator does not have to infer it from raw ready/busy fields.

**Uncommitted queue head dispatch guard (`#qdispatchloss`):** Route selects an inactive `agent:queue` head from the live on-disk document (`std::fs::read_to_string`), but the JetBrains/VS Code plugin may have synced an *uncommitted* operator queue edit to disk before it reaches a git-committed snapshot. Route must never consume/dispatch a head that is not backed by the committed snapshot — doing so moves a possibly half-typed line into the agent prompt and then loses it (the consume never lands in a committed snapshot, so the item disappears and the turn stalls uncommitted). Before surfacing an inactive head for activation/dispatch, `inactive_route_queue_head_in_content` compares the candidate head text against the queue prompt/completed entries in `snapshot::load` (the committed-state proxy): a head absent from a present committed queue — or any head when the committed snapshot has no queue component at all — is treated as "still being edited" and the read fails closed (returns `None`, logging `route_dispatch_uncommitted_head ... reason=head_not_in_committed_snapshot decision=defer`). The activate path therefore no-ops, the operator's edit is left intact on disk for the next cycle, and the bare reopen routes through the normal preflight path, which commits the queue edit first and then dispatches the head from the committed snapshot. This guard is conservative — a missing committed snapshot (untracked scaffold) or an unreadable/unparseable snapshot allows the head (bootstrap escape hatch; cannot prove divergence, so a legitimate drain is never stalled). It applies only to the inactive-activation path; active go-mode continuation heads flow through `queue_continuation::live_continuation_head`, which only sees heads that were committed when the queue activated, so the running auto-loop is unaffected. This is the no-crash sibling of the crash-durability item `#qdurcrash`.

**Dead fallback session guard:** Route may fall back to the current tmux session or an already-alive harness fallback session, but it must not auto-start a brand-new implicit fallback session such as `"claude"` or `"codex"` just because no explicit target survived resolution. If the only remaining target is a dead implicit fallback name, route fails closed.

**Duplicate-pane guard:** When a document's registry entry is stale but there are still live panes whose process trees or supervisor PID still prove that document, route first computes the full candidate set. It only auto-picks when the winner is unambiguous: a single provable owner overall, or a single owner in the active tmux window while every other candidate is already stashed. Otherwise route fails closed with an ambiguity report and direct inspect/claim/kill commands.

**No hidden fallback-pane guard:** When route needs a fresh pane, it may split beside a visible authoritative anchor or create a brand-new window only when no `agent-doc` window exists yet. If `split-window` fails beside the chosen anchor, or if the target session already has an `agent-doc` window but no safe registered anchor pane, route must fail closed and show tmux inspect/kill commands instead of creating a hidden stash fallback pane.

**Failed fresh-start cleanup guard:** If route creates a new pane, registers it, and later fails closed because a fresh-start admission projection was not observed, cleanup must preserve that pane when it is still the live registered owner for the document. The operator should see the admission-projection failure, not a killed pane.

**Stranded-trigger resubmit on a fresh pane (`#jbtsiftnosub2`):** When a freshly-created route pane produces no reactive admission projection, route classifies the pane from a single capture before deciding. A pane that is back at a dispatch-ready prompt with an *empty* composer is a legitimate idle no-op (empty/halted queue, preflight `no_changes`) and the live idle session is kept. But a pane that is dispatch-ready while the injected trigger is *still visible unsubmitted in the composer* is the JB-created-fresh-pane "prompt added but not submitted" drift — `wait_for_agent_ready` proved a transient ready prompt and the supervisor-IPC inject typed the trigger, but the submit key never registered against the still-initializing composer. This must NOT be kept as a healthy idle no-op (which strands the operator's request forever); route resubmits the stranded draft once with a bare harness submit key (`Enter`) and re-awaits the Project Controller projection. If the resubmit finally starts a document cycle the session is kept; otherwise route records a `FreshStart` startup-miss and fails closed so the miss is visible and recoverable. The composer-pending check strips all whitespace from both the capture and the trigger so a trigger wrapped across terminal columns still matches, and it cannot false-fire because a genuinely-submitted turn clears the composer and projects the new cycle before this branch is reached. Keep `fresh_start_admission_outcome`/`pane_composer_has_pending_trigger` (`agent-doc-controller`) and the route consumer (`agent-doc-route-io` `startup.rs`/`startup_ready.rs`) aligned with this rule.

The starting-actor wait decision is conjunctive: route continues through `starting`, through restart-bootstrap `busy`, and through `ready` without prompt proof; only `ready` plus dispatch-ready prompt proof plus dispatch eligibility may release the route. If that wait times out, the timeout is keyed by document, pane, and actor generation so repeated editor reroutes produce one `route_authoritative_actor_starting_not_ready` diagnostic for the stuck generation and only coalesced wait telemetry until the actor reaches `ready`, `closed`, or `blocked` or a different pane/generation becomes authoritative. A route-owned `starting_actor_timeout` block is recoverable only by direct dispatch-ready prompt proof on the same pane/generation. Because the timeout record is sticky (`wait_for_authoritative_actor_ready` short-circuits once it is set), route polls the blocked actor's live pane for that proof up to the harness recovery budget before failing closed, so a healthy but slow-starting harness — for example a heavy Codex model that takes several seconds to present its idle composer after a supervisor restart — recovers within the same reroute instead of dead-ending on a single capture. A busy pane never satisfies the dispatch-ready proof (the harness busy cue short-circuits it), so the bounded poll preserves the promote-only-proven-idle invariant. That proof clears the saved timeout and promotes the actor to `ready` before any reopen is submitted.

`routed_reopen` is the FlowCore owner for these decisions. Route may keep tmux,
controller, and supervisor side effects in `route.rs`, but actor binding,
prompt-ready barrier, dispatch authorization, submit, and dispatch-proof
branches must be representable as typed FlowCore outcomes. Prompt-ready
fail-closed branches emit mirror-mode `flow_event flow=routed_reopen
stage=prompt_ready_barrier outcome=failed_closed ...` diagnostics so ops
summary can group route failures by stage instead of relying only on tactical
log strings. The route coordinator imports the pure routed-reopen delivery,
retry-budget, direct-submit outcome, dispatch-only proof policy,
dispatch-start proof, degraded-authority, and prompt-ready-barrier classifiers
from `flow::routed_reopen`; `route.rs` remains the tmux/supervisor/controller
I/O boundary.

**Auto-filed Run Agent Doc route bugs (`#3ygp`):** When route exhausts a bounded
submit or dispatch-start proof path, it must file a session-document backlog item
tagged `#jbrunautobug #agent-doc-bug`. The item records the failure class,
document, stage, pane, best-effort actor generation, editor attempt id, dispatch
proof, saved route-submit diagnostic path, and `ops.log` marker/path. The
symptom key is stable for the same document/stage/failure so repeated JetBrains
`Run Agent Doc` failures append evidence to one active backlog item instead of
creating duplicate work. The existing direct-pane submit recovery still retries
visible drafted triggers through the bounded Enter-resubmit loop before this bug
filing path runs.

**Dead-harness pre-send guard (#1vhn):** the dispatch-ready prompt proof can go
stale between the readiness check and the actual send if the harness crashes or
exits to a bare interactive shell in that window. Before typing the routed
trigger into a pane, the direct-pane send path re-verifies that a live harness
still owns the pane: it fails closed when `#{pane_current_command}` reports a
bare shell (`zsh`/`bash`/`sh`/`fish`/`dash`/`ksh`/`tcsh`/`csh`, with or without a
login `-` prefix) **and** the captured pane shows no harness dispatch-ready
prompt. Both signals are required so a harness that briefly spawns a subshell, or
a momentary `#{pane_current_command}` read while the harness composer is still
the visible prompt, does not trip a false positive. On a dead-harness pane route
refuses the send, logs `route_dispatch_into_dead_shell_blocked ...
reason=harness_exited_to_bare_shell`, and surfaces claim/restart guidance instead
of leaving `agent-doc <FILE>` as un-run text in a shell. This closes the
crash-mid-dispatch race observed when a `/model` switch / restart dropped the
harness to a shell right as "Run Agent Doc" routed the trigger.

## Stash Window Routing

The stash system preserves running Claude sessions when the user switches editor tabs. Panes are moved to a hidden stash window rather than killed, keeping the Claude session alive for later reuse.

**Window-scoped routing:** Each editor split maps to a tmux pane in the primary window (`@0`). When the user switches files, `reconcile()` swaps panes by detaching unwanted ones into the stash and attaching needed ones back.

**Stash lifecycle:**

| Phase | Operation | Detail |
|-------|-----------|--------|
| DETACH | `stash_pane()` | Moves an unwanted pane into the stash window via `tmux join-pane` |
| — | target selection | Targets the LARGEST pane in the stash (by height) to avoid "pane too small" errors |
| — | overflow | If join fails, `break_pane_to_stash()` creates an overflow stash window (also named `"stash"`) |
| ATTACH | `reconcile()` | Joins a stashed pane back into `@0` when needed again |
| RESCUE | `sync` pre-resolution | Rescues same-session stashed panes back to agent-doc window via guarded `join-pane` before layout; cross-session stash rescue fails closed and preserves the live pane in place |

**Discovery:** `find_all_stash_windows()` returns all stash windows — both the primary stash and any overflow windows. All windows named `"stash"` or matching `"stash-*"` (tmux auto-deduplication) are treated as stash windows by `is_stash_window_name()`.

**Invariants:**
- Stashed panes keep running — the Claude session remains alive inside
- Stash windows are named `"stash"` for consistent discovery
- The stash window is resized to 200 rows before join operations to prevent minimum-size failures
- Focus never leaves window `@0` during stash operations (`-d` flags are always set)
- Successful route replacements do not kill older stash panes unless the cleanup path has explicit provenance that the older pane is a throwaway artifact from the current recovery attempt
- Dispatch-only startup-window tmux submits are allowed only after the target pane shows harness-specific dispatch-ready evidence. Codex and OpenCode model/context footers are UI chrome, but bottom idle chrome is accepted as composer-ready when no busy cue, hook-review prompt, protected prompt input, or drafted text is visible. OpenCode's idle splash screen, including `Ask anything...`, build-plan text, command/footer chrome, and cwd/version status, is also a dispatch-ready composer signal even when no standalone `>` glyph is rendered, and OpenCode gets the longer harness-specific redraw/recovery budget before route surfaces a `still booting` refusal.
- Codex hook-review warnings are blocking UI, not idle chrome. If recent pane output says a hook needs review or asks the operator to open `/hooks`, route/start must fail closed before injection even when a capability-proof line follows the warning.
- Normal route/start/sync/preflight/finalize flows must not kill tmux panes. Pane-kill cleanup is reserved for explicit repair surfaces after the operator has decided which pane is expendable.
- Scoped `agent-doc fix <FILE>` may kill redundant unregistered stash panes for that document, but only after it has already rebound the file to a unique provable winner

**Editor-to-tmux truth table:**

| Editor / session inputs | agent-doc window result | stash result |
|---|---|---|
| Explicit sync `--window` points at session `S` and visible docs reconcile cleanly | Reconcile in `S:agent-doc` even if another tmux session is currently attached | Leave unrelated stash panes untouched |
| No `--window`, project `.agent-doc/config.toml` pins live session `S0`, caller is attached to session `S1` | Reconcile in `S0:agent-doc`; do not inherit `S1` just because it is current | Rescue only stash panes that already belong to `S0`; leave foreign-session stash panes alone |
| No `--window`, project pin missing or dead, caller is attached to live session `S1` | Reconcile in `S1:agent-doc` | Use `S1` stash windows only |
| Visible document already owns a live pane in the target session's stash window | Rescue that pane back into the target `agent-doc` window before provisioning a replacement | The rescued pane leaves stash; no duplicate replacement pane is created |
| Visible columns include a markdown doc plus an empty/non-markdown sibling column | Keep the visible markdown doc in its requested column; remembered sibling panes may fill the empty column only when they do not duplicate a currently visible doc | No stash mutation unless a same-session pane must be rescued back to satisfy the remembered layout |
| Visible docs span multiple project roots | Build per-root registry context, then place each doc in the target session's `agent-doc` window without aliasing two docs onto one pane | Preserve stash panes that still prove ownership inside their own project root; fail closed on ambiguous duplicates |

**Cross-project stash preservation is telemetry, not an error (`#cross-project-stash-pane-condition`).** When nested project roots share one tmux session (e.g. a superproject and its submodule both auto-detect the attached session), the current root's resync stash sweep can encounter a pane that is the live, registered pane of a *different* root. Preserving that pane (skipping the kill) is correct cross-project behavior. The resync sweep records that decision via `ops_log` telemetry (`.agent-doc/logs/ops.log`), **not** stderr, so the benign `registered in its own project root — skipping kill` event does not surface in the route/resync stderr stream that IDE plugins render as an error. Only genuine failures (for example a failed `kill-pane`) stay on stderr. Surfacing this benign class as an IDE notification distinct from real failures is tracked separately under the plugin UX item.
| A needed pane exists only in a foreign tmux session's stash window | Fail closed / log cross-session rescue block; do not silently move it into the current target session | Keep the foreign-session stash pane where it is until an explicit session-targeting action resolves the conflict |

**Commit write contract:** `commit()` always stages a clean snapshot copy (no `(HEAD)`, no stale boundaries) and normalizes the boundary to end-of-exchange. The committed blob and snapshot converge to the same clean shape. The working tree and editor buffer preserve `(HEAD)` annotations so the user can see which response headings are new.

**Snapshot boundary cleanup:** After committing, `commit()` keeps the snapshot in the same clean shape as the committed blob: ALL stale boundaries are stripped, heading-level transient ` (HEAD)` markers are removed, and a single fresh 8-char boundary is inserted at end-of-exchange.

**Working-tree boundary cleanup:** The working tree uses `reposition_boundary_to_end_preserve_head()`, which removes stale boundaries and inserts a fresh one but keeps `(HEAD)` annotations. Preflight classifies `(HEAD)` differences as `boundary_artifact`, so they do not cause false-positive change detection.

**Editor cleanup invariant:** The editor-side reposition helpers (JetBrains and VS Code) must collapse stale boundaries. When the IPC reposition signal includes `preserve_head: true`, the plugin should call `agent_doc_reposition_boundary_to_end_preserve_head_with_id()` for the normal case, but it must not overwrite the just-committed disk response with a stale unsaved buffer that only differs by agent-owned response-heading attribution, already-answered `❯ ` prompt-prefix churn, and/or boundary churn. Boundary-only cleanup must not create a follow-up diff that is only old boundary IDs, stale harness-label headings such as `codex (HEAD)` must not be written back over the committed model attribution, and historical answered prompts must keep their binary-owned prefix in the preserved working tree/editor view.

**Boundary reposition lifecycle:**
1. **Before IPC patch JSON (clean boundary reposition):** All IPC write paths (`run_ipc`, `try_ipc`, and retry re-sends) read the on-disk document and normalize boundaries in memory before extracting `boundary_id` values. This removes ALL stale boundaries and inserts a single fresh one. The repositioned document is used solely to extract `boundary_id` values — never written to disk. This ensures the `boundary_id` points to end-of-exchange (after the user's prompt), not the stale mid-exchange position.
2. During `agent-doc write`: the `reposition_boundary: true` IPC flag tells the plugin to move the boundary after applying the response patch. The plugin should call `agent_doc_reposition_boundary_to_end()` via FFI to ensure identical cleanup logic.
3. During `agent-doc commit`: (a) the snapshot is cleaned via the clean reposition helper, and (b) a standalone IPC signal with `preserve_head: true` is sent when a live listener exists so the plugin preserves `(HEAD)` in the editor buffer. Without a live listener, the working tree is rewritten with the preserve-head variant.
4. If no plugin is active, the file is rewritten locally with `reposition_boundary_to_end_preserve_head()`, preserving `(HEAD)` annotations while removing stale boundaries.

**Terminal editor-intent guard:** Once a document cycle reaches `committed`, editor delivery for that cycle is terminal. Late socket retries and receipts are rejected by `cycle_id`, `intent_id`, generation, and the `state.db` phase. They cannot re-dirty the document, publish a new recovery projection, or run another commit for the closed cycle.

## Pane Lifecycle — Binding Invariant

**The editor-selected document drives pane resolution. It either finds an existing pane that already claims that document, or provisions a new one. It NEVER commandeers another document's pane.**

This is the **Binding invariant** — the foundational rule of pane management.

### Bounce-Back Suppression

JetBrains (and potentially VS Code) fires spurious `selectionChanged` events in split layouts: after the user selects a file in one split, the IDE re-fires selection for the other split's file ~1 second later. Without suppression, the tmux pane focus bounces back to the previous file, making it appear as if navigation doesn't work.

Both editor plugins track:
- `lastCommandCompletedAt` — timestamp of the last successful focus/sync command
- `focusedFileBeforeLastCommand` — the file that was focused before the command ran

Within a 1.5-second settle window after a command, if a new selection event arrives with the same visible file set and targets the pre-command focused file, it is classified as `BounceBack` and suppressed.

### Resolution Path

When the user navigates to a document in the editor:

1. **Sync fires** — JB plugin sends `agent-doc sync --col <file1> --col <file2> --focus <focused_file>`
2. **Initialization** — `ensure_initialized()` runs for each file in `col_args`:
   - If file is empty (no frontmatter, no content) → auto-scaffold as template with frontmatter + exchange component
   - If file has `agent_doc_format` but no `agent_doc_session` → assigns a UUID
   - If no snapshot exists → creates snapshot + `git add` + `git commit`
3. **File resolution** — `resolve_file()` reads frontmatter. Files with `agent_doc_session` → `FileResolution::Registered`. Non-`.md` files or files with content but no frontmatter → `Unmanaged`. For mixed-root editor layouts, resolution and later registry write-back must canonicalize each file path and consult the nearest `.agent-doc` ancestor for that file, not the caller's current working directory, so sibling repos in the same IDE window cannot borrow or overwrite each other's pane bindings.
4. **Reconciliation** — `tmux_router::sync` matches the declared layout to tmux panes:
   - Pane exists for this session → **focus it** (Binding found)
   - Pane in stash → **rescue it** (join-pane it back to the agent-doc window without evicting a visible pane)
   - No pane exists → trigger **Provisioning**
5. **Provisioning** — `route::provision_pane()` creates a new tmux pane:
- Serializes concurrent provisioning with per-document and per-session startup flocks before choosing the split target
- Re-checks the registry after the lock is acquired so a concurrent route that already registered this document is reused instead of double-started
- Splits alongside an existing pane in the agent-doc window
- For nested-project documents sharing that window, accepts a pane from another project as a split-only anchor only when every visible pane's process tree proves a different agent-doc document owner; unknown or same-document ownership stays fail-closed
- Registers the session→pane **Binding** in the controller state transaction
   - Starts Claude asynchronously in the new pane

### Cross-Repository Owner Guard (`#cross-repo-owner-guard`)

Nearest-`.agent-doc` resolution alone does **not** isolate sibling repositories when a
nested git submodule carries no local `.agent-doc/`. `find_project_root` walks up to the
nearest ancestor that contains `.agent-doc/`, so a document under a submodule such as
`src/lazily-rs/` (no `.agent-doc/` of its own) collapses up to the **superproject** root —
the same root as a superproject document like `tasks/professional/sampleportal.md`.
The submodule and superproject then share one `.agent-doc/` keyspace (registry, controller
store, supervisor sockets, session logs), and every `.agent-doc`-root equality check
(`pane_assignment_matches_document_root`, `registry_entry_matches_document_root`) compares
`superproject == superproject` and cannot tell a submodule pane apart from a superproject
document. Left unguarded this let a supervisor restart re-attach a submodule agent session
(e.g. a lazily Claude pane) onto a superproject document's pane.

Owner resolution therefore draws the durable project boundary at the **git repository**, not
the nearest `.agent-doc/`. `reject_cross_document_owner_pane` — the single chokepoint that
both the heuristic (`find_live_owner_pane*`) and normal-path (`find_normal_path_owner_pane*`)
resolvers funnel their candidate through — rejects any pane whose working-directory
`git rev-parse --show-toplevel` differs from the document's git toplevel. The comparison is a
strict tightening: an unknown toplevel on either side (git unavailable, non-git tree) is never
treated as foreign, so an ordinary same-repo owner is never spuriously cold-started. This also
closes the bare-foreign-session gap that the `.md`-document cross-document check misses — a raw
`claude`/`codex` pane carries no agent-doc document path in its command line, but its git
repository still identifies it as foreign.

### Invariants

| Invariant | Enforcement |
|-----------|-------------|
| One document per pane | Registry check in `claim::run()` and transaction-level actor-store handoff that closes and clears displaced cross-document owners |
| A pane never owns a document from a different git repository (`#cross-repo-owner-guard`) | `reject_cross_document_owner_pane` drops any owner candidate whose working directory resolves to a different `git rev-parse --show-toplevel` than the document, so a nested submodule pane cannot be re-attached to a superproject document (and vice versa) |
| Document drives, pane follows | Sync resolves files first, then matches to panes |
| Editor-selected document owns the requested pane | `auto_start` creates new panes when needed; actor-store writes recover stale aliases by making the incoming document authoritative and clearing displaced owners |
| Stashed panes stay alive | `join-pane` moves to stash, doesn't kill |
| Initialization is idempotent | `ensure_initialized()` checks snapshot existence first |

### Terminology (Domain Ontology)

| Term | Definition | Module |
|------|-----------|--------|
| **Binding** | Transactional document→pane association | `claim.rs`, registry state adapters |
| **Reconciliation** | Matching editor layout to tmux layout | `sync.rs` |
| **Provisioning** | Creating a new pane + starting Claude | `route.rs` (`auto_start`) |
| **Initialization** | Assigning UUID + snapshot + git tracking | `snapshot.rs` (`ensure_initialized`) |
