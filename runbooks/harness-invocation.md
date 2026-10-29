# Harness Invocation Patterns

## Skill auto-update

Compare `agent-doc --version` with the installed `agent-doc-version`. If the
binary is newer, update only the active harness:

- Claude Code: run `agent-doc skill install --harness claude --reload restart`
  unless `agent_doc_auto_compact` explicitly selects compact reload. Stop only
  on a real `SKILL_RELOAD=...`; if already current, continue the turn.
- Codex: run `agent-doc skill install --harness codex` without `--reload`, then
  re-read the installed `.codex/skills/agent-doc/SKILL.md` completely and
  continue the same turn. Do not restart the active supervisor.
- OpenCode: run `agent-doc skill install --harness opencode`; continue under the
  installed skill when it is already current.
- Other harnesses: run `agent-doc skill install` for the detected environment.

An "already up to date" result means the visible copy was stale instruction
drift, not a reason to stop. If the executable is absent or versions match, skip
the update.

## Grok Build

Install with `agent-doc skill install --harness grok`. The skill and bundled
runbooks live in `.grok/skills/agent-doc/`; the project MCP server is registered
in `.grok/config.toml`, preserving an existing entry. Enable/trust it in Grok
when required by the host. Select `agent: grok` (`grok-build` is an alias).

Submit `agent-doc <FILE>` as a normal prompt. With no FILE (`/agent-doc` or
`agent-doc` alone), call connected `agent_doc_admit` without `file`: it admits
the document this tmux pane claims and returns it as `file`; use that path as
FILE for the whole cycle, and stop on a refusal. Grok's passive hooks cannot supply
the Claude/Codex admission marker or block Stop. For Grok, call connected
`agent_doc_admit` before reading/planning/answering and `agent_doc_finalize`
for the terminal write/commit and queue continuation. This admission rule
supersedes hook-marker instructions below. Never recursively invoke the owner
pane or shell preflight. Continue actionable queue heads in the same turn after
committed closeout; do not run another session-check after a terminal connected
finalize.

Grok's UserPromptSubmit hook cannot inject admission context. Its PostToolUse
hook can (`#midturn-steering`, `#steeringharnessparity`): while this pane owns
the session, settled steering from the live editor replica — new queue rows,
claimed-row strikes, and claim-forwarded edits — is pushed into the turn as
`additionalContext`. That is the same real-time steering Claude Code and Codex
receive through PostToolUse and OpenCode receives by polling
`agent-doc steering <FILE>` while its turn runs. The Read tool sees disk, which
trails that replica. On that context, re-call connected `agent_doc_plan <FILE>`
in the same turn and dispatch the new `queue_subagent_dispatch` rows without
another user message. Inspect without consuming with
`agent-doc steering --peek <FILE>`. For OpenCode the poll is the in-turn
delivery and it advances; it remains the monitor path for other hookless
harnesses. The connected finalize's terminal report still carries anything the
hook did not surface. An explicit `Run Agent Doc` while this turn is running
is a separate path (`#routesyncsteer`): route submits the latest diff since
the last send into the composer as one mid-turn follow-up. With
`ui.follow_up_behavior = "steer"` that follow-up is injected into this turn
at the next safe gap; with `"queue"` it waits until the turn ends. Handle
that diff as operator steering. Do not start a second turn. When the turn
ends, the supervisor wakes the idle Grok pane for settled, unsurfaced,
unclaimed steering (`#steeringwake`) through the same guarded idle dispatch.

Routing reuses the supervisor lifecycle and document-bound resume UUID.
Readiness requires a complete empty Grok composer with no live cancel/stop
indicator; unknown screens and drafts remain protected. Fullscreen UI was
verified with Grok Build 1.0.24. `/clear` starts a fresh conversation; Ctrl+Q
twice exits. Goal dispatch and native token streaming are not advertised.

Grok Build has a native `spawn_subagent` tool. When `agent-doc plan` returns a
non-empty `queue_subagent_dispatch` list for a `subagents` / `fan-out` queue,
dispatch it immediately without waiting for another operator prompt: claim each
item, call `spawn_subagent` with `background=true` and its authorization preamble
verbatim, then pass the returned `subagent_id` to the item's `start_command`.
Use one native background worker per dispatch item and an external worktree for
repository writes. `agent-doc orchestrate` is only the fallback when the native
tool is genuinely unavailable.

The connected plan is Grok's queue-lifecycle contract. A non-empty
`queue_dispatch_acknowledgement_required` means an earlier claim never reached
the worker-bound state: use its exact `start_command` if the spawn succeeded,
or its `release_command` to requeue if no worker was spawned. These fields are
actionable even when `prompt_targets` is empty; never interpret that combination
as `no_changes` or resume the work inline (`#connectedqueuedispatch`).

