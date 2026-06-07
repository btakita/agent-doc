<!-- tsift:opencode-command v=0.1.62 name=tsift-symbol-read -->
---
description: Read symbol body with AST metadata via tsift symbol-read
---

Read the symbol named by `$ARGUMENTS` using `tsift --envelope symbol-read '<symbol>' --budget normal`. Prefer this over reading entire source files when you need a specific function, struct, or type definition. The envelope returns the symbol body, AST span metadata, child references, and expansion commands for graph/source navigation. When `$ARGUMENTS` includes a file hint, pass it as `--file '<path>'` to disambiguate duplicate names. Use the returned `expand` commands to inspect callers, callees, or the full source file. Fall back to `tsift --envelope source-read '<file>' --start <n> --lines <n> --budget normal` when the symbol is not found or when you need raw source without AST context.
