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
- **NEVER swallow errors** — no `let _ =` on fallible operations. Always log at minimum a warning to stderr. Silent failures make bugs invisible and waste debugging cycles.
- **Behavioral fixes are packagable, not per-user agent memory** — when an agent-doc *session* behaves wrong (the agent stalled the queue, asked the wrong thing, mishandled closeout), fix it in the **product** so every user benefits: a binary heuristic, a `SKILL.md`/runbook instruction surface, or these development instructions. Do **not** resolve agent-doc behavior problems by writing a per-user agent-memory note — agent-doc ships to other people, and a memory only helps one operator. Memory is for facts about *a specific environment*, never for correcting shipped agent-doc behavior.
- **Do the agent-doable deploy/release work without asking (`#deploy-just-do-it`)** — when a session produces a shippable agent-doc change, execute *every* agent-doable release/deploy sub-step autonomously: version bump (`Cargo.toml` + `pyproject.toml`), `VERSIONS.md` entry, `make check`, commit, `make install-full`, push, `agent-doc admin recycle`, tag, and publish. Do **NOT** defer these to the operator, and do **NOT** ask "should I deploy/release?" — operator approval is assumed for anything an agent can do. The **only** operator-gated step is the genuine live-session eyeball (a human watching a real editor/pane prove the behavior); record it as a non-blocking `[operator-verify]` follow-up. An `[operator-verify]` item never licenses skipping the build/install/push/recycle/publish — it means "do all the agent-doable sub-steps now, leave only the live human check." Asking permission for agent-doable deploy work is itself the bug.
- **Operator-visible document text is authoritative** — Preserve user edits; let `agent-doc write --stream` merge. Operator-visible document text is authoritative: never recover, patch, or hook-closeout by replacing it with `content_ours`, a snapshot, or ACK-content if that would drop operator text. Snapshots are backup/audit state, not hot-path authority; fail closed or retry through the editor instead.
- **Session-accretion compaction guidance is queue-aware (`#no-compact-prompt-during-queue-drain`)** — while an `agent:queue` is actively draining (`queue_active: true`), `session_accretion.rs` must NOT surface "ask the user before compacting" guidance: a self-driving queue is meant to run unattended, so a compaction question stalls the queued work. On an active queue the binary emits don't-stall guidance and compacts only on an explicit `agent_doc_auto_compact` opt-in; off the queue it asks before compacting. Keep `compaction_guidance` in `session_accretion.rs` the single source of that wording.
- **Operator-authored queue order is authoritative (`#qauthorder`)** — queue convergence must keep an operator-added queue line at the document slot the operator authored it in: never auto-bubble it to the top and never duplicate it. Holding the slot must not mutate the line's visible text (do **not** inject a `:pushpin:` the operator never typed); use a position-lock keyed off the line's stable identity instead. Free-text operator lines need the same convergence dedup the `do [#id]` heads already get (`#qdedupsync` is free-text-blind) so a CRDT/backlog-sync re-emit cannot leave a visible duplicate. Reconcile with `sort_prompts_by_priority` position-lock (`#queue-operator-pin-position-lock`), `annotate_manual_queue_additions` (`#7r2s`), and `#backlog-queue-append-stable` rather than regressing them. Plan: `tasks/agent-doc/plan-queue-preserve-operator-author-order.md`.
- **All deterministic behavior in the binary** — document manipulation (compact, diff, merge, patch, write), snapshot management, git operations, and component parsing must live in Rust. The SKILL.md skill is the non-deterministic orchestrator (reads diff, generates response, decides what to write). Never implement deterministic document logic in the skill or ad-hoc scripts.
- **Harness arg resolution is explicit** — `agent_args` is the shared override, `claude_args` is Claude-only, `codex_args` is Codex-only, and `opencode_args` is OpenCode-only. Keep those precedence chains in `start.rs`, `frontmatter.rs`, `config.rs`, and the docs/specs aligned.
- **`tmux-router` is a live sibling development target in agent-loop** — when generic tmux pane/session mechanics move out of `src/agent-doc`, update `../tmux-router` in the same turn and keep the workspace cargo patch (`../.cargo/config.toml`) plus harness instruction surfaces aligned so local builds exercise the extracted code instead of the published crate.
- **Skill install content is part of the product contract** — changes in `src/skill.rs`, `SKILL.md`, bundled runbooks, or bundled OKF concepts must keep the installed `.claude/skills/agent-doc/SKILL.md`, `.codex/AGENTS.md`, `.opencode/skills/agent-doc/SKILL.md`, managed root `AGENTS.md` mirrors, harness runbooks, and harness OKF directories aligned. Claude/Codex/OpenCode hot-path instructions should render from one shared source surface, with differences limited to harness-specific invocation wording and frontmatter description. `audit-docs` must fail on generated agent-doc instruction surfaces that still carry managed frontmatter but no longer match the running binary, while preserving custom root `AGENTS.md` files. In particular, the shared Claude/Codex/OpenCode manual-repair guidance must distinguish inserting a missing user prompt from repairing a missed assistant response, route the latter through `agent-doc write --commit <FILE>`, and reject flows that stop after bare `agent-doc write`.
- **Compound `commit + push` turns must keep the session doc off manual repo commits** — when the user requests ordinary repo commit/push work inside an `agent-doc` turn, manual git commits may stage only the intended non-session repo files, must stop immediately on any stage failure, must verify the staged diff still matches that intended path set, and must commit only that validated set. The active session document still closes through `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`, and the push happens after that binary-owned closeout commit lands. Keep `SKILL.md`, `README.md`, `SPEC.md`, and the bundled runbooks aligned on that ordering rule.
- **Completed backlog archive is `agent:done`** — reaped tracked work must be recognized only from the canonical `agent:done` component. `agent:backlog-done` and `agent:pending-done` are migration inputs, not runtime aliases.
- **Selective commit is conservative** — `git::commit()` stages the snapshot, not arbitrary working-tree drift. The only absorbable out-of-band repair path is narrow agent-owned drift (`status`, `### Re:` response-block insertion, pending-ID superset) when the redacted component structure still matches. Plain user prompts must remain uncommitted. Already-committed historical response-block drift may repair the snapshot only when the working tree matches `HEAD` modulo transient boundary / `(HEAD)` markers, and that same self-heal also tolerates committed exchange-only prompt-prefix normalization on already-answered prompts (for example, historical `❯ do ...` vs committed bare `do ...` directly above a real `### Re:` block). Even under extreme snapshot/file drift, tracked docs must not wholesale re-sync the snapshot from the live file — reserve that bootstrap escape hatch for untracked scaffold snapshots only. After a successful commit, boundary cleanup must collapse the **snapshot** to the same clean shape as the committed blob. The **working tree** (and editor buffer via IPC) preserves `(HEAD)` annotations so the user sees which headings are new — preflight classifies `(HEAD)` differences as `boundary_artifact`, not user edits.
- **Route readiness stays in the binary** — `route.rs` owns pane prompt detection and trigger acceptance. Keep that logic resilient to shell startup noise / echoed command text, key it off real harness prompt shapes rather than generic shell `>` echoes, and do not push harness-readiness heuristics into the skill layer.
- **Managed capability proof stays out of pane transcripts** — `start.rs` must keep successful/failed managed proof events in the session log and surface the user-visible `[start] managed ... capability proof` summary through tmux `display-message` on the owned pane. Do not write those diagnostics to the child pane stdout/stderr stream where they can perturb prompt detection or the next agent input.
- **Restart/auto-install child stdio must never inherit the agent pane (`#restartstderrbleed`)** — the route-owned supervisor renders the agent TUI into its own **fd1 (stdout)** while only **fd2 (stderr)** is process-redirected to `.agent-doc/logs/supervisor-stderr.log` (`SupervisorStderrRedirect`). Any child the supervisor spawns during a restart/recycle/auto-install (notably `make install` in `run_auto_install_steps_once`, whose unsuppressed recipe echo goes to stdout) must have its stdout+stderr explicitly wired to the supervisor-log target (a dup of fd2) and its stdin nulled — never `.status()`/`.output()` with inherited stdio, which sends build/recipe output straight into the live agent pane. Route child stdio through `auto_install_child_stdio` (`project_controller/rpc.rs`) and keep the fd-bleed regression test green.
- **Route progress diagnostics must stay UTF-8 safe** — any stderr/status trimming of captured tmux lines in `route.rs` must truncate on char boundaries so Unicode prompt/status glyphs cannot panic a live reroute.
- **Starting actor reroutes promote only proven idle panes** — when the project controller still reports the authoritative actor as `starting`, route may promote it to `ready` only after the live pane shows a harness-specific dispatch-ready prompt, then use the normal managed/dispatch-only send path. If the prompt never becomes dispatch-ready, the route path must fail closed before sending tmux or supervisor input. Keep this aligned in `route.rs`, `README.md`, `SPEC.md`, and the installed harness surfaces.
- **Fresh route panes stay authoritative** — once `route.rs` creates a fresh pane for a document, later geometry-only registry churn must not hand dispatch back to an older same-session pane. Keep the fresh-pane authority rule aligned in `route.rs`, `README.md`, `SPEC.md`, and the session/tmux command spec.
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
- **Response replay is capture-backed** — final parsed responses must be durably persisted before write/hook emission in `.agent-doc/captures/<doc-hash>/<cycle-id>.json`. Recovery replays the captured response body only when the captured snapshot/file hashes still match the current baseline; otherwise fail closed.
- **Bounded accretion context must anchor to the edited turn** — when session-accretion prompt packing replaces the full exchange tail, `prompt_context.rs` must select the `### Re:` block at the prompt's actual position in `exchange`: enclosing response for inline prompt edits, immediately previous response for tail follow-ups, with older unrelated turns left on-demand.
- **`finalize` is the strict response happy path** — `agent-doc finalize <FILE>` must fail before mutating a non-git document and must not report success unless the cycle reaches `committed`. Keep that contract aligned in `main.rs`, `write.rs`, and the command docs.
- **Run must recheck after pre-commit repair** — when `git::commit()` repairs an already-committed missed patchback and no prompt-bearing diff remains, `run.rs` must fail before child-agent dispatch and point to `agent-doc write --commit <FILE>` instead of submitting an empty or stale prompt.
- **Post-commit user follow-ups are not missed-response repair** — when `git::commit()` sees `snapshot == HEAD` plus a later user follow-up prompt, keep that prompt uncommitted for the next cycle and log `post_commit_user_follow_up`; do not label that safe shape as `prior_patchback_without_response_body` or `out_of_band_write`. Keep `git.rs`, `README.md`, `SPEC.md`, and closeout specs aligned.
- **Direct-exec post-write guard stays explicit** — keep `SKILL.md`, `runbooks/commit.md`, `runbooks/harness-invocation.md`, and `specs/07-commands.md` aligned on the Codex/OpenCode/direct-exec requirement to run `agent-doc session-check <FILE>` after `finalize` or manual `write --commit`, and fail closed if it reports an open cycle, a prompt-only exchange tail with no assistant response, or a likely direct assistant patchback that bypassed the binary write path. The only self-heal exception is already-committed historical snapshot drift proven by `HEAD`.
- **Optional closeout sidecars stay advisory** — keep `session_check.rs`, `capture.rs`, `cycle_state.rs`, `startup_miss.rs`, `snapshot.rs`, and the docs/specs aligned on the rule that a late `NotFound` while reading optional closeout sidecars is treated as absent state rather than as a transient `ENOENT` failure.
- **Codex hook backstop is binary-owned** — keep `src/codex_hook.rs`, `src/skill.rs`, `SKILL.md`, `runbooks/harness-invocation.md`, `README.md`, and `SPEC.md` aligned on the installed `.codex/hooks.json` / `.codex/config.toml` contract: `UserPromptSubmit` tracks the active document, and `Stop` first tries to finish the response cycle deterministically from `last_assistant_message` via the normal repair/write/commit path before falling back to capture-and-block / fail-closed behavior. Empty `last_assistant_message` on an open cycle must still fail closed with diagnostics and tracked-prompt recovery because tool-only/authentication steps (for example MCP OAuth / `authenticate`) are sub-steps, not successful closeout boundaries.
- **Required SSH drift detection must include bare socket EPERM when SSH context is proven** — keep `src/agent/codex.rs`, `README.md`, `SPEC.md`, and the bundled skill surfaces aligned on the rule that a resumed Codex `command_execution` event with output like `socket: Operation not permitted` still counts as required-SSH capability drift when the same event proves an `ssh` command against a declared `required_ssh_targets` entry. Do not collapse localhost/CDP `Operation not permitted` signatures into the SSH path.
- **Required SSH fresh retries must discard stale resumed prelude text** — keep `src/agent/codex.rs`, `README.md`, `SPEC.md`, and the bundled skill surfaces aligned on the rule that resumed Codex streams for SSH-gated docs buffer early assistant chunks until required SSH is proven safe or the turn completes, so a required-SSH fresh retry can drop stale prelude text from the discarded resumed session.
- **Response ordering is part of the contract** — keep the same files aligned on the rule that requested implementation / verification / build-install work finishes before final response persistence, and that only `session-check`, recovery, and final reporting remain after `finalize` / `write --commit`.
- **Harnesses own full-suite verification** — keep `Makefile`, `SKILL.md`, `SPEC.md`, and installed harness instruction surfaces aligned on the rule that agents explicitly run the full project verification suite after changes instead of relying on a git pre-commit hook to do it implicitly.
- **Tmux CI review is part of test-bearing turns** — whenever a turn runs or changes tests, review the latest GitHub Actions CI run for the tmux leg (`make tmux-ci`). Check CI with `gh run list --workflow CI --limit 1` to make sure it is not already red; if CI reports tmux failures after runner startup, reproduce locally with `make tmux-ci`, fix the issue, and add or update deterministic SimWorld coverage for the failure class when that behavior can be modeled without live tmux. If the latest run is queued or in progress, record that status and continue with local verification evidence instead of blocking the turn for CI completion; do not use `gh run watch` as a closeout gate unless the user explicitly asks. Empty-step jobs with no logs because GitHub never started a runner (for example billing/spending-limit exhaustion) are external CI-start blockers, not code/tmux regressions; record the annotation and continue with local verification evidence. Keep `SKILL.md`, `SPEC.md`, `README.md`, and installed harness surfaces aligned on this rule.
- **Preflight is a stable binary contract** — keep `src/preflight.rs`, `SKILL.md`, `.claude/skills/agent-doc/SKILL.md`, and the top-level docs aligned on the interrupted-cycle guard (`preflight_started`, `response_captured`, and `write_applied` count as open; only recoverable or stale-empty `preflight_started` cycles auto-close), tier fields (`effective_tier`, `required_tier`, `suggested_tier`, `model_switch`, `model_switch_tier`), and `agent_model` short-name attribution.
- **Oversized specs should split behind a stable index** — when a spec or instruction file grows past a clean single-purpose boundary, follow [runbooks/split-spec-files.md](runbooks/split-spec-files.md): keep the existing numbered entrypoint as an index, move normative detail into focused sibling files, update the top-level catalogs instead of growing another monolith, and keep that ownership rule aligned across managed Claude/Codex/OpenCode harness surfaces while leaving custom root instruction files opt-in unless they still match the generated baseline.
- **FFI-first for editor integration (Shared Foundation pattern)** — when adding features that editors need (sync debounce, busy guards, IPC listeners, layout validation), implement in the FFI layer (`ffi.rs`) first, then call from editor plugins via JNA/FFI. Editor plugins should be thin event reporters — layout changed, file selected, etc. Business logic (debouncing, locking, socket listeners, idempotency checks) belongs in the shared FFI library, not duplicated across IntelliJ/VS Code plugins. **Ontology:** Both the FFI library and each editor plugin are **Systems** with their own **Perspectives**. Each exposes an **Interface** (C ABI, JNA bindings) — the defined boundary through which Systems communicate. The Shared Foundation pattern places shared logic at the broadest **Scope** (FFI library) so all consumer Systems access it through their Interfaces. **Test:** "Does this feature need to work in >1 editor?" → implement in FFI. Example: socket IPC listener lives in `ffi.rs` (`agent_doc_start_ipc_listener`), not in `PatchWatcher.kt`.

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
  sessions.rs       # Session registry (sessions.json) + Tmux struct
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
  resync.rs         # Validate sessions.json, remove dead panes, detect wrong-session/wrong-process panes (--fix [--session <target>])
  session_cmd.rs    # Show/set configured tmux session with pane migration
  history.rs        # Exchange version history from git + restore
  upgrade.rs        # Self-update via crates.io / GitHub Releases
  plugin.rs         # Editor plugin install/update/list via GitHub Releases
  write.rs          # Write command: parse patches, IPC-first writes, disk fallback
  template.rs       # Template mode: patch parsing, apply_patches, boundary lifecycle
  boundary.rs       # Boundary marker management (insert, remove, reposition)
  crdt.rs           # CRDT foundation (yrs-based conflict-free merge)
  merge.rs          # 3-way merge + CRDT merge path
  stream.rs         # Stream command: real-time CRDT write-back loop
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

