# Agent-Doc Context Audit and OKF Report

Date: 2026-06-27

## Scope

This report completes `tasks/agent-doc/plan-agent-doc-context-audit-okf.md`.
It covers the source `src/agent-doc` instruction surface, installed Claude
Code/Codex/OpenCode/Cursor mirrors in this workspace, OKF concept bundles,
the generic `okf/` install path, `audit-docs` checks, and the first
loaded-context ledger implementation.

Token estimates use `bytes / 4`. They are approximate and are reported only as
measurements, not as a claimed model-token reduction.

## Inventory

| Source | Class | Bytes | Words | Est. tokens | Owner |
| --- | --- | ---: | ---: | ---: | --- |
| `src/agent-doc/SKILL.md` | hot-path | 17,979 | 2,259 | 4,495 | Trigger/invariant router |
| `.claude/skills/agent-doc/SKILL.md` | installed hot-path | 28,913 | 3,746 | 7,229 | Rendered Claude mirror |
| `.codex/AGENTS.md` | installed hot-path | 18,138 | 2,294 | 4,535 | Rendered Codex mirror |
| `.opencode/skills/agent-doc/SKILL.md` | installed hot-path | 17,733 | 2,236 | 4,433 | Rendered OpenCode mirror |
| `.cursor/rules/agent-doc.md` | installed hot-path | 28,913 | 3,746 | 7,229 | Rendered Cursor mirror |
| `src/agent-doc/runbooks/*.md` | procedure | 104,064 | 14,850 | 26,016 | Branch workflow detail |
| `src/agent-doc/runbooks/dynamic-context.md` | procedure | 2,557 | 337 | 639 | Context authoring branch |
| `src/agent-doc/okf/*.md` | concept | 3,149 | 428 | 788 | Durable vocabulary |
| Generated `agent-doc plan` output for this task | generated-state | budget 6,000 | n/a | budget 6,000 | Binary-owned current state |
| Generated tsift/job context sidecars | generated-state | budgeted per job | n/a | budgeted per job | Binary-owned current state |

## Ownership Rules

- `SKILL.md` owns trigger wording, mandatory closeout boundaries, and the ordered
  response cycle spine.
- `runbooks/*.md` owns branch-specific procedure detail.
- `okf/*.md` owns durable concept definitions and vocabulary.
- Generated packs (`agent-doc preflight`, `agent-doc plan`, tsift context packs,
  job context sidecars) own current-state facts only.
- Project/root instructions own local routing and local overrides.

## Changes Made

- Added stable `concept_id` frontmatter to the three OKF concept files:
  `agent-doc.session-cycle`, `agent-doc.instruction-surface`, and
  `agent-doc.dynamic-context`.
- Added a tracked submodule-local Claude OKF bundle beside
  `src/agent-doc/.claude/skills/agent-doc/SKILL.md`, because explicit
  `audit-docs --root src/agent-doc` audits that installed surface.
- Added `audit_managed_okf_bundles` and wired it into `audit-docs`. The audit now
  fails on missing OKF files, stale OKF content, stale managed concept markdown,
  duplicate/missing bundled concept IDs, or instruction surfaces that link to
  unbundled `okf/*.md` files.
- Extended OKF install coverage to Cursor and the generic `okf/` fallback path.
  `install --all` remains aligned with `agent-kit`'s four named harness mirrors:
  Claude Code, Codex, OpenCode, and Cursor.
- Added `loaded_context_ledger` to `agent-doc plan` `tsift_context` output and to
  materialized job context sidecars. Ledger records carry `source_id`,
  `source_kind`, `path`, `content_hash`, optional `concept_id`, `loaded_at`, and
  `expansion_reason`.

## Measurements

Before/after numbers for the hot path and OKF bundle:

| Item | Before bytes | After bytes | Before est. tokens | After est. tokens | Delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| `src/agent-doc/SKILL.md` | 17,979 | 17,979 | 4,495 | 4,495 | 0 |
| Source OKF bundle | 3,033 | 3,149 | 759 | 788 | +29 tokens |

Loaded-context scenarios:

| Scenario | Files loaded | Bytes | Words | Est. tokens |
| --- | --- | ---: | ---: | ---: |
| Default hot path | `SKILL.md` only | 17,979 | 2,259 | 4,495 |
| Dynamic-context branch | `SKILL.md`, `runbooks/dynamic-context.md`, `okf/index.md`, `okf/dynamic-context.md` | 22,095 | 2,805 | 5,524 |
| Worst-case bundled context | `SKILL.md`, all bundled runbooks, all OKF concepts | 125,192 | 17,537 | 31,298 |

OKF concepts loaded per cycle:

- Default cycle: 0 OKF concept files.
- Dynamic-context branch: 2 OKF files (`index.md` and the target concept).
- Worst-case all-concept expansion: 4 OKF files.

Ledger measurements:

- `agent-doc plan tasks/agent-doc/plan-agent-doc-context-audit-okf.md` emitted 4
  ledger entries and `duplicate_expansions_suppressed: 0` for the real current
  cycle.
- The focused unit test `loaded_context_ledger_suppresses_duplicate_source_hashes`
  proves duplicate suppression by reducing 3 attempted records to 2 entries with
  `duplicate_expansions_suppressed: 1`.
- The job sidecar test proves materialized `tsift context-pack` sidecars include
  `loaded_context_ledger`.

## Verification

Passing focused checks:

- `cargo test -p agent-doc audit_managed_okf_bundles`
- `cargo test -p agent-doc install_okf_creates_concepts_for_each_harness`
- `cargo test -p agent-doc install_okf_all_creates_for_each_env`
- `cargo test -p agent-doc loaded_context_ledger_suppresses_duplicate_source_hashes`
- `cargo test -p agent-doc create_writes_tsift_context_sidecar_when_command_succeeds`
- `cargo run -p agent-doc -- audit-docs --root /home/brian/work/btakita/agent-loop/src/agent-doc`

The scoped `audit-docs` run reports no blocking issues and only mtime advisories.
The workspace-root `audit-docs` run reaches unrelated checked-out/bootstrap
instruction files and fails on pre-existing non-OKF issues; no OKF bundle drift
was reported before those unrelated failures.

`cargo test -p agent-doc` passed before the final test-only generic OKF assertion
was added. After that assertion, focused install/audit/ledger/sidecar tests passed;
a full rerun was interrupted after `tests/run_integration.rs` wedged on an
unrelated defunct-child wait in
`run_heartbeats_redirect_stderr_under_managed_tui_but_persist_progress`.

## Status

The OKF integration is now mechanically audited, installed bundles are aligned
for the managed harnesses present in this workspace, the generic install path is
covered, runtime planning and job context sidecars can prove loaded context
sources by hash, and token impact is measured without claiming an unproven
reduction.
