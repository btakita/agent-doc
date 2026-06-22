<!-- tsift:opencode-command v=0.1.69 name=tsift-source-read -->
---
description: AST-aware source code reading via tsift source-read
---

Read source code using `tsift --envelope source-read <file> --budget normal`, where `<file>` is `$ARGUMENTS` or the file the user wants to inspect. Prefer this over the raw Read tool for source code files (Rust, TypeScript, JavaScript, Python, Markdown, and other indexed languages). The envelope returns an AST-symbol projection with stable span metadata, `symbol-read` expansion commands for bodies, and `expand.window` commands for literal line previews. When `$ARGUMENTS` includes a line range, pass it as `--start <n> --lines <n>` to bound the AST projection. Add `--style window` only when the user needs numbered source lines. Fall back to the raw Read tool only for non-indexed files or binary assets.
