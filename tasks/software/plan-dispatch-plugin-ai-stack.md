# Plan: plugin AI stack for the dispatch host (#gbdispatch6)

Which AI actually runs inside an actor plugin, and where it runs. Companion to
[`plan-brookebrodack-guestbook-dispatch-service.md`](plan-brookebrodack-guestbook-dispatch-service.md).

Sourced from a research pass on 2026-07-18. Claims below carry links; where the
research could not verify something it is marked, and those gaps are real.

## 1. The load-bearing decision: inference lives on the host, not in the sandbox

Run **one resident model server on the host**, bound to localhost. Each plugin
stays a thin client whose only permitted egress is that endpoint — which is
exactly the `egress_hosts` allowlist the sandbox already implements.

This is not a performance tweak, it is what keeps the sandbox cheap. If each
one-shot plugin loaded a model, per-request cost would be dominated by model
load, and the ephemeral-sandbox property (an injection cannot survive into the
next request) would become unaffordable. Hoisting inference to the host keeps
plugins disposable.

It also mostly dissolves the "which agent harness" question — see §3.

## 2. No agent framework inside a plugin. Deliberately none.

A one-shot stdin→stdout classifier is **one HTTP POST and one JSON parse** —
on the order of 100 lines of Rust with no framework.

Every framework added inside the sandbox costs startup latency, dependency
surface, and — decisively — **capability**. A tool-using agent reading
attacker-controlled text is precisely the prompt-injection scenario the sandbox
exists to prevent. *The safest plugin has no tools to hijack.* Constrained
decoding plus zero tools plus the sandbox is defence in depth that no agent
framework improves on.

Consequence for `#2amm`: fixing `hermes` is **not** on this critical path. A
multi-provider agent CLI is the wrong shape for a no-network, no-tools, one-shot
judge. Fix hermes for its own sake if wanted; do not block moderation on it.

## 3. Serving

- **[`llama-server`](https://github.com/ggml-org/llama.cpp) first.** Single
  static binary, low idle footprint, and native **JSON-schema → GBNF constrained
  decoding** via `response_format`
  ([grammars](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md)).
  That directly serves the fail-closed rule: unparseable output means "no
  opinion", so constraining the decode removes a whole class of abstention.
  Note the schema constrains decoding but is **not** shown to the model — put it
  in the prompt as well.
- Slot/prefix caching keeps the constant policy prompt hot, so per-call latency
  is dominated by the short input plus a handful of output tokens.
- **[vLLM](https://github.com/vllm-project/vllm)** is the upgrade path once many
  plugins fire concurrently: continuous batching, automatic prefix caching,
  xgrammar structured output. RTX 5090 / sm_120 is supported as of v0.17+
  including FP8 — the 2025-era Blackwell trouble is over.
- **[mistral.rs](https://github.com/ericlbuehler/mistral.rs)** is worth a look
  because the host is Rust: usable as an embedded crate, so the host could own
  model lifecycle in-process with no separate daemon. Smaller quant/ecosystem
  coverage is the trade.

## 4. Models — an ensemble, which is what the voting design already wanted

An [ICLR 2026 workshop benchmark](https://arxiv.org/html/2605.28830v1) of open
guard models found **no single model dominates**; precision and recall trade off
sharply (ShieldGemma is high-precision but misses ~55% of unsafe content). That
is empirical support for unanimous-to-approve across diverse judges rather than
an argument against it — the architecture was right for a reason beyond
prompt-injection defence.

| Model | Size | Role |
|---|---|---|
| [Qwen3Guard](https://arxiv.org/pdf/2510.14276) (Apache-2.0) | 0.6B / 4B / 8B | Cheap fast pre-filter; 119 languages |
| [gpt-oss-safeguard-20b](https://openai.com/index/introducing-gpt-oss-safeguard/) (Apache-2.0) | ~16GB MXFP4 | Heavyweight voter — **bring-your-own-policy at inference time** |
| [Llama Guard 4](https://huggingface.co/meta-llama/Llama-Guard-4-12B) | 12B multimodal | When jobs include images |

**`gpt-oss-safeguard` is the standout for the multi-site goal**: policy is
supplied as text at inference time, so each site gets its own moderation policy
without a fine-tune. That is precisely the "many plugins across various
websites" requirement, solved by the model rather than by us.

Caveat: guard models emit their own fixed format (`safe`/`unsafe` plus category
lines), not arbitrary JSON. Either parse that natively in the plugin or
grammar-constrain to their format; reserve JSON-schema decoding for a general
instruct model filling the vote schema.

## 5. Where the coding harnesses (tmux-router) actually fit

**Not on the per-request path.** Wrong on every axis: multi-second session
startup against a sub-second budget, per-call cost at moderation volume,
nondeterministic multi-turn behaviour where a deterministic verdict is needed,
and — worst — those harnesses carry shell and filesystem access, so piping
attacker-controlled text into them **inverts** the sandbox threat model.

Two places they do fit:

1. **The escalation tier.** Cases the ensemble escalates are rare and already
   off the visitor's critical path. They can afford seconds and genuinely
   benefit from a real reasoning harness.
2. **The meta-loop** — see §6.

## 6. The harness that learns is the plugin registry, not the request path

Two June-2026 systems are relevant, and they are easy to conflate:

- **[Ornith-1.0](https://github.com/deepreinforce-ai/Ornith-1)** — open-weights
  model RL-trained to write its own scaffold as step one of every task, then
  discard it.
- **[Self-Harness](https://arxiv.org/abs/2606.09498)** — an agent that improves
  its own *persistent* harness over time: weakness-mining from execution traces
  → harness proposal → regression-gated validation. Terminal-Bench 2.0
  40.5% → 61.9%.

**Self-Harness is the pattern to steal**, and it maps onto this system directly:

- Log every plugin vote alongside the eventual human decision at
  `/guestbook/admin`. Those human reviews are the gold labels — the same asset
  the distilled-classifier icebox item depends on.
- Periodically mine disagreements and misses offline. This is where a coding
  harness via tmux-router is the right tool.
- Propose prompt/policy updates, and **regression-gate them against held-out
  past cases** before they go live.

The loop improves the registry between requests. It never touches the
per-request path, so it cannot cost latency or add capability inside a sandbox.

## 7. Sequence

1. Host-side `llama-server` with Qwen3Guard-0.6B; one thin Rust plugin client.
2. Add `gpt-oss-safeguard-20b` as the policy-carrying voter; per-site policy text.
3. Vote/outcome logging — the prerequisite for both §6 and the distilled
   classifier. Worth doing early even though its payoff is much later.
4. Escalation tier via tmux-router.
5. Offline weakness-mining loop with regression gate.

## 8. Unverified

- **Ornith-1.0 merit is unproven** — three weeks old, self-reported benchmarks.
  Existence and mechanism were verified; quality was not.
- Guard-model latency on a 5090 was **not measured**. The "0.6B pre-filter +
  20B reasoner" tiering is inferred from model sizes, not benchmarked. Measure
  before committing to the inline deadline.
- Whether Meta ships a Llama Guard past 4 — as of mid-July 2026, Llama Guard 4
  (Apr 2025) is still current and is the oldest option here.
- [HaloGuard 1.0](https://arxiv.org/html/2607.02079v1) (July 2026) surfaced but
  weights availability and quality were not verified. Flagged, not recommended.
