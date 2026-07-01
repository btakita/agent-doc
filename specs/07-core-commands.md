> Extracted from [07-commands.md](07-commands.md)

# Core Commands

This file covers the lower-churn command surface that is not primarily about tmux/session routing, response closeout, or orchestration.

## run

`agent-doc [run] <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]`

- Inside a supported harness process (Codex, Claude Code, or OpenCode), bare `agent-doc <FILE>` is a harness-native alias for `agent-doc run <FILE>`. In a normal shell with no supported harness environment, bare `agent-doc <FILE>` must fail immediately before opening a run cycle, explain that the bare form is harness-native, and direct the operator to an explicit subcommand such as `agent-doc run <FILE>`, `agent-doc route <FILE>`, or `agent-doc start <FILE>`.
- `run` computes the diff, resolves document mode from frontmatter, sends the prompt to the configured backend, durably captures the final parsed response, applies the response through the matching write path, updates the resume/session id, records `write_applied`, and then runs the same strict closeout helper used by `finalize`.
- `#queue-context-reset`: after a clean active queue item closeout, an automatic continuation must inspect session accretion before launching the next prompt. If the document is at `warn`/`block` accretion or the exchange was recently compacted without a later tracked context clear, direct `run` must start the next backend call from a fresh agent session (ignore the current `resume` for that dispatch, then persist the returned new session id). In Codex Stop-hook continuation, the hook keeps the next head in the current owner turn and records when a background reset would have been requested; automatic supervisor `/clear` handoff is disabled, so only an explicit operator clear or an explicit queued slash command may reset the session.
- `#clearcodex`: the Codex Stop-hook continuation clear decision must be observable in `.agent-doc/logs/ops.log`. Whenever the project is opted into `agent_doc_queue_context_reset`, each Codex continuation emits the canonical `[s760] clear-decision optIn=… threshold=… pct=… clear=…` line plus a `[clearcodex] codex-continuation optIn=true reason=… clear_instructed=false background_clear_suppressed=…` companion that records the effective reset reason without authorizing a background clear. Codex context percentage is read from the latest matching `~/.codex/sessions/**/rollout-*.jsonl` `token_count` event (`last_token_usage` plus `model_context_window`); if no readable event exists, the ctx% gate logs `pct=none clear=false` and fails safe. When the numeric Codex pct crosses the configured threshold, or when accretion/compaction would require fresh context, the hook emits `codex_background_context_clear_suppressed ... result=in_pane_continuation` and continues the queue in pane instead of returning control to the supervisor to send `/clear`. Route-in-flight, active-turn, pending-clear, and one-clear-per-head gates remain mandatory for explicit clear sources. When the project is not opted in, the continuation stays silent (no marker, no pre-emptive clear). This gives the operator structured hook-path log proof to confirm or deny that a queue-turn clear was suppressed without re-deriving it. The `s760_clear_decision_clear_true` gate verifier accepts only a real anchored `^[<timestamp>] [s760] clear-decision ...` line (the bracketed `<timestamp>` may be a bare epoch or an ISO-8601 UTC stamp, `#opslogts`) with `optIn=true`, `pct >= threshold`, and `clear=true`; quoted prose in queue-diff logs is not proof.
- The direct invocation cycle is represented by `flow::session_cycle`: prompt-target extraction, plan/backlog-only scope selection, pending-mutation finalize requirements, and the required `finalize` command shape are derived from one typed contract that `preflight` and `plan` share.
- `finalize` / `write --commit` must reject stale snapshot/CRDT reset drift before applying granular `--backlog-*`, `--review-*`, `--icebox-*`, or `--status` mutations. A closeout that cannot safely place the exchange response must not still mutate backlog/review/icebox/status state, because that creates active queue work without the response/proof that explains it.
- After its pre-commit repair step, `run` rechecks the diff before child-agent dispatch. If the repair consumed the whole diff because the response patchback was already committed and no new assistant response body was supplied, `run` must fail before invoking the configured backend and point the operator to `agent-doc write --commit <FILE>`.
- `#codex-owned-pane-prompt-miss`: when a Codex-owned pane re-invokes `agent-doc <FILE>` for the document it already owns **and** an unresolved exchange prompt is still pending, `run` must fail closed *before* pre-commit and before `start_run_cycle`. The diagnostic must name the unresolved prompt and the in-pane recovery path (answer it in this owner pane's current turn, then persist with `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`) and must tell the operator not to re-run the same direct command from the same pane. Because the early guard bails before pre-commit, the prompt stays uncommitted and executable rather than being baselined into `HEAD`. The detector is a strict subset of the recursive same-pane case, so non-recursive runs are unaffected.
- `#codex-owned-pane-auto-queue-stuck`: when a Codex-owned pane re-invokes `agent-doc <FILE>` for the document it already owns **and** a ready active go-mode queue head remains (no unresolved exchange prompt — the prompt-miss guard above takes precedence), `run` must also fail closed *before* pre-commit / `start_run_cycle` via `queue_continuation::detect` + `owned_pane_queue_handoff_diagnostic`. The diagnostic names the live head (and id), the in-owner-turn `finalize` / `write --commit` recovery, and warns against re-running the same direct command. Bailing early keeps the queue head live and avoids the pre-commit queue/boundary drift the late recursive guard would otherwise leave behind.
- `#recguard-wedge-escape`: the `#codex-owned-pane-auto-queue-stuck` guard fails closed correctly, but in a self-driving go-mode `agent:queue` loop with no operator watching, a busy owner pane that re-invokes `agent-doc <FILE>` **mid-turn** (Option B `#codex-self-reinvoke-prevent` only redirects the *Stop-hook* continuation, not a mid-turn re-run) can trip the same guard on the same head every cycle — an unbounded retry storm. `run` tracks the count of *consecutive* owner-pane self-invocation guard fires for the same head (`recguard_wedge`, keyed on the head text; a different head resets the count, and consuming a head in `write` clears it). Once the count reaches `WEDGE_THRESHOLD` (3), `run` breaks the dead-loop: it halts the runaway queue (`frontmatter::merge_queue_state(false)` → `queue: stop`), clears the counter, logs `recursive_self_invocation_wedge_halt` (file, head id, count), and bails with an escalated diagnostic naming the wedge and the one recovery action that actually advances the head (answer in the owner turn + `finalize` then re-enable `queue: go`, or `agent-doc start <FILE>` and trigger from outside the owner pane). The head stays live and no snapshot/queue drift is committed. Healthy loops that self-invoke at most twice in a row never escalate.
- `#recursion-guard-wedge-escape`: the `start` entry (`agent-doc start <FILE>` and the bare `agent-doc <FILE>` start path) must apply the same recursive self-owned-pane guard as `run`. When `start` is invoked inside the Codex pane that already owns the document (`run::recursive_codex_start_invocation_diagnostic`, a Codex-only owner-pane self-invocation), it must fail closed *before* relocating panes or spawning a replacement owner — otherwise it loops re-injecting `agent-doc <FILE>` into the owner pane (the self-owned-pane recursion wedge with no clean operator escape). The guard fires unconditionally (even under `--force`, since the deadlock is inherent to same-pane nesting; `--force` only governs cross-pane stale-registration reuse), logs `start_recursive_self_owned_pane_refused` (file, pane, session id), and bails with an out-of-pane recovery path: reconcile a possibly stale-busy actor with `agent-doc session status <FILE>`, then if the pane is genuinely wedged run `agent-doc session interrupt-clear <FILE>` from a different pane, escalating to `agent-doc session interrupt-clear <FILE> --force` when normal interrupt/clear cannot settle. The force path is the explicit destructive hatch: it closes the actor when possible, removes the sessions registry projection, signals the supervisor/child PIDs, kills the owner pane, removes the supervisor socket, writes clear cooldown, and reclaims an empty orphaned preflight cycle in one command. Never re-run `agent-doc start <FILE>` from the wedged pane. The detector requires `detect_harness() == "codex"`, so it only fires inside a live Codex agent that owns the doc and never blocks a legitimate same-pane restart from a bare shell after the supervisor exits.
- The late recursive same-pane deadlock guard (`#recguard-abandon`) still applies for a genuine non-queue, non-prompt dispatchable diff in the owner pane: it abandons the empty `preflight_started` cycle as terminal so the owner session is not wedged. `session-check` is the backstop — an abandoned `recursive_direct_invocation_blocked` cycle whose document still carries an unresolved exchange prompt with no later response is reported as a missed-prompt recovery, not accepted as terminal closeout.
- `#codex-owned-pane-prompt-miss-followups` (structured result): `preflight` emits a typed `owned_pane_self_invocation` field (file, current pane, session id, actor generation/state, `kind` = `unresolved_prompt` | `active_queue_head`, work excerpt, optional head id, and the exact persistence command) whenever the document is a Codex owner-pane self-invocation with unresolved exchange work. An unresolved exchange prompt (derived from the cycle's prompt-target diff so it survives the post-commit boundary) takes precedence over an active auto-queue head. Codex guidance reads this to drive an in-pane response cycle instead of only reading the run-time bail diagnostic. The field is null for non-owner panes, non-Codex harnesses, and documents with no unresolved exchange work.
- `#queue-continuation-buries-prompt`: `unresolved_exchange_prompt` (the snapshot-independent detector that backs the `session-check` queue-continuation guard and the `run`-path precedence) must not treat a **queue-continuation** response heading (`### Re: do [#id]` / `### Re: re [#id]`, any h-level) as answering a preceding **free-text** user prompt — that response answered a queue/backlog item, not the prompt. A free-text exchange prompt followed only by queue-continuation responses stays unresolved, so a queue continuation (including a concurrent second actor draining `agent:queue auto`) cannot advance the boundary past an unanswered user prompt and bury it in the snapshot (the JB "agent-doc ignored my previous prompt" failure). The tail scan stops at the first response heading so a queue-continuation's own response body is never mistaken for prompt text; a genuine free-text `### Re:` answer still resolves the prompt (no false positives).
- After opening the response `preflight_started` cycle, `run` emits parent-visible heartbeat stderr during long child-agent waits every `AGENT_DOC_RUN_HEARTBEAT_SECS` seconds, defaulting to 30. In a tmux pane owned by a Codex/OpenCode parent harness with terminal stderr, routine run/diff/commit stderr is redirected to `.agent-doc/logs/run-stderr.log` unless verbose input diagnostics are enabled, so progress output cannot paint over the foreground TUI. Each heartbeat preserves the open phase while updating the cycle state's `updated_at` and `last_event` with the current phase, elapsed time, timeout budget, and agent name.
- If the pending diff contains executable directives such as `do #id`, `run tests`, `build + install`, `commit + push`, `go`, or imperative pending-item prose, status-only or meta-only agent replies are invalid. The response must contain either concrete execution evidence or a concrete blocker.
- If the diff contains a bare `compact exchange` request, `run` must fail closed and direct the caller to `agent-doc compact <FILE> --commit`.
- Once a cycle records `committed`, later repair bookkeeping must not rewind the persisted cycle state to `response_captured` or `write_applied`.

## compact

`agent-doc compact <FILE> [--component NAME] [--keep N] [--message TEXT|-] [--tag NAME|skip] [--commit]`

- Template-mode full exchange compaction must split the component at the live `agent:boundary` marker. Content before the boundary is archiveable and may be summarized; content after the boundary is unresolved live prompt drift and must remain visible in the working tree while staying out of the archive body, compact summary digest, saved snapshot, and closeout commit.
- Template-mode partial exchange compaction follows the same unresolved-tail rule when keeping recent `### Re:` sections.
- `--commit` closes compacted state through the normal binary-owned commit path. It commits only the compacted snapshot state; any unresolved post-boundary prompt left visible remains the next prompt-bearing diff for a later `agent-doc <FILE>` cycle.

## init

Two modes:

- `agent-doc init` initializes the project-level `.agent-doc/` directories and installs bundled skill content.
- `agent-doc init <FILE> [TITLE] [--agent NAME]` scaffolds a new session document and lazily runs project init first when needed.

## install

`agent-doc install [--editor jetbrains|vscode] [--skip-prereqs] [--skip-plugins]`

- Verifies `tmux` and the configured agent CLI are present unless skipped.
- Installs editor plugins either for the requested editor or for auto-detected editors.
- Local source installs inside the `agent-loop` workspace must resolve sibling crates without ad hoc Cargo patch flags.

## diff

`agent-doc diff <FILE>` prints the unified diff between the saved snapshot and the current document.

## response-toc

`agent-doc response-toc <FILE> [--id BACKLOG_ID] [--query TEXT] [--limit N] [--json]`

- Lists lightweight locators for current live `### Re:` sections plus matching archived response sections for the same document.
- `--id` accepts either `restoc` or `#restoc` and filters both live and archived entries.
- `--query` matches normalized heading/body text.
- Output locators are stable enough for follow-up `response-fetch` calls, for example `live:3` or `archive:.agent-doc/archives/hash.md#2`.

## response-fetch

`agent-doc response-fetch <FILE> --locator LOCATOR [--before N] [--after N] [--json]`

- Loads the exact live or archived response section referenced by a `response-toc` locator.
- `--before` / `--after` include adjacent response sections from the same source so agents can pull bounded neighboring context on demand instead of rereading whole exchanges or archives.
- Archive fetches read from the derived archive index; callers do not need to open sqlite directly.

## archive-index

`agent-doc archive-index <FILE> [--rebuild]`

- Builds or refreshes the derived sqlite compacted-turn index at `.agent-doc/archive-index.db`.
- The index is rebuildable from `.agent-doc/archives/*.md`; archive markdown remains the canonical history artifact.
- `--rebuild` drops all derived rows and recreates them from the archive corpus.

## archive-search

`agent-doc archive-search <FILE> [--query TEXT] [--id BACKLOG_ID] [--session SESSION_ID] [--limit N] [--json] [--rebuild]`

- Queries indexed compacted-turn chunks rather than rereading archive markdown manually.
- Results are ranked to prefer the current document, exact `#id` matches, and recent archives.
- `--id` accepts either `sqlarcidx` or `#sqlarcidx`.
- `--rebuild` refreshes the derived index before search.

## memory

`agent-doc memory index <FILE> [--db PATH] [--json]`

`agent-doc memory search <FILE> --query TEXT [--db PATH] [--limit N] [--json] [--rebuild]`

- `memory index` writes first-class agent-doc session memory events into `<project>/.tsift/memory.db` by default.
- Indexed surfaces are current `agent:backlog`, `agent:review`, `agent:icebox`, `agent:done` (including repo-relative `.done.md` archives), and live exchange `### Re:` response sections.
- `memory search` searches indexed events plus the current document's parsed tracked work so dedupe/review checks can detect already-tracked or already-fixed items before a full agent cycle.
- The implementation uses the shared `tsift-memory` library crate directly. The heavy codebase index remains in the tsift CLI and is not part of the per-cycle hot path.
- `--rebuild` indexes the current document before searching; `--json` emits the same report fields used by automation.

## reset

`agent-doc reset <FILE>` clears the saved session id and deletes the snapshot plus CRDT state for the document, including both `.yrs` and `.overlay.yrs` sidecars. `agent-doc reset --from-current <FILE>` clears the saved session id and rebuilds snapshot, legacy CRDT, and overlay CRDT sidecars from the current visible markdown, which is the recovery path after manually cleaning a document whose persisted snapshot/CRDT state is stale.

`agent-doc reset --from-current --preserve-session <FILE>` is the non-destructive recovery for baseline drift after a manual user commit: it refreshes the snapshot, legacy CRDT state, overlay CRDT state, and preflight baseline from the current visible markdown while leaving the document frontmatter, cycle state, and capture history untouched.

## clean

`agent-doc clean <FILE>` squashes all `agent-doc:` commits for the file into one via `git reset --soft`.

## gc

`agent-doc gc [--root DIR] [--dry-run]`

- Garbage-collects orphaned snapshots, captures, locks, hooks, status files, repair diagnostics, Codex blocked-stop diagnostics, sockets, and dead registry entries under `.agent-doc/`.
- The orphaned-socket cleanup keeps sockets whose supervisor PID is alive or whose socket still answers.
- Stale `starting` actor records older than one hour are closed unless a live supervisor PID still has a fresh supervisor heartbeat proving the actor is booting; this updates the controller SQLite store and re-emits `session-actors.json` as a projection. A live PID with a stale heartbeat is treated as stuck startup state.
- A controller wedged in handoff `Preparing`/`Promoted` past the seconds-scale stuck-handoff threshold (`AGENT_DOC_STALE_PREPARING_CONTROLLER_SECS`, default 45s) is terminated (#kqr6 / #sjwm / #stuckhandoff). Unlike the stale-`starting` actor cleanup — which closes a projection record and cannot stop a live process — this kills the live wedged `controller serve` process (verified by `/proc` cmdline and never self) so it stops racing the IDE listener on `ipc.sock`, then supersedes the bootstrap with `Failed` so the next bind promotes a clean controller and the `1002 → 1004 → 1006` respawn loop cannot continue. It logs `stale_preparing_controller_reaped pid=… generation=… age_secs=… caller=…`. The same reaper runs as a self-heal step at controller bind (`connect_or_launch`) before any handoff/promote, and is exposed for operators as `agent-doc admin reap-stale-controllers [--dry-run]` (replacing the manual `pkill -f 'controller serve … --handoff-state preparing'`). `--dry-run` reports without killing.
- Prunes accumulated pre-mutation recovery tags (`#x8aw`): keeps the newest `KEEP_RECOVERY_TAGS` (20) `agent-doc/<doc>/pre-auto-run-N` and `pre-compact-N` tags **per `<doc>/<slug>` series**, deleting older ones. One tag is created per queue auto-run / compaction, so without pruning they grow unbounded over a document's life. Best-effort: a non-git root or git failure is a no-op. `--dry-run` reports the deletions without applying them.
- `preflight` runs the full orphan-file GC automatically at most once per day via `.agent-doc/gc.stamp`; `preflight`, `start`, and `sync` still run the lightweight stale-`starting` actor cleanup every cycle.

## checkpoint

`agent-doc checkpoint <FILE> [--restore TAG] [--diff TAG]`

- Guided recovery for the pre-mutation checkpoint tags created by `compact` (`pre-compact-N`) and the queue auto-run (`pre-auto-run-N`, `#misfire-recovery-snapshot`). Named `checkpoint` because `recover` is already a `repair` alias for orphaned-response recovery, a distinct concern (`#kc5e`).
- Default (no flags): lists the document's checkpoint tags newest-first (commit date, then ordinal) with short SHA, date, and subject, plus the inspect/restore command hints.
- `--diff TAG`: prints `git diff <TAG> -- <FILE>` so the operator can see what changed since the checkpoint.
- `--restore TAG`: runs `git checkout <TAG> -- <FILE>`, restoring **only** that document from the checkpoint (other files untouched), then prompts the operator to review and commit. Surgical and non-destructive to unrelated files — it never resets the whole tree.
- A document with no checkpoint tags prints guidance rather than erroring.

## preflight

`agent-doc preflight <FILE>` emits non-blocking `warnings[]` in its JSON contract. When frontmatter `agent:` is set and differs from the active harness detected from Claude Code, Codex, or OpenCode environment markers after alias normalization, preflight emits `code: "harness_mismatch"` and keeps running; the skill surfaces the warning and continues with the active harness attribution and closeout path.

When the active document lives in (or beside) an `agent-doc` source checkout, preflight also emits `code: "stale_install"` if any installed/built artifact (`~/.cargo/bin/agent-doc`, the lib-installed `~/.cargo/bin/libagent_doc-*.so` cdylib, or the freshest built binary/cdylib from `target/release` and `target/local-install/release-local`) predates the latest buildable source commit by more than a 300-second grace window (`#install-stale-guard`). This catches the failure mode where a same-version commit ships new behavior but `make install` was not re-run, so live tmux / JetBrains sessions silently execute stale code. The check is best-effort and silently no-ops when no `agent-doc` source repo is locatable (for example a crates.io install); the source repo is found at the document's git root or its `src/agent-doc` submodule, and staleness is keyed off the last commit touching `*.rs` / `Cargo.toml` / `Cargo.lock` / `build.rs` so doc-only commits never trip it.

Before preflight performs document-mutating recovery, commit, pending maintenance, or duplicate-residue cleanup, it waits for the shared editor typing indicator to become idle. The emitted `baseline_file` is captured from the same stable visible content used for diff computation, not from an earlier pre-debounce cleanup projection.

`agent-doc preflight --probe <FILE>` runs the same inspection (recovery, commit, queue analysis, diff, JSON output) but is a **pure inspection probe**: it never opens a `preflight_started` cycle (`#preflight-probe-side-effect-free`). The default (dispatch/response-bound) preflight opens that cycle so the upcoming response is bound to it; a diagnostic probe is not response-bound, and an open `preflight_started` cycle left by a probe is exactly the state that later wedges `session-check`. Use `--probe` for diagnostic/recursive-guard inspection so the probe leaves no open cycle behind (a terminal `committed`/`abandoned` cycle from the idempotent commit step is still acceptable). Internal response-bound callers such as `orchestrate` keep the default cycle-opening behavior.

## audit-docs

`agent-doc audit-docs [--root DIR]`

- Audits instruction files such as `CLAUDE.md`, `AGENTS.md`, `README.md`, and `SKILL.md` for path accuracy, actionable content, and line budget.
- Discovery prunes heavy skip directories before descent so audit time is spent on real instruction surfaces.
- Generated agent-doc instruction surfaces are audited as release artifacts: if a root `AGENTS.md`, `.codex/AGENTS.md`, `.opencode/skills/agent-doc/SKILL.md`, or `.claude/skills/agent-doc/SKILL.md` still carries the agent-doc managed frontmatter/sections, it must match the content rendered by the running binary. Without `--root`, a submodule checkout audits the git superproject install root used by normal release installs. With explicit `--root DIR`, generated surfaces are checked under `DIR` exactly, so `--root src/agent-doc` intentionally reports stale tracked submodule-local artifacts such as `.claude/skills/agent-doc/SKILL.md`. Custom root instruction files that do not look agent-doc-managed remain user-owned and are not rewritten or failed for content mismatch.
- Filesystem mtime freshness is advisory for agent-doc audits. Source-only changes may print `Mtime advisory` rows for broad prose or instruction files, but they must not fail the command unless a content-based check also reports blocking drift.

## ops summary

`agent-doc ops summary [--project-root DIR] [--limit N] [--json]`

- Reads `.agent-doc/logs/ops.log` and groups high-signal operational events by document path and session id when the log line provides them.
- The tracked event families are `ipc_write_consumed`, `commit_success`, `commit_noop`, `route_dispatch_start_proven`, `route_submit_issue`, `post_commit_user_follow_up`, `post_commit_local_drift`, `session_clear_active_pane_allowed`, `session_clear_protected_input_guard_refused`, legacy `session_clear_live_busy_guard_bypassed` / `session_clear_live_busy_guard_refused`, current `session_clear_live_busy_guard_blocked`, `route_authoritative_actor_starting_not_ready`, route/start replay lines (`route_starting_actor_timeout_coalesced`, `route_cycle_start_missing*`, `ipc_socket_sidecar_timeout`, `run_preflight_timeout`), closeout/capture drift lines (`interrupted_cycle_detected`, `late_fallback_patch_rejected`, `stale_snapshot_reset_drift_blocked`, `commit_blocked_missing_captured_response`, `session_check_commit_boundary_recovered`), dispatch-only route lines with `proof_scope=accepted_only`, `sync_latency` entries with `status=over_budget`, Codex manifest warning storms, SQLite count markers, session-review guardrails, cross-harness correlation markers, and FlowCore `flow_event` lines. FlowCore lines are grouped first by known high-signal flow/stage/outcome buckets, then by a generic `flow <flow> <stage> <outcome>` bucket so newly typed route/write/commit/session/orchestration events stay visible before a named bucket exists.
- The report also emits ranked `bug_clusters`. Each cluster carries severity, count, latest timestamp, example lines, and correlation keys gathered from `file`, `session` / `session_id`, `cycle` / `cycle_id` / `capture_id`, and Codex/Claude thread markers. Closeout/capture drift, route/start replay gaps, Codex warning storms, SQLite correlation counts, cross-harness markers, session-review guardrails, and working-tree drift are clustered separately so repeated expected no-op closeouts do not bury actionable failures.
- Follow-up prompt drift after an already-committed response is expected operator activity, not anomalous local drift. Human and JSON summaries must bucket `post_commit_user_follow_up`, `post_commit_local_drift kind=user_follow_up`, and `commit_noop drift_kind=user_follow_up` separately from `working_tree_edits` drift/noops so routine reruns do not hide real dirty-working-tree anomalies.
- Already-current no-op closeouts with `commit_noop drift_kind=none` are expected closeout confirmations, not actionable drift. Protected-input clear refusals are expected fail-closed operator guardrails and must be bucketed separately from busy-clear failures or other actionable session problems.
- `--limit` scans only the trailing N log lines, defaulting to a bounded recent tail; `--limit 0` scans the full log.
- Human output is optimized for quick operator review. `--json` emits the same buckets for editor plugins or dashboards.

`agent-doc ops diagnose [--project-root DIR] [--file FILE] [--cycle-id ID] [--patch-id ID] [--session-id ID] [--limit N] [--json]`

- Requires at least one correlation key from `--cycle-id`, `--patch-id`, `--session-id`, or `--file`.
- Gathers a source-grouped diagnosis report from `.agent-doc/logs/ops.log`, cycle JSONL, harness session logs, editor/plugin debug logs, capture JSON, Codex hook records, hook payloads, patch files, actor/session state, and agent-doc state sidecars.
- Text log sources match by path or line content and obey the same `--limit` tail contract as `ops summary`; `--limit 0` scans full text files.
- JSON sources are redacted before output, large payload fields are summarized instead of dumped, and `--json` emits the structured source/match report for editor plugins or reproducible bug attachments.

## prompt

`agent-doc prompt <FILE>`

- Detects active permission prompts from Claude Code and OpenCode panes by scanning the captured pane footer.
- Supports Claude Code bracketed legacy options, Claude Code numbered-list options, and OpenCode horizontal `Allow once` / `Allow always` / `Reject` permission rows.
- `prompt --answer` uses Claude Code's vertical Up/Down movement for Claude prompts and OpenCode's Tab/BackTab selector movement for OpenCode permission prompts. OpenCode prompt detection captures panes with ANSI attributes so the currently highlighted option is read from the TUI state before navigation; plain-text captures are not sufficient because they lose the highlight. Selecting OpenCode `Allow always` also sends the follow-up confirmation Enter because OpenCode opens a second `Always allow` confirmation prompt before persisting that choice.
- `--answer N` selects an option by one-based position in the parsed `options` array, not by the option's displayed TUI label number, then presses Enter.
- `--all` polls every live session and serializes prompt fields flat on each entry: `session_id`, `file`, `cwd`, `active`, optional `question`, optional `options`, and optional 0-based `selected`. Editor integrations must answer from the entry's `cwd` so prompts owned by submodule or sibling project roots do not run against the wrong registry.

## skill

`agent-doc skill install` writes the bundled skill into the current project, and `agent-doc skill check` compares the installed copy to the bundled version.

- The installed skill always renders `agent-doc-version` from the running binary version.
- Harness-specific reload flows must use explicit `--harness` selection rather than environment guessing.
- Harness installs refresh a managed root `AGENTS.md` mirror when it still looks generated, so `.codex/AGENTS.md` and the root mirror cannot drift across `agent-doc-version` bumps. Custom root `AGENTS.md` files are opt-in and must be preserved.
- Generated Claude, Codex, and generic hot-path instruction surfaces must stay compact: the shared source template is budgeted at 140 lines, and rendered harness-specific surfaces are budgeted at 150 lines. Rare recovery detail belongs in bundled runbooks rather than the always-loaded skill body.

## outline

`agent-doc outline <FILE> [--json]` reports markdown heading structure, line counts, and approximate token counts.

## upgrade

`agent-doc upgrade` checks crates.io for a newer release and upgrades through the GitHub Release / `cargo install` / `pip` cascade.

The runtime version warning cache lives at `~/.cache/agent-doc/version-cache.json`.

## plugin

`agent-doc plugin install|update|list <EDITOR>`

- Supports JetBrains and VS Code.
- Pulls assets from GitHub Releases, preferring signed assets when available.

## rename

`agent-doc rename <OLD_PATH> <NEW_PATH>`

- Migrates hash-keyed state files such as snapshots, baselines, locks, pending state, legacy CRDT state, overlay CRDT state, and pre-response artifacts to the new path hash.
- Auto-migration through `ensure_initialized` still handles the common rename path; `rename` remains the explicit fallback.

## watch

`agent-doc watch [--stop] [--status] [--debounce MS] [--max-cycles N]`

- Watches registered session files and re-submits them when they change.
- CRDT/reactive documents use zero debounce.
- Busy documents are skipped so the watch daemon cannot race the live write path.

## history

`agent-doc history <FILE>` lists exchange history from git.

`agent-doc history <FILE> --restore <COMMIT>` prepends a historical exchange back into the current exchange component.

## transfer

`agent-doc transfer <SOURCE> <TARGET> <COMPONENT> [--bypass-claim] [--items ...] [--referral]`

- Full transfer moves an entire component, optionally carrying backlog and icebox context too.
- Selective `--items` transfer operates on backlog/icebox parent items keyed by `[#id]` and moves the full tracked block, including indented continuation lines.
- `--bypass-claim` is the explicit cross-pane override.
- `--referral` leaves the source content in place and inserts a structured pointer in the target instead of moving content.

## extract

`agent-doc extract <SOURCE> <TARGET> [--component NAME]`

- Moves the last exchange entry from the source into the target's matching component and preserves both documents' snapshots.

## backlog

`agent-doc backlog <FILE> <ACTION>`

- Canonical surface for tracked work. `agent-doc pending` remains a deprecated alias only.
- Supports add/edit/done/reorder/prune/list/gate operations against the canonical `agent:backlog` component.
- Non-item separator lines and headings inside backlog/icebox must be preserved during mutation.
- Flush-left parent items are the tracked units; indented nested lists travel with the parent during edit/reorder/reap/transfer.

## boundary

`agent-doc boundary <FILE> [COMPONENT]`

- Inserts a transient `agent:boundary` marker into the working-tree document and signals the editor so the next IPC write can use a current insertion point.
- It must not update the saved snapshot, stage files, or create a git commit. The marker is setup state, not a response closeout boundary.
- A later preflight/commit may normalize marker-only working-tree churn as already committed, but standalone boundary insertion must never become the snapshot basis for a boundary-only commit.

## terminal

`agent-doc terminal <FILE> [--session NAME]`

- Opens an external terminal that attaches to the target tmux session, but only when another attached client does not already exist.
- The terminal command comes from user config or `$TERMINAL`.

## migrate

`agent-doc migrate [FILES...] [--all] [--dry-run]`

- Migrates deprecated `agent:pending` markers to the canonical `agent:backlog` markers and strips deprecated backlog tag attributes.
- Skips fenced code blocks and inline code.

## dedupe

`agent-doc dedupe <FILE>`

- Removes consecutive duplicate `### Re:` response blocks and updates the snapshot.
- Also deletes the stale queued patch file so a plugin restart cannot replay the removed duplicate.
- The normal template write/finalize path runs the same consecutive-response dedupe before saving snapshots, CRDT state, or disk content. Sidecar-normalization and IPC dedupe repair must prove editor delivery before saving; otherwise the write fails closed with retry state intact. `session-check` fails closed if a duplicate survives closeout instead of reporting success.
- Active stream IPC timeout leaves the queued patch/pending response for retry and does not perform a local write or commit, so `dedupe` is not a cleanup mechanism for that timeout shape.

## cancel

`agent-doc cancel <FILE>`

- `#cancel-orphans-preflight-cycle`: explicit run-cancel reclaim. When the user cancels an in-progress run, the orphaned cycle otherwise blocks the next `Run Agent Doc` until the `STALE_EMPTY_PREFLIGHT_TTL_SECS` (60s) staleness window elapses. `cancel` abandons that cycle immediately so the next dispatch starts fresh.
- Fail-safe: it abandons the open cycle **only** when it is still `preflight_started` **and** owns no response capture. A cycle that advanced past preflight (`response_captured` / `write_applied` / `committed`) or already captured a response is left intact, so a cancel can never discard real in-flight work. Logs `cancel_preflight_cycle_abandoned` on abandon.
- Exposed to editor plugins via the `agent_doc_cancel_preflight_cycle(file_path) -> i32` FFI export (1 = abandoned, 0 = nothing reclaimed / protected, -1 = error). The JB "cancel run" action is the thin reporter that calls it; the abandon decision is the pure binary `repair::cancel_preflight_cycle` function. Plan: `tasks/agent-doc/plan-cancel-orphans-preflight-cycle.md`.
