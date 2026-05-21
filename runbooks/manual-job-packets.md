# Manual Job Packets

Use this runbook when a parent `agent-doc` turn should split bounded work for
lower-tier workers without giving them the whole session document.

## Create Packets

1. Run `agent-doc preflight <FILE>` and `agent-doc plan <FILE>`.
2. Confirm the plan has `dispatch_candidate=true`, concrete `write_scope`, and
   required proof fields.
3. Generate packets with:

```sh
agent-doc jobs create <FILE> --operation-doc
```

Packets are written under `.agent-doc/jobs/<cycle>/`. The optional operation
doc records the parent decision and collection commands.

## Taxonomy

- **Job packet:** a worker-facing contract for one bounded task. It owns write
  scope, context handles, required proof, and the worker result schema.
- **Operation doc / opdoc:** a parent-facing audit artifact for the whole
  operation. It references the generated packets, records why dispatch was used,
  preserves collection commands, and captures verification plus parent review.
- **Plan:** a design note linked from backlog work. A plan can justify a packet,
  but it is not itself a worker contract unless copied into a job packet.
- **Runbook:** reusable procedure for humans or harnesses. This file is a
  runbook; generated packets and opdocs are per-cycle artifacts.

Keep opdocs. They are retained evidence, not scratch space: after workers finish,
the parent should collect results into the opdoc and cite the verification used
to accept, revise, or reject each packet result.

## Packet Rules

- Treat the packet frontmatter as the worker contract.
- The worker may edit only files in `write_scope`.
- Missing or stale tsift context is not permission to guess; the worker records
  it in `needs_parent_attention` or escalates.
- The parent reviews all worker results and runs required verification before
  finalizing the session document.

## Worker Result

Workers save `<job-id>.result.json` next to the packet, or paste a JSON object
under `## Worker Result` in the packet:

```json
{
  "contract_version": "agent-doc-worker-result-v1",
  "status": "complete|blocked|escalate",
  "changed_paths": [],
  "commands_run": [],
  "findings": [],
  "proof": [],
  "confidence": "low|medium|high",
  "needs_parent_attention": []
}
```

The parent collects results with:

```sh
agent-doc jobs collect <FILE> --json
```

## Examples

Search job: `write_scope` is empty or read-only, expected output is findings
plus source handles.

Test triage job: `write_scope` includes tests or fixtures, expected output is
the failing command, minimized failure summary, and proposed fix boundary.

Small patch job: `write_scope` is one file or one module, expected output is
changed paths, focused verification, and confidence.

Spec update job: `write_scope` includes `specs/` or `runbooks/`, expected output
links the changed spec text to the behavior or test that proves it.
