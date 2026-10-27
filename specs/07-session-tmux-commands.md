> Extracted from [07-commands.md](07-commands.md)

# Session And Tmux Commands

This file covers the session-bound command surface: pane ownership, routing, sync/reconcile, stash handling, and tmux session selection.

Codex resume recovery accepts harness-thread identity from hook observations,
including parked bindings. Externally recorded document control prompts carry
document-session identity and remain prompt/cooldown evidence only; they cannot
override a Codex resume ID. Legacy external clear rows without a turn ID are
excluded from thread recovery. Explicit hook provenance permits genuine hooks
whose payload omits a turn ID.

The hook ledger is shared by Claude Code and Codex, so every binding records the
harness whose `UserPromptSubmit` hook wrote it (`harness: claude|codex`). Codex
resume recovery ignores rows tagged with another harness, and rejects any
candidate id — from an untagged legacy row or from frontmatter `resume.codex` —
that owns a Claude transcript (`~/.claude/projects/<cwd>/<id>.jsonl`), logging
`start_resume_foreign_harness` / `start_codex_session_projection_rejected` and
launching a fresh Codex session instead. Switching a document's `agent:` from
claude to codex therefore never runs `codex resume <claude-session-id>`
.

Supervisor compilation does not imply an unsafe injection boundary. Recycle
`InFlight` is published at the actual replacement boundary and settled by the
fresh watch loop. A reopen awaiting that transition uses the complete controller
settlement budget plus transport margin; it never injects across an unsettled
boundary or inherits the shorter ordinary-RPC deadline.

Layout sync treats a stale route-owned supervisor as a process-identity fact,
not an age heuristic. Only a live supervisor PID whose `/proc/<pid>/exe` is
unlinked is positively stale. During a harness-owned active turn, sync leaves
the pane and child intact and refreshes the durable safe-boundary recycle
request. At the first observed idle boundary, sync asks the project controller
once for a forced continuation replacement of that exact stale PID. The
controller owns replacement; the layout gate never kills or reaps directly,
and no elapsed-time threshold authorizes the request. A failed controller
submission releases the claim so a later sync may retry.

Every derived layout publisher is width-bounded before publication. Given
`retained` columns, `observed` live panes, `gated` explicitly admitted columns,
and `asserted` columns, the derived bound is
`max(min(retained, observed + gated), asserted, 1)`; without an observation it
is `max(retained, asserted, 1)`. Only a positive editor split observation may
widen beyond that derived bound. Stale generations are ignored, stale panes
outside the target window cannot add width, and reconnect, duplicate, delayed,
or reordered state-plane delivery does not relax the bound. Recycle settlement
may republish the retained intent, but remains subject to the same rule.

## start

`agent-doc start <FILE> [--force] [--client-id ID]`

- Server identity observation represents an absent server or empty bootstrap
  reply explicitly and leaves the prior identity/registry untouched. The first
  valid identity after creation performs the existing initialize/keep/replace
  transition, including clearing rows from a replaced server. Nonempty malformed
  replies, launch failures, and failed queries against a live server remain errors.
  The tmux query uses a printable space between `#{pid}` and `#{start_time}`;
  parsing accepts arbitrary ASCII whitespace so older tab-delimited durable values
  remain readable even on tmux builds that rewrite control separators.
- `AGENT_DOC_TMUX_SOCKET` scopes the shared configured tmux handle for the whole
  invocation, including claim, route, and supervisor descendants. A selected
  socket must never fall back to the operator's default server during provisioning.
- Starts the configured harness in the current tmux pane and registers the pane as the session owner.
- Start reads and binds the **document's** store, never the process cwd's (`#startcwdfallback`):
  the nearest `.agent-doc` project of the canonical document, or for a document with no
  `.agent-doc` ancestor its canonical parent directory
  (`agent_doc_project_root_io::project_root_or_file_parent`), the store the route, claim
  (`#claimwrongstore`) and sync's recovered-owner bind (`#syncnorootfallback`) use. Every start
  path resolves it once (`start_project_root`): the outside-tmux bootstrap (session ensure,
  start-pane provisioning) and the in-pane admission (`prepare_start_runtime_with_admission`).
  Admission used to fall back to `current_dir()` for a rootless document, so a start run from
  inside another project bound the document there. Every consumer of the admission store uses
  it: route-owned queue control, session identity, stale-actor cleanup, the stderr redirect, the
  project controller, the session policy, navigation-lane config, the `tmux_session` pin the
  pane is relocated toward (`project_tmux_session_for_root`), the dead-pin rebind
  (`update_project_tmux_session_for_root`), the existing-pane and prior registry entry reads
  (`lookup_entry_in`), the actor bind, and the registry publication. The terminal-host policy
  reads the same store's config. A start of a rootless document from another project's cwd
  therefore writes no store, actor, registry entry, controller, or config pin into the cwd's
  project, and the cwd's `tmux_session` pin never becomes its relocation target. The registry
  entry's `cwd` stays the supervisor's process cwd: that is where the harness child runs.
  `tests/start_rootless_store_tmux.rs` (`make tmux-ci`) runs the real in-pane start this way
  against a fake harness.
- Route-owned tmux autostart panes must not automatically restart the child harness after a clean exit. If the newly started child exits before or after surfacing a prompt, `start --route-owned` must surface the local restart/quit prompt instead of immediately spawning another child process.
- Route-owned reap decisions after a committed cycle may use the supervisor actor's stable `ready` state as prompt proof even when the terminal tail still contains transient renderer text. Explicit blocking prompt states, including queued drafts, permission prompts, hook-review prompts, history search, and clean-exit restart prompts, still preserve the pane.
- If another alive pane is already bound to the same document session, normal `start` must fail closed instead of reusing, restarting, or replacing it.
- That failure must print concrete tmux inspection/capture/kill commands so the user can decide which pane to keep and which pane to kill manually.
- `--force` is the only supported escape hatch for intentionally rebinding the current pane during repair work, and the registry rebind must still record supersession provenance in the session log.
- `--force` retires the live owner it bypasses (GH #243, GH #354), but controller admission is the authority edge: the replacement generation must be accepted before any destructive retirement of the prior owner. Before admission, the pane the non-force path would have refused is preflighted and retirement is permitted only when its process tree proves it is agent-doc's supervisor for this document **and** tmux/turn evidence proves that destructive retirement is safe: no active turn, no protected prompt input, and no attached client displaying the pane. A protected duplicate is reported with its pane, window, requested owner pane, and protection reason; the force-start is deferred without starting a second supervisor, and the operator retries after the protected pane becomes safe. Controller rejection or panic leaves the prior supervisor and pane untouched. After successful admission, retirement revalidates the same facts; a newly protected owner receives a durable pid/pane-targeted idle-retirement request while the accepted controller authority fences its writes. The same guard applies when a route finds non-authoritative duplicate supervisors: routing to the authoritative owner continues, while a protected duplicate receives that durable retirement request. Its own supervisor consumes the request through the existing idle watcher only after revalidating the authoritative owner and reaching an idle, input-free, unwatched boundary; it records an intentional exit, tears down its child, reports the closeout with duplicate pane/window/owner, and exits without relaunch. Immediate safe retirement SIGTERMs the supervisor (its handler records an intentional exit, so the controller watchdog does not respawn it), escalates to SIGKILL after a 5s grace, stops any descendant that outlived it, closes the pane, and prints/logs the closeout. A transient `agent-doc write|compact|session … <doc>`, bare harness session (`#bare-foreign-session-guard`), stale registry row, or idle shell is never ownership proof and is left alone.
- When a fresh start falls back to a new session binding because the configured session is dead, the document store's `.agent-doc/config.toml` must be updated to the new live session (never the process cwd's, `#startcwdfallback`).
- Harness launches must auto-add writable roots for parent-repo patchback and nested submodule git metadata when needed.
- Start pane launch: an outside-tmux `start` runs its re-exec'd
  `agent-doc start` as the provisioned pane's own process and never types it into the
  pane's freshly spawned shell. Nothing proves when an interactive shell starts reading
  its command line, and an rc step that reads the terminal first swallows typed bytes:
  zsh with no `~/.zshrc` runs `zsh-newuser-install`, which ate the leading `/` of the
  absolute agent-doc path (onboarding pass, 2026-10-26). No readiness wait fixes that,
  because that prompt itself waits for input. The launch (`agent_doc_tmux_io::launch_command_in_pane`)
  is one `respawn-pane -k` on the provisioned pane, keeping its pane id and its working
  directory, whose argv is `/bin/sh -c` with a POSIX script (`agent_doc_tmux_commands::pane_launch_script`),
  so the operator's `default-shell` never parses it. The script turns on job control
  (`set -m`) so tmux names the start, not the wrapper, as `#{pane_current_command}`;
  traps SIGINT so the wrapper survives a Ctrl-C the start receives (the start itself keeps
  the default disposition); runs the start; then `exec`s `$SHELL`, so the pane keeps the
  operator's shell after the start exits exactly as a typed start left it. No environment
  variable is passed in the tmux argv: a tmux client outside any session already hands its
  own `PATH` to the panes it spawns. `make tmux-ci` covers it against a stand-in shell that
  reads one terminal byte before its first command line (`pane_launch_live_*`, plus real
  zsh with an empty `HOME` when installed), and `start_bootstrap_runs_the_start_whole_under_a_terminal_reading_shell`
  drives `start`'s own provisioning and dispatch against it.
- Every other freshly provisioned pane launches its command the same way, never by
  typing it into the pane's new shell:
  - Route-owned auto-start (`agent-doc-route-io` `route.auto_start`) runs
    `agent-doc start --route-owned ...` with `agent_doc_tmux_io::launch_command_in_pane_at`
    in the directory the document path was rewritten against, handing the pane to the
    operator's shell when the start exits (`PaneLaunchExit::HandOffToShell`). Its input
    diagnostic is one `tmux_input_event source=route.auto_start destination=pane:<id>
    transform=route_owned_start_launch key=launch bytes=<n> detail=sha256=<hash>` event
    in the document's ops log; the typed path's `transform=route_owned_start_enter`
    text + `key=Enter` pair no longer exists, because nothing is typed.
    `route_owned_start_runs_as_the_pane_process_under_a_terminal_reading_shell` covers it.
  - `parallel` fan-out panes run their task (`<env exports>claude -p --output-format json
    --model ... < .agent-doc-prompt.txt > .agent-doc-result.json 2>.agent-doc-result.log`)
    as the pane's own process in the task directory, ending the pane with the task
    (`PaneLaunchExit::EndWithCommand`, the completion signal `parallel` polls; the typed
    form appended `; exit`). The frontmatter `env` prefix stays unexpanded
    (`export KEY="$(passage show ...)" && `): the pane's `/bin/sh` expands it, so a secret
    value reaches only that shell's and `claude`'s environment, never the tmux argv, the
    process list, the ops log (`source=parallel.fanout transform=parallel_task_launch
    key=launch`, length + hash only), or scrollback.
    `fanout_task_runs_as_the_pane_process_under_a_terminal_reading_shell` covers it.
  - A process environment note: a launched command gets the tmux session environment
    plus the launching client's `PATH`, not the operator's interactive rc. The typed
    form ran inside the pane's interactive shell and saw whatever its rc exported.
  - The controller's supervisor-replacement cold start into a *preserved* pane
    (`controller.supervisor_replacement.cold_start_preserve_pane`) still types its
    start: that pane is an existing idle shell the operator kept, not a fresh one, and
    respawning it would discard the operator's shell.
