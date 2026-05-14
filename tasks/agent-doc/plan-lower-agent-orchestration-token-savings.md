# Plan: Lower-agent orchestration for token savings

## Purpose

Design an agent-doc orchestration layer that uses smaller, cheaper agents for
bounded work while preserving correctness through explicit context shaping,
structured proof, and high-model review only where it adds value.

The goal is not to make lower-tier models solve broad ambiguous tasks. The goal
is to stop handing them broad ambiguous tasks. agent-doc should turn a session
prompt into typed job packets with clipped context, objective outputs, and clear
escalation rules. tsift supplies the context-shaping layer that makes those job
packets small enough to be useful.

## Target Outcome

- agent-doc remains the durable control plane: prompt history, queue/backlog,
  closeout proof, job packet references, operation docs, and final response.
- A high or medium model performs a thin orchestration pass only when needed:
  classify the prompt, split work, choose model tiers, and define proof.
- Lower agents receive job packets, not whole sessions.
- tsift builds bounded context packs from repo state, session history, diffs,
  symbols, tests, logs, and prior outcomes.
- The parent agent reviews compact structured results rather than full worker
  transcripts.
- The system measures token savings and failure modes per task class.

## Vocabulary

- **Agent doc**: the durable session document and user interface.
- **Job packet**: a short-lived, bounded execution document for one worker.
- **Operation doc**: retained audit document for a larger multi-job operation.
- **Runbook**: reusable procedure or policy.
- **Plan**: intended work and implementation phases.
- **Context packet**: tsift-generated evidence bundle used inside a job packet.

## Architecture

```text
user edit in agent doc
        |
        v
agent-doc preflight / plan
        |
        v
orchestration planner
        |
        +--> tsift context planner
        |       |
        |       +--> source handles
        |       +--> symbol/file neighborhoods
        |       +--> diff/test/log digests
        |       +--> historical routing outcomes
        |
        v
job packets
        |
        +--> lower agent: search / file map / repro
        +--> lower agent: small scoped patch
        +--> lower agent: spec or test update
        +--> medium/high agent: ambiguous design or merge review
        |
        v
structured proof bundle
        |
        v
parent review, verification, finalize
```

## Routing Policy

### Lower-agent default tasks

Route these to lower-cost workers first when the scope is clear:

- repo search and file discovery
- symbol map generation
- failing test/log summarization
- small single-file patches
- docs/spec alignment
- fixture updates
- deterministic repro construction
- command output summarization
- stale context detection

### Medium/high model tasks

Escalate earlier for:

- ambiguous product intent
- cross-module design changes
- concurrency, CRDT, routing, tmux, or git invariants
- security/privacy-sensitive decisions
- broad refactors
- conflicts between worker outputs
- repeated lower-agent failure on the same task class

### Structural routing signals

`agent-doc plan` should emit routing fields derived from the document and repo,
not from vague model preference:

```json
{
  "dispatch_candidate": true,
  "task_class": "cross_module_bugfix",
  "risk": "high",
  "parallelizable": true,
  "suggested_parent_tier": "high",
  "job_packet_budget_tokens": 6000,
  "required_proof": ["changed_paths", "commands", "confidence", "escalation"]
}
```

Useful signals:

- number of prompt targets
- pending item count and age
- touched path count
- diff size
- presence of failing tests/logs
- known high-risk modules
- requested presets
- previous routing outcomes for similar task classes
- tsift index freshness

## Job Packet Format

Job packets should be filesystem-native markdown documents, generated under
`.agent-doc/jobs/<cycle>/<job-id>.md` by default. They are usually ignored or
deleted after success, and preserved on failure or when audit mode is enabled.

Required fields:

```markdown
# Job Packet: <title>

parent_doc: tasks/agent-doc/agent-doc-bugs2.md
cycle_id: cycle-...
job_id: ...
task_class: repo_search | patch | test | spec | review
model_tier: low | med | high
write_scope:
  - src/agent-doc/src/write.rs
context_budget_tokens: 6000
source_snapshot: <git sha>
tsift_index: fresh | stale | missing

## Goal

## Allowed Commands

## Required Context

## tsift Handles

## Acceptance Criteria

## Output Schema

## Escalation Conditions
```

