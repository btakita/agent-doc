---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: claude-opus-4-6
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Recorded Codex capability proof verification.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260514-211200.md`*

Compacted content:
- Archived 2 response topic(s): log review; managed cycle closeout bugs
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Please read https://www.linkedin.com/posts/andrei-trandafira-6282a813_pushed-gpt-54-mini-from-481-to-93-on-activity-7459581138357923840-M6Nd?utm_source=share&utm_medium=member_desktop&rcm=ACoAAAC0KQ4BlxIAK5Vbm7Gssr6SzIaiyNG0e68

❯ How can agent-doc help orchestrate smaller models to save on tokens yet be effective?
### Re: smaller-model orchestration — gpt-5

I could read the public LinkedIn page, but only the visible public text was available: Andrei Trandafira says he pushed `gpt 5.4-mini` from `48.1%` to `93%` on one ProgramBench task using the Adeu reasoning harness. The important read is not that the small model became globally equivalent to a larger one; it is that harness structure can turn a cheap model from a one-shot generator into a disciplined search-and-verification worker. ProgramBench itself is useful context here because it emphasizes whole-program reconstruction, architectural decisions, generated behavioral tests, and the fact that generic harness scores remain very low while task-specific scaffold tuning can move outcomes a lot.

For agent-doc, the useful direction is to make orchestration explicit and auditable instead of treating model choice as a chat setting. The session document can be the control plane, while the CLI enforces the cycle boundary:

1. Use a stronger model to write the task contract: goal, repo scope, acceptance tests, known risks, and escalation triggers.
2. Split work into bounded document-native jobs: investigation, repro, spec update, test creation, mechanical implementation, verification, and closeout.
3. Route cheap/smaller models to jobs with narrow context and objective outputs: `rg` findings, file maps, failing-test summaries, small patches, benchmark runs, and docs deltas.
4. Keep the expensive model out of the loop until it is actually needed: ambiguity, conflicting evidence, broad architecture changes, security/concurrency risk, or repeated small-model failure.
5. Require every worker to return structured proof: changed paths, commands run, failure text, confidence, and whether escalation is needed.
6. Let `agent-doc finalize` / `write --commit` own the final merge into the session document so the orchestration has a durable audit trail instead of disappearing into console-only output.

The token-saving mechanism is mostly context shaping. Smaller models waste tokens when they are handed the whole repo plus a vague mission. They become useful when agent-doc gives them a clipped packet: exact prompt target, relevant files, allowed write set, expected artifact, and verification command. That also means the parent model can review a compact evidence bundle instead of rereading every file.

A concrete agent-doc design would be:

- `agent-doc plan` classifies each prompt target into job types and emits `model_tier`, `context_budget`, `write_scope`, `required_proof`, and `escalation_condition`.
- `agent-doc orchestrate` fans out independent jobs to mini-tier workers first.
- Workers write back only structured result blocks or scoped patches, not freeform giant transcripts.
- The parent cycle consumes those result blocks, resolves conflicts, runs full verification when code changed, and finalizes the document.
- Historical outcomes feed a routing table: if mini succeeds on `repo_search`, `test_triage`, or `single-file patch` tasks, keep using it; if it fails twice on `architecture`, `cross-module concurrency`, or `ambiguous product behavior`, promote earlier next time.

That matches the lesson from the post while preserving ProgramBench's caution: harness wins are real, but they should be measured per task class. Agent-doc can help by making those task classes, routing decisions, and proof requirements first-class data in the document rather than implicit model vibes.

Sources: [LinkedIn post](https://www.linkedin.com/posts/andrei-trandafira-6282a813_pushed-gpt-54-mini-from-481-to-93-on-activity-7459581138357923840-M6Nd?utm_source=share&utm_medium=member_desktop&rcm=ACoAAAC0KQ4BlxIAK5Vbm7Gssr6SzIaiyNG0e68), [ProgramBench](https://programbench.com/), [ProgramBench paper](https://arxiv.org/abs/2605.03546).
❯ Should I make tsift public and publish to assist with the context shaping?
### Re: publishing tsift for context shaping — gpt-5

Yes, but I would separate "public" from "published package" and do it in stages.

Making `tsift` public is useful for context shaping because it gives agent-doc a concrete, inspectable companion tool: bounded source reads, search handles, context packs, session review, cost/session digests, and stale-index reminders are exactly the surfaces that let smaller models work from clipped evidence instead of whole-repo dumps. Public source also makes the thesis easier to explain: agent-doc is the document/control plane; tsift is the context-shaping and evidence-compaction layer.

I would publish the repository first, then package only after the install story is clean. A public repo with a good README, examples, recorded token-savings fixtures, and an `agent-doc + tsift` workflow demo is enough to support the idea. Publishing to crates.io or a binary channel should wait until the CLI contract is stable enough that early users will not be broken by command renames or output schema churn.

The practical sequence I would use:

1. Make the GitHub repo public once secrets, local paths, private session fragments, and accidental personal/work data are audited.
2. Add a top-level README that frames tsift as "context shaping for coding agents", not as generic search.
3. Include two demos: one `context-pack` flow for an agent-doc session, and one `source-read/search/explain` flow showing how a small model gets bounded handles.
4. Mark JSON/envelope outputs as either stable or experimental so integrations know what they can rely on.
5. Publish binaries or crates.io only after `tsift init`, `status --fix`, and the common `search -> source-read -> context-pack` workflow feel boring on a clean machine.

The main risk is privacy, not competition. tsift's value is tied to local repo/session/log material, and that kind of tooling easily grows assumptions from your own workspace. Before making it public, I would run an explicit release audit for hardcoded paths, fixtures, archived logs, private project names, API/provider assumptions, and README claims that imply a more general product than the CLI currently supports.

So my answer is: make it public if the audit is clean; do not rush package publication. A public repo plus repeatable demos will help agent-doc's smaller-model orchestration story immediately, while package publication should be treated as a separate distribution-readiness milestone.
❯ If agent-doc depends on tsift, can this assist with orchestration?
❯ Would memgraph be a good augmentation on top of sift + sqlite? Would memgraph replace sqlite instead?
### Re: tsift dependency and Memgraph augmentation — gpt-5

Yes, agent-doc depending on tsift can help orchestration, but I would make it a capability dependency rather than a hard correctness dependency at first.

The useful split is: agent-doc owns the durable workflow contract, while tsift owns context shaping. `agent-doc preflight` / `plan` can decide what work needs to happen; tsift can provide bounded packets for that work: `session-review --next-context`, `context-pack`, `search`, `source-read`, `graph`, `explain`, `diff-digest`, `test-digest`, and `log-digest`. That lets smaller models receive a tight job packet with handles and expansion commands instead of a vague prompt plus a large repo dump.

For orchestration, the concrete win would be a routing loop like this:

1. agent-doc classifies the prompt into task type, risk level, write scope, and required proof.
2. tsift builds the context packet: active prompt tail, touched files, touched symbols, fresh/stale index state, recent failures, relevant source windows, graph neighbors, and digest commands.
3. smaller workers get only that packet plus allowed commands and output schema.
4. agent-doc reviews structured proof, escalates only when ambiguity or risk crosses a threshold, then finalizes the document.

That is exactly the kind of context discipline that makes smaller models viable. The dependency risk is bootstrap and failure mode: agent-doc must still be able to answer and close a document if tsift is missing, stale, or broken. So I would design this as `agent-doc detects tsift -> uses tsift orchestration packets`, with fail-closed warnings for stale context, rather than making every document cycle require a healthy tsift index immediately.

On Memgraph: I would treat it as an augmentation on top of sift + SQLite, not a SQLite replacement.

SQLite is still the right local source of truth for tsift's current index: embedded, durable, zero service dependency, easy to snapshot, easy to ship, good enough for symbols, call edges, summaries, file state, and CLI reads. It keeps tsift usable in any repo without asking the user to run infrastructure.

Memgraph becomes interesting when the graph stops being just "symbols and call edges in one checkout" and starts becoming an orchestration graph:

- tasks, prompts, backlog items, claims, commits, files, symbols, tests, failures, agents, model tiers, and verification evidence as connected nodes
- historical routing outcomes: which model tier succeeded for which task class, which context packet was enough, when escalation was needed
- cross-session memory: repeated failures, recurring files, stale assumptions, ownership boundaries, and successful runbooks
- multi-hop questions: "what files are implicated by this prompt through prior failures and recent commits?", "which pending item depends on this flaky test?", "what small-model job packet should be generated for this class of issue?"

That is where Memgraph's current positioning lines up: their docs emphasize graph traversal plus vector search in one engine, GraphRAG pipelines as Cypher queries, traceable reasoning paths, and MCP/agent integrations. Those are useful for shared or long-lived orchestration state. They are overkill for tsift's local AST index.

So the architecture I would aim for is:

- keep SQLite as tsift's canonical local index
- keep sift/ripgrep as the fast lexical/exact retrieval layer
- optionally export/project selected tsift + agent-doc facts into Memgraph for graph-level orchestration
- make Memgraph a service-backed accelerator for teams, dashboards, and advanced routing, not a required local dependency

If this becomes productized, the clean boundary is `tsift graph-export` or `agent-doc orchestration-export`: emit nodes/edges from the committed document state and tsift indexes, then let Memgraph answer graph questions. Do not put Memgraph in the hot path until the graph queries prove they produce better worker packets or better routing decisions than SQLite-backed tsift alone.

Sources: [Memgraph overview](https://memgraph.com/), [Memgraph Vector Search](https://memgraph.com/vector-search), [Memgraph GraphRAG](https://memgraph.com/graphrag), [Memgraph AI docs](https://www.memgraph.ai/docs).
❯ Would either short-lived or long-lived "virtual documents" help with bounded context? These "virtual-documents" can be created on the filesystem. Short-lived "virtual documents" would be deleted once the job is complete. A git commit can be useful for auditing.
### Re: virtual documents for bounded context — gpt-5

Yes. Virtual documents are a strong fit for bounded context, as long as they are treated as derived job artifacts rather than a second source of truth.

The main agent-doc session document should remain the durable user interface: prompt, response, backlog, done archive, and closeout proof. Virtual documents can sit underneath it as scoped context packets for workers. That gives smaller models a filesystem-native artifact they can read without inheriting the whole parent exchange.

I would use two classes:

1. Short-lived job documents: created under something like `.agent-doc/jobs/<cycle>/<job-id>.md`, handed to a worker, then deleted or archived after the parent cycle finalizes.
2. Long-lived memory documents: committed under a stable path when the artifact has reuse value, such as a release audit, subsystem map, routing outcome table, failure taxonomy, or benchmark result.

Short-lived virtual documents help most with orchestration. They can contain exactly the worker contract:

- parent document path and cycle id
- task type and acceptance criteria
- allowed write scope
- tsift context-pack/source-read/search handles
- relevant files and symbol windows
- commands the worker may run
- required response schema
- escalation conditions

That is much cleaner than asking a smaller model to infer its job from the full session document. It also makes retries cheaper: regenerate or amend the job document, rerun the worker, compare outputs.

Long-lived virtual documents help when the output becomes an asset: architecture maps, stable runbooks, model-routing calibration, public release audits, or recurring failure analyses. Those should be committed, because the git history becomes part of the audit trail. A commit answers: what context was handed off, what decision was made, what evidence existed at the time, and how the orchestration policy evolved.

The design rule I would enforce is provenance. Every virtual document should say whether it is ephemeral or retained, what generated it, which source snapshot it summarizes, and whether it is authoritative. Most should be explicitly non-authoritative: "derived from session X, tsift index commit Y, source HEAD Z." That prevents stale packets from silently outranking the live repo or the main session document.

A practical shape:

```text
.agent-doc/
  jobs/
    cycle-123/
      job-search-auth.md
      job-fix-router.md
      job-test-summary.md
  artifacts/
    release-audits/
    routing-calibration/
    subsystem-maps/
