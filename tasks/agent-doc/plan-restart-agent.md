# Plan: Restart Agent + agent-change auto-restart

Operator ask (agent-doc-bugs2.md): *"Add `Restart Agent` Plugin menu command. Changing the agent such as from claude to opencode should restart the agent during the next boundary."*

## Key finding (the non-obvious prerequisite)

The supervisor restart path **does not re-resolve the harness from frontmatter** — it reuses the harness resolved once at start:

- `SharedSupervisorState.harness: HarnessConfig` / `harness_binary: String` are resolved a single time and threaded through the whole restart loop (`agent-doc-orchestration/src/start.rs:171`, used at 450/472/484/682/892/1226/...).
- The in-process restart (`supervisor/in_process.rs:277 restart(with_continue)`) kills the child and respawns via the `child_factory`; respawn is owned by the start.rs supervisor restart loop (`in_process.rs:140`, `#pcpc5e1`). The factory was built with the original harness — it does **not** read `agent:` again.
- `HarnessConfig::from_context(fm, config)` (`agent-doc-orchestration/src/harness.rs:116`) is the resolver that reads `fm.agent` (`agent-doc-core/src/frontmatter.rs:364`) → it is simply never called again on restart.

So `agent-doc restart-supervisor` today restarts the **same** harness (claude→claude). Changing `agent:` and restarting will not bring up the new harness until the whole `agent-doc start` is relaunched. Both deliverables below depend on fixing this first.

There is **no** existing agent-change detection anywhere (`agent_changed` / `harness_changed` — no matches).

## Phase 1 — `#agentreloadrestart` (Rust core; do first)

Make the restart path re-resolve the harness from current frontmatter, and auto-restart at the next safe boundary when the resolved harness changed.

1. **Re-resolve on restart.** Before the restart loop respawns the child, re-read the session file frontmatter and recompute `HarnessConfig::from_context(fm, config)`. If it differs from the running `shared.harness`, rebuild the `child_factory` (and `harness_binary`, tmux submit profile, capability-proof harness) for the new harness, then respawn. This makes `agent-doc restart-supervisor` (and the Phase 2 menu command) actually switch harnesses. Touch points: `start.rs` restart loop + the `child_factory` construction; `SharedSupervisorState.harness`/`harness_binary` must become updatable (or recomputed per respawn).
2. **Auto-restart at boundary.** At the idle/turn boundary where `#supselfheal`/`#ctlrecycle` already decides to recycle (the `prompt_visible && !turn_active` gate — see `agent_doc_supervisor_auto_recycle` in `specs/06-config.md:85`, idle dispatch hook around `start.rs:490`), compare the current frontmatter-resolved harness against the running harness. If they differ, trigger the same restart used by `IpcMethod::Restart { mode: Fresh }` (`supervisor/ipc.rs:439`). Gate exactly like the auto-recycle: only at a turn boundary (never mid-turn), debounced at idle, and log `agent_changed from=<old> to=<new> action=restart_at_boundary`.
3. **Config knob.** Add `agent_doc_agent_change_restart` (env + frontmatter + project setting, default ON) mirroring `agent_doc_supervisor_auto_recycle` resolution, so the boundary auto-restart can be disabled.
4. **Tests (SimWorld/unit).** (a) restart re-resolves harness when `agent:` changed; (b) boundary check triggers restart only when harness differs; (c) never fires mid-turn; (d) knob off disables it; (e) no-op when `agent:` unchanged.
5. SPEC: document in `specs/06-config.md` (new knob) + the supervisor/boundary spec; VERSIONS entry.

Risk: this edits the live supervisor restart loop that hosts the running session — implement in a clean focused cycle with full `make check` + `make tmux-ci`.

### Real-world harm already observed (root cause of "agent-doc killed equityfundingsource.md turn mid-run")

Operator report 2026-06-18: changing `agent: claude → opencode` in `tasks/professional/equityfundingsource.md` **killed the in-flight claude turn**. ops.log evidence: `idle_queue_watch_drain ... harness=claude target=%83` (ts 1781762190) → `harness=opencode target=%90` (ts 1781762396); session `62fe1f41` migrated `[claude] %83` → `[opencode] %90`, abandoning the running `%83` turn. With no graceful agent-change restart, the route/sync/start path treated the harness mismatch as a reason to **cold-start a replacement pane**, killing the live turn.

