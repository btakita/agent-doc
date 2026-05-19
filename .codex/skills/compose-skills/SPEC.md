# Compose Skills Functional Specification

## Purpose

`compose-skills` designs a small system of reusable skills before individual skills are implemented. It is a planning and audit skill, not a replacement for `skill-creator`.

## Inputs

- A workflow description, existing skill, or rough skill idea.
- Optional examples of user prompts and expected outputs.
- Optional existing skill paths for overlap analysis.

## Outputs

The skill emits a markdown architecture plan with:

- decision boundary for one skill versus many skills;
- proposed skill list with names, purposes, triggers, inputs, outputs, resources, invocation policy, and handoff points;
- resource inventory covering references, scripts, and assets;
- validation plan with positive, chained, and negative trigger tests;
- recommendation to create standalone skills, extend an existing skill, or keep the behavior as ordinary prompt guidance.

## Rules

1. Prefer a single skill until splitting buys reuse, safety, clearer validation, or lower maintenance.
2. Split planning, deterministic tooling, risky side effects, and domain-specific references when they have different triggers or policies.
3. Keep `SKILL.md` concise and move detailed examples or domain rules into references.
4. Use scripts for repeated deterministic checks or transformations.
5. Mark high-risk actions as explicit or user-only; do not hide deploys, sends, purchases, or publication behind implicit routing.
6. Hand off individual skill implementation to `skill-creator` after the composition plan is validated.

## Required Plan Sections

Every composition plan must include:

- `Decision Boundary`
- `Proposed Skills`
- `Resource Inventory`
- `Invocation Policy`
- `Validation Plan`
- `Recommendation`

## Validation Contract

`scripts/validate_composition_plan.py` must fail when required sections are absent or when a plan has no candidate skill entries. A candidate skill entry is any markdown list item or heading containing `name:` with a hyphen-case skill name.