```

Short-lived job docs should probably be ignored by default, with an option to preserve them on failure or when `--audit-jobs` is set. Long-lived artifacts should be normal tracked files with clear ownership and review.

This also pairs well with tsift. agent-doc can ask tsift for bounded context, write the result into a virtual document, route that document to a smaller model, then finalize only the distilled result back into the main session. That turns context shaping into a visible filesystem protocol rather than hidden prompt assembly.

So yes: virtual documents would help. I would start with short-lived per-job documents for orchestration packets, then add committed long-lived artifacts only for outputs that are useful across cycles. Do not let virtual documents become unbounded transcript dumps; their value is that they are smaller, typed, and easier to audit than the parent context.
❯ What advantage would a virtual-document have over a plan file and runbook for respective use casese/
### Re: virtual documents vs plans and runbooks — gpt-5

A virtual document is not better than a plan file or runbook in general. It has a different job.

A plan file is for intended work. It says what should happen, why, and in what order. It is useful when the human or parent agent needs to inspect or approve the approach before implementation.

A runbook is for repeatable procedure. It says how a class of work should be handled every time: release flow, closeout rules, routing rules, verification policy, recovery steps. It should be stable and reused across many sessions.

A virtual document is for one bounded execution context. It says: for this specific worker, in this specific cycle, here is the clipped packet of prompt, scope, evidence, handles, commands, expected output, and escalation rules. Its advantage is that it can be generated mechanically from the live session, tsift, repo state, and orchestration policy.

So the distinction is:

| Artifact | Lifetime | Audience | Contents | Authority |
| --- | --- | --- | --- | --- |
| Plan file | medium | human + parent agent | proposed work and milestones | advisory until executed |
| Runbook | long | all agents/harnesses | reusable procedure | policy/procedure |
| Virtual document | short or selected long | one worker/job | bounded task packet and evidence | derived context, usually non-authoritative |

The advantage of a virtual document over a plan is operational precision. A plan might say "investigate route closeout failures." A virtual document says "worker A: read these 3 source windows, inspect this failing log digest, do not edit outside `src/agent-doc/src/route.rs`, return JSON with findings and confidence." That is much more suitable for smaller models.

The advantage over a runbook is specificity. A runbook should not contain live line numbers, current failures, temporary branch state, model routing decisions, or tsift handles for a single job. A virtual document should contain exactly those things.

The clean model is to compose them:

1. Runbook defines the procedure.
2. Plan file defines the intended multi-step work.
3. Virtual documents instantiate individual executable jobs from the plan and runbook using current context.
4. The main session document receives only the durable outcome and audit summary.

That keeps each artifact from bloating into the others. Plans stay strategic, runbooks stay reusable, and virtual documents stay compact enough to hand to cheap workers.
❯ What advantage would a virtual-document have over a plan file and runbook for respective use cases?
### Re: virtual documents vs plans and runbooks — gpt-5

A virtual document is not better than a plan file or runbook in general. It has a different job.

A plan file is for intended work. It says what should happen, why, and in what order. It is useful when the human or parent agent needs to inspect or approve the approach before implementation.

A runbook is for repeatable procedure. It says how a class of work should be handled every time: release flow, closeout rules, routing rules, verification policy, recovery steps. It should be stable and reused across many sessions.

A virtual document is for one bounded execution context. It says: for this specific worker, in this specific cycle, here is the clipped packet of prompt, scope, evidence, handles, commands, expected output, and escalation rules. Its advantage is that it can be generated mechanically from the live session, tsift, repo state, and orchestration policy.

So the distinction is:

| Artifact | Lifetime | Audience | Contents | Authority |
| --- | --- | --- | --- | --- |
| Plan file | medium | human + parent agent | proposed work and milestones | advisory until executed |
| Runbook | long | all agents/harnesses | reusable procedure | policy/procedure |
| Virtual document | short or selected long | one worker/job | bounded task packet and evidence | derived context, usually non-authoritative |

The advantage of a virtual document over a plan is operational precision. A plan might say "investigate route closeout failures." A virtual document says "worker A: read these 3 source windows, inspect this failing log digest, do not edit outside `src/agent-doc/src/route.rs`, return JSON with findings and confidence." That is much more suitable for smaller models.

The advantage over a runbook is specificity. A runbook should not contain live line numbers, current failures, temporary branch state, model routing decisions, or tsift handles for a single job. A virtual document should contain exactly those things.

The clean model is to compose them:

1. Runbook defines the procedure.
2. Plan file defines the intended multi-step work.
3. Virtual documents instantiate individual executable jobs from the plan and runbook using current context.
4. The main session document receives only the durable outcome and audit summary.

That keeps each artifact from bloating into the others. Plans stay strategic, runbooks stay reusable, and virtual documents stay compact enough to hand to cheap workers.
<!-- agent:boundary:f672edba -->
<!-- /agent:exchange -->
###
<!--
If I 'm
-->

<!-- agent:queue auto -->
preset spec-test-build-install-commit-push
- do [#ds58]
- do [#djwb]
- do [#m2hx]
- do [#ha62]
- do [#aymr]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
