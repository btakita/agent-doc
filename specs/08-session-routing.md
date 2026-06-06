> Extracted from SPEC.md — see [index](../SPEC.md)

# Session Routing

## Registry

`sessions.json` maps canonical absolute document paths to the owning tmux pane plus the current supervisor identity:

```json
{
  "/path/to/project/tasks/plan.md": {
    "session_id": "cf853a21-...",
    "pane": "%4",
    "pid": 12345,
    "supervisor_instance_id": "9a18c1b2-...",
    "cwd": "/path/to/project",
    "started": "2026-02-25T21:24:46Z",
    "file": "tasks/plan.md",
    "window": "1"
  }
}
```

The registry key is the stable document identity; `session_id` stays in the value so the same document can preserve its session UUID while the owning supervisor instance changes. Multiple documents can still map to the same pane (one Claude/Codex session, multiple files). The `window` field (optional) enables window-scoped routing — `claim --window` and `layout --window` use it to filter panes to the correct IDE window.

The broader single-owner session-actor contract lives in
[08a-session-actor-contract.md](08a-session-actor-contract.md). In phase 1 the
registry remains a projection/binding helper, while the session log carries the
monotonic ownership-generation provenance.

## Use Cases

| # | Scenario | Command | What Happens |
|---|---|---|---|
| U1 | First session for a document | `agent-doc start plan.md` | Creates tmux pane, launches Claude, registers pane |
| U2 | Submit from JetBrains plugin | Plugin `Ctrl+Shift+Alt+A` | Calls `agent-doc route <file>` → sends to registered pane |
| U3 | Submit from Claude Code | `/agent-doc plan.md` | Skill invocation — diff, respond, write back |
| U4 | Claim file for current session | `/agent-doc claim plan.md` | Skill delegates to `agent-doc claim` → updates sessions.json |
| U5 | Claim after manual Claude start | `/agent-doc claim plan.md` | Fixes stale pane mapping without restarting |
| U6 | Claim multiple files | `/agent-doc claim a.md` then `/agent-doc claim b.md` | Both files route to same pane |
| U7 | Re-claim after reboot | `/agent-doc claim plan.md` | Overrides old pane mapping (last-call-wins) |
| U8 | Pane dies, plugin submits | Plugin `Ctrl+Shift+Alt+A` | `route` detects dead pane → auto-start cascade |
| U9 | Install skill in new project | `agent-doc skill install` | Writes bundled SKILL.md to `.claude/skills/agent-doc/` |
| U10 | Check skill version after upgrade | `agent-doc skill check` | Reports "up to date" or "outdated" |
| U11 | Permission prompt from plugin | PromptPoller polls `prompt --all` | Shows bottom bar with numbered hotkeys in IDE |
| U12 | Claim notification in session | Skill reads `.agent-doc/claims.log` | Prints claim records, truncates log |
| U13 | Clean up dead pane mappings | `agent-doc resync` | Removes stale entries from sessions.json |

## Claim Semantics

`claim` binds a document to a **tmux pane**, not a Claude session. The pane is the routing target — `route` sends keystrokes to the pane. Claude sessions come and go (restart, resume), but the pane persists. If Claude restarts on the same pane, routing still works without re-claiming.

Last-call-wins: any `claim` overwrites the previous mapping for that document's session UUID.

**Canonical same-document claim reuse:** `claim` must judge "is this pane already mine?" by canonical document identity, not by the raw map key it happens to be iterating in `sessions.json`. The registry key is a canonical file path, not a session UUID. Re-claiming the same live pane for the same document, including submodule-relative `entry.file` shapes, must remain idempotent and must not provision a duplicate pane.

**Cross-session claim guard:** `claim` may only bind to a pane in another tmux session when the configured project session is no longer alive or the user explicitly passes `--force`. A healthy configured session is a hard boundary; cross-session claims fail closed instead of silently rebinding the document.

**Fresh-start stale-session rebind:** When `start` falls through to a fresh pane, finds that the configured project session is dead, and therefore registers the document in the caller's current live tmux session instead, it must persist that new session back to `.agent-doc/config.toml`. That keeps later `route` / `claim` resolution aligned with the new binding instead of repeatedly targeting the dead session name.