Worker responses should be structured:

```json
{
  "status": "complete|blocked|escalate",
  "changed_paths": [],
  "commands_run": [],
  "findings": [],
  "proof": [],
  "confidence": "low|medium|high",
  "needs_parent_attention": []
}
```

## tsift Workstream

tsift is the context-shaping dependency. It should remain optional at first:
agent-doc can run without it, but orchestration quality improves when it is
available and fresh.

### Capabilities agent-doc needs from tsift

- `context-pack`: generate a bounded packet for a prompt, file set, or symbol.
- `source-read`: return stable source windows by handle.
- `search`: lexical and semantic search over code, specs, docs, and sessions.
- `diff-digest`: summarize changed files and likely affected symbols.
- `test-digest`: summarize failing test output with linked source handles.
- `log-digest`: summarize tmux/CI/session logs with relevant spans.
- `graph`: return symbol/file/test neighborhoods.
- `session-review`: summarize prior agent-doc cycles and unresolved context.
- `index-status`: report freshness, stale paths, and recommended reindex.
- `routing-history`: store task class, model tier, packet size, result, and
  verification outcome.

### tsift data model additions

- Add content types for agent-doc sessions, job packets, operation docs, and
  verification logs.
- Store stable handles for source windows, prompt spans, response spans, test
  failures, commits, and backlog items.
- Track relationships:
  - prompt -> job packet
  - job packet -> source handles
  - job packet -> worker result
  - worker result -> changed paths
  - changed paths -> tests
  - task class -> model tier -> outcome
- Record token estimates for parent prompt, job packet, worker response, and
  parent review.

### tsift CLI contracts

Initial command shape:

```text
tsift context-pack --for-agent-doc <doc> --prompt-target <id> --budget 6000
tsift source-read <handle>
tsift diff-digest --base <sha> --head <sha>
tsift test-digest --command "make check" --log <path>
tsift routing-history record --task-class <class> --tier <tier> --outcome <outcome>
```

agent-doc should consume JSON envelopes from tsift. Human-readable output can
exist, but orchestration should rely on stable machine-readable fields.

## Operation Docs

For larger orchestrations, agent-doc should create an operation doc under a
tracked path such as:

```text
tasks/agent-doc/operations/<operation-id>.md
```

An operation doc records:

- original user prompt
- planning decision
- job packet list
- assigned model tiers
- tsift context pack summaries
- worker results
- merge decisions
- verification evidence
- token/cost estimates
- follow-up backlog

The session document should link to the operation doc and summarize the outcome,
not absorb the entire worker transcript.

## Token Savings Model

Savings come from avoiding repeated full-context reads:

1. Parent reads session diff and minimal related context.
2. Planner creates narrow job packets.
3. Workers receive only job-specific context.
4. Parent reviews structured proof and selected handles.
5. Full repo/session context is pulled only for ambiguity or failure.

Metrics to record per job:

- parent planning tokens
- tsift context packet estimated tokens
- worker input/output tokens
- parent merge/review tokens
- verification commands
- success/failure/escalation
- number of source handles expanded
- whether a higher-tier retry was needed

Useful target metrics:

- reduce average prompt tokens for repo-search/test-triage jobs by 60% or more
- keep lower-agent retry rate below 20% for low-risk task classes
- keep parent review under 25% of the original full-context prompt size
- never let token saving bypass required verification or closeout

## Implementation Phases

### Phase 1: Manual packet protocol

- [ ] Define job packet and worker result schemas in an agent-doc spec.
- [ ] Add a runbook for manually creating job packets from a session prompt.
- [ ] Add examples for search, test triage, small patch, and spec update jobs.
- [ ] Preserve job packets on failure; delete or ignore them on success.
- [ ] Add closeout guidance for summarizing worker proof in the parent document.