- Navigation lanes (`#ideriderindependentnav` P6): `--client-id` names the requesting editor
  surface client. From outside tmux, with `editor_navigation = "per_installation"`, a
  `tmux_session` pin, and an installation-scoped client, the start pane is created in that
  client's own lane session (provisioned on first use; a new lane's idle placeholder pane becomes
  the start pane and loses its `@agent_doc_navigation_lane_placeholder` mark, so the lane gc
  judges it by its foreground command), logged `start_navigation_lane_target`. A document that already has a live
  pane anywhere is reused where it lives, never started again in the lane. Inside tmux the start
  binds the current pane and the flag changes nothing. Shared mode and unscoped clients keep the
  shared session. In `per_installation` mode a start whose pane lives in one of the project's own
  navigation lane sessions (marked `@agent_doc_navigation_lane` = root) is never auto-relocated
  into the project session (`start_navigation_lane_pane_kept`): before this, every lane-started
  pane (route, claim, or a lane layout's provisioning) was moved into the shared session when its
  `agent-doc start` ran. In `shared` mode a lane session is stale, so the pre-lane relocation
  still applies, alongside the shared sync's adoption of stale lane panes.
- Start relocation preserves the pane's visibility class (`agent_doc_sync::start_relocation_placement`).
  A pane that `start` (including a supervisor re-exec onto a fresh binary) finds in a stash window
  of the wrong session moves into the expected session's `stash` window; it never joins beside the
  expected session's active pane, because whether a document is a visible column is the layout
  plane's decision, derived from the editor's visible splits. A visible pane started in the wrong
  session still joins beside the expected session's active pane. Every relocation is recorded in
  ops.log as `start_pane_session_relocated` (or `start_pane_session_relocation_failed`) with
  `from_session`, `to_session`, and `placement`. Before this, an auto-install re-exec of a
  document parked in another session's stash added a third pane to a two-split layout, and a
  converged projection treats a later extra pane as operator-owned drift, so nothing removed it
  until the next structural sync (reported 2026-10-20 as "3 tmux panes").

## route

`agent-doc route <FILE> [--pane P] [--debounce MS] [--client-id ID]`

- Routes a harness-native reopen command into the authoritative pane for the document.
- Unless explicitly disabled with `--debounce 0`, route first waits for both filesystem mtime and the shared editor typing indicator to be idle for the debounce window (default 500ms). If the combined proof does not settle within the bounded wait, route fails before inserting frontmatter, cleaning duplicate answered prompt tails, or submitting pane input.
- Route must ask the project controller for the document's authoritative actor binding and fail closed when that binding cannot be proven; no compatibility file participates in owner resolution.
- Before route submits a managed or dispatch-only reopen to an actor-owned pane, it must record a controller `dispatch` attempt for the current session id, pane id, generation, and command kind. Stale session, pane, or generation requests fail closed before input is submitted.
- Existing managed reroutes must use supervisor IPC for the reopen path; they must not fall back to typing directly into a live Claude/Codex pane. If the document path disappeared from the child argv but a healthy supervisor PID still maps to the registered pane, that supervisor-owned binding is enough to keep the managed reroute on supervisor IPC even when a direct pane prompt probe cannot classify the visible prompt.
- Actor-backed reroutes may refresh the transactional registry binding for the actor pane, but they must not opportunistically steal another same-file pane or re-register to a heuristic winner while the authoritative actor is healthy.
- Session-log owners, `registry_rebind` successors, and generic same-file process-tree matches are repair/diagnostic signals only; route must fail closed with explicit inspect/claim/kill guidance instead of promoting them back to authority on the normal path.
- Route must fail closed on ambiguity and list concrete follow-up commands instead of guessing.
- Routed dispatch must target an actually idle composer. Drafted user input, queue-only Codex composer states, reverse-i-search, permission prompts, or similar blockers are not safe.
- Codex reroutes always send the bare `agent-doc <FILE>` reopen. Multiline payloads or content-edited payloads are invalid.
- If unresolved prompt-bearing drift exists and the pane is busy, route may attempt one scoped `agent-doc fix <FILE>` pass and then one bounded fresh-restart recovery, but it must still fail closed if no clean dispatch path emerges.
- For live same-document panes with no new prompt-bearing drift, route may focus the pane and return success without sending a duplicate reopen.
- Editor `Run Agent Doc` submits the controller-owned route directly. It must not create, attach, select, focus, or reveal an IDE terminal tab as a precondition; the route/controller owns cold start and uses the live actor/session authority described below.
- The route's layout wait is owned by the pane-layout graph. If a newer passive
  editor generation still contains the routed document, route follows that
  generation until convergence instead of republishing another layout intent.
  While that route is in flight, a passive generation that omits the document
  is retained but cannot supersede the route; the newest retained generation is
  projected after the routed dispatch attempt settles. A newer explicit route,
  operator ownership, or a terminal non-converged state still fails closed
  before dispatch (`#routelayoutlease`). When a newer foreground route or
  command supersedes an in-flight route, the refusal is terminal for that
  generation: it fails immediately rather than retrying until the shared budget
  expires, and names the winning publisher, generation, and observed columns so
  the operator can distinguish intentional supersession from a stuck projection
  (`#gh143`).
- Fresh auto-starts and live reroutes both require the Project Controller's reactive per-document admission projection after dispatch; accepted input alone is not sufficient.
- Automatic editor-layout `Sync` generations are structural edges and must cross the tmux effect boundary even when the desired columns match a retained structural receipt. A first observation or controller-observed drift cannot be declared converged from an older pane assignment; focus-only changes use their separate effect.
- When a project has no explicit tmux-session configuration, the layout observer must resolve the observed session from that generation's effect-owned pane assignment. A configured session remains authoritative; the observer must not reject an already-applied shared-session effect merely because the target project omitted redundant session configuration.
- A windowless editor-layout generation must derive its target from generation-fenced live actor bindings before ambient terminal context: prefer the focused actor when it is already visible in an `agent-doc` window, otherwise a desired sibling actor already visible in that shared window, then the focused actor's live session as the last actor-owned fallback. A focused pane sitting in another session's stash must not re-home an existing shared `agent-doc` window. An explicit window remains authoritative, while the controller process's ambient `TMUX_PANE` and current client session are fallback evidence only; controller recycle or editor-terminal attachment must not relocate a surviving actor surface. This actor-driven target applies only where the layout observer itself follows the effect's panes (a multi-session `tmux_sessions` project). A single-session project with a live `tmux_session` pin, and every navigation lane generation, is never windowless at the effect: the controller passes the window its convergence observer reads (the pin's or the lane's `agent-doc` window, `agent_doc_controller::pane_layout::structural_effect_window`) as the effect's explicit window. The effect and the observation must name the same window; before this a focused pane living in another session's `agent-doc` window moved the editor's columns there while the operator's pinned window kept one pane, and every retry re-applied the same wrong window (reported 2026-10-20 as "1 tmux pane with 2 editor panes").
- An editor `projection` generation may reconcile structure and focus once. After that exact generation has converged, a later live extra pane or active-pane mismatch is operator-owned drift: background observation, identical editor publication, and controller recycle must not stash the pane or select the prior editor-focused pane again. A new editor selection publishes a new generation and may reconcile once in turn.
- Exact-visible provisioning effects must consume the controller's generation-fenced actor bindings before any registry/content fallback. A binding is reusable only while its pane is alive and the live process tree does not own another document; a stale actor, registry, or structural-effect projection must be rejected and repaired/provisioned rather than relabeling or focusing the foreign pane. If the routed actor already owns a live pane, an unrelated actor occupying the desired window must not be positionally relabeled as that routed document.
- Pane-layout actor bindings are scoped to the desired document set. Recorded
  window-id equality is not layout-membership evidence: unrelated stashed actors
  may legitimately share the same repaired stash window, so they must not be
  joined into the desired layout (`#layoutbindstale`).
- Route auto-start may not create a duplicate hidden fallback pane just because split/join heuristics failed. A shared `agent-doc` window may supply a split-only cross-project anchor when every visible pane proves process-tree ownership of a different agent-doc document; this does not rebind or inject into the anchor. If any visible pane has unknown ownership, one appears to own the requested document without authoritative registration, or `split-window` fails beside the chosen anchor, route must fail closed with tmux cleanup commands instead of creating and stashing a second pane.
- Before route or sync auto-start allocates a pane, it must resolve the live same-document owner independently of the registry row. A proven live owner wins over a missing, dead, or stale registry pane and is reused; startup must not allocate a launcher that can discover and refuse the existing owner only after the duplicate pane exists. This shared rule applies to JetBrains, Zed, VS Code, and CLI-triggered routing.
- Once route has created a fresh pane for a document, that pane stays authoritative for the reroute. A concurrent geometry-only registry rebind must not hand dispatch back to an older same-session pane and make the fresh pane disposable. Only a projection rebind is reclaimed this way: an owner or operator that committed a newer generation for another pane wins, and the route follows it (`#dispatchrebindaudit`, see route-owned startup under `sync`).
- Route must never transiently register an existing pane to a different file just to probe readiness. If a candidate pane is already bound to another document, reroute fails closed instead of emitting a temporary cross-file `session_superseded` / `session_end origin=registry_rebind`.
- Actor-store binding must fail closed when the target pane has a non-closed record for another document. Cross-document cleanup is a repair-only operation, not part of normal route/start ownership transfer.
- Route progress diagnostics must be UTF-8 safe when trimming captured tmux lines for stderr/status output. Prompt/status lines containing Unicode glyphs such as `…` or `·` must never panic the binary during a live reroute.

### Startup-miss tracking

- When route/startup acknowledgment times out, the binary records a `startup_miss` state row with pane/session provenance in `.agent-doc/state.db` and shows a visible diagnostic in tmux.
- Codex authentication selectors and browser/device-code waits are never dispatch-ready prompts. Their `>` option glyph must not accept an agent-doc trigger; if authentication never completes, the normal bounded startup deadline records `startup_miss` and fails closed.
- A fresh route-owned Codex pane that resumes a conversation still attached to another app may recover the exact modal `This conversation is open in another app` / `r retry   f fork` screen by choosing `f fork` once, then continuing within the original startup-readiness deadline. This recovery is scoped to the newly provisioned pane: existing-pane reroutes remain fail-closed, unrelated blockers are never answered automatically, a repeated lock after the one fork attempt fails closed, and a later idle composer supersedes the old lock text left in scrollback.
- On the next route/start/sync/session-check path, the tool must distinguish between a stale startup-miss marker and a still-stranded owner. A same-pane marker may only be cleared once newer session-log provenance proves a later open run on that same pane. If the document is now registered to a different pane/session, the old marker is stale and must clear from the current file owner's later `session_start` provenance instead of staying tied to the superseded `session_id`.
- A successful post-dispatch admission projection clears the startup-miss marker.
- `route --dispatch-only` still uses a one-shot reopen instead of the managed command-submission path, but for any existing live managed session that one-shot reopen must submit directly through the resolved pane's tmux input path instead of routing back through supervisor IPC or writing raw bytes into the child PTY. Editor callers add `--plain-trigger` and a longer `--wait-for-ready` budget, which makes the one-shot reopen the plain `agent-doc <FILE>` prompt even when the document's harness normally uses slash commands while allowing slow supervisor startup to settle. The route owns the pane input window for that startup-readiness wait too: before waiting on a latest-run or starting pane, it records a controller-backed route-submit projection (`RouteSubmitStarted`, later `RouteSubmitSettled` or bounded `RouteSubmitBlocked`) so supervisor-owned queue drains, context-reset clears, pending queue context-clear projections, and orphan visible `/clear` / `/new` drafts yield instead of pressing Enter into the same composer. That live-pane submit must use the same centralized tmux submit profile as file-scoped `session clear`: Codex, Claude, OpenCode, and default harnesses send the normalized text plus a named `Enter` key in one tmux `send-keys -t <pane> <text> Enter` operation. It must never use literal `\r` or `\n` as the tmux submit key. Raw child-PTY fallback writes may still encode Enter as carriage return, but that path is not a tmux `send-keys` submit. It must reuse the same bounded repair/restart checks before injecting into an existing pane, and it must honor the authoritative actor readiness guard: prompt-bearing reroutes may not inject into `starting` or dispatch-only `busy` actors until the current generation reaches a dispatch-ready prompt (`prompt_ready=true`) and refreshes to `ready`. A tracked Codex `/clear` or missing managed capability proof may still force a fresh restart on the managed non-dispatch route, but dispatch-only editor reroutes must keep the plain reopen on the authoritative live-session boundary once that readiness guard passes.
- Prompt-bearing one-shot delivery into an existing authoritative actor must still advance the reactive per-document admission projection. Pane acceptance cannot prove that a long-lived harness loaded newly installed admission hooks. A missing projection fails closed; Claude Code and Codex receive one fresh supervisor restart and retry so their startup hook snapshot is reloaded. The route await is a controller-side projection subscription notified by the applied preflight fact, not a polling loop over cycle persistence.
- Every direct-pane submit must emit redacted `tmux_input_event` diagnostics for the payload/key transform and a `route_submit_observation` after route's capture/proof checks. The observation records pane id, harness, phase, elapsed time, whether the trigger was still visible in captured tmux content, and capture length/hash; it never logs the routed prompt text. If capture still shows the trigger after the acceptance window, route also logs `route_submit_issue issue=prompt_not_submitted`. If the trigger leaves the input surface but no dispatch-start proof appears where required, route logs `route_submit_issue issue=accepted_without_dispatch_start_proof`. For forensic recovery, route must also write redacted terminal snapshots under `.agent-doc/logs/route-submit/` and log `route_pane_snapshot ... snapshot_path=...` when it detects a pre-existing drafted trigger, a still-visible trigger after submit/resubmit polling, or accepted pane delivery without required dispatch-start proof. Protected-prompt route refusals must include pane id, protected-input reason, snapshot path when available, and a bounded redacted `draft_preview=...` of the blocking composer input so editor notifications can tell the operator exactly which draft to clear without opening the snapshot first.
- Dispatch-only reroutes should record stronger dispatch-start proof when the harness exposes it, such as Codex hook-state advance or OpenCode pane-state transition. If no stronger proof arrives but the command leaves the input surface, route still treats the shared text+`Enter` acceptance as successful delivery and logs `proof=accepted proof_scope=accepted_only` so log review can distinguish accepted delivery from stronger dispatch-start proof.
- Once hook, actor, or pane-state evidence proves the fresh dispatch started, a slow harness that has not opened its agent-doc cycle by the later admission-projection window remains an in-flight successful dispatch. Route keeps the authoritative pane and must not record or reap it as a `startup_miss`; the eventual admission projection still clears any older miss marker.
- When a prompt-bearing `route --dispatch-only` targets an authoritative actor pane whose runtime still reports `starting`, route must wait one bounded readiness window for that same actor generation to report `ready` with dispatch-ready prompt proof, then fail closed with a single wait/retry diagnostic if it remains unready. The caller's `--wait-for-ready` value is the total readiness budget across same-pane and handoff recovery attempts; recovery may consume the remaining budget but must never reset it. Repeated reroutes for the same pane and generation must reuse that saved timeout state and may log coalesced wait attempts, but must not emit another `route_authoritative_actor_starting_not_ready` timeout until the actor generation or pane changes, or the same generation clears through `ready`, `closed`, or `blocked`. If a route-owned `starting_actor_timeout` block later shows a dispatch-ready prompt for the same pane and generation, route must clear the saved timeout, promote the actor to `ready`, and continue through the normal prompt-gated submit path. No supervisor IPC queue, direct-pane submit, split churn, pane kill, or same-file ownership re-election may occur while the actor is still `starting`.
- When the controller record and healthy supervisor runtime still report an authoritative actor as `starting`, route may promote the controller lifecycle to `ready` only after the resolved actor pane for the current generation shows a harness-specific dispatch-ready prompt (`prompt_ready=true`). After that promotion, dispatch-only uses the same direct-pane submit boundary as other ready authoritative actor reroutes. Before that promotion, the only user-facing recovery is to wait for the prompt and rerun `agent-doc <FILE>`, or restart the stuck owner with `agent-doc start <FILE>`. Timeout diagnostics must be typed and include elapsed time, pane id, generation, actor state, supervisor health/runtime state, prompt-ready status, and last lifecycle transition.
- Claude Code and Codex readiness/draft checks must treat user-configured status lines as presentation rather than protocol. When tmux exposes a cursor row, the direct-pane proof is scoped through that row so arbitrary chrome rendered below the editable composer cannot hide an idle prompt or become part of a draft; busy cues and protected input at or above the cursor still fail closed. If the cursor row is unavailable, the existing whole-pane compatibility classifiers remain authoritative. OpenCode retains its full-pane boxed-composer/chrome proof and lifecycle event integration because its cursor row is not itself a prompt glyph.
- Codex's exact empty-composer rendering `› |` is dispatch-ready: the trailing bar is the TUI cursor placeholder, not an unsent operator draft. A real non-placeholder suffix remains protected and must not be overwritten.
- When a prompt-bearing `route --dispatch-only` targets an authoritative actor pane whose runtime reports `waiting_input` because the supervisor is sitting at its clean-exit restart prompt, route must request one fresh supervisor restart and then resume the same bare reopen flow instead of fail-closing without a submit. `Run Agent Doc` should not strand the pane at `Press Enter to restart...` with no routed `agent-doc <FILE>` follow-up.
- A background orphan-drain route (`background_existing_pane_only`) never takes the degraded path below for a supervisor whose recycle is `InFlight` on its first attempt for a head: that supervisor is mid re-`execve` and its replacement's idle watch owns the next dispatch and the queue auto-clear before it, so the route defers (`route_background_existing_pane_guard outcome=deferred reason=supervisor_restarting`) without recording a dispatch attempt. See `specs/supervisor.md` (`#autoclearorphandrain`).
- When `route --dispatch-only` has a live authoritative actor pane but the supervisor socket no longer reports a healthy runtime or actor state, route must keep that pane as the recovery target when it still matches the current registered/live owner binding. It logs `route_dispatch_only_authoritative_degraded_direct_pane` with supervisor health and runtime actor state, records the controller dispatch attempt, and then uses the normal direct-pane readiness and blocker checks before submitting. It must still fail closed if the pane is not dispatch-ready. Accepted-only delivery is successful for Claude Code, Codex, and OpenCode; Codex hook proof and OpenCode pane-state proof only upgrade the logged proof scope when available.
- Editor `Run Agent Doc` callers may retry the route command only for the authoritative-actor startup guard. Dispatch-only latest-run boot timeouts such as `latest run is still booting ... (timed_out)` already spent the binary ready-wait window and must surface the persisted route diagnostic immediately instead of silently re-waiting the full ready timeout per attempt. A real active-turn cue accepts the editor's reopen trigger immediately; protected-input blockers such as shell history search remain persistent failures.
- Controller `editor_route` owns one plain-trigger admission for fresh runs, interrupted-turn replacements, selected-text runs, and diff steering. Non-empty legacy `selected_text` / `steering_id` fields are compatibility metadata only: they enter the same `route --dispatch-only --plain-trigger` state machine, never a separate supervisor-steering RPC. Plain-trigger admission suppresses prompt-diff queue mutation and submits the harness-native reopen trigger through a live `ready`, `busy`, or `waiting_input` actor; protected interactive substates and non-turn surfaces still fail closed. `plain_trigger` scopes what is *attached* to the dispatch (no prompt-bearing context), not which trigger the harness gets: Claude/OpenCode receive `/agent-doc <FILE>` and Codex receives the bare `agent-doc <FILE>`, matching the supervisor idle-drain rule below (`#runpromptverbose`). It previously forced the bare form for every harness, so the two dispatch paths disagreed for the same harness.
- The explicit editor `--wait-for-ready` budget applies to every pre-submit dispatch-only readiness wait that is trying to prove a starting/latest-run pane has become dispatch-ready, including the session-log `latest run is still booting` probe. It must not fall back to the short Codex dispatch-only default while the user-visible JetBrains command asked for a longer startup-readiness window.
- The controller editor route (`cp:editor_route`) spends that same `--wait-for-ready` budget on layout convergence BEFORE harness readiness, and hands the harness only the remainder (GH #110). Its layout gates (command-plane convergence, then routed-document membership in the published columns, where a column may stack comma-separated documents) re-observe within the budget: the per-attempt await is the remaining budget, and a refused attempt waits for the tmux layout effect state to change (bounded by a short pace) before republishing a fresh route intent. `operator_owned_layout` is terminal and is not retried. Every refusal names the routed document, the receipt `reason`, and the published columns; the focus refusal names both the focused and routed documents. A route-owned supervisor still running a superseded binary after the route effect is surfaced as a route diagnostic (`controller_editor_route_supervisor_binary_stale` plus a warning line in the route output), never as a refusal. The controller re-observes freshness after the route settles: if a pending safe-boundary recycle re-executes the supervisor during that route, it logs the settlement and does not append the obsolete pre-route warning.
- Controller `editor_route` admission gates on superseded plugin bytes by capability, not by the jar on disk (GH #76, #257, #267). When a live JetBrains registration for the routed document maps a plugin jar that is no longer the installed one (`MappedPluginJar::Deleted`/`Superseded`), admission probes that pid's native agent-doc endpoint (the same IPC root controller → editor sends resolve) with one hello round trip (`probe_editor_endpoint_build_for_pid`). An endpoint that answers with the controller's own build proves the editor still serves the route: an exact-263 modular IDE keeps the old jar until restart but hot-reloads the native library, and that library owns the endpoint. The route is admitted, `editor_route_superseded_editor_admitted ... reason=native_endpoint_current` is logged, and a `plugin_bytes_superseded_advisory` line (restart only loads the new plugin UI) is appended to the route output; it is never a refusal. The route still fails closed before dispatch (`editor_route_refused_superseded_editor ... reason=plugin_bytes_superseded`) when the endpoint answers with another build (the native library has not reloaded, so it rejects the controller's messages), does not answer a hello, or is not bound for the project. The refusal names the pid, the mapped jar and that endpoint evidence. Its remedy is the recorded-verdict ladder (`superseded_editor_remedy`: restart once an install recorded that this process refused the restart-free upgrade, otherwise one reinstall). An other-build endpoint first gets the restart-free native reload rung (`agent-doc admin reload-lib`), because the route is admitted as soon as the endpoint answers with the controller's build. Current or inconclusive mappings never probe and never refuse.
- That editor readiness budget is one absolute deadline shared by pane resolution, startup recovery, and dispatch-only retries; each phase receives only the remaining time. A route must also inspect the authoritative pane for an operator composer draft before spending that budget, so a protected draft returns its terminal refusal promptly instead of surfacing as the editor caller's generic timeout.
- If a prompt-bearing dispatch-only reroute reaches a live pane that reports a plain active Codex or OpenCode turn before input is submitted, route must queue that rerun in plain `agent:run`, sync the snapshot, strip any legacy `auto` from the touched queue tag, and print a `queued pending dispatch ... in active agent:run` diagnostic instead of surfacing a persistent editor route failure. A historical `starting`/latest-run readiness probe cannot override a current healthy authoritative actor on the same pane once that actor is `busy` and the pane proves an active-turn blocker; route must leave the startup wait immediately and take this queue path. This queue path is only for active-turn blockers; hook review prompts, permission prompts, shell history search, and clean-exit restart prompts still fail closed with their specific recovery action. Queued composer input also remains protected unless Codex's current queued-input surface contains exactly the reopen trigger requested by this route. That exact match proves the reopen is already accepted behind the active turn, so route coalesces it as successful existing input without sending text or another submit key; a different trigger or any added operator text still fails closed.
- The supervisor idle-queue watch owns the later drain payload for that queued dispatch. Claude/OpenCode drains inject the normal slash-command harness reopen, and Codex drains inject the bare `agent-doc <FILE>` reopen so editor-triggered queue continuation goes through the same harness-native entrypoint as a manual Codex `Run Agent Doc`.
- Drain ownership is single-owner at any instant (#kp5z / #qflood). When a self-driving harness loop is the active drain owner, the supervisor idle-queue watch must defer instead of also injecting `agent-doc <FILE>`, or the two owners flood the live harness input queue with duplicate triggers. The tie-break is a short-TTL drain-owner lease (`.agent-doc/drain-owner/<pathhash>.json`, owner `claude_loop`): the Claude Code `/loop` auto-loop refreshes it via `agent-doc drain-claim <FILE>` just before re-invoking `/loop`; while the lease is fresh the watch returns `SkipSelfDrivingLoopOwner` and logs `idle_queue_drain_decision decision=SkipSelfDrivingLoopOwner owner=… lease_age=…`. A stale or absent lease hands ownership back to the supervisor, so non-`/loop` harnesses (e.g. the Codex `Stop`-hook loop, which never writes a lease) keep getting supervisor drive. The TTL is short so a crashed loop returns ownership quickly; the lease self-expires and never needs manual release.
- Route queue enqueue must be resilient to an unparseable existing `agent:run` body. When an earlier corruption has merged free-text / log-dump content into the queue component (`#jb-run-agent-doc-response-queue-contamination`), `queue::parse` rejects it, but the route must not propagate that as a fatal `route queue dispatch: failed to parse existing agent:run` error that bricks every reroute for the document. Instead it preserves the existing body verbatim, appends the new pending dispatch as a well-formed entry beneath it (idempotently — a dispatch already present in the raw body is not duplicated), and records a `route_queue_dispatch_unparseable_preserved` ops-log diagnostic. The queue corruption is left for separate repair rather than silently dropped.
- When any busy-pane dispatch-only path finds the pane still busy after auto-fix, route must attempt interrupt recovery for OpenCode as well as Codex. For OpenCode, the interrupt sequence sends Escape twice and then waits for an idle prompt. This handles the post-`Clear Session Context` scenario where the agent-doc child process is still running and OpenCode is showing a progress bar.
- File-scoped `agent-doc session clear <FILE>` must resolve a direct-pane submit target in the same order that routed reopen prefers an existing live session binding: authoritative actor pane first, then a current live-owner pane for the same file, then the registry pane. Only when none of those panes is directly addressable on the default tmux server may clear fall back to supervisor IPC inject.
- File-scoped `agent-doc session clear <FILE>` must not run the starting-actor readiness guard, but its operator guard is owned by FlowCore `operator_clear`: clear may submit `/clear` after an idle prompt, clean-exit proof, no live pane evidence, or an otherwise unclassified busy projection with no protected input and no explicit busy cue. A live `agent-doc`/harness wrapper process is not blocking evidence by itself. Protected prompt input such as a permission prompt, queued draft, shell history search, or drafted user text remains a distinct refusal reason; explicit busy cues such as an active Codex turn, hook-review prompt, or help/usage screen fail closed with typed UX data and a pointer to `agent-doc session interrupt-clear <FILE>`. Operators use `interrupt-clear` for intentional destructive discard.
- The `operator_clear` guard consults the supervisor's live-turn evidence (`harness_turn_live`) BEFORE any input is sent, and that evidence outranks a rendered prompt and the unclassified-busy fallback (GH #232). The evidence is the same read the supervisor's empty-preflight reclaim probe makes: the harness-authored turn-active lease for the evidence pane (with its `#staleharnessturnlive` interrupted-transcript retirement, its `#sdkclearwedge` abandoned-prompt retirement, and TTL expiry), or an observed, unsettled `running` dispatch receipt for the document (`#idledispatchstack`) inside the in-flight horizon, which covers the gap between a dispatched trigger going busy and the hook writing its lease. A lease that the harness transcript proves ended (an interrupt, or a prompt cancelled before Claude Code persisted it) is retired before either read, and the `running` receipts dispatched at or before it are settled with it. When the turn ended before its preflight promoted any receipt (Run Agent Doc, then Ctrl+C in a Codex pane, whose preflight runs from the model's first tool call rather than inside `UserPromptSubmit`), the oldest still-`accepted` dispatch receipt at or before the lease is settled instead (`#codexclearwedge`), so the ended turn no longer reads as `dispatch_turn_pending` for the 120s pre-turn grace. That settle runs on the lease-retirement edge itself (`#infraclearsession`), so it also holds when another reader retired the interrupted lease before the guard looked. The guard logs `turn_status_projection_repaired source=operator_clear_guard`, so an idle prompt after a cancelled turn clears (`specs/15-turn-lifecycle.md` § Cancelled-before-persist prompts). Background subagents (`← N agents`) are not live-turn evidence. The evidence is read for an unobservable viewport too (a blank or too-small frame, e.g. a starting supervisor that has not begun relaying its child's screen): it hides the composer, not the turn, so live-turn evidence classifies it as a live turn and the clear is queued; without evidence it stays protected input (`#queuedclearflake`). While it holds, the guard records `flow=operator_clear stage=operator_guard outcome=blocked reason=harness_turn_live`, never `reason=idle_prompt`; an active auto-queue loop takes the existing pause-and-defer path. Otherwise, when the document's supervisor is healthy and reachable and owns the evidence pane, the clear is QUEUED behind the turn (`#clearqueuedliveturn`): the command records one deferred context-clear projection with source `operator_live_turn_deferred_clear`, logs `session_clear_live_turn_deferred ... ui_outcome=queued`, prints `session_clear queued for <file> — a turn is live in pane <pane> (harness_turn_live=true evidence=<kind>, source=<source>) ... ui_outcome=queued ui_outcome_class=queued next_action=none unblocker=none`, types nothing into the pane, and exits 0. The notice cites the evidence and its age (`evidence=<kind> evidence_age_secs=<n>`, the lease's `UserPromptSubmit` second or the oldest open dispatch receipt), so a receipt that outlived its turn is visible instead of silently absorbing presses. Live-turn evidence also includes `dispatch_turn_pending`: a dispatch accepted for the document (Run Agent Doc) whose turn has not started, inside the pre-turn grace (`DISPATCH_PRE_TURN_GRACE_SECS`, 120s); a clear typed then would race the trigger into the composer, so it is queued behind that turn. A trigger that never became a turn does not hold that evidence once withdrawn: when the guard's `#cancelturnprompt` step withdraws this document's exact trigger from the composer and the pane still has no turn-active lease after a 1s hook-latency window, that dispatch's never-started receipt is settled (GH #317; `lease=none reason=trigger_never_submitted settled_pending_dispatches=<n>`), and the same clear re-reads the idle pane and runs (`specs/15-turn-lifecycle.md` § Cancelled trigger restored into the composer). Operator command receipts (`session_*`, including the clear's own) are never dispatches: they are not pending evidence and a turn's preflight never promotes one to `running`. A repeat press while a live-turn clear is pending RE-ARMS the one queued clear (a new clear epoch whose `marked_secs` is the latest press; `rearmed=true` in the notice and in `session_clear_live_turn_deferred`) instead of absorbing it; an auto-loop deferral is still reported as `session_clear already deferred`. Only when no supervisor owns the pane — so nothing would ever deliver a queued clear — does clear refuse, with `ui_outcome=blocked_with_exact_unblocker next_action=wait_for_turn_or_interrupt_then_retry unblocker=wait_for_turn_end_no_supervisor_to_queue_clear`, stating that no `/clear` was sent or queued. That refusal must not name `clear_queued_behind_busy_turn`, which is reserved for the post-delivery verdict where the harness itself queued a typed `/clear` in its composer (`harness_queued_input`). Neither the queued outcome nor this evidence-backed refusal (nor a protected-input refusal) is a dogfood `ACTIONABLE_AGENT_DOC_FIX_PROMPT`: they are the guard working, with an exact operator unblocker, and are not a turn that failed to complete.
- A deferred operator clear dropped for a newer prompt is surfaced to the operator, not silent (`#clearsupersedenotice`). The watch retires it through `retire_superseded_queue_context_clear_deferred_for_file(file, "newer_prompt")`, whose `queue_context_clear_settled` RPC carries `retired_reason=newer_prompt`; the controller folds that `QueueContextClearSettled { retired_reason }` fact, when it settles a still-`Deferred` clear, into `QueueContextClearProjection.superseded` (`clear_epoch`, `target`, `source`, `reason`, `deferred_at`, `superseded_at`) and logs `queue_context_clear_projection_settled ... retired_reason=newer_prompt superseded_epoch=<n>`. A settle without a reason (delivery, `interrupt-clear`, the `#sdkclearwedge` harness-satisfied retirement) or of a clear that was not deferred records no notice, and the next deferred or started clear clears it. The document-turn authority Computed projects it as `TurnProjection.deferred_clear_superseded` (`clear_epoch`, `pane`, `reason`, `superseded_at`), a reactive edge of the document projection on the existing `document_turn_authority_stream`, independent of the turn phase (the newer prompt's turn is usually still running). The supersession happens asynchronously inside the supervisor, long after `session clear` exited with `ui_outcome=queued`, so this stream — not the command's exit — is the only path to the editor. Editors render it per `editors/SPEC.md` § Clear Session Context.
- The supervisor idle-queue watch delivers a live-turn-queued clear exactly like an auto-loop deferral: at an idle boundary (dispatch-ready prompt, no turn-active lease for its pane), it delivers the clear the way an idle operator Clear reaches the same harness and promotes the projection to in-flight. A harness whose native clear keeps the process (Claude Code, OpenCode) gets it typed into the live child; Codex, whose `/clear` exits the process (`#clear-ready-restart`), gets the supervised operator-Clear fresh restart (`clear:fresh`, so the relaunch re-triggers the document only when work is pending) and is never typed `/clear`: a typed `/clear` ended the Codex child and left the pane at the supervisor's restart-or-quit prompt. Delivery logs `idle_queue_watch_deferred_clear_delivered file=<file> target=<pane> harness=<h> clear_epoch=<n> delivery=native_clear|restart_fresh` to `ops.log`, and the replacement Codex's watch settles the in-flight projection once its fresh prompt is idle. While the clear waits, the watch logs `idle_queue_watch_deferred_clear_waiting ... prompt_visible=<b> turn_active=<b> dispatch_turn_running=<b>` once per change of those gate inputs, so a clear held past its turn names the gate that holds it. The dispatch-ready prompt is read from the supervisor's screen with its ghost style kept (`specs/supervisor.md`): a faint harness placeholder is an empty composer even when its wording is on no static list (Codex 0.163's post-turn `› Ask a follow-up question`, haiven-dev `tasks/fpe.md` pane %12, 2026-10-20, where the queued clear otherwise never fired). Recording the deferral is itself a watch edge (`#idleclearrestart`): the controller publishes a document-scoped `agent-doc/operator-clear-wake/v1/<document>` state-plane frame for every deferral (each re-arm included), and the watch parks on it. A Ready, quiescent watch otherwise re-derives nothing until a document revision, editor delivery, or actor edge, and a deferred clear produces none of those; a clear queued behind a route-dispatched turn (the actor never moves) or an abandoned prompt waited for an unrelated document edit. A failed wake publish is logged (`queue_context_clear_wake_publish_failed`) and never fails the deferral. A live-turn deferral additionally waits while the document has an unsettled `running` dispatch receipt, because that turn may have been proven live by the receipt alone and a composer redrawn between tool calls is not an idle boundary (GH #232); an unreadable receipt store fails closed (keeps waiting). It equally waits on a pending dispatch (accepted, its turn not yet started; `dispatch_turn_pending`). The watch and the guard read the `running` and pending receipts as ONE snapshot (`#clearqueuedciflake`): the pane facts are sampled earlier in the watch tick, and a Run Agent Doc turn whose preflight promotes its receipt `accepted` -> `running` between two separate reads read as neither, so a clear queued behind that run was typed into the turn that had just started (main CI 40485993738). The receipt settles at the turn end: the harness turn-end hook (`turn-status idle`) settles the `running` receipts of its pane's documents dispatched at or before the ended turn's lease heartbeat (`specs/15-turn-lifecycle.md` § Cancelled-before-persist prompts, Dispatch receipt), because a route dispatch never moves the actor and so never meets a `Ready` edge. `tests/clear_queued_live_turn_tmux.rs` (`make tmux-ci`) runs this end to end against the real supervisor with a fake harness: a clear pressed while a route-dispatched turn is proven live only by its receipt exits 0 with `ui_outcome=queued`, a repeat press reports the queued clear, exactly one `/clear` reaches the pane after the `Stop` hook, and a newer prompt before the turn end delivers none. It also covers Clear → Run → Clear after a normally ended route turn with Claude background agents running (the clear at the idle prompt is immediate; the second waits behind the run and lands after it) and a repeat press re-arming the clear behind a run started before it, including a run that starts while the watch is held between its pane sample and its receipt decision (the debug-build `AGENT_DOC_TEST_HOLD_DEFERRED_CLEAR_RECEIPT_PROBE` handshake; the held tick must have sampled `prompt_visible=true turn_active=false`). It also replays GH #317 against a fake harness that renders its unsubmitted composer line (`FAKE_CLAUDE_COMPOSER=1`): Run Agent Doc is accepted and its trigger typed but never submitted (no Enter, no turn hook), so the receipt is the only live-turn evidence (`dispatch_turn_pending`); one Clear press withdraws the trigger, logs `lease=none reason=trigger_never_submitted settled_pending_dispatches=1`, passes the guard as `reason=idle_prompt` with no `harness_turn_live` refusal or `session_clear_live_turn_deferred`, and types exactly one `/clear` into the same harness process, which never received the trigger. It also asserts an idle-prompt Clear types `/clear` into the SAME fake harness process (PID unchanged, no restart, no follow-up document trigger) while a background job that process owns keeps running (`#idleclearrestart`). A fake Codex harness that renders Codex 0.163's composer (a bold `›` and a faint `Ask Codex to do anything` / post-turn `Ask a follow-up question` placeholder) and exits on `/clear` covers the pane %12 case: a clear pressed during a Codex turn proven live by its lease alone (the actor stays `ready`) is queued, held while the turn runs (`idle_queue_watch_deferred_clear_waiting ... turn_active=true`), delivered exactly once after `Stop` as `delivery=restart_fresh`, never typed into Codex, and settled by the replacement Codex's watch; every step waits on an ops.log or harness-record handshake. Once a deferral is observed, every watch tick reaches this decision until it is delivered or retired, so delivery follows the turn end instead of the quiescent-maintenance cadence. A deferred clear (either source) never fires after it is superseded: an explicit `interrupt-clear` settles it before running; a harness `SessionStart` `clear`/`startup` receipt newer than the deferral retires it (`#sdkclearwedge`; this also covers the session restarting or the document's session closing and starting again); and a NEWER prompt on the pane — a turn-active lease whose `UserPromptSubmit` second is later than the first lease the watch observed for that deferral — retires it with `idle_queue_watch_deferred_clear_superseded reason=newer_prompt`, because the operator continued the conversation. Exception: when the document has an open dispatch accepted after the deferral's own turn started and at or before its latest press (`marked_secs`) — Run Agent Doc pressed BEFORE Clear — the newer lease is that dispatched turn, so the watch adopts it as the clear's turn (`idle_queue_watch_deferred_clear_adopted_turn reason=dispatched_before_press`) and delivers after it. Clear → Run Agent Doc → Clear therefore ends with the clears in press order, never with zero (haiven-dev `tasks/sdk.md` pane %180, 2026-10-11). The baseline is the first lease observed, not the lease at deferral time, so a clear queued on `dispatch_turn_running` evidence before the hook wrote its lease adopts that late lease as its own turn. The baseline is supervisor-local: a supervisor re-exec while a deferral is pending re-baselines on the turn it then observes. Protected input and clean-exit proof keep their own reasons. The guard is read-only with respect to actor state: an idle-looking composer under live-turn evidence never projects `state=ready reason=live_pane_idle` and never releases the dispatch in-flight marker; every operator idle-projection reconcile skips with `session_operator_idle_projection_reconcile_skipped reason=harness_turn_live`.
- A clear queued behind a `turn_active_lease` whose bound Claude Code transcript shows an **unsettled abandoned prompt** (a prompt snapshot with no prompt record, still inside `ABANDONED_PROMPT_SETTLE_SECS`; see `specs/15-turn-lifecycle.md` `#sdkclearwedge`) is not reported as a live turn (`#clearctxslow`). The guard still queues it exactly as above — the settle window and the supervisor's delivery are unchanged, because a follow-up record that keeps the lease live (a `queue-operation`) can land well after the snapshot — but `session_clear_live_turn_deferred` logs `evidence=abandoned_prompt_settling settles_in_secs=<n>` and stderr prints `session_clear queued for <FILE> — the last prompt in pane <pane> has not started a turn (harness_turn_live=pending evidence=abandoned_prompt_settling settles_in_secs=<n> settle_window_secs=<w>, ...)` with the same `ui_outcome=queued` contract and exit 0. The probe (`settling_abandoned_prompt_for_pane_for_file`) is read-only: it never retires the lease. Observed 2026-10-15 on haiven-dev `tasks/sdk.md` (pane %180): Run Agent Doc's lease at 14:49:04Z, its snapshot at 14:49:06.9Z with no prompt record, Clear Session Context pressed at 14:49:10Z and 14:49:16Z; both presses were told "a turn is live in pane %180" over an idle pane, and the clear ran at 14:49:26Z when the snapshot settled.
- A clear queued on `dispatch_turn_pending` evidence alone — Run Agent Doc accepted for the document, its turn not yet started, so its trigger has not reached the harness or is still being submitted — is not reported as a live turn either (`#clearaftrun`). The queue decision is unchanged (a clear pressed after Run Agent Doc runs after that run; the watch adopts that run's turn instead of retiring the clear), but stderr prints `session_clear queued for <FILE> — Run Agent Doc for pane <pane> was accepted <n>s ago and has not started its turn yet (harness_turn_live=pending evidence=dispatch_turn_pending evidence_age_secs=<n> pre_turn_grace_secs=<g>, source=<source>) ...`, states that no turn is running, that the clear runs after that run and is not dropped by it, and that if the run never starts the clear runs within about `<g> - <n>` seconds (the pre-turn grace expiring releases the delivery gate). It does not offer `interrupt-clear` as a way to clear sooner: on an idle pane `interrupt-clear` skips the interrupt and re-enters this same guard, which queues again. Same `ui_outcome=queued` contract and exit 0; `session_clear_live_turn_deferred` still logs `evidence=dispatch_turn_pending`. Observed 2026-10-15 19:40Z on haiven-dev `tasks/sdk.md` (pane %180): Run Agent Doc accepted at 19:39:58Z (receipt 3847) waited ~22s on the previous cycle's compaction commit before typing its trigger at 19:40:20Z; Clear Session Context at 19:40:10Z was told "a turn is live in pane %180 ... no `/clear` was typed into the running turn" over an idle pane, a second press at 19:40:20Z reported the clear already deferred, the run's turn ran, and the clear was delivered at its end (19:42:25Z).
- After a successful clear delivery, `agent-doc session clear <FILE>` must reclaim any orphaned open preflight cycle the cleared turn left behind so a following `Run Agent Doc` is not wedged by a stale open cycle (`#clear-stops-running-process`). It reuses the same reclaim semantics as explicit run-cancel (`#cancel-orphans-preflight-cycle`): an empty `preflight_started` cycle is abandoned, while a cycle that already captured a response is protected and left intact. The reclaim is best-effort — a failure is logged as a warning and never fails the clear, which already delivered — and records a `session_clear_cycle_reclaim` ops-log diagnostic with the reclaim outcome.
- Editor-surface `Focus` is selection-only: it cannot resume a missing actor or promote a stashed pane. Async focus's `no_promotion=true` takes precedence over a legacy `resume_latest` request. A missing/hidden target returns a not-focused receipt so the retained layout worker owns structural reconciliation; an old focus request must never grow a converged two-column layout to three panes.
- Clear submission proof must wait for a dispatch-ready rendered prompt; an absent command during a blank repaint or spinner is not acceptance. A blank frame uses a bounded ten-second rendering budget, independent of the 900ms stuck-draft budget. A persistent blank ends with `pane_not_rendered` and sends no duplicate clear; visible unconsumed drafts retain their bounded submit-key repair.
- File-scoped supervisor recycle requested/started/settled/status/wait RPCs preserve canonical document identity. A sibling's reexec neither blocks nor settles this document's route. The existing controller document Source derives a keyed recycle Computed, and settlement events wake its bounded waiters. Project-level compatibility events remain separate; old global in-flight markers cannot latch file routes.
- A stale supervisor may consume that document-scoped request before the current harness turn ends only through the child-preserving `execve` transition. Admission requires the exact live child PID/master PTY, no child-reap conflict, drained editor/controller IPC, no executing supervisor-IPC effect, no PTY forwarding transaction, and a terminal filter between escape sequences. The outgoing image freezes supervisor effects and both PTY directions until `execve`; the replacement adopts the same child, pane, actor, session, and open cycle without injecting or replaying a trigger. Failure leaves the old image running. An adopt-only boot that discovers the child gone starts no replacement turn, so it cannot duplicate an input whose consumption is unknown.
- File-scoped `agent-doc session clear <FILE>` and `route --dispatch-only <FILE>` must each record their delivery branch in ops-log, including whether the command crossed the live pane's tmux submit boundary or supervisor IPC inject. Dispatch-only route logs must also record both `proof` and `proof_scope`: attempts without dispatch-start proof are `proof=accepted proof_scope=accepted_only`; Codex hook proof may record `proof=consumed` or `proof=submitted`, and OpenCode pane-state proof records `proof=pane_state_changed`, each with `proof_scope=dispatch_start`.
- Dispatch-only Claude Code, Codex, and OpenCode reroutes may report successful delivery on pane-input acceptance alone, using `proof=accepted proof_scope=accepted_only`. Codex still records stronger hook proof when it appears, and OpenCode still records stronger pane-state proof when the routed trigger leaves the composer and the pane leaves idle chrome within the OpenCode redraw budget.
- Direct-pane reroute telemetry must keep pane-input acceptance latency distinct from harness dispatch-start proof. An empty first capture is not acceptance proof until it remains stable, so a delayed Codex composer draft can still be seen and submitted. A direct submit whose pane-input disappearance is not observed within the tmux/control-mode acceptance window must not be logged as a submit timeout when later proof shows the routed prompt was consumed, submitted, or moved the OpenCode pane out of idle state; in that case `route_latency phase=direct_pane_submit` uses `outcome=acceptance_unobserved_dispatch_proven`, while `phase=dispatch_start_proof` carries the dispatch-start proof. If Codex later has only accepted-without-dispatch-start proof but the same prompt is visibly drafted, route sends one late Enter retry and logs `route_submit_late_resubmit`.
- Supervisor-owned inject coverage must include at least one real socket-backed tmux regression, not only mocked writer tests, so the live IPC listener proves it can hand submitted text off to a real pane through tmux.
- The tmux integration is a hybrid architecture, not a tmux replacement. `tmux-router` owns the common policy: control mode is the lifecycle/event backend, `pipe-pane` is the preferred live output stream when a file/process sink is needed, and tmux `send-keys` remains the visible-pane submit fallback. Managed supervisors own the child PTY and maintain an `alacritty_terminal` screen model for readiness/help/protected-prompt detection, so those checks do not depend on tmux `capture-pane`; owned PTY input is scoped to managed Claude/OpenCode supervisors where byte-exact child input is required and must not become the default layout or pane-management backend.
- If dispatch-only has to inspect a starting-pane reroute, route must spend one bounded recovery window looking for the same actor generation to become `ready` or for a newer same-file startup handoff before it surfaces a `still booting` refusal. A same-file successor pane may be followed; a cross-file rebind must still fail closed. The refusal must happen before tmux or supervisor input is submitted.

### Live-child admission-projection rules

- A live `agent-doc` child or healthy supervisor is not itself proof that the rerouted prompt started a new cycle.
- For Codex reroutes, hook-backed submission proof is preferred before the later cycle-start health check.
- For dispatch-only reroutes, accepted tmux delivery without routed submission proof is a successful accepted-only delivery. Codex hook proof and OpenCode pane-state proof upgrade the log scope to `dispatch_start`; missing stronger proof does not make the editor action fail.
- If dispatch-only route refuses a Codex pane with `route_dispatch_only_blocked reason=codex hook review prompt`, the user-facing error must name the hook-review approval gate and tell the operator to open `/hooks`, approve or disable the pending hook change, wait for the idle composer, and rerun the dispatch-only route or editor Run Agent Doc action. A generic "restore an idle prompt" hint is insufficient for this blocker.
- Claude Code, Codex, and OpenCode dispatch-only routes label accepted pane delivery as accepted-only when no dispatch-start proof is available. OpenCode pane-state transition proof, when observed, is recorded as stronger dispatch-start proof (`proof=pane_state_changed proof_scope=dispatch_start`).
- Frontmatter-only metadata churn must not count as prompt-bearing work that blocks or requires a routed admission projection.

- Navigation lanes (`#ideriderindependentnav` P6): a route acts for a requesting editor client,
  `--client-id` on the CLI or the controller `editor_route` payload's `client_id` (JetBrains sends
  its surface client id). With `editor_navigation = "per_installation"`, an installation-scoped
  client, and a resolved target that is the `tmux_session` pin, the route's target session becomes
  that client's lane session, provisioned on first use (an unmarked same-named session fails
  closed), logged `route_navigation_lane_target`. Only a pane the route starts lands there: an
  existing pane is resolved from the actor binding and registry and dispatched where it lives, in
  any lane (`requester_pane_placement`); a stash rescue applies only to a pane already in the lane.
  A start into an installation lane session (any session marked `@agent_doc_navigation_lane` for
  this root) is staged in the lane's `stash` window (`route_startup_navigation_lane_staged`) for
  the lane's layout to place: the lane's idle placeholder shell never blocks it as an unowned
  anchor, and route runs no raw post-start window sync there. The controller `editor_route` from
  such a client merges over, publishes into, and awaits that client's own lane graph
  (`controller_editor_route_navigation_lane`), never the shared lane's, and skips the cross-root
  main-window delegation (an installation lane is this root's own session); the lane's layout
  provisions the routed document's pane in the lane. The route stays a passive lane claim, so it
  never moves a pane; the Claim actions hand one over.
  **Contended route** (`#ridernavp6routecontended`): when the routed document's live pane (its
  actor binding) lives in another lane of the project (another installation's lane session, or the
  pin for an installation lane), `requester_route_layout` returns `DispatchInOtherLane` and the
  controller publishes **no** layout: the requester's lane would strip the document from every
  frame it projects (`cross_lane_layout_dropped`), so a layout wait on it could never see the
  document. The route skips the layout publication and its wait entirely
  (`controller_editor_route_owned_by_other_lane ... outcome=dispatched_in_other_lane
  layout=unpublished`), dispatches into the existing pane where it lives (its single live owner:
  no second harness, no pane move, no stash rescue across sessions), and reports where it is: the
  result carries `owned_by_other_lane { pane, owner_lane, owner_session }` and its output ends with
  the line `[agent-doc] owned_by_other_lane: <file> runs in pane <pane> of navigation lane <lane>
  (tmux session <session>); ...`, which the JetBrains plugin shows as an information
  notification. The requester's retained lane layout is untouched. A pane in the requester's own
  lane, a pane in a session that is no lane of this project, and a document without a live pane
  keep the publishing route. Shared mode, a route without a client, an unscoped client, and a
  multi-session document binding other than the pin keep the shared target byte for byte; shared
  mode reads no actor pane or tmux session for this decision.

## claim

`agent-doc claim <FILE> [--position left|right|top|bottom] [--window W] [--pane P] [--new-pane] [--client-id ID]`

- After pane/session admission, claim persists any generated document UUID before
  every provisioning branch, including occupied-pane and cross-root fallbacks.
  Its generated-ID diagnostic, route registration, and the new supervisor's
  frontmatter must describe the same identity.
- A provisioning claim binds the new pane before it launches the pane's
  `agent-doc start` and writes no actor state after the launch; the new
  supervisor's `start_session` owns the next generation (`#claimpanecasrace`,
  see route-owned startup under `sync`).
- Claims a document for a tmux pane that is already running the harness.
- Claim reads and writes the **document's** store, never the process cwd's (`#claimwrongstore`):
  the nearest `.agent-doc` project of the canonical document, or for a document with no
  `.agent-doc` ancestor its canonical parent directory
  (`agent_doc_project_root_io::project_root_or_file_parent`), the store `start`, the route, and
  sync's recovered-owner bind (`#syncnorootfallback`) bind it in. The file-scoped stale-claim
  cleanup, the pane-occupancy check, the bind (`register_with_pid_and_cwd_in`, transition
  `claim`/`claim_bind`), the default-component merge into `.agent-doc/config.toml`, and
  `claims.log` all use it. A claim run from inside another project therefore binds the document
  where every reader of it looks, and writes no actor record, registry entry, or config into the
  cwd's project. The cleanup and occupancy reads never create a store that does not exist yet.
  The bind keeps its semantics: it is an operator write that re-projects the actor record as
  `Ready` and records the document's nearest git root as the entry's `cwd` (`#tw4a`).
- Claim's session policy (`tmux_sessions`) and the `tmux_session` pin its cross-session check
  compares the pane with are the same store's config (`tmux_session_policy_for_root`,
  `project_tmux_session_for_root`), never the process cwd project's (`#loadprojectfordoccwd`).
  A claim run from inside another project is neither refused by that project's live pin nor
  exempted from the document store's; a rootless document's store has no pin until it is
  created. Every document-scoped config read (`load_project_for_doc`) resolves the same store
  ([Config](06-config.md#project-config)).
- `--new-pane` is a provisioning mode, not a force/reuse mode. It bypasses current-pane resolution and cross-session validation, provisions exactly one pane in the project's configured tmux session (or the normal auto-detected session when unconfigured), and binds the new document there. It conflicts with `--position`, `--window`, `--pane`, `--force`, and `--isolate`, so it cannot silently replace or migrate the operator's current pane/window.
- The command must enforce the one-live-pane-per-document binding invariant. If the requested pane already belongs to another alive document session, `claim` provisions a new pane instead of commandeering the old one.
- The binding invariant is enforced **across project/submodule roots**, not just against the document's own store registry. A pane owned by a document rooted in another project/submodule does not appear in that store's rows, so `claim` also inspects the requested pane's live process tree: if it runs an agent-doc/codex owner session for a different document, `claim` provisions a new pane instead of commandeering it.
- Cross-session claim is invalid unless the requested pane is in the operator's current tmux session, the configured project session is stale, or the user forced the claim explicitly. The current-session path lets editor "Claim for Tmux Pane" follow the live `agent-doc` tmux window without rewriting `.agent-doc/config.toml`.
- On a cross-session **Reject**, `claim` emits a stable machine-readable marker line to stderr **before** the human bail text so editor plugins (JetBrains "Claim for Tmux Pane", VS Code claim command) can branch on it and render a choice dialog instead of surfacing the raw exit-1 message. The marker shape is `[claim] cross-session-reject pane_id=<id> pane_session=<session> configured=<session>` (`CROSS_SESSION_REJECT_MARKER` + stable `pane_id`/`pane_session`/`configured` field order, formatted by `cross_session_reject_marker`). The human bail message (`pane <id> is in tmux session '<session>' but project session is '<session>'; switch to the configured session or pass --force`) is preserved unchanged for terminal use. The JetBrains and VS Code plugins present **New Pane in This Session / Force Claim / Switch Project Session / Cancel**: New Pane runs `claim --new-pane` without positional reuse; Force Claim runs `claim --force`; Switch Project Session runs `session set <pane_session>` then re-claims; Cancel leaves the file unclaimed. `session set` is also the supported round-trip command for moving managed `agent-doc` and `stash` windows back to the project's authoritative session.
- Pane validation runs **before** any file mutation (`#claim-validate-before-scaffold`). `claim` resolves the pane and applies the cross-session guard before the empty-file auto-scaffold writes/commits the document. A rejected cross-session claim must leave the target file byte-identical (no scaffold, no `agent-doc(<doc>)` commit) — earlier the auto-scaffold ran first, so an empty `.md` opened in a pane on the wrong tmux session was scaffolded and committed and *then* the claim failed with "pane %N is in tmux session '1' but project session is '0'", converting a file the operator never opted into.
- Explicit claim performs only file-scoped stale-binding validation. It must not invoke global resync/prune work or wait on unrelated panes before mutating the requested document.
- For new template documents, `claim` scaffolds the canonical component set, including `<!-- agent:run priority subagents clear -->` and `<!-- agent:backlog priority queue -->`, and saves a baseline snapshot with empty exchange content. Every generated `##` heading shares the opening component-tag line, and the runnable-work section is titled `## Run` rather than `## Queue`. New runnable-work components use `agent:run`; the legacy `agent:queue` spelling remains accepted and existing document headings and markers are not rewritten. `priority queue` makes new backlog work priority-ordered and eligible for backlog-to-queue mirroring by default.

- Navigation lanes (`#ideriderindependentnav` P6): `--client-id` names the requesting editor
  surface client (the JetBrains Claim for Tmux Pane, Force Claim, and New Pane actions pass it).
  With `editor_navigation = "per_installation"`, an installation-scoped client, and the pin as the
  claim's authoritative session, every pane the claim provisions (`--new-pane`, no pane evidence,
  an occupied or foreign-owned pane) starts in that client's own lane session, provisioned on
  first use (`claim_navigation_lane_target`) and staged in its `stash` for the lane's layout to
  place. A pane that already lives in that client's own marked lane session passes the
  cross-session check (`claim_navigation_lane_pane_accepted`); a pane in another IDE's lane is
  still refused without `--force`. Shared mode and unscoped clients are unchanged.

## focus

`agent-doc focus <FILE> [--pane P] [--blocking|--synchronous]` focuses the pane that currently owns the document session.

- Default focus runs through the Project Controller command plane, the same
  serialized latest-wins boundary used by editor selection. When the actor is
  closed/missing or its pane died but the document still has a durable session,
  the typed `resume_latest` policy starts a replacement owner before selecting
  it; a closed projection must not reject that recovery before the policy is
  evaluated. Restart uses typed `ProvisionOnly` startup policy so pane
  creation/start/selection completes inside the short focus RPC; harness
  readiness and the safe-passive layout pass settle asynchronously.
Provision-only startup may split only beside a pane already visible in the
`agent-doc` window; a durable registration in `stash` is ownership evidence,
never a geometry anchor.
- The JetBrains low-latency selection lane publishes a retained `focus_only`
editor-surface Source to the selected document's own controller. The controller
graph derives typed `observe_only` focus rather than `resume_latest`: it may
select an already-visible pane but never performs recovery or pane creation.
The spanning editor surface graph owns that structural work.
- JetBrains editor observations carry a stable `surface_id` derived from the
  owning `ToolWindowPane`, and controller roots are retained independently per
  `(project, client, surface)`. The controller is the sole policy owner for the
  IDE terminal presentation: one focused visible surface exclusively owns a
  document, a later focused detached surface atomically transfers ownership,
  stale generations/sequences cannot move it, and retiring the owning surface
  deterministically returns it to another visible surface or stashes it. The
  JetBrains effect moves only the dedicated agent-doc terminal tool window; it
  never relocates unrelated tabs from the stock Terminal tool window. Missing
  pane-placement capability fails closed without changing tmux ownership.
  Each accepted publication is positive liveness evidence for its own endpoint:
  JetBrains unions the current/published surface with an available backend
  window snapshot before retiring previously retained endpoints, so a lagging
  detached-frame enumeration cannot retire the surface that just published and
  fence every later observation in that generation. An empty backend window
  snapshot remains unknown liveness for Remote Dev and retires no endpoint.
Its `active_window_guard` registers a project-scoped fence at async admission;
supersession and the command deadline are checked again while holding that
fence across the final `select-pane` effect. A worker that was accepted earlier
cannot select after a newer editor intent has arrived. On i3, the same final
effect also requires a supported editor to own desktop focus. When the operator
has moved to another i3 window, editor-origin focus is suppressed. Automatic
layout reconciliation may still repair pane placement, but its focus target is
stripped at the effect boundary so it cannot surface `stash` after the editor
loses desktop focus.
- When several editor installations publish surfaces into one project (IntelliJ IDEA and
  Rider; `#idefocusowner`), only the installation that most recently gained focus drives the
  shared tmux `agent-doc` window. Installations are parsed from installation-scoped client ids
  (`<lane>@<installation>-pid:<pid>`); an accepted `focus_only` focus-lane fact is the focus-gain
  claim. Each controller root keeps a `Source` of the pure `InstallationFocus` fold and derives
  the owner and a per-surface `SurfaceAuthority` as `Computed`s in the controller `ProcessScope`; no
  caller recomputes or polls it. A deferred surface's observation is accepted, folds to `Idle`, and
  its receipt names `deferred_to`; it derives no tmux consequence from an editor fact, a tmux
  refold, or a retained retry until its installation claims again. Every ownership move starts a
  new epoch, and the first observation a surface folds under a new epoch re-projects its layout
  unless tmux is observed to match it. Retiring the owner's last surface on the root falls back to
  the most recently focused live installation under a fresh epoch. Unscoped clients never claim
  or defer.
- Layout ownership is keyed by `LayoutOwnerKey` = (project root, navigation lane)
  (`#ideriderindependentnav`), never by the root alone. The mode is `editor_navigation` in
  `.agent-doc/config.toml` (`shared` | `per_installation`), read once when the controller
  starts; absent or unknown is `shared` (an unknown spelling logs
  `editor_navigation_mode_rejected` and a warning). `NavigationMode::Shared` maps every client of
  a root to the shared lane, so the key is exactly the per-root `#idefocusowner` owner above.
  `NavigationMode::PerInstallation` gives each installation-scoped client its own lane, backed by
  its own tmux session `<project session>-<installation>` (characters outside `[A-Za-z0-9_-]`
  become `-`, e.g. `agent-loop-IU-0a1b2c3d`); unscoped clients stay on the shared lane. A lane has
  one installation, so no installation is ever deferred to another, and retiring one
  installation re-stamps no other lane's owner. A lane is a tmux session, not a window: tmux keeps
  one current window per session for every attached client, and a pane belongs to exactly one
  session. The controller keeps one pane-layout graph per lane (its own desired layout,
  observation, receipt, structural assignments, width memory, and published generation, all in the
  controller process scope and derived from the one actor store); the shared lane's graph is the
  one every non-lane caller uses (a route without an installation-lane client, and a
  `sync_tmux_layout` command without a client id), and installation lanes' graphs are created on
  first use. An editor route from an installation lane's client uses that lane's graph (P6, see
  `route`).
- Plugin layout publications are lane-routed (P3): a `pane_layout_desired` state-plane frame
  carries its publisher's surface `client_id`, and the controller projects it into that client's
  lane graph (a frame without one, and every frame in shared mode, is the shared lane). Each lane
  has its own latest-wins projector, so one installation's newer frame never coalesces away
  another's. The manual Sync Tmux Layout action publishes this way, so it arranges the requesting
  IDE's own lane. A lane frame counts as seen for the shared lane's route plane catch-up from
  the moment it is dispatched to its lane, before (and regardless of whether) the lane's
  projector applies it, so a shared-lane route never waits on another lane's projector.
  Publication returning is not dispatch: the state plane only schedules the channel's
  projection Effect, which runs in whichever thread holds the controller context's Effect
  drain (often a lane projector publishing into its own lane graph), so the route's catch-up
  is a bounded wait woken by the dispatch-time note, never a zero-time check
  (`#laneplaneflake`).
- The `sync_tmux_layout` controller command is lane-routed (P6): its invocation may carry the
  requesting editor surface's `client_id` (FFI `agent_doc_sync_tmux_layout_for_client_json`; the
  command-plane payload `agent-doc.sync_tmux_layout.v1` carries the same optional field, omitted
  from the wire when unset). The controller consumes the field and publishes the command, as the
  `command` publisher, into that client's lane graph (`NavigationLane::for_client`), logging
  `sync_tmux_layout_lane_routed navigation_lane=.. client_id=.. generation=..`; the receipt is that
  lane graph's projection of the published generation. An installation lane's command never writes
  the shared durable column memory or the shared status channel. A command without a client id, an
  unscoped client, and every client in `shared` mode publish into the shared lane exactly as before
  (the legacy FFI `agent_doc_sync_tmux_layout_json` is the client-scoped entry with no client).
  A `manual` command is an explicit claim in its own lane (P4).