**Prompt-bearing rerun acknowledgment:** When `route` dispatches to an already-running pane and the document already has prompt-bearing user drift on top of a closed cycle, pane-input acceptance is not sufficient proof of success. Route must observe a newer per-document cycle state (`preflight_started` or later) before returning success; if the baseline was already `committed`, that means a newer cycle id, not a same-cycle `commit_already_current` mutation. Otherwise it fails closed instead of silently leaving the prompt stranded in the document. This guard keys off document-body prompt drift only; frontmatter-only metadata edits such as `agent: codex` must not trigger the rerun-ack wait.

**Queued rerun preemption (`#jb-run-preempt-autoloop-priority`):** When `route --dispatch-only` cannot inject a prompt-bearing editor rerun because the authoritative actor is busy and it records that rerun in `agent:queue auto`, the rerun is inserted **ahead of pending auto items** so a manual `Run Agent Doc` preempts the auto-loop instead of landing at the tail. The priority insert lands after any leading queue directive (preset / start fence) and must not supersede a lone auto prompt — preempt preserves the pending auto-loop item (manual prompt first, then the pending item) rather than replacing it, which avoids the data loss the legacy single-prompt supersede risked. The legacy tail-append + lone-prompt supersede behavior remains available to non-priority (`priority=false`) `enqueue_route_dispatch_prompt` callers and is covered by its own tests.

**Turn-in-progress status on the queued path (`#claude-busy-status-during-active-turn`):** when that busy-actor path queues the prompt and returns success, route must surface a visible turn-in-progress status on the owned pane (tmux `display-message`) naming that the live session is busy and the Run Agent Doc was queued and will run when the current turn finishes — explicitly *not* asking the operator to rerun (the prompt already auto-queued). This is status-surfacing only (no hard block): it closes the gap where the queued path returned silently and the session looked idle while a turn was in flight. Hard busy-lease enforcement remains gated on the `#subagent-blocks-session` scope decision.

