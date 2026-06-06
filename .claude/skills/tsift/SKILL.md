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

## Verification

Run `make check` (clippy + full suite) from `src/tsift` before committing; capture the exit explicitly (an rtk wrapper can mask it — grep output for `result: FAILED`). After local changes, check CI with `gh run list --workflow CI --limit 1` and fix failures before calling work complete.

## Runbooks

- [runbooks/command-reference.md](runbooks/command-reference.md) — full command/flag table + global flags.
- [runbooks/conventions.md](runbooks/conventions.md) — read-only SQL/graph, fail-closed scope rules, transactional index updates, lint discovery, edit-intents semantics.
- [runbooks/hook-integration.md](runbooks/hook-integration.md) — auto-reindex, search/digest rewrite, RTK filtering, cross-harness fallback, stale-session recovery.
- [runbooks/internals.md](runbooks/internals.md) — graph module, dependencies, development, versioning, repo.
- [runbooks/search-strategies.md](runbooks/search-strategies.md) — lexical vs hybrid vs vector.

Spec: the normative behavior lives in [src/tsift/SPEC.md](../../../src/tsift/SPEC.md) (index) and its `specs/*.md` siblings.
