# Model Tier Gate

Harness-agnostic tier selection used by `agent-doc preflight` to advise / block execution based on document-declared model requirements.

## Tiers

`low | med | high | auto`. `auto` means "no preference — always proceed."

## Precedence (highest → lowest)

1. **Inline `/model <x>` command** in the diff (stripped from the downstream diff before the agent sees it).
2. **`<!-- agent:model -->` component** content.
3. **`agent_doc_model_tier` frontmatter field**.
4. **Diff heuristic** — preflight's `suggested_tier`, derived from `diff_type` and length.

Preflight composes these into `effective_tier`, plus optional `required_tier` (when source 2 or 3 specifies a hard requirement) and `model_switch` (when source 1 fires).

## Gate rules

- **`model_switch` set** (user typed `/model <x>`): acknowledge the switch in the response and proceed. The user explicitly chose this tier.
- **`required_tier` set and current session tier is lower**: tell the user
  > "This document requires `<required_tier>` tier. Run `/model <x>` in your terminal, then re-run `/agent-doc`."
  and stop. Do not generate a response.
- **`effective_tier` higher than current session** (advisory only, no `required_tier`): if the task looks substantial (`diff_type ∈ {approval, implementation, architecture}` or a long diff), suggest escalation in the response but still proceed best-effort.
- **`effective_tier = auto`**: always proceed; no advisory.

## Model-to-tier mapping

| Model ID | Short name | Tier |
|----------|-----------|------|
| `claude-opus-4-6` | opus-4-6 | high |
| `claude-opus-4-7` | opus-4-7 | high |
| `claude-sonnet-4-6` | sonnet-4-6 | med |
| `claude-haiku-4-5-20251001` | haiku-4-5 | low |

agent-doc template-mode sessions require `high` tier as a floor. Models below `high` may skip mandatory steps (write, commit, pending mutations) at lower effort levels.

## Notes

The tier comparison is advisory within a running session — the skill cannot hot-swap models, but it surfaces the mismatch so the user can `/model` and re-invoke. `required_tier` is the only hard gate.