This runbook covers the harness-specific differences in how agent-doc is invoked.
The core workflow (preflight, respond, persist the response) is identical across all harnesses; see `commit.md` for the shared commit-boundary contract.

## Harness-Native Entrypoints

- A harness-native `agent-doc` invocation is an executable workflow entry, not a generic request to edit the markdown file by hand.
- Claude Code `/agent-doc <FILE>`, Codex `agent-doc <FILE>`, OpenCode `/agent-doc <FILE>`, and equivalent direct-entry forms all start the same binary-owned response cycle.
- **No FILE:** `/agent-doc` (Claude Code, Grok Build, OpenCode) or `agent-doc` (Codex) with no argument runs the same cycle on the document this tmux pane claims. The Claude Code and Codex hooks resolve it in the binary, rewrite the trigger, and begin the admitted context with `[agent-doc] no FILE argument: this turn's FILE is `<FILE>``; use that path wherever the workflow says `<FILE>`. Grok Build calls connected `agent_doc_admit` without `file`, which admits the pane's claim and returns it as `file`. OpenCode runs `agent-doc document` first and uses its printed path. Outside tmux, with no claim, or with several claims on the pane, admission is refused by name: report the reason and stop. Never guess a document, and never run `agent-doc claim` on the operator's behalf to make it resolve.
- Do **not** treat that turn as successful until the response crosses `agent-doc finalize <FILE>` for the normal path or `agent-doc write --commit <FILE>` for a repair path.
- Do **not** end a normal harness-native `agent-doc` turn with "not committed" or equivalent wording unless the user explicitly asked to leave the response uncommitted.
- MCP auth / OAuth steps are part of that same turn. If a tool pauses for authentication or browser approval, resume the managed response afterward and still finish through connected `agent_doc_finalize`; its successful result is the terminal closeout report and includes queue continuation. Direct-exec paths still finish with `finalize` / `write --commit` plus `session-check`. The auth step is not the success boundary.
- If `finalize`, `write --commit`, or `session-check` reports
  `editor_convergence_required` or missing `operator_text_authority_v1`, the
  harness has not proven that delivery preserves operator text. Do not report success, stop, continue an auto-queue, or emit a final answer that implies
  closeout succeeded. Retry through the editor/CRDT path after the frontend has
  the required capability or the live editor state is otherwise proven. Do not run `--force-disk` unless the operator explicitly chooses that recovery.

## Directive Semantics

- Imperative user edits inside an `agent-doc` session document are executable directives, not just topics to comment on.
- `do #qj5w now`, `fix this`, `run tests`, `build + install`, `commit + push`, and similar document edits authorize the same underlying repo work they would authorize in chat.
- If that work includes an ordinary repo `commit + push`, the manual repo commit must exclude the active session document. Resolve the intended non-session path set first, run stage commands only for that set, stop immediately on any stage failure, verify the staged diff still matches the intended set, then commit only that validated set. `agent-doc finalize` / `write --commit` still owns the session-document closeout commit, and the push happens after that closeout so the response commit is included.
- The agent should either perform that work before `finalize` / `write --commit`, or stop on a concrete blocker. Do **not** emit status-only progress prose while doing neither.
- **Direct-chat preset write-back:** when a session-document preset (for example `#commit-push`, `#spec-test-build-install-commit-push`) triggers repo work through a direct chat turn rather than through `agent-doc finalize`, the turn is not complete until the response is written into the session document with `agent-doc write --commit <FILE>` and `agent-doc session-check <FILE>` passes. Completing side-effect repo work (commit, push) in chat without writing the response patchback is the same closeout violation as skipping `finalize`. The Codex Stop hook cannot auto-close a plain-text response that lacks `<!-- patch:exchange -->` blocks, so the agent must explicitly write back before reporting success.
- **Codex same-thread document binding:** a clean Codex closeout parks the exact
  thread-to-document binding instead of deleting it. The next ordinary prompt in
  that same thread therefore creates document write-back debt even when the
  previous document prompt is already committed; answering only in chat is not a
  valid closeout. Explicit `/clear`, `/new`, and a new agent-doc invocation sever
  or replace the parked binding so unrelated threads are not captured.

## Post-Preflight Planning