### Phase 2: tsift context packets

- [ ] Add tsift JSON output for `context-pack`, `source-read`, `diff-digest`,
      `test-digest`, and `index-status`.
- [ ] Add agent-doc session and job packet content types to tsift indexing.
- [ ] Add source/prompt/test handles stable enough for worker packets.
- [ ] Add freshness checks so stale tsift context gates orchestration or emits a
      clear warning.
- [ ] Add token estimates for generated context packets.

### Phase 3: agent-doc planning integration

- [ ] Extend `agent-doc plan` with task class, dispatch candidate, model tier,
      context budget, write scope, and required proof fields.
- [ ] Teach `agent-doc plan` to request tsift context when available.
- [ ] Add a fallback path when tsift is missing or stale.
- [ ] Add a frontmatter opt-in such as `agent_doc_dispatch: auto`.
- [ ] Add tests for routing decisions from structural document signals.

### Phase 4: binary-owned job packet generation

- [ ] Add `agent-doc jobs create <FILE>` to generate job packets from a plan.
- [ ] Add `agent-doc jobs list/status/collect` commands.
- [ ] Add optional preservation mode for audit.
- [ ] Add operation-doc generation for multi-job workflows.
- [ ] Ensure job packet paths and operation docs are linked from closeout.

### Phase 5: orchestration runner

- [ ] Route lower-tier jobs through harness-native subprocesses or managed
      agents with isolated write scopes.
- [ ] Support parallel independent jobs and sequential dependent jobs.
- [ ] Require structured worker result envelopes.
- [ ] Merge worker results through parent review, not blind patch application.
- [ ] Run required verification before finalize when any code/spec/test changed.

### Phase 6: learning loop

- [ ] Record routing outcomes in tsift.
- [ ] Build per-task-class success/retry/escalation reports.
- [ ] Adjust routing thresholds based on observed outcomes.
- [ ] Add dashboards or reports for token savings versus failure rate.
- [ ] Promote task classes to lower tiers only after deterministic evidence.

## Safety Invariants

- The parent session document remains authoritative.
- Job packets are derived artifacts unless explicitly promoted to operation docs.
- Lower agents must not edit outside declared write scope.
- No worker output is final until parent review and required verification pass.
- Stale tsift context must be visible and actionable.
- Token savings never override closeout, tests, git staging boundaries, or
  session-check.
- Every orchestration result must be reproducible from committed session state,
  source snapshot, job packet, and worker proof.

## Regression Coverage

- Unit: task classification and routing fields from `agent-doc plan`.
- Unit: job packet rendering and parsing.
- Integration: generate job packets from a multi-target session prompt.
- Integration: tsift missing/stale/fresh behavior.
- Integration: lower-agent result collection and merge summary.
- Integration: failed worker escalation to parent/high tier.
- SimWorld: parallel workers with conflicting outputs.
- SimWorld: stale context packet rejected before dispatch.
- SimWorld: successful low-tier search + medium-tier patch + high-tier review.

## Open Decisions

- Should job packets live under `.agent-doc/jobs` only, or can operation-owned
  packets be tracked under `tasks/agent-doc/operations`?
- Should tsift be a binary dependency, optional subprocess, or plugin provider?
- Which harnesses can safely run lower agents with write scopes today?
- Should lower-agent patches be applied directly to worktrees, or returned as
  patch artifacts for parent application?
- What is the first production task class to promote: repo search, test triage,
  or docs/spec update?

## MVP Recommendation

Start with manual job packets plus tsift context packets. That proves the main
token-saving mechanism without committing to full autonomous orchestration.

The first useful workflow:

1. `agent-doc plan` identifies a dispatch candidate.
2. Parent creates two or three markdown job packets.
3. tsift fills each with bounded handles and digests.
4. Lower agents complete search/test/docs tasks.
5. Parent reviews structured results, performs any risky code work, verifies,
   and finalizes.

Only after that works repeatedly should agent-doc spawn lower agents
automatically.
