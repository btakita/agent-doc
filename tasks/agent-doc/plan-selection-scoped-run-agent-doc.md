# Plan: Selection-scoped Run Agent Doc

Status: design pass for `#promptselect`

Related architecture:

- [`plan-exchange-tree-seqcrdt-and-ipc-unify.md`](plan-exchange-tree-seqcrdt-and-ipc-unify.md)
  owns the per-node exchange CRDT and realtime-steering aggregate.
- This plan owns selection ingress, command completion, prompt delivery, and
  prompt-to-response linkage across the CLI, Project Controller, JetBrains,
  and VS Code.

## Outcome

When an editor selection is non-empty, **Run Agent Doc** sends exactly that
selection as a semantic prompt. When there is no selection, Run Agent Doc keeps
its current whole-document trigger behavior.

The controller first admits the selected text as a durable exchange prompt
node. It then derives one of three delivery outcomes from current reactive
state:

1. an idle document dispatches a new turn for that prompt;
2. an active document exposes the prompt immediately in the turn's aggregate
   steering set;
3. replay of an already admitted command is deduplicated by command identity.

The prompt is not copied into `agent:queue`. Queue remains the operator's
explicit durable work-ordering surface. An admitted but temporarily
undeliverable prompt remains an unresolved exchange prompt, and controller
effects retry delivery from that state.

## Terminology

There is no separate “legacy markdown” format. The existing `❯` prompt and
`### Re:` response text is the current markdown compatibility projection.
The semantic model gains stable node identities and relationship edges while
continuing to project readable markdown.

## Existing seams

- `agent-doc-markdown-ast::exchange_tree` already parses prompt and response
  nodes and exposes `add_prompt` / `add_response`, but currently appends a flat
  text tail.
- The exchange-tree/SeqCrdt plan already specifies per-node CRDT ownership and
  an observable aggregate of realtime steering directives.
- JetBrains `CpRouteClient.runEditorRoute` and its command-plane payload already
  accept `selected_text` and `steering_id`, but `SubmitAction` does not capture
  an editor selection.
- The Project Controller currently logs those fields as
  `legacy_steering_normalized` and deliberately converts them into a plain
  trigger. This normalization is the controller migration seam.
- VS Code's Run Agent Doc action has the active editor but its editor-route
  payload has no selection fields.
- The existing `agent-doc prompt` CLI name detects harness permission prompts.
  New text submission must extend that grammar without silently changing the
  meaning of existing invocations.
- Editor command-plane clients already distinguish transport acceptance from a
  terminal `applied` command projection. That terminal boundary is retained.

## Semantic model

### Stable identities

Typed prompt submission carries a unique `command_id`. The controller derives
an opaque `prompt_id` from document identity plus `command_id`; retrying the
same command converges on the same prompt node, while intentionally submitting
the same text twice creates two prompt nodes.

This refines the exchange-tree plan's body-hash sketch:

- `prompt_id` is occurrence identity and must not be a body hash;
- `content_hash` detects text equality and CRDT convergence;
- imported markdown without IDs receives deterministic migration IDs based on
  document identity, structural occurrence, and content;
- hidden projection metadata preserves IDs across later markdown edits.

Body equality therefore never erases a repeated prompt.

### Prompt and response relations

The durable exchange graph contains:

```text
PromptNode {
  prompt_id,
  body,
  origin,
  admitted_command_id,
}

ResponseNode {
  response_id,
  body,
  primary_prompt_id,
  addressed_prompt_ids,
}
```

`primary_prompt_id` is the originating prompt under which the response is
placed in the markdown compatibility projection. `addressed_prompt_ids` is a
non-empty set and allows one response to settle the initial prompt plus several
concurrent steering prompts without duplicating response text.

The Document Tree projects the response below every addressed prompt. The
primary edge owns the response body; secondary edges are references to the same
response node. Relationship authority is the ID edge, never heading text,
`(HEAD)`, prompt prefixes, or physical adjacency.

For ordinary one-prompt turns, `primary_prompt_id` is the sole addressed ID and
the visible result is simply:

```text
❯ selected prompt

### Re: Response

answer
```

Finalize receives the admitted prompt ID and the current aggregate steering
IDs. It records which IDs the response addresses. Any unaddressed prompt stays
unresolved and remains eligible for the next turn rather than being struck by
an in-progress marker or response adjacency.

## Command contract

Add a typed command-plane payload, `agent-doc.selection_prompt.v1`:

```text
SelectionPrompt {
  document_id,
  command_id,
  text,
  origin,       // cli | jetbrains | vscode
}
```

Submission has two distinct receipts:

1. **transport accepted** means the controller durably admitted the command
   identity and may continue after disconnect or recycle;
2. terminal **applied** means the prompt node is visible in the authoritative
   CRDT projection and includes `prompt_id`, `content_hash`,
   `crdt_generation`, and the derived delivery outcome.

No additional plugin acknowledgement is required. The plugin may trust the
controller's terminal `applied` command projection because the included CRDT
generation/content receipt proves semantic application, not merely socket
delivery. If the connection drops after acceptance, the plugin resumes by
querying the same `command_id`; it does not resubmit with a new identity.

The command may be accepted during controller replacement, but it cannot become
terminal `applied` until the promoted controller observes the prompt node at or
beyond the receipt generation. This composes with mid-turn recycle.

During migration, the classic `editor_route` endpoint translates non-empty
`selected_text` plus `steering_id` into the same typed command. It must not
retain a separate selected-text implementation. Empty selection continues down
the plain route path.

## CLI contract

Extend the current `prompt` grammar:

```text
agent-doc prompt <FILE> "<text>"
agent-doc prompt <FILE> --stdin
```