- **`#preflightinbinary`:** the Claude `UserPromptSubmit` hook (`agent-doc hook preflight-user-prompt-submit`, installed into `.claude/settings.json`) and the Codex hook (`agent-doc hook codex-user-prompt-submit`, installed into user-level `$CODEX_HOME/hooks.json`) run preflight in-process when the `agent-doc <FILE>` trigger arrives. They print the contract followed by the `[agent-doc] cycle contract ...` success marker; Codex then appends an explicit in-pane directive forbidding a recursive shell `agent-doc <FILE>` invocation, while connected Codex controller flows may instead use `agent_doc_admit`. The model must never run or poll `agent-doc preflight`; a missing marker is a fail-closed harness-admission defect. Other harness adapters must likewise obtain one binary-owned admission contract before the model turn.
- **Claude queue continuation:** Claude admission persists the exact session-to-document binding. A `/loop agent-doc <FILE>` trigger claims the `claude_loop` lease in the binary before preflight, so do not precede it with `agent-doc drain-claim`. Only a SUBMITTED prompt re-enters: a `loop` Skill call never runs `UserPromptSubmit`, so it admits no cycle (`#loopskillnoadmit`); re-enter with `ScheduleWakeup` submitting `/loop agent-doc <FILE>`. After a clean closeout proves another drainable head, the installed `agent-doc hook claude-stop` boundary keeps the turn going and directs the same session to that scheduled re-entry — unless a live, fresh supervisor with no overdue handoff will drain the queue, in which case it allows the stop and the supervisor idle-queue watch submits the next trigger (`#stopblocksupervisorowned`). With queue auto-clear on, session-check reports `queue_auto_clear_yield=true` for that ready supervisor: end the turn without scheduling `/loop`; the supervisor types `/clear` into the same Claude process before it submits the next trigger (`#bgclearpolicy`). The continuation is emitted as non-error Stop-hook feedback (stdout JSON `{"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"..."}}`, exit 0), which Claude Code (2.1.163+) shows as "Stop hook feedback"; `decision: "block"` and exit-2 stderr are both labeled a hook error, so only a genuine hook failure uses `decision: "block"` (`#stopfeedbacknoterror`). A head dispatched to a subagent is claimed (`agent-doc queue claim <FILE> --item <...> --owner subagent:<label>`); the loop does not re-enter for it, and when every remaining head is claimed or operator-gated the hook lets the turn end quietly and logs `state=waiting_on_claims`. The claim is released when the item closes, by `agent-doc queue release`, or when its TTL expires (`#queueclaim`). The supervisor idle-queue watch does not yet consult claims. It no-ops for unbound sessions, allows the final answer when the closed run captured no response (`#stopnoresponserun`: a stale-lock repair answered nothing, and a refused admission must reach the operator), and breaks its own `stop_hook_active` recursion after one block.
- **Codex Stop outcome and label:** Codex's Stop wire protocol requires `decision: "block"` to create a continuation prompt; for this event it means continue the turn, not reject it, although the host UI renders `Blocked by hook`. agent-doc models three disjoint outcomes: `AllowExit` when persistence is proven or no debt exists; `ContinueTurn(class)` when debt exists and same-turn recovery is possible; and `RefuseExit` when debt exists but bounded recovery is exhausted. `ContinueTurn(Queue)` begins `Queue continuation, not an error:`; session-writeback and recovery continuations likewise identify themselves as non-refusal/non-error before their instruction. A replayable exact-thread response is auto-closed through the binary-owned capture/write/commit path before any continuation is emitted, including an admitted or abandoned open cycle; only a closeout that cannot finish exposes the host label. Agents must still call connected `agent_doc_finalize` before emitting their terminal final answer, so this Stop path remains a safety backstop rather than the normal workflow (`#stopwritebackfirst`). Codex prints a continuation payload verbatim in the operator's pane, so it stays terse and carries only what the agent must do (`#codexstopterse`): a suppressed background context reset (session accretion, recent compaction, or ctx% over threshold) never changes or lengthens it and is recorded only as `codex_background_context_clear_suppressed` in `ops.log`. Keep draining under high accretion; the reset is not the agent's to perform. With queue auto-clear on (the default, positively declared by bare `clear`; legacy `disable-auto-clear` turns it off) and a drain-ready supervisor, the Stop hook instead allows the stop at the item boundary (`codex_stop_queue_continuation_yield`), and the supervisor restarts Codex fresh and dispatches the next head (`#bgclearpolicy`); subagent queues wait for claims to settle before the idle handoff, and a clear never reduces accretion, which only compaction does. While the Codex turn and auto queue remain active, dispatch-only plain reopens coalesce without pane input because the Stop hook exclusively owns the next continuation; typing both inputs can strand the handoff (`#codex-stop-continuation-label`).
- **Codex retained-holder queue pause (`#sdkretained`):** when the Stop hook says it kept `agent:run auto` paused, it names the exact durable holder it observed: a retained editor write (`intent_id=<id>`) or the post-commit compact projection (`continuation_id=<id>`, terminal edge `compact_projection_completed`). Wait for that holder's controller edge; do not resend, recapture, rerun finalize/write/repair, force disk, or recycle. If the hook instead names a recovery (`verdict=delivery_rejected|replica_unserved|stranded`), the holder cannot converge by waiting — follow that recovery. A pause never cites a placeholder id; a gate that observes no holder does not pause the queue.
- **Codex post-commit Stop is idempotent (`#fixlazilycodex`, `#postcommitpromptownership`):** after a committed cycle, a worker-claimed queue head is not this session's prompt debt, and a Stop whose closing message HEAD already holds is the committed closeout's own text — the hook neither reopens a cycle nor captures it as a `### Re: recovered response`, and it does not fail closed. Textual novelty is not enough to transfer ownership: when that committed cycle settled the tracked prompt but fresh queue or exchange work appears before the first Stop, a ready supervisor owns the settled edit through its reactive idle steering wake, so the hook allows Stop without opening a successor or exposing Codex's `Blocked by hook` label. The committed predecessor stays unchanged as the wake base, and the supervisor dispatches a normal fresh in-pane turn once the pane is idle. If no supervisor is provably ready, the hook retains the safe fallback: open an empty successor, hand the work back in-pane, and skip the predecessor turn's nonverbatim closing summary so only the continuation's recursive Stop may capture its actual answer. With nothing else pending the hook allows the stop silently. Do not hand-commit or `repair` to "clear" either case.
- **`#hookcontractlost`:** a `UserPromptSubmit` hook injects **stdout** as turn context, so a preflight error written only to stderr reaches the operator's log and never the agent — a refusing hook then looks identical to a hook that never ran. The hook therefore always names its outcome on stdout: the success marker above, or `[agent-doc] cycle contract UNAVAILABLE (preflight admission failed; ...)` plus `document:`, `reason:`, and `remedy:` lines. Read the three states as: contract marker ⇒ admitted; UNAVAILABLE ⇒ hook ran, preflight refused for the stated reason (report it and stop; never shell `agent-doc preflight` to recreate admission or write a response — a deadline refusal still permits a binary-owned `agent-doc backlog <FILE> requeue|done|keep-unqueued <id>` repair that `session-check` names, GH #129); neither ⇒ the hook is not wired. Separately, preflight now auto-commits *document-only* closeout drift (recovery states `Clean` / `BoundaryOnlyDrift` / `QueueMetadataDrift` / `NestedParentPointerStale`, whose own recovery command is `agent-doc commit` and which have no response body at risk) instead of bailing with a remedy only the operator could run — that bail is what made the wedge self-sustaining, since preflight owns recovery *before* diffing and only a new preflight could clear the drift.
- A prompt-bearing direct-pane reroute awaits a reactive Project Controller admission projection, not merely accepted tmux input. The controller parks the route request on its Lazily document projection and wakes when a post-dispatch cycle appears; route does not poll cycle sidecars. If a long-lived Claude or Codex process predates the installed hook settings, a missing admission projection causes one fresh supervisor restart and retry so the harness reloads its startup hook snapshot; a second miss fails closed.
- After preflight — hook-run or explicit — the next dispatch step should consume a binary-owned planning record rather than improvise from raw prose alone.
- Run `agent-doc plan <FILE>` and execute the cycle from its `prompt_targets`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers`.
- If the plan says `handoff=orchestrate`, run the emitted `agent-doc orchestrate ...` command before attempting a manual response.

## Manual Repair Default

- For **Claude Code**, **Codex**, and **OpenCode**, the default documented manual-repair path is `agent-doc write --commit <FILE>` once the user prompt is already present in the document.
- Do **not** document or follow a manual-repair flow that stops after bare `agent-doc write`; that leaves the response on the wrong side of the commit boundary.
- If the user prompt itself is missing, insert that prompt into `exchange` first, then return to `agent-doc write --commit <FILE>` for the assistant response.

## Response Header Attribution

- Always attribute `### Re:` headings with the resolved model short name **and** the response timestamp, for example `### Re: topic — opus-5 · 2026-09-11T23:45-04:00` (`#timestampresponseheader`).
- Never use the harness label as the model name. `### Re: topic — codex` and `### Re: topic — claude` are wrong.
- The timestamp is local time with its UTC offset, `YYYY-MM-DDTHH:MM±HH:MM`, taken when you begin composing. With no resolvable clock, emit the model alone (`### Re: topic — opus-5`) rather than a dangling separator.
- Keep **exactly one** spaced em dash in the heading: the model and timestamp are joined to each other by ` · `. Three heading parsers split on the FIRST em dash and `strip_re_heading_attribution` splits on the LAST, so a second em dash leaves the model name behind and reads as permanent post-commit heading drift. Format owned by `agent_doc_turn::response_text::response_heading`.