- The recycle-settle republish is per lane (P6). When a supervisor reports a settled recycle, every
  lane graph the controller holds (shared first) claims the republish against its own last pass's
  gated documents (GH #136 (d)) and, when that lane gated the settled document, republishes its own
  retained layout as a fresh `recycle_settled` generation into its own lane only
  (`pane_layout_recycle_settled_republished document=.. navigation_lane=..`; the shared lane's line
  has no `navigation_lane`). A lane that did not gate the document, or holds no retained layout, is
  not republished, and one lane's settle never re-projects another lane. Under `per_installation`
  every lane's republish is passive: a retained explicit claim (`manual`) is republished with
  `caller_kind = recycle_settled` (`RECYCLE_SETTLED_CALLER_KIND`), so the republish can only drop a
  document whose pane has since moved to another lane (`cross_lane_layout_dropped`), never hand a
  pane over; columns, focus, window, and `no_autostart` are the retained ones, so it starts nothing
  the retained layout would not. In `shared` mode the republish is the retained invocation
  unchanged, byte for byte as before lanes.
- Each lane graph has its own effect sink and latest-wins worker, and its structural-effect
  cancellation is bound to that lane's published generation, so a newer layout in one lane never
  supersedes or cancels another lane's sync body.
- Pane-layout status is per lane (P6): each lane graph publishes its own derived status (phase,
  reason code, attempt, columns, observation) on its own state-plane channel, with its own producer
  epoch. The shared lane's channel is `agent-doc/pane-layout/status/v1`, unchanged from before
  lanes; an installation lane's channel is `agent-doc/pane-layout/status/v1/lane/<installation>`
  (`NavigationLane::state_channel`). A `state_plane_subscribe` to the status channel may carry the
  subscriber's surface `client_id`; the controller then reads that client's lane channel
  (`NavigationLane::for_client`), so an installation lane's sync progress or failure never shows
  to another lane's subscriber and never displaces the shared channel's retained snapshot. A
  subscribe without `client_id`, an unscoped client (VS Code), and every client in `shared` mode
  read the shared channel exactly as before, and the field is omitted from the wire when unset.
  Other channels ignore `client_id`.
- Lane routing (`per_installation` only; shared mode is the identity and reads no extra tmux
  state): an editor surface `Sync` publishes into the observing client's lane graph. An
  installation lane's desired layout never writes the shared lane's durable column memory and
  never republishes the shared `pane_layout_status` channel. Before its structural effect, a
  lane's worker resolves its route: an installation lane requires a `tmux_session` pin
  (`navigation_lane_requires_tmux_session`, refused), provisions its session on the first driving
  layout (`new-session -d -s <lane> -n agent-doc -c <root>` + `stash`, marked with the
  `@agent_doc_navigation_lane` session option = project root, logged
  `navigation_lane_session_created`), and targets that session's `agent-doc` window explicitly.
  A same-named session without that mark is refused (`navigation_lane_session_not_owned`), never
  adopted. The drift survey and the target-window fallback of an installation lane observe its
  own session, never the pin. A lane's surveyed layout refolds only that lane's editor clients.
- Pane ownership per lane: a document's pane lives in exactly one session. A lane's sync keeps
  only documents with no live pane or with a pane in its own session; a document whose pane lives
  in the project session or a sibling lane session is dropped from that lane's columns
  (`cross_lane_layout_dropped document=.. owner_lane=.. owner_session=..`), and a focus target
  among them is dropped too. The sync body re-applies the same rule after column memory as
  defense in depth. An editor `Focus` on such a document is refused with `owned_by_other_lane`
  and never escalates to a structural sync. An installation lane's focus escalation republishes
  that lane's layout. Lanes are allowed sessions under `tmux_sessions` when derived from an
  allowed session, and a document bound to the pin belongs in every lane of the pin. The sync body
  applies the lane rule again after a passive focus-only expansion, which restores columns from the
  project's remembered layout and so could otherwise name another lane's document.
- Explicit lane handoff (P4, `per_installation` only): a lane publication whose `caller_kind` is
  `manual` (the JetBrains `Sync Tmux Layout` action and the layout sync that follows `Claim for
  Tmux Pane`, `Force Claim`, and `Load Tmux Window`) is an explicit claim
  (`LaneClaim::Explicit`). Its worker keeps each document whose pane lives in another lane of the
  project and hands the pane over before the structural sync: the pane moves into the requesting
  lane session's `stash` window (pane id unchanged, so its actor binding stays valid; ownership
  is where the pane lives, so no other record moves) and the sync places it
  (`navigation_lane_handoff document=.. pane=.. from_lane=.. to_session=.. placeholder=..`).
  Both sessions must be lanes agent-doc owns, the project session or an installation lane
  session carrying `@agent_doc_navigation_lane` = this root; a session that only carries a
  lane's name is never a source or a target (`#bare-foreign-session-guard`). A pane that has
  left the claimed session since the claim was routed, or any other refusal, keeps the pane where
  it is and drops the document as a passive sync would (`navigation_lane_handoff_refused
  reason=navigation_lane_handoff_{source,target}_not_owned|pane_moved|pane_missing|move_failed..`).
  When the pane was the last one in the old lane's `agent-doc` window an empty shell is left in its
  place so that lane keeps its window (its next sync stashes the shell inside its own session). The
  lane the pane left, when this controller holds its retained layout, is republished as a fresh
  generation of that layout with `caller_kind = lane_handoff`, `no_autostart`, and publisher
  `lane_handoff` (`navigation_lane_handoff_republished`), so it re-projects without the document
  and can never take the pane back or start a new one. Every other `caller_kind` (`automatic`
  heartbeats, focus escalations, `editor_route`, `resync`, `recycle_settled` republishes) is
  passive and never moves a pane. A handoff retried by the same generation is a no-op (`AlreadyInLane`).
  Shared navigation mode never resolves lane sessions, so it never hands over.
