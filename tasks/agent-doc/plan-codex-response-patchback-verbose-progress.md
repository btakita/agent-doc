# Plan: keep Codex progress chatter out of response patchback

## Problem

Direct `agent-doc <file>` Codex child runs can patch back a very verbose
response into a template document because the Codex backend concatenates every
JSONL `item.completed` event whose item type is `agent_message`.

Evidence:

- Document: `src/boost-client/tasks/monsterrodholders.md`
- Commit: `a4ac31b agent-doc(monsterrodholders): 2026-05-12 22:40:26`
- Symptom: the response for `Read the image. ![img_7.png](img_7.png)` includes
  the useful final closeout, then appends many intermediate status updates such
  as "I'm opening it now", "The server confirms the mismatch", "I'm pushing it
  now", and similar progress prose.
- Code path: `src/agent-doc/src/agent/codex.rs` `send_once()` appends every
  `item.completed` `agent_message` into `response_text`.
- Write path: `src/agent-doc/src/run.rs` writes `response.text` directly through
  the template response pipeline, so the overly broad capture becomes durable
  patchback content.

The verbosity is a capture-selection bug, not a Markdown merge bug. The
template writer is preserving the response body it was handed.

## Desired Behavior

1. The patchback body should contain the final assistant closeout for the active
   turn, not progress updates or replayed transcript messages.
2. Progress/status messages may still appear in the live console while work is
   running.
3. Durable capture and repair should store/replay the same filtered closeout
   body that was written to the document.
4. If the Codex JSONL stream does not expose a reliable final-closeout boundary,
   `agent-doc` should fail closed with diagnostics rather than committing a
   transcript-shaped response.

## Investigation Steps

- [ ] Capture a minimal `codex exec --json` transcript where a child agent emits
      multiple progress messages plus one final response.
- [ ] Identify which JSONL fields distinguish the active turn's final response
      from intermediate `agent_message` items, replayed transcript items, and
      commentary/status messages.
- [ ] Check whether `turn.completed`, item IDs, roles, output indices, or event
      ordering are stable enough to select exactly one final response.
- [ ] Compare the non-streaming `send_once()` path with the streaming parser so
      both paths use the same response-selection contract.

## Implementation Steps

- [ ] Replace naive concatenation in `Codex::send_once()` with an explicit
      response accumulator that selects only the current turn's final assistant
      closeout.
- [ ] Add a guard that rejects transcript-shaped payloads containing multiple
      progress/status messages when no final boundary can be proven.
- [ ] Keep console progress behavior unchanged; only the document patchback body
      should be filtered.
- [ ] Make capture/replay use the filtered body so interrupted cycles do not
      replay the verbose raw transcript.

## Regression Coverage

- [ ] Unit-test Codex JSONL parsing with several `agent_message` events where
      only the final closeout is written to `AgentResponse.text`.
- [ ] Regression-test the Monster Rod Holders shape: final closeout plus
      progress updates must patch back only the closeout.
- [ ] Add a negative test for ambiguous multiple `agent_message` payloads with
      no final-response proof; the run should fail closed before write/commit.
- [ ] Verify streaming and non-streaming Codex paths agree on the selected final
      response body.

## Exit Criteria

- `agent-doc <file>` no longer commits child Codex progress chatter into the
  session document.
- The response capture ledger stores only the final closeout body.
- Verbose transcript-shaped Codex output is rejected or filtered deterministically.
