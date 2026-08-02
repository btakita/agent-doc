# agent-doc

Interactive document sessions with AI agents.

## Conventions

- Use `clap` derive for CLI argument parsing
- Use `serde` derive for all data types
- Use `serde_yaml` for frontmatter parsing
- Use `similar` crate for diffing (pure Rust, no shell `diff` dependency)
- Use `serde_json` for agent response parsing
- Use `std::process::Command` for git operations (not `git2`)
- Use `toml + serde` for config file parsing
- No async — sequential per-run
- Use `anyhow` for application errors
- **Prefer a Lazily reactive topology before imperative coordination (`#lazily-reactive-first`)** — For every agent-doc bug involving polling, retry loops, stale observations, repeated reconciliation, or a caller that must remember to recompute state, first sketch the lifecycle-scoped graph: external observations and durable effect receipts are `Source`s; applicability, authority, readiness, convergence, and other state-derived decisions are `Computed`s; idempotent I/O is driven by an `Effect`. Reuse `DocumentScope`, `TurnScope`, or `ProcessScope`; do not create a disconnected context merely to claim a value is reactive. Imperative commands live inside an `Effect` whenever a suitable lifecycle graph exists, including tmux, editor IPC, filesystem, and process-launch commands; the `Effect` publishes its receipt back to a `Source`, and producer edges—not a caller-owned retry loop—drive reevaluation. During review, treat imperative commands outside an available `Effect`, manual recomputation, timer polling for already-observable state, duplicated generation checks outside a computed join, and “call this again after X” recovery guidance as evidence of a missing reactive edge. This is opportunistic, not dogmatic: a direct command is the narrow exception for an actorless, truly one-shot boundary with no long-lived state relationship, and the implementation must state why.
- **Extract reactives during architecture review (`#reactive-computed-extraction`)** — Before implementation, inventory validators, applicability/readiness/authority checks, caches, and reconciliation helpers that callers invoke imperatively. Any derived fact whose inputs can change or that has multiple consumers belongs in a lifecycle-scoped `Computed`; adapters publish its input `Source`s and observe the projection. Effects publish typed, generation-fenced receipts back into the graph. Do not preserve a manual “call validator/recompute/retry after X” API merely because it already exists: migrate its consumers to the computed projection in the same change, or record the exact missing lifecycle edge as the blocker. The architecture-first closeout must name the imperative derived-state calls extracted and justify every remaining one-shot exception.
- **Events are the preferred reactive ingress (`#reactive-boundary-ingress`)** — Systems outside agent-doc need not be reactive. Prefer an event channel, subscription, filesystem watch, or other push stream that publishes typed, generation-tagged observations or receipts into lifecycle-scoped `Source`s. Use RPC-triggered observation only when no event stream exists; use bounded polling only when the external system exposes neither events nor a suitable trigger, and document that missing capability plus freshness evidence. Keep transport scheduling, freshness, and errors in the boundary adapter. Once the fact enters agent-doc, unify the internal model as reactive state: derive decisions in `Computed`s and drive idempotent `Effect`s from them instead of adding internal polling, RPC fan-out between policy owners, manual recomputation, or caller-owned retry loops.
- **Lazily state owns hot-path coordination (`#lazily-hot-path`)** — When a live project/document actor exists, ownership, compare-and-swap, leases/heartbeats, deduplication, queue/closeout serialization, and other state-derived decisions must be actor transitions against the in-memory Lazily projection. SQLite may persist actor-emitted facts and hydrate cold start, but callers must not independently reload or transact on SQLite to arbitrate a hot-path transition. `flock` is reserved for controller bootstrap or an explicitly actorless compatibility boundary, never document closeout. If actor state could be stale, route the fact producer through the actor; do not refresh SQLite at the decision seam.
- **NEVER swallow errors** — no `let _ =` on fallible operations. Always log at minimum a warning to stderr. Silent failures make bugs invisible and waste debugging cycles.
- **Behavioral fixes are packagable, not per-user agent memory** — when an agent-doc *session* behaves wrong (the agent stalled the queue, asked the wrong thing, mishandled closeout), fix it in the **product** so every user benefits: a binary heuristic, a `SKILL.md`/runbook instruction surface, or these development instructions. Do **not** resolve agent-doc behavior problems by writing a per-user agent-memory note — agent-doc ships to other people, and a memory only helps one operator. Memory is for facts about *a specific environment*, never for correcting shipped agent-doc behavior.
- **Diagnosis is not a deliverable (`#diagnose-then-fix`)** — when a session investigates a reported bug and lands on a root cause, the SAME session must fix it: implement, add regression coverage, run `make check`, build/install, and close the item. Handing back a well-written set of backlog items describing defects the session already understands is **not** closeout — it turns completed analysis back into unstarted work and forces the operator to ask for the fix again. "It spans several crates", "this deserves a focused cycle", and "I did not want to land a partial change" are stalls: land and verify what is proven, and leave only a genuinely blocked remainder. When one investigation surfaces several defects, fix them together and put any survivors at the TOP of `agent:backlog` so the queue takes them next. Only an operator-gated proof (a live editor/pane eyeball, an external approval) justifies leaving a diagnosed defect unfixed, and the item must state exactly what unblocks it.
- **Do the agent-doable deploy/release work without asking (`#deploy-just-do-it`)** — when a session produces a shippable agent-doc change, execute *every* agent-doable release/deploy sub-step autonomously: version bump (`Cargo.toml` + `pyproject.toml`), `VERSIONS.md` entry, `make check`, commit, `make install` (see `#installfulloom` below — `make install` is the iteration install; `make install-full` is a pre-release parity step, not something to run on every fix), push, `agent-doc admin recycle`, tag, and publish. Do **NOT** defer these to the operator, and do **NOT** ask "should I deploy/release?" — operator approval is assumed for anything an agent can do. The **only** operator-gated step is the genuine live-session eyeball (a human watching a real editor/pane prove the behavior); record it as a non-blocking `[operator-verify]` follow-up. An `[operator-verify]` item never licenses skipping the build/install/push/recycle/publish — it means "do all the agent-doable sub-steps now, leave only the live human check." Asking permission for agent-doable deploy work is itself the bug.
- **External CI is observed, never awaited (`#ci-no-closeout-wait`)** — local full-suite verification is the proof gate. After push, inspect the latest CI run once and report a queued, in-progress, failed, or successful status. Do not poll, watch, or keep the turn open for CI to finish unless the user explicitly asks you to wait. If an already-visible failure belongs to the change, fix it; otherwise close from local evidence and leave the external status explicit.
- **Operator-visible document text is authoritative** — Preserve user edits through Lazily-owned semantic response checkpoints and binary-owned `agent-doc respond --stream` turn resolution (`finalize` is a compatibility alias). Checkpoint only complete `### Re:` sections; never publish incomplete token prefixes to the document, and never recover, patch, or hook-closeout by replacing operator text with `content_ours`, a snapshot, an incomplete token capture, or a lazily visible-write receipt. Snapshots and incomplete token captures are backup/audit state, not hot-path authority; fail closed or retry through the editor instead.
- **Response closeout is atomic** — The complete final response, queue-head consumption, backlog/done mutations, snapshot, and commit succeed as one transaction or none becomes authoritative. `repair` is exceptional crash recovery, never part of a healthy response cycle.
- **Realtime operator steering must be surfaced verbatim and in aggregate, never dropped or thrashed (`#realtime-steering-verbatim` / `#realtime-steering-aggregate` / `#no-thrash-steering`)** — a document is realtime: the operator may add a prompt while the turn is active, and every item the operator adds must be addressed and worked on, not committed-and-ignored. When a committed cycle's document carries a fresh operator prompt, the binary must (a) hand the agent that prompt **verbatim** (the full text via `RealtimeSteering::verbatim`, not a first-line preview) and (b) instruct it clearly to address the prompt in the CURRENT turn — never a bare interrupt that reads like a failed closeout. **Steering is a concurrent aggregate, not a FIFO queue:** when the operator adds several prompts mid-turn, the binary must surface **all** of them at once — not just the first, and not drained one head at a time — so the agent processes the concurrent directives together and can find patterns across them. `agent_doc_diff::all_unstarted_prompt_bearing_changes_from_diff` returns every unstarted directive (with `first_*` defined as its head), `baseline_comparison::RealtimeSteeringSet` / `realtime_steering_all[_between]` aggregate them, `RealtimeSteeringSet::verbatim_aggregate()` concatenates them verbatim with a count header, and `session-check`'s `detect_unstarted_prompt_bearing_diff` (`realtime_steering_set_since_turn_baseline`) surfaces that aggregate. The clear instruction must say: the prior response is already committed in HEAD; re-run `agent-doc <FILE>` to continue; do **not** re-run finalize on the prior response, do **not** `--force-disk` (that clobbers the operator's live edits), and do **not** re-answer a prompt already committed in HEAD (the realtime replica reconciles the committed response back into the live buffer). This is what prevents the thrash loop (repeated preflight/finalize, empty cycles, force-disk clobbers) that a concurrent operator edit used to induce. Keep `session_check.rs`'s `realtime_steering_closeout_guidance`, `baseline_comparison::RealtimeSteering{,Set}`, `prompt_bearing.rs`, `is_committed_prompt_diff_interruption`, `SKILL.md`, and `runbooks/respond.md` aligned; the strategic direction is to back exchange + steering with the lazily graph (`SeqCrdt` exchange nodes, plus a reactive **observable set** — not a drain-one-head `QueueCell` — for steering) so concurrent editing reconciles as a CRDT instead of a bespoke merge.
- **A later response acknowledges only its directly associated prompt block (`#prompt-response-adjacency`)** — unstarted-prompt selection must retain unsuppressed diff encounter order. Substantive assistant prose, another prompt block, binary-owned component maintenance, or any other meaningful edit between an operator prompt and a later response heading keeps that prompt actionable. Explicit steering never uses the unmarked plain-prose compatibility heuristic. Marker-only `❯` normalization of text already present at the baseline is metadata, not a new prompt. A narrow structural repair that proves it carries the complete prompt forward (for example inline boundary-fragmentation repair) runs before the broad live-steering guard; potentially prompt-rewriting template normalization remains behind that guard.
- **Relative document paths resolve to absolute, non-empty project roots (`#ctrlrelroot`)** — `agent_doc_fs::find_project_root` anchors a relative path to the process working directory before walking ancestors. Controller clients must never turn the relative root sentinel `PathBuf("")` into a launch claim; the controller's empty-root admission check remains the fail-closed backstop. Tmux pane-CWD resolution first preserves a reported nonexistent path as stale fallback evidence, so an invalid relative CWD cannot borrow the controller client's unrelated process root.
- **Session-accretion compaction guidance is queue-aware (`#no-compact-prompt-during-queue-drain`)** — while an `agent:queue` is actively draining (`queue_active: true`), `session_accretion.rs` must NOT surface "ask the user before compacting" guidance: a self-driving queue is meant to run unattended, so a compaction question stalls the queued work. On an active queue the binary emits don't-stall guidance and compacts only on an explicit `agent_doc_auto_compact` opt-in; off the queue it asks before compacting. Keep `compaction_guidance` in `session_accretion.rs` the single source of that wording.
- **Operator-authored queue order is authoritative (`#qauthorder`)** — queue convergence must keep an operator-added queue line at the document slot the operator authored it in: never auto-bubble it to the top and never duplicate it. Holding the slot must not mutate the line's visible text (do **not** inject a `:pushpin:` the operator never typed); use a position-lock keyed off the line's stable identity instead. Free-text operator lines need the same convergence dedup the `do [#id]` heads already get (`#qdedupsync` is free-text-blind) so a CRDT/backlog-sync re-emit cannot leave a visible duplicate. Reconcile with `sort_prompts_by_priority` position-lock (`#queue-operator-pin-position-lock`), `annotate_manual_queue_additions` (`#7r2s`), and `#backlog-queue-append-stable` rather than regressing them. Plan: `tasks/agent-doc/plan-queue-preserve-operator-author-order.md`.
- **Operator queue deletions are authoritative; convergence never resurrects or duplicates them (`#qdedup-directive-twin`)** — a `do [#id]` directive twin counts as intentional "run it twice" ONLY up to its **snapshot-authored multiplicity**, exactly like free-text and bare-reference heads. `converge_queue_via_lifecycle` (`document_queue.rs`) must NOT give live directive twins an unbounded (`usize::MAX`) allowance: a byte-identical CRDT live-edit re-emit twin then masquerades as intentional and is never collapsed — the "you restored the queue items I deleted" duplication. Cap every shape at `authored.get(key).max(1)`. Free-text queue identity must be **marker-invariant**: both the convergence identity (`QueueItemIdentity::from_prompt` → `agent_doc_element_queue::strip_priority_markers`) and the CRDT cell-merge free-text keyer (`crdt.rs::normalize_item_text`) must strip the FULL leading marker set (`:pushpin:`/`📌`/`📍`/`🚧`/`⏭️` + `**pin**`/`**prioritized**` word forms) before keying, or an agent's `🚧 <text>` copy keys differently than the operator's bare `<text>` line and the base-delete guard resurrects a plainly-deleted free-text line. Keep the one shared `strip_priority_markers` (element-queue) as the single marker set across dedup, convergence, and cell-merge.
- **Backlog-to-queue closeout reconciliation is mutation-scoped and atomic (`#backlogqueuepopulation`)** — an add or ungate that makes tracked work actionable records the normalized id in `CycleState::pending_actionable_ids`; after current-head consumption, the same closeout insert-only mirrors exactly those ids into an explicit go-mode queue whose backlog opts into `queue`. The inserted block is ordered by hard `after=#id` dependencies, then priority, while all existing queue bytes stay untouched. Never replace this with an all-open-backlog sweep: unrelated open ids may have been deliberately deleted from the queue, and operator deletion remains authoritative.
- **All deterministic behavior in the binary** — document manipulation (compact, diff, merge, patch, write), snapshot management, git operations, and component parsing must live in Rust. The SKILL.md skill is the non-deterministic orchestrator (reads diff, generates response, decides what to write). Never implement deterministic document logic in the skill or ad-hoc scripts.
- **Harness arg resolution is explicit** — `agent_args` is the shared override, `claude_args` is Claude-only, `codex_args` is Codex-only, and `opencode_args` is OpenCode-only. Keep those precedence chains in `start.rs`, `frontmatter.rs`, `config.rs`, and the docs/specs aligned.
- **`tmux-router` is a live sibling development target in agent-loop** — when generic tmux pane/session mechanics move out of `src/agent-doc`, update `../tmux-router` in the same turn and keep the workspace cargo patch (`../.cargo/config.toml`) plus harness instruction surfaces aligned so local builds exercise the extracted code instead of the published crate.
- **Skill install content is part of the product contract** — changes in `src/skill.rs`, `SKILL.md`, bundled runbooks, or bundled OKF concepts must keep the installed `.claude/skills/agent-doc/SKILL.md`, `.codex/AGENTS.md`, `.opencode/skills/agent-doc/SKILL.md`, managed root `AGENTS.md` mirrors, harness runbooks, and harness OKF directories aligned. Claude/Codex/OpenCode hot-path instructions should render from one shared source surface, with differences limited to harness-specific invocation wording and frontmatter description. `audit-docs` must fail on generated agent-doc instruction surfaces that still carry managed frontmatter but no longer match the running binary, while preserving custom root `AGENTS.md` files. In particular, the shared Claude/Codex/OpenCode manual-repair guidance must distinguish inserting a missing user prompt from repairing a missed assistant response, route the latter through `agent-doc write --commit <FILE>`, and reject flows that stop after bare `agent-doc write`.
- **Compound `commit + push` turns must keep the session doc off manual repo commits** — when the user requests ordinary repo commit/push work inside an `agent-doc` turn, manual git commits may stage only the intended non-session repo files, must stop immediately on any stage failure, must verify the staged diff still matches that intended path set, and must commit only that validated set. The active session document still closes through `agent-doc respond <FILE>` (`finalize` compatibility alias) or `agent-doc write --commit <FILE>`, and the push happens after that binary-owned closeout commit lands. Keep `SKILL.md`, `README.md`, `SPEC.md`, and the bundled runbooks aligned on that ordering rule.
- **Completed backlog archive is `agent:done`** — reaped tracked work must be recognized only from the canonical `agent:done` component. `agent:backlog-done` and `agent:pending-done` are migration inputs, not runtime aliases.
- **Selective commit is conservative** — `git::commit()` builds the snapshot-selected session document in a private index rooted at the observed `HEAD`, advances `HEAD` by compare-and-swap, and never includes unrelated staged entries. It stages the snapshot, not arbitrary working-tree drift. The only absorbable out-of-band repair path is narrow agent-owned drift (`status`, `### Re:` response-block insertion, pending-ID superset) when the redacted component structure still matches. Plain user prompts must remain uncommitted. Already-committed historical response-block drift may repair the snapshot only when the working tree matches `HEAD` modulo transient boundary / `(HEAD)` markers, and that same self-heal also tolerates committed exchange-only prompt-prefix normalization on already-answered prompts (for example, historical `❯ do ...` vs committed bare `do ...` directly above a real `### Re:` block). Even under extreme snapshot/file drift, tracked docs must not wholesale re-sync the snapshot from the live file — reserve that bootstrap escape hatch for untracked scaffold snapshots only. After a successful commit, boundary cleanup must collapse the **snapshot** to the same clean shape as the committed blob. The **working tree** (and editor buffer via IPC) preserves `(HEAD)` annotations so the user sees which headings are new — preflight classifies `(HEAD)` differences as `boundary_artifact`, not user edits.
- **Route readiness stays in the binary** — `route.rs` owns pane prompt detection and trigger acceptance. Keep that logic resilient to shell startup noise / echoed command text, key it off real harness prompt shapes rather than generic shell `>` echoes, and do not push harness-readiness heuristics into the skill layer.
- **Managed capability proof stays out of pane transcripts** — `start.rs` must keep successful/failed managed proof events in the session log and surface the user-visible `[start] managed ... capability proof` summary through tmux `display-message` on the owned pane. Do not write those diagnostics to the child pane stdout/stderr stream where they can perturb prompt detection or the next agent input.
- **Route-owned boot/restart/auto-install stderr must never inherit the agent pane (`#restartstderrbleed`, `#restartstderrbleed2`, `#fresh-project-supervisor-log`)** — `agent-doc start` owns supervisor stderr setup in-process: resolve `agent_doc_supervisor_stderr_log` from project config, create its parent directories, open it append-only (default `.agent-doc/logs/supervisor-stderr.log`), fall back to a deterministic temporary project-keyed path when the primary cannot be opened, and then redirect **fd2 (stderr)** while the agent TUI renders through **fd1 (stdout)**. The shell command that boots `agent-doc start --route-owned` must contain no log-path argument and no `2>>` redirect. Any child the supervisor spawns during a restart/recycle/auto-install (notably `make install` in `run_auto_install_steps_once`, whose unsuppressed recipe echo goes to stdout) must have its stdout+stderr explicitly wired to the opened supervisor-log target and its stdin nulled — never `.status()`/`.output()` with inherited stdio, which sends build/recipe output straight into the live agent pane. Route child stdio through `auto_install_child_stdio` (`project_controller/rpc.rs`) and keep the fresh-project log, boot-command, and child fd-bleed regression tests green.
- **A surviving-child supervisor reexec is transport continuity, not `session_start` (`#supervisor-reexec-lifecycle-continuity`)** — consume the inherited child/PTY handoff before start admission. The replacement image must validate the existing controller document/session/pane binding, preserve its actor generation and runtime state, refresh only the supervisor lease, and adopt the child without firing start hooks or emitting an ownership transition. Binding drift fails closed before any replacement generation is allocated.
- **Route progress diagnostics must stay UTF-8 safe** — any stderr/status trimming of captured tmux lines in `route.rs` must truncate on char boundaries so Unicode prompt/status glyphs cannot panic a live reroute.
- **Starting actor reroutes promote only proven idle panes** — when the project controller still reports the authoritative actor as `starting`, route may promote it to `ready` only after the live pane shows a harness-specific dispatch-ready prompt, then use the normal managed/dispatch-only send path. If the prompt never becomes dispatch-ready, the route path must fail closed before sending tmux or supervisor input. Keep this aligned in `route.rs`, `README.md`, `SPEC.md`, and the installed harness surfaces.
- **Fresh route panes stay authoritative** — once `route.rs` creates a fresh pane for a document, later geometry-only registry churn must not hand dispatch back to an older same-session pane. Keep the fresh-pane authority rule aligned in `route.rs`, `README.md`, `SPEC.md`, and the session/tmux command spec.
- **A no-ack fresh pane with the trigger still in the composer is stranded, not idle (`#jbtsiftnosub2`)** — when a freshly-created route pane produces no document cycle, an empty dispatch-ready composer is a legitimate idle no-op (keep it), but a dispatch-ready composer still showing the injected trigger *unsubmitted* is the JB-created-fresh-pane "prompt added but not submitted" drift. Route must resubmit the stranded draft once (bare harness `Enter`) and re-check for a cycle ack; keep the session only if the resubmit acknowledges, otherwise record a `FreshStart` startup-miss and fail closed. The classification (`fresh_start_ack_outcome`, `pane_composer_has_pending_trigger`) lives in `agent-doc-controller`; the resubmit consumer lives in `agent-doc-route-io` (`startup.rs`, `startup_ready.rs`). Keep this aligned with `specs/08-session-routing.md`.
- **Dispatch-only proof scope must be explicit** — when `route.rs` reuses a live pane for `route --dispatch-only`, ops/status language must distinguish pane-input acceptance from dispatch-start proof. Hook-visible Codex requires routed submit proof and fails once with the accepted-but-unproven reason; Claude Code and OpenCode currently have no equivalent prompt-submit hook, so their dispatch-only success must be labeled accepted-only instead of implied consumed/submitted parity. Keep this aligned in route, docs, specs, and skill surfaces.
- **Late associated-pane proof still blocks auto-start** — if stale registered-pane recovery clears the old binding and only then a live legacy associated pane becomes provable, the normal route path must fail closed with explicit claim guidance instead of silently cold-starting a replacement or re-electing that legacy pane. Keep that boundary aligned in `route.rs`, `README.md`, `SPEC.md`, and the session/tmux command spec.
- **Stale startup-miss supersession follows the current file owner** — once a document is registered to a newer pane/session, later `start` / `route` / `sync` / `session-check` passes must clear the old pane's `startup_miss` marker from the current owner's later `session_start` provenance instead of staying keyed to the superseded `session_id`. Keep that rule aligned in `startup_miss.rs`, `README.md`, `SPEC.md`, and the harness instruction surfaces.
- **GC closes only unproven stale startup state** — `gc.rs` may close one-hour-old `starting` controller actors unless a live supervisor PID still has a fresh heartbeat for the same generation. A live but non-heartbeating `agent-doc start` process is not proof forever. It may prune old Codex blocked-stop diagnostics after seven days, but fresh blocked-stop records remain visible for closeout forensics. Keep this aligned in `project_controller.rs`, `README.md`, `SPEC.md`, and the core command spec.
- **Session operator state uses direct pane evidence** — `session_actor_cmd.rs` must keep `session status`, `session clear`, and `session interrupt-clear` aligned with the direct `alive-idle` / `alive-busy` / closed pane evidence. Idle direct evidence can repair stale busy actor/lease projection; busy direct evidence stays fail-closed unless the operator chooses the explicit interrupt-and-clear discard path.
- **Codex operator interrupt keys are state-scoped (`#codex-interrupt-clear-ctrl-g-opens-editor`)** — `send_operator_interrupt_sequence` may send `C-g` to a Codex pane only when the live capture proves a shell `reverse-i-search` / history-search state; in the normal Codex TUI composer `C-g` opens the external editor (`$EDITOR`, e.g. nvim) instead of interrupting, so `session interrupt-clear` and `restart-supervisor --force` fall through to `Escape` + `C-c` everywhere else. The same gating governs `route.rs`'s busy-existing-pane reroute interrupt (`attempt_busy_existing_pane_interrupt_recovery`, `#codex-route-busy-ctrl-g-opens-editor`): it sends `C-g` only when the authoritative busy `blocker_reason` is a shell search or a fresh whole-capture re-classification (via `dispatch_only_blocker_reason`) proves it — never on a bare timeout or active turn. Keep `session_actor_cmd.rs`, `route.rs`, `specs/07-session-tmux-commands.md`, and the editor-recovery safety net aligned.
- **Forwarded operator quit keys own the keepalive prompt** — when stdin-forwarded `Ctrl+D` or a terminating stdin-forwarded `Ctrl+C` reaches a Codex child, `start.rs` must return to the restart-or-quit prompt even if the run already committed a document cycle. Keep that rule aligned in `start.rs`, `README.md`, `SPEC.md`, and the supervisor/Codex specs.
- **Optimistic fresh-restart retries stay explicit** — when a routed Codex fresh-restart retry never regains a dispatch-ready prompt, the binary must record the resulting `startup_miss` against the original routed pane and preserve the canonical document path for later recovery instead of silently redirecting the retry through a replacement pane.
- **Passive mixed-root sync must preserve visible layout on blocked files** — keep `sync.rs`, `README.md`, `SPEC.md`, and the session/tmux command spec aligned on the fail-safe rule for `sync --no-autostart`: if any visible file remains blocked, skip tmux-router reconciliation and warn instead of collapsing the remaining foreign pane set into the authoritative `agent-doc` window, but still reselect an already-visible requested focus pane inside that preserved layout.
- **Manual Sync Tmux Layout runs doctor-backed repair first** — keep `sync.rs`, `README.md`, `SPEC.md`, editor specs, and the session/tmux command spec aligned on the full `agent-doc sync` repair invariant: full sync must use the same file-scoped repair path as `session doctor <FILE> --repair`, including stale explicit `stash` window targets, and must finish with `0:agent-doc`, `1:stash`, and adjacent overflow `N:stash` windows, renaming `stash-*` aliases back to `stash`. Passive `sync --no-autostart` remains non-destructive and must not run this repair step.
- **Passive sync must also avoid attach-first pane growth around protected closeouts** — keep `sync.rs`, `README.md`, `SPEC.md`, and the session/tmux command spec aligned on the rule that `sync --no-autostart` preserves the current visible layout when a missing requested pane would require a new attach but already-visible unwanted panes are still protected by open cycles, while still handing focus to an already-visible requested pane.
- **Passive editor sync should take the fast handoff path after the sync lock** — keep `sync.rs`, `README.md`, `SPEC.md`, and the session/tmux command spec aligned on the `sync --no-autostart` ordering: acquire the bounded sync lock before selecting a hidden actor pane, then prefer latest matching pane first, alive exclusive registered pane second, and cold-start only when neither exists. Do not reinsert the heavier supervisor/process-tree recovery into the common tab-switch path.
- **Sync lock contention must recover stale orphaned owners** — keep `sync.rs`, `README.md`, `SPEC.md`, and the session/tmux command spec aligned on reaping stale orphaned `agent-doc sync` processes that still hold `.agent-doc/sync.lock`, then retrying lock acquisition before reporting contention.
- **Fresh session-log ownership beats stale process-tree fallback** — keep `sync.rs`, `README.md`, `SPEC.md`, and the session/tmux command spec aligned on the live-owner precedence rule: prefer path/supervisor proof and the latest open session-log owner before generic same-file process-tree matches, so stale panes cannot steal authority back from a fresh reroute.
- **A pane never owns a document from a different git repository (`#cross-repo-owner-guard`)** — nearest-`.agent-doc` resolution does NOT isolate a nested git submodule that has no local `.agent-doc/`: `find_project_root` collapses it up into the superproject's `.agent-doc/` keyspace, so every `.agent-doc`-root equality check compares `superproject == superproject` and cannot tell a `src/lazily-rs` submodule pane apart from a superproject `tasks/…` document. That blind spot let a supervisor restart re-attach a submodule agent session onto a superproject document's pane. Owner resolution must draw the durable boundary at the **git repository**: `reject_cross_document_owner_pane` (the single chokepoint for `find_live_owner_pane*` and `find_normal_path_owner_pane*`) rejects any candidate pane whose working-directory `git rev-parse --show-toplevel` differs from the document's git toplevel. Keep it a strict tightening — an unknown toplevel on either side is never foreign, so same-repo owners are never spuriously cold-started. Keep `sync.rs`, `agent-doc-tmux-io` (`pane_current_path`), `specs/08-session-routing.md`, `README.md`, and `SPEC.md` aligned.
- **Never claim or reap a harness session agent-doc did not start (`#bare-foreign-session-guard`)** — the cross-repo guard above does **not** close the bare-foreign-session gap, and must not be described as if it does. An operator who runs `claude`/`codex` by hand **inside the project** has the same git toplevel as the document, so every same-repo check passes it. `cmdline_owns_other_document` additionally requires an `.md` token, so a bare harness pane answers "owns no document" — and callers read that absence of ownership proof as *permission* to claim and reap. Operator-reported 2026-07-19: a pure Claude Code session in the project directory (tmux session 1, window 0) was hijacked and its panes killed. **Ownership must be proven, never assumed.** `cmdline_is_unmanaged_harness_session` (`agent-doc-controller/src/command_line.rs`) recognizes a harness process carrying no bound document that did not come through the `agent-doc` binary; `process_tree_runs_unmanaged_harness_session` lifts it to a process tree, and `pane_runs_other_document_owner` treats such a pane as foreign so owner election and reaping leave it alone. agent-doc's own panes stay *managed* and remain reapable. When a check cannot prove agent-doc owns a pane, the safe answer is to leave the pane alone.
- **Response replay is transaction-backed** — final parsed responses must be durably persisted in `state.db` before write/hook emission. Recovery resumes the same response-cell intent from its recorded phase and semantically rebases it onto Lazily current text; terminal cycles never expose retained history as an active capture.
- **Bounded accretion context must anchor to the edited turn** — when session-accretion prompt packing replaces the full exchange tail, `prompt_context.rs` must select the `### Re:` block at the prompt's actual position in `exchange`: enclosing response for inline prompt edits, immediately previous response for tail follow-ups, with older unrelated turns left on-demand.
- **`respond` is the strict response happy path** — `agent-doc respond <FILE>` (`finalize` compatibility alias) must fail before mutating a non-git document and must not report success unless the cycle reaches `committed`. Keep that contract aligned in `main.rs`, `write.rs`, and the command docs.
- **Run must recheck after pre-commit repair** — when `git::commit()` repairs an already-committed missed patchback and no prompt-bearing diff remains, `run.rs` must fail before child-agent dispatch and point to `agent-doc write --commit <FILE>` instead of submitting an empty or stale prompt.
- **Post-commit user follow-ups are not missed-response repair** — when `git::commit()` sees `snapshot == HEAD` plus a later user follow-up prompt, keep that prompt uncommitted for the next cycle and log `post_commit_user_follow_up`; do not label that safe shape as `prior_patchback_without_response_body` or `out_of_band_write`. Keep `git.rs`, `README.md`, `SPEC.md`, and closeout specs aligned.
- **Direct-exec post-write guard stays explicit; managed closeout is turn-reactive (`#sessioncheckturnreactive`)** — keep `SKILL.md`, `runbooks/commit.md`, `runbooks/harness-invocation.md`, and `specs/07-commands.md` aligned on the Codex/OpenCode/direct-exec requirement to run `agent-doc session-check <FILE>` after `finalize` or manual `write --commit`, and fail closed if it reports an open cycle, a prompt-only exchange tail with no assistant response, or a likely direct assistant patchback that bypassed the binary write path. Connected clients and managed orchestration consume strict finalize's terminal report and must not invoke a redundant companion check; the next turn's preflight evaluates those guards and opening the newer cycle supersedes any older closeout owner immediately. The only self-heal exception is already-committed historical snapshot drift proven by `HEAD`.
- **Cold recovery projections never become hot-path authority** — keep session-check, snapshot, and repair code aligned on the rule that `state.db` owns closeout intent/state and Lazily owns the current document. Optional recovery projections may aid audit or reconstruction, but their presence, absence, or freshness must never elect delivery, overwrite an editor buffer, or advance the write state machine.
- **Codex hook backstop is binary-owned** — keep `src/codex_hook.rs`, `src/skill.rs`, `SKILL.md`, `runbooks/harness-invocation.md`, `README.md`, and `SPEC.md` aligned on the installed `.codex/hooks.json` / `.codex/config.toml` contract: `UserPromptSubmit` tracks the active document, and `Stop` first tries to finish the response cycle deterministically from `last_assistant_message` via the normal repair/write/commit path before falling back to capture-and-block / fail-closed behavior. Empty `last_assistant_message` on an open cycle must still fail closed with diagnostics and tracked-prompt recovery because tool-only/authentication steps (for example MCP OAuth / `authenticate`) are sub-steps, not successful closeout boundaries.
- **Required SSH drift detection must include bare socket EPERM when SSH context is proven** — keep `src/agent/codex.rs`, `README.md`, `SPEC.md`, and the bundled skill surfaces aligned on the rule that a resumed Codex `command_execution` event with output like `socket: Operation not permitted` still counts as required-SSH capability drift when the same event proves an `ssh` command against a declared `required_ssh_targets` entry. Do not collapse localhost/CDP `Operation not permitted` signatures into the SSH path.
- **Required SSH fresh retries must discard stale resumed prelude text** — keep `src/agent/codex.rs`, `README.md`, `SPEC.md`, and the bundled skill surfaces aligned on the rule that resumed Codex streams for SSH-gated docs buffer early assistant chunks until required SSH is proven safe or the turn completes, so a required-SSH fresh retry can drop stale prelude text from the discarded resumed session.
- **Response ordering is part of the contract** — keep the same files aligned on the rule that requested implementation / verification / build-install work finishes before final response persistence, and that only `session-check`, recovery, and final reporting remain after `finalize` / `write --commit`.
- **Harnesses own full-suite verification** — keep `Makefile`, `SKILL.md`, `SPEC.md`, and installed harness instruction surfaces aligned on the rule that agents explicitly run the full project verification suite after changes instead of relying on a git pre-commit hook to do it implicitly.
- **Temporary Rust source mutations must restore Cargo freshness (`#cargo-mutation-freshness`)** — mutation probes that patch a tracked `.rs` file must restore the original bytes in a `finally`/`trap`, then immediately refresh that file's mtime (`os.utime(path, None)` / `touch`) before any later Cargo command. Moving a backup over the source or using `cp -p` preserves the backup's old mtime; Cargo can then reuse the rlib compiled from the mutated source even though the working tree is clean, producing impossible missing-symbol/type failures. After fixing a probe that restored an old mtime, rerun the entire probe sweep because earlier verdicts are untrusted. If a clean source tree reports compiler errors contradicted by the visible definitions, refresh the affected restored sources and use the narrowest `cargo clean -p <package>` that invalidates the stale crate before escalating to one full `cargo clean`.
- **Tmux CI review is part of test-bearing turns** — whenever a turn runs or changes tests, review the latest GitHub Actions CI run for the tmux leg (`make tmux-ci`). Check CI with `gh run list --workflow CI --limit 1` to make sure it is not already red; if CI reports tmux failures after runner startup, reproduce locally with `make tmux-ci`, fix the issue, and add or update deterministic SimWorld coverage for the failure class when that behavior can be modeled without live tmux. If the latest run is queued or in progress, record that status and continue with local verification evidence instead of blocking the turn for CI completion; do not use `gh run watch` as a closeout gate unless the user explicitly asks. Empty-step jobs with no logs because GitHub never started a runner (for example billing/spending-limit exhaustion) are external CI-start blockers, not code/tmux regressions; record the annotation and continue with local verification evidence. Keep `SKILL.md`, `SPEC.md`, `README.md`, and installed harness surfaces aligned on this rule.
- **Preflight is a stable binary contract** — keep `src/preflight.rs`, `SKILL.md`, `.claude/skills/agent-doc/SKILL.md`, and the top-level docs aligned on the interrupted-cycle guard (`preflight_started`, `response_captured`, and `write_applied` count as open; only recoverable or stale-empty `preflight_started` cycles auto-close), tier fields (`effective_tier`, `required_tier`, `suggested_tier`, `model_switch`, `model_switch_tier`), and `agent_model` short-name attribution.
- **Oversized specs should split behind a stable index** — when a spec or instruction file grows past a clean single-purpose boundary, follow [runbooks/split-spec-files.md](runbooks/split-spec-files.md): keep the existing numbered entrypoint as an index, move normative detail into focused sibling files, update the top-level catalogs instead of growing another monolith, and keep that ownership rule aligned across managed Claude/Codex/OpenCode harness surfaces while leaving custom root instruction files opt-in unless they still match the generated baseline.
- **FFI-first for editor integration (Shared Foundation pattern)** — when adding features that editors need (sync debounce, busy guards, IPC listeners, layout validation), implement in the FFI layer (`ffi.rs`) first, then call from editor plugins via JNA/FFI. Editor plugins should be thin event reporters — layout changed, file selected, etc. Business logic (debouncing, locking, socket listeners, idempotency checks) belongs in the shared FFI library, not duplicated across IntelliJ/VS Code plugins. **Ontology:** Both the FFI library and each editor plugin are **Systems** with their own **Perspectives**. Each exposes an **Interface** (C ABI, JNA bindings) — the defined boundary through which Systems communicate. The Shared Foundation pattern places shared logic at the broadest **Scope** (FFI library) so all consumer Systems access it through their Interfaces. **Test:** "Does this feature need to work in >1 editor?" → implement in FFI. Example: socket IPC listener lives in `ffi.rs` (`agent_doc_start_ipc_listener`), not in `PatchWatcher.kt`.

## Reactive graphs are lifetime-typed; disconnected islands are an anti-pattern (`#stategraphjoin`)

Every ad-hoc `Context` / `ThreadSafeContext` / `AsyncContext` constructed inside a
type is a **private graph island**. Nothing outside can derive from its cells,
invalidation never crosses it, and a `Computed` created in one is Computed in name
only — it recomputes in isolation and nothing can depend on it. Audited 2026-07-25:
**50 sites** across the workspace call `*Context::new()` directly, and
`agent-doc-state-backbone` alone held **nine** state machines that each minted their
own context in their constructor.

Two smells prove you are looking at an island:

- a "reactive" value that a caller still has to recompute, or that is rebuilt per
  query and dropped (constructing a whole context to answer one comparison is
  strictly worse than the comparison);
- a derived fact that only updates because some code path remembered to call an
  update, rather than because its inputs changed.

**A shared context is necessary but not sufficient — the scope must be typed.** A
bare `&ThreadSafeContext` parameter lets a cell join *any* graph, including one with
the wrong lifetime, and neither mistake is caught at runtime:

- a document-scoped cell placed in a turn graph is torn down at closeout and silently
  stops updating;
- a turn-scoped cell placed in a document graph leaks across turns.

Both surface much later as a stale value, which is the most expensive failure shape
this codebase has. So the scope is a **type**, and the type names the lifecycle:
`DocumentScope` (one open document), `TurnScope` (one response cycle, dropped at
closeout), `ProcessScope` (controller/supervisor lifetime). Dropping a scope drops
its context and every cell in it, so teardown *is* the scope's lifetime rather than a
separate deregistration step.

**Rules.**

- New document/turn/process state joins the matching scope: constructors take
  `&DocumentScope` (or the appropriate scope type), never a bare context.
- A bare `*Context::new()` inside a type is allowed only for a genuinely standalone
  pure-transition helper — typically the `X::new()` kept beside `X::new_in(scope, ..)`
  for unit tests. If a long-lived owner holds it, it is an island; fix it.
- Do not "make it a `Computed`" by building a context per call. If there is no scope
  to join yet, joining one *is* the work — say so rather than shipping the shape
  without the properties.
- The same applies to `Effect`: a side effect that must be *called* at the right
  moment (a startup fan-out, a settle emission) is the imperative form. Gated on a
  derived signal in a real scope, it fires whenever the signal says so and is
  idempotent when the signal is empty — which removes the "remember to call this"
  failure mode entirely.

**The scope types live in `agent-doc-state-scope`,** a leaf crate with nothing but
`lazily` under it. They started in `agent-doc-state-backbone`, which depends on
`agent-doc-turn` — so the crates holding the remaining islands could not name a scope
without a dependency cycle. `agent-doc-state-backbone` re-exports them, so existing
`agent_doc_state_backbone::DocumentScope` paths keep working.

**The rule is code-enforced** by `agent-doc-state-scope/tests/architecture_scope_guard.rs`.
It cannot tell an island from a helper — that needs the author's intent — so it
requires the intent be *written*: every non-test `*Context::new()` must carry a
`// #stategraphjoin-allow: <reason>` comment within six lines above it. If you cannot
write a reason, that is the finding; join a scope instead. "It is only a small
context" is not a reason — an island is an island at any size.

**Reactive vocabulary is `Source` / `Computed` / `Effect`, read with `get` (`#lzcellkernel`).**
`signal` / `get_signal` / `ThreadSafeSignalHandle` are the pre-kernel two-node shape (a
memoized slot plus a puller effect). They still compile, which is exactly why the same
guard bans them: nothing else stops a new caller from reaching for the old form. Same
escape hatch, different marker — `// #lzcellkernel-allow: <reason>`. The single-source
fix is `#[deprecated]` on those methods in `lazily-rs` (matching the existing
`get_cell`/`set_cell` deprecations); the thread-safe half is blocked on an eager
`Computed` existing there.

**Both thread-safety families are scoped (`#stategraphjoin-local`, closed).** The
original scopes wrapped `ThreadSafeContext` only, which left single-threaded
`lazily::Context` state with no scope of the right kind to join — the thread-local
memo caches in `ops-log-io`, `tmux-io`, `supervisor-io`, `sync-io`, plus
`prompt-context`'s projection, all named that gap in a comment. A comment is not a
lifetime: it drops nothing and does not stop the next `Context::new()`. So
`agent-doc-state-scope` also ships `LocalDocumentScope` / `LocalTurnScope` /
`LocalProcessScope` and `LocalReadScope`, and those sites join them.

`LocalReadScope` names the lifecycle those memo caches actually have: a **bounded read
scope**, narrower than a turn — valid until the underlying system is mutated, not until
the turn ends. Re-taking the scope *is* the invalidation, which is why they never
needed an eviction policy.

One difference is load-bearing: `ThreadSafeContext` is `Clone`, so a thread-safe scope
is shared by handing out clones (`X::new_in(&DocumentScope, ..)`). `lazily::Context` is
`Rc`-based and **not** `Clone`, so a local scope is *owned* by the state whose lifetime
it is; a caller that needs the same graph borrows the owner's `ctx()`.

The remaining `#stategraphjoin-allow` markers are the genuinely exempt shapes, not
deferred work: an `X::new()` kept beside `X::new_in(scope, ..)` for unit tests, the
`run-context-io` scope factory (the context *is* the returned value), the `RelayHub`'s
own graph, and bounded per-call pure transforms such as `adstatechart`'s read-only
advisory snapshot.

### Absence of evidence is not evidence of change (`#idlerevisionreactive`)

A cheap probe that gates expensive work must keep "I did not look", "I looked and
got no answer", and "here is the answer" as **distinct** outcomes. Collapsing them
into `Option::None` and reading `None` as "changed" inverts the system's behavior
at the worst moment: the supervisor's idle watch answered a controller that could
not serve a *cheap* revision probe by issuing the *expensive* ones every 500ms
instead of every 60s — up to 120x the intended load, generated by the wedge and
feeding it. That is what the write path then sees as
`controller_model_backpressure` and a 5s authority-resolve timeout.

Losing a change is the risk this trades against, and it was already covered:
`IDLE_WATCH_FULL_RECONCILE_INTERVAL` reruns the authoritative projection anyway.
Invalidate-on-failure bought no safety that was not already there.

**Backoff is derived, not stamped.** A retry deadline computed from *when* health
changed cannot be a `Computed` — it needs a clock input that invalidates the graph
continuously — so it ends up as an effect writing a variable, which is a
`Computed` in disguise sitting outside the graph. Count **skipped observations**
instead: `should_probe_controller` is then a pure function of the observation
stream, self-regulating (obeying it feeds the stream that clears it), and needs no
clock, no cell, and no effect. Never gate the probe on degraded health alone —
suppressed ticks deliberately hold the unresolved streak, so that latches and the
controller is never asked again.

**Effects are for effects.** `agent-doc-supervisor/src/idle_revision.rs` keeps
exactly one: writing the diagnostic. If an `Effect`'s whole body assigns a value,
it should have been a `Computed`.

**Fold history in plain data, not across cells.** `RevisionTracking` + `advance`
is the history-dependent part as a pure total function, with the `StateMachine`
cell holding it and `Computed`s over its state handle. The first draft wrote three
`Source`s by hand from one method and shipped two ordering bugs — a baseline
advanced in the tick it was compared against, and a baseline that lagged two
observations when an unanswered probe sat between two real ones. Neither is
expressible once the history is one value advanced by one function.

## Durable effect sinks (`#lzdurablesink`)

Durable storage is an **effect sink**, not a transition authority. This restates
and sharpens `#lazily-hot-path` as the cross-family rule
([`lazily-spec` § Durable Effect Sinks](https://github.com/lazily-hub/lazily-spec/blob/main/docs/durable-sinks.md);
formal backstop `lazily-formal/LazilyFormal/DurableSink.lean`):

- Live coordination — claims, heartbeats, phase changes, deduplication,
  supersession, compare-and-swap — happens in Lazily state. A sink is **write-only
  with respect to transition authority**. Loading and migration belong to a
  separate startup hydrator, never the decision seam.
- Persisting the **latest projection** = idempotent upsert of the settled epoch
  from an `Effect` / `AsyncEffect` (a batch `A → B → C` persists only `C`).
  Persisting **ordered history** (every accepted fact, lossless) = a
  `TopicCell` / `Outbox` drain (append / replay-from-cursor / `ack_through`) — not
  ordinary effects.
- Success advances a monotone `durable_through(epoch)`; a sink failure stays in
  live state as `pending` / `retrying` / `backpressured`. `Ephemeral`-plane values
  must not enter a durable sink (reuse the `Durable` marker).

**Architecture-review prompt (agent harness).** When a change combines a
hot-path transition with a SQLite `SELECT`, storage compare-and-swap, or `flock`,
ask — before implementing — whether the operation is really **cold hydration**
(startup, before the runtime is live), **actorless bootstrap** (the compatibility
boundary in `#lazily-hot-path`), or a genuine sink write routed through the live
actor. A storage read/CAS/lock on the decision seam is the inversion this rule
forbids; route the fact producer through the actor instead. This is a design
review question, not a generic AST linter.

## Binary vs Agent Responsibility

See [README.md](README.md) for the full responsibility table.

**Rule of thumb:** If the operation can be unit-tested with fixed inputs → binary. If it requires understanding natural language → skill.
- **Inline component attributes:** `<!-- agent:name patch=append max_lines=50 -->` — patch mode and max_lines are configurable on the tag itself. `mode=` is accepted as a backward-compatible alias for `patch=`; `patch=` takes precedence if both are present. `max_lines=N` trims content to the last N lines after patching (0 or absent = unlimited). Precedence: inline attr > `components.toml` > built-in defaults.
- **`agent_doc_format: inline`** is the canonical name for the old "append" format (`append` accepted as backward-compat alias). Template mode uses components; inline mode uses User/Assistant blocks.

## Module Layout

Use this layout when adding modules. Add new subcommands in their own file, wired through `main.rs`.

```
src/
  main.rs           # CLI entry point (clap derive)
  submit.rs         # Core loop: diff, send, merge-safe write, snapshot, git
  init.rs           # Scaffold session document; no-arg mode initializes project (.agent-doc/ dirs + SKILL.md)
  reset.rs          # Clear session + snapshot
  dedupe.rs         # Remove consecutive duplicate response blocks
  diff.rs           # Preview diff (dry run) + comment stripping
  clean.rs          # Squash git history
  agent-doc-element/src/element.rs # Element parser (<!-- agent:name --> markers) + name validation
  patch.rs          # Replace/append/prepend component content, config + shell hooks
  watch.rs          # Watch daemon: auto-submit on file change with debounce + loop prevention (reactive mode for stream docs)
  frontmatter.rs    # YAML frontmatter parse/write
  snapshot.rs       # Snapshot path/read/write
  git.rs            # Commit, branch, squash (includes `commit` subcommand + narrow missed-patchback self-heal)
  config.rs         # Global config (~/.config/agent-doc/config.toml)
  sessions.rs       # Transactional session-registry adapter + Tmux struct
  route.rs          # Route harness-specific agent-doc triggers to the correct tmux pane (pub auto_start for sync.rs)
  codex_hook.rs     # Codex UserPromptSubmit/Stop hook bridge + active-doc tracking
  start.rs          # Start configured agent harness inside tmux pane
  claim.rs          # Claim document for current tmux pane
  focus.rs          # Focus tmux pane for a session document
  layout.rs         # Arrange tmux panes to mirror editor split layout
  outline.rs        # Markdown section structure + token counts
  prompt.rs         # Detect permission prompts from Claude Code sessions (strip_ansi is pub(crate))
  skill.rs          # Manage bundled SKILL.md + harness-specific installed content (e.g. Codex AGENTS.md and runbooks)
  install.rs        # System-level setup: check prerequisites (tmux + agent CLI) and install editor plugins
  resync.rs         # Validate controller bindings, remove dead panes, detect wrong-session/wrong-process panes (--fix [--session <target>])
  session_cmd.rs    # Show/set configured tmux session with pane migration
  history.rs        # Exchange version history from git + restore
  upgrade.rs        # Self-update via GitHub Releases / PyPI
  plugin.rs         # Editor plugin install/update/list via GitHub Releases
  write.rs          # Write command: parse patches, IPC-first writes, disk fallback
  template.rs       # Template mode: patch parsing, apply_patches, boundary lifecycle
  boundary.rs       # Boundary marker management (insert, remove, reposition)
  crdt.rs           # CRDT foundation (yrs-based conflict-free merge)
  merge.rs          # 3-way merge + CRDT merge path
  stream.rs         # Stream command: recovery checkpoints + one final CRDT write
  ffi.rs            # C ABI exports for editor plugins (JNA/FFI); ffi_git_commit unsets GIT_DIR/GIT_INDEX_FILE/GIT_WORK_TREE so it works correctly when called from git hook contexts
  ipc_socket.rs     # Socket-based IPC (Unix domain sockets via interprocess crate)
  lib.rs            # Library target re-exports
  capture.rs        # Durable response-capture ledger + replay hash validation
  repair.rs         # Orphaned pending/captured response detection + recovery
  compact.rs        # Exchange compaction (archive + truncate)
  convert.rs        # Bidirectional format conversion (inline ↔ template)
  extract.rs        # Extract exchange sections to new documents
  undo.rs           # Undo last response (pre-response snapshot restore)
  mode.rs           # Document mode resolution (format + write strategy)
  autoclaim.rs      # SessionStart hook: auto-claim documents
  commands.rs       # List available commands for plugin autocomplete
  hooks.rs          # Cross-session hook integration (fire_post_write, fire_post_commit, capture refs)
  hook_cmd.rs       # CLI subcommands: agent-doc hook fire/poll/listen/gc
  ops_log.rs        # Best-effort operational logging to .agent-doc/logs/ops.log
  cycle_state.rs    # Persisted per-document cycle phase/hash state for interrupted-cycle enforcement
  sync.rs           # Sync pane state between editor and tmux (reconciler always runs, no early exits, column memory)
  preflight.rs      # Pre-agent checks: layout check, repair, commit, claims, diff, document read → JSON
  model_tier.rs     # Re-export shim for agent-doc-model-tier (tier selection and model switch scanning)
  agent/
    mod.rs          # Agent trait
    claude.rs       # Claude backend (Agent + StreamingAgent)
    junie.rs        # Junie backend (Agent + StreamingAgent)
    streaming.rs    # StreamingAgent trait + stream-json parser
  terminal.rs       # Launch external terminal with tmux session
  queue_dispatch.rs # Classify orchestration items as prompt/command; dispatch commands via supervisor IPC, tmux, or inline
  parallel.rs       # Parallel fan-out with git worktrees
  worktree.rs       # Git worktree management for parallel sessions
  audit_docs.rs     # Audit instruction files (via instruction-files crate)
editors/
  jetbrains/        # IntelliJ plugin (Kotlin/Gradle)
  vscode/           # VS Code extension (TypeScript)
```

## Release Process

1. Run `make release-version VERSION=<version>` to project the version across
   every workspace package, internal path constraint, `Cargo.lock`,
   `pyproject.toml`, and both `SKILL.md` copies. Do not bump these surfaces
   manually.
2. Update `VERSIONS.md` with a new version entry summarizing the changes
3. `make check` (clippy + test)
4. `make install-full` — install a full release-profile local build and verify the changed behavior end-to-end (the agent runs `make check` + automated checks as the verification; do not wait on a human).

   **`#installfulloom`: run this ONCE, at release time — never as the per-fix install.** `[profile.release]` is `lto = "fat"` + `codegen-units = 1` over a 144-crate workspace, which fat-LTO collapses into a single enormous LLVM process; repeated runs can OOM the machine (observed 2026-07-18: a session that invoked it ~10 times while iterating was killed with SIGKILL/137, losing the live session). For the edit → install → recycle loop use **`make install`**, which builds `release-local` (`lto = off`, `codegen-units = 256`, incremental) and installs the same binary + cdylib + editor packages.
5. **No operator gate on agent-doable steps (`#deploy-just-do-it`):** proceed straight through steps 6-9 without asking. The only operator-gated step is a live human eyeball of the changed behavior in a real editor/pane — record it as a non-blocking `[operator-verify]` follow-up; it never blocks the build/install/push/publish/recycle.
6. Branch → PR → squash merge to main (or commit + push to main directly in this dogfooding repo)
7. Tag: `git tag v<version> && git push origin v<version>`
8. `maturin publish` (PyPI); every agent-doc Cargo package has `publish = false`
9. `gh release create v<version> --generate-notes` with prebuilt binary (GitHub Release)

## Agent Backend Contract

Each agent backend implements: take a prompt string, return (response_text, session_id).
The prompt includes the diff and full document. The agent backend handles CLI
invocation, JSON parsing, and session flags.

### StreamingAgent Contract

Streaming backends implement `StreamingAgent::send_streaming()` → `Iterator<StreamChunk>`.
Used by `agent-doc stream` for real-time generation with one final write-back. Currently only `claude` supports streaming
(via `--output-format stream-json`). Each `StreamChunk` has cumulative text, optional
`thinking` content, `is_final` flag, and optional `session_id` on the final chunk.

## Stream Mode

Stream mode (`agent_doc_format: template` + `agent_doc_write: crdt`) enables real-time agent generation with CRDT-based conflict-free final merge. Partial output never enters the document. Legacy: `agent_doc_mode: stream` is still supported as a deprecated alias.

**Usage:** `agent-doc stream <FILE> [--interval 200] [--agent claude] [--model opus] [--no-git]`

**How it works:**
1. Validates document uses CRDT write strategy (`resolved.is_crdt()`), reads `StreamConfig` from frontmatter
2. Computes diff, builds prompt requesting patch-block format
3. Spawns streaming agent (`claude -p --output-format stream-json`)
4. Buffers accumulated text outside the document; timer ticks may update only cold recovery projections
5. On completion: validates and writes the complete response once, then saves CRDT state + snapshot, updates resume ID, and optionally commits

**Frontmatter:**
```yaml
agent_doc_format: template
agent_doc_write: crdt
agent_doc_stream:
  interval: 200           # stream polling/final flush interval (ms), default 200
  strip_ansi: true        # strip ANSI codes from output
  target: exchange        # target component name
  thinking: false         # include chain-of-thought (default: false)
  thinking_target: log    # route thinking to separate component (optional)
```

### Chain of Thought

Stream mode can capture the agent's chain-of-thought (thinking blocks) from Claude's
`stream-json` output. Controlled by `thinking` and `thinking_target` in `StreamConfig`:

| Config | Behavior |
|--------|----------|
| `thinking: false` (default) | Thinking blocks silently skipped |
| `thinking: true` (no `thinking_target`) | Thinking interleaved in target component as `<details><summary>Thinking</summary>...</details>` |
| `thinking: true` + `thinking_target: log` | Thinking routed to separate `<!-- agent:log -->` component; response goes to target |

**Parser:** `extract_assistant_content()` in `streaming.rs` extracts both `"type": "text"` and
`"type": "thinking"` content blocks. Thinking is buffered separately (`thinking_buffer`) and
is published only with the complete final response.

Use this frontmatter structure to enable thinking with a separate log component:
```markdown
---
agent_doc_format: template
agent_doc_write: crdt
agent_doc_stream:
  target: exchange
  thinking: true
  thinking_target: log
---
<!-- agent:exchange -->
User prompt here.
<!-- /agent:exchange -->
<!-- agent:log -->
<!-- /agent:log -->
```

### Flush Behavior

Stream flushes use the normal mode resolution chain (inline attr > `components.toml` > built-in default). `run_stream()` no longer hardcodes replace mode for exchange — mode resolution applies normally.

Implementation: `flush_to_document()` uses `template::apply_patches()`.

### Lazily-First Writes

All write paths (`run`, `stream`, `write`) capture intent in `state.db`, compare-and-swap against Lazily current state, and send a named intent to the PID-scoped editor socket when an editor owns the document. The editor publishes accepted and visible receipts back through Lazily; only then may the state machine project to disk and commit. There is no file patch inbox, live-buffer sidecar, or file-signal ACK path. On timeout, the binary retains the same intent and recovery resumes from its recorded phase without recapturing the response. Use `agent-doc write --force-disk` only as an explicit operator escape hatch for a detached document.

- `try_ipc(file, patches, unmatched, frontmatter_yaml, baseline, content_ours)` — component-level patches for template/stream documents. `content_ours` is the agent-owned response candidate for merge/proof only; IPC success must verify the editor-visible post-apply content and save that verified state, never an older `content_ours` image that drops operator text.
- `try_ipc_full_content()` — canonical document intent for inline-mode documents, fenced by the same expected-current proof
- Detached documents project directly to disk through the authority resolver; attached documents never fall back around Lazily
- When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware exchange patch automatically

**Key files:** `crdt.rs` (CRDT foundation), `merge.rs` (CRDT merge path), `stream.rs` (command),
`agent/streaming.rs` (StreamingAgent trait + chunk parser), `agent/claude.rs` (streaming impl)

**Reactive file-watching:** CRDT-mode documents (`resolved.is_crdt()`) get reactive file-watching (zero debounce) from the watch daemon. The `WatchEntry` has a `reactive: bool` field set by `discover_entries()` for CRDT docs. Reactive paths are tracked in a `HashSet<PathBuf>` and use `Duration::ZERO` for the debounce check, enabling instant re-submit on file change.

**One session per document:** Each `agent-doc stream` spawns its own Claude CLI process.
Multiple documents stream in parallel via separate tmux panes.

**CRDT state storage:** Lazily owns the live document cell. A cold restart projection
may be checkpointed in `state.db` at explicit recovery/recycle boundaries; streaming
and ordinary writes never materialize a per-document CRDT file or use persisted state
as live authority. `agent-doc compact` compacts the document and its state-ledger
history without introducing a second document model.

## Domain Ontology

agent-doc extends the existence kernel vocabulary with domain-specific terms. See the full ontology table in [README.md](README.md#domain-ontology).

<!-- tsift:code-navigation v=0.1.79 -->
## Code Navigation

Run `tsift status` at session start from the owning repo root. If the task or file lives under a git submodule (for example `src/tsift/...`), switch to that submodule root first so the harness loads the narrower local instructions and repo state instead of the superproject root. If status prints a `run:` recommendation for stale or missing tsift state, run `tsift status --fix` before relying on tsift results; when the harness cannot perform write commands, ask the user to run the printed command instead.

Prefer tsift envelopes over raw reads:
- `tsift --envelope search <query>` instead of `grep`/`rg`
- `tsift --envelope source-read <file>` / `tsift --envelope symbol-read <symbol>` instead of `cat`/`head`
- `tsift --envelope explain <symbol>` and `tsift graph <symbol> --callers` / `--callees` for call graphs
- `tsift diff-digest [path]` instead of `git diff`, `git show`, or patch-style `git log`
- `tsift --envelope session-review <path>` / `tsift --envelope context-pack <path>` instead of replaying long session docs, transcripts, or runtime logs
- `tsift --envelope digest-runner --kind test|log --path . --shell-command '<command>'` instead of raw test/build output

Command detail lives in [`runbooks/code-navigation.md`](runbooks/code-navigation.md) — budgets, `tsift workflow search`, `report.scale_guard` handling, the harness rewrite path for `PreToolUse`-less harnesses, and Codex/OpenCode integration. `tsift init` writes and versions that runbook alongside this block, so it is present in every initialized checkout; read it before broad exploration instead of expanding this block. A repository that also ships a current `.claude/skills/tsift/SKILL.md` should use that skill as the deeper source.

For local verification, run `make check` before committing. After local changes, check the latest GitHub Actions CI run with `gh run list --workflow CI --limit 1` and fix any failing tests before calling the work complete.

Only read full source files when tsift results are insufficient.
<!-- /tsift:code-navigation -->
