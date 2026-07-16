# Final Response Transactions

The live response is a monotonic document transaction. Lazily is the sole live
buffer authority: the binary may advance that transaction with cumulative,
semantically complete response checkpoints before the turn is sealed.

## Operator-visible progress

- Send concise progress through the harness console/commentary channel.
- Incomplete token prefixes may be retained as crash-recovery evidence, but they
  are never a live-buffer sidecar and never become document authority.
- At a semantic breakpoint, pipe the cumulative complete response through
  `agent-doc response-checkpoint <FILE>`. The binary replaces the prior
  uncommitted response tail in Lazily; it does not append another response.
- A semantic breakpoint is the end of a complete `### Re:` section with balanced
  code fences and component/patch markers. In a multi-response stream, seeing the
  next `### Re:` heading proves that the preceding section is complete. Arbitrary
  token, sentence, timer, or byte-count boundaries are not checkpoints.
- Checkpointing never consumes queue heads, mutates backlog/done, advances the
  cycle to `write_applied`, or commits.

## Final write

1. Finish the complete response, including every required `### Re:` section and
   balanced patch/component marker. Its cumulative text may already be visible
   through one or more response checkpoints.
2. Seal the response transaction through the binary-owned closeout boundary:

```bash
agent-doc respond <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill
```

3. Run `agent-doc session-check <FILE>`. A healthy turn must reach `committed`
   without invoking `agent-doc repair`.

`respond` is the binary-owned turn-resolution command: it seals and commits, but
is not the first document write. `finalize` is a compatibility alias. Harness
integrations should checkpoint completed sections and invoke `respond`
automatically at turn end. `agent-doc write --commit` remains
the explicit missed-patchback/crash-recovery spelling. Bare `agent-doc write`,
including `write --stream`, remains rejected for session responses.

## Atomicity rules

- Response text may become authoritative before closeout, but answered queue-head
  removal, backlog/done mutations, sealing, and commit remain one exact-once
  transaction.
- `AlreadyApplied` is acceptable only as proof that the same cumulative response
  cell is visible. A checkpoint is not proof that closeout mutations or commit ran.
- Save and reuse the immutable preflight baseline. Never re-save the document as a
  new baseline after checkpointing.
- `compact`, `preflight`, and `session-check` must reject malformed component trees
  and inline exchange boundaries. Duplicate response-replay cells or standalone
  protocol boundaries are normalized through Lazily before that generic gate.
