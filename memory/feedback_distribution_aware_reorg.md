---
name: Distribution-aware file reorganization
description: Before suggesting file moves, verify the distribution/install path still works — co-located files that ship together must stay together
type: feedback
---

Before suggesting file reorganization, check: how are these files delivered to their consumers? If files are installed/distributed as a unit (skill install, cargo publish, npm pack), they must stay co-located. Centralization only works when the consumer can resolve cross-boundary references at runtime.

**Why:** Suggested moving skill runbooks to a centralized repo, but skills distribute runbooks via `agent-doc skill install` using relative paths. The move would have broken the install workflow.

**How to apply:** When proposing file moves, ask "If I move this file, does the install/deploy path still work without adding new dependencies or cross-repo references?" If yes, the move is safe. If no, the files belong where they are.