**Authoritative actor dispatch:** When the durable actor store has a healthy authoritative record for the document generation, route must treat that actor pane as the dispatch target and send the bare reopen through supervisor IPC. Harness compatibility at this boundary is checked against the actor store's canonical harness ids (`claude-code`, `codex`, `default`), not raw supervisor binary labels, so a live `claude` supervisor must accept an actor record stored as `claude-code` instead of failing closed on the alias. A cross-harness mismatch is a conflict only when the recorded pane still has a healthy live supervisor and a non-closed actor state; dead panes, closed actors, and unreachable supervisor records are stale authority and must fall through to fresh start/rebind instead of blocking an intentional harness switch such as Codex to Claude. Supervisor-owned command injection must use the same claimed-pane tmux submit path as real operator input instead of writing reopen bytes directly into the child PTY. The IPC payload still carries one canonical single-line submit command, but the actual delivery boundary is the centralized `tmux-router` hybrid submit policy through the authoritative pane so `Run Agent Doc`, `Clear Session Context`, queue-dispatch, and auto-trigger share one real Enter method instead of distinct PTY-write semantics. Existing managed-session reroutes must not fall back to direct tmux `send-keys`; they either dispatch through the supervisor socket or fail/recover explicitly. If process argv no longer exposes the document path, a healthy supervisor PID that maps to the registered pane is still a supervisor-owned managed binding; direct pane prompt classification must not downgrade that path to focus-only no-op. The controller `actor_binding` response is typed: `bound` includes the actor record, `not_found` is an explicit absent-binding result, and `ok:true` without data is a controller protocol error rather than a routing fallback. When route does have to type into a non-authoritative fallback pane, the submit contract stays the same: one explicit byte-stream write ending in carriage return, not a split literal-text send followed by a later `Enter` or a newline-terminated payload. When there is prompt-bearing reroute work and the authoritative actor is still `starting`, route must first wait for the current actor generation to refresh to `ready` with dispatch-ready prompt proof and then fail closed if it remains unready; no managed, dispatch-only, or direct-pane path may inject into a still-starting actor. That fail-closed diagnostic must say to wait and rerun `agent-doc <FILE>`, name `agent-doc start <FILE>` as the stuck-owner recovery, and persist the last observed pane id, generation, actor state, supervisor health/runtime state, prompt-ready status, elapsed wait, and lifecycle transition. A `busy` actor may still accept one supervisor-owned queued reopen for managed reroutes, but dispatch-only reroutes must first observe the current generation return to `ready`; otherwise they fail closed instead of using a direct pane submit during bootstrap or an active turn. Direct pane evidence repairs a stale busy projection (`#snrun`): when a dispatch-only reroute finds the authoritative actor projected `busy` but the live pane proves a dispatch-ready prompt in the current generation, the busy/lease projection is stale — the actor is not actually mid-turn — so route promotes it to `ready` and dispatches via direct pane submit instead of queuing the prompt into `agent:queue auto`. A `busy` projection without that proven ready prompt is left untouched and still fails closed (queues), consistent with the direct-evidence rule that idle direct evidence repairs stale busy while busy direct evidence stays fail-closed. `waiting_input`, `blocked`, and `closed` remain hard stop states for duplicate reopen injection. A plain dispatch-only reopen with no prompt-bearing work (an editor `Run Agent Doc` reopen) against a `busy` actor classifies as focus-only, but it must fail closed with the busy-not-ready diagnostic after focusing the pane instead of returning focus-only success — otherwise the editor caller reports a routed run even though nothing was submitted, and the operator sees no feedback after the full ready-wait timeout (`#jb-run-agent-doc-command-route-miss`). The narrow exception is an already-startable inactive `agent:queue auto` head: route sets `queue_active: true`, syncs the snapshot, logs `route_dispatch_only_busy_existing_queue_deferred`, and returns deferred queued feedback so the supervisor idle-queue watch can drain that head when the pane becomes idle. Plain inactive queues without `auto` or a start fence stay inert. A second deferral case (`#jb-busy-reopen-auto-drain-when-idle`) covers a document whose queue is **already actively auto-looping**: when there is no inactive head to activate but `queue_continuation::detect` reports a live continuation (the running `agent:queue auto` loop is still draining this document), a bare dispatch-only reopen must report **deferred success** (logging `route_dispatch_only_busy_active_auto_loop_deferred`) rather than the busy-not-ready refusal — the operator's `Run Agent Doc` against a self-driving loop catches a brief inter-iteration gap and would otherwise see an error for a session that is in fact making progress; the active loop continues the document, so there is nothing to inject. This deferral only fires on a proven active continuation; a busy actor with a drained/inactive queue still fails closed with the busy refusal. The fail-closed shape logs `route_dispatch_only_authoritative_actor_busy_focus_only_not_dispatched` and carries the same `the authoritative actor is busy did not return to a dispatch-ready prompt` message the editor classifies as a "session still running" notification, surfaced immediately rather than after silent retries (a still-booting `starting` pane stays the only silently-retried shape). The busy ready-wait itself is gated on the live pane (`#jb-run-agent-doc-busy-active-turn-stall`): when a bare dispatch-only reopen with no prompt/queue fallback targets an actor whose pane proves a genuine active turn (a working spinner or `esc to interrupt` busy-proof line), route skips the bounded ready-wait entirely and refuses immediately, logging `route_dispatch_only_busy_active_turn_skip_wait`, because a multi-minute turn cannot reach a dispatch-ready prompt inside that budget and waiting only produces a silent stall before the inevitable refusal. The refusal is then worded as a busy active turn (naming the cue) instead of the misleading cold-start "after waiting Ns" phrasing. A `busy` projection *without* a live active-turn cue (transient or stale) still serves the bounded ready-wait so a turn about to finish is still picked up. After any IPC or auto-trigger dispatch marks the actor `busy`, the next visible idle harness prompt for that same child must drive the supervisor actor state back to `ready`; this ready transition cannot be limited to the first prompt observed after process spawn. In that state, `sessions.json` is only a projection/binding helper: route may refresh the registry to match the actor pane, but it must not re-elect ownership from live-owner heuristics, same-file associated panes, or opportunistic registry rebinds.

**Dead fallback session guard:** Route may fall back to the current tmux session or an already-alive harness fallback session, but it must not auto-start a brand-new implicit fallback session such as `"claude"` or `"codex"` just because no explicit target survived resolution. If the only remaining target is a dead implicit fallback name, route fails closed.

