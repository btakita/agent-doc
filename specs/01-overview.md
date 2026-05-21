> Extracted from SPEC.md — see [index](../SPEC.md)

# Overview

agent-doc manages interactive document sessions between a human and an AI agent.
The human edits a markdown document, sends diffs to the agent, and the agent's
response is appended. Session state is tracked via YAML frontmatter, snapshots,
and git commits.

## Taxonomy

- **Agent docs** are markdown session documents that act as the user interface.
  They contain frontmatter, exchange history, queues, tracked work components,
  and closeout state. The user edits these directly.
- **Runbooks** are durable operator instructions for recurring workflows. They
  explain how a harness or human should use the binary, but deterministic
  document mutation rules still belong in Rust.
- **Plans** are design or implementation notes linked from backlog items. They
  are living intent records until the item is complete, then remain historical
  context for why a change was made.
- **Job packets** are bounded worker contracts generated from a parent session
  cycle. They define write scope, required proof, context handles, and the
  result schema for delegated work.
- **Operation docs** or **opdocs** are retained audit artifacts for one parent
  operation. An opdoc records the dispatch decision, packet set, collection
  commands, verification evidence, and parent review result. Job packets are
  inputs to the opdoc; the opdoc is the durable parent-level audit trail.
