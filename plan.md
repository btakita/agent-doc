# Model Tier Selection Plan

Implements harness-agnostic model tier selection for agent-doc. See `tasks/agent-doc/agent-doc-model.md` for the design discussion.

**Priority (highest wins):** inline `/model <x>` command → `<!-- agent:model -->` component → `agent_doc_model_tier` frontmatter → diff heuristic.

All sources resolve into a single `effective_tier` field in preflight JSON that the skill consumes.

## Layer 0 — Tier type + config

- [ ] Add `Tier` enum (`Auto`, `Low`, `Med`, `High`) in a new `src/model_tier.rs` module with `serde` derive, `FromStr`, `Display`, and `PartialOrd` (Auto < Low < Med < High)
- [ ] Extend `src/config.rs` to parse `[model]` and `[model.tiers.<harness>]` sections from `~/.config/agent-doc/config.toml`:
  - `[model]` → `auto: bool`, `budget_threshold: f32`, `preferred: Tier`, `budget_fallback: Tier`
  - `[model.tiers.<harness>]` → `low | med | high` concrete model name strings
  - Built-in defaults: `claude-code` → `haiku | sonnet | opus`, `codex` → `gpt-4o-mini | gpt-4o | o3`, `default` → `haiku | sonnet | opus`
- [ ] Add harness detection helper `detect_harness() -> String` reading env vars (`CLAUDE_CODE_SESSION`, `CODEX_SESSION`, fallback `default`)
- [ ] Add `resolve_tier_to_model(tier: Tier, harness: &str, config: &Config) -> Option<String>` — tier → concrete model name via config
- [ ] Unit tests: config parsing, tier ordering, harness detection fallback, tier→model resolution for each built-in harness
- [ ] `make check`

## Layer 1 — Frontmatter `agent_doc_model_tier`

- [ ] Extend `src/frontmatter.rs` `Frontmatter` struct with `agent_doc_model_tier: Option<Tier>` (defaults to `Auto` when absent)
- [ ] Unit tests: frontmatter parse for each tier value, absent field, invalid value rejection
- [ ] `make check`

## Layer 2 — `<!-- agent:model -->` component read

- [ ] Extend `src/preflight.rs` layout check (step 0) to scan for `<!-- agent:model -->...<!-- /agent:model -->` component and extract the inner trimmed content
- [ ] Parse component content as `Tier` (or treat concrete model name like `opus` → resolve back to tier via config reverse-lookup)
- [ ] Hold the parsed `Tier` for later `effective_tier` composition
- [ ] Unit tests: component present/absent, concrete model name resolution, invalid content falls back to `Auto`
- [ ] `make check`

## Layer 3 — Inline `/model <x>` diff scanner

- [ ] In `src/preflight.rs` step 4 (diff), after computing the unified diff but before returning JSON:
  - Scan user-added lines (lines starting with `+` excluding `+++`) for regex `^\s*/model\s+(\S+)\s*$`
  - On match: capture `model_switch` (the raw argument, e.g., `opus` or `high`), resolve to `Tier` via config reverse-lookup or direct parse, record as `model_switch_tier`
  - Strip the matched lines from the diff text returned in JSON (they are commands, not content)