`--stdin` reads multiline text to EOF, supporting
`cat prompt.txt | agent-doc prompt <FILE> --stdin`. `TEXT`, `--stdin`,
`--answer`, and `--all` are mutually exclusive. Empty text is rejected after
checking for non-whitespace content; otherwise content is preserved verbatim
apart from CRLF-to-LF normalization.

For compatibility:

- `agent-doc prompt <FILE>` continues to inspect a harness permission prompt;
- `agent-doc prompt <FILE> --answer N` continues to answer one;
- `agent-doc prompt --all` continues to poll all sessions.

The semantic path belongs behind the Project Controller command, not inside
editor-specific code. CLI, JetBrains, and VS Code are peers that submit the
same command.

## Editor behavior

### JetBrains

`SubmitAction` captures `CommonDataKeys.EDITOR.selectionModel.selectedText`
synchronously before scheduling pooled work. The captured value cannot be
re-read after focus changes.

- non-empty selection: submit `selection_prompt.v1` and await its terminal
  projection;
- empty selection: retain the current save plus `editor_route` behavior.

The existing command registry deduplicates by `command_id`, not only by
document route key, so two intentional selections are not coalesced. Clear
Session Context ordering must preserve pending semantic prompt commands.

### VS Code

`submitAction` captures
`editor.document.getText(editor.selection)` before its first `await`.

- non-empty selection: build the same typed command and await terminal
  `applied`;
- empty selection: retain `startRunForDocument`.

VS Code adds the typed payload to `commandPlane.ts`; it does not add selection
fields to a second editor-only protocol.

## Reactive state

The controller graph makes each edge explicit.

### Sources

- durable `SelectionPromptIntent(command_id, document_id, text, origin)`;
- authoritative exchange-node CRDT projection and generation;
- authoritative document turn projection;
- response capture/finalize intent;
- controller generation and promotion state.

### Computed values

- `PromptAdmission`: `NeedsAppend | Visible(prompt_id, generation) | Settled`;
- `UnresolvedPromptSet`: all admitted prompt IDs without a settling response;
- `PromptDelivery`:
  `IdleDispatch | ActiveSteering | AwaitingAuthority | AlreadyDelivered`;
- `SteeringAggregate`: all unresolved prompts admitted after the active turn's
  baseline, ordered for presentation but modeled as an identity-keyed set;
- `SelectionCommandCompletion`: pending until prompt visibility and delivery
  ownership are both proven.

### Effects

- append one prompt node for a not-yet-visible command;
- route an idle document or wake an active turn;
- attach a captured response to its primary and addressed prompt IDs;
- settle addressed steering identities;
- publish markdown/editor projections;
- resume incomplete effects after controller promotion.

Effects are idempotent by `command_id`, `prompt_id`, and `response_id`.
No timer, plugin callback, queue insertion, response header, or disk adjacency
is authoritative state.

## Rollout

1. **Model and migration**
   - Land stable prompt/response IDs and response edges in the exchange model.
   - Import current markdown into identified nodes and preserve the current
     readable projection.
   - Replace body-hash node identity with occurrence identity plus content hash.

2. **Controller command**
   - Add `selection_prompt.v1`, durable command projection, CRDT receipt, and
     classic selected-text translation.
   - Compute idle dispatch versus active aggregate steering from controller
     turn state.
   - Rehydrate incomplete prompt effects after recycle.

3. **Finalize linkage**
   - Carry admitted/steering prompt IDs through the turn checkpoint.
   - Make finalize attach responses by ID and settle only explicitly addressed
     prompts.
   - Remove heading/adjacency/in-progress-marker inference from this path.

4. **CLI**
   - Extend `agent-doc prompt` with positional text and `--stdin`, preserving
     permission-prompt compatibility modes.

5. **Editors**
   - Wire synchronous selection capture in JetBrains and VS Code.
   - Await the shared terminal command receipt and expose actionable failure or
     pending state without inventing a plugin ACK.

6. **Compatibility cleanup**
   - Remove `legacy_steering_normalized` after both editors ship the typed
     command.
   - Keep the classic translation for one compatibility window, with a
     diagnostic counter proving whether it is still used.

## Dev-harness acceptance matrix

Add these cases to the agent-doc development harness:

- empty editor selection takes the existing whole-document route;
- multiline selection is stored byte-for-byte apart from newline
  normalization and rendered with exactly one prompt prefix;
- idle selection appends the prompt before dispatch;
- active selection becomes steering without starting a second turn;
- several mid-turn selections are delivered together in the aggregate;
- identical text with distinct command IDs creates distinct prompt nodes;
- retry with the same command ID creates one prompt node and one delivery;
- controller recycle after transport acceptance but before CRDT visibility
  resumes to the same terminal `applied` receipt;
- a terminal receipt names a generation that contains the prompt;
- no selection-prompt path inserts or consumes `agent:queue`;
- one response links below its originating prompt;
- one aggregate response settles several addressed prompt IDs without copying
  its body;
- an unaddressed prompt remains unresolved;
- response headings, `(HEAD)`, `❯`, and in-progress markers cannot create or
  change response edges;
- a response can never acquire a prompt prefix through projection or repair;
- deleting or moving visible markdown preserves relationship identity;
- JetBrains captures selection before pooled execution/focus changes;
- VS Code captures selection before its first asynchronous boundary;
- both editors recover terminal state by `command_id` after a dropped socket;
- classic `editor_route.selected_text` translates to the same semantic command
  during the compatibility window;
- existing permission-prompt CLI invocations remain unchanged.

Local release verification should run the Rust full suite, JetBrains plugin
tests, VS Code tests, and structural dev-harness checks. External CI may be
observed afterward but is not a closeout wait condition.
