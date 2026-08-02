# Plan: Realtime salient response nodes

Status: implementation contract for `#realtimesalient`

## Architecture-first contract

**Invariant:** only structurally complete, explicitly declared salient progress may
enter the live document before finalize, and one final response atomically
removes that cycle's progress node before becoming authoritative.

**Policy owner:** `agent_doc_turn::salient_response` owns structural admission;
`agent_doc_markdown_ast::exchange_tree` owns the distinct node and replacement
model; the Project Controller response-cell endpoint owns the CRDT mutation.

**Transition table:**

| Cycle | Candidate | Existing node | Decision |
|---|---|---|---|
| not open | any | any | reject |
| open | empty / protocol-bearing / unbalanced fence | any | reject |
| open | eligible | absent | append one cycle-keyed salient node |
| open | same eligible content | identical | no-op |
| open | newer eligible content | present | replace that node |
| final response | any | present | remove salient node, then add final response |
| final response replay | any | stale salient node | remove stale node without duplicating final response |

**Evidence inputs:** cycle ID and phase, declared text, structural fence/marker
scan, controller canonical document, and CRDT delivery receipt.

**Reactive topology:** open-cycle projection and the controller canonical are
Sources; structural admission and the cycle-keyed desired salient node are
Computed decisions; controller CRDT replacement is the Effect; the relay write
and editor delivery acknowledgement are receipt Sources. The CLI is a one-shot
boundary adapter because semantic salience is an agent decision, not a
long-lived derived fact.

**Imperative extraction audit:** no timer or byte-count heuristic decides
salience. Existing partial-token capture remains crash evidence only. The new
command publishes an explicit semantic observation; controller application
derives append/replace/no-op from canonical state. Finalize does not separately
remember to clean progress: response-cell insertion always removes it.

**Allowed edit surfaces:**

- `agent-doc-turn`: pure structural eligibility.
- `agent-doc-markdown-ast`: salient node parsing/rendering/upsert/removal.
- `agent-doc-merge`: final response replacement invariant.
- `agent-doc-crdt-relay-io` and `agent-doc-controller-io`: one controller-owned
  semantic mutation and delivery receipt.
- `agent-doc-write-runtime-io` and CLI: one-shot command adapter.
- `SKILL.md`, response/streaming runbooks, specs, and dev-harness boundary tests:
  tell harnesses when to publish salient checkpoints.

**Verification:** pure policy rows; exchange byte-stability and
append/replace/remove tests; response-cell finalization/replay tests; controller
and CLI boundary tests; full local `make check`.

**Out of scope:** automatic token classification, timer-based document writes,
committing progress nodes, queue consumption at checkpoint time, or replacing
the existing complete-response checkpoint API.

## Salience rule

A salient checkpoint is a standalone result that remains useful if the turn
stops immediately: a confirmed diagnosis, an architecture decision, a verified
implementation outcome, or another complete conclusion. “Still working,”
guesses, promises, raw tool output, arbitrary sentences, and incomplete token
prefixes are not salient.

The agent declares semantic salience. The binary rejects structurally unsafe
content: empty input, agent-doc protocol markers, and unbalanced Markdown code
fences.

## Projection

The exchange projection uses one cycle-keyed block:

```md
<!-- agent:salient-response cycle="cycle-id" -->
#### Live response (not final)

Standalone salient text.
<!-- /agent:salient-response -->
```

The exchange parser classifies the whole block as `Salient`, even when its body
contains headings or prompt-like text. It is never a Prompt or Response node,
never proves that a queue head was answered, and is never committed as the final
answer.

`agent-doc salient-checkpoint <FILE>` reads the declared salient text from
stdin. Repeated calls replace the same cycle node. `response-checkpoint`
continues to accept cumulative complete `### Re:` sections; the two APIs are
not aliases.
