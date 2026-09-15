---
description: "Primary source for the tsift code-RAG CLI: AST-aware search, call-graph queries, the graph substrate, digests/session-review, batch/semantic editing, and model routing. TRIGGER: user invokes /tsift or asks to search/explore/navigate a codebase with tsift, or work on the tsift repo. VERSION CHECK: compare the installed tsift binary against tsift-version below before relying on copied text."
user-invocable: true
argument-hint: "[query or symbol]"
tsift-version: "0.1.62"
---

# tsift

Token-efficient CLI for code agents — AST-aware search, call-graph queries, a provider-neutral graph substrate, batch + semantic editing, bounded digests/session-review, SQL introspection, and model-tier routing.

This skill is the **primary agent-facing source** for tsift. It carries the full command surface here and dispatches to [runbooks/](runbooks/) for detail so a turn loads only what it needs. The submodule's `src/tsift/AGENTS.md` (and its `CLAUDE.md` symlink) defer to this skill; for standalone tsift checkouts, fall back to `tsift --help` and `src/tsift/VERSIONS.md`.

## Version & Freshness Check

- Run `tsift --version`; compare to `tsift-version` above. If the binary is newer, treat copied text as stale — prefer `tsift --help` / subcommand `--help` for the live surface, and refresh from `src/tsift/VERSIONS.md`. Keep `tsift-version` aligned with `src/tsift/Cargo.toml`.
- Run `tsift status` at session start before relying on `search`, `explain`, or `graph`. If it reports `index: stale` or `use: (none …)`, run `tsift status --fix` (or `tsift index .`). If `tsift search` times out with "The index may be stale" (common in resumed tmux sessions), run `tsift index .` and retry. Reserve `--timeout 0` for intentional uncapped searches on a fresh index.

## TRIGGER

- Search code, find symbols, navigate callers/callees, communities, or shortest paths with `/tsift`.
- Explore architecture / build a graph map / get a resumable context pack for a codebase.
- Work on the tsift repo itself (`src/tsift/...`).

## SKIP

- Plain grep/ripgrep (handled by the rtk/tsift rewrite hook), file-level search (Glob/Grep), non-code search (WebSearch).

## Prerequisites

Binary: `tsift` (installed via `cargo install --path src/tsift`). Run from the owning repo/submodule root; under a submodule (e.g. `src/tsift/...`) switch to that root first so the narrower local index/instructions load.

## Command surface

Prefer envelope previews and bounded digests over raw reads. Full flag detail and fail-closed semantics: [runbooks/command-reference.md](runbooks/command-reference.md).

**Search & navigation** — `search` (hybrid BM25+vector; `--strategy lexical|hybrid|vector`, `--scope`/`--federated`), `symbol-read`, `explain`, `graph` (`--callers`/`--callees`), `communities` (Louvain), `path` (BFS shortest path), `index` (`--workspace`/`--submodule`/`--check`/`--exit-code`), `status`, `locks`.

**Graph substrate** — `traverse`, `graph-db` (`map`, `refresh`, `status`, `evidence`, `related`, `neighborhood`, `doctor`, `compact`, `backend-eval`), `convex-sync`, `conflict-matrix`, `dispatch-trace`, `dependency-dag`. See [src/tsift/specs/graph.md](../../../src/tsift/specs/graph.md).

**Editing** — `edit` (atomic JSON batch), `edit-intents` (semantic code + Markdown intents with `--verify`/`--apply`).

**Digests & sessions** — `diff-digest`, `test-digest`, `log-digest`, `metric-digest`, `session-digest`, `session-cost`, `session-review` (`--next-context`), `context-pack`.

**Cached analysis & lint** — `summarize` (cached LLM analysis), `lint` (markdown concept lint), `audit`, `audit-tagpath`.

**Tooling** — `route` (task→model tier), `rewrite` (`--run` executes the envelope-first tsift equivalent for harnesses without `PreToolUse` hooks), `sql`, `init` (`--codex`/`--opencode`/`--workspace`).

