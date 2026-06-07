<!-- tsift:opencode-command v=0.1.62 name=tsift-graph -->
---
description: Call graph navigation via tsift graph
---

Navigate the call graph for the symbol named by `$ARGUMENTS` using `tsift graph '<symbol>' --callers` or `tsift graph '<symbol>' --callees`. Use `--callers` to find who calls the symbol, `--callees` to find what the symbol calls. The output lists edges with file locations, edge kinds, and navigation hints. Adjust `--limit` (default 20) to cap edges per direction. For a broader overview including community membership, prefer `tsift --envelope explain '<symbol>' --budget normal`. Fall back to `tsift --envelope search '<symbol>' --budget normal` when the symbol is not found in the index.