1. Bump version in `Cargo.toml` + `pyproject.toml` (keep in sync)
2. Update `VERSIONS.md` with a new version entry summarizing the changes
3. `make check` (clippy + test)
4. `make install-full` — install a full release-profile local build and verify the changed behavior end-to-end (the agent runs `make check` + automated checks as the verification; do not wait on a human)
5. **No operator gate on agent-doable steps (`#deploy-just-do-it`):** proceed straight through steps 6-10 without asking. The only operator-gated step is a live human eyeball of the changed behavior in a real editor/pane — record it as a non-blocking `[operator-verify]` follow-up; it never blocks the build/install/push/publish/recycle.
6. Branch → PR → squash merge to main (or commit + push to main directly in this dogfooding repo)
7. Tag: `git tag v<version> && git push origin v<version>`
8. `make publish-crate` (crates.io; publishes dependency graph in order)
9. `maturin publish` (PyPI)
10. `gh release create v<version> --generate-notes` with prebuilt binary (GitHub Release)

## Agent Backend Contract

Each agent backend implements: take a prompt string, return (response_text, session_id).
The prompt includes the diff and full document. The agent backend handles CLI
invocation, JSON parsing, and session flags.

### StreamingAgent Contract

Streaming backends implement `StreamingAgent::send_streaming()` → `Iterator<StreamChunk>`.
Used by `agent-doc stream` for real-time write-back. Currently only `claude` supports streaming
(via `--output-format stream-json`). Each `StreamChunk` has cumulative text, optional
`thinking` content, `is_final` flag, and optional `session_id` on the final chunk.

