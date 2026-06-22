<!-- tsift:opencode-command v=0.1.69 name=tsift-explain -->
---
description: Explain a symbol via callers, callees, and community preview
---

Explain the symbol named by `$ARGUMENTS` using `tsift --envelope explain '<symbol>' --budget normal`. Prefer this when you need callers, callees, or community context for a function, struct, or type. The envelope returns ranked caller/callee lists with file locations, community membership, and expansion commands for graph traversal. When the report includes a `scale_guard`, run one of its `narrow_commands` before dispatching parallel agents. Use `tsift graph '<symbol>' --callers` or `--callees` for full call-graph navigation. Fall back to `tsift --envelope search '<symbol>' --budget normal` when the symbol is not found in the index.
