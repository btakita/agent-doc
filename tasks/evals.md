---
agent_doc_session: f752ad0f-ae8c-4a55-9865-2a0f845fea38
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Sonnet baseline complete — 11/21 (52.4%), 5 failure clusters identified
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Please propose a plan for continuous evals to improve the effectiveness of the agent harnesses

Start with https://platform.claude.com/docs/en/test-and-evaluate/develop-tests

### Plan: Continuous Evals for Agent Harnesses

Drawing from Anthropic's [eval development guide](https://platform.claude.com/docs/en/test-and-evaluate/develop-tests), here's a phased plan tailored to your agent-loop ecosystem (agent-doc, corky, Claude Code skills).

#### Phase 1: Define Success Criteria per Harness

**agent-doc (SKILL.md orchestration)**
- **Diff interpretation fidelity**: Does the agent correctly identify user intent from diffs? (incomplete edits, sarcasm, multi-topic)
- **Template patch correctness**: Are patch blocks well-formed and targeting the right components?
- **Pending drift**: Does the pending component stay in sync with actual work state?
- **Concurrent edit preservation**: Are user edits during response preserved via CRDT merge?

**corky (email/correspondence)**
- **Voice consistency**: Does output match voice.md guidelines?
- **Threading correctness**: Are reply chains (In-Reply-To, subject Re:) properly formed?
- **Safety compliance**: No auto-send, no secrets leaked, PHI/PII handling

**Claude Code skills (dev, testing, email, etc.)**
- **Tool selection accuracy**: Does the skill pick the right tool for the job?
- **Instruction adherence**: Does the skill follow CLAUDE.md conventions?
- **Error recovery**: Does the skill self-correct on failures vs. brute-forcing?

#### Phase 2: Build Eval Test Suites

**Recommended eval types by harness:**

| Harness | Eval Type | Grading Method |
|---------|-----------|----------------|
| agent-doc diff interpretation | Golden-answer pairs (diff → expected response intent) | LLM-graded rubric |
| agent-doc patch output | Structural validation (well-formed patches, correct targets) | Code-graded (parse + validate) |
| agent-doc pending sync | State comparison (expected pending vs actual) | Exact match |
| corky voice/tone | Likert scale (1-5 on voice.md criteria) | LLM-graded |
| corky threading | Header validation (Message-ID, In-Reply-To, Subject) | Code-graded |
| skill tool selection | Golden-answer (scenario → expected tool) | Exact match |
| skill instruction adherence | Rubric check against CLAUDE.md rules | LLM-graded |

**Test case structure:**
```
evals/
├── agent-doc/
│   ├── diff-interpretation/    # diff input → expected intent
│   ├── patch-correctness/      # template doc → expected patches
│   └── pending-sync/           # state transitions → expected pending
├── corky/
│   ├── voice-consistency/      # prompts → rated outputs
│   └── threading/              # conversation chains → header checks
└── skills/
    ├── tool-selection/         # scenarios → expected tool calls
    └── instruction-adherence/  # CLAUDE.md rules → compliance checks
```

#### Phase 3: Automation & CI Integration

1. **Eval runner script** — Python or Rust CLI that:
   - Loads test cases from `evals/` directory
   - Runs them against Claude API (or local binary for code-graded)
   - Outputs scores per category + overall
   - Stores results in `evals/results/` with timestamps

2. **Regression detection** — Compare current run against baseline scores. Flag any category that drops >5%.

3. **Triggered evals:**
   - On SKILL.md changes → run skill adherence evals
   - On agent-doc binary changes → run patch correctness + pending sync evals
   - On voice.md changes → run voice consistency evals

#### Phase 4: Continuous Improvement Loop

1. **Failure analysis**: When evals fail, classify root cause (prompt drift, missing edge case, binary bug)
2. **Test case expansion**: Use Claude to generate variations from failing cases
3. **Prompt iteration**: A/B test prompt changes against the eval suite before deploying
4. **Score tracking**: Dashboard or markdown table tracking scores over time

#### Recommended Starting Point

Start with **agent-doc diff interpretation** evals — highest-leverage area since it's the core reasoning loop every session depends on. Create 20-30 golden-answer test cases from real session diffs (plenty in git history), then expand with Claude-generated variations.
Continue
### Re: Phase 1 Kickoff

Understood — starting Phase 1 now. First priority is the agent-doc diff interpretation evals since that's the highest-leverage entry point. I'll:

1. Create `evals/` directory structure
2. Extract 10–15 real diffs from git history (interesting edge cases: multi-topic edits, incomplete fragments, sarcasm)
3. Write golden-answer test cases (diff → expected intent)
4. Build minimal eval runner using the Anthropic SDK

