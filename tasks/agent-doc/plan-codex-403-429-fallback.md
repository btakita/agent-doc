# Plan: Codex 403→429 Transport Fallback Detection

**Bug:** `#codex-403-429-fallback`
**File:** `src/agent-doc/agent-doc-orchestration/src/agent/codex.rs`
**Spec:** `src/agent-doc/specs/05-agent-backend.md`

## Problem

When a Codex session receives 403 Forbidden on the WebSocket handshake to
`wss://chatgpt.com/backend-api/codex/responses`, the codex CLI falls back to
HTTPS, which may then hit 429 Too Many Requests. Other concurrent Codex
sessions can continue working fine — this is a per-session/per-connection
issue (rate limit, Cloudflare edge block, or token/auth state).

agent-doc currently treats this as a generic non-zero exit with opaque stderr,
indistinguishable from an agent-doc routing failure. The 429 exhausts the
retry budget permanently without operator visibility.

## Approach

### 1. Detection (`agent/codex.rs`)

Add `looks_like_codex_transport_403_429(text: &str) -> bool` that checks for:
- `403` + (`forbidden` or `websocket` or `handshake` or `wss://`)
- `429` + (`too many requests` or `rate limit` or `request id`)
- Both patterns appearing together (the 403→429 chain)

Add `format_transport_403_429_diagnostic(stderr: &str) -> String` that produces
a clear diagnostic distinguishing transport rejection from agent-doc routing.

### 2. Hook into error paths

**Non-streaming (`send_once`):** In the `!output.status.success()` branch
(line 1655), before the generic `bail!("codex command failed: ...")`, detect
the 403→429 pattern and emit the transport-rejection diagnostic with retry
guidance.

**Streaming (`StreamIterator::next`):** In the non-zero exit handling
(lines 2138–2176), add the same detection before the generic error. For the
streaming path, also suggest restarting the Codex session via `agent-doc
session clear` or re-dispatching.

### 3. Retry guidance

The diagnostic message will include:
- "Codex transport rejection (403 Forbidden on WebSocket → 429 Too Many
  Requests on HTTPS fallback)" — clearly labeled as a transport issue
- "This is NOT an agent-doc routing failure"
- "Possible causes: per-session rate limit, Cloudflare edge block, or
  session-specific token/auth state"
- "Suggested actions: wait and retry, or restart the Codex session via
  `agent-doc session clear <FILE>` and re-dispatch"

### 4. Spec update

Add a paragraph to `specs/05-agent-backend.md` documenting the transport
rejection detection and its diagnostic shape.

## Files Changed

- `src/agent-doc/agent-doc-orchestration/src/agent/codex.rs` — detection
  functions + error path hooks + tests
- `src/agent-doc/specs/05-agent-backend.md` — transport rejection spec

## Tests

- `detects_403_then_429_in_stderr` — both patterns present
- `detects_403_only_in_stderr` — 403 without 429
- `detects_429_only_in_stderr` — 429 without 403
- `no_false_positive_on_normal_error` — unrelated error text
- `format_diagnostic_includes_transport_label` — diagnostic quality
