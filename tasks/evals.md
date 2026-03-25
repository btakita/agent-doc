---
agent_doc_session: f752ad0f-ae8c-4a55-9865-2a0f845fea38
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Planning — proposed eval framework for agent harnesses
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
<!-- agent:boundary:a6148076 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Create `evals/` directory structure
- [ ] Write first 10 agent-doc diff interpretation test cases from real sessions
- [ ] Build minimal eval runner (Python script using Anthropic SDK)
- [ ] Establish baseline scores
- [ ] Expand to corky voice/threading evals
- [ ] Add CI integration for triggered evals
<!-- /agent:pending -->
