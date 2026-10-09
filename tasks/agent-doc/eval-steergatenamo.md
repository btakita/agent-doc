# #steergatenamo — pretrained end-of-turn models as a steering-gate feature

Evaluated 2026-10-03 with `src/agent-doc/scripts/steergate-model-eval` (agent-doc 0.35.445+).

## Question

Should a pretrained end-of-turn text model join the steering completion gate as an
extra feature? The gate is the deterministic rules tier plus the
`#steergateperceptron` online logistic regression.

## Data reality

- `steering_gate_log` held **1 row, 0 labels**. Rows also stored only a `text_hash`,
  so no text model could ever have been scored on them. Fixed: each row now keeps a
  bounded `text_tail` (the last 280 chars it judged). `--dataset` scores the models
  against the logged rules+perceptron probability once labelled rows exist (default
  floor 200).
- Proxy corpus used instead: 171 of the operator's own submitted prompts, taken from
  local Claude Code transcripts. Each full prompt is labelled finished, and 2 random
  word-boundary prefixes of it are labelled still typing (507 examples). Nothing
  from the corpus is printed or stored.

## Results (proxy corpus, 5-fold grouped held-out)

| | AUC alone | held-out AUC with rules | premature at 90% delivery | ONNX | latency (mean / p95) |
|---|---|---|---|---|---|
| rules only | 0.799 | 0.782 | 58.3% | — | — |
| Namo v1 English (DistilBERT, quantized) | 0.753 | 0.819 | 42.0% | 136 MB | 2.5 / 5.4 ms |
| turnsense (SmolLM2-135M, quantized) | 0.871 | **0.891** | **33.6%** | 176 MB | 17.6 / 32.4 ms |

- **Where the rules give no verdict** (58% of examples), AUC is 0.60 for Namo and 0.75 for turnsense.
- **When the rules do say "finished"**, they are almost never wrong: 0.9% premature. They say it for only 39% of finished prompts.
- **Prefixes that end on a sentence boundary are the failure case.** These are the pause points a typist actually makes ("Fix X." … then keeps going). Both models score them as more finished than real finished prompts: AUC 0.25 for Namo and 0.38 for turnsense against complete prompts.

## Decision: do not integrate now

1. **Namo** adds little where it matters (AUC 0.60 on rules-inconclusive text). Drop it.
2. **turnsense** adds real proxy signal (+0.11 held-out AUC), but a model can't be
   shipped on the strength of a proxy:
   - Random prefixes are not the operator's real pauses, which cluster at sentence
     ends, exactly where both models invert.
   - The proxy has no timing. The perceptron's quiet-window and typing-speed features
     are what separate a pause from a finished prompt there.
   - Cost: 176 MB download, plus an `ort` native runtime, plus about 18 ms of CPU per
     decision on the hook path.
3. **Next proof:** run `scripts/steergate-model-eval --dataset` once 200 or more labelled
   rows carry `text_tail`. Integrate turnsense locally (ONNX via `ort`, lazily
   downloaded and opt-in) only if rules+perceptron+turnsense beats rules+perceptron on
   held-out log-loss. If it comes close but falls short, fine-tune it on the logged
   rows as the backlog item proposed.

## #steergatedecision — real-log rerun (2026-10-09)

The rerun gate was met. `agent-doc steering dataset` reported 225 rows
(premature=9, late=1, on_time=181, open=0), and `steergate-model-eval --dataset`
found 224 labelled rows that carry a `text_tail` (floor 200). Held-out results:

| | held-out log-loss | held-out AUC | AUC alone |
|---|---|---|---|
| rules only | — | — | 0.646 |
| rules+perceptron (in-sample) | — | 0.943 | — |
| rules+perceptron (held-out baseline) | **0.2092** | 0.9390 | — |
| plus turnsense | 0.2105 | 0.9387 | 0.571 |
| plus Namo v1 English | 0.2202 | 0.9337 | 0.658 |

These match the earlier 223-row check: baseline 0.2110, turnsense 0.2123, Namo 0.2222.
Neither model beats rules+perceptron on held-out log-loss. Namo makes it worse, and
turnsense adds nothing. On the real log, both models score far below what the
proxy corpus suggested (turnsense 0.571 alone, against 0.871 on the proxy).

### Decision

1. **Do not integrate turnsense** (and not Namo). The integration test above fails:
   neither model beats the baseline on held-out log-loss.
2. **Skip the fine-tune.** The data has only about 10 minority labels (9 premature,
   1 late). That is too few to fine-tune a 135M-parameter model, or even to measure
   whether a fine-tune helped. The baseline's ~0.94 AUC is mostly the perceptron
   separating a large `on_time` majority.
3. **Re-gate on minority-label count, not total rows.** Revisit only once the log
   holds **at least 50 premature/late labels** combined. Total row count is the
   wrong trigger, because rows grow almost entirely as `on_time`.
