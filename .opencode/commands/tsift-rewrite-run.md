<!-- tsift:opencode-command v=0.1.69 name=tsift-rewrite-run -->
---
description: Run a shell command through tsift rewrite
---

Run the shell command named by `$ARGUMENTS` through `tsift rewrite --run '<command>'`. Use this for broad `rg`/recursive `grep`, raw transcript/session/log reads, `git diff`/`git show`/single-patch `git log`, `cargo test`/`pytest`, and cargo build/check/clippy/install commands so Codex/OpenCode get the same bounded search, session-digest, diff-digest, and digest-runner path as the Claude hook. If tsift reports no rewrite, do not retry automatically; summarize the reason and run the original command only when the user still needs exact raw output.
