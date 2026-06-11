---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-233419.md`*

Compacted content:
- Archived 7 response topic(s): queue duplicate cleanup; codebase-memory-mcp evaluation; graph-db snapshot artifact prototype; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
- `cargo test -p tsift-cli --test exit_code session_review_next_context_collapses_noop_closeout_guidance`
- Pushed `btakita/agent-loop@fae1d9254` (`chore: update tsift submodule`) to `origin/main`.
- Pushed `btakita/tsift@756b44d` (`test: cover graph db doctor lock recovery`) to `origin/main`.
- `tsift --version` -> `tsift 0.1.67`
- `cargo install --path . --locked`
- `cargo build`
- `make check`
- `cargo test -p tsift-cli graph_db_status_copies_wal_sidecars_when_locked`
- `cargo test -p tsift-cli graph_db_status_uses_snapshot_fallback_when_rollback_journal_is_locked`
- `cargo test -p tsift-cli graph_db_doctor_reports`
- `cargo fmt -p tsift-cli`
- Added an Unreleased `#tsbuglock` note.
- Both tests refresh a real traversal graph DB, hold a deterministic SQLite lock, run `append_sqlite_graph_doctor_checks`, and assert doctor stays `ok` while emitting `sqlite_graph_db_read_recovery` with the rollback-journal or WAL-aware snapshot fallback diagnostic.
- Added `graph_db_doctor_reports_wal_snapshot_fallback_when_locked`.
- Added `graph_db_doctor_reports_snapshot_fallback_when_rollback_journal_is_locked`.
- `src/tsift` submodule pointer in the superproject
- `src/tsift/VERSIONS.md`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `cargo test -p tsift-cli --test exit_code session_review_next_context_collapses_actionable_guardrail_failures`
- `cargo test -p tsift-cli session_review_next_context_budget_limits_lists`
- `tsift --envelope source-read ...` / `rg ...` for the token-action implementation, spec, archived done rows, and tests.
- `tsift status` from `src/tsift`; fresh.
- `tsift status` from the superproject; it refreshed submodule indexes but reported one unrelated missing workspace scope.
- `packages/tsift-cli/tests/exit_code.rs` has fixture-backed CLI coverage for actionable guardrail collapse: prompt budget, cache resend, restart loop, and no-op closeout move into `next_token_actions` and disappear from unresolved guardrail failures.
- `packages/tsift-cli/src/lib.rs` has unit coverage asserting that the full action report orders those six action kinds exactly once and carries raw-read and command-bundle rewrite commands.
- `packages/tsift-cli/src/session_review_budget.rs` builds one deduped, priority-sorted action list for `prompt_budget`, `cache_resend`, `repeated_raw_read`, `repeated_command_bundle`, `restart_loop`, and `noop_closeout`, including digest commands and rewrite commands.
- `src/tsift/specs/digests-sessions.md` specifies that `session-review --next-context` emits prioritized `next_token_actions`, maps prompt-budget/cache-resend/restart-loop/no-op closeout guardrails to compact/restart/digest/rewrite commands, and adds `repeated_raw_read` plus `repeated_command_bundle` actions with exact rewrite/source-read commands.
- `tasks/software/tsift.done.md` already records `#token-budget-guardrail`, `#token-action-coverage`, and `#token-actions-rewrite` as completed work covering the same prompt-budget/cache-resend/restart-loop/no-op closeout and rewrite-action surface.

### Re: install/trust UX comparison — gpt-5

Completed `#cbmtrust` as a design comparison only; no tsift source or Rust architecture changed. Upstream codebase-memory-mcp has a stronger single-command trust story today: the README advertises platform archives, static binaries, SHA-256 checksums, setup scripts, AUR install, and agent-mediated install; its current `install.sh` detects OS/arch, refuses non-HTTPS download URLs except localhost, downloads a latest-release archive, verifies `checksums.txt`, installs the binary, and then runs `codebase-memory-mcp install -y` unless `--skip-config` is passed. Its security policy also claims release-time SLSA provenance, Sigstore/cosign signatures, SBOMs, SHA-256 checksums, VirusTotal blocking scans, OpenSSF Scorecard, native AV checks, CodeQL, fuzzing, and other release gates. The multi-agent UX matters because v0.5.0 release notes describe auto-configuring multiple coding agents, and the later security-hardening issue says `install --plan` / a machine-readable install receipt was added so config/hook writes can be inspected before mutation. Sources checked: https://github.com/DeusData/codebase-memory-mcp, https://raw.githubusercontent.com/DeusData/codebase-memory-mcp/main/install.sh, https://raw.githubusercontent.com/DeusData/codebase-memory-mcp/main/SECURITY.md, https://github.com/DeusData/codebase-memory-mcp/releases, https://github.com/DeusData/codebase-memory-mcp/issues/397, and https://github.com/DeusData/codebase-memory-mcp/issues/388.