## Harness Detection

Identify your harness from your environment:

| Signal | Harness |
|--------|---------|
| You have a `Skill` tool and slash commands (`/agent-doc`) | **Claude Code** |
| You are Codex CLI / `CODEX_CLI` env var is set | **Codex** |
| You are OpenCode / `OPENCODE` env var is set | **OpenCode** |
| You are Cursor / `CURSOR_SESSION_ID` env var is set | **Cursor** |
| None of the above | **Generic** |

### Another document's harness is not yours (`#otherdocownerfirst`)

The table above identifies **your** harness. It says nothing about a *different*
document, and an operator can switch one from `agent: claude` to `agent: codex`
at any time — agent-doc emits `harness_change_detected old=… new=…` followed by
`agent_restart_performed … action=spawn_fresh_harness` when they do.

So when the operator reports a problem about a document other than this
session's — a pasted pane excerpt, an error from another tab, a screenshot of a
lock screen — resolve that document first:

```bash
agent-doc session status <OTHER_FILE>
```

It reports the `harness:` actually running it, the owning `actor:` / `registry:`
pane, live pane state and tail, the supervisor, and any `startup_miss`. A pasted
excerpt carries none of that, and the harness you happen to be running in is not
evidence about the other document.

Two failure shapes this prevents, both observed 2026-09-28:

