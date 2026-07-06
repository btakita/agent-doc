---
name: refactor-spiral-pattern-guardrails
description: Brian's recurring multi-week refactor-block pattern and the standing guardrails he asked for — spike-before-commit, conviction gates, recovery-rate tripwire, probe→load-bearing promotion, strangler-not-big-bang
metadata:
  type: user
---

Brian (2026-07-06) named a recurring career-long pattern: large library/tooling efforts hit massive refactors that block other work for days to months (agent-doc realtime migration is the current instance). Self-diagnosis: too-aggressive scope in unknown spaces, abandoning lazily-style architecture convictions at project start, supporting legacy code. Refactor outcomes are good; duration and disruption are the problem. He explicitly asked for help never re-entering this state.

**Why:** he builds tools/libraries to build alpha; multi-week blocks destroy the alpha the tools exist to create. Agent-assisted coding amplifies the trap: code is cheap to generate, architecture change is not, so codebases outgrow solo architecture-refresh capacity.

**How to apply (standing guardrails — raise these proactively):**
1. **Spike before commit:** entering unknown technical space → push for a disposable, day-budgeted probe whose deliverable is a design decision, before building on the conviction architecture (lazily-style substrate from day one).
2. **Convictions as gates:** new modules start from lazily-substrate scaffolds with code-enforced checks; a stated preference will lose to delivery pressure, a check won't.
3. **Tripwire:** the recovery/feedback memory creation rate is the debt alarm — when new recovery rituals cluster, recommend stopping feature work for a bounded correction NOW rather than a multi-month refactor later.
4. **Promotion event:** distinguish probe code from load-bearing code; promotion is explicit and includes the rewrite.
5. **Strangler, never big-bang:** rebuild behind a stable contract seam (e.g. agent-doc's document format + CLI surface), cut over per-subsystem, delete old paths (no legacy support in own tools).
6. **Behavioral done-bar for refactors:** define exit as observable behavior (e.g. one normal session, zero recovery interventions), not structural completeness; cap refactor time (~2 days/week) so revenue work isn't hostage.

Related: [[product-sequencing-patchboard-first-agentdoc-later]], [[realtime-merge-experience-marketable-alpha]], [[project_agent_doc_percell_crdt_merge_intent]]. Full analysis: agent-loop `tasks/session-findings-2026-07-04.md` §7.
