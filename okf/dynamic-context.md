---
type: concept
tags: [agent-doc, dynamic-context, okf]
---

# Dynamic Context

Dynamic context is the pattern of keeping the hot path small while routing to
bounded resources only when needed. `SKILL.md` owns triggers and invariants,
runbooks own branch procedures, OKF owns durable vocabulary, and generated
commands such as `agent-doc preflight`, `agent-doc plan`, and `tsift` envelopes
own current-state packs.

Policy stays in committed files. Generated context rows or cache entries are
derived artifacts and must carry enough source identity to be invalidated.

## Use When

Load this concept when adding new context sources, splitting oversized
instructions, or deciding whether content belongs in a runbook, an OKF concept,
a generated pack, or the hot-path router.
