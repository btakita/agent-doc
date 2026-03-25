---
agent_doc_session: e02128be-5154-406d-9f1f-f919a0150d7c
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
**Active** — refined audit.md with agent-actionable results
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

| Ontology Term  | Eval Role |
|----------------|---|
| **System**     | The module under evaluation — viewed from an internalized perspective |
| **Resolution** | The granularity of measurement — boolean (1.0/0.0) is lowest resolution; Likert scales, continuous scores are higher |
| **Pattern**    | What evals detect — repeatable, predictable elements that should transfer across contexts |
| **Scope**      | The scope bounding what's relevant to each eval — limits the information needed for coherent judgment |
| **Focus**      | Finite attention applied — each eval focuses on one measurable aspect to avoid dilution |

Re: Context: this term has a broader scope than "scope". It includes Agentic Context and human context...which is effectively whatever is held in attention space at any given moment. I want to be careful to be accurate with the terminology. And not overconstrain terms.

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

Update /home/brian/work/btakita/agent-loop/src/module-harness/ontology/src/eval.md with our research here.
### Assistant

**Re: Context vs Scope terminology**

You're right — I was using Context where Scope was more precise. Fixed in the table. Context is the broader term: it encompasses the full attention space (agentic + human) active at the moment of evaluation, not just the bounds of what's relevant. Scope limits; Context includes everything held in working memory. The eval.md ontology node now reflects this distinction:

- **Scope** → bounds what's relevant to each eval
- **Context** → the full attention space active during evaluation (broader than scope)

**Updated `src/module-harness/ontology/src/eval.md`**

Added three new sections to the existing ontology node:

1. **Ontology chain mapping table** — System, Resolution, Pattern, Scope, Context, Focus with their eval roles (preserving the Context/Scope distinction)
2. **Quantification Model** — Boolean, Ordinal, Continuous, Composite scoring tiers
3. **Range Evals** — band scoring for "too much/too little" criteria (hook count, API surface)
4. **Eval Execution Layers** — Static (code-graded), Dynamic (test-backed), Agentic (LLM-graded)

The file retains its original Ontology/Axiology/Epistemology structure with the new content integrated under Epistemology.

> ### Range Evals

It seems this is a scalar value. But can we also have a vector direction for over-provisioning/under-provisioning, etc?
### Assistant

**Re: Range Evals — vector direction**

Yes — a scalar score alone loses the "which direction am I failing?" information. The solution is to make range evals return a **signed deviation** alongside the score:

**Scalar only (lossy):**
- `hook_count = 12, band = [5, 10]` → `score = 0.8` (degraded, but *which way?*)

**Scalar + direction vector (lossless):**
- `hook_count = 12, band = [5, 10]` → `score = 0.8, direction = +over` (over-provisioned by 2)
- `hook_count = 3, band = [5, 10]` → `score = 0.6, direction = -under` (under-provisioned by 2)

**Why direction matters:**
- **Actionable diagnostics**: "remove 2 hooks" vs "add 2 hooks" — the agent knows *what to do*
- **Trend analysis**: consecutive evals trending toward `+over` signals architectural drift
- **Asymmetric penalties**: over-provisioning might be cheaper to fix than under-provisioning (or vice versa), so the score degradation curve can differ by direction

**Proposed eval result structure:**

```
eval_result {
  score: 0.0–1.0,        // how far from optimal
  deviation: f64,         // signed distance from band center (- = under, + = over)
  direction: Under | Within | Over,  // categorical label
}
```

This maps to the ontology: **Resolution** now carries both magnitude (score) and direction (deviation) — a richer measurement that preserves information about *where* the value sits relative to the optimal band, not just *how far* it is.
Add audit to the ontology.
### Assistant

**Created `src/module-harness/ontology/src/audit.md`**

Audit is the active process that connects evals to actionable results. Key distinctions from related terms:

- **Eval** defines *what* to measure → **Audit** is *running* those measurements
- **Coverage** is a *metric* the audit computes → **Drift** is a *condition* the audit detects
- **Backtest** compares audit results *across time* → a single audit is a *point-in-time snapshot*

The node follows the same Ontology/Axiology/Epistemology structure and integrates:
- The three audit layers (static, dynamic, agentic) mirroring eval execution layers
- The `eval_result` type with `{score, deviation, direction}` from our range eval discussion
- Aggregation levels: module score → directory score → drift report
- The audit-as-universal-pattern framing (financial, security, code audits all share the structure)

> An audit produces a structured report with quantified results.

When I have been using audit-docs, we have been getting actionable results that the agent can perform.

Quantifiable results are interesting and we very well may want them.
### Assistant

**Re: Audit actionability**

Good catch — the original definition said "structured report with quantified results" which is too passive. In your experience with audit-docs, the audit produces **actionable items that the agent performs**, not just a report to read. Updated `audit.md`:

1. **Ontology definition** now says "actionable results — structured items that agents can directly perform" instead of "structured report with quantified results"
2. **Axiology** now includes the closed-loop pattern: `audit → detect → remediate → re-audit`. An agentic audit that finds missing spec entries doesn't just report the gap — it generates the entries.

This is the key difference between a traditional audit (produces a document for humans to act on) and an agentic audit (produces a work queue that agents execute). The audit becomes a **self-healing** mechanism when the agent can act on its own findings.
<!-- agent:boundary:726706cf -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Extend eval result type with signed deviation + direction enum
- Build module-harness binary support for eval scoring (boolean + ordinal + continuous + range)
- Create reference plugin harness with runnable evals
- Update eval.md ontology node with directional range eval spec
- ([done]) Formalize eval.md spec connecting ontology → quantification model
- ([done]) Add audit.md to ontology
- ([done]) Refine audit.md with agent-actionable results pattern
<!-- /agent:pending -->