- [ ] Guard: do NOT match lines inside code fences (track ``` state) or blockquotes (lines starting with `+>`)
- [ ] Unit tests: `/model opus` resolves to `High` + stripped; `/model med` direct tier; `/model haiku` → `Low`; fenced `/model` ignored; blockquoted `/model` ignored; unknown name → no match
- [ ] `make check`

## Layer 4 — Diff heuristic `suggested_tier`

- [ ] Add `suggested_tier(diff_type: DiffType, lines_added: usize, doc_path: &Path) -> Tier` in `src/preflight.rs` or `src/model_tier.rs`:
  - `SimpleQuestion` | `ContentAddition` < 10 lines → `Low`
  - `ContentAddition` ≥ 10 lines | `MultiSection` → `Med`
  - `CodeChange` | `ArchitectureChange` → `High`
  - `AmbiguousCommand` → `Med` (safe default)
  - Unknown/missing → `Med`
- [ ] Secondary doc path boost: `tasks/software/` or `src/**/specs/` → bump one tier (cap at `High`)
- [ ] Unit tests: each diff type mapping, path boost, cap behavior
- [ ] `make check`

## Layer 5 — `effective_tier` composition + JSON output

- [ ] In `src/preflight.rs`, compose final fields in precedence order:
  1. `model_switch_tier` (Layer 3) — highest, ephemeral, one-shot
  2. Component tier (Layer 2)
  3. Frontmatter tier (Layer 1)
  4. Heuristic `suggested_tier` (Layer 4)
- [ ] Emit in preflight JSON:
  ```json
  {
    "effective_tier": "high",
    "required_tier": "high",          // from component/frontmatter, null if none
    "suggested_tier": "med",          // always present (heuristic)
    "model_switch": "opus",           // from inline /model, null if none
    "model_switch_tier": "high"       // resolved tier for model_switch, null if none
  }
  ```
- [ ] `required_tier` = component tier if present, else frontmatter tier if present, else `null`
- [ ] `effective_tier` = first non-Auto among (model_switch_tier, required_tier, suggested_tier)
- [ ] Integration test: fixture documents exercising each precedence path, assert JSON fields
- [ ] `make check`

## Layer 6 — Skill gate logic

- [ ] Update `SKILL.md` step 0 (after preflight) to read `effective_tier`, `required_tier`, `model_switch`, `model_switch_tier`
- [ ] Skill current-model detection: read model from frontmatter `model` field or env var `CLAUDE_CODE_MODEL` (fallback: unknown → proceed)
- [ ] Gate rules:
  - If `model_switch` is set and resolved tier ≠ current tier: write a `<!-- patch:exchange -->` note `"⚠ Model switch requested: <name>. Run /model <name> at the terminal and re-invoke /agent-doc."` and stop
  - Else if `required_tier` > current tier: write a gate note `"This document requires tier <tier>. Run /model <concrete> and re-invoke."` and stop
  - Else if `suggested_tier` > current tier: write an advisory note but continue
  - Else: proceed silently
- [ ] Update `agent-doc skill install` to bundle the new SKILL.md
- [ ] Manual test: document with `agent_doc_model_tier: high` on a sonnet session gates correctly; inline `/model opus` on opus session is no-op; `<!-- agent:model -->high<!-- /agent:model -->` gates like frontmatter
- [ ] `make check`

## Layer 7 — Documentation + VERSIONS.md

- [ ] Update `README.md` with the `[model]` config section and tier semantics
- [ ] Update `src/agent-doc/CLAUDE.md` module layout list with `model_tier.rs`
- [ ] Add `VERSIONS.md` entry summarizing the feature
- [ ] Bump `Cargo.toml` + `pyproject.toml` version
- [ ] Final `make check` — record results in Review section below

## Review

- Pending.

## 2026-04-22 — `#cyc1` cycle completion invariants

- [ ] Add binary-owned cycle-state tracking for per-document preflight/write/commit lifecycle so interrupted cycles are identified by exact phase/hash state rather than loose heuristics.
- [ ] Update `preflight`, `write`, `recover`, `git`, and `session_check` to advance/validate the cycle state, auto-attempt recovery+commit, and fail closed when the prior cycle still has no terminal committed state.
- [ ] Add focused tests and spec/docs updates for the new invariant, then verify with targeted tests, `make check`, `cargo build --release`, and `cargo install --path .`.

## 2026-04-22 — route stash-pane eviction (`agent-doc-bugs2`)

- [x] Confirm the repeated auto-start path that creates replacement stash panes for an already-known document session.
- [x] Evict an existing live stash pane for the same session before re-registering a newly provisioned replacement pane.
- [x] Add a regression test covering repeated stash provisioning and verify with targeted route tests.

## 2026-04-22 — missing response patchback investigation (`agent-doc-bugs2`)

- [x] Check the root `agent-loop/.agent-doc` logs for `tasks/agent-doc/agent-doc-bugs2.md` rather than the `src/agent-doc` submodule logs.
- [x] Confirm whether the latest `agent-doc-bugs2.md` session reached `preflight` / `write` / `commit`, or only `session_start` / `codex_start`.
- [x] Summarize the evidence in the task document, including the stale snapshot vs live-file drift and the absence of pending-response artifacts.

## 2026-04-22 — `agent-doc-bugs` failing tests follow-up

- [x] Reproduce the current `src/agent-doc` test failures and isolate whether they come from the new cycle-state/durable-capture changes or from existing harness/tmux instability.
- [x] Patch the minimal implementation or test expectation needed to restore the intended behavior without disturbing unrelated dirty worktree changes.
- [x] Re-run the relevant targeted tests plus the requested broader verification (`make check`, build, install when still applicable) and patch the exact result back into `tasks/agent-doc/agent-doc-bugs.md`.

## 2026-04-22 — `codex_args` follow-up (`agent-doc-bugs`)

- [x] Run the `agent-doc` test/build checks against the current `codex_args` change set and isolate the concrete failures.
- [x] Fix whichever side is wrong: implementation, docs/spec expectations, or regression tests, while preserving the intended Codex arg-resolution contract.
- [x] Re-run the relevant verification (`cargo test`, `cargo check`, and any required build/install step) and patch back the result into the task document.

## 2026-04-22 — `#wrtc1` Codex/manual-repair write path

- [x] Tighten the bundled skill/runbooks so Codex/manual-repair patchbacks explicitly require `agent-doc write --commit <file>` when the prompt is already present in the document, and explain why direct file patching is the wrong path.
- [x] Add regression tests around the bundled skill/install content so the Codex-facing instructions and manual-repair guidance are enforced by tests.
- [x] Verify with targeted tests plus build/install for local testing, then patch back the outcome and answer whether a hook/helper should try to auto-write the missed patchback.