- Reading the wrong harness's state. A Codex lock screen (`This conversation is
  open in another app`, `r retry  f fork  esc/ctrl+c/q exit  ctrl+t transcript`)
  was diagnosed against Claude Code's session registry. Codex's equivalent is one
  advisory lock per conversation under `$CODEX_HOME/thread-writer-locks/`; route
  now names its holders, and `session status` would have said `harness: codex`
  before any of that was needed.
- Quoting a relative path back. A trigger's relative path resolves against the
  *invoking pane's* working directory. `agent-doc tasks/infra.md` names the real
  document from a submodule pane and a nonexistent file from the superproject
  pane one window over. Quote another pane's document absolutely, always.

## Claude Code

- **Invocation:** User types `/agent-doc <file>` which triggers the `Skill` tool.
- **Slash commands:** Execute via the `Skill` tool. Strip the leading `/`; pass remaining args.
- **Auto-update prompt:** Default to `--reload restart`; only use `--reload compact` and ask the user to run `/compact` when `agent_doc_auto_compact` is explicitly set in frontmatter or `.agent-doc/config.toml`.
- **Write-back:** Pipe the normal response cycle via `Bash` using `agent-doc finalize ...`; use `agent-doc write --commit ...` only for manual repair when the prompt already exists.
- **Manual repair / missed patchback:** Use the shared default above. In Claude Code that still means piping the response through `Bash`, but the command should be `agent-doc write --commit <FILE>` once the prompt already exists in the document.
- **Mid-turn steering hook (`#midturn-steering`):** `agent-doc skill install` adds a `PostToolUse` entry running `agent-doc hook steering-post-tool-use` (timeout 10s) to `.claude/settings.json`. After each tool call it resolves this session's tracked document, compares it with the cycle watermark preflight seeded, and injects settled new operator steering as `hookSpecificOutput.additionalContext`; after closeout it keeps reporting new edits in boundary (next-cycle) terms while the same turn runs (`#steeringafterclose`); it is silent with nothing new and never fails the tool call. When the turn ends, the route-owned supervisor wakes the idle pane for settled, unsurfaced, unclaimed steering (`#steeringwake`) by submitting `/agent-doc <FILE>` through the guarded idle dispatch. Act on it per `runbooks/respond.md`.
- **Final status synchronization:** after queue and response-persistence obligations settle, the `Stop` hook re-surfaces a persistent `agent:status sync="..."` instruction once against the live editor-authoritative document. If the result changed, use `agent-doc status set <FILE> --text "<current result>"` and then `agent-doc commit <FILE>`; pending `queue report-complete` cells do not block that commit, which lands them with the status change. If unchanged, write nothing. The recursive `stop_hook_active` boundary passes through without asking again.
- **Tool guard hook (`#tmuxguardclaudeinstall`, `#tmuxguardclaudeuser`):** `agent-doc skill install` also merges an unmatched `PreToolUse` entry running `agent-doc hook coined-id-pre-tool-use` into `.claude/settings.json`, keeping the operator's own entries. A bare-root install (no `--root`) also merges that entry, and only that entry, into user-level `~/.claude/settings.json`, so every Claude project and subagent is covered; Claude Code runs the two identical copies once. It refuses a Bash command that would kill the operator's live tmux server (`#tmuxkittyterminal`) in any pane, and a coined `#id` written into source or a commit message in a governed one. Grok Build and Cursor also run the hooks in `~/.claude/settings.json`. `agent-doc turn-status install --user` writes only the turn-status hooks into the user layer.
- **Built-in commands** (e.g., `/compact`, `/clear`): Cannot invoke via Skill. Write a document note instructing the user to run it at the terminal.

## Codex

