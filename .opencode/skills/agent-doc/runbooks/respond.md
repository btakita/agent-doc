# Respond / pending detail

Detail for SKILL.md Workflow **step 1 (Respond)**, **step 1b (Update pending)**,
and **step 1c (Record operator TODOs)**. The spine in SKILL.md keeps the
reconcile-oldest-first rule, the `### Re:` header + model-attribution format,
and the pending granular-flags-only rule. This runbook carries the rest.

## Step 1 — Respond

- Address the user's changes naturally in the console; that response is the
  document response.
- **Native choice prompts are document-visible (`#choicecheckpoint`).** Before
  invoking a harness-native choice UI (Claude Code `AskUserQuestion`, Codex
  `request_user_input`, or the active harness's equivalent), materialize the
  exact public prompt in `agent:exchange`:

  ```bash
  cat <<'JSON' | agent-doc choice-checkpoint <FILE>
  {
    "context": "The user-visible facts and tradeoff that make this decision necessary.",
    "question": "Which option should the agent use?",
    "choices": [
      {"label": "First (Recommended)", "description": "Effect of the first choice."},
      {"label": "Second", "description": "Effect of the second choice."}
    ]
  }
  JSON
  ```

  The native UI MUST present the same question, labels, descriptions, ordering,
  and recommendation marker. Include an automatically added free-form/Other
  choice in the checkpoint when the harness shows one. `context` is a concise
  operator-facing summary of relevant facts and tradeoffs, not analysis,
  scratch work, hidden chain-of-thought, or tool output. The command accepts
  only single-line bounded fields, 2–5 uniquely labelled choices, and no HTML
  comments; it Markdown-escapes every value before using the existing
  controller/CRDT salient-response path. Therefore the live record is visibly
  assistant-authored, is not a new operator prompt, is not answer evidence, and
  cannot consume queue/backlog work or commit the cycle. If no native choice UI
  exists, write the same checkpoint before yielding the question in the
  harness's ordinary final channel.

  Once the operator answers, retain a compact decision record in the cumulative
  final response: the context, question, choices presented, and selected/free-
  form answer. Final response application removes the non-final checkpoint; the
  decision record preserves what the operator saw and chose. Do not close or
  abandon the response cycle merely because the native UI paused the turn.
- Reconcile the changed exchange tail oldest-first. Do not stop at the newest
  question; answer or group each unresolved prompt in that tail and each
  unresolved `prompt_target`; treat `content_edit` items as user corrections.
- **Chat-originated prompts are session turns (`#chatprompt`, GH #125).** When the
  operator types the prompt into the harness chat (Claude Code, Codex, OpenCode)
  instead of editing the document — including a bare `prompt_presets` key such
  as `#upgrade` — the document has no diff for it by construction. Record it:
  begin the `patch:exchange` response with the prompt verbatim using the exact
  hook-issued record prefix, `> **Chat prompt (#chatprompt:<id>):** <prompt>`
  (multi-line prompts continue on `>` lines; steering never reads that record
  back as operator input). Every prompt gets its own compact surrogate ID; old
  ID-less `(#chatprompt)` records remain valid and are not rewritten. Then do the
  work, and persist through `agent-doc respond <FILE>` /
  `agent-doc write --commit <FILE>` with its queue/backlog mutations, exactly
  like a document-originated turn. This works on a pure chat turn too (no
  `agent-doc <FILE>` trigger, no open cycle): `respond` reopens a fresh cycle
  from HEAD (`cycle ... was already committed; auto-reopened a fresh cycle` is
  that path, not an error). The `UserPromptSubmit` hook marks such a prompt in
  a document-bound session with `[agent-doc] chat prompt for session document
  <FILE>` (plus the resolved `prompt_preset` body) and keeps it in a
  per-session ledger. The next admitted `agent-doc <FILE>` cycle **carries**
  every prompt the document still does not record: the contract lists them in
  `chat_prompts` and `user_intent_prompt_changes`, reports `no_changes: false`
  (`diff_type: "chat_prompt"` when nothing else changed), and is preceded by an
  `[agent-doc] unrecorded chat prompt(s) for this document` notice. Record and
  answer them in that cycle. A closeout that commits without the record makes
  `session-check` warn `chat prompt not recorded (#chatprompt)` with the repair
  (a warning, not a failure): pipe a response that begins with the record line
  through `agent-doc respond <FILE>`.
- **Informational components stay informational (`#notes-not-a-prompt`).** Content
  in `agent:notes` remains available as context when another exchange/queue
  prompt starts the turn, but it is not itself a prompt or completion target.
  Do not answer, strike, clear, or otherwise mutate a note merely because its
  prose looks imperative or ends in a question. Modify notes only when an
  explicit exchange/queue prompt asks for that notes edit. This keeps completed
  queue lifecycle separate from persistent operator-owned context.
- **Realtime steering (`#realtime-steering-verbatim`):** a document is realtime —
  the operator may add a prompt WHILE your turn is running. Every item the
  operator adds must be addressed and worked on, never committed-and-ignored. When
  the operator steers after your response committed, `session-check` (and the
  `respond` / `finalize` / `write --commit` terminal report) prints
  `[session-check] steering pending: …` and exits `0` (`#steerinterruptexit`):
  pending steering is **not** a failed closeout or an interruption, and your prior
  response is already in HEAD. Each item is listed **verbatim** with its
  `dispatch`; handle it per the dispatch rules below — an `address_now` prompt is
  the next cycle's input (`agent-doc <FILE>` admits it; Claude Code re-enters with
  `/loop agent-doc <FILE>`, and the Stop hook holds the final answer once for it
  unless a ready supervisor's steering wake will submit the trigger). Do NOT
  re-run respond/finalize on the prior response, do NOT `--force-disk` (it
  clobbers the operator's live edits), and do NOT re-answer a prompt already
  committed in HEAD. Genuine integrity failures still exit nonzero as
  `INTERRUPTED`.
- **Mid-turn steering (`#midturn-steering`):** steering no longer waits for
  closeout. After every tool call the PostToolUse hook (Claude Code, Codex)
  hands the running turn new operator edits as `[agent-doc] operator steering
  arrived mid-turn`, each item verbatim with a typed `dispatch`. Edits pass the
  typing gate first (`#steeringtypinggate`, shared with preflight admission):
  a line ending in an article or function word (`… publish the`), an
  unbalanced backtick, quote, or bracket, or a dangling `,` `:` `+` is held;
  finished-looking text (terminal punctuation, a closed `[#id]`, a URL)
  settles after half the quiet window (`agent_doc_steering_debounce_ms`,
  default 2500) and anything else after the full window. A hard max-hold
  (`agent_doc_steering_max_hold_ms` in `.agent-doc/config.toml`, default
  45000) releases a stuck item anyway, marked `possibly_partial: true`: act
  only on its unambiguous part and re-read it before relying on its ending.
  Each item is surfaced exactly once; a re-edit before delivery replaces the
  held version.
  - `address_now`: a new exchange prompt, an edit/removal of the queue item
    this turn is executing (shown as `previous` → `verbatim`), or an ordinary
    queue item marked `sent=explicit`. Address it in the current turn: adjust the
    in-progress work, or stop/wrap up for a removal.
  - `drain_after_current`: a passively observed new or edited queue item. It runs
    in operator queue order AFTER the current item closes. Acknowledge it; do
    not interrupt, interleave, or start it now.
  - `forward_to_owner` (`#claimedsteerwake`): a queue item a worker already
    claimed; `owner=subagent:<label>` names it. For an edit, FORWARD
    `previous` -> `verbatim` to that running subagent (for example with
    SendMessage). A nested child that says `push` or `publish` (ignore a
    trailing `⟦effect:N⟧`, a struck child, and a negation such as `do not
    push`) is operator authorization to do that action now
    (`#nestedoutwardauth`): tell the owner it is not a question, not a chat
    reply, and not blocked by a preset or skill that says to wait.
    `publish` on a review posts it as a plain comment unless the line says
    approve. Never dispatch a new subagent for it and never re-claim it:
    the claim follows its line through an in-place edit, whether an appended
    note or a corrected URL or wording (`#claimfollowedit`), and the worker
    can still `--refresh` / `release` by the text it was handed. An edit of a
    claimed head also wakes an idle coordinator. A different row typed over a
    claimed one is not an edit: it arrives as `cancel_claimed` for the old row
    plus ordinary new work for the new one (`#krwr`).
  - `cancel_claimed` (`#queue-removal-cancels`): the operator deleted a queue
    row with a durable active claim. Deletion is authoritative cancellation,
    never a best-effort hint and never permission to continue because substantial
    work is already done. The binary has tombstoned the exact identity and
    `generation=` and retired pending completion delivery; stale replay cannot
    resurrect/integrate it, and a same-text re-add is a fresh generation. If the
    exact `worker=` is still alive, use the harness interrupt primitive at the
    nearest safe consistency boundary. Best judgment
    governs only shutdown mechanics. Preserve the worker's branch, worktree, and
    commits; stop further implementation, tests, integration, publication,
    release, and external issue closure. Then run the rendered generation-fenced
    `agent-doc queue release` command. Report effects that already crossed an
    irreversible boundary; do not try to undo them, close out the removed row, or
    requeue it. The binary reports the owner/worker contract but cannot itself
    kill a harness subagent.
  - `degraded_claim_mismatch` (`#queue-removal-cancels`): a deleted row looked
    struck/terminal in Markdown but durable claim, reported-receipt, or completion
    outbox evidence still owns it. Preserve and fence that work; do not interrupt,
    release, integrate, apply, or publish it. Report the mismatch and wait for an
    explicit authoritative resolution. A struck deletion with no such durable
    evidence is presentation-only cleanup and needs no worker action.
  - `persist_reported` (`#reportedrelease`): a queue item whose worker already
    reported and whose claim you released; `owner=` names that worker. Its
    answer is only in your context. Persist it now with the closeout that
    strikes the line (the `> **Queue prompt:**` echo, or `--done <id>`). Never
    dispatch a new subagent for it and never re-claim it.
  - `sent=explicit` (`#claimedsteerwake`): the operator pressed Run Agent Doc
    (`agent-doc route`), which flushes all pending steering at once past the
    typing gate. Treat those items as final. Explicitly sent ordinary queue
    work is upgraded from `drain_after_current` to `address_now`; claimed-owner
    forwarding and subagent dispatch retain their stronger routing.
  - `subagent`: a queue item with subagent intent (`#subagents` on the line, a
    preset expanding to "run … in subagents", or such a preset at queue or cycle
    scope). Spawn a background subagent for it immediately, one per item; when it
    touches a repo give it its own worktree outside the IDE-watched project and
    never run two subagents against one repo checkout. Keep working the current
    item yourself and record the dispatch in your response.
  OpenCode, Grok Build, Cursor, and other hook-less harnesses poll
  `agent-doc steering <FILE>` (`--json` for the typed form) between
  long-running steps; that poll is their delivery channel, so it advances. To
  inspect steering without consuming it (an operator, a monitor, another
  agent), always use `agent-doc steering --peek <FILE>`.
  After closeout (`#steeringafterclose`) the hook keeps reporting new edits,
  framed as next-cycle queue work, while the same harness turn runs (for
  example while you wait on subagents). When the turn ends and the pane is
  idle, the route-owned supervisor wakes it (`#steeringwake`): settled,
  unsurfaced, unclaimed steering becomes an idle-drain subject and the
  supervisor submits `agent-doc <FILE>` through the same guarded dispatch as a
  queue head (harness prompt-ready proof, never into a busy turn, drain-owner
  lease, convergence gate), once per steering set (a durable receipt fences
  it). The wake reads without consuming, so the woken turn still receives the
  items through preflight, the hook, or the boundary report. Items a worker
  claimed (`agent-doc queue claim`) never wake the session, but an edit of a
  claimed item's text does (`forward_to_owner`) when it is an edit of the
  same row (same tracked id, or similar/contained text); the exact
  owner/generation remains registered. Deleting a claimed row wakes it with
  `cancel_claimed`, including when a different row is added in its place: a
  replacement is new, unclaimed work, never an edit (`#krwr`).
  Steering goes only to the
  turn that owns the document: a subagent's tool calls neither receive nor
  consume it (`#steerowneronly`). Cursor has no
  route-owned pane, so it is never woken; it receives steering at its next
  agent-doc command.
  Steering does not depend on the hook (`#closeout-steering`): whatever no hook
  delivered is appended to the `respond` / `write --commit` terminal output and
  the post-commit `session-check` as `[agent-doc] operator steering arrived
  during this turn and was not yet surfaced` (once), and `agent-doc steering`
  keeps reporting it after the cycle closes. A `subagent` item there is a
  dispatch-now directive with its claim command; handle it before ending the
  turn or scheduling the next re-entry. The next preflight lists new
  subagent-intent queue items under `queue_subagent_dispatch` (never in
  `selected_queue_prompts`): run each `claim_command`, dispatch it, then run
  its `start_command` with the returned worker identity and printed generation.
- **Exchange subagents coordinator.** Under `<!-- agent:exchange subagents -->`
  (alias `fan-out`), preflight lists each new exchange prompt under
  `exchange_subagent_dispatch`. Dispatch every listed prompt immediately, one
  background subagent per prompt and a dedicated worktree for repository work.
  Exchange prompts have no queue claim/release lifecycle and no preamble, so
  the coordinator puts the scratch-isolation and live-tmux isolation rules (the
  `exchange_subagent_dispatch_guidance` text) in each subagent prompt itself. The parent retains
  the response cycle: review the worker result, answer the exchange prompt, and
  perform the normal binary-owned closeout.
- **Queue subagents coordinator (`#queuesubagents`).** Under
  `<!-- agent:run subagents -->` (alias `fan-out`; bare dispatches every
  eligible head and `=N` caps concurrent claims), the in-session agent is the
  coordinator, and `queue_subagent_dispatch` is the list to dispatch THIS cycle. Preflight
  recomputes it every cycle, so a missed claim is offered again. Heads held by
  the cap or by a still-queued `after=` / ordered-list predecessor are already
  excluded from the loop; do not drain them inline. Lines tagged `[inline]` or
  `[operator-verify]` drain in queue order as usual. Never run a dispatch
  item yourself, even a small one: an inline answer does not close it. The
  closeout, the preflight catch-up, and the session-check residue guard all
  keep an echoed dispatch item without a worker claim queued, warn
  `queue_subagent_inline_answer_refused`, and offer it for dispatch again
  (`#subagentinlineveto`). If this harness cannot spawn a subagent, leave the
  item queued and write a `**Deferred:** no subagent available` line for it.
  If you judge an item blocked, name it and the blocker in the response
  instead of skipping it silently. For each dispatch item:
  1. Run its `claim_command` (`agent-doc queue claim <FILE> --item <...> --owner subagent:<label>`).
     Dispatch only after the command succeeds: success includes projecting the
     claimed row's visible `🟡` marker through the current editor/CRDT authority.
     This means reserved/dispatching, not running.
  2. Paste the entry's `authorization.subagent_prompt_preamble` VERBATIM at the
     top of the subagent's Agent prompt (`#waypostauthorization`). The binary
     resolved it from the document: the verbatim item, the backlog text a
     `do [#id]` head names, and the queue/item preset bodies (for example
     `preset="#spec-test-build-install-commit-push"` → "update spec + tests.
     build + install for local testing. commit + push"), scoped to that one
     item, plus the coordinator rules (own worktree, never `make install` or a
     release, never touch the session document, scratch isolation). Never paraphrase or extend it. When
     `authorization.status` is `unresolved_preset`, the queue names a preset
     frontmatter does not define; the preamble already tells the subagent that
     no authorization beyond the item text was resolved, so do not add any.
     `agent-doc queue brief <FILE> --item <...>` (or the entry's
     `brief_command`; `--json` for the whole object) re-prints the same text.
  3. Dispatch one background subagent per item. When it touches a repo, give it
     its own git worktree OUTSIDE the IDE-watched project (for example
     `~/worktrees/<repo>-<label>`), and never run two subagents in
     one checkout. Heads that `agent-doc plan` gives the same repo
     `write_scope` still get separate worktrees, but they are integrated one at a
     time. A head marked `parallelizable: false` runs alone.
     After the spawn succeeds and returns its concrete worker identity, run the
     entry's `start_command` (`agent-doc queue start <FILE> --item <...>
     --owner subagent:<label> --generation <N> --worker-id <WORKER>`). This
     generation-fenced acknowledgement is the only `🟡` → `🚧` transition.
     Never infer running from a label, claim, process poll, or intended spawn;
     if spawn fails, leave the attempt `🟡` and release/requeue it. A claim
     generation binds to at most one worker and one worker binds to at most one
     live claim in the document; a stale generation or conflicting binding is
     refused without mutation.

     On Grok Build, use native `spawn_subagent` with `background=true` for one
     native background worker per dispatch item. Start each claim immediately
     with the returned `subagent_id`; do not wait for another operator prompt
     and do not substitute `agent-doc orchestrate` while that tool is available.
     Collect completions with `get_command_or_subagent_output` and preserve the
     same generation fence through report-complete and closeout.
  4. Treat `[#subagent-operator-request-<unique-id>] subagent operator request` as a
     blocking handback, not worker completion. Copy the entire packet verbatim
     into `agent:exchange` so the exact request, why it is needed, target/scope,
     risk or effect, and every offered choice remain visible. Present that same
     request from the coordinator's main harness session, then send the
     operator's answer verbatim to the same worker when continuation is
     available. Do not infer an answer from the task, choose on the operator's
     behalf, or convert a chat reply into a tool permission: a protected action
     may be retried only after the harness permission system grants that exact
     action. The closeout derives a same-packet-id todo and projects
     `🙋 [#request-id]` onto the originating `queue_item`. If supplying the todo
     explicitly, use `id=<request-id> ...`. When the operator's answer and the
     worker's result complete it, use `--todo-done <request-id>` in that
     completion closeout; do not leave a stale open TODO or queue annotation.

     Harness transport is capability-specific:

     - Claude Code natively passes foreground and background tool-permission
       prompts to the main session, but removes `AskUserQuestion` from
       subagents. Use `SendMessage`/`SubagentHandback` for the packet when
       available and the completed-result handback otherwise; native permission
       UI remains the grant authority.
     - Codex subagents use `send_message` to deliver the packet to the parent and
       `followup_task` to resume after the answer. If messaging is unavailable,
       return the packet as the final handback. The Codex approval UI, not an
       inter-agent message, authorizes a protected tool call.
     - Grok Build documents child-summary return to the parent but no portable
       mid-run question channel. Stop and return the packet in the summary; the
       coordinator asks and redispatches or resumes only if the running harness
       exposes a supported continuation.
     - OpenCode background children notify the parent with their result, and the
       built-in `general` subagent denies questions by default. Return the packet
       as the child result; after the coordinator asks, continue the same child
       session ID when available. Child permissions remain independent.
  5. While a subagent runs past a cycle, keep its claim alive with
     `agent-doc queue claim <FILE> --item <...> --owner subagent:<label> --refresh --generation <N>`
     using the generation printed by the original claim. Refresh is only a
     heartbeat for an already bound `🚧` worker; it never starts a `🟡` attempt.
     Under the
     attribute, an expired attempt becomes `⚠️` lost and is offered for dispatch
     again with a new generation, never drained inline.
  6. A subagent commits and pushes its own branch, fast-forward only by default.
     Verbatim operator authorization that explicitly names another target branch
     or explicitly requires a force push overrides only that default; the worker
     executes exactly the named scope and uses the `[#subagent-operator-request-<unique-id>]` protocol for
     any harness-native permission gate.
     It never runs `make install`, a release build, a version bump, a tag, or a
     release. Concurrent subagents share the coordinator's scratchpad, so a
     subagent passes its commit message through `git commit -F -` (a stdin
     heredoc) or a file inside its own worktree, never a fixed file in a shared
     scratchpad or `/tmp`, and puts other temp files in its own worktree or a
     unique `mktemp` path (one subagent's commit once carried a sibling's
     message).
  7. As soon as a subagent reports back, run
     `agent-doc queue report-complete <FILE> --item <...> --generation <N> --response '<worker response>'`.
     Pass the worker's complete handback verbatim; an empty response is refused,
     and there is no placeholder-only completion mode.
     The exact generation fence prevents a late result from mutating a reclaimed
     item and changes its marker to `☑️` (worker-complete handoff/outbox, awaiting integration). The binary appends
     immediate, deduplicated completion feedback to `agent:exchange` through
     its cross-process document owner and editor/CRDT authority while leaving
     the claim active; the worker never edits the session document. The
     feedback is a structurally complete, N−1-inert response cell whose
     heading names the worker and item and whose quoted body is the worker's
     actual response. A binary digest binds that body to the completion marker,
     so edited prose loses completion authority. It is binary-owned feedback,
     not your response: never
     hand-commit, edit, or replay it — the next preflight's document-only
     auto-commit (or your closeout) lands it, however many workers reported.
     A binary-owned `agent-doc commit` (for example the final status-sync
     checkpoint's, after `agent-doc status set`) lands pending cells with its
     own change, so run it as prescribed.
     If an earlier v0.34 binary wrote
     bodyless completion headings, migrate those exact artifacts with
     `agent-doc queue repair-completion-feedback <FILE>`; this does not replay
     the committed closeout or change cycle/claim state. Then
     integrate the work serially: rebase or
     cherry-pick each onto main, run the preset's verification, run ONE
     `make install` per integrated batch, and push. Then persist the result in
     THIS turn with the closeout that strikes the item: `--done <id>` for an
     `#id` head, or open the response with the verbatim
     `> **Queue prompt:** <line>` echo for a free-text head (a `### Re:`
     heading alone leaves the line queued). For a parent with nested
     sub-prompts the echo is the whole head, one quoted line per row:
     `> **Queue prompt:** <parent>` then `>   - <child>` for every child.
     A child that begins with `Include #subagent:` is release context, not a
     sub-prompt (`#ftstrike`, `#nestedechostrike`). Echoing the parent with
     those include lines does not strike the row while any named include is
     still open (a live queue row owned by `subagent:<label>`). When every
     named include is done, that same echo strikes the parent and its include
     lines together.
     When an `#id` row's work is finished but its tracked item must stay open
     (a Todo that waits on an operator check, where `--done` would resolve
     it), close the ☑️ row with
     `agent-doc queue consume <FILE> --ack-id <id>` instead; it ends the
     reported attempt, keeps the item open, and refuses a claimed, running or
     lost attempt (`#nestedechostrike`). That closeout also ends the claim,
     and the same queue-strike successor retires the matching binary completion
     cell; unmatched worker reports remain visible. Thus the integrated response
     replaces its temporary “awaiting integration” notice instead of leaving a
     stale `agent-doc-subagent-completion` marker beside it. No release is needed
     first (`#claimstrike`). A bare
     `agent-doc queue release <FILE> --item <...> --generation <N>` records that the worker
     reported (`#reportedrelease`): the line stays queued, is never offered to
     a new worker, steering routes it `dispatch=persist_reported`, and the
     Codex Stop hook blocks until it is persisted, so release only when
     integration must finish before you can answer, and persist before the
     turn ends. For abandoned, failed, or requeued work run
     `agent-doc queue release <FILE> --item <...> --generation <N> --requeue`: no report is
     recorded and the line is dispatchable to a new worker again.
  Harnesses without an in-session Agent tool (Codex, OpenCode, Cursor) claim
  the same items and fall back to
  `agent-doc orchestrate <FILE> --mode parallel --task "<item>" ...` over the
  dispatch items. If neither path exists, say so in the response and drain the
  items inline. When every remaining head is claimed or held, end the turn
  quietly (the Stop hook logs `state=waiting_on_claims`): subagent completion
  notifications resume the loop, and you do not need to poll or schedule a re-entry.
- If session-accretion supplies bounded context, use the included `### Re:`
  blocks as prompt-position anchors, not proof that older turns are absent.
- Execute from the planning record. If `execution_scope=plan_backlog_only`, stay
  in plan/backlog capture mode. Otherwise complete the requested repo work before
  persistence or stop on a blocker. Do not keep appending "starting/continuing"
  status prose while the requested work remains undone.

**Draining a free-text queue head — quote it (`#qdeferstrike`).** When you answer a
free-text queue prompt (a queue head with **no** `#id`), quote it verbatim as a
`> **Queue prompt:**` blockquote at the top of your response. The position-
independent strike (`#ftstrike` / `strike_answered_free_text_queue_heads`) matches
the head's prose against your response's **blockquote** region — a `### Re:` heading
alone is normally not enough, so a heading-only answer leaves the head queued and
the loop churns re-answering it forever. The narrow historical-recovery exception
is a heading that repeats the head's same long underscore-delimited code identifier;
ordinary prose topics, filenames, and short symbols do not qualify. This matters
most when the free-text head sits
*behind* a deferred `[operator-verify]`/`[focused-cycle]` id-head: the leading
consume stops at that deferred id-head, so the **only** path that can strike the
free-text item is the blockquote-matched `#ftstrike`. `do [#id]` heads are
different — they strike by id via `--done <id>` regardless of position and need no
quote.

**Nested `agent:run` leaves (`#nestvector`).** A nested queue item that is its own
head — indented under group labels, not a `#nestedsubprompts` child folded into
its parent — is quoted as the whole nest vector, from the root parent down to
the item: `> **Queue prompt:** root parent: parent: item`. An id-backed ancestor
is its `#id` (`[#vscodecodersmoke]` → `#vscodecodersmoke`); a free-text ancestor
is that row's line; the leaf stays the item text, including a `do [#id]` line.
A free-text head that owns nested sub-prompts stays the multiline quote
(`> **Queue prompt:** parent` then `>   - child`), not a colon vector. The vector
answers the leaf, not its ancestors. Closeout rewrites a bare leaf quote into
the vector. The prompt submitted for that leaf is that vector plus the entire prompt
tree (`#nestsubmit`): a synthetic queue diff, a plan queue prompt, a direct
run's active-queue diff, and a subagent's submitted prompt. The tree is
context from the parsed queue AST, including a sibling branch that is not on
the lineage path. An example lineage is
`https://github.com/haiven-dev/haiven-api/pull/758: #rebase`. Identity, claims,
`--item`, and the selected queue prompt stay the leaf, so a quote of either
the vector or the leaf answers that leaf. Do not quote the tree as the queue
prompt. A nested child that begins with
`Include #subagent:` is release context for that quote, not a sub-prompt: the
quote does not strike the row while any named include is still an open queue
row (`#ftstrike`, `#nestedechostrike`).

**Quoting a free-text head you are NOT finishing this cycle (`#deferstrike`).** The
quote alone reads as "answered" and strikes the head. To keep it queued, follow the
`> **Queue prompt:**` quote with a paragraph that opens with the bold lead
`**Deferred:**` (any `**Deferred…**` lead, such as `**Deferred, not done:**`,
counts) and says what it waits for. Plain phrasing such as "not done yet", "keep
this head open", "dispatched to a subagent" or "waits for" in the quote's paragraph
is also read as a deferral, but the bold lead is the contract.

**Claimed heads (`#claimstrike`).** A worker claim (`agent-doc queue claim <FILE>
--item "<head>" --owner subagent:<label>`) says who owns the work; it does not keep
a quoted head queued. When a subagent finishes a claimed free-text head and you echo
it as `> **Queue prompt:**` with no deferral, finalize strikes the head and releases
its claim in the same closeout, and preflight's residue strike does the same for an
echo already committed. You do not need `agent-doc queue release` first. While the
worker is still running, quote the head only with a `**Deferred:**` paragraph (or do
not quote it): the claim stays, and the pre-write gate does not demand evidence for a
claimed head this cycle selected. `do [#id]` heads complete by `--done <id>` whether
or not they are claimed.

That exact echo is the **only** evidence a claimed head accepts
(`#ftstrikeclaimedmention`). A status response may name claimed heads freely:
mentioning a claimed head, quoting its prompt-preset body, putting it in a
`### Re:` heading, or quoting it without the `**Queue prompt:**` label strikes
nothing. The closeout's leading-head consume never takes a claimed head on the
closeout alone, and the binary never inserts its own `> **Queue prompt:**` echo for
one. Separately, a preset head such as `#gh-fix <url>` is answered by its expanded
preset body only when the response also contains the head's own argument (the URL),
so one quote of a shared preset body never answers every head that uses the preset.

**Annotated `#id` heads are operator steering — answer the annotation (`#qheadannotation`).**
The canonical backlog-mirror head is `do [#id]`. When the operator appends their own
text to it — `do [#id]: can the *.h files be generated too?`, `[#id] and keep the old
API` — that text is a directive for the **current** turn in its own right, not
decoration, and it applies whether it was there at preflight or typed onto the head
mid-turn. Preflight surfaces it as `queue_head_annotations: [{id, annotation_verbatim}]`
with `queue_head_annotation_guidance`. Do the backlog item **and** answer the
annotation in the same response, quoting it as:

```markdown
> **Operator note:** can the *.h files be generated too?

<your answer>
```

Quoting the whole head in the `> **Queue prompt:**` echo is **not** an answer (that
is exactly how the sdk.md question was consumed unanswered on 2026-10-02). An
id-completing closeout (`--done <id>`) checks the live head: if its annotation has
no `> **Operator note:**` quote followed by answer prose, the pre-write gate fails
closed and nothing is captured — add the answer and retry. If an annotation only
appears after the response was captured, the consume does not drop it: the head is
consumed by id and the annotation is re-queued verbatim as its own free-text line
right after it. A bare `[#id]: note` (no `do`) is inert prose, i.e. an ordinary
free-text head answered through the `#qdeferstrike` quote above.

**Deferring an annotated head (`#annotateddeferstrike`).** Deferral does not waive
the annotation answer. For an annotated head you are not finishing this cycle
(`do [#id]: note`, `[#id]: note`, or a free-text head carrying an operator note),
write the three parts in this order, each its own paragraph:

```markdown
> **Queue prompt:** do [#id]: Test yourself

> **Operator note:** Test yourself

**Deferred:** dispatched to a subagent (claim `subagent:label`); it reports back with ...
```

The `**Deferred:**` paragraph is both the answer prose for the operator note and
the deferral of the head: the deferral scan skips `> **Operator note:**` quotes
that follow the echo, so the head stays queued and its claim stays held. A
deferred id-backed head is then consumed only by an explicit `--done <id>` (or
`--pending-gate`/`--review-resolve`); its `### Re: #id` heading or the prompt
diff no longer completes it. Without the `**Deferred:**` lead the same shape
reads as answered and the head is consumed.

A queue head that references one or more registered `#preset` names is a preset
head, not ordinary free text, including composable forms such as `#gh-fix URL`.
Address every resolved preset expansion supplied by preflight. The closeout gate
matches response evidence against those expansions, so a literal `#preset` or
`[#preset]` queue-head quote is optional.

An in-progress marker on a free-text head records selection, not completion
(`#bugautostruck`). It never replaces the exact `> **Queue prompt:**` evidence.
If the response does not actually quote and answer that head, closeout must leave
it active even when the marker was added earlier in the cycle.
When a selected head enters the live queue after the pre-turn baseline, durable
cycle-state selection plus an exact current-response quote and answer prose
allows the same atomic closeout to consume it. The visible marker alone is not
authority. This prevents the next Stop hook
from reopening work the just-committed response demonstrably completed; neither
the marker nor preset-expansion wording is sufficient by itself.

**Already-complete or backlog-tracked free-text heads auto-strike (`#qftbklgstrike`).**
Separately from the exchange-answer strike above, preflight queue maintenance
auto-strikes a live free-text head when a deterministic scorer proves the work is
already captured elsewhere — either it is already complete (a matching `agent:done`
item) or a backlog item already addresses it (a matching active `agent:backlog`
item) — even with no exchange answer for it. The head is struck in place and
annotated `— auto-struck: completed by #<id>` or `— auto-struck: tracked by backlog
#<id>`; it is never deleted. The match must clear a conservative threshold (a near
restatement of the tracked item), so an unrelated operator prompt is never struck:
you do not need to do anything to trigger or avoid this — a genuinely unanswered,
unrelated head always survives. id-backed heads are unaffected (they strike by id).

The `#ftstrike` pass is **conservative about in-flight edits (`#qstrikeexplain`)**: it
strikes a free-text head only when that head was present in the stable pre-turn
baseline (the preflight baseline), or durable cycle state proves preflight
selected that exact head and the current response exactly echoes and answers it.
A head that first appeared in the live buffer *this* turn — a line the operator
is still typing — is **never** same-cycle struck merely because it has a visible
marker or fuzzy-matches a quoted prompt; it defers to the cycle that
actually answers it (editor-wins, consistent with `#queue-user-edit-overwrite`). So
quoting a queue head you did not intend to drain cannot strike a line the operator
is mid-authoring.

**Response header format (template mode):** use `### Re: topic` markdown headers —
**not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings. Use
h4–h6 for sub-sections within a response.

**Model attribution and timestamp (`#timestampresponseheader`):** always append the
resolved model short name and the response timestamp after a spaced em dash,
joined to each other by a spaced middle dot:
`### Re: topic — opus-5 · 2026-09-11T23:45-04:00`. Use `preflight.agent_model` if
non-null (from frontmatter); otherwise use your own model identity. Never use the
harness label (`codex`, `claude`) as the model name, and never omit it. The
timestamp is local time with its UTC offset, `YYYY-MM-DDTHH:MM±HH:MM`, taken when
you begin composing the response; if you genuinely cannot resolve a clock, emit the
model alone (`### Re: topic — opus-5`) rather than a dangling separator.
The agent authors this attribution from the preflight contract; the production
write path preserves the complete heading through
`agent_doc_template::response_materialization::canonicalize_strict_closeout_response_heading`.

**The heading must contain exactly ONE spaced em dash.** That is why the timestamp
joins the model with ` · ` instead of a second ` — `. Four parsers read this
heading and they do not agree on which dash bounds the attribution: three take the
FIRST (`response_prompt_target_from_re_heading`,
`queue_response::response_heading_topic`, `response_replay::normalize_replay_topic`)
while `transient_markers::strip_re_heading_attribution` takes the LAST via
`rfind(" — ")`. A second em dash leaves the model name behind on the strip path,
and because that strip backs `normalize_post_commit_re_heading_drift` — the
comparison that lets an attributed heading match an unattributed one — the residue
reads as permanent heading drift on every committed response. The format is owned by
`agent_doc_turn::response_text::response_heading`; the cross-parser property is
pinned by `a_timestamped_heading_reads_the_same_through_every_parser`.

**Response checkpoints and sealing:** stream incomplete progress only to the
harness console. A standalone conclusion that remains useful before closeout may
be written with `agent-doc salient-checkpoint <FILE>`. The binary structurally
validates the complete Markdown body and upserts one cycle-scoped
`Live response (not final)` node. It is not answer evidence and does not consume
queue/backlog work or commit. After each complete `### Re:` section (balanced
fences and component markers), the harness may persist the cumulative response
with `agent-doc response-checkpoint <FILE>`. The binary replaces the
uncommitted response tail in Lazily; it does not consume queue/backlog work or
commit. A final response removes any live-response node for the cycle.
Turn-end `agent-doc respond` asks the binary to resolve those closeout mutations
and commit exactly once (`finalize` remains an alias). See
[streaming-checkpoints.md](streaming-checkpoints.md).

**`#agent-doc-bug` plan proof:** if the prompt contract requires a plan, create
the plan file before closeout and cite every plan path. If
`execution_scope=plan_backlog_only`, create plan/backlog items and explain the
deferred implementation boundary instead of editing code.

## Step 1b — Update backlog (template mode)

Mutate `<!-- agent:backlog -->` (or legacy `agent:pending`) only through granular
`agent-doc write` flags: `--backlog-add`, `--done <id>`, `--backlog-edit
"id=text"`, `--backlog-reorder`, `--backlog-gate`, `--backlog-ungate`,
`--review-add`, `--review-edit`. Full-replace via `<!-- patch:backlog -->` /
`<!-- patch:review -->` is rejected; see [pending-ops.md](pending-ops.md). For
`<!-- agent:icebox -->`, use granular `--icebox-add*` flags.

Completed/reaped items live under canonical `<!-- agent:done -->`; legacy
`agent:backlog-done` and `agent:pending-done` tags require `agent-doc migrate`.

**Backlog capture rule:** if the response creates concrete follow-up work, add it
to `agent:backlog` in the same cycle. Before choosing placement, compare its
urgency with the existing queue head (#lzfollowupqueueorder):

- Plain `--backlog-add` deliberately prepends. Use that default when the new
  follow-up is more urgent or more task-relevant than the work already queued.
- During an active drain, a merely adjacent, equal-priority, or lower-priority
  follow-up must not preempt the existing head. Keep the cheap add operation, but
  pass `--backlog-queue-placement append` so the queue mirror puts it after the
  work the operator already ordered.
- When extending an ordered batch, use `--backlog-add-after <id>` or
  `--backlog-add-before <id>`. Those explicit anchors determine both backlog and
  queue-mirror order; `--backlog-queue-placement` applies only to an unanchored
  plain add and does not override them.

If the item is only a recommendation, include `[recommended]`.

**Cross-document backlog rule:** if a prompt preset or user instruction names
another backlog file, add the item to that target with `--backlog-add-to
<target-file> "<item>"` on the final `agent-doc respond` command. Do not satisfy
an explicit target by running `--backlog-add` against the current session
document. If the target is missing or lacks a backlog component, stop on the
binary error and report the blocker.

**Plan-backed backlog items:** create the plan file first and include that exact
plan file path in the backlog text. For multi-phase implementation work, prefer
one backlog ID per actionable phase (for example `#crdtrespfx1`, `#crdtrespfx2`)
instead of one parent ID that gets repeatedly `--backlog-gate`d after partial
progress; keep the parent plan file as context, but queue and close out concrete
phase IDs.

**`do #id` closeout rule:** when the user directs `do #id ...`, record the backlog
outcome before persistence: `--done <id>` if completed, `--backlog-gate <id>` if
code-complete but awaiting review/external validation, or explain concretely why
it stays open. `session-check` enforces the `pending_done_guard`; projects may opt
into `review_done_guard` when review must precede done.

**Complete over gate.** Default to `--done` — finish the work this cycle. Gating
to `agent:review` is exceptional, only for work genuinely blocked on something the
turn cannot do (live editor/pane verify, external approval, CI outage), and the
item must name what unblocks it. `[operator-verify]` is metadata, not proof of a
gate: an item with remaining agent-executable steps belongs in `agent:backlog` and
may retain the tag. When a review gate clears but work remains, use
`--backlog-ungate <id>` (and `--backlog-edit` if needed), not `--review-edit`, so
the same id becomes actionable again. Unblocked follow-up goes to `agent:backlog`,
not `agent:review`. Keep `agent:review` small (target < 10); convert stale/satisfied
gated items to `--done` or actionable backlog items rather than letting them
accumulate. See [pending-ops.md](pending-ops.md) for the full review-discipline
rule.

## Step 1c — Record operator TODOs (template mode, `#operatortodo`)

`agent:todo` records concrete actions that the operator, rather than the agent,
must perform. When the work creates one, add one brief checklist item
in the same cycle. Start with the action and keep only the context needed to act:

- Prefer a compact reference to an existing `agent:review` or `agent:backlog`
  `#id` over restating that tracked item's description.
- A draft file path is a valid reference. Publishing an unpublished draft is the
  canonical example: `- [ ] Publish draft docs/launch-note.md`.
- A reference does not change ownership or scheduling. Agent-runnable work stays
  in `agent:backlog`; a human gate stays in `agent:review`; `agent:todo` only
  records the operator action.

Record it with the closeout flag `--todo-add "<action>"` (repeatable), for
example `agent-doc write --commit <FILE> --todo-add "Verify [#gate1] in a live pane"`.
The binary appends `- [ ] [#<id>] <action>`, creates `agent:todo` when the
document has none (right below `agent:backlog`), keeps every existing checklist
item, and skips an action that is already listed. A missing `agent:todo`
component is never a reason to skip the TODO.

**Todo ids (`#todoids`, `#todoaddidhijack`).** Every todo you add carries an
own id that is its OWN identity: your explicit `id=<id> ` always wins, else the
binary mints one. A todo never takes the id of an open `agent:review`,
`agent:backlog`, or `agent:icebox` item, and a `#<id>` citation in the action stays a
reference. To surface a decision on a tracked item, cite its id in the action:
when the action cites exactly one open tracked item the binary links the todo
to it with the canonical source locator — the example above becomes
`- [ ] [#t1a2] Verify [#gate1] in a live pane (source: agent:review [#gate1])`
— so checking the box reports the prerequisite done and resolving the item
checks the todo off. A second add without `id=` for an item an open todo
already links is a no-op (edit it with `--todo-edit` instead). `id=<id> ` is
refused when an open tracked item already owns `<id>` (pick a new todo id and
cite the item) and when `<id>` is already resolved. Prefer
`--todo-add` over `patch:todo` for new items because it expresses the identity
directly. A strict closeout still assigns an own id to an id-less `patch:todo`
row before commit (`#todoidcloseout`); the committed row never waits for the
next preflight. Preflight audits explicit and implicit
legacy rows that identify exactly one open tracked source, reports them as
`legacy_source_unqualified` in `todo_signals`, and appends the canonical source
locator through the live-editor/detached-disk authority path behind the typing
gate (`#todocontextmigration`).

**The operator finishes a todo by checking its box.** `- [x]` in the editor is
the done signal; no agent turn is needed and no agent write may undo it (a
`patch:todo` carrying the item open keeps it checked; only `--todo-reopen`
unchecks). Preflight reconciles it in `todo_signals`:

- `operator_done_review_open` — the operator checked the todo for an open
  review gate: read any operator note, then resolve the gate this cycle
  (`--review-resolve <id>`) or record the follow-up. The binary never resolves
  a gate from a checkbox.
- `operator_done_backlog_open` — the operator prerequisite for a backlog item
  is done; treat it as unblocked (`--backlog-ungate <id>` when gated).
- `operator_done_icebox_open` — the operator made the decision a todo
  surfaced for a parked icebox item: read any operator note, then act on it
  this cycle (`--done <id>` to drop it, move it into active work, or record
  the follow-up). The binary never moves or closes parked work from a
  checkbox.
- `linked_work_resolved` — the todo's own id is resolved work; the next
  closeout with tracked-work flags checks it off automatically
  (`[todo] auto-done`), or run `--todo-done <id>` now.
- `cited_work_resolved` — every tracked item the todo cites is resolved; check
  it off with `--todo-done` only if the operator action itself is finished.
  These are never auto-completed.

**Mark it complete when it is finished (`#tododone`).** When the operator says
an action is done, or you verify it is (the gate it cites resolved, the draft is
published), check it off in the SAME closeout:
`agent-doc write --commit <FILE> --todo-done <id>`. The selector is the item's
own `[#id]`, a `#id` the item cites (`... (#gate1)`), or its exact text, so
operator-written items without an id work too. The binary flips `[ ]` to `[x]`
in place (position, id, and text kept; a repeat is a no-op) and folds any
duplicate copy of the item. Never complete a todo by appending a second
`- [x] ...` line — that leaves the open original behind. Related flags:
`--todo-edit "id=new text"` (keeps id/state/position, e.g. to add a completion
note), `--todo-reopen <id>`, and `--todo-remove <id>` for an item that no longer
applies. Checked items stay in `agent:todo` for the operator to prune.

`<!-- patch:todo -->` also works and also creates a missing component, but it is
a guarded replacement: reproduce every existing checklist item plus the brief
new or updated item. Never use it to prune operator TODOs; the write path
rejects any replacement that lowers the checklist-item count.

When a closeout opens an `agent:review` gate that `agent:todo` does not mention,
the write prints `[todo] advisory (#operatortodo)`. If that gate waits on the
operator, add the TODO next cycle (`--backlog-only --todo-add ...` for a
tracked-work-only closeout); a gate waiting on CI or a third party needs none.
A response that presents a blocking `[#subagent-operator-request-<unique-id>]`
packet owes a same-id todo (`#wayposttodo`). The binary derives it from the
packet's `request:` and choices unless `--todo-add` already supplies
`id=<request-id>`, and links its `queue_item` with `🙋 [#request-id]`
(`[todo] blocking operator request packet (#packettodo)`).
Preserve the canonical shape when copying the packet into exchange: the bare
marker line, emphasized-key list fields, the `queue_item`, `request`, and
`context` values as a nested list beneath their key bullet (the context as
short one-fact bullets), and choices nested below `- *choices:*`. The parser
keeps historical inline-value and plain-key packets readable.
Every operator ask in the response needs its own todo: an explicit request
(`Say \`post\``, `needs your ok`), an unpublished draft path, and each item of
a bolded residual-work list (`**Still stale:**`, `**Also stale:**`,
`**Not done:**`, `**Still open:**`, `**Open:**`, `**Remaining:**`). Add one
`--todo-add` per ask that `agent:todo` does not already hold; one add does not
cover a whole list. Otherwise the write prints `[todo] advisory (#operatortodo):
this closeout response leaves N operator actions pending ...` quoting each ask
no todo matches (GH #318); record them next cycle.
When a closeout resolves work (`--done`, `--review-resolve`), an OPEN todo that
owns that id is checked off in the same closeout (`[todo] auto-done`); one that
only cites it gets `[todo] advisory (#tododone)` with the exact `--todo-done`
command — if the operator action is finished, run it next cycle.

**Queue heads that name a todo (`#sdktododone`).** When the operator queues a
todo's own id (`[#tdrename]: rename to sdk-dev`, `[#tdgo]: go ahead`) and you
finish that work, close with `--done <id>`: it checks the todo off in place
and strikes the head in one transaction. Never hand-patch the box. When the
head only asks a question about a decision todo, answer it and leave the todo
open; do not pass `--done` for it. A todo's own id is its leading `[#id]` or
bare `#id`; `--todo-edit` keeps either spelling.

**Id-less todo rows get ids at preflight (`#todoidbackfill`).** A row the
operator typed by hand, or one an older write left without an id, gets an own
id at the next preflight (`[preflight] todo: assigned own id ...`). A row citing
exactly one open review/backlog/icebox id reuses that id. When the operator reports
the action done, close the row by that id (`--todo-done <id>`); never append a
checked copy.
