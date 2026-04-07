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
# configure Claude (claude-sonnet-4-6) as the evaluator (HEAD)
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
<!-- agent:boundary:56f420e3 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