- **Invocation:** Direct prompt injection or user instruction referencing the document. Do **not** type `/agent-doc`; Codex CLI reserves leading `/` for its own built-in slash commands and will reject project-defined `/agent-doc`.
- **Use this form instead:** `agent-doc <FILE>` as a normal message, for example `agent-doc tasks/agent-doc/agent-doc-bugs.md`.
- **Entry semantics:** once that message is accepted, the turn is inside the binary-owned response cycle. Do **not** downgrade it into a manual document-editing task or report a useful-but-uncommitted success state.
- **Direct prompt body:** extra chat text after that invocation still counts. For example, `agent-doc tasks/agent-doc/agent-doc-bugs.md #agent-doc-bug` or a following-line `do #id ...` prompt is now captured by the Codex hook and reused by `preflight` / `plan` even when the document itself has no new diff. Bare `agent-doc <FILE>` with no trailing body remains a no-op until the document changes.
- **Slash commands:** Codex has no slash commands. If preflight returns `slash_commands`, skip them. If `builtin_commands`, write a document note.
- **Auto-update prompt (`#codex-skill-reload-in-place`):** run `agent-doc skill install --harness codex` without `--reload`. After a real update, re-read the installed `.codex/skills/agent-doc/SKILL.md` completely and continue the same turn. Do not restart the supervisor or stop: replacing the active Codex child interrupts the conversation, while the explicit re-read loads the new workflow in place. If install says the skill is already up to date, continue normally under the installed instructions.
- **Installed hook backstop (`#codex-user-hook-owner`):** `agent-doc skill install` writes the consequential Codex `UserPromptSubmit` / `Stop` commands once into user-level `$CODEX_HOME/hooks.json`, while `.codex/config.toml` remains project-local for `features.hooks = true` and `[mcp_servers.agent-doc] default_tools_approval_mode = "approve"`. Codex loads user and project hook layers together, so reinstall removes only agent-doc's managed lifecycle commands from an existing project `.codex/hooks.json` and preserves unrelated hooks; this prevents duplicate preflight where project hooks work and avoids nested-submodule/worktree misses where they do not. The user-level commands are `agent-doc hook codex-user-prompt-submit` for `UserPromptSubmit` and `agent-doc hook codex-stop` for `Stop`. In one binary-owned invocation, `UserPromptSubmit` first makes active-document tracking durable across nested `.agent-doc` roots in the same workspace, then runs preflight for a valid `agent-doc <FILE>` trigger and writes the cycle contract, its trailing success marker, and the in-pane no-shell-reinvocation directive to stdout for Codex to inject into the turn. Ordinary prompts remain context-free; tracking or preflight failures emit diagnostics but no success marker, so admission fails closed. `Stop` first tries to finish the response cycle deterministically from `last_assistant_message`, but only when that payload validates as a single assistant closeout. Transcript-shaped payloads (for example full `agent:exchange` dumps, prompt-target lines, or repeated response headings) are blocked and saved only for diagnostics instead of being replayed, even when the stop arrives on a later turn in that same Codex session. If `last_assistant_message` is empty because the turn stopped after a tool-only/authentication step, the hook still blocks, records a diagnostic artifact, and sends the turn back through the normal `finalize` / `session-check` recovery path. After a clean closeout with `agent:run auto` still active, the hook blocks final answer delivery and instructs Codex to continue the next queue head **in-pane** — answer it in the current turn and persist with `agent-doc finalize <FILE>` (or `agent-doc write --commit <FILE>`) — rather than re-running `agent-doc <FILE>` from the owner pane, which would re-enter the pane it runs in and trip the recursive-direct-invocation deadlock guard (`#codex-self-reinvoke-prevent`); if that hook continuation repeats without the queue head advancing, it fails closed instead of looping. This continuation no longer depends solely on tracked in-memory session state: every clean binary closeout writes a durable `.agent-doc/queue-continuations/<doc-hash>.json` marker, so a `Stop` that arrives with missing/stale session state still consults the marker (re-confirmed against the live document) and blocks. Codex direct-exec closeout paths can additionally enforce this with `agent-doc session-check <FILE> --codex-final-gate`, which exits nonzero whenever a clean document still owes an `agent:run auto` continuation. (`#codex-auto-queue-stalled-final-gate`)
- **Mid-turn steering hook (`#midturn-steering`):** the user-level `$CODEX_HOME/hooks.json` also carries a `PostToolUse` entry running `agent-doc hook steering-post-tool-use`. Codex `PostToolUse` fires after Bash, `apply_patch`, MCP, and other local tools and accepts the same `hookSpecificOutput.additionalContext` envelope, so Codex receives the same settled, typed operator steering as Claude Code, including after closeout, and its idle pane is woken by the supervisor the same way (`#steeringwake`).
- **Tool guard hook (`#tmuxguardclaudeinstall`):** the user-level `$CODEX_HOME/hooks.json` also carries an unmatched `PreToolUse` entry running `agent-doc hook coined-id-pre-tool-use`, the same live-tmux-server and coined-id guard the Claude install wires. Because it lives in the user layer, it covers every Codex project, governed by a session document or not.
- **Final status synchronization:** after queue and response-persistence obligations settle, the `Stop` hook re-surfaces a persistent `agent:status sync="..."` instruction once against the live editor-authoritative document. If the result changed, use `agent-doc status set <FILE> --text "<current result>"` and then `agent-doc commit <FILE>`; pending `queue report-complete` cells do not block that commit, which lands them with the status change. If unchanged, write nothing. The recursive `stop_hook_active` boundary passes through without asking again.
- **Bounded hook ownership:** Codex lifecycle entries carry explicit harness timeouts. The Stop bridge has a shorter in-binary budget and performs only one opportunistic captured-closeout resume; on budget expiry it emits a valid fail-closed response before the harness can discard output, while the route-owned supervisor continues keyed recovery.
- **Long-turn liveness:** the `UserPromptSubmit` turn-active timestamp is a base lease, not a one-hour maximum turn duration. After that base TTL, recent timestamped activity in the exact bound rollout keeps the turn live, so the supervisor must not abandon its open preflight or restart its harness while tools are still running. If an ops log reports an expired-lease reclaim while the bound rollout contains newer ordinary turn activity inside the TTL, treat that as a lifecycle-liveness defect; preserve the transcript and turn-status rows for diagnosis rather than forcing another restart.
- **Owner-pane self-invocation (`#codex-owned-pane-prompt-miss`):** when `preflight`/`plan` returns a non-null `owned_pane_self_invocation`, you are already running inside the Codex pane that owns this document — do **not** launch another `agent-doc <FILE>` child from this pane (that recursive dispatch is refused as a deadlock and abandons the cycle). Instead, treat it as an in-pane response cycle and act on the structured fields: read/plan the document, answer the `work_excerpt` (`kind: "unresolved_prompt"` is an unresolved `agent:exchange` prompt that still needs a reply; `kind: "active_queue_head"` is the next `agent:run` head named by `head_id`) in the current assistant turn, then persist with the exact `persistence_command` (`agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`) followed by `agent-doc session-check <FILE>`. An unresolved exchange prompt stays executable user intent until a response body crosses `finalize` / `write --commit`; do not let an active auto-queue head run ahead of it, and do not send final chat output as if the prompt were handled while the document still owes a response.
- **Auto-queue heading discipline (`#codex-queue-head-exact-heading-guidance`):** when a Codex auto-loop turn completes the active `agent:run auto` head, the Stop-hook auto-consume fires only on an explicit completion signal. Either (a) pass `--done <id>` (or `--backlog-gate <id>` / `--backlog-edit "<id>=…"`) on `agent-doc finalize` naming the head id, or (b) title the response heading **exactly** with the queue head — `### Re: do [#id]` for a `do [#id]` head, or `### Re: #id` for a preset/synthetic head (a prompt carrying a trailing `#preset` id). A descriptive heading (`### Re: <free-form topic>`) does **not** auto-consume the head, so the auto-queue will not advance and the hook fails closed on the next non-advancing repeat. Headings whose topic carries trailing modifiers (`### Re: #id halt` / `### Re: #id deferred`) never consume — that is the intended halt/refusal behavior (`#queue-strike-on-halt`).
- **Ordering:** finish the turn's requested coding / testing / build-install work before the response persistence command. Do not patch the document early and then keep working for the same turn.
- **Commit/push ordering:** when the document directive includes ordinary repo `commit + push`, do not stage the active session document into that manual git commit. Resolve the intended non-session path set, stage only that set, stop on any stage failure, verify `git diff --cached --name-only` (or stricter submodule-pointer proof) still matches the intended set, commit only that validated non-session set, run `agent-doc finalize` / `write --commit`, then push after the closeout commit lands.
- **Write-back:** Execute `agent-doc finalize` directly for the normal response cycle (Codex runs shell commands natively), then immediately run `agent-doc session-check <FILE>`.
- **Fail closed:** If `agent-doc session-check <FILE>` exits nonzero after write-back, the cycle is still open or the closeout failed an integrity guard. Do **not** report success or stop; continue recovery instead. Operator steering that arrived after the committed response is **not** a failure (`#steerinterruptexit`): `session-check` exits `0` with `[session-check] steering pending: …` plus each item verbatim with its `dispatch` — handle the items per `runbooks/respond.md` (the next `agent-doc <FILE>` cycle answers `address_now` prompts) instead of recovering anything. Under `--codex-final-gate` the same state exits `2`, the gate's "work is still owed" code.
- **Manual repair / missed patchback:** Use the shared default above. Do **not** patch the assistant response directly into the file. After `agent-doc write --commit <FILE>`, run the same `agent-doc session-check <FILE>` guard before ending the turn. That repair write-back should also be the last substantial action of the turn.
- **Session resume:** Codex uses `codex resume --last` instead of `--continue` for ordinary continue flows.
- **Tracked `/clear` recovery:** if the latest tracked Codex prompt for a document was `/clear`, the next managed `agent-doc route` rerun (not `--dispatch-only`) must restart the live session fresh before injecting `agent-doc <FILE>` so the original sandbox, writable roots, and network policy are reapplied instead of trusting post-clear resume inheritance. Editor `Run Agent Doc` actions still use `agent-doc route --dispatch-only --plain-trigger`, which must send the plain `agent-doc <FILE>` reopen into the live session instead of turning a post-clear reroute into a restart shortcut.

