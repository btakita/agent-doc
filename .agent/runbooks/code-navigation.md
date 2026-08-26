<!-- tsift:code-navigation-runbook v=0.1.88 -->
# Code Navigation

Managed by `tsift init` (versioned markers) — do not hand-edit between the markers; re-run `tsift init` to refresh. Text outside the markers is preserved.

This runbook is the detail behind the `Code Navigation` block in `AGENTS.md`. That block carries the hot path; everything below is the full command surface.

## Session start

Run `tsift status` from the owning repo root. If the task or file lives under a git submodule (for example `src/tsift/...`), switch to that submodule root first so the harness loads the narrower local instructions and repo state instead of the superproject root. `tsift status` repairs the `.tsift/` index state it owns and never rewrites tracked files (`--no-fix` skips even that). If status reports stale or missing instructions, run `tsift init` to refresh the tracked Code Navigation block and runbook; it names every tracked file it rewrites or moves. When the harness cannot perform write commands, ask the user to run the printed `run:` command instead.

Codex projects can install a prompt-time auto-reindex hook with `tsift init --codex`; OpenCode projects can install per-project tsift command shortcuts with `tsift init --opencode`.

## Search, read, and graph

Use the commands listed in `tsift status`'s `use:` output:

- `tsift --envelope source-read <file> --budget normal` — AST-symbol projection with span metadata and source-window expansion commands (prefer over raw cat/head/tail/sed/less reads for source files)
- `tsift --envelope symbol-read <symbol> --budget normal` — token-budgeted symbol body, AST span metadata, child refs, and graph/source expansion commands
- `tsift --envelope search <query> --budget normal` — AST-aware hybrid search preview (prefer over grep/rg)
- `tsift --envelope explain <symbol> --budget normal` — callers, callees, community preview
- `tsift graph <symbol> --callers` / `--callees` — call graph navigation
- `tsift summarize <symbol>` — cached summary (only when listed in `use:`)
- `tsift workflow search` — ordered exact/search/explain/summarize/digest recipe that preserves result handles across expansions

When a search envelope includes `report.scale_guard`, run one of its `narrow_commands` before dispatching parallel agents. The guard means the original result set or corpus is broad enough that fan-out should start from a narrower cited handle, path, or exact query.

## Bounded digests instead of raw output

Prefer bounded digest commands over raw transcript, diff, and verbose-log reads:

- `tsift --envelope session-review <path> --next-context` / `tsift --envelope context-pack <path>` when a resumable handoff is the goal
- raw-read rewrites route recognized agent-doc/JSONL sessions to `tsift session-digest --input <path>` and captured logs to `tsift log-digest --input <path>` instead of replaying them with `cat`, `head`, `tail`, `sed`, or `less`
- `tsift diff-digest [path]` (`--cached`, `--revision <rev>`, repeatable `--pathspec <pathspec>`) instead of `git diff`, commit-form `git show`, or patch-style `git log`. Blob-form `git show <rev>:<path>` is an object read and is deliberately not rewritten.
- `tsift --envelope digest-runner --kind test --path . --shell-command '<test command>'` / `tsift --envelope digest-runner --kind log --path . --shell-command '<build command>'` for noisy test/build/install output, or let the rewrite/hooks create those artifact-backed envelopes for `cargo test`, `pytest`, and verbose cargo commands.
- If RTK is installed, digest-runner delegates supported generic command families through `rtk rewrite` and records the chosen compact filter in `report.filter` while preserving tsift artifact handles.

## Harnesses without `PreToolUse` hooks

Codex, OpenCode, and other harnesses without Claude-style `PreToolUse` hooks should run `tsift rewrite --run '<command>'` before broad `rg`/recursive grep, raw transcript/session/log reads, `git diff`/`git show`/single-patch `git log`, `cargo test`/`pytest`, and cargo build/check/clippy/install commands so the same search, session-digest, diff-digest, and digest-runner rewrites apply manually. OpenCode can install this path as `/tsift-rewrite-run` with `tsift init --opencode`.

## Verification

For local verification, run `make check` before committing. After local changes, check the latest GitHub Actions CI run with `gh run list --limit 1` and fix any failing tests before calling the work complete.

Only read full source files when tsift results are insufficient.
<!-- /tsift:code-navigation-runbook -->
