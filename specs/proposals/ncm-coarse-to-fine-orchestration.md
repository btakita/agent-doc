# Research: Next-Concept Modeling and agent-doc

> **Status:** research note, non-normative. Nothing here changes agent-doc's
> runtime contract. Normative behavior remains in [`SPEC.md`](../../SPEC.md)
> and the numbered `specs/` files.

## Question

Can agent-doc learn from *Next-Concept Modeling: Fast, Parallel Coarse-to-Fine
Text Generation* (Kumbong et al., 2026), and should it adopt any part of the
paper?

Primary source: [arXiv:2610.14162](https://arxiv.org/abs/2610.14162).

## Decision

Yes, agent-doc can use the paper as a design analogy and benchmark hypothesis,
but it should **not** add NCM-specific runtime code today.

NCM changes a language model's tokenizer, training objective and decoding loop.
agent-doc orchestrates existing harness models and does not control those
internals. Reimplementing NCM inside agent-doc would therefore create a second
model runtime, require trained tokenizer/generator checkpoints and GPU-serving
infrastructure, and bypass the harness contracts agent-doc is designed to
preserve.

The useful lesson is narrower: do expensive work in a short sequence of
dependency levels, make the earliest level cheap and globally informative,
and run independent work within one level in parallel. agent-doc already has
most of the deterministic substrate for testing that idea at the orchestration
layer. Any adoption should be an experiment over those existing primitives,
not a claim that subagent orchestration is equivalent to neural NCM.

## What the paper establishes

NCM learns a hierarchy of discrete representations with a multiscale residual
VQ-VAE. A transformer then predicts levels sequentially from coarse to fine,
while predicting all codes inside a level in parallel. The paper attributes
its efficiency to two properties:

1. generation takes `K` level steps rather than one step per output token;
2. early levels contain fewer positions, so early steps are cheaper than a
   full-sequence refinement pass.

The reported results are model-level evidence, not agent-orchestration
evidence. At roughly 100M parameters, the paper reports fewer function
evaluations and faster inference than its diffusion-language-model baselines,
plus competitive quality on generation, conditional generation and Sudoku.
Those results do not establish that decomposing software work into more agent
turns improves cost, latency or correctness.

## Mapping to agent-doc

| NCM mechanism | Closest agent-doc primitive | Applicability |
| --- | --- | --- |
| learned coarse representation | bounded context pack and planning record | useful analogy; do not introduce learned document codes |
| levels generated coarse to fine | `operation_tree` and dependency-ordered queue work | testable orchestration hypothesis |
| positions within a level generated in parallel | subagent work pool and parallel independent heads | already implemented |
| fine levels encode residual error | follow-up heads and proof-driven closeout | useful only when the follow-up names concrete residual work |
| nested dropout makes partial reconstructions useful | bounded-context tests at several budgets | useful evaluation idea, not a runtime dependency |
| conditioning corruption improves rollout robustness | restart, stale-state and missing-result simulations | useful fault-injection analogy |
| minimum Bayes risk candidate selection | multiple independent reviews plus deterministic evidence | potentially useful for high-risk review, too costly as a default |

The analogy has a hard boundary: NCM's parallel predictions share one trained
latent space and one model step. Subagents have separate contexts, can perform
side effects, and can disagree about work identity. Their outputs require
claims, lineage, integration and closeout. A parallel subagent layer is not a
drop-in substitute for an NCM level.

## Proposed experiment

Before adding a new directive or planner field, run an offline benchmark using
the current planning and queue surfaces.

### Coarse-to-fine arm

1. Produce one bounded planning record containing the objective, selected
   operation path, acceptance evidence and a dependency DAG.
2. Dispatch only dependency-ready leaves. Independent leaves at the same DAG
   depth may use the existing subagent pool in parallel.
3. After each depth, reconcile results into one evidence ledger and enqueue
   only concrete residual failures or missing proofs.
4. Stop when the acceptance predicate is satisfied; do not require a fixed
   number of refinement levels.

### Control arm

Use the existing task-specific workflow without forcing level-by-level
dispatch. Keep the same harness, model tier, token budget, repository revision
and acceptance tests.

### Measurements

- wall-clock time to a verified closeout;
- total model turns and estimated tokens;
- number of duplicate or superseded effects;
- retries caused by stale context or dependency violations;
- acceptance-test pass rate;
- human corrections after closeout.

The experiment must separate planning cost from execution cost and report
tasks where the coarse plan was wrong. Faster dispatch with more integration
debt is a regression.

## Adoption gate

Do not add a public `coarse-to-fine` attribute or a new queue lifecycle until
the experiment demonstrates all of the following on a mixed task corpus:

1. no reduction in acceptance-test pass rate;
2. lower median wall-clock time or model-token cost;
3. no increase in duplicate outward effects;
4. no increase in operator corrections;
5. a deterministic fallback when the plan has only one level or changes after
   new evidence.

If the gate passes, the smallest product change is a planner strategy that
emits an ordinary dependency DAG and uses the existing work pool. The queue,
claim, lineage and closeout protocols should remain unchanged. If the gate
does not pass, retain this note as a documented rejection and make no runtime
change.

## Provider-level opportunity

If a supported harness later exposes an NCM-family model, agent-doc may treat
it as another model/provider capability. Selection belongs in the harness
adapter and model-tier policy, with the same prompt, permission and closeout
contracts as other models. agent-doc should not assume that a provider's
internal coarse-to-fine decoding exposes levels that are safe to steer,
checkpoint or map onto queue rows.

## Conclusion

The paper supports a disciplined experiment in hierarchical planning and
parallel dependency levels. It does not justify embedding a neural tokenizer,
decoder or latent-code hierarchy in agent-doc. The current operation tree,
bounded context, work pool, lineage and simulation surfaces are sufficient to
test the transferable hypothesis without expanding the runtime contract.