- Lane migration and cleanup (P5, `agent-doc gc` and the project controller;
  `project_controller::reconcile_navigation_lane_sessions`, planned by the pure
  `plan_navigation_lane_reconcile`). The reconcile observes every session whose name is an
  installation lane of the `tmux_session` pin (a project without a pin has no lanes and is a
  no-op): whether it carries `@agent_doc_navigation_lane` = this root, whether a tmux client is
  attached or the caller reports its installation live (the CLI reports none), and each pane's
  ownership (a session-registry entry of this project names it) and foreground command (anything
  but a known bare shell is busy). A session without the mark is retained (`not_marked`) and
  nothing in it is read, moved, or closed (`#bare-foreign-session-guard`); a registry that cannot
  be read observes nothing.
  - Placeholder shells (`#lanegcflake`): every shell pane agent-doc itself creates in a lane (the
    initial `agent-doc` pane, the `stash` shell, a handoff placeholder) carries the pane option
    `@agent_doc_navigation_lane_placeholder` = this root. While no tmux client has ever attached
    to its session (`#{session_last_attached}` is empty), such a pane is never busy, whatever its
    foreground command: a shell that is still starting reports `tmux` before `exec` and then its rc
    files' children (`grep`, `mkdir`, prompt helpers), none of which the operator started, so the
    command is not evidence. Once a client has attached (an IDE terminal, `attach`,
    `switch-client`), the mark no longer applies and the foreground command decides again, so a
    program the operator left running in it keeps the lane. A pane agent-doc did not create is
    never covered by the mark. An unreadable attach history counts as attached.
  - `per_installation` (enable migration): no pane ever moves. Panes that lived in the project
    session before the switch stay there, and each installation lane starts empty and acquires
    panes through its own layouts and explicit claims. A lane session is closed only when it holds
    no document pane (`owned_panes`), no busy unowned pane (`busy_unowned_pane`), and no live
    surface (`live_surface`); an installation that retired keeps its lane until then. A closed lane
    is provisioned again by its next driving layout or IDE terminal attach.
  - `shared` (disable migration): every lane is retired. Each document pane of a lane session
    moves into the project session's `stash` window (`navigation_lane_pane_folded pane=..
    from_session=.. to_session=..`; same pane id, so its actor binding stays valid; nothing is
    restarted or duplicated), from where the shared lane's sync places it. When the project
    session is not alive nothing moves or closes (`project_session_missing`). After the folds the
    lane closes unless a busy unowned pane or an attached client keeps it.
  - Switch back before gc (`#ridernavp6switchback`): after the switch to `shared` and before
    `agent-doc gc` runs, lane sessions still hold document panes. The shared lane's sync then
    (a) never arranges a lane session: when the session it resolves is a lane of the pin marked
    for this root (the focused document's pane, or the tmux client's current session, is still
    there, or the caller names the lane's own `agent-doc` window), it arranges the live project
    session instead and drops the explicit window (`navigation_lane_stale_target_retargeted
    stale_session=.. target_session=.. explicit_window=..`, decided by the pure
    `shared_sync_target_session`), so no project pane is ever joined or swapped into a lane;
    and (b) adopts each laid-out document whose pane is still in such a lane before the
    reconcile, with the P4 handoff mechanics (`hand_over_navigation_lane_pane` into the project
    session's `stash`, an empty shell left when it was the lane window's last pane;
    `navigation_lane_stale_pane_adopted` / `navigation_lane_stale_pane_adopt_refused`), so the
    reconcile places the same pane from the project session. This applies to every shared sync,
    passive heartbeat and manual Sync Tmux Layout alike: in shared mode no IDE navigates a lane,
    so adopting is the migration gc would do, not a cross-lane move. The pane id and actor
    binding are unchanged and the pane is alive and registered, so nothing autostarts a second
    pane; a document outside the shared layout stays in its lane until a sync shows it or gc
    folds it, and gc then closes the lane. A session that only carries a lane's name (no mark)
    keeps the pre-lane rules (`#bare-foreign-session-guard`); a dead project session leaves the
    resolved target unchanged.
  - A close re-observes the session first and closes it only when the fresh plan still closes it
    and it holds no document or busy pane (`close_refused_on_reobserve` otherwise), then runs
    `kill-session -t =<session>` (`navigation_lane_session_closed session=.. mode=..`), which
    takes only bare shells (the session's initial pane, its stash shell, a handoff placeholder).
    `--dry-run` reports the plan and changes nothing.
  - Controller trigger (`#ridernavp6gc`): the project controller runs the same reconcile on its
    own, so neither an IDE that quits nor a switch back to `shared` waits for `agent-doc gc`.
    `NavigationLaneReconcileGraph` lives in the controller `ProcessScope`. The editor surface
    graph publishes the installations that have at least one unretired surface on the project
    (`editor_installation` of each client id, every lane) whenever a surface joins or retires;
    `seen` accumulates them and `live` is the current set (one `Source`, so no transient
    retirement is ever derived). A `Computed` derives the request with
    `derive_lane_reconcile_trigger(mode, seen, live)`: under `per_installation` only once a seen
    installation has retired (`retired = seen - live`; startup and joins request nothing, and a
    relaunched installation leaves the set), under `shared` always (the first request is the
    disable migration at controller start; a retirement re-requests it). An `Effect` hands each
    new request to one worker thread (requests coalesce to the newest), which runs
    `reconcile_navigation_lane_sessions_in_mode` with the controller's own mode (read at its
    start, never a re-read of the config) and publishes a `NavigationLaneReconcileReceipt`
    (`navigation_lane_reconcile_triggered mode=.. retired=.. folded=N closed=N failures=..`).
    Liveness is `LaneLiveness::RetiredOnly(retired)` under `per_installation`: only a retired
    installation's lane may close, and a lane whose installation this controller never saw (it
    quit before the controller started, or has not reconnected) counts as live and is left to
    `gc`. Under `shared` no installation keeps a lane (`LaneLiveness::Live({})`), exactly as
    `gc`. Every rule above (marked sessions only, busy unowned panes and attached clients retain,
    unreadable registry observes nothing, re-observe before close) applies unchanged.
    `agent-doc gc` keeps running the reconcile with `LaneLiveness::Live` of the installations it
    is given (none from the CLI).
  - Ownership against the shared sync's stale-lane adoption (`#ridernavp6switchback`): after a
    switch back to `shared`, a sync adopts the lane panes its layout shows, and the controller's
    start reconcile folds every lane pane and closes the lanes. Every non-dry-run reconcile
    (controller or `gc`) observes and applies while holding the project's
    `.agent-doc/sync.lock`, the flock every sync body holds, so the two never interleave: the
    first to run moves a pane and the other observes it already in the project session (a fold
    whose pane is already home is skipped, not a failure). An unobtainable lock fails closed
    (`sync_lock_unavailable`, nothing moved). The controller's worker also runs only while the
    configured `editor_navigation` still equals the mode its request was derived under
    (`mode_changed:<from>-><to>` otherwise), so a controller handing off to a successor started
    under the other mode never folds or closes lanes the successor routes into.