## OpenCode

- **Invocation:** User types `/agent-doc <FILE>` which triggers the installed OpenCode command. The command template emits `agent-doc <FILE>` as the prompt, which the agent recognizes and loads the skill. A bare `/agent-doc` emits `agent-doc` with no FILE: run `agent-doc document` and use its printed path as FILE, or report its refusal and stop.
- **Entry semantics:** once that message is accepted, the turn is inside the binary-owned response cycle. Do **not** downgrade it into a manual document-editing task or report a useful-but-uncommitted success state.
- **Tool guard plugin (`#tmuxguardclaudeuser`):** a bare-root `agent-doc skill install --harness opencode` writes `~/.config/opencode/plugins/agent-doc-tool-guard.js`. Its `tool.execute.before` hook pipes each `bash` call to `agent-doc hook coined-id-pre-tool-use` and throws on exit 2, which refuses a command that would kill the operator's live tmux server. Every other outcome lets the call run.
- **Slash commands:** The `/agent-doc` command is an OpenCode custom command installed by `agent-doc skill install --harness opencode` into `.opencode/commands/agent-doc.md`. If preflight returns `slash_commands`, skip them (OpenCode handles them natively). If `builtin_commands`, write a document note.
- **Write-back:** Execute `agent-doc finalize` directly for the normal response cycle, then immediately run `agent-doc session-check <FILE>`. Do **not** output the response to the CLI without piping it through `agent-doc finalize` — response text visible in the console but absent from the session document is the same closeout violation as skipping finalize entirely.
- **Mid-turn steering (`#midturn-steering`):** OpenCode has no post-tool hook, so steering is not pushed into the turn. Poll `agent-doc steering <FILE>` (add `--json` for the typed `dispatch` form) between long-running steps and act on its items per `runbooks/respond.md`; each item is reported once per consumer. Inspect without consuming with `agent-doc steering --peek <FILE>`. `agent-doc steering --follow <FILE>` streams settled steering as JSON lines for monitors. When the turn ends, the route-owned supervisor wakes the idle OpenCode pane for unsurfaced steering (`#steeringwake`) through the same guarded idle dispatch, using OpenCode's composer-readiness detection.
- **Fail closed:** If `agent-doc session-check <FILE>` exits nonzero after write-back, the cycle is still open or the closeout failed an integrity guard. Do **not** report success or stop; continue recovery instead. Operator steering that arrived after the committed response is **not** a failure (`#steerinterruptexit`): `session-check` exits `0` with `[session-check] steering pending: …` plus each item verbatim with its `dispatch` — handle the items per `runbooks/respond.md` (the next `agent-doc <FILE>` cycle answers `address_now` prompts) instead of recovering anything. Under `--codex-final-gate` the same state exits `2`, the gate's "work is still owed" code.
- **Manual repair / missed patchback:** Use the shared default above. Do **not** patch the assistant response directly into the file; use `agent-doc write --commit <FILE>` once the prompt already exists. After `agent-doc write --commit <FILE>`, run the same `agent-doc session-check <FILE>` guard before ending the turn. That repair write-back should also be the last substantial action of the turn.

## Cursor / Generic

- **Invocation:** Follow the same pattern as Codex (direct execution).
- **Mid-turn steering (`#midturn-steering`):** Cursor has no post-tool hook and no route-owned pane, so steering is neither pushed into the turn nor able to wake an idle session (`#steeringharnessparity` gap: the supervisor has no composer to submit into). Poll `agent-doc steering <FILE>` between long-running steps; inspect without consuming with `agent-doc steering --peek <FILE>`. Unsurfaced steering also arrives at the next agent-doc command's terminal report.
- **Tool guard hook (`#tmuxguardclaudeuser`):** a bare-root `agent-doc skill install --harness cursor` adds a `beforeShellExecution` entry running `agent-doc hook coined-id-pre-tool-use` to `~/.cursor/hooks.json`. Cursor also imports the guard from `~/.claude/settings.json` by default. A refusal exits 2 with Cursor's `permission: deny` reply.
- **Slash commands:** Not available. Skip `slash_commands`; note `builtin_commands`.
- **Write-back:** Execute `agent-doc finalize` directly for the normal response cycle.