tsift already has the right architectural base but a thinner trust wrapper. Its release workflow is tag-driven, checks that all crate versions match the `vX.Y.Z` tag, runs the full CI/package checks, publishes GitHub release archives for Linux x86_64, macOS arm64, and Windows x86_64, attaches per-asset `.sha256` files, publishes crates.io packages, and publishes the OpenCode npm plugin through OIDC trusted publishing. The local `scripts/install.sh` verifies the downloaded archive against the matching `.sha256` before copying `tsift` into `~/.local/bin`, but it only covers Linux x86_64 and macOS arm64, does not expose attestation/SBOM/scan verification, and does not configure agents. Agent integration is intentionally separate: `tsift init` writes managed `AGENTS.md`/`CLAUDE.md` instruction sections, can install a Codex `UserPromptSubmit` hook, and can install OpenCode commands. Those config writes are already idempotent and marker-aware: existing managed blocks become `already present` or `updated`, duplicates are removed for Codex hooks, and unmanaged OpenCode command files fail closed instead of being clobbered. Local sources checked: `src/tsift/scripts/install.sh`, `src/tsift/specs/release-integration.md`, `src/tsift/.github/workflows/release.yml`, `src/tsift/packages/tsift-index/src/init.rs`, and `src/tsift/packages/opencode-tsift/src/install.js`.

Recommendation: copy the trust UX, not the C/static-binary architecture. Keep tsift's Rust workspace, Cargo install path, SQLite graph store, and split crates. Add a thin trust layer in three pieces: first, extend releases with an aggregate `checksums.txt`, GitHub build provenance attestations, an SBOM, and documented `gh attestation verify` / checksum verification steps; add scan gates only where they are deterministic enough not to create noisy release blocks. Second, add a JSON plan/receipt mode to the existing init path, e.g. `tsift init --plan --codex --opencode [--workspace]`, returning planned files, hook events, command sources, created/updated/already-present/conflict actions, redacted paths where appropriate, `writes_started=false`, and the next apply command. Third, let `scripts/install.sh` remain a binary installer by default but optionally chain to that plan/apply flow, for example `--agent-config codex,opencode --plan` and `--agent-config codex,opencode --yes`, so agents can verify binary provenance and config mutations in one flow without moving config-writing logic out of Rust.

Priority order for tsift: `init --plan`/receipt first, because it directly addresses sensitive agent config upserts and can reuse the current idempotent init code; release attestations/SBOM second, because they strengthen binary trust without changing runtime behavior; installer chaining third, because it is mainly UX once the binary and init plan surfaces are trustworthy. Verification: no build/test run was needed because this cycle made no source, spec, or workflow edits. Commands/proof: `agent-doc preflight`, `tsift status`, `tsift --envelope source-read`, `tsift --envelope symbol-read`, `tsift --envelope search`, local release/install file reads, `git status`, and upstream primary-source review. Escalation: none.

### Re: next-steps — gpt-5

> **Queue prompt:**
>
> What are #next-steps to fix bugs in tsift?


Top backlog item: `#tsbugev`.

I added a fresh priority-ordered tsift bug-fix backlog instead of reopening archived completed ids. The highest-priority bug is graph evidence target resolution: the local command `tsift graph-db --json --path . evidence next-steps --depth 3 --limit 8` resolved `next-steps` to an unrelated `tasks/professional/equityfundingsource.md` backlog row, which means generic queue-head ids can pull the wrong document context. That should be fixed before relying on graph-backed planning for ambiguous queue terms.

Priority order added: `#tsbugev`, `#tsbugtok`, `#tsbuglock`, `#tsbugcm`, `#tsbugidx`, `#tsbugcache`.

