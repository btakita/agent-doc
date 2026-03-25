---
agent_doc_session: e02128be-5154-406d-9f1f-f919a0150d7c
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
**Active** — expanding ontology → eval mapping and plugin eval example
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Assistant

Module-harness session ready. This document tracks work related to the `/module-harness` skill — generating, updating, and auditing module-level harness context (specs, contracts, evals) for any programming language.

What would you like to work on? Some options:
- Define or refine the module-harness skill spec
- Generate harness context for a specific module/submodule
- Audit existing harness coverage across the repo
- Plan improvements to the skill workflow
Expand on ontology -> eval.md
The eval uses the agents to quantifiably measure phenomena.
- Quantification includes boolean logic (true -> 1.0, false -> 0.0)

Let's also have a plugin example to demonstrate how evals can work within the module.
Some evals could include:
- Are there enough hooks?
- Are there too many hooks?
- Is the interface too simple or too complex?
### Assistant

**Ontology → Eval Connection**

The eval framework maps directly to the ontology chain:

| Ontology Term | Eval Role |
|---|---|
| **System** | The module under evaluation — viewed from an internalized perspective |
| **Resolution** | The granularity of measurement — boolean (1.0/0.0) is lowest resolution; Likert scales, continuous scores are higher |
| **Pattern** | What evals detect — repeatable, predictable elements that should transfer across contexts |
| **Context** | The scope bounding what's relevant to each eval — limits the information needed for coherent judgment |
| **Focus** | Finite attention applied — each eval focuses on one measurable aspect to avoid dilution |

**Quantification model:**
- Boolean: `true → 1.0, false → 0.0` — pass/fail evals (contract violations, structural checks)
- Ordinal: Likert `1-5` mapped to `0.0–1.0` — quality/style evals (voice consistency, complexity assessment)
- Continuous: `0.0–1.0` — coverage ratios, similarity scores
- Composite: weighted average of sub-evals → aggregate module health score

**Plugin Example: JetBrains Plugin Evals**

For a JetBrains plugin module (e.g., `python-intentions`), the harness evals would measure:

```kotlin
/**
 * # Module: PythonIntentionsPlugin
 *
 * ## Spec
 * - registerIntentions(): registers all intention actions with IDEA platform
 * - IntentionAction.invoke(): transforms code at caret position
 *
 * ## Agentic Contracts
 * - No ActionPromoter registered (breaks intention chain in .md files)
 * - Self-disabling via update() when HighlightInfo exists at caret
 *
 * ## Evals
 * - hook_coverage: all extension points have registered listeners → true/false (1.0/0.0)
 * - hook_excess: no unused/dead extension registrations → true/false (1.0/0.0)
 * - interface_complexity: public API surface ≤ 5 entry points → true/false (1.0/0.0)
 * - interface_depth: max inheritance depth ≤ 3 → true/false (1.0/0.0)
 * - intention_chain_safety: ActionPromoter absent from plugin.xml → true/false (1.0/0.0)
 * - hot_reload_compat: no structural class changes requiring restart → 0.0–1.0 (ratio)
 */
```

**How evals run within the module:**

1. **Static evals** (code-graded, instant): Parse `plugin.xml` and source → count hooks, measure API surface, check structural rules
2. **Dynamic evals** (test-backed): Run against a test IDE instance → verify intention actions fire, check no regressions
3. **Agentic evals** (LLM-graded): Agent reads the module + harness → judges whether contracts match implementation, flags drift

**Your eval criteria mapped to resolution levels:**

| Eval Question | Resolution | Measurement |
|---|---|---|
| Are there enough hooks? | Boolean | `registered_hooks ≥ required_hooks → 1.0` |
| Are there too many hooks? | Boolean | `registered_hooks ≤ max_hooks → 1.0` |
| Is the interface too simple? | Ordinal | `API_surface_score` on 1-5 scale |
| Is the interface too complex? | Ordinal | `API_surface_score` on 1-5 scale |

The "too simple vs too complex" question is a **range eval** — the optimal value sits in a band, not at an extreme. This maps to the ontology's **Resolution** concept: you need enough granularity to distinguish "just right" from "too much" or "too little."
<!-- agent:boundary:216afbf6 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Formalize eval.md spec connecting ontology → quantification model
- Build module-harness binary support for eval scoring (boolean + ordinal + continuous)
- Create reference plugin harness with runnable evals
- Add range-eval support (band scoring for "too much/too little" criteria)
<!-- /agent:pending -->
