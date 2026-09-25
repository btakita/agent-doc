---
name: tsift
description: "Use tsift for token-efficient repository navigation, code search and reading, call graphs, diffs, logs, tests, session context, and workspace memory. TRIGGER: exploring or changing a codebase with tsift installed, or working on the tsift repo. SKIP: plain file-level glob, non-code web search. VERSION CHECK: compare `tsift --version` against tsift-version below before trusting any command text copied from this file."
user-invocable: true
argument-hint: "[query or symbol]"
tsift-version: "0.1.101"
---
<!-- tsift:skill v=0.1.101 -->
# tsift

**Check the version first.** Run `tsift --version` and compare it to `tsift-version` in the frontmatter above. If the binary is newer, this file is stale: treat its command text as a hint, not a contract, and read the live surface from `tsift --help` / `<subcommand> --help`. `tsift init` refreshes this file and stamps both values from the installed binary, so a mismatch means the skill was never refreshed after an upgrade. `tsift audit` reports the same drift as an issue.

## Command surface

`search`, `symbol-read`, `source-read`, `explain`, `graph`, `communities`, `path`, `index`, `status`, `locks` — search and navigation. `traverse`, `graph-db`, `convex-sync`, `conflict-matrix`, `dispatch-trace`, `dependency-dag` — graph substrate. `edit`, `edit-intents`, `ast-grep` — batch and semantic editing. `diff-digest`, `test-digest`, `log-digest`, `metric-digest`, `session-digest`, `session-cost`, `session-review`, `context-pack`, `digest-runner` — bounded digests and session context. `summarize`, `semantic`, `lint`, `audit`, `audit-tagpath` — cached analysis and drift checks. `route`, `rewrite`, `sql`, `memory`, `init`, `workflow` — tooling. Global flags: `--envelope`, `--compact`, `--terse`, `--ultra-terse`, `--schema`, `--tabular`, `--absolute`, `--pretty`.

## Session start

Run `tsift status` at session start from the owning repo root. If the task or file lives under a git submodule (for example `src/tsift/...`), switch to that submodule root first so the harness loads the narrower local instructions and repo state instead of the superproject root. `tsift status` repairs the `.tsift/` index state it owns and never rewrites tracked files (`--no-fix` skips even that). If status reports stale or missing instructions, run `tsift init` to refresh the repository-local tsift skill and its reference; it names every tracked file it rewrites or moves. When the harness cannot perform write commands, ask the user to run the printed `run:` command instead.

Prefer tsift envelopes over raw reads:
- `tsift --envelope search <query>` instead of `grep`/`rg`
- `tsift --envelope source-read <file>` / `tsift --envelope symbol-read <symbol>` instead of raw `cat`/`head`/`tail`/`sed`/`less` source reads
- `tsift --envelope explain <symbol>` and `tsift graph <symbol> --callers` / `--callees` for call graphs
- `tsift diff-digest [path]` (`--pathspec <pathspec>` to preserve scoped reviews) instead of `git diff`, commit-form `git show`, or patch-style `git log`; blob-form `git show <rev>:<path>` stays a raw object read
- `tsift --envelope session-review <path>` / `tsift --envelope context-pack <path>` instead of replaying long session docs or transcripts
- raw-read rewrites route recognized session docs/transcripts to `tsift session-digest --input <path>` and captured logs to `tsift log-digest --input <path>`
- `tsift --envelope digest-runner --kind test|log --path . --shell-command '<command>'` instead of raw test/build output

**No rewrite hook? Drive the list above yourself.** Only a `PreToolUse`-equipped harness redirects `cat`/`grep`/`git diff` for you. Everywhere else nothing intercepts them, so issue these commands directly instead of waiting for a rewrite; the reference below names the one-shot equivalent and the per-harness setup, `--harness` included.

Command detail lives in [`references/code-navigation.md`](references/code-navigation.md) — budgets, `tsift workflow search`, `report.scale_guard` handling, the harness rewrite path for `PreToolUse`-less harnesses, and Codex/OpenCode integration. `tsift init` writes and versions that reference alongside this skill, so it is present in every initialized checkout; read it before broad exploration instead of expanding this file.

For local verification, run `make check` before committing. After local changes, check the latest GitHub Actions CI run with `gh run list --limit 1` and fix any failing tests before calling the work complete.

Only read full source files when tsift results are insufficient.
<!-- /tsift:skill -->
