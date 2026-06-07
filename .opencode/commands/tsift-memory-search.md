<!-- tsift:opencode-command v=0.1.62 name=tsift-memory-search -->
---
description: Search first-party tsift memory graph
---

Run `tsift graph-db --path . --json related '<query>'`, where `<query>` is `$ARGUMENTS`; ask for a query if `$ARGUMENTS` is empty. Summarize semantic readiness, useful memory/source hits, and any refresh/import fallback commands. Prefer tsift-memory/graph-db retrieval and do not call direct claude-mem or `/mem-search`; claude-mem remains only a fallback import source through `tsift memory import-claude-mem` when graph memory is missing.