- The `tmux_focus_state` read projection is per lane: the request's `caller` is the editor client
  id, and the projection reports the active pane of that client's lane session (shared lane: the
  configured session). A lane reports a document only while its own session's current window is
  `agent-doc`; a lane whose session does not exist is `tmux_session_not_alive`, never a fallback
  onto the shared session. An installation lane's projection carries `navigation_lane`; the
  shared projection omits the field, keeping its pre-lane wire shape. FFI:
  `agent_doc_tmux_focus_state_for_client_json(project_root, client_id)` reads the client's lane;
  `agent_doc_tmux_focus_state_json` keeps the shared lane. Without a runtime (a cold read) the
  mode is the project's configured `editor_navigation`.
- A `focus_only` refusal the selection lane cannot repair (`actor_pane_not_visible`,
`outside_agent_doc_window`) escalates to the structural layout owner. With no
editor columns it republishes the retained desired layout **unchanged** when
that layout already covers the document. It must never append the asking
document as a new column (GH #106): the escalation's output is its own next
input, so appending grew the column count by one per tab switch. A document the
retained layout does not cover cannot identify a structural column and therefore
defers (`cause=focus_requires_frontend_layout`); it never replaces the focus or
rightmost column. A covered document may update focus intent without changing
membership or order (GH #120, GH #126, GH #343). A later complete positive
frontend snapshot may include the uncovered document and replaces the cached
target atomically.
The structural handoff's result is terminal for that focus observation. A
successful publication returns `focus_escalated_to_structural_layout`; a policy
deferral returns `focus_deferred:<cause>` (for example
`focus_deferred:no_frontend_layout_snapshot`). Neither is an effect failure:
the controller clears the editor-surface retry instead of replaying the same
focus, and the editor retains the receipt instead of forcing a complete layout
reconcile. Only an actual publication error leaves the original focus refusal
actionable. This is load-bearing when Remote Dev supplies flat evidence: a
forced `sync_tmux_layout` has the same missing geometry and is certain to be
refused, so it cannot be a recovery for the deferral (GH #357).
An `outside_agent_doc_window` refusal always publishes a fresh structural effect generation even
when its columns, order, and focus are identical to the retained escalation: selecting the
`agent-doc` tmux window is itself the missing consequence. Other identical escalation reasons may
coalesce; that branch writes `controller_editor_surface_focus_escalation_coalesced` with the
document, reason, and retained generation so an inert publication is diagnosable (GH #154).
An escalation is a derived focus intent, never structural evidence (GH #224, GH #343). It may
update focus intent against the last accepted frontend snapshot, but it must not replace, insert,
remove, regroup, or reorder a column. If its requested document is absent from that snapshot, the
controller preserves the exact cached frontend column vector and defers structural work until the
frontend publishes a complete positive snapshot. A route lease may buffer such a snapshot, but
release applies that exact vector unchanged; backend state, focus history, tmux observation, event
timing, and an `unknown`/partial manual capture are never allowed to reconstruct editor structure.
When a frontend connection is authenticated, its authority is pending immediately—before the
first debounced snapshot or schema negotiation completes—so local backend fallback cannot act in
that window. Between snapshots the controller may continue enforcing only the last accepted
frontend vector. An explicit `Sync Tmux Layout` replaces stale structure only when its capture is a
complete positive frontend snapshot; otherwise it preserves the last accepted vector or defers.
- Realising a column is audited after tmux-router runs (GH #109). A column pane
that moved windows (a stash → layout promotion), runs another document, or
whose route-owned supervisor maps a superseded binary gets one
`layout_column_pane_selected` line naming its source (`pre_resolved`,
`registry`, `router_ephemeral`), origin and final window, binding, supervisor
freshness, and the candidates. A pane bound to another document is reported as
`layout_column_pane_foreign_document` and never touched. A stale supervisor in
the column's own pane (its `/proc/<pid>/exe` names an unlinked file) is gated
out BEFORE tmux-router selects or promotes it (GH #121,
`layout_column_pane_excluded`) and is never reaped: the layout path requests the
existing safe-boundary recycle, at most once per supervisor per ten minutes.
A desired stale pane already in the target window is the zero-movement
exception (`layout_column_pane_stale_visible_preserved`): it remains in the
router input while the recycle is pending, because gating it out would itself
stash a visible live session during an install. The focused document's stale
pane outside the target window is admitted
(`layout_column_pane_stale_focus_admitted`) only when realising it stashes no
pane in the target window that holds a fresh turn-active lease — an inactive
stale-supervisor pane never replaces a live agent pane (GH #124,
`admission=excluded_focused_live_turn_protected:<panes>`). The tie changes when
the requested stale pane itself has a fresh turn-active proof: a newer explicit
editor focus may replace a visible live-turn peer because tmux only moves that
peer to `stash`; neither process nor turn is interrupted. That path records
`admission=admitted_focused_active_turn_over_live_turn_peers:<panes>`, preserves
the editor-observed column count, and makes the newly focused turn visible.
When every column is gated out the current layout is preserved.
- The focus exception is bounded (GH #136). It is granted on the strength of
the safe-boundary recycle that will make the pane fresh, so it is decided by a
total transition function (`layout_column_audit::stale_focus_admission`) over
facts local to the controller at effect time — never over the order, timing,
or arrival of editor publications, which cross JetBrains Remote Dev and
Zscaler and may be delayed, reordered, duplicated, dropped, or replayed after a
reconnect. A stale focused pane already visible is never moved by it. A stale
focused pane still in a `stash` window is promoted only while (1) its recycle
request is not *overdue* and (2) promoting it does not realise more columns
than the target window holds (`max(window_panes, 1)`) — a stale stash pane may
take the place of a column, never add one (`admission=excluded_focused_stash_would_widen`).
The request is *overdue* when the FIRST unconsumed request (the oldest
`SupervisorRecycleRequested` epoch above the highest `Started`/`Settled` epoch)
is older than `AGENT_DOC_STALE_RECYCLE_CONSUME_BOUND_SECS` (default 120s) while
the pane has no active-interaction proof (`admission=excluded_focused_recycle_overdue`,
`prior_request=overdue:reason=...:age_secs=...`). That proof is a fresh
turn-active lease from the document's owning project root, a live foreground
harness blocker (including an active turn or permission prompt), or a matching
generation-fenced `Busy`/`WaitingInput` actor with a fresh live supervisor
lease. The owning-root and foreground fallbacks matter for nested-project
documents and operator interactions that outlive short coordination markers;
they read
`pending:...:deferred_by_turn`. Re-requests and install fan-outs refresh the
request's reason but never its clock, so an overdue refusal cannot flap back
into an admission until the request is consumed. The bound is a liveness
budget for a local process, never a safety argument: every invariant holds for
any value (`formal/tla/StaleColumnRecycle.tla`, where it is an arbitrary
`Expire` event).
- The recycle request is durable, level-triggered `state.db` state, re-read by
the owning supervisor at every idle boundary and settled per epoch, never a
one-shot notification. A request whose cause is a replaced binary
(`stale_supervisor_turn_stage`, `install_fanout`, `install_fanout_force`) stays
live past the request TTL for as long as the supervisor is stale (GH #136: an
install fan-out used to overwrite a non-lapsing stale request with one that
lapsed after 900s, leaving `prior_request=unconsumed` for up to 22 hours).
- The layout effect acknowledges the columns it gated out:
`ControllerTmuxLayoutSyncReceipt.gated_documents` (serde-defaulted). The
tmux layout effect state measures convergence against the desired columns minus
those documents and does not demand focus of a gated document, so a gated
column converges in one attempt instead of cycling `retry_pending` for as long
as the supervisor stays stale. An all-gated pass converges as
`all_columns_gated_layout_preserved`. Re-admission happens on the next layout
publication after the pane reads fresh. agent-doc's own `⚠ STALE SUPERVISOR`
title is diagnostic only, never evidence. Every registered agent-doc pane title
contains the document basename in idle, active-turn, stale-supervisor, and
stale-plus-active states. The four projections are respectively `sample.md`,
`⟳ sample.md — turn in progress`, `⚠ STALE SUPERVISOR — sample.md`, and
`⚠ STALE SUPERVISOR — sample.md — turn in progress`. A pane title carries at
most one status marker: a busy stale pane never welds the busy `⟳` marker behind
the warning. If registration is not yet observable, the legacy generic title is
a best-effort bootstrap fallback until the next hook or freshness projection.
- Explicit `--pane` remains a direct tmux selection escape hatch. `--blocking`
  keeps the legacy synchronous local actor/registry resolver and may surface a
  stashed pane before returning.
- When the controller-local actor cache has a live actor for the document
  session, default focus selects that actor-owned pane. The transactional
  registry remains a fallback binding helper inside controller reconciliation.
- Live-owner precedence: a pane resolved from the local actor cache or the
  transactional registry only proves the pane is alive, not that it still owns
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
- Editor command-plane focus must apply the same live-owner precedence when the
actor projection is `closed`/`blocked` or has already been pruned. Its narrow
fallback may use the latest open session-log pane only when the pane is alive,
its foreground still owns a live agent, and its process tree exactly names the
selected document. A bare-shell remnant or a pane reused by another document
must fail closed; focus never mutates the actor or registry to make the proof
pass.
- Editor-to-tmux focus and tmux-to-editor recall must not feed back into each
  other. A fresh editor selection acquires a bounded handoff lease only after
  the retained focus projection's exact effect receipt proves `select-pane`;
  accepted state, `missing_actor_record`, and suppressed focus acquire no lease.
  The reverse poll ignores the previous pane while that proven lease is live,
  consumes it when tmux reaches the intended document, and lets tmux regain
  authority only after the lease expires.
- The command-plane `tmux_focus_state` read projection must recover the active
route-owned document from exact process-tree ownership when the active pane's
actor row is missing. The recovered path must resolve inside that controller's
project root; a foreign-root owner remains unbound for that controller.
- Reverse tmux-to-editor focus observation is demand-driven by the active embedded
  agent-doc terminal. Its invariant is that an open markdown tab alone never
  generates periodic controller RPCs: a background project surface, editor focus,
  another tool window, or a hidden agent-doc terminal parks the 500ms observation
  effect and clears its last-observed document. The JetBrains terminal host owns
  the active-terminal source, and `TmuxPaneFocusSync` owns the derived poll
  decision and socket effect. The transition table is `markdown open + project
  surface active + agent-doc terminal selected/active -> poll`; every other input
  combination is `park`. The evidence inputs are JetBrains project-surface focus,
  selected terminal content, tool-window activity, and open markdown files. The
  reactive topology is those focus/content sources -> the pure poll predicate ->
  the existing `tmux_focus_state` effect; imperative extraction is limited to the
  terminal-host adapter and no durable controller state is inferred from it. The
  allowed surfaces are the JetBrains plugin and this focus contract; controller
  handoff and tmux ownership policy are out of scope. Kotlin truth-table tests
  verify that idle/background states do not reach the transport and that the
  active embedded-terminal state still does.
- Editor selection focus is a single latest-wins retained source per project.
All tab targets share one focus generation, so a burst of selection events
coalesces to the newest document instead of replaying older pane selections
after it.
- A surface change that replaces a visible document and changes focus is one
retained tmux layout effect state with two consequences: reconcile columns, then
select the requested pane. Column observation alone must not mark that
projection converged. Its typed effect receipt must also prove the final
`select-pane`, and the latest-input-revision worker mutex fences that effect so
a newer desired layout or actor binding either supersedes it before selection or
deterministically follows it. A failed exact-input attempt publishes
`retry_pending` while that same worker remains retained, waits on a bounded
backoff that a newer desired-layout or actor-binding revision can interrupt,
and retries the exact current generation. Convergence, supersession, or
controller teardown ends that ownership.
Tmux observation and retry must not run on the Project Controller IPC accept
thread or in a detached periodic timer.
- Layout focus is refused for a pane that is not co-visible
  (`#panewindowdrift`). A file→pane assignment, the actor row, and the registry
  entry each carry the window captured when the pane was bound, and none of them
  re-derives it — so after the pane is moved into a `stash` window every
  record-vs-record check still agrees and the projection mirrors editor focus
  onto a pane the operator cannot see. The reference window is the projection's
  own target window (`invocation.window` when it is already a window id,
  otherwise the `agent-doc`-named window in the project's configured session —
  the same resolution the drift survey performs), never an inferred active,
  first-column, or majority window, so a legitimate multi-window layout keeps its
  own reference. Before `select-pane` the effect compares that window against the
  pane's live `#{window_id}` via
  `agent_doc_controller::pane_layout::pane_window_binding_drifted` and, on drift,
  publishes `focus_pane_not_co_visible:<file>:<pane>:live_window=<w>:layout_window=<w>`
  with `focus_applied: false`; the structural reconcile then runs and the retry
  focuses the pane once it is back in the layout window. Strict tightening: an
  unresolved window on either side is never drift, so a transient tmux read
  cannot refuse a legitimate focus. The drift survey applies the same rule to its
  observation — an assignment naming a pane outside the target window's own
  ordered pane list is not a visible column and must not satisfy a layout slot or
  stand in as `expected_focus_pane`, so a stashed pane never counts toward
  `observation=synced`.
- A pane in a non-active window is never a layout focus target
  (`layoutpublisherarbiter`, GH #120). The focus effect reads the candidate
  pane's `#{window_id}`, `#{window_active}` and `#{window_name}` in one tmux
  query. A `stash`/`stash-*` window is refused by name
  (`focus_pane_stashed:...`) whether or not the layout window resolved; a known
  layout window refuses every other window by the drift rule above; and when
  the layout window is unresolved, a pane whose window is not its session's
  active window is refused as
  `focus_pane_window_inactive:<file>:<pane>:live_window=<w>:live_window_name=<n>`,
  because the tmux selection also runs `select-window` and would switch the
  operator's client to a window they did not pick. An unknown activity answer
  is never a refusal.
- A focus request for a document this controller does not own is re-addressed,
  not refused (`#focuscrossroot`). Each project controller answers focus from
  its own actor store, keyed on its own `project_root`. An IDE rooted at the
  superproject publishes one editor surface per IDE process, so selecting a tab
  inside a submodule that carries its own `.agent-doc/` delivers that focus
  intent to the *superproject's* controller, whose store has no row for it. The
  honest local answer there was `missing_actor_record` and nothing focused,
  while the submodule's controller held the live, correct binding and was never
  asked. So `handle_focus_document_pane` resolves the document's own project
  root through `agent_doc_project_root_io::project_root_containing` first: when
  it differs from the controller's root, the request is delegated to that
  root's controller with its original missing-pane policy intact and its
  receipt is returned verbatim. In particular, `ProvisionForLayout` must remain
  structural across this hop: after a subproject document's tmux pane is killed,
  manual **Sync Tmux Layout** asks the owning controller to create a replacement
  pane; it must not be downgraded to `ObserveOnly`. The receiving controller
  records
  `controller_focus_delegated document=<doc> owner_root=<root> policy=<policy>
  focused=<b> reason=<r>`. The decision is pure
  (`agent_doc_controller::focus_routing::focus_owner`) and terminates: the
  delegated call re-enters it on the owning controller, where the roots now
  match. An unknown document root is never delegated — with no proof of another
  owner, the local answer stands. A delegation that cannot reach the owning
  controller reports `cross_root_controller_unavailable`, never
  `missing_actor_record`: "the owning controller is unreachable" and "this
  controller has no actor for the document" are different faults, and reporting
  the second for the first is what kept this invisible.
- A selection effect uses the document's **bound** pane; a live probe may only
  invalidate that binding, never replace it (`#fpeselectstashpane`). The actor
  row and the durable registry entry are the binding. The live-owner signals are
  recovery inputs derived from history and from whole-server scans — the session
  log's `latest_start_pane` records where the session was last *started*, not
  where it currently lives, and the heuristic resolver additionally walks
  supervisor-pid process trees and every pane's command line. After a reroute,
  resume, or rebind, each of those can still name a superseded pane whose stale
  owner process is alive and parked in `stash`, which is how editor selection
  focused a stash pane for a document whose actor row *and* registry both named
  the correct visible pane. So the bound pane outranks a differing live-owner
  candidate whenever the bound pane's own process tree still runs an agent-doc
  owner session for that document, and the resolver is not consulted at all in
  that case. Only a binding that has stopped being true falls through to
  live-owner repair, and a `Blocked`/`Closed` actor projection is never revived
  by this precedence. The proof must be a question about the one named bound
  pane; a scan that can return some *other* pane is a resolution, not a guard.
- Controller focus surfaces a proven live-owner pane before selecting it. The
  promotion and `select-pane` are one editor focus-fenced effect, and the
  controller re-reads `#{window_id}` after promotion before reporting success.
  A superseded focus generation, inactive desktop editor, failed move, or pane
  that remains outside the active `agent-doc` window must not select inside
  stash. The editor's full retained surface still follows as structural
  authority and reconciles the exact visible column set; the focus effect owns
  immediate recovery of the selected pane, not full stash consolidation.
- Standalone non-blocking `run_with_tmux` helpers retain deferred stash promotion
  (`#jb-nav-3pane-promote-swap`) for layout internals and explicit-pane escape
  hatches. Their caller already owns the following structural reconcile, so
  they must not race it with an additive promotion.
- Blocking focus (`#stash-pane-promote-on-focus`): `agent-doc focus <FILE>
  --blocking` (alias `--synchronous`) preserves the previous standalone focus
  behavior for manual/operator flows that explicitly want to surface a stashed
  pane before the command returns. Once blocking focus has resolved the pane, if
  that pane is parked in a `stash` window it best-effort reparents the pane into
  the session's `agent-doc` window before selection, not merely selecting it in
  place inside the stash. Promotion is single-pane scoped
  (`sync::promote_pane_to_agent_doc_window`): a failed or declined move is
  logged, focus re-observes the pane, and while it is still in a `stash` window
  focus refuses to select it (`layoutpublisherarbiter`, GH #120) — a pane in a
  non-active window is never a focus target, and `select-pane` there would also
  `select-window` the operator onto the invisible stash. It does not run full
  stash consolidation.
  Keep this aligned in `focus.rs`, `sync.rs`, and this spec.
- Editor automatic tab-to-pane sync must not use `focus` as a substitute for
  the retained exact-visible tmux layout effect state; beyond the selected-pane
  recovery above, focus does not own full stash consolidation, safe passive
  replacement, or preserved-layout reselect proof.

## layout

`agent-doc layout <FILE>... [--split h|v] [--window W]`

- Mirrors editor split layout in tmux by rejoining the wanted panes into a target window and preserving non-session panes.

## reliable-sync-status

`agent-doc reliable-sync-status [--json] [--check]`

- Reports the reliable-sync plane, the controller's editor-surface auto-sync state (last surface observation, last pane-layout outcome), the tmux pane placement of every registered document, and the surface/pane divergence verdict (GH #62). `--check` exits non-zero only on a divergence finding computed against an accepted surface observation.
- **No observation is reported, not silenced (GH #72).** Without an accepted surface observation the layout effect synthesises its expectation from focus alone, so a `converged_focus_only` outcome is scoped to focus and cannot vouch for the layout. The status must then (a) label the layout outcome `expectation synthesised from focus` with the count of registered documents absent from that expectation, (b) replace the bare `not evaluated` divergence line with the placement that is decidable without the editor — registered documents counted in `agent-doc`, in `stash`, elsewhere, and without a pane — plus an `UNVERIFIED` line when any are parked in stash, and (c) name whether each live JetBrains plugin build predates the surface publisher (first shipped in 0.2.334) or ships it, which separates "restart the editor" from "the editor→controller path is silent". `--json` carries the same facts as `unobserved_surface_gap` (`null` when an accepted observation exists). The gap is advisory: with no observation, whether a stashed document *should* be visible is undecidable, so it does not fail `--check`.

## resync

`agent-doc resync [FILE] [--fix]`

- Prunes dead panes from the registry, reaps idle stash windows, and reports orphaned windows.
- In scoped mode, it must limit repair to the target document and fail closed if ownership is still ambiguous.
- `--fix` may deregister wrong-process panes, move wrong-window panes into stash, and either kill or relocate wrong-session panes, but only when no stronger live-owner proof keeps the current pane authoritative.
- Stash cleanup must preserve recoverable agent panes that still prove ownership of a registered document or still host a live supervisor socket.
- Resync repairs registry/liveness state; it does not infer editor visibility from
  the registry and must never bulk-promote registered stash panes. A live
  `InStash` owner remains reactively parked until the retained pane-layout
  projection requests that document.
- The editor **Resync / Fix Sessions** action is one operator command with two
  ordered boundaries: registry cleanup completes first, then the editor
  publishes its current exact-visible columns with `caller_kind=resync`.
  That distinct retained generation is the sole visible-layout writer.
  Repeating resync with two editor panes must preserve exactly two visible tmux
  panes; unrelated registered documents remain in stash.

## fix

`agent-doc fix [FILE] [--session <name>]`

- Canonical repair surface; document-scoped form shares the `resync --fix` implementation but limits mutations to the target document.
- When a `FILE` is given, `fix` first finishes any unfinished agent-doc turn on that document (`#jb-fix-document-finish-turn`): it runs the deterministic repair path until `session-check` is clean or no further progress is made — recovery can need a second pass (commit a stranded response, then persist the reap) — and only then reconciles tmux routing. Best-effort: a document that stays interrupted falls through to the routing fix with a warning rather than aborting. The JB `Fix Document` action wraps this command, so it finishes the turn without leaving the editor.

## sync

`agent-doc sync [--col <FILES>,...] [--window W] [--focus FILE] [--no-autostart] [--exact-visible]`

- Declaratively mirrors editor layout into tmux columns.
- Standalone CLI sync requires at least one explicit `--col`. It must fail
  closed when columns are omitted instead of replaying a scope-local
  `state.db` row that may describe a different nested project or older editor
  projection. Editor actions capture and publish their complete current
  projection through the controller.
- Files with session ids are managed even when their current registry entry was pruned; `claim` is the only command that creates a new session id.
- A renamed managed file keeps its existing live pane. The normal editor
  document-path transition rekeys the binding; compatibility sync may recover
  a missed transition only by finding exactly one same-session registry entry,
  confirming its old path is absent, and proving that its pane still owns the
  old path. Existing old paths, duplicate session rows, and unproven panes fail
  closed instead of being rebound or removed.
- Sync must synthesize a per-run tmux-router registry from each visible file's own nearest `.agent-doc` root instead of forcing all files through the caller's current root.
- When `.agent-doc/state.db` has a live authoritative actor row for a visible
document, sync must treat that actor-owned pane as the owner-of-record and keep
tmux-router registry metadata only as a projection of that binding.
- An alive pane is not reusable solely because the pane id exists; normal sync may reuse only the live authoritative actor pane returned by the controller for that specific document.
- A Project Controller-owned exact-visible passive sync must treat its local authoritative SQLite actor row as resolved ownership proof. This lets an already-running hidden pane replace the stale same-side pane on editor tab selection without autostarting anything; standalone passive CLI sync must still avoid a nested actor-binding RPC.
- When ownership falls back to legacy associated-pane evidence (`session-log`, `registry_rebind`, generic same-file process tree), sync must fail closed and require explicit claim/repair instead of choosing a winner automatically.
- When ownership proof weakens but the alive pane still contains protected Codex drafted input or still appears as the newest open pane in the session log, sync must fail closed for that file instead of fabricating `registered_pane_missing`.
- If two visible files point at the same pane, sync must either find one decisive owner or drop the duplicate from the synthetic registry so tmux-router cannot alias both files onto one pane.
- Once a live pane is reserved for one file during the pass, later files in the same pass must treat it as unavailable.
- The auto-start pre-sync pass must make at most one pane decision per document. The same document may appear in more than one requested column (column memory, focus + column overlap, repeated layout requests), but its candidate set is deduped by path (first-seen order) before resolution. Without this, a document requested twice can cold-start a second pane on its second occurrence — before the first freshly-provisioned pane has recorded a registry / session-log binding that the second occurrence's lookup or `find_associated_panes` could see — producing the duplicate-editor-pane regression ("3 tmux panes with 2 editor panes"). This complements `find_associated_panes` (which dedups against already-discoverable live panes) by closing the same-run, not-yet-discoverable window.
- Route-owned auto-start under Project Controller authority must publish a
provisional `Starting` actor for the new pane before releasing document/session
startup admission and before sending the child start command. A concurrent
layout generation must reuse that actor binding while process-tree ownership is
still materializing; an alive registry pane without either proof remains
non-authoritative. Standalone startup retains its no-controller path.
- Route-owned startup hands the controller actor generation to the child it
launches (`#claimpanecasrace`). The provisioner's binding write (registry
projection plus the actor binding) and, under Project Controller authority, its
provisional `Starting` actor happen under the startup locks and before the
launch. After the launch the provisioner never writes the actor or the binding
again: the child's `start_session` owns every later generation advance, and a
provision (`skip_wait`) returns the pane it bound before the launch. A
post-launch provisioner write races that advance: it either overwrites the
child's `Starting` record as `Ready` or, when the child commits inside the
write's read-then-store window, fails the provision (an occupied-pane `claim`)
with `controller actor generation compare-and-swap failed: expected N, found
N+1`. Scoped test processes can set
`AGENT_DOC_TEST_ROUTE_START_POST_LAUNCH_BARRIER` to hold the provisioner after
the launch until the child has committed, which makes any such write
deterministic.
- The waiting route path and every route path that restarts a supervisor bind
through the fenced post-launch bind (`#dispatchrebindaudit`): the fresh-pane
re-validation after the ready wait (`resolve_fresh_dispatch_target_after_ready_wait`),
the post-restart rebinds in `launch_contract.rs` and `pane_resolution.rs`, and the
restart-handoff adoption in `restart_handoff.rs`. The owner (the launched child
or the restarted supervisor) holds the actor generation from its launch on, so
the route never writes over the owner's record. When the authoritative actor
already binds the session to the pane, nothing is written and the owner's state
(`Starting`, `Busy`, or a `Closed` it wrote) stands. When a projection write (a
`route` or `sync` dispatch bind; sync's supervisor-backed `recover_owner` is not
one) moved the binding off a pane the route created, the route reclaims the
pane, compare-and-swapped on the generation and the record revision it decided
against: a commit in between, such as a supervisor restart's `start_session` or
a same-generation owner transition like the supervisor going `Busy`
(`#syncbindingfence`), fails the write and the route re-decides against the
newer record instead of re-projecting it as `Ready`. When an owner (`start`, `supervisor`) or an operator (`claim`,
`admin`, `session`) committed the current generation for another live pane,
the route follows that pane and writes nothing; a gone owner pane fails the
route closed. Pre-launch provisioning binds keep the plain
`route`/`dispatch_bind` projection.
- A pre-dispatch rebind of an already-running pane binds through
`bind_running_dispatch_target` (`#predispatchreadyoverwrite`): the
associated-pane rebind, the registered-pane and lazy-claim rebinds, the stash
rescue, and the busy-pane optimistic dispatch in `pane_resolution.rs`; the
stranded-trigger resubmits and the pre-send binds in `dispatch_only.rs`; and the
authoritative-actor dispatch in `authoritative_dispatch.rs`. The route never
writes `Ready` over a live owner. When the actor record already binds the
session to the pane in a live state (`Starting`, `Ready`, `Busy`,
`WaitingInput`), no actor record is written: the owner's state, generation, and
last transition (including a readiness-proving reason such as `prompt_ready`)
stand, and only the registry metadata is refreshed. When an owner (any writer
other than a `route`/`sync` projection) holds the document in a non-`Closed`
state on another live pane, the route writes nothing and fails closed, because
binding the new pane would advance the generation and retire that owner's
supervisor. Otherwise (no record, a `Closed`/`Blocked` record on the pane, a
projection binding, or an owner whose pane is gone) the route binds `Ready`,
compare-and-swapped on the generation and record revision it decided against;
an owner commit in between, including a same-generation transition such as the
supervisor going `Busy` (`#syncbindingfence`), fails the write and the route
re-decides against the newer record.
Dispatch admission needs only the binding: the authoritative route reads the
supervisor's runtime state, which overrides a live record state, and the
controller's `authorize_dispatch` checks the record's pane, session, and
generation. The controller does read the record's lifecycle state at
`authorize_dispatch` (an `admin queue drain` blocks a non-`Ready` actor with
`actor_busy_draining`), so a `Ready` written there would have waved a live
`Busy` turn through that gate.
A stale `Busy` the route proves idle from pane evidence (the ready-prompt,
timeout-idle, and eager two-capture repairs in `authoritative_dispatch.rs`)
stays an in-memory promotion: the route writes no lifecycle state, and the
controller's drain gate takes no route-supplied idle evidence
(`#drainbusymask`). The route's evidence is weaker than the owner's: it does not
consult the harness turn lease or the 15s post-dispatch grace, and the ready-prompt
repair is a single capture, which a live turn can satisfy between tool calls. A
route `Ready` would also release the turn's dispatch receipts
(`#queuedclearflake`) under a supervisor that still holds the actor `Busy`.
The owner heals the record instead: the supervisor's stale-busy self-heal
writes `idle_pane_reconcile` for every busy entry and retries an unlanded write
(see [Supervisor](supervisor.md)), so a drain blocked on a stale `Busy`
(`actor_busy_draining`) is a bounded delay that the caller's backoff retries.
- The route's own lifecycle writes are fenced on the record they decided
against (`#bindingcallersaudit`). A ready prompt in the pane and a ready-wait
timeout are projection evidence, so `promote_starting_authoritative_actor_if_dispatch_ready`
(`Starting` to `Ready`), `recover_starting_timeout_blocked_actor_if_dispatch_ready`
(starting-timeout `Blocked` to `Ready`), and `mark_starting_actor_timeout_blocked`
(`Starting` to `Blocked`) go through `mark_route_lifecycle_fenced`: it re-reads
the record with its revision, writes nothing when the session, pane,
generation, state, or last transition changed since the decision, and
otherwise compare-and-swaps the transition on that revision. An owner commit
landing in between (the supervisor's `Busy` turn start, its own `Ready`, a
`Closed`) wins, the route logs
`route_authoritative_actor_lifecycle_superseded`, and it keeps its earlier
target (promotion), follows an owner that left the timed-out `Blocked` on the
same binding (recovery), or leaves the owner's state alone (timeout block).
- Sync never writes an actor binding it decided from an earlier read without a
fence on that read (`#syncbindingfence`). Every sync/resync actor write decides
from one read of the record and compare-and-swaps on its generation and
revision (see the same-generation lifecycle invariant in
[Single-process control plane](08b-single-process-control-plane.md)):
  - The registry refresh for an authoritative actor (`refresh_durable_registry_for_actor_record`)
    writes the durable registry only, never the actor; an actor that no longer
    binds the session to the pane is left to its owner.
  - A recovered-owner re-registration (`reregister_recovered_owner`: resync's
    target-document recovery, sync's rename reuse and associated-pane recovery)
    goes through `rebind_recovered_owner_in`. A record that already binds the
    session to the pane, not `Closed`, is left alone and only the registry is
    refreshed, so the owner's `Busy`/`Starting` is never re-projected as
    `Ready`. Otherwise the pane is bound (sync's supervisor-backed
    `recover_owner` when a live supervisor is proven in the pane, else a
    `route`/`dispatch_bind` projection), fenced on the record read. After a lost
    compare-and-swap an owner or operator commit for another live pane wins and
    nothing is written; a projection write is re-decided.
  - Both of the above resolve the document's actor store from the document,
    never from the sync process cwd (`#syncnorootfallback`). A document with no
    `.agent-doc` ancestor uses its canonical parent directory's store, the
    store `start` (`canonical.parent()`) and the route
    (`project_root_or_file_parent`) bind it in. Until that store exists the
    document has no actor record and no registry entry: the registry refresh
    writes nothing and never creates the store, and a recovered-owner
    re-registration binds through the same fenced `rebind_recovered_owner_in`
    in that store (creating it, as `start` would). The fallback this replaced
    called `sessions::register`, which re-projected an unfenced `Ready` record
    into the cwd project's store (creating `<cwd>/.agent-doc/` if absent and
    evicting that project's registry entries on the same pane) even for a
    registry-only refresh, where no reader of the document ever looks.
  - The pane-window correction (`#panewindowbindingrebind`, doctor and the
    post-layout pass) keeps the observed pane, session, generation, and state
    and rewrites only the window (`correct_binding_window_fenced_in`). A record
    that changed after the read is left for the next pass.
- If a registered pane is stashed, sync must rescue it back into the visible `agent-doc` window rather than treating the stash copy as disposable.
- An active actor pane outside the exact visible editor projection is
  detachable, not terminal layout ownership. Reconciliation moves it to
`stash` without killing or interrupting its harness and remains retryable
until the requested pane count and order converge.
- A successfully applied structural effect remains controller-ownership
provenance even when a newer desired generation supersedes it before terminal
observation. Exact-visible reconciliation must not reclassify a pane placed by
that effect as operator-owned merely because its actor is absent from the newer
projection; it must reconcile or stash that pane. Truly operator-placed panes
without structural provenance retain the operator-owned protection.
- Protected outgoing on a 1-in/1-out reconcile preserves layout (`#jb-nav-3pane-promote-swap`): when the reconciler's SWAP fast path detects exactly one pane to attach and one to detach, and the outgoing pane is protected (busy / `protect_pane`), it must NOT fall through to ATTACH (join the incoming) while DETACH skips the protected outgoing pane — that grows the window to N+1 panes. Instead it preserves the current layout, leaves the incoming pane in its stash, and defers; the caller's deferred-retry resurfaces it once the busy pane frees. Keep this aligned in the `tmux-router` reconciler (`sync.rs`) and this spec.
- Managed target/stash role names must be pinned by disabling
  `automatic-rename` and `allow-rename` before repair resizes or consolidates
  windows; process-title changes must not reclassify stash contents as duplicate
  visible target panes. Already-pinned and unrelated windows retain their options.
- Manual/full sync is also the operator repair surface behind editor
  `Sync Tmux Layout`: before reconciliation it runs file-scoped doctor repair for
  the focused/session document. That closes recoverable `jb_cache_conflict_cancel`
  commit-boundary drift before pane liveness can short-circuit, then runs
  `repair_layout` for the inferred target session so the visible layout is repaired
  to `0:agent-doc`, `1:stash`, and adjacent overflow stash windows before panes are
  reconciled.
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
- A validated explicit `--window W` is the layout target; sync must not replace
  it by re-resolving the session's named window or report that the named
  window is absent. Exact-visible editor layouts may span project roots. For
  each nested-root document, the parent layout effect must consume the owning
  controller's existing live actor binding before document-content
  resolution, move that pane into `W`, and keep the binding ephemeral. Missing
  foreign bindings fail closed: no parent-root registry write, content
  mutation, resume, or pane creation is allowed under `--no-autostart`.
- A successful structural layout receipt is generation-fenced ownership
  evidence. The controller must retain its file-to-pane assignments in a
  Source and join them with live actor state and current desired documents in
  the same binding Computed; an RPC worker must not imperatively append this
  derived evidence to an invocation. A desired-state change invalidates stale
  receipt assignments for structural reuse before the next effect runs. It
  does not erase ownership: a separate process-scoped Computed joins retained
  physical assignment receipts with current actor bindings, including outgoing
  documents and actors with nonempty session IDs. Layout drift invalidates
  geometry reuse only; actor rebinding retires obsolete ownership proof.
- When that retained binding projection misses for a nested-root document,
  interactive sync may make exactly one read-only request to an already-running
  owning controller. The request has a 250ms deadline, must not launch or retry
  the controller, and emits `cross_root_binding_deadline_exceeded` before
  falling through fail-closed. The same file must not repeat that request after
  document-content resolution.
- Post-sync registry updates must fail closed if tmux-router reports a
  geometry-only pane assignment that disagrees with a still-live authoritative
  actor pane for that document.
- After structural reconciliation moves or provisions a pane, sync must publish
  the pane's live tmux window to both the document actor and durable registry
  before returning convergence. A pre-move window id cannot satisfy later
  editor-to-tmux focus synchronization, even when the pane id is unchanged.
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
- Stale-supervisor admission is atomic for safe-passive exact-visible layout.
  If its gate removes any requested editor column, `sync --no-autostart`
  preserves the current tmux layout rather than reconciling the non-stale
  subset. The recycle-settled generation republishes the complete projection.
  Full/manual sync may still realize a non-empty admitted remainder for
  operator repair; if no requested columns remain, every mode preserves.
- Test coverage for those pure ownership/cardinality decisions should live in
  deterministic `SimWorld` traces. Keep real tmux coverage only for the
  minimal smoke surface that proves pane/window movement, tmux-router detach,
  and focus selection still work against a live server; duplicate manual vs
  passive ownership variants do not both need default-suite tmux coverage after
  matching simulator traces exist.
- Sync serialization is bounded: a contended `.agent-doc/sync.lock` may delay a
  later editor-triggered sync only up to the lock wait budget, after which the
  later sync logs the contention and continues rather than starving selection,
  ownership proof, or auto-start handling behind a prior process. Sync latency
  logs must include the `sync_lock_wait` phase so contention is not hidden
  inside the safe-passive total. Safe-passive contention diagnostics must
  classify redundant editor selection churn with `coalesced=skipped_stale` while
  preserving the retry marker consumed by editor plugins. Acquisition blocks in
  the kernel on `flock(2)` with no userspace polling; the lock is released
  automatically when the holding process exits (fd close), so a dead or
  orphaned owner can never wedge it and no `/proc` orphan reap is required.
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
  and post-router registry transaction must share a per-cycle proof cache
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
  applied state. Focus-only observations cannot resume missing sessions or
  promote stashed panes. Structural layout intent owns provisioning; its
  cross-project effect asks the owning controller for `provision_for_layout`,
  which returns a live binding without selecting or promoting it. A
  `sync --no-autostart` pass never requests missing-owner provisioning.
- Safe-passive editor sync should not prove whether live unregistered agent
  panes in stash are still owned. Live agent-pane ownership proof and
  kill-or-preserve decisions belong to full sync/repair paths.
- Safe-passive focus-only editor sync must not turn a visible split into a
  one-pane `agent-doc` window. If the editor event provides only the focused
markdown file, sync must expand that one-column projection from the project
`state.db` layout row; when no saved layout exists, it must derive the sibling
  projection from the registered panes already visible in the target
  `agent-doc` window. If the focused file is already in the remembered or
  visible projection, sync must select that column rather than replacing the
  currently active tmux pane; active-pane inference is only for true same-side
  replacements.
- Automatic editor plugins that have captured the full visible markdown
  projection must publish it as exact-visible structural intent. The controller
  may run that intent with full, safe provisioning so missing owners can be
  staged, but the projection remains atomic: a single provided `--col` is
  authoritative and must not expand from remembered layout state, and a blocked
  legacy owner must preserve the current tmux layout rather than collapsing the
  window to the resolved subset. Switching the editor away from a document like
  `tasks/software/corky.md` therefore cannot reintroduce its stale pane as a
  sibling.
- A focus-only editor projection never derives columns. When its exact effect
  receipt says the selected document has no usable visible owner—because the
  pane is stashed, dead, reaped, or its actor/registry binding is missing or no
  longer focusable—the editor adapter must generation-fence that receipt and
  republish its complete current surface as a forced structural edge. The layout
  graph may then provision only the documents required by that authoritative
  surface and reconcile cardinality; the narrow focus projection must never
  create a pane itself. Other focus refusals remain non-structural failures.
- Ordinary sync/preflight/finalize recovery paths must never kill a tmux pane. When sync observes a dead pane during missing-pane repair, it may capture diagnostics and keep the dead pane retained for manual inspection, but only explicit repair surfaces such as `fix` / `resync --fix` may escalate to pane-kill cleanup.
- Automatic stash and `resync --fix` orphan-agent cleanup must preserve live agent processes even when registry or supervisor observations are absent during a handoff. Missing ownership metadata is not exit proof. Orphan-agent reaping requires a retained-dead pane; existing dead-pane and idle-shell cleanup remains available.
- Recent repeated `missing_pane` recoveries, unresolved startup-miss state, or a `registry_rebind` closeout whose recorded successor pane is still alive and rooted to the same document all block passive `--no-autostart` cold-start.
- If any visible file stays blocked under passive `--no-autostart`, or under an
  autonomous exact-visible projection that lacks explicit repair/routing
  authority, sync must preserve the current visible tmux layout and warn instead
  of reconciling the remaining foreign pane set into a new authoritative layout.
  Explicit manual/projection full sync retains its repair behavior and may
  realize the resolved remainder. This includes the live mixed-root replay shape
  where `tasks/agent-doc/agent-doc-bugs2.md` shares the visible `agent-doc`
  window with `src/session-share/tasks/claudescore-3.md`; a blocked sibling file
  must not let the remaining visible pane set collapse into a new authoritative
  layout.
- A preserve-layout return that successfully reselects the requested already-visible focus pane must print `[sync] safe_passive_layout_preserved_reselected_focus ...` to command output, not only to `/tmp/agent-doc-sync.log`, so editor plugins can mark that focus handoff as applied.

### Sync-specific invariants

- `provision_pane` is the passive sync-specific pane-creation path. It stages a new owner off-screen without blocking on prompt readiness; the current layout determines column placement.
- Queue control fences autonomous route dispatch, not structural editor-layout ownership. A binary-owned provision-only sync may create and start an idle keep-alive owner while queue control is paused or draining, but it must preserve a typed `layout-provision` purpose through both route admission and the spawned `agent-doc start` lifecycle check. The initial sync sends no reopen, and the supervisor's queue watcher must continue honoring queue control before any later dispatch. A fresh layout-provision supervisor must wait one complete orphan-observation interval before it may reap a stashed pane from an inherited terminal cycle, giving the newly resumed child time to admit its prompt. That destructive orphan decision must inspect live child output directly and must not trust a stale controller `Ready` state over an active harness turn. An unchanged desired tmux target must settle after the pane becomes observable instead of retrying the blocked allocation on a timer.
- Full/manual sync uses the strict provision-and-route variant: it resolves and creates the pane in the intended tmux session, registers it, waits for harness dispatch readiness, and submits the document route as one fallible controller command. A readiness or submit failure must keep the terminal command non-applied and expose its diagnostic to the caller.
- For a cross-root Sync Tmux Layout whose exact editor-visible set contains two documents, a `Ready` actor row for the second document is not reusable merely because it still names a pane. If that pane is dead or its current process tree does not prove ownership of the document, `ProvisionForLayout` must treat the binding as missing, provision a replacement through the document-owning controller, replace the stale binding, and allow the main-window owner to converge to exactly two panes. `ObserveOnly` must neither focus nor reuse the ownership-unproven pane and must not provision or otherwise mutate the layout.
- A paused queue reduces manual layout startup to provision-only; it never resumes the queue to make geometry converge. Run Agent Doc publishes an `editor_route` layout intent that awaits exact layout convergence but does not dispatch newly created sibling panes. Only the selected document crosses the subsequent route boundary.
- With an existing visible `agent-doc` window, layout-owned startup creates a detached staging pane and leaves visible geometry and selection unchanged. Only the current layout Effect may attach the returned pane. A startup that finishes after its layout was superseded stays off-screen, preserving the exact editor-visible pane count. First-window bootstrap retains its existing behavior.
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
- `repair_layout` converges, and reports whether it converged
  (`#syncdoctorconverged`). It returns `AlreadyConverged` when one window
  observation proves the always-run consolidation and index-normalization phases
  would move nothing, and `Repaired` only when it issued mutating tmux commands.
  Callers report a repair note only for `Repaired`; an unconditional note made
  the sync log claim a repair on every sync, which is indistinguishable from a
  repair that never converges. The converged path must issue **no** tmux command
  beyond that single window listing: full sync runs this pass twice per
  invocation (pre- and post-reconcile), and the always-run phases otherwise
  re-list the session's windows once per `normalize_window_to_index` call, so an
  editor tab switch paid roughly a dozen redundant tmux round-trips per gesture
  to rediscover a layout that was already correct.
- Owned-window size pins (`#stashresizerestore`, GH #244). Only the
  agent-doc-owned windows of the repaired session (`agent-doc` and `stash*`)
  are ever re-fit; an operator's own windows keep whatever size and
  `window-size` option they have (`#bare-foreign-session-guard`). Two pins are
  released, each with `resize-window -A` followed by `set-option -w -u
  window-size`, so the window follows its client again under the global or
  session policy:
  - the exact 1000-row stash-join staging height left by older binaries
    (`layout_repair_unpinned_window`);
  - a per-window `window-size manual` that holds the window at a size **no**
    client attached to the session sees it at, measured against each client's
    size less its status rows (`layout_repair_window_size_pin_released`). tmux's
    own `resize-window -A`, commonly bound to a "fit window" key, sets that pin
    and leaves it, so once the clients change the window stays at the old size:
    dot-padded in a larger client, cropped in a smaller one. With two
    differently sized clients attached, `-A` takes the larger width and the
    larger height independently, so the pin can be too wide for one client and
    too tall for the other at once; it is released in that case, and it stays
    releasable after either client detaches (GH #268 — the #244 guard released
    only a pin smaller than every client, so a cropping pin stuck for good).
    The pin is kept when it is only inherited (a global/session `manual`
    policy), when no client is attached, when any client sees the window at
    exactly its pinned size, or when any client's size is unreadable (tmux
    reports no height for a control-mode client).
  A repair that releases a pin returns `Repaired`; a pinned window that is
  still undersized is never `AlreadyConverged`. The check costs nothing on the
  converged path: the single window listing carries each window's width and
  effective `window-size`, and the local option and client list are probed only
  for an owned window whose effective policy is `manual`.
- Client geometry record (GH #83, GH #268). The controller logs
  `tmux_client_geometry` for the layout window whenever the attached clients'
  sizes, the effective `window-size`, or the window size change, and on every
  pass while two or more clients differ by more than 10 cells in width or
  height. A mismatched record carries
  `warning=clients_of_different_sizes_attached` and a `remedy=` that matches
  the state: `set_window-size_largest` is offered only when the clients agree
  in one dimension (`largest` takes width and height independently, so clients
  differing in both are each cropped by the other's larger dimension), leaving
  `detach_the_extra_client` alone otherwise; under an effective `manual`
  policy the remedy names the pin first
  (`set-option_-wu_window-size_then_...`), because neither detaching a client
  nor a global `largest` moves a pinned window.
- Both doctor-repair passes are metered as `doctor_repair_pre` and
  `doctor_repair_post` in the sync phase log. They previously sat between
  measured phases and were invisible to the phase meter, which is how that cost
  hid on the editor hot path.

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

- Every actor command that accepts `FILE` must resolve both document content and
  `agent_doc_session` through the realtime read-authority boundary. When an open
  editor's converged CRDT projection is newer than disk, the command uses that
  projection; it must not report a missing session from a stale disk snapshot.
- `agent-doc session status <FILE>` prints the authoritative actor record,
  registry projection, supervisor runtime state, startup-miss marker, and
  latest session-log summary for the document. It must fetch the actor record,
  supervisor lease, recent operator/dispatch attempt, and projection drift
  diagnostics from the project controller rather than treating
  diagnostic files as authority. It must also query tmux directly and
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
command into the authoritative session — the same live harness process, never
a replacement child, whenever a responsive composer can take it
(`#idleclearrestart`; restarting killed the harness's background agents and
shells with no report) — through the same canonical
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
  disabled` (`#codex-capability-proof-unrecoverable`). At an idle `ready`
  prompt the supervisor types the clear into the live child and the client
  logs `session_clear_sent ... delivery=supervisor_ipc` and runs the same
  submit verification as any typed clear. The supervisor answers
  `restart_fresh=true` (logged `delivery=supervisor_restart_fresh`) only when no
  live child exists, the actor is `waiting_input` (keepalive or permission
  prompt), or the harness's clear exits the process (Codex,
  `#clear-ready-restart`); see `specs/supervisor.md`. A fresh restart over an
  open cycle is applied at once only when the actor is `Ready` and the cycle's
  response is durably captured (`#clearcapturedcycle`), and the relaunched
  child then waits at its prompt instead of re-running the document over that
  supervisor-owned closeout (`#clearsessioncontext`); otherwise the
  supervisor latches it until the cycle closes, the client logs
  `session_clear_deferred ... deferred_until=agent_doc_cycle_closes`, and its
  output says the clear is accepted but NOT applied yet instead of "Cleared"
  (an empty preflight abandoned by the clear's own reclaim releases the latch
  and reports as cleared). For Codex, it still
  records the clear prompt state so the next reroute can reapply the original
  launch contract. Before contacting tmux or the supervisor, it must record a
  controller operator-command acceptance or fail with the rejected stage. A
  controller generation in a non-stable handoff state must refuse that
  authorization without writing an acceptance receipt; the client waits for
  the promoted generation and retries before any `/clear` delivery
  (`#clearunobservedrecycle`). Clear
  is a session-context operation only: it must not rewrite the session markdown,
save a snapshot, or delete live text below an `agent:boundary` marker.
Clear delivery is successful only after observable command consumption, a pane
content transition, or proof that the pane is already in the cleared state. If
the command remains visible in a split-submit composer, recovery may retry only
the harness submit key. If delivery is unobserved and retained conversation
history proves the clear did not take effect, recovery may resend the full
idempotent clear command once within the same bounded retry budget. Capture
failure is not resend authority, and an accepted or already-cleared observation
must never receive another command. A recognized harness startup banner without
a dispatch-ready prompt is the distinct non-accepted `pane_restarting` state;
its only recovery is to wait for harness startup to finish, and it must never
authorize a full-command resend (`#clearsubmitpanesettle`).
Proof that the pane is already in the cleared state is read from the LATEST
chat in the pane, never from the length of the whole tmux scrollback: both
Codex and Claude Code keep the old conversation in tmux history after
`/clear`, so a whole-history line bound can only pass on a pane shorter than
a screen (`#clearfreshchatproof`). Codex proves a fresh chat by its
`Context 0% used` footer or by the tail after its last `>_ OpenAI Codex`
banner. Claude Code proves it by the tail after its last startup banner
(`#claudeclearfreshchat`): `/clear` repaints the banner, echoes the command
as the new conversation's first prompt, and draws an empty composer. That
tail passes only when it is short, carries the command echo, holds no
submitted prompt, operator draft, assistant/tool bullet, tool result,
spinner, or any other row that is not composer chrome after the echo, and
ends at a dispatch-ready composer; a command still sitting in the composer
fails closed first. Rows below a CLOSED composer frame (a rule, one `❯`
composer row, a rule) are the harness footer and are classified by position,
not by string (`#claudeclearfooter`, gh-271): Claude Code draws the
permission-mode row, the built-in or an operator-configured `statusLine`, and
transient notices such as `✔ Update installed · Restart to update` there,
while the conversation always renders above the frame, so no per-string
chrome rule is needed for them. The footer still never excuses a submitted
prompt or composer row, an assistant/tool bullet, a tool result, or a
spinner; the same text above or inside the frame, directly under the echo, or
below a frame whose composer holds a draft stays unknown output and fails
closed. A clear that did not run cannot pass, because a lost
clear paints no new banner and the tail after the last one holds the
retained conversation. The same proofs resolve a blank-viewport
(`pane_not_rendered`) window from scrollback for both harnesses. The
`harness_queued_input` label (`#clearqueuedcomposer`) is read from the ACTIVE
composer only — the bottom-most row that is neither harness chrome nor
composer-frame footer (`#claudeclearfooter`) — because
Claude Code repaints its composer frame into scrollback and stale
`❯ Press up to edit queued messages` rows from ended turns are not the pane's
state. The probe dispatches on the resolved `HarnessConfig` binary, not on
the raw harness label, and the session commands resolve that label with
`default` treated as unresolved at every layer: explicit document `agent:`,
then the actor record, then the harness the live supervisor reports in its
`State` reply (`current_harness`, normalized `claude` -> `claude-code`), then
ambient detection. A clear probe on a Claude Code pane must therefore never
run, or be reported, as `harness=default` while a live supervisor names the
harness (gh-266).
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
  authoritative actor closed when possible, removes the document's registry row
  in the same controller transaction, writes the clear cooldown, signals the
  supervisor/child PIDs when known, kills the bound tmux pane when it is still
  alive, removes the supervisor socket, and reclaims any empty orphaned
  preflight cycle. This force path is intentionally separate from ordinary
  `session clear` and normal `interrupt-clear`, and its ops-log summary must
  report which cleanup steps actually ran.
  Stale-binary freshness is not a wedged-owner discard: stale-supervisor
  warnings and compaction recovery hints must use `agent-doc admin recycle` or
  normal `agent-doc session restart-supervisor <FILE>`. A healthy live
  supervisor accepts that request as a pending drain-to-boundary handoff: an
  `alive-busy` turn continues uninterrupted, and replacement runs only after
  supervisor IPC is drained and the harness-owned turn lease becomes idle.
  It does not borrow the destructive semantics of this force path.
  For Codex panes, a capture
  that shows only Codex status/footer chrome such as the model/cwd/context line,
  with no prompt input or busy cue, is direct idle evidence for operator
  clear/status; Codex idle placeholder prompts such as
  `› Ask Codex to do anything`, `› Explain this codebase`, and safe generated
  placeholders ending in `in @filename`, `for @filename`, or
  `on my current changes` are also prompt-ready evidence when the pane does not
  show an active `Working (... esc to interrupt)` cue. These idle forms must
  override stale actor/supervisor busy projection and may prove route
  dispatch-ready state when no protected prompt input or busy cue is visible. A
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
  default harnesses send normalized text plus a named `Enter` key in one tmux
  `send-keys -t <pane> <text> Enter` operation. These
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

Multi-session projects (`tmux_sessions` non-empty, GH #17) insert two
authorities and an allow-list gate into this order; see
[Session Routing](08-session-routing.md#multi-session-projects-ghmultitmux).
With `tmux_sessions` empty the order above is unchanged.

Current tmux session resolution must target the caller's `TMUX_PANE` owner pane
when it is available. Bare `display-message -p "#{session_name}"` may follow
another attached client's selected session and must only be used as a fallback.
Windowless sync must prefer the live project pin over the caller pane's tmux session.

## tmux-spawn scope stewardship

- GNU Make Rust-test entry points (`check`, `check-fast`, `test`, `tmux-ci`,
  `sim-medium`, `sim-net`, and `cross-editor-simworld`) export an unreachable
  `DBUS_SESSION_BUS_ADDRESS` target-specifically. The value reaches test
  processes and the private tmux servers they create, preventing systemd-backed
  tmux from creating a transient scope per pane. Install and release targets do
  not inherit the override.
- A non-temporary project controller may trigger the binary-owned sweep from its
  idle maintenance boundary. A per-user locked timestamp limits all controllers
  to one sweep per configured interval; inability to read or advance the stamp
  is logged as an unavailable sweep rather than silently disabling maintenance.
- The sweep acts only on names matching `tmux-spawn-*.scope`. It resets failed
  units because reset-failed does not signal a process. It keeps a scope whose
  launching tmux server and pane process are both alive.
- A running orphan may be stopped only when its cgroup is empty, or every member
  is proven by `/proc/<pid>/exe` to be the `agent-doc` executable and every
  relevant path is lexically contained by a `tempfile` `.tmp*` fixture root
  that is gone or old enough. `argv[0]` is not ownership evidence, parent-path
  traversal is not fixture containment, and unreadable or foreign processes are
  reported without signalling them.
- `agent-doc admin tmux-scopes [--dry-run] [--json]` runs the same classifier on
  demand. Scheduled and admin runs emit bounded `tmux_scope_sweep` detail plus
  aggregate counts to the project ops log when a project log is available.

## controller

`agent-doc controller status [--project-root ROOT] [--ensure]` reports the
project-local controller bootstrap as JSON. Without `--ensure`, status is
read-only: it attempts the controller socket and, if inactive, reports the
last persisted bootstrap row from `.agent-doc/state.db` without
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
- bootstrap claim: a process-lifetime namespaced local-socket endpoint derived
  from the canonical project root
- bootstrap state: `.agent-doc/state.db` `controller_bootstrap` row

On Linux, where the claim lives in the abstract socket namespace, releasing it
frees its name only when the last descriptor for the endpoint closes (other
Unix platforms map it to a `/tmp` socket path that the release unlinks). A
child that any thread of the holding process forks while the claim is held
carries a copy until it execs (the endpoint is close-on-exec; there is no
close-on-fork), so a release becomes observable to the next
acquirer only after every such in-flight fork image has exec'd or exited. A
caller that may re-acquire right after another release must use the bounded
blocking acquire; an immediate non-blocking re-acquire is guaranteed only in a
process where nothing else forks. Tests that assert immediate re-acquisition run
in a dedicated child process for that reason.

Lazy startup must connect first, acquire the kernel-owned bootstrap claim only
when needed,
verify that the active controller was started from the same agent-doc binary
identity as the caller, recheck the socket after acquiring the claim, and then
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
and any registry refresh as `projection_refresh`; over-budget
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
and `sync` read actor bindings through the controller. Tmux-router state and
session logs remain diagnostic inputs only.
Destructive or heuristic recovery belongs behind explicit operator repair flows.