Global flags: `--compact`, `--envelope`, `--pretty`, `--terse`, `--schema`, `--absolute`, `--tabular`.

When a search envelope includes `report.scale_guard`, run one of its `narrow_commands` before fanning out parallel agents.

## Pure sessions (no `/agent-doc`, no hooks) — use tsift manually

This skill applies in **every** session, not just hook-equipped Claude Code. In a pure Claude, Codex, or OpenCode session there is no `PreToolUse` rewrite hook to redirect `cat`/`grep`/`git diff` for you, so **drive the envelope-first commands yourself** to keep token cost down:

- **Session start:** run `tsift status` from the owning repo (or submodule) root; if it prints a `run:` recommendation or `index: stale`, run `tsift status --fix` (or `tsift index .`). When the harness can't perform write commands, ask the user to run the printed command.
- **Reading source:** `tsift --envelope source-read <file> --budget normal` and `tsift --envelope symbol-read <symbol> --budget normal` instead of `cat`/`head`/`sed` on code.
- **Searching:** `tsift --envelope search <query> --budget normal` instead of `rg`/`grep -r`; `tsift --envelope explain <symbol> --budget normal` and `tsift graph <symbol> --callers|--callees` for navigation.
- **Diffs / sessions / noisy output:** `tsift diff-digest [path]` instead of `git diff`/`git show`/`git log -p`; `tsift --envelope session-review <path> --next-context --budget normal` (or `context-pack`) instead of replaying long session docs / JSONL transcripts / runtime logs; `tsift --envelope digest-runner --kind test|log …` for `cargo test`/`pytest`/verbose cargo output.
- **One-shot equivalent:** `tsift rewrite --run '<command>'` executes the same envelope-first, artifact-backed tsift rewrite for any supported command — the manual stand-in for the missing hook. It covers broad search, raw transcript/session/log reads, git diff/show/patch-log, cargo/pytest tests, and cargo build/check/clippy/install. If RTK is installed and supports the wrapped command, the output filter delegates to RTK while preserving tsift artifact handles.

**Per-harness setup so the rewrites are one keystroke:**
- **Claude Code:** install the `UserPromptSubmit` auto-reindex + `PreToolUse` rewrite hooks (see [runbooks/hook-integration.md](runbooks/hook-integration.md)) so the rewrites happen transparently.
- **Codex:** `tsift init --codex` injects the prompt-time auto-reindex hook into `.codex/hooks.json`. Codex has no `PreToolUse` rewrite, so still run `tsift rewrite --run '<command>'` before broad search / raw reads / diffs / tests.
- **OpenCode:** `tsift init --opencode` installs project-local command shortcuts including `/tsift-rewrite-run`; use it (or `tsift rewrite --run`) in place of the missing hook.

Fall back to raw `cat`/`grep` only when a tsift result is genuinely insufficient.

## Verification

Run `make check` (clippy + full suite) from `src/tsift` before committing. Preserve the command's exit status explicitly across wrappers and pipes; do not replace it with a generic output grep, because unrelated runners may never emit that string. When the runner exposes a failure tally, independently assert its runner-native count as a second proof. After local changes, check CI with `gh run list --workflow CI --limit 1` and fix failures before calling work complete.

## Runbooks

- [runbooks/command-reference.md](runbooks/command-reference.md) — full command/flag table + global flags.
- [runbooks/conventions.md](runbooks/conventions.md) — read-only SQL/graph, fail-closed scope rules, transactional index updates, lint discovery, edit-intents semantics.
- [runbooks/hook-integration.md](runbooks/hook-integration.md) — auto-reindex, search/digest rewrite, RTK filtering, cross-harness fallback, stale-session recovery.
- [runbooks/internals.md](runbooks/internals.md) — graph module, dependencies, development, versioning, repo.
- [runbooks/search-strategies.md](runbooks/search-strategies.md) — lexical vs hybrid vs vector.

Spec: the normative behavior lives in [src/tsift/SPEC.md](../../../src/tsift/SPEC.md) (index) and its `specs/*.md` siblings.
