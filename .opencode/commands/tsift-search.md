<!-- tsift:opencode-command v=0.1.62 name=tsift-search -->
---
description: AST-aware content search via tsift search
---

Search code using `tsift --envelope search '<query>' --budget normal`, where `<query>` is `$ARGUMENTS`. Prefer this over grep/rg for content search in indexed projects. The envelope returns ranked search hits with symbol families, file previews, AST-aware scoring, and expansion commands. When the report includes a `scale_guard`, run one of its `narrow_commands` before dispatching parallel agents. Use `tsift workflow search` for the ordered exact/search/explain/summarize/digest recipe that preserves result handles across expansions. Fall back to grep/rg only when the project is not indexed or for non-code file patterns (e.g. glob-only searches).
