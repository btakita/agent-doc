# Final Response Transactions

Streaming is a generation and recovery mechanism, never a document-persistence
mechanism. Buffer partial model output outside the document and publish exactly one
complete response after generation finishes.

## Operator-visible progress

- Send concise progress through the harness console/commentary channel.
- The runtime may save partial text to recovery-only sidecars. Those checkpoints are
  not document authority and cannot satisfy response placement, queue consumption,
  or closeout.
- Never append a partial `### Re:` section, code block, patch block, or response
  prefix to the session document.

## Final write

1. Finish the complete response, including every required `### Re:` section and
   balanced patch/component marker.
2. Pipe the complete payload once through the binary-owned closeout boundary:

```bash
agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
```

3. Run `agent-doc session-check <FILE>`. A healthy turn must reach `committed`
   without invoking `agent-doc repair`.

`agent-doc write --commit` is reserved for an explicit missed-patchback or crash
recovery. Bare `agent-doc write`, including `write --stream`, is rejected for session
responses before stdin, capture, or document mutation.

## Atomicity rules

- The final response, answered queue-head removal, and backlog/done mutations are
  one transaction. None may become authoritative before all validate.
- `AlreadyApplied` is acceptable only as exact proof that the complete final payload
  from the same transaction is visible. A prefix or earlier checkpoint never counts.
- Save and reuse the immutable preflight baseline. Never re-save the document as a
  new baseline after partial generation.
- `compact`, `preflight`, and `session-check` must reject malformed component trees
and repeated/inline exchange boundaries; they are not repair or commit escape hatches.