**Duplicate-pane guard:** When a document's registry entry is stale but there are still live panes whose process trees or supervisor PID still prove that document, route first computes the full candidate set. It only auto-picks when the winner is unambiguous: a single provable owner overall, or a single owner in the active tmux window while every other candidate is already stashed. Otherwise route fails closed with an ambiguity report and direct inspect/claim/kill commands.

**No hidden fallback-pane guard:** When route needs a fresh pane, it may split beside a visible authoritative anchor or create a brand-new window only when no `agent-doc` window exists yet. If `split-window` fails beside the chosen anchor, or if the target session already has an `agent-doc` window but no safe registered anchor pane, route must fail closed and show tmux inspect/kill commands instead of creating a hidden stash fallback pane.

**Failed fresh-start cleanup guard:** If route creates a new pane, registers it, and later fails closed because fresh-start acknowledgment was not observed, cleanup must preserve that pane when it is still the live registered owner for the document. The operator should see the startup-ack failure, not a killed pane.

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
- Dispatch-only startup-window tmux submits are allowed only after the target pane shows a harness-specific dispatch-ready prompt. Codex model/context footers are UI chrome, not proof that the prompt can consume a routed reopen. OpenCode's idle splash screen, including `Ask anything...`, build-plan text, command/footer chrome, and cwd/version status, is a dispatch-ready composer signal even when no standalone `>` glyph is rendered, and OpenCode gets the longer harness-specific redraw/recovery budget before route surfaces a `still booting` refusal.
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
1. **Before IPC patch JSON (clean boundary reposition):** All IPC write paths (`run_ipc`, `try_ipc`, IPC-timeout fallback) read the on-disk document and normalize boundaries in memory before extracting `boundary_id` values. This removes ALL stale boundaries and inserts a single fresh one. The repositioned document is used solely to extract `boundary_id` values — never written to disk. This ensures the `boundary_id` points to end-of-exchange (after the user's prompt), not the stale mid-exchange position.
2. During `agent-doc write`: the `reposition_boundary: true` IPC flag tells the plugin to move the boundary after applying the response patch. The plugin should call `agent_doc_reposition_boundary_to_end()` via FFI to ensure identical cleanup logic.
3. During `agent-doc commit`: (a) the snapshot is cleaned via the clean reposition helper, and (b) a standalone IPC signal with `preserve_head: true` is sent when a live listener exists so the plugin preserves `(HEAD)` in the editor buffer. Without a live listener, the working tree is rewritten with the preserve-head variant.
4. If no plugin is active, the file is rewritten locally with `reposition_boundary_to_end_preserve_head()`, preserving `(HEAD)` annotations while removing stale boundaries.

**Terminal IPC cycle guard:** Once a document cycle reaches `committed`, response IPC for that cycle is terminal. Late socket/file fallback writers, including disabled full-content fallback cleanup, must remove stale queued patch JSON, write a claimed-patch sentinel when a `patch_id` exists, and return an explicit committed-cycle skip that callers do not classify as `ipc_write_consumed`. IPC polling must re-check the terminal cycle state before treating a consumed patch file as success, so stale writers terminate instead of saving a new snapshot for an already-closed cycle. No fallback path may re-dirty the session document, save a new snapshot, or run another already-current commit for that closed cycle.

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
   - Registers the session→pane **Binding** in `sessions.json`
   - Starts Claude asynchronously in the new pane

### Invariants

| Invariant | Enforcement |
|-----------|-------------|
| One document per pane | Registry check in `claim::run()` (line 142-156) |
| Document drives, pane follows | Sync resolves files first, then matches to panes |
| Never commandeer another document's pane | `auto_start` creates new panes; `claim` validates pane isn't already bound |
| Stashed panes stay alive | `join-pane` moves to stash, doesn't kill |
| Initialization is idempotent | `ensure_initialized()` checks snapshot existence first |

### Terminology (Domain Ontology)

| Term | Definition | Module |
|------|-----------|--------|
| **Binding** | Document→pane association in `sessions.json` | `claim.rs`, `sessions.rs` |
| **Reconciliation** | Matching editor layout to tmux layout | `sync.rs` |
| **Provisioning** | Creating a new pane + starting Claude | `route.rs` (`auto_start`) |
| **Initialization** | Assigning UUID + snapshot + git tracking | `snapshot.rs` (`ensure_initialized`) |
