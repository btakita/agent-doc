<!-- tsift:opencode-command v=0.1.62 name=tsift-source-read -->
---
description: AST-aware source code reading via tsift source-read
---

Read source code using `tsift --envelope source-read <file> --start <n> --lines <n> --budget normal`, where `<file>` is `$ARGUMENTS` or the file the user wants to inspect. Prefer this over the raw Read tool for source code files (Rust, TypeScript, JavaScript, Python, Markdown, and other indexed languages). The envelope returns a bounded source window with AST symbol metadata, line previews, and expansion commands for before/after/full-file ranges. When `$ARGUMENTS` includes a line range, parse `start` and `lines` from it; otherwise default to `--start 1 --lines 80`. Use the returned `expand` commands to read adjacent ranges instead of re-reading the entire file. Fall back to the raw Read tool only for non-indexed files or binary assets.
