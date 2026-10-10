> Extracted from [07-commands.md](07-commands.md)

# Core Commands

This file covers the lower-churn command surface that is not primarily about tmux/session routing, response closeout, or orchestration.

All commands that name an existing markdown session document perform one
fail-open, 250 ms supervisor `pid` query at command entry. A stale-binary result
automatically schedules the document owner for idempotent safe-boundary recycle;
commands do not wait for the recycle and never interrupt an active turn.

## run

`agent-doc [run] <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]`

- Inside a supported harness process (Codex, Claude Code, or OpenCode), bare `agent-doc <FILE>` is a harness-native alias for `agent-doc run <FILE>`. In a normal shell with no supported harness environment, bare `agent-doc <FILE>` must fail immediately before opening a run cycle, explain that the bare form is harness-native, and direct the operator to an explicit subcommand such as `agent-doc run <FILE>`, `agent-doc route <FILE>`, or `agent-doc start <FILE>`.
- `run` computes the diff, resolves document mode from frontmatter, sends the prompt to the configured backend, durably captures the final parsed response, applies the response through the matching write path, updates the resume/session id, records `write_applied`, and then runs the same strict closeout helper used by `finalize`.
- `#queue-context-reset`: after a clean active queue item closeout, an automatic continuation must inspect session accretion before launching the next prompt. If the document is at `warn`/`block` accretion or the exchange was recently compacted without a later tracked context clear, direct `run` must start the next backend call from a fresh agent session (ignore the current `resume` for that dispatch, then persist the returned new session id). In Codex Stop-hook continuation, the hook keeps the next head in the current owner turn and records when a background reset would have been requested; automatic supervisor `/clear` handoff is disabled, so only an explicit operator clear or an explicit queued slash command may reset the session.
- `#clearcodex`: the Codex Stop-hook continuation clear decision must be observable in `.agent-doc/logs/ops.log`. Whenever the project is opted into `agent_doc_queue_context_reset`, each Codex continuation emits the canonical `[s760] clear-decision optIn=… threshold=… pct=… clear=…` line plus a `[clearcodex] codex-continuation optIn=true reason=… clear_instructed=false background_clear_suppressed=…` companion that records the effective reset reason without authorizing a background clear. Codex context percentage is read from the latest matching `~/.codex/sessions/**/rollout-*.jsonl` `token_count` event (`last_token_usage` plus `model_context_window`); if no readable event exists, the ctx% gate logs `pct=none clear=false` and fails safe. When the numeric Codex pct crosses the configured threshold, or when accretion/compaction would require fresh context, the hook emits `codex_background_context_clear_suppressed ... result=in_pane_continuation` and continues the queue in pane instead of returning control to the supervisor to send `/clear`. Route-in-flight, active-turn, pending-clear, and one-clear-per-head gates remain mandatory for explicit clear sources. When the project is not opted in, the continuation stays silent (no marker, no pre-emptive clear). This gives the operator structured hook-path log proof to confirm or deny that a queue-turn clear was suppressed without re-deriving it. The `s760_clear_decision_clear_true` gate verifier accepts only a real anchored `^[<timestamp>] [s760] clear-decision ...` line (the bracketed `<timestamp>` may be a bare epoch or an ISO-8601 UTC stamp, `#opslogts`) with `optIn=true`, `pct >= threshold`, and `clear=true`; quoted prose in queue-diff logs is not proof.
- The direct invocation cycle is represented by `flow::session_cycle`: prompt-target extraction, plan/backlog-only scope selection, pending-mutation finalize requirements, and the required `finalize` command shape are derived from one typed contract that `preflight` and `plan` share.
- `finalize` / `write --commit` must reject stale snapshot/CRDT reset drift before applying granular `--backlog-*`, `--review-*`, `--icebox-*`, or `--status` mutations. A closeout that cannot safely place the exchange response must not still mutate backlog/review/icebox/status state, because that creates active queue work without the response/proof that explains it.
- After its pre-commit repair step, `run` rechecks the diff before child-agent dispatch. If the repair consumed the whole diff because the response patchback was already committed and no new assistant response body was supplied, `run` must fail before invoking the configured backend and point the operator to `agent-doc write --commit <FILE>`.
- Raw `commit` continues to refuse editor-authored typed-component drift when the staged snapshot already matches `HEAD`, because that drift is an unanswered prompt. Direct `run` consumes the refusal's typed `UnansweredEditPending` verdict as permission to answer that exact real diff without pre-committing it; synthetic queue prompts and every other refusal remain fail-closed.
- The pre- and post-repair closeout-drift gates share the same fresh-prompt exemption. A proven route/queue snapshot commit boundary recovers before that exemption because it belongs to the prior dispatch; the live prompt belongs to the next response cycle. If repair reaps completed backlog state into the snapshot while preserving that prompt only in the visible document, the maintenance remains pending for the response transaction and is not misclassified as orphaned closeout drift. A live prompt does not suppress the narrow inline-boundary-fragmentation repair: that transformation preserves the complete prompt and crosses the normal authority-fenced write before broad template normalization is gated.
- `#codex-owned-pane-prompt-miss`: when a Codex-owned pane re-invokes `agent-doc <FILE>` for the document it already owns **and** an unresolved exchange prompt is still pending, `run` must fail closed *before* pre-commit and before `start_run_cycle`. The diagnostic must name the unresolved prompt and the in-pane recovery path (answer it in this owner pane's current turn, then persist with `agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`) and must tell the operator not to re-run the same direct command from the same pane. Because the early guard bails before pre-commit, the prompt stays uncommitted and executable rather than being baselined into `HEAD`. The detector is a strict subset of the recursive same-pane case, so non-recursive runs are unaffected.
- `#hook-owned-cycle-reentry`: a harness `UserPromptSubmit` hook that re-enters preflight for a cycle just opened by the same owner pane must identify its harness from the hook entry point, not ambient subprocess variables. Pane and harness must still both match the authoritative actor before the fresh uncaptured cycle can be preserved; foreign-pane and harness-handoff recovery remains fail-closed.
- `#preflightoverrunphase` (GH #78): a `UserPromptSubmit` admission that exceeds its budget must name the **measured** cause, never a fixed guess. Preflight marks each top-level step with `progress::enter(<phase>)` on a per-run handle the hook installs on the worker thread, so concurrent runs never share a record. On overrun, the refusal names the phase still running and how long it had run, lists the completed phases costliest first, names an accepted `agent-doc admin inspect <FILE>` invocation (the bare form is rejected without a target), and states that the abandoned worker stops when the hook process exits, so it cannot race the next trigger. The same breakdown goes to stderr and ops.log as `preflight_admission_overrun ... running=<phase>:<ms> completed=<label>:<ms>/<n>x ...`.
- `#codex-owned-pane-auto-queue-stuck`: when a Codex-owned pane re-invokes `agent-doc <FILE>` for the document it already owns **and** a ready active go-mode queue head remains (no unresolved exchange prompt — the prompt-miss guard above takes precedence), `run` must also fail closed *before* pre-commit / `start_run_cycle` via `queue_continuation::detect` + `owned_pane_queue_handoff_diagnostic`. The diagnostic names the live head (and id), the in-owner-turn `finalize` / `write --commit` recovery, and warns against re-running the same direct command. Bailing early keeps the queue head live and avoids the pre-commit queue/boundary drift the late recursive guard would otherwise leave behind.
- `#recguard-wedge-escape`: the `#codex-owned-pane-auto-queue-stuck` guard fails closed correctly, but in a self-driving go-mode `agent:queue` loop with no operator watching, a busy owner pane that re-invokes `agent-doc <FILE>` **mid-turn** (Option B `#codex-self-reinvoke-prevent` only redirects the *Stop-hook* continuation, not a mid-turn re-run) can trip the same guard on the same head every cycle — an unbounded retry storm. `run` tracks the count of *consecutive* owner-pane self-invocation guard fires for the same head (`recguard_wedge`, keyed on the head text; a different head resets the count, and consuming a head in `write` clears it). Once the count reaches `WEDGE_THRESHOLD` (3), `run` breaks the dead-loop: it halts the runaway queue (`frontmatter::merge_queue_hold` → `queue: pause`, `#queuestopremove`), clears the counter, logs `recursive_self_invocation_wedge_halt` (file, head id, count), and bails with an escalated diagnostic naming the wedge and the one recovery action that actually advances the head (answer in the owner turn + `finalize` then delete the `queue: pause` line, or `agent-doc start <FILE>` and trigger from outside the owner pane). The head stays live and no snapshot/queue drift is committed. Healthy loops that self-invoke at most twice in a row never escalate.
- `#recursion-guard-wedge-escape`: the `start` entry (`agent-doc start <FILE>` and the bare `agent-doc <FILE>` start path) must apply the same recursive self-owned-pane guard as `run`. When `start` is invoked inside the Codex pane that already owns the document (`run::recursive_codex_start_invocation_diagnostic`, a Codex-only owner-pane self-invocation), it must fail closed *before* relocating panes or spawning a replacement owner — otherwise it loops re-injecting `agent-doc <FILE>` into the owner pane (the self-owned-pane recursion wedge with no clean operator escape). The guard fires unconditionally (even under `--force`, since the deadlock is inherent to same-pane nesting; `--force` only governs cross-pane stale-registration reuse), logs `start_recursive_self_owned_pane_refused` (file, pane, session id), and bails with an out-of-pane recovery path: reconcile a possibly stale-busy actor with `agent-doc session status <FILE>`, then if the pane is genuinely wedged run `agent-doc session interrupt-clear <FILE>` from a different pane, escalating to `agent-doc session interrupt-clear <FILE> --force` when normal interrupt/clear cannot settle. The force path is the explicit destructive hatch: it closes the actor when possible, removes the sessions registry projection, signals the supervisor/child PIDs, kills the owner pane, removes the supervisor socket, writes clear cooldown, and reclaims an empty orphaned preflight cycle in one command. Never re-run `agent-doc start <FILE>` from the wedged pane. The detector requires `detect_harness() == "codex"`, so it only fires inside a live Codex agent that owns the doc and never blocks a legitimate same-pane restart from a bare shell after the supervisor exits.
- `#routequeuesyncpending`: route queue enqueue and activation may repeat current-document resolution once when an attached editor exhausts `sync_pending` recovery and consequently refuses disk-read authority. The second bounded observation window lets asynchronous replica re-registration and retained-intent replay converge; a second exhaustion or any unrelated resolver error still fails closed, and the route must never descend to disk while the editor remains authoritative. Queue recovery also treats the cosmetic `🚧` marker as presentation state when comparing `do [#id]` identities, so an abandoned prompt mirrored as `- 🚧 do [#id]` remains eligible for binary-owned commit recovery.
- The late recursive same-pane deadlock guard (`#recguard-abandon`) still applies for a genuine non-queue, non-prompt dispatchable diff in the owner pane: it abandons the empty `preflight_started` cycle as terminal so the owner session is not wedged. `session-check` is the backstop — an abandoned `recursive_direct_invocation_blocked` cycle whose document still carries an unresolved exchange prompt with no later response is reported as a missed-prompt recovery, not accepted as terminal closeout.
- `#codex-owned-pane-prompt-miss-followups` (structured result): `preflight` emits a typed `owned_pane_self_invocation` field (file, current pane, session id, actor generation/state, `kind` = `unresolved_prompt` | `active_queue_head`, work excerpt, optional head id, and the exact persistence command) whenever the document is a Codex owner-pane self-invocation with unresolved exchange work. An unresolved exchange prompt (derived from the cycle's prompt-target diff so it survives the post-commit boundary) takes precedence over an active auto-queue head. Codex guidance reads this to drive an in-pane response cycle instead of only reading the run-time bail diagnostic. The field is null for non-owner panes, non-Codex harnesses, and documents with no unresolved exchange work.
- `#queue-continuation-buries-prompt`: `unresolved_exchange_prompt` (the snapshot-independent detector that backs the `session-check` queue-continuation guard and the `run`-path precedence) must not treat a **queue-continuation** response heading (`### Re: do [#id]` / `### Re: re [#id]`, any h-level) as answering a preceding **free-text** user prompt — that response answered a queue/backlog item, not the prompt. A free-text exchange prompt followed only by queue-continuation responses stays unresolved, so a queue continuation (including a concurrent second actor draining `agent:queue auto`) cannot advance the boundary past an unanswered user prompt and bury it in the snapshot (the JB "agent-doc ignored my previous prompt" failure). The tail scan stops at the first response heading so a queue-continuation's own response body is never mistaken for prompt text; a genuine free-text `### Re:` answer still resolves the prompt (no false positives).
- After opening the response `preflight_started` cycle, `run` emits parent-visible heartbeat stderr during long child-agent waits every `AGENT_DOC_RUN_HEARTBEAT_SECS` seconds, defaulting to 30. In a tmux pane owned by a Codex/OpenCode parent harness with terminal stderr, routine run/diff/commit stderr is redirected to `.agent-doc/logs/run-stderr.log` unless verbose input diagnostics are enabled, so progress output cannot paint over the foreground TUI. Each heartbeat preserves the open phase while updating the cycle state's `updated_at` and `last_event` with the current phase, elapsed time, timeout budget, and agent name.
- If the pending diff contains executable directives such as `do #id`, `run tests`, `build + install`, `commit + push`, `go`, or imperative pending-item prose, status-only or meta-only agent replies are invalid. The response must contain either concrete execution evidence or a concrete blocker.
- If the diff contains a bare `compact exchange` request, `run` must fail closed and direct the caller to `agent-doc compact <FILE> --commit`.
- Once a cycle records `committed`, later repair bookkeeping must not rewind the persisted cycle state to `response_captured` or `write_applied`.

## compact

`agent-doc compact <FILE> [--component NAME] [--keep N] [--message TEXT|-] [--tag NAME|skip] [--commit]`

- `--lint off|warn|strict` overrides the dialect lint gate mode exactly like
  `agent-doc write --lint` (CLI > frontmatter `agent_doc_lint_dialect` >
  `.agent-doc/config.toml` `[lint] dialect` > default `warn`). The CLI forwards
  it to the controller as the optional `lint` field of the compact invocation;
  an absent field resolves the mode as before. The mandatory integrity gate is
  never bypassed by `--lint off` (GH #227).

- A component compact has a component-scoped compare-and-swap boundary. For
  `--component exchange`, only the attributes and content of `agent:exchange`
  participate in drift detection. Frontmatter and all sibling components are
  rebased from the latest authoritative document and preserved verbatim; their
  changes are irrelevant to whether the Exchange compact can converge. Drift
  within `agent:exchange` remains fail-closed.
- Template-mode full exchange compaction must split the component at the live `agent:boundary` marker. Content before the boundary is archiveable and may be summarized; content after the boundary is unresolved live prompt drift and must remain visible in the working tree while staying out of the archive body, compact summary digest, saved snapshot, and closeout commit.
- Template-mode partial exchange compaction follows the same unresolved-tail rule when keeping recent `### Re:` sections.
- `--commit` closes compacted state through the normal binary-owned commit path. It commits only the compacted snapshot state; any unresolved post-boundary prompt left visible remains the next prompt-bearing diff for a later `agent-doc <FILE>` cycle.
- Controller-local compact continuation facts must pass through the same typed
  admission and deduplication boundary as external events and enter the existing
  document graph before publication returns. Compact replies must not wait for
  whole-project ledger rehydration to make a retained continuation visible.
- The initiating command treats its typed `retained_pending` outcome as accepted
  asynchronous work and exits successfully after printing the continuation note;
  only a repeated/foreign `already_pending` request exits nonzero. Neither outcome
  claims that the compact is already in HEAD.
- Editor-visible compact text still requires an exact native-save receipt. Both
  pending delivery and an already-visible retained target keep the latest-durable
  save effect active until the write settles; visibility alone must not strand
  the compact continuation before snapshot and commit. A semantically rebased
  visible target saves that exact current text without emitting another CRDT write.
- The controller must carry that intentional split as two targets through the whole transaction. Editor-buffer flush and relay fallback repair use the live target (including unresolved input); snapshot staging and post-commit HEAD verification use the committed target. Closeout must not force a whole-buffer relay rebootstrap when the live target is already converged, and must fail closed instead of overwriting a concurrent live-editor change.
- If compact reaches the pre-write barrier while an earlier editor delivery is retained, it may subsume that pending projection only when the typed retained state and the relay's exact canonical text prove the same base used to derive the compact successor. The successor still requires normal CRDT compare-and-swap and editor-delivery settlement. Relay drift, detached authority, and unrelated errors remain fail-closed, and this state must not prescribe `commit` or `write --commit` because no response cycle owns the earlier projection.
- The retry guidance for that pre-write-only refusal names the compact mutation:
  retry the same compact after editor convergence. It must not fall through to
  generic response-cycle recovery guidance or suggest a forced disk write.
- An exchange whose archiveable body is nothing but one rendered compact
  summary (the `### Session Summary` heading, its `*Compacted. ...*` archive
  pointer, the `Compacted content:` header and `- ` digest bullets) is already
  compacted: compact is a no-op there, like an empty component, and writes no
  archive (`#compactsummaryonly`). Re-compacting it would only archive the
  summary into a summary whose digest is a pointer to a pointer. Any other text
  before the boundary is real content and still compacts.

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

## read

`agent-doc read <FILE> [--component NAME]` prints the current authoritative
document content, or the body of one named template component. An unknown
component name is an operator usage error on both attached and detached
documents: it exits unsuccessfully with the document's valid component names
and never emits the dogfood `ACTIONABLE_AGENT_DOC_FIX_PROMPT` terminal-failure
banner.

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

`agent-doc reset <FILE>` clears the saved session id, cold snapshot, and document-keyed recovery rows in `state.db`. `agent-doc reset --from-current <FILE>` clears the saved session id and rebuilds the cold snapshot plus `state.db` CRDT recovery checkpoint from Lazily current markdown. It never imports a file-sidecar candidate into live authority.

`agent-doc reset --from-current --preserve-session <FILE>` refreshes the cold snapshot and recovery checkpoint while leaving frontmatter and the active `state.db` transaction intact. The retained response intent is rebased semantically onto the operator-approved current markdown; no capture or baseline file is rewritten.

## clean

`agent-doc clean <FILE>` squashes all `agent-doc:` commits for the file into one via `git reset --soft`.

## gc

`agent-doc gc [--root DIR] [--dry-run] [--database-only]`

- `--database-only` skips file scanning, actor cleanup, and supervisor effects. It reports allocated/free bytes and ledger row counts, runs the existing superseded-history retention on database open, and reclaims sufficiently fragmented free pages. It preserves live recovery facts, queues, and session documents. Pairing it with `--dry-run` uses a read-only SQLite connection without schema initialization or retention; a missing database remains absent. Reported free bytes are reusable pages, not a promise that every byte will be returned to disk.

- Garbage-collects orphaned cold snapshots, locks, hooks, repair diagnostics, sockets, and dead transactional rows under `.agent-doc/`.
- The orphaned-socket cleanup keeps sockets whose supervisor PID is alive or whose socket still answers.
- Stale `starting` actor records older than one hour are closed unless a live supervisor PID still has a fresh supervisor heartbeat proving the actor is booting; this updates the controller SQLite store transactionally. A live PID with a stale heartbeat is treated as stuck startup state.
- A controller wedged in handoff `Preparing`/`Promoted` past the seconds-scale stuck-handoff threshold (`AGENT_DOC_STALE_PREPARING_CONTROLLER_SECS`, default 45s) is terminated (#kqr6 / #sjwm / #stuckhandoff). Unlike the stale-`starting` actor cleanup — which closes a projection record and cannot stop a live process — this kills the live wedged `controller serve` process (verified by `/proc` cmdline and never self) so it stops racing the IDE listener on `ipc.sock`, then supersedes the bootstrap with `Failed` so the next bind promotes a clean controller and the `1002 → 1004 → 1006` respawn loop cannot continue. A promoted controller's immutable argv can still say `--handoff-state preparing`; that argv is discovery evidence only. While the authoritative bootstrap records a fresh replacement handoff, orphan scanning uses its handoff start time rather than the predecessor's process age and cannot reap the healthy predecessor/replacement transition. It logs `stale_preparing_controller_reaped pid=… generation=… age_secs=… caller=…`. The same reaper runs as a self-heal step at controller bind (`connect_or_launch`) before any handoff/promote, and is exposed for operators as `agent-doc admin reap-stale-controllers [--dry-run]` (replacing the manual `pkill -f 'controller serve … --handoff-state preparing'`). `--dry-run` reports without killing.
- A detached controller serving a test-style temporary project root (`.tmp*`
  directly under `/tmp`, `/var/tmp`, or `/dev/shm`) exits after 60 seconds with
  no active or newly accepted clients. The classifier is deliberately narrow;
  persistent project roots and live clients are never reaped by this idle rule.
- Prunes accumulated pre-mutation recovery tags (`#x8aw`): keeps the newest `KEEP_RECOVERY_TAGS` (20) `agent-doc/<doc>/pre-auto-run-N` and `pre-compact-N` tags **per `<doc>/<slug>` series**, deleting older ones. One tag is created per queue auto-run / compaction, so without pruning they grow unbounded over a document's life. Best-effort: a non-git root or git failure is a no-op. `--dry-run` reports the deletions without applying them.
- `preflight` runs the full orphan-file GC automatically at most once per day via a coordination throttle in `.agent-doc/state.db`; `preflight`, `start`, and `sync` still run the lightweight stale-`starting` actor cleanup every cycle.

## checkpoint

`agent-doc checkpoint <FILE> [--restore TAG] [--diff TAG]`

- Guided recovery for the pre-mutation checkpoint tags created by `compact` (`pre-compact-N`) and the queue auto-run (`pre-auto-run-N`, `#misfire-recovery-snapshot`). Named `checkpoint` because `recover` is already a `repair` alias for orphaned-response recovery, a distinct concern (`#kc5e`).
- Default (no flags): lists the document's checkpoint tags newest-first (commit date, then ordinal) with short SHA, date, and subject, plus the inspect/restore command hints.
- `--diff TAG`: prints `git diff <TAG> -- <FILE>` so the operator can see what changed since the checkpoint.
- `--restore TAG`: runs `git checkout <TAG> -- <FILE>`, restoring **only** that document from the checkpoint (other files untouched), then prompts the operator to review and commit. Surgical and non-destructive to unrelated files — it never resets the whole tree.
- A document with no checkpoint tags prints guidance rather than erroring.

## preflight

`agent-doc preflight <FILE>` emits non-blocking `warnings[]` in its JSON contract. When frontmatter `agent:` is set and differs from the active harness detected from Claude Code, Codex, or OpenCode environment markers after alias normalization, preflight emits `code: "harness_mismatch"` and keeps running; the skill surfaces the warning and continues with the active harness attribution and closeout path.

When `agent_doc_format: template` selects strict component-patch closeout, preflight emits a structured `response_contract` naming `--template`, the matching `<!-- patch:exchange -->` / `<!-- /patch:exchange -->` envelope, and `plain_stdin_allowed: false`; it also prints the same requirement as a concise diagnostic before the JSON. Inline/append documents omit this field. This is an early instruction contract only: write/finalize still reject markerless strict-template input before capture or mutation.

When the active document lives in (or beside) an `agent-doc` source checkout, preflight also emits `code: "stale_install"` if any installed/built artifact (`~/.cargo/bin/agent-doc`, the lib-installed `~/.cargo/bin/libagent_doc-*.so` cdylib, or the freshest built binary/cdylib from `target/release` and `target/local-install/release-local`) predates the newest buildable source-file mtime by more than a 300-second grace window (`#install-stale-guard`). The warning distinguishes a committed gap from newer uncommitted source edits, but remains advisory in both cases: an unrelated document session surfaces it and continues without rebuilding, while the cycle that owns development/release of the agent-doc source tree owns `make install` and the supervisor owns the safe recycle. This prevents concurrent source-tree development from stalling an otherwise independent queue drain while still identifying committed fixes that are not yet active. The check is best-effort and silently no-ops when no `agent-doc` source repo is locatable (for example a prebuilt or PyPI install); the source repo is found at the document's git root or its `src/agent-doc` submodule.

Before preflight performs document-mutating recovery, commit, pending maintenance, or duplicate-residue cleanup, it waits for the shared editor typing indicator to become idle. The recorded cycle baseline is captured from the same stable visible content used for diff computation, not from an earlier pre-debounce cleanup projection. It is binary-owned cycle state; preflight emits no `baseline_file` path and no command accepts one.

`agent-doc preflight --probe <FILE>` runs the same inspection (recovery, commit, queue analysis, diff, JSON output) but is a **pure inspection probe**: it never opens a `preflight_started` cycle (`#preflight-probe-side-effect-free`). The default (dispatch/response-bound) preflight opens that cycle so the upcoming response is bound to it; a diagnostic probe is not response-bound, and an open `preflight_started` cycle left by a probe is exactly the state that later wedges `session-check`. Use `--probe` for diagnostic/recursive-guard inspection so the probe leaves no open cycle behind (a terminal `committed`/`abandoned` cycle from the idempotent commit step is still acceptable). Internal response-bound callers such as `orchestrate` keep the default cycle-opening behavior.

A response-bound preflight invoked from the authoritative actor's own pane may re-enter its existing fresh `preflight_started` cycle (`#preflightinbinary`) instead of treating that cycle as interrupted. This exception requires both pane and live harness to match the generation-fenced actor record, and the cycle must remain uncaptured and younger than the stalled-cycle deadline. Registry-only, desired-harness, non-owner, stale, and response-captured cycles receive no exception and continue through fail-closed recovery. This ordering lets a same-owner sibling queue edit be classified against the active turn scope without abandoning or falsely committing the response that is still running.

## audit-docs

`agent-doc audit-docs [--root DIR]`

- Audits instruction files such as `CLAUDE.md`, `AGENTS.md`, `README.md`, and `SKILL.md` for path accuracy, actionable content, and line budget.
- Discovery prunes heavy skip directories before descent so audit time is spent on real instruction surfaces.
- Generated agent-doc instruction surfaces are audited as release artifacts: if a root `AGENTS.md`, `.codex/AGENTS.md`, `.opencode/skills/agent-doc/SKILL.md`, or `.claude/skills/agent-doc/SKILL.md` still carries the agent-doc managed frontmatter/sections, it must match the content rendered by the running binary. Without `--root`, a submodule checkout audits the git superproject install root used by normal release installs **and** the submodule checkout itself: `skill install` resolves to the superproject, so submodule-local copies such as `src/agent-doc/.claude/skills/agent-doc/SKILL.md` are otherwise never content-checked (`#skillinstallstalemirror`). With explicit `--root DIR`, generated surfaces are checked under `DIR` exactly. Custom root instruction files that do not look agent-doc-managed remain user-owned and are not rewritten or failed for content mismatch.
- Installed runbook bundles are audited against the running binary (`#skillinstallstalemirror`). In a managed harness runbook directory (`.claude/skills/agent-doc/runbooks/`, `.opencode/…`, `.codex/skills/agent-doc/runbooks/`, `.cursor/rules/runbooks/`) a stale file, a missing file, and unbundled Markdown all block. A surviving retired `.codex/runbooks/` mirror blocks until `skill install` migrates it away. A directory listed in `skill_runbook_mirrors` is shared with project-owned runbooks, so it blocks on a stale or missing bundled file only and never on extra Markdown.
- `make check-fast` is the affected-scope iteration tier; it follows changed Rust packages through their transitive reverse dependents and dispatches relevant non-Rust checks, but never produces release proof.
- `make check` runs `audit-docs` once after integration and release-version projection and records a worktree-local content/toolchain receipt. `make release` first runs cheap version/editor-parity checks, then reuses independently recorded full-suite and tmux-CI receipts only when repository paths/bytes and toolchains still match; staging, committing, or tagging identical bytes does not invalidate proof. An audit reachable only from `precommit` therefore cannot gate a release.
- Release coordinators integrate every completed claimed queue fix ready at the pre-version batch boundary, then run one version/check/install train. `make release` times and reports `tag-publish-handoff` independently from `local-install-full`; it runs the full-profile install once, after the tag push, and preserves a successful publish outcome even when that local install fails.
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
- Gathers a source-grouped diagnosis report from `.agent-doc/logs/ops.log`, the `state.db` event/projection ledger, harness session logs, editor/plugin debug logs, Codex hook records, hook payloads, and controller actor/session state. Large recovery projections may be summarized as cold evidence, but live state is never inferred from a file transport.
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

- The installed skill always renders `agent-doc-version` from the running binary version. Installed copies are the installer's output, never a `release-version` sed target: stamping a new version into a copy whose body was never regenerated makes a stale install look current to every version-marker reader (`#skillinstallstalemirror`). `release-version` projects the version into bundled skill *sources* only and then invokes the installer.
- `--root DIR` installs into an explicit root. Bare root resolution prefers the git superproject, so a submodule-local install (for example `src/agent-doc/.claude/skills/agent-doc/SKILL.md` in a dogfooding checkout) is unreachable without it; `make install` and `make install-full` run both the bare and the `--root .` install.
- Runbook install targets: the four managed harness directories are exclusive (unbundled Markdown is reaped), directories listed in the project `skill_runbook_mirrors` config are refreshed in place but never created and never reaped, and the retired `.codex/runbooks/` mirror is migrated away.
- Harness-specific reload flows must use explicit `--harness` selection rather than environment guessing.
- Harness installs refresh a managed root `AGENTS.md` mirror when it still looks generated, so `.codex/AGENTS.md` and the root mirror cannot drift across `agent-doc-version` bumps. Custom root `AGENTS.md` files are opt-in and must be preserved.
- Generated Claude, Codex, and generic hot-path instruction surfaces must stay compact: the shared source template is budgeted at 140 lines, and rendered harness-specific surfaces are budgeted at 150 lines. Rare recovery detail belongs in bundled runbooks rather than the always-loaded skill body.

## outline

`agent-doc outline <FILE> [--json]` reports markdown heading structure, line counts, and approximate token counts.

## board

`agent-doc board [ROOT] [--json] [--dag] [--all] [--no-submodules]` renders one severity-ordered fleet **work** view: the queue and backlog of every session document across a superproject and its submodules, grouped into one section per project.

It is the complement of `admin dashboard`, not a replacement: `admin dashboard` answers "which controllers are alive", `board` answers "where is the work, and what is not moving". Both may be open at once.

- **Root discovery** climbs to the outermost working tree with `git rev-parse --show-superproject-working-tree`, then takes every `.gitmodules` path that exists and carries its own `.agent-doc/`. Each such root is its own section, so `src/sample-service`, `src/sample-app`, and `src/sample-portal` never share a section with the superproject. `--no-submodules` restricts the board to the given root; a root with no session documents produces no section.
- **Document discovery** per root is the durable session registry plus a bounded filesystem scan. The scan never descends into another board root, a hidden directory, a leading-underscore aside/quarantine directory, or `node_modules` / `target` / `dist` / `build` / `vendor` / `tmp`, and it skips `*.done.md` archives. A document therefore belongs to exactly one project section.
- **States are severity-ordered, worst first**: `BLOCKED` (an auto-DAG item is blocked on a decision), `STALLED` (a drainable queue head with no live actor), `OPERATOR` (only operator-gated heads remain), `DRAINING` (a live actor is draining), `READY` (open work, nothing queued), `CLEAR` (nothing open; hidden unless `--all`). The first three count as needing attention; both rows within a section and sections themselves sort by worst state.
- **The load-bearing signal is `STALLED`**: the queue says there is agent-drainable work and no live actor is draining it. Everything else on the board is ordered under that.
- **Columns** are `state`, `document`, `queue` (drainable, or `drainable+deferred`, or `stopped` for a parked queue), `backlog`, `review`, `auto-dag` (the per-document lane rollup), `actor`, and `needs` (one phrase naming why an attention row needs a person). `--dag` appends the per-project auto-DAG lane rollup.
- **Read-only.** `board` never writes a document, claims a pane, or mutates controller state, and a project that fails to load degrades to a missing section rather than aborting the board.
- `--json` emits the whole board under the stable `agent-doc-fleet-board-v1` contract version. Classification, ordering, and rendering are pure and live in `agent_doc_work_graph::fleet_board`; discovery and parsing live in the binary.

## dashboard

`agent-doc dashboard [ROOT] [--json] [--write [PATH]] [--watch] [--interval-ms N] [--all] [--no-submodules]` renders the Agent Doc dashboard (`gvqv`): the `board` work view composed with the controller/supervisor liveness of `admin dashboard`, as one markdown document. (`dashboard` was previously an alias of `board`; it now prints that board plus liveness.)

- **Sections**: a one-line summary (documents needing attention, project controllers running, actors and flagged actors), `Needs attention` (every `BLOCKED` / `STALLED` / `OPERATOR` row with its reason), `Work board` (the `board` table per project, document cells linked relative to the file), and `Controllers and supervisors` (per project: live `controller serve` pids from one `/proc` scan, then the `admin list` actor rows with pane liveness, supervisor pid, and `admin detect` finding kinds). Per-actor `controller inspect` diagnostics are deliberately omitted so a refresh never round-trips through controller RPC.
- **Scope** follows `board`: the superproject fan-out by default, the given root only with `--no-submodules`.
- Without `--write` the markdown (or the `agent-doc-dashboard-v1` JSON model with `--json`) goes to stdout.
- **`--write [PATH]`** writes the projection atomically (temp file + rename) to PATH, default `.agent-doc/dashboard.md` under the project root. It refuses to replace an existing file that is not a dashboard projection. A rewrite whose body (everything but the `_Last change:` stamp) is unchanged writes nothing, so an open editor reloads only on a real change.
- **Live updates are controller-owned.** Writing the default path arms the project controller: it re-renders after any controller state change (state event or memory refresh), debounced 500 ms, plus a 5 s poll for operator edits that never reach the controller, with renders spaced at least 1 s apart. An absent file costs one `stat` per poll; deleting it disarms the refresh. Document facts are memoized by `(mtime, len)`, so a warm refresh re-parses only changed documents. The projection's first line, `<!-- agent-doc-dashboard v1 scope=fleet|project all=true|false -->`, records the render parameters the controller re-renders with. `--watch` keeps the invoking process re-rendering instead (for a custom PATH, or with no controller running).
- **Never a session document.** The projection has no frontmatter, contains no `<!-- agent:` marker and no `agent_doc_*` token (free text is escaped; such links are percent-encoded), lives under the gitignored `.agent-doc/` by default, and its first-line marker makes `is_agent_doc_document`, the `board` / `serve` scans, and therefore the cross-document sweep and editor tab sync reject it even under `auto_session_for_all_md` or a `**/*.md` include glob.
- Editor plugins expose it as the **Dashboard** action (`editors/SPEC.md` § 6c).

## upgrade

`lib-path` is a machine-only native bootstrap query: it prints exactly the
existing platform library path on stdout and skips startup update checks and
notices on both streams. Failure diagnostics remain on stderr. JetBrains must
read only stdout for this protocol and inherit stderr separately, including
when an older binary emits an upgrade notice. Rejected candidates report the
exit code, stdout candidate, and existence check.

`version [--json]` reports the running binary's build identity: `version`, the
IPC `build_id` (`<version>+<source digest>`, the identity the handshake
compares), the `executable` path, the sibling native `library` that `lib-path`
would print (or `null`), and `expected_plugins.{jetbrains,vscode,zed}`, the
editor package generations this build was compiled against (`null` when that
manifest was absent at build time). `--json` emits it under the
`agent-doc-build-info-v1` contract; the text form starts with the same
`agent-doc <version>` line as `--version`. Like `lib-path`, it skips startup
update checks and notices. The native library exposes the same report through
`agent_doc_build_info_json()` (`component = "native_library"`, no paths), so an
editor's "About Agent Doc" action can prove whether the CLI and the loaded
library are one build (`editors/SPEC.md` section 6b).

Release artifacts ship the platform cdylib beside the binary
(`libagent_doc.so` / `.dylib` / `agent_doc.dll` in each release archive, and in
the wheel's `.data/scripts/` so `pip install` lands it in the same `bin/`).
`lib-path` resolves the library as a sibling of the executable, so a package
install that omitted it could only run the editor plugins in the degraded
file-based-IPC mode (GH #52). The missing-library remedy branches on how the
binary was installed: a Cargo build tree is told to `cargo build --release`, and
a package install is pointed at a release asset or `agent-doc lib-install
--source <dir>` rather than at a toolchain it does not have.

`lib-install` refuses a library whose embedded IPC build id
(`<version>+<source digest>`) differs from the binary it pairs with — the
installing binary itself, or for `self-install` the binary it just built
(`#installbuildskew`). The handshake rejects every peer on a different build
id, and the editor's `reload_library` recovery loads the installed library, so
a skewed pair can never converge: every editor intent stays refused until the
next install. Because the id is a digest of the working tree, `make install`,
`make install-full` and `self-install` build the binary and the library in one
cargo invocation (one build-script run) before installing either; two
invocations let a concurrent edit of the shared tree land between them.

`lib-install`'s controller fan-out never starts a controller for a project
root nobody has open (`#installworktreecontrollers`). Running controllers are
found by walking `/proc`, which also finds idle subagent worktrees and dev roots
left behind by earlier sessions; recycling those launched a replacement each
time, and the `reload_library` status query launched one wherever the socket did
not answer, so idle roots kept a controller forever. A root counts as in use
only with a listening PID-scoped editor socket or an open `agent-doc start`
supervisor serving a document in it. Idle roots are skipped by the recycle
(`install_fanout_recycle_skipped reason=idle_project_root` in their `ops.log`)
and are reached by `reload_library` through an already-running controller only
(`reload_library_idle_root_skipped` when none answers). A controller on a
replaced binary whose root is idle, that owns no documents, and that has had no
client for 60s retires instead of handing off
(`controller_idle_root_retired_instead_of_handoff`). Test fixtures that launch a
controller own a `ProjectControllerReaper` from `agent-doc-test-support`, which
terminates every controller rooted under the fixture directory when it drops,
including during a panic, and a controller rooted anywhere inside a
`<temp>/.tmpXXXX` directory (for example `<temp>/.tmpXXXX/project`) shuts itself
down after 60s without a client, so a test process killed before its `TempDir`
dropped no longer leaks one.

Every `lib-install` writes a new `libagent_doc-<version>.so` beside the binary
and swaps the unversioned symlink onto it, so the directory used to grow by one
library per install with nothing ever reaping the predecessors
(`#gclibsoninstall`, GH #58). A successful `lib-install` now sweeps its own
target directory: it removes every versioned library that is neither the current
symlink target nor held by a live PID lock, and removes `\*.pid.<pid>` locks
whose PID is no longer alive — an indefinitely-trusted dead lock would otherwise
make its version look held forever once that PID is reused. The sweep is
best-effort and never fails an install that already succeeded. The same policy
is reachable on demand as `agent-doc gc-libs`, and `agent-doc gc-libs --dry-run`
reports what it would remove, and why (superseded by the current library, or not
the installed library — in either case with no live holder), without deleting
anything.

Every tag publishes six hosted targets on demand: three Linux targets, Windows,
and both Darwin architectures. The standard macOS runners are included for this
public repository, so the release workflow builds each Darwin archive with its
binary and `.dylib` and includes it in the initial checksum manifest.

`make release-macos-assets TAG=v<version>` remains a manual repair path for a
legacy or failed Darwin upload. `make release-macos-coverage-check` names any
published release that dropped either Darwin archive, counting a half-shipped
pair as a drop.

`agent-doc upgrade` checks GitHub Releases for a newer stable version and upgrades
through the prebuilt GitHub binary / `pip` cascade. A prebuilt archive is installed
only after its bytes match the release's `SHA256SUMS` entry. The agent-doc Rust
workspace is private and is not a crates.io upgrade source. The one-shot path
also reconciles every already-installed JetBrains and VS Code-family plugin to
the latest release (GH #107) — after a successful binary upgrade, and also when
the binary is already current, so a workspace skewed by an earlier binary-only
upgrade is repaired by re-running it. A release can split one fix across the
binary and the plugin, so a failed plugin reconciliation exits non-zero and says
the release is not fully installed; it never reports success over a skew. When
the binary upgrade itself fails, plugins are left alone rather than moved ahead
of the binary.

`agent-doc upgrade --auto [--interval-seconds N]` is a foreground release watcher.
It checks immediately and then polls every 900 seconds by default; the interval
floor is 60 seconds. One recoverable per-user PID lock prevents concurrent
watchers. When a new release appears, it upgrades the binary and reconciles every
already-installed JetBrains and VS Code-family plugin, but never installs a plugin
into a newly discovered IDE. Binary and plugin failures are independent and retry
on the next poll. The watcher records the effective binary and reconciled release
in memory so the old process that replaced its own executable does not reinstall
the same release forever.

PyPI publishing is cadence-gated (`#pypicadence`): milestone tags (`vX.Y.0`)
publish automatically and any other tag publishes on demand with
`gh workflow run PyPI --ref <tag>`. Every tag still produces a GitHub Release.
Per-tag PyPI publishing exhausted the 10 GiB project quota at ~5 GiB/month, which
made uploads fail with `400 Project size too large` and left PyPI 20 versions
behind the newest tag without surfacing; the PyPI workflow now asserts the tagged
version is resolvable on PyPI after publishing so that drift fails loudly.

The runtime version warning cache lives at `~/.cache/agent-doc/version-cache.json`.

## plugin

`agent-doc plugin install|update|list <EDITOR>`

- Supports JetBrains and VS Code.
- Pulls assets from GitHub Releases, preferring signed assets when available.
  Published asset names are versioned (`agent-doc-jetbrains-0.2.392.zip`), so the
  signed preference matches the complete versioned package shape rather than an
  exact unversioned filename that no release has ever carried (GH #55).
  Selection must not depend on the order the API returns assets in. JetBrains
  install/update derives the platform branch from the versioned IDE data root:
  builds 242–261 select `agent-doc-jetbrains-<version>.zip`, build 262 selects
  `agent-doc-jetbrains-262-<version>.zip`, and an unprovable or unsupported
  target fails before any installed tree is replaced. `--local --all-installed`
  applies the same policy independently to every existing installation.
  When a live IDE owns a 262 target, the modular package has no restart-free
  dynamic upgrade entry point: the installer skips the attach, replaces the
  files, and reports restart-required naming that reason
  (`declined_by=modular_package`), not a legacy-package or upgrader failure.
- Downloaded editor packages are verified before they are extracted or handed to
  the editor CLI (`#editorpkgdigest`, GH #55). The expected digest comes from the
  release's `EDITOR-PACKAGES.sha256` manifest, and from GitHub's per-asset
  `digest` field when the release predates that manifest. A declared digest that
  disagrees with the downloaded bytes refuses the install; a release that
  publishes neither installs with an explicit warning, so `plugin update`'s
  fallback walk can still reach an older asset. `SHA256SUMS` stays
  platform-archives-only — the PyPI bootstrap launcher parses exactly that
  manifest.
- VS Code-family installs resolve the editor CLI (`cursor`, `codium`, `code`)
  BEFORE downloading, and report an absent CLI as a missing prerequisite with
  install guidance (GH #57). Detection returning a candidate it has just proven
  absent is a defect: the resulting `No such file or directory` reads as if the
  vsix were missing.
- JetBrains install/update treats the restart-free dynamic upgrade as an
  optimization, never the update itself (GH #63). The upgrader's JVM resolves
  from the target IDE first (the process executable when it is `java`, else a
  `jbr/bin/java` beside an ancestor of it), then `JAVA_HOME`, then `PATH`; an
  unresolved JVM names every candidate tried. When the dynamic upgrade fails
  for any reason (no JVM, attach refused, a platform signature the upgrader
  cannot call), or `--no-dynamic` (alias `--restart-required`) skips it under a
  live IDE, the package is still replaced on disk (directory removed and
  rewritten, so the live IDE keeps its old inodes) and the command succeeds
  with a restart-the-IDE warning naming the reason.
- A JetBrains upgrade staged for the next IDE start can never remove the
  installed plugin without installing its replacement (`#jbstagebackup`,
  following GH #115 and `#jbpluginvanish`). IntelliJ's pending-install executor
  (`StartupActionScriptManager.executeActionScript`, verified on 2024.2, 2025.2
  and 2026.1 builds) runs `action.script` in order, STOPS at the first command
  that throws, and deletes the script either way; it has no conditional
  command. The platform's own `delete:<plugin dir>` + `unzip:<package>` block
  therefore destroyed the plugin whenever the unzip failed at restart (package
  vanished, unreadable, disk full). The staged block is instead:
  1. `unzip:<package>:<probe>` — `<probe>` is `.agent-doc-jetbrains-probe-<v>+<nonce>`,
     a hidden sibling of the IDE plugins directory (same filesystem, never
     scanned as a plugin). A missing, corrupt or unextractable package throws
     here and aborts the script before anything touches the installed plugin;
  2. `delete:<probe>` — frees the space the probe used, so the real extraction
     fits wherever the probe did;
  3. `delete:<plugin dir>`, `unzip:<package>:<plugins>`, `delete:<package>`.
  The staging verifies the saved script holds exactly one probe unzip, before
  exactly one plugin-dir delete, before exactly one install unzip. There is no
  unguarded fallback (`#jbstagefallback`): on a build whose
  `StartupActionScriptManager` `DeleteCommand`/`UnzipCommand` classes are not
  constructible, the staging is refused before `action.script` is read or
  written (prior stagings stay as they were) instead of calling
  `PluginInstaller.installAfterRestart`, which would write the platform's
  unguarded delete-then-unzip block. The refusal (`restart required: refusing
  to stage ...`) is the upgrader's failure, so the launcher takes the
  failed-dynamic-upgrade path above: the package is replaced on disk and the
  command reports restart-required, with manual guidance to restart the IDE and
  rerun the install or install the package from disk.
  Rationale: a durable backup copy of the package would only cover the
  "package vanished" cause and only if something restored it before the
  restart; ordering the probe first closes the vanish AND full-disk causes at
  the moment they matter, using the executor's own abort semantics, with no
  extra state for agent-doc to keep alive.
- Preflight repairs stagings instead of only warning (`#jbstagebackup`):
  for each plugins directory whose install lock is free (a running install
  purges doomed stagings itself), a doomed staging — its package gone — is
  purged from a text-format `action.script` and reported once as
  `jetbrains_plugin_staging_repaired`; leftover `.agent-doc-jetbrains-probe-*`
  directories (a restart whose probe aborted) are removed; and agent-doc staged
  packages older than 10 minutes in an IDE whose `plugins/action.script` no
  longer exists are removed as orphans (nothing can unzip them). A package a
  pending script still names is never deleted. Current IntelliJ builds save
  `action.script` as a Java-serialized command array, which these text-format
  readers cannot parse (they leave it untouched, by design); the guarded block
  above is what protects those IDEs.
- JetBrains install/update success reports the installed plugin package version
  from the extracted plugin JAR, matching `plugin list`, rather than reporting the
  enclosing agent-doc release tag.
- An exhausted GitHub API rate limit reports the wait (`resets in 12m 3s`)
  alongside the raw epoch, not the epoch alone (GH #57).

## rename

`agent-doc rename <OLD_PATH> <NEW_PATH>`

- Transactionally rekeys typed state events and their embedded document hashes,
  retires old-path editor acknowledgement cursors, and updates the durable
  session registry.
- Filesystem sidecars remain untouched write-only crash state. Neither the
  explicit command nor auto-migration scans or imports them.
- Auto-migration through `ensure_initialized` handles the common rename path
  from session-registry identity; `rename` is the explicit old/new-path form.

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
- Supports add/edit/done/reopen/reorder/prune/list/gate operations against the canonical `agent:backlog` component.
- `backlog <FILE> reopen <ID>` is the explicit inverse of reap: it removes all
  same-id entries from canonical inline or external `agent:done`, restores the
  newest entry as an open item with the same id/text/continuation, and publishes
  the archive plus session document in one tracked-work transaction.
  `--queue` also removes a same-id struck directive and prepends one live
  `do [#ID]` directive without rewriting unrelated queue bytes.
- `backlog <FILE> requeue <ID>...` and `backlog <FILE> keep-unqueued <ID>...`
  repair the `#queue-clear-unrun-items` session-check finding (GH #129), a
  runnable queue head dropped while its backlog item stayed open. `requeue`
  appends one live `do [#ID]` per open id that has no live queue entry, through
  the tracked-work transaction; `keep-unqueued` records the ids as kept open on
  the last cycle's state, writing no document, when the operator removed the
  head on purpose. Both refuse an id that is not open backlog work. Neither
  needs an admitted cycle, so both stay legal after a preflight
  admission-deadline refusal, which forbids a response write but names these
  repairs as permitted; the finding clears without `write --commit`.
- Non-item separator lines and headings inside backlog/icebox must be preserved during mutation.
- Flush-left parent items are the tracked units; indented nested lists travel with the parent during edit/reorder/reap/transfer.
- `backlog <FILE> set-attr <ATTR> [VALUE]` / `unset-attr <ATTR>` mutate the
  component's MARKER attributes rather than its content (`#bkqattrcli`), through the
  same editor-converging write path as `add`. Accepted: `queue` and `priority` on
  `agent:backlog`, `priority` on `agent:icebox`; a `queue` value must be a
  recognized sync mode. An unrecognized attribute is refused because it would parse
  and then be ignored. See `specs/pending-system.md`.

## boundary

`agent-doc boundary <FILE> [COMPONENT]`

- Inserts a transient `agent:boundary` marker into the working-tree document and signals the editor so the next IPC write can use a current insertion point.
- It must not update the saved snapshot, stage files, or create a git commit. The marker is setup state, not a response closeout boundary.
- A later preflight/commit may normalize marker-only working-tree churn as already committed, but standalone boundary insertion must never become the snapshot basis for a boundary-only commit.

## terminal

`agent-doc terminal <FILE> [--session NAME]`

- Opens an external terminal that attaches to the target tmux session, but only when another attached client does not already exist.
- The terminal command comes from user config or `$TERMINAL`.

## env

`agent-doc env [--json]`

- Captures process environment and tmux availability once, then delegates to the
  pure terminal-host classifier shared with editor integrations.
- Coder detection uses `CODER=true` and the `CODER_WORKSPACE_*` identity
  variables. It never reads or reports `CODER_AGENT_TOKEN`.
- IDE integrations provide authoritative observations through
  `AGENT_DOC_JETBRAINS_PRODUCT_MODE=backend` or
  `AGENT_DOC_VSCODE_REMOTE_NAME=<vscode.env.remoteName>`; VS Code remote names
  are extension-defined and remain opaque.
- `--json` reports the input classification, resolved host, and the reason for
that resolution. A host without tmux fails closed with workspace-image
guidance.

## tmux ensure

`agent-doc tmux ensure <FILE> [--session NAME] [--ide-terminal] [--json]`

- Ensures one detached tmux session exists for the document and is safe to call
  repeatedly. An already-live registry target wins so the command never starts
  a second session for a document that is already owned.
- For a cold document, session resolution is explicit `--session`, then project
  `tmux_session`, then `0`.
- Multi-session projects (`tmux_sessions` non-empty, GH #17): the document's
  `tmux_session:` frontmatter binding is used before the project pin (resolution
  `document_binding`). A live registry target is reused only from an allowed
  session that matches the binding. An explicit `--session`, the binding, or the
  resolved name outside the allow-list fails closed.
- Human output reports the session, pane, attach command, creation state,
  resolved terminal host and reason, and whether a tmux client is already
  attached. `--json` additionally reports `terminal_host`,
  `terminal_host_reason`, `auto_start_tmux`, the session-resolution source, and
  any registered document pane. `--ide-terminal` is a typed observation from an
  IDE integration, not a host override.
- The receipt is the sole plugin policy input. Plugins focus an already-matching
  terminal, reuse or create an IDE terminal only for a detached session whose
  resolved host is `ide`, and perform no presentation for `external` or `none`.
- `agent-doc start <FILE>` uses the same operation outside tmux, provisions a
  pane when necessary, and re-executes the original lifecycle flags inside that
  pane. A host without tmux fails closed with workspace-image guidance from the
  shared terminal-host classifier.

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

- `preflight_started` with no capture is also the normal state while the harness is generating its first response. Therefore this diagnostic command does not itself claim that the run was canceled; it protects the cycle and directs callers to `agent-doc session cancel-turn <FILE>`.
- `#cancel-orphans-preflight-cycle`: only a caller that has successfully canceled/interrupted the harness run may use the immediate reclaim authority. `session cancel-turn`, successful session clear, and the editor cancel callback use that authority; generic closeout recovery and `agent-doc cancel` do not.
- `#duplicatepreflightunblock`: run cancellation is not the only reclaim authority. A controller closeout projection reporting that the owning actor **released** the cycle, **paired with** the cycle having sat untouched past `STALLED_CYCLE_RESOLVE_SECS`, carries reclaim authority too (`cancel_preflight_cycle_after_owner_release`). The route closeout drain uses it on the `RecoverAfterOwnerRelease` branch, after its bounded 30s projection await, so a duplicate `agent-doc <FILE>` invocation against an orphaned empty preflight resolves without an operator typing `session cancel-turn` at the right moment. Before this, the drain's `cancel_empty_preflight` step bound to the unproven entry point and therefore refused by construction, leaving the branch structurally dead and the document wedged across every retry. **Both halves of the new proof are load-bearing:** the projection also reports `OwnerReleased` when there was no owner to release, which is indistinguishable from a fresh cycle whose model has not answered yet, so release alone would overtake a live turn; the stall deadline, sized in `#suprecyclespin-falseabandon` to clear normal first-response latency, is what separates an orphan from a slow first response. An explicit run cancel needs no deadline — the caller stopped the run itself. The authority widens **who** may reclaim, never **what**: a cycle past `preflight_started` or owning a response capture is still protected, and callers with neither proof still get `Protected` with `reason=run_cancel_not_proven`. Authorities live once, as `agent_doc_turn::repair::EmptyPreflightCancelAuthority`; each names a distinct ops-log `proof=` token, and a release refused for want of the deadline logs `reason=owner_released_cycle_not_stalled`.
- `#runctrlclaude`: a third reclaim authority, `HarnessTurnEnded` (`cancel_preflight_cycle_after_harness_turn_end`, ops-log `proof=harness_turn_ended`), needs no stall deadline. It holds when the authoritative actor's pane has **no** fresh turn-active lease **and** a harness turn-end receipt (`SessionStart` for `/clear`, startup or resume, or a settled interrupt record; see `specs/15-turn-lifecycle.md` § Harness turn-end receipts) is **strictly** newer than the cycle's `updated_at`. The reclaim reads the same turn fence as the owner-release path and abandons only while that fence is unchanged. The route closeout drain's first `cancel_empty_preflight` step binds to it (before, that step bound to the unproven entry point and refused by construction). The idle supervisor's `#gh179` probe tries it before the owner-release + stall fallback. So an operator who interrupts a turn and then `/clear`s gets the next route dispatched at once, instead of a `replay_safe [open_empty_preflight]` block whose only unblocker was `session cancel-turn`. A refusal logs `cancel_preflight_cycle_protected ... reason=harness_turn_ended_not_proven turn_live=<bool> turn_ended_at=<secs|none> cycle_updated_at=<secs>`. It stays fail-closed: with a live lease, no receipt, a receipt from before the cycle's last re-entry, or a receipt in the same second, the cycle is protected.
- Fail-safe: even with run-cancel authority, the binary abandons the open cycle **only** when it is still `preflight_started` **and** owns no response capture. A cycle that advanced past preflight (`response_captured` / `write_applied` / `committed`) or already captured a response is left intact. Logs `cancel_preflight_cycle_abandoned` on abandon and `cancel_preflight_cycle_protected ... reason=run_cancel_not_proven` when generic recovery refuses.
- Exposed to editor plugins via the `agent_doc_cancel_preflight_cycle(file_path) -> i32` FFI export (1 = abandoned, 0 = nothing reclaimed / protected, -1 = error). The editor's "cancel run" action calls this only after canceling the run. Plan: `tasks/agent-doc/plan-cancel-orphans-preflight-cycle.md`.
