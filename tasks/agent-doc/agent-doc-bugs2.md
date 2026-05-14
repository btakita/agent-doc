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
<!-- agent:boundary:fb3f9179 -->
<!-- /agent:exchange -->
###
<!--
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
