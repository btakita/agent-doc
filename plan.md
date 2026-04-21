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
