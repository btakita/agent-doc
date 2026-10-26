# Session Cycle

This is the mandatory cycle spine. Read it for every normal `agent-doc` turn, then
load the focused runbooks named by the active branch. The installed `SKILL.md` is
only the compact routing table that survives context compaction.

## Entry and authority

- **Harness-native `agent-doc` entrypoints start the binary-owned response cycle.**
  Treat the entrypoint as an executable workflow start, not a generic
  document-editing request. Do not manually patch the final assistant response
  into the document. The binary resolves and commits the turn through
  `agent-doc respond <FILE>` (`finalize` is its compatibility alias) or the
  repair path `agent-doc write --commit <FILE>`.
- **No FILE means this pane's claim.** A bare `/agent-doc`
  or Codex `agent-doc` runs on the document this tmux pane claims. Claude Code
  and Codex hooks name it first in the admitted context (`no FILE argument:
  this turn's FILE is ...`); hookless harnesses run
  `agent-doc resolve-pane-document`. Use that path as FILE; on a refusal,
  report it and stop without guessing.
- **Document is the UI.** User edits are the prompt. A chat-only prompt is also
  a session turn (`#chatprompt`, GH #125; `#chatprompt`, GH #140): begin the response with
  the exact hook-issued `> **Chat prompt (#chatprompt:<id>):** <verbatim prompt>`
  prefix so it is recorded in the document. OpenCode has no `UserPromptSubmit` hook,
  so its agent MUST self-apply this rule with a unique compact ID. Legacy
  ID-less records remain readable and must not be rewritten just to normalize their marker.
  A non-empty `chat_prompts` is never `no_changes`, and
  `session-check` warns `chat prompt not recorded` if closeout skipped it.
- **Imperative edits are executable directives.** `do #id`, `fix this`, `go`,
  `push`, tests, build/install, and commit/push authorize the named work. A
  resolved preset body carries the same authorization. Do not ask the operator
  to say `go` / `push` again or invoke the preset again. Harness-native tool
  permission remains a separate gate. A harness-native tool-permission prompt
  still requires the harness permission system's grant.
- **Operator-visible document text is authoritative.** Never recover, patch, or
  hook-closeout by replacing it with `content_ours`, a snapshot, an incomplete
  token capture, or a lazily visible-write receipt when that could drop operator
  text. Snapshots and incomplete token captures are backup/audit state, not
  hot-path authority; fail closed or retry through the editor instead.
- Use named remote hosts only when the prompt, document/frontmatter,
  project-local `.agent-doc/config.toml`, or a project-local runbook identifies
  them. Globally approved SSH commands and ambient SSH config do not establish
  project scope.

## 0. Admit and plan

1. Detect `claim`, `compact`, and `compact exchange` before the normal cycle.
   `compact` runs `agent-doc compact <FILE> --commit`; `compact exchange`
   follows [compact-exchange.md](compact-exchange.md).
2. Apply the version gate in [harness-invocation.md](harness-invocation.md).
3. **Preflight is binary-owned (`#preflightinbinary`, `#hookcontractlost`).** The
   turn context must contain the trailing `[agent-doc] cycle contract (preflight
   already ran in the binary; ...)` success marker. Consume that sealed contract;
   never shell back to `agent-doc preflight <FILE>` from the model turn. An
   `UNAVAILABLE` marker means admission was refused: report its reason, run the
   named `session-check`/repair if authorized, and stop. No marker is a
   fail-closed harness-admission defect.
4. If `no_changes: true`, relay `no_changes_explanation.guidance` and stop,
   except when the turn carries a chat prompt absent from the document. Surface
   warnings and claims. Trust the binary-owned baseline in `state.db`. There is
   no `--baseline-file` flag; never invent or pass a baseline path.
5. On the first cycle, use `agent-doc read <FILE>` if document content is not in
   context. Never read the snapshot directly.
6. Handle `slash_commands`, `builtin_commands`, `orchestration_request`, and
   `prompt_preset_expansions` before composing. Follow
   [preset-runbooks.md](preset-runbooks.md),
   [model-tier-gate.md](model-tier-gate.md), and
   [planning-dispatch.md](planning-dispatch.md). Run `agent-doc plan <FILE>` and
   execute its `prompt_targets`, `execution_scope`, `repo_actions`,
   `required_commands`, `pending_mutations`, `handoff`, and `blockers` contract.

## Mid-turn steering and work

- After tool calls, process every `[agent-doc] operator steering arrived
  mid-turn` item by its emitted `dispatch`. Address `address_now` this turn;
  leave `drain_after_current` in queue order; dispatch each `subagent` item as
  directed. Never re-answer prompts already committed. Harnesses without the
  hook poll `agent-doc steering <FILE>` between long steps.
- Subagent claims are attempt- and worker-fenced (`#queueclaimstate`,
  `#queueclaimworker`). Save the claim generation. `📥` is pending/unclaimed,
  `🟡` is reserved/dispatching, `🚧` is worker-bound/running,
  `☑️` is worker-complete handoff/outbox, `⚠️` is lost, and `❔`/`🔄` need
  reconciliation. Worker-owned rows also name their exact owner and generation.
  Claim, spawn, start with the concrete worker id, report completion, integrate
  serially, and persist the result in the same turn. The complete coordinator
  protocol lives in [respond.md](respond.md) (`#queuesubagents`).
- MCP auth / OAuth is a sub-step, not a closeout boundary. Resume the same cycle
  after authentication.
- Complete requested implementation, verification, build/install, and local
  inspection before response persistence. Do not keep appending
  "starting/continuing" status prose while requested work remains undone.

## 1. Respond and mutate tracked work

Read [respond.md](respond.md) completely. Reconcile the changed exchange tail
oldest-first: do not stop at the newest question, and address each unresolved
prompt and `prompt_target`. Load [pending-ops.md](pending-ops.md) for backlog,
review, done, icebox, and TODO changes. In template mode use real headings:

`### Re: topic — opus-5 · 2026-09-11T23:45-04:00`

Under `#timestampresponseheader`, use the preflight model short name; never the harness label (`codex`, `claude`).
Use a local `YYYY-MM-DDTHH:MM±HH:MM` timestamp.
The heading contains exactly ONE spaced em dash; keep those words together as
the parser invariant.

## 2. Verify and persist — mandatory

Read [persist-closeout.md](persist-closeout.md) and [commit.md](commit.md)
completely before closeout. The mandatory normal boundary is:

```bash
cat <<'RESPONSE' | agent-doc respond <FILE> --stream --origin skill
<!-- patch:exchange -->
<complete cumulative response>
<!-- /patch:exchange -->
RESPONSE
```

Template responses require both patch markers. `respond` must reach `committed`
and pass its binary-owned post-commit check. Direct-exec Codex/OpenCode paths
also run `agent-doc session-check <FILE>`; connected clients consume the
terminal finalize result and do not run a redundant check. After `respond` /
`write --commit`, start no more long-running work for that turn.

This boundary is mandatory unless the user explicitly told you to leave the
response uncommitted.

Manual missed-patchback repair uses `agent-doc write --commit <FILE>`, never a
bare `agent-doc write` and never direct response patching. Captured finalize is
binary-owned: do not recapture, re-answer, alternate closeout commands, or elect
`--force-disk`; the keyed supervisor worker resumes the retained intent with
bounded retry and exact-once commit semantics.

Manual repository commits exclude the active session document and stage only
the resolved intended non-session path set. Stop on stage failure and verify the
staged diff before commit. The binary closeout owns the session document.

## Branch index

- Natural-language focused extraction: [transfer-extract.md](transfer-extract.md).
- Queue auto-loop and supervisor handoff: [queue-auto-loop.md](queue-auto-loop.md).
- Routing/startup/tmux details: [harness-invocation.md](harness-invocation.md),
  [code-enforced-directives.md](code-enforced-directives.md), and
  [multi-tmux-sessions.md](multi-tmux-sessions.md).
- Baseline/cache recovery: [baseline-drift.md](baseline-drift.md) and
  [jb-cache-conflict.md](jb-cache-conflict.md).
- Image delegation: [describe-image.md](describe-image.md).
