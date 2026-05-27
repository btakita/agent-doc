# Plan — direct run should not persist progress commentary as response body

## Problem

The direct `agent-doc <FILE>` path can capture a Codex response shaped as:

1. plain progress commentary emitted during the work, followed by
2. a valid `patch:exchange` closeout.

The Codex Stop-hook replay guard already treats this as recoverable by stripping the safe leading commentary and replaying only the patch body. The direct run/write path does not apply that same normalization, so the unmatched commentary can be appended into the session document exchange after the closeout.

Observed in `tasks/root.md` on 2026-05-09: the useful `### Re: dev asset preflight` response landed, but the progress updates were also written into the exchange and had to be manually condensed.

## Classification

This is an agent-doc bug, not primarily a harness modification.

Codex progress updates are legitimate console/user-facing messages while work is in progress. The session-document closeout should persist the final response or an intentional digest, not every progress update. Agent-doc already owns that response-shape boundary in `replay_guard`; the direct run/write path should share the same normalization.

Harness prompt guidance can still reduce the frequency by asking agents to keep progress updates concise, but correctness should not depend on that.

## Fix

- Reuse `replay_guard::classify_replay_payload` or an equivalent shared sanitizer before template patch application in the direct `agent-doc run` / write path.
- If the payload is `safe leading commentary + clean patch body`, apply only the patch body.
- If preserving progress context is desired, synthesize a short "Progress digest" from structured lifecycle events rather than copying raw commentary lines.
- Keep blocking transcript-shaped, interstitial, or trailing unmatched content.

## Regression

Add a direct-run/template write test where the captured response body contains safe progress prose followed by a valid `patch:exchange`. Assert that the committed exchange contains the response patch and does not contain the raw progress lines.