## Stream Mode

Stream mode (`agent_doc_format: template` + `agent_doc_write: crdt`) enables real-time agent output with CRDT-based conflict-free merge. Legacy: `agent_doc_mode: stream` is still supported as a deprecated alias.

**Usage:** `agent-doc stream <FILE> [--interval 200] [--agent claude] [--model opus] [--no-git]`

**How it works:**
1. Validates document uses CRDT write strategy (`resolved.is_crdt()`), reads `StreamConfig` from frontmatter
2. Computes diff, builds prompt requesting patch-block format
3. Spawns streaming agent (`claude -p --output-format stream-json`)
4. Timer thread (default 200ms) periodically flushes accumulated text to document:
   `flock → read file → apply template patch (replace mode) → atomic write → unlock`
5. On completion: saves CRDT state + snapshot, updates resume ID, optional git commit

**Frontmatter:**
```yaml
agent_doc_format: template
agent_doc_write: crdt
agent_doc_stream:
  interval: 200           # write-back interval (ms), default 200
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
flushed on the same timer interval as response text.

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

### IPC-First Writes (v0.17.5)

All write paths (`run`, `stream`, `write`) try IPC to the IDE plugin when `.agent-doc/patches/` exists (plugin installed) and `--force-disk` is not set. IPC writes a JSON patch file to `.agent-doc/patches/<hash>.json`; the IDE plugin applies it via Document API (preserving cursor, undo stack, no "externally modified" dialog) and deletes the file as ACK. On IPC timeout or missing response proof, stream/finalize closeout retains the pending response and queued patch for retry, refuses a direct session-document disk write, and logs `recovery=retry_without_disk_write`. Use `agent-doc write --force-disk` only as an explicit operator escape hatch to bypass IPC and write directly to disk.

- `try_ipc(file, patches, unmatched, frontmatter_yaml, baseline, content_ours)` — component-level patches for template/stream documents. `content_ours` is the agent-owned response candidate for merge/proof only; IPC success must verify the editor-visible post-apply content and save that verified state, never an older `content_ours` image that drops operator text.
- `try_ipc_full_content()` — full document replacement for inline-mode documents
- Both are safe to call unconditionally; they return `false` immediately if `.agent-doc/patches/` does not exist
- When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware exchange patch automatically

**Key files:** `crdt.rs` (CRDT foundation), `merge.rs` (CRDT merge path), `stream.rs` (command),
`agent/streaming.rs` (StreamingAgent trait + chunk parser), `agent/claude.rs` (streaming impl)

**Reactive file-watching:** CRDT-mode documents (`resolved.is_crdt()`) get reactive file-watching (zero debounce) from the watch daemon. The `WatchEntry` has a `reactive: bool` field set by `discover_entries()` for CRDT docs. Reactive paths are tracked in a `HashSet<PathBuf>` and use `Duration::ZERO` for the debounce check, enabling instant re-submit on file change.

**One session per document:** Each `agent-doc stream` spawns its own Claude CLI process.
Multiple documents stream in parallel via separate tmux panes.

**CRDT state storage:** `.agent-doc/crdt/<hash>.yrs` — persisted after each stream for
subsequent merges. Compacted via `agent-doc compact` to GC tombstones.

## Domain Ontology

agent-doc extends the existence kernel vocabulary with domain-specific terms. See the full ontology table in [README.md](README.md#domain-ontology).

<!-- tsift:code-navigation v=0.1.74 -->
## Code Navigation

Keep this block self-contained for Codex/OpenCode prompt reuse. If this repository also ships current `.claude/skills/tsift/SKILL.md` or `runbooks/code-navigation.md`, use those deeper runbooks for command detail instead of expanding this block.

Run `tsift status` at session start from the owning repo root. If the task or file lives under a git submodule (for example `src/tsift/...`), switch to that submodule root first so the harness loads the narrower local instructions and repo state instead of the superproject root. If status prints a `run:` recommendation for stale or missing tsift state, run `tsift status --fix` before relying on tsift results; when the harness cannot perform write commands, ask the user to run the printed command instead. Codex projects can install a prompt-time auto-reindex hook with `tsift init --codex`; OpenCode projects can install per-project tsift command shortcuts with `tsift init --opencode`.

Use the commands listed in its `use:` output:
- `tsift --envelope source-read <file> --budget normal` — AST-symbol projection with span metadata and source-window expansion commands (prefer over cat/head for source code files)
- `tsift --envelope symbol-read <symbol> --budget normal` — token-budgeted symbol body, AST span metadata, child refs, and graph/source expansion commands
- `tsift --envelope search <query> --budget normal` — AST-aware hybrid search preview (prefer over grep/rg)
- `tsift --envelope explain <symbol> --budget normal` — callers, callees, community preview
- `tsift graph <symbol> --callers` / `--callees` — call graph navigation
- `tsift summarize <symbol>` — cached summary (only when listed in `use:`)
- `tsift workflow search` — ordered exact/search/explain/summarize/digest recipe that preserves result handles across expansions

When a search envelope includes `report.scale_guard`, run one of its `narrow_commands` before dispatching parallel agents. The guard means the original result set or corpus is broad enough that fan-out should start from a narrower cited handle, path, or exact query.

Prefer bounded digest commands over raw transcript, diff, and verbose-log reads:
- `tsift --envelope session-review <path> --next-context --budget normal` or `tsift --envelope context-pack <path> --budget normal` instead of replaying long session docs, JSONL transcripts, or agent-doc runtime logs with `cat`, `tail`, or `sed`.
- `tsift diff-digest [path]` (`--cached`, `--revision <rev>`) instead of `git diff`, `git show`, or patch-style `git log`.
- `tsift --envelope digest-runner --kind test --path . --shell-command '<test command>'` / `tsift --envelope digest-runner --kind log --path . --shell-command '<build command>'` for noisy test/build/install output, or let the rewrite/hooks create those artifact-backed envelopes for `cargo test`, `pytest`, and verbose cargo commands.
- If RTK is installed, digest-runner delegates supported generic command families through `rtk rewrite` and records the chosen compact filter in `report.filter` while preserving tsift artifact handles.
- Codex, OpenCode, and other harnesses without Claude-style `PreToolUse` hooks should run `tsift rewrite --run '<command>'` before broad `rg`/recursive grep, raw transcript/session/log reads, `git diff`/`git show`/single-patch `git log`, `cargo test`/`pytest`, and cargo build/check/clippy/install commands so the same search, session-digest, diff-digest, and digest-runner rewrites apply manually. OpenCode can install this path as `/tsift-rewrite-run` with `tsift init --opencode`.

For local verification, run `make check` before committing. After local changes, check the latest GitHub Actions CI run with `gh run list --workflow CI --limit 1` and fix any failing tests before calling the work complete.

Only read full source files when tsift results are insufficient.
<!-- /tsift:code-navigation -->