Notes: `#tsbugtok` comes from this session's current token/churn signals: prompt_budget, cache_resend, repeated_raw_read, restart_loop, and noop_closeout all appeared in `session-review --next-context`. `#tsbuglock`, `#tsbugcm`, `#tsbugidx`, and `#tsbugcache` come from archived dogfood findings that are still credible root-cause work: graph-db locking, conflict-matrix mixed verdicts, stale index/summary readiness, and unstable full-projection cache reuse. I did not add the stale `agent-doc` install warning to the tsift backlog because that belongs to `src/agent-doc`, but it is real: preflight warned that the installed binary/cdylib predates source commit `616ef21f`, so live sessions may need `make install` in `src/agent-doc` separately.

Changed paths: session document/backlog only. Verification: no tsift source/build/test run was needed for this planning-only cycle. Commands/proof: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `tsift status`, `tsift --envelope context-pack`, `tsift --envelope session-review --next-context`, archive/backlog searches with `rg`, `git -C src/tsift status --short`, and `tsift graph-db --json --path . evidence next-steps --depth 3 --limit 8`. Confidence: high on the ordering; medium on deeper implementation effort until each item is isolated in source. Escalation: none.

### Re: graph evidence target resolution — gpt-5

> **Queue prompt:**
>
> [#tsbuglock]


> **Queue prompt:**
>
> [#tsbugtok]


> **Queue prompt:**
>
> [#tsbugev]


Completed `#tsbugev`.

Changed paths: `src/tsift/packages/tsift-core/src/store.rs`, `src/tsift/packages/tsift-sqlite/src/lib.rs`, `src/tsift/packages/tsift-cli/src/lib.rs`, `src/tsift/packages/tsift-cli/src/commands/infra.rs`, `src/tsift/packages/tsift-cli/src/conflict_matrix.rs`, `src/tsift/packages/tsift-cli/tests/graph_db_conformance.rs`, `src/tsift/specs/graph.md`, plus the superproject `src/tsift` gitlink.

Implementation: `GraphStore` now exposes candidate-level evidence target lookup, SQLite resolves those candidates through indexed `graph_node_properties` and can filter by `path`, and `graph-db evidence` derives a preferred session path when `--path` points at an agent-doc markdown file. Exact graph node ids still bypass path scoping. Non-id evidence targets now prefer the hinted session document; unscoped same-kind collisions across multiple session paths fail closed with disambiguation guidance instead of silently choosing an unrelated row. The graph spec records that contract.

Regression coverage: added a graph-db conformance fixture with `#b000` in both `tasks/software/tsift.md` and `tasks/professional/equityfundingsource.md`. It proves unscoped `evidence b000` is ambiguous and `--path tasks/software/tsift.md evidence b000` resolves to the tsift session row.

Commits/push: pushed `btakita/tsift@dff637d` (`fix: scope graph evidence target resolution`) to `origin/main`, then pushed `btakita/agent-loop@2947a18be` (`chore: update tsift submodule`) so the superproject points at that tsift commit.

Verification: `cargo fmt`; `cargo test -p tsift-sqlite sqlite_resolves_evidence_targets_with_indexed_properties`; `cargo test -p tsift-cli --test graph_db_conformance graph_db_evidence_prefers_hinted_session_path_for_colliding_backlog_ids`; `cargo test -p tsift-cli --test graph_db_conformance`; `make check`; `cargo build`; `cargo install --path . --locked`; `tsift --version`. All passed locally, and the installed binary reports `tsift 0.1.67`. Latest tsift CI after the push is in progress (`27314796862`); the previous CI run on `main` was green, and local full-suite evidence is clean. The agent-loop repo has no workflow named `CI`.

Confidence: high. Escalation: none. Note: unrelated dirty working-tree changes were already present in `src/tsift` and the superproject and remain unstaged.

### Re: graph-db lock recovery bookkeeping — gpt-5

Bookkeeping closeout for the repeated active-queue continuation prompt. `#tsbuglock` was already implemented, verified, committed, pushed, and marked done in the prior cycle; no additional source changes were made here.

Changed paths: session document only.

Verification: no code verification rerun for this bookkeeping cycle. The `#tsbuglock` implementation cycle already passed `make check`, `cargo build`, `cargo install --path . --locked`, and focused lock-recovery tests.

No actionable follow-up items were introduced by this bookkeeping prompt.


### Re: conflict-matrix dispatch semantics — gpt-5

> **Queue prompt:**
>
> [#tsbugcm]


`#tsbugcm` is already satisfied in the committed tsift code, so I made no source changes for this queue item.

Evidence checked:
- `ConflictMatrixCandidate` and `ConflictMatrixWorkerPromptPacket` expose `previously_completed`.
- `ConflictMatrixReport` exposes both aggregate `fail_closed` / `can_parallel` and the split fields `cross_target_parallel_safe` plus `per_target_fail_closed`.
- `conflict_matrix_candidate_from_evidence` treats missing source ownership as per-target fail-closed only when there is no completed `worker_result`; completed worker results downgrade missing source handles to an informational warning and set `previously_completed=true`.
- `build_conflict_matrix_report_from_prepared_graph` computes per-target fail-closed separately from cross-target overlap safety, then derives `can_parallel` from both.
- Conformance tests cover the two key contracts: low-overlap targets with missing source ownership fail closed per target while cross-target safety stays true, and completed worker results without source handles are exposed as previously completed instead of redispatch-blocking hard failures.

Changed paths: none for this queue item.

Verification:
- `cargo test -p tsift-cli --test graph_db_conformance conflict_matrix_separates_per_target_fail_closed_from_cross_target_overlap`
- `cargo test -p tsift-cli --test graph_db_conformance conflict_matrix_downgrades_completed_worker_result_without_source_handles`
- `cargo test -p tsift-cli conflict_matrix_uses_cargo_package_mentions_as_ownership_evidence`

All focused tests passed. The earlier `#tsbuglock` cycle also ran the full `make check` suite successfully before commit/push.

Build/install/commit/push: skipped for this cycle because there were no source changes to build, install, commit, or push.

No actionable follow-up items were introduced by this duplicate verification.

Confidence: high. Escalation: none. Note: unrelated dirty working-tree changes remain in `src/tsift` and the superproject; I did not stage or alter them.
Confidence: high. Escalation: none.

### Re: stale index and summary readiness — gpt-5

`#tsbugidx` is already satisfied by the current implementation, so I did not make source changes for this item.

The graph DB readiness path already fails closed when semantic rows are unavailable and the summary cache is empty/unavailable. In that case it returns blocked readiness, sets `fail_closed`, and emits the repair sequence through `next_commands`, including `tsift summarize --extract .` and the graph DB refresh command. The status/context-pack side also already reports stale scoped indexes with one explicit repair command before summary extraction is trusted.

Verification passed:

- `cargo test -p tsift-cli context_pack_status_reminders_include_stale_index_state`
- `cargo test -p tsift-cli --test graph_db_conformance graph_db`
- `cargo test -p tsift-status status_workspace_scoped_indexes_report_stale`

No actionable follow-up items for this queue head.

### Re: full-projection cache reuse — gpt-5

`#tsbugcache` is already satisfied by the current implementation, so I did not make source changes for this item.

Evidence checked:

- `VERSIONS.md` records `#gcachemiss`, and `plans/gcachemiss-evidence.md` documents the root cause, fix, and gate update.
- `graph_db_backend_eval_full_projection_source_watermark` hashes stable projection inputs: projection/cache versions, scope/path, indexed symbols/call edges/routes, and semantic summary rows. It excludes generated artifacts and session markdown, and it does not depend on `.tsift/summaries.db` file metadata.
- The full-projection cache-hit path reports `hit: true`, emits cache read/decode phases, and reports `full_projection.source_graph_build` plus `full_projection.projection_rows` as zero-duration skipped work.
- The backend-eval performance gate requires `full_projection.cache.hit` and states that binding full-projection performance evidence requires a cold populate leg followed by cache-leg samples with `full_projection.cache.hit=1`; cache-miss samples are diagnostics, not backend or hop-cap promotion proof.

Verification passed:

- `cargo test -p tsift-cli traversal_source_watermark_uses_summary_rows_not_summaries_db_metadata`
- `cargo test -p tsift-cli full_projection`
- `cargo test -p tsift-cli --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`

No actionable follow-up items for this queue head.

### Re: queue closeout recovery — gpt-5

Closeout recovery note for the already-visible queue responses. `#tsbugidx` and `#tsbugcache` are answered above, the backlog is empty, and no new actionable items are being added.
<!-- agent:boundary:d90e5c6b -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#tsbugev]~~
- ~~[#tsbugtok]~~
- ~~[#tsbuglock]~~
- ~~[#tsbugcm]~~
<!-- /agent:queue -->
<!--
- ~~[#tsbugidx]~~
- ~~[#tsbugcache]~~
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