**Added Phase 1 requirement — kill-guard.** When the frontmatter-resolved harness differs from the running same-session pane, the route/sync/start path must NOT cold-start a replacement pane / evict the in-flight turn. Instead defer to the graceful agent restart at the next turn/idle boundary (item 2 above), preserving the pane where possible and never interrupting an active turn. Add a guard that refuses to replace a busy same-session pane *solely* because `agent:` changed. Verify with `make tmux-ci` across two live panes (one mid-turn) that the switch waits for the boundary and does not kill the running turn.

## Phase 2 — `#restartagentmenu` (JB plugin; after Phase 1)

Add a "Restart Agent" command to the JB plugin menu (and mirror to VS Code if cheap).

- New `RestartAgentAction.kt` mirroring `RestartSupervisorProcessAction.kt` (`editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/`); register in `plugin.xml` (`<actions>`, alongside `AgentDoc.RestartSupervisorProcess` at lines 111-118) with text "Restart Agent" / description "Restart the agent harness for this document (reloads a changed `agent:` frontmatter)".
- Wire through `TerminalUtil` (mirror `restartSession` → `runRestartSupervisorCommand`, `TerminalUtil.kt:680`). After Phase 1, restart-supervisor re-resolves the harness, so this command brings up the new agent.
- Bump `pluginVersion` (`editors/jetbrains/gradle.properties:3`, currently `0.2.170`) per the per-edit bump rule; build both unsigned + signed zips (`./gradlew buildPlugin signPlugin`).
- Live gate: operator installs the signed zip + restarts IntelliJ, then verifies "Restart Agent" appears and restarts the harness.

Decide whether "Restart Agent" should replace or sit alongside "Restart Supervisor Process" (do not remove the existing one without operator confirmation — they are now distinct: supervisor-process vs harness-child semantics).

## Phase 1a — SHIPPED (detection + policy + knob + logging)

Landed (this turn, agent-doc 0.34.14 unreleased):

- **Knob** `agent_doc_agent_change_restart` — frontmatter + project config + env
  `AGENT_DOC_AGENT_CHANGE_RESTART`, resolver `resolve_agent_change_restart` /
  `agent_change_restart_enabled` (env > fm > project > default ON).
- **Pure boundary policy** `start::decisions::agent_change_restart_decision`
  (`None` / `Restart` / `WaitForBoundary`), gated exactly like
  `supervisor_recycle_action` (act only at a quiet dispatch-ready prompt, never
  mid-turn, only when the harness changed + knob on). Unit-tested.
- **Detection + logging** in the idle-queue watch: re-resolves the harness from
  CURRENT frontmatter each tick via `HarnessConfig::from_context`, and on a
  change (deduped per new harness) logs `harness_change_detected old=.. new=..
  gate=..` (ops.log) + `agent_restart_boundary_gate .. note=phase1b_execution_pending`
  (session log). This satisfies the "LOGGING (live-gate prove/disprove)"
  requirement so the operator can confirm detection live before execution lands.
- Tests: `agent_change_restart_decision_policy`, `resolve_agent_change_restart_precedence`.
  `make check` green.

## Phase 1b — REMAINING (execution; live-supervisor-critical, focused cycle)

Still to do (needs `make tmux-ci` + a focused clean cycle, per the Risk note above):

1. **Execution** — when the decision is `Restart` at the boundary, actually bring
   up the new harness: either re-derive the launch spec (env + base_args + codex
   network + capability proof) in the restart loop from current frontmatter, OR a
   no-preserve-child recycle variant (`supervisor_perform_reexec` currently
   *adopts* the live child — a harness change must spawn the new harness FRESH).
   Emit `agent_restart_performed`.
2. **Kill-guard** (the real-harm item above) — route/sync/start must NOT cold-start
   a replacement pane solely because `agent:` changed; defer to the boundary restart.
3. SimWorld coverage where modelable; live two-pane `make tmux-ci` verification.
