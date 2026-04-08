---
agent_doc_session: 92537d9c-8a74-4005-af42-083c7b50ed65
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
DeepEval research complete — 2026-04-07
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Please tell me more about [DeepEval](https://deepeval.com/).
https://github.com/confident-ai/deepeval

How can it be used within Claude Code?
Please tell me more about [DeepEval](https://deepeval.com/).
https://github.com/confident-ai/deepeval

How can it be used within Claude Code?

### Re: DeepEval Overview

**DeepEval** is an open-source Python evaluation framework — "Pytest for LLMs." It solves the problem of systematically validating LLM applications before and after deployment, treating AI quality like software testing rather than vibes-based judgment.

**Core idea:** Write test cases with inputs/outputs, attach metrics, run `deepeval test run`. Each metric outputs a 0–1 score with a configurable pass threshold (default 0.5).

```python
from deepeval import assert_test
from deepeval.test_case import LLMTestCase
from deepeval.metrics import AnswerRelevancyMetric

def test_rag_response():
    test_case = LLMTestCase(
        input="What is DeepEval?",
        actual_output="DeepEval is a testing framework for LLMs.",
        retrieval_context=["DeepEval is an open-source LLM evaluation library."]
    )
    assert_test(test_case, [AnswerRelevancyMetric(threshold=0.7)])
```

**50+ metrics across categories:**

| Category | Examples |
|----------|---------|
| RAG | Answer Relevancy, Faithfulness, Contextual Precision/Recall |
| Agentic | Plan Quality, Plan Adherence, Tool Correctness, Goal Accuracy, Task Completion |
| Safety | Hallucination, Bias, Toxicity, PII Leakage |
| Code gen | HumanEval, pass@k |
| Custom | G-Eval (chain-of-thought), DAG (decision-tree) |

**Agentic metrics** are particularly relevant for evaluating Claude-based agents:
- **Tool Correctness** — did the agent call the right tool?
- **Argument Correctness** — were the tool arguments accurate?
- **Plan Quality/Adherence** — did the agent plan well and follow through?
- **Goal Accuracy** — did the agent accomplish the user's actual intent?

**Execution tracing** via `@observe()` decorator captures nested spans (agent → LLM reasoning → tool calls) for component-level analysis.

**LLM-as-judge:** Uses configurable judge models (Claude, OpenAI, Ollama, etc.) running entirely locally — no external API required unless you use the Confident AI cloud platform.

**CI/CD integration:** Runs via pytest, hooks into GitHub Actions, stores results for regression detection.

### Re: Using DeepEval Within Claude Code

Three integration angles:

**1. MCP Server (native IDE integration)**

DeepEval ships an MCP server via Confident AI. You can register it in `~/.claude/settings.json` and then:
- Run evals, pull datasets, inspect traces directly from Claude Code
- Use `MCPUseMetric`, `MultiTurnMCPUseMetric`, `MCPTaskCompletionMetric` to evaluate Claude's MCP tool usage

**2. Evaluate Claude Code's own outputs**

If you're building scripts/tools that use Claude (e.g., via the Claude API), DeepEval can evaluate:
- Code generation quality (HumanEval, pass@k)
- Agentic task completion (Goal Accuracy, Task Completion)
- Plan quality for multi-step workflows

Use Anthropic's Claude as the judge model too:
```python
from deepeval.models import DeepEvalBaseLLM
# configure Claude (claude-sonnet-4-6) as the evaluator
```

**3. Evaluate the agent-doc / agent-loop workflow**

This repo is a natural fit. You could write evals for:
- Does a `/agent-doc` session correctly respond to user edits? (Answer Relevancy)
- Does the agent follow its plan? (Plan Adherence)
- Are tool calls correct during a session? (Tool Correctness, Argument Correctness)
- Does the final document match the user's intent? (Goal Accuracy)

The `@observe()` tracing would attach to Claude Code tool calls, letting you see exactly where quality degrades across a multi-step session.

**Getting started:**
```bash
pip install -U deepeval   # Python 3.9+
deepeval login            # optional: connect to Confident AI for cloud tracking
```

Write tests in `evals/`, hook `deepeval test run evals/` into the existing eval runner infra in `src/eval-runner/`.

Is DeepEval worth making part of the stack? Can the same analysis be built or is this something that we should have a dependency?
Is DeepEval worth making part of the stack? Can the same analysis be built or is this something that we should have a dependency?

### Re: Build vs. Dependency

**Short answer:** Useful as an ad-hoc research/exploration tool; not worth adding as a core stack dependency given what's already in place.

**Reasoning:**

**The existing eval infra already covers the core need.** `src/eval-runner/` and `evals/agent-doc/diff-interpretation/` represent narrow, purpose-fit evaluations specific to agent-doc behavior. Those are exactly the right shape — they test the specific behavior you care about, not general LLM quality metrics.

**Language mismatch.** DeepEval is Python; the agent-doc stack is Rust. Adding it as a core dependency means either a Python subprocess layer or maintaining separate Python eval tooling that runs alongside the Rust binary. That's friction without a clear payoff.

**The metrics you'd actually use are buildable.** For agent-doc specifically:
- *Answer relevancy* for session responses → straightforward LLM-as-judge prompt
- *Plan adherence* → compare the planned steps in the doc to what was executed
- *Tool correctness* → already partially captured by the existing diff-interpretation evals

The hard part of DeepEval is the breadth (50+ metrics), not the depth. For a focused use case, that breadth is noise.

**Where DeepEval *would* earn its place:**

If agent-loop expands to evaluate general-purpose agent behavior across many dimensions — e.g., if `src/eval-runner` becomes a product rather than internal tooling — then DeepEval's metric library and LLM-as-judge infrastructure would be worth integrating rather than rebuilding. The `@observe()` tracing is particularly well-done; building that from scratch is non-trivial.

**Suggested stance:**
- Keep the existing custom evals as the primary system
- Use DeepEval as a reference: study the metric implementations for ideas on improving custom metrics
- Revisit as a dependency if eval scope grows beyond agent-doc into general LLM output quality across the stack

❯ Can DeepEval scoring Ruberics be used to measure the effectiveness of a practitioner's agent harness and prompts?
❯ How can we know what the expected response is to a given prompt?
### Re: DeepEval Rubrics for Agent Harness Evaluation

❯ Yes — this is actually one of DeepEval's stronger use cases. **G-Eval** (the custom rubric metric) and **DAG** (decision-tree metric) are designed exactly for this: you define what "good" looks like in natural language criteria, and an LLM scores outputs against them.

❯ For a practitioner's harness/prompts specifically:
❯ - G-Eval can score "does the agent follow the workflow the harness specifies?" with criteria written as rubric points
❯ - You can evaluate system prompt effectiveness by running the same user prompt through different system prompts and comparing G-Eval scores
❯ - Task Completion + Plan Adherence metrics measure whether the harness actually shapes agent behavior toward the intended goal

❯ The limitation: rubric quality determines metric quality. A vague rubric gives a noisy signal. But for well-defined workflows (like agent-doc's write→commit→snapshot cycle), you can write precise rubrics.

### Re: Knowing the Expected Response

❯ This is the core challenge of LLM evaluation — the ground truth problem. There's no single answer; it depends on eval type:

❯ | Approach | How | When to use |
❯ |---|---|---|
❯ | **Golden dataset** | Manually curated expected outputs | Narrow, stable tasks; expensive upfront |
❯ | **LLM-as-judge (reference-free)** | Another LLM scores against rubric criteria, no expected output needed | Most open-ended tasks; G-Eval / DAG path |
❯ | **Behavioral invariants** | Test properties that must hold, not exact output | Structural constraints (output format, tool must be in allowed set) |
❯ | **Regression baseline** | Compare to human-approved previous outputs | Catch degradation without needing perfect ground truth |
❯ | **Task oracle** | Run the output to verify (code compiles/runs, structured output validates schema) | Code gen, JSON output, function calls |

❯ Does DeepEval use all of these approaches or only LLM-as-judge?

❯ For agent harness evaluation specifically, the most tractable path is **behavioral invariants + LLM-as-judge**:
❯ - Invariants handle the structural (did the right tool get called? was the response committed?)
❯ - LLM judge handles the qualitative (was the response relevant? did it follow the plan?)

❯ You never fully know the expected response — you define a quality space, then measure whether outputs fall within it. The more constrained the task, the tighter that space gets.
### Re: DeepEval's Evaluation Approaches (HEAD)

DeepEval spans multiple approaches, though LLM-as-judge dominates:

- **LLM-as-judge** — the default for most quality metrics (G-Eval, Answer Relevancy, Faithfulness, etc.)
- **Task oracle** — HumanEval and pass@k actually execute code and check results; no LLM involved
- **Golden dataset (reference-based)** — metrics like AnswerRelevancy accept `expected_output` as a reference, though an LLM still does the comparison reasoning
- **Custom/invariant** — `BaseMetric` lets you implement any scoring logic without an LLM; pure Python checks
- **Regression** — not a first-class metric, but the Confident AI cloud platform stores scores over time and surfaces regression across runs

So the complete picture: LLM-as-judge is the default path, task oracle for code evaluation, and custom metrics for structural invariants. The framework is composable — you mix approaches within a test suite.
<!-- agent:boundary:c9fab718 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
