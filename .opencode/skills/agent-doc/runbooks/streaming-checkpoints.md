# Streaming Checkpoints

For long, multi-topic responses, flush partial content at natural breakpoints so the user sees progress in their editor without waiting for the final write.

## When to checkpoint

- After each `### Re:` section in a multi-topic response.
- After completing a code-implementation summary.
- After any response block that takes >15s to generate.

Skip checkpoints for short single-topic responses — one final write is fine.

## How to flush

1. Build the partial response as patch blocks (typically `<!-- patch:exchange -->`).
2. Pipe through `agent-doc write` with `--stream` and the current baseline:

   ```bash
   cat <<'RESPONSE' | agent-doc write <FILE> --baseline-file <baseline_tmp> --stream --origin skill
   <partial patch blocks>
   RESPONSE
   ```

3. **Re-save the baseline** after each flush — the document has changed:

   ```bash
   cp <FILE> /tmp/agent-doc-baseline-$$.md
   ```

4. Continue responding; pass the updated baseline to the next `agent-doc write`.

## Why `--stream`

`--stream` uses CRDT merge — the user's concurrent edits merge without conflicts with the agent's incoming patches. All write-back in this skill uses `--stream`; there is no non-stream path for checkpoints.

## Baseline rules

- **First write in a cycle:** use `preflight.baseline_file` (a post-commit snapshot).
- **Subsequent checkpoints:** re-save from the current file after each successful flush. A stale baseline means the CRDT merge re-applies old content.
