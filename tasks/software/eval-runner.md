---
agent_doc_session:
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

# eval-runner

<!-- agent:status -->
**Status:** Scaffolded | **Location:** `src/eval-runner/` | **Tests:** builds clean
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**Standalone Rust CLI crate** at `src/eval-runner/`. Reads YAML eval case definitions, sends prompts to Claude API, uses LLM-as-judge scoring, reports results to CLI.

**Core pipeline:** Parse `cases.yaml` -> build prompt from diff + context -> send to Claude API -> get response -> LLM-grade against `expected_behavior` -> report pass/fail with score + reasoning

**CLI interface:**
```bash
eval-runner run <cases.yaml> [--model <model>] [--judge-model <model>] [--threshold <0.0-1.0>] [--json] [--baseline <file>] [--system-prompt <file>]
```

**Key design decisions:**
- Custom Rust CLI over Promptfoo/Inspect AI to avoid Node.js/Python dependencies
- LLM-as-judge with 0.0-1.0 continuous scoring (not just binary pass/fail)
- Compatible with existing `cases.yaml` format from `evals/agent-doc/diff-interpretation/`
- Regression tracking via `--baseline` flag comparing against prior run results
- Exit code 0/1 for CI integration (`make check`)

**Eval case format** (from `cases.yaml`):
```yaml
- id: case-name
  diff: |
    --- snapshot
    +++ document
    @@ ...
  document_context: |
    Full document content...
  expected_behavior:
    - "Expected behavior description 1"
    - "Expected behavior description 2"
  grading:
    pass_threshold: 0.7
```

**Metrics tracked:**
- Per-case: score (0.0-1.0), latency (ms), token count (input/output), cost, response length
- Aggregate: pass rate, mean score, p50/p95 latency, total tokens, total cost
- Regression: delta vs baseline run, flagged case-level regressions

**Relationship to other tools:**
| Tool | Role |
|------|------|
| **module-harness** | Audit instruction files, validate specs, generate harness context |
| **eval-runner** | Read YAML cases, send prompts to LLM, grade responses, report results |
| **agent-doc** | Produces the SKILL.md and diff-interpretation behavior being evaluated |

**Market context:**
- Promptfoo: closest match (YAML cases, llm-rubric grading, CLI-first) but requires Node.js
- Inspect AI: rich log viewer + static HTML bundles, Python-based
- DeepEval: pytest-based, Python
- This crate: same patterns, no external runtime dependency, ~300 LOC Rust
<!-- /agent:architecture -->

## Exchange

<!-- agent:exchange patch=append -->
### Context (transferred from tasks/software/agent-doc.md)

**Origin:** Eval-runner was designed and scaffolded during the agent-doc session on 2026-03-26. The discussion covered:

1. **Motivation:** agent-doc has 21 eval cases in `evals/agent-doc/diff-interpretation/cases.yaml` but no runner. Module-harness holds eval criteria/definitions but shouldn't own execution.

2. **Design process:** Evaluated Promptfoo, Inspect AI, DeepEval, Braintrust, and custom Rust. Chose custom Rust for zero external dependencies and tight integration with existing YAML format.

3. **Implementation:** ~300 LOC scaffolded at `src/eval-runner/`. Parses all 21 cases. Claude API client for both response generation and LLM-as-judge. ANSI-colored CLI output. JSON output for CI. Regression tracking.

4. **Quantifiable measurements:** 0.0-1.0 continuous score from LLM judge, per-case latency/tokens/cost, aggregate pass rate/mean score/p50/p95 latency, regression detection vs baseline.

5. **Open items:** ANTHROPIC_API_KEY not yet in `pass`. No end-to-end run completed. HTML/SVG/dashboard output deferred.

## User
### Context (transferred from tasks/software/agent-loop.md, 2026-04-03)

**YAML vs TOML for eval cases:**

The question was raised whether to switch from YAML to TOML for eval case definitions.

**TOML pros:** Rust ecosystem alignment, stricter typing, no YAML gotchas (yes/no as booleans).
**TOML cons:** Multi-line strings more verbose, nested arrays noisier, 30+ cases = longer files.

**Eval framework ecosystem survey:**
- OpenAI Evals, Braintrust, promptfoo, Inspect AI, HELM, LangSmith — all use YAML/JSONL
- No major eval framework uses TOML
- YAML dominates because: Python-centric tooling, multi-line string handling, HuggingFace dataset standards

**Decision: Keep YAML.** Eval cases are *data* not *config*. The data ecosystem is YAML/JSON. Interoperability with other eval tools, datasets, and CI pipelines favors YAML. The YAML gotchas are mitigated by serde_yaml deserializing to typed `String` fields.
<!-- agent:boundary:3b8d0bd4 -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] Set up ANTHROPIC_API_KEY in `pass` and run full eval suite
- [ ] Test with agent-doc diff-interpretation cases end-to-end
- [ ] Add `make check` integration
- [ ] JSON output format documentation
- [ ] Regression tracking with `--baseline` end-to-end test
<!-- /agent:pending -->

## Todo (HEAD)

<!-- agent:todo -->
- [ ] HTML/SVG report output
- [ ] Promptfoo-compatible JSON export (for `promptfoo view`)
- [ ] Inspect AI log format export (for `inspect view`)
- [ ] Cost estimation before run (`--dry-run`)
- [ ] Parallel case execution
- [ ] Custom grading rubric per case
<!-- /agent:todo -->
