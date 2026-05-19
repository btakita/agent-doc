---
name: compose-skills
description: Design composable agent skill systems before implementation. Use when Codex needs to split an oversized workflow into focused skills, audit whether a skill should be one skill or several, define boundaries between cooperating skills, or create a skill architecture plan before using skill-creator to implement individual skills.
---

# Compose Skills

## Workflow

Use this skill before implementing or revising skills when the problem is bigger than one obvious task. Produce a short skill architecture plan, then hand individual skill creation to `skill-creator`.

1. Identify the workflow goal and concrete trigger prompts.
2. Inventory existing skills that already cover part of the work.
3. Split the workflow into candidate skills only when the split improves focus, reuse, validation, or safety.
4. Define each candidate skill's boundary, inputs, outputs, resources, and invocation policy.
5. Validate the plan against the composability checklist before implementation.

For the formal contract, read `SPEC.md`. For examples, read `references/examples.md` when the split is ambiguous.

## Decision Boundary

Keep one skill when the workflow has one user-facing goal, one natural trigger, shared resources, and few reusable subparts.

Split into multiple skills when at least two of these are true:

- A subtask is reusable across unrelated workflows.
- The workflow has distinct failure modes or validation checks.
- A deterministic script or reference set would be useful outside the original workflow.
- A subtask should have different invocation policy or risk controls.
- The current skill is hard to test because it mixes planning, execution, and publishing.

Do not split only to make names tidy. Extra skills add routing overhead and maintenance cost.

## Plan Shape

Return plans with these sections:

```markdown
## Decision Boundary
## Proposed Skills
## Resource Inventory
## Invocation Policy
## Validation Plan
## Recommendation
```

`scripts/validate_composition_plan.py` checks for those headings and basic skill-entry fields.

## Candidate Skill Fields

For each proposed skill, specify:

- `name`: lowercase letters, digits, and hyphens.
- `purpose`: the single job it owns.
- `triggers`: user wording or contexts that should activate it.
- `inputs`: artifacts, paths, prompts, credentials, or external systems required.
- `outputs`: files, plans, code changes, messages, or reports it produces.
- `resources`: `SKILL.md` only, `references/`, `scripts/`, or `assets/`.
- `invocation`: `implicit`, `explicit`, `agent-only`, or `user-only`.
- `handoff`: which skill or ordinary workflow should consume its output.

## Resource Inventory

Prefer deterministic scripts for repeated parsing, validation, generation, API calls, or mechanical transformations. Prefer references for domain rules, schemas, examples, or workflow variants. Prefer assets only for templates or files copied into outputs.

If a proposed skill has no reusable resources and no stable trigger, keep it as ordinary prompt guidance or a section in another skill.

## Invocation Policy

Use `implicit` for low-risk analysis or formatting skills with clear trigger descriptions.

Use `explicit` when users should choose the workflow intentionally.

Use `agent-only` for helper skills that should be chained by other skills but are noisy or confusing in a user menu.

Use `user-only` for higher-risk actions such as deploys, publishing, sending messages, purchasing, or mutating remote systems.

## Validation

Run the validation script on the plan:

```bash
python3 .codex/skills/compose-skills/scripts/validate_composition_plan.py <plan.md>
```

Forward-test with at least three prompts:

- One prompt that should use a single proposed skill.
- One prompt that should require multiple proposed skills.
- One prompt that should not trigger the proposed skill set.

Revise the plan if the split is hard to explain, the same resource appears in multiple skills, or the invocation policy is unclear.