Once that's working, we can expand to corky threading and skill adherence.
### Re: Phase 1 Status

Phase 1 is nearly complete — prior sessions already built most of it:

- **`evals/agent-doc/diff-interpretation/`** — directory exists with `cases.yaml` (20 cases, includes real git history diffs) and `baselines/` directory
- **`src/eval-runner/`** — Rust eval runner built with Claude CLI + Anthropic API backends, LLM-as-judge scoring, parallel execution, regression detection
- **`baselines/haiku-2026-04-03.json`** — Haiku baseline complete: 9/21 passed (42.9%), mean score 0.57
- **`baselines/sonnet-2026-04-04.json`** — exists but empty (0 bytes) — running now

Running Sonnet baseline against all 20 cases.
### Re: Sonnet Baseline Results (HEAD)

Sonnet baseline complete. 21 cases, 4 parallel workers.

**Results: 11/21 passed (52.4%), mean score 0.560**

| Model | Passed | Pass Rate | Mean Score |
|-------|--------|-----------|------------|
| haiku-2026-04-03 | 9/21 | 42.9% | 0.570 |
| sonnet-2026-04-04 | 11/21 | 52.4% | 0.560 |

Sonnet passes +2 more cases but mean score is nearly identical — both models are struggling in the same areas.

**Per-case breakdown:**

| Case | Sonnet Score | |
|------|-------------|--|
| simple-question | 0.40 | FAIL — didn't mention `&lt;!-- agent:name --&gt;` syntax |
| incomplete-fragment | 0.70 | PASS |
| multi-topic | 0.85 | PASS |
| comment-content-ignored | 0.00 | FAIL — responded to comment scratchpad content |
| blockquote-annotation | 0.30 | FAIL — acknowledged preference, didn't act |
| deletion-of-content | 1.00 | PASS |
| truncated-sentence | 0.75 | PASS |
| structural-reorg | 1.00 | PASS |
| approval-to-implement | 0.10 | FAIL — described plan instead of implementing |
| mixed-valid-and-comment | 0.40 | FAIL — addressed task but also responded to comments |
| inline-colon-annotation | 0.85 | PASS |
| boundary-only-change | 0.50 | FAIL — verbose where silence expected |
| head-label-removal | 0.10 | FAIL — confirmed cleanup, should have been silent |
| single-word-approval | 0.00 | FAIL — "go" treated as new topic, not approval |
| pasted-terminal-output | 0.85 | PASS |
| multi-item-instruction-list | 0.10 | FAIL — asked questions instead of executing |
| single-word-correction | 0.85 | PASS |
| mid-sentence-truncated-real | 0.60 | FAIL — correct flag but excessive explanation |
| inline-annotation-with-truncation | 0.85 | PASS |
| multi-topic-approval-plus-questions | 0.72 | PASS |
| url-reference-question | 0.85 | PASS |

**Failure clusters:**

1. **Comment leakage** (comment-content-ignored, mixed-valid-and-comment) — responding to `[//]: # (...)` scratchpad that should be invisible
2. **Silence vs. response** (boundary-only-change, head-label-removal) — responding when no-op is correct
3. **Approval interpretation** (single-word-approval, approval-to-implement) — "go"/"yes" → implement, not describe
4. **Verbosity on partials** (mid-sentence-truncated-real) — flag + stop, not flag + explain
5. **Multi-item execution** (multi-item-instruction-list) — execute the list, don't confirm first

**Meta:** `single-word-approval` scored 0.00 — this model interpreted "go" (the trigger for this session) as a new topic rather than approval. The case exists exactly for this failure mode.

**Next:** Cross-reference Haiku per-case scores, then pick a failure cluster to attack via SKILL.md improvements.
<!-- agent:boundary:495bbe61 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Approve Phase 1 plan
- [x] Create `evals/` directory structure with agent-doc/diff-interpretation/
- [x] Extract 10-15 real session diffs from git history as test fixtures (20 cases total)
- [x] Write first 10 diff interpretation test cases (diff input → expected intent)
- [x] Build eval runner (Rust CLI, src/eval-runner, with Claude CLI + Anthropic API backends)
- [x] Haiku baseline: 9/21 passed (42.9%), mean score 0.57
- [x] Sonnet baseline: 11/21 passed (52.4%), mean score 0.56
- [ ] Cross-reference Haiku vs Sonnet per-case scores
- [ ] Pick failure cluster to attack first
- [ ] Improve SKILL.md/system prompt based on failure patterns
- [ ] Re-run evals to measure improvement
<!-- /agent:pending -->
