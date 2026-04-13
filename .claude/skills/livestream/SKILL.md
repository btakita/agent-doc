---
description: Security and operational guardrails for livestreamed coding sessions
user-invocable: true
argument-hint: "<command> [args]"
---

# livestream

Operational guardrails for livestreamed coding sessions — secret protection, audience-safe output, and session auditing.

## Invocation

```
/livestream on              — activate livestream mode for this session
/livestream off             — deactivate livestream mode
/livestream audit [paths]   — scan files/transcripts for exposed secrets
```

## Commands

### `on` — Activate Livestream Mode

Sets session-wide guardrails. When livestream mode is active:

**Secret Protection (CRITICAL):**
- **NEVER** inline cleartext secrets in Bash commands. Always use `pass` subshell: `"$(passage path/to/secret)"`
- **NEVER** read files that contain secrets (`tokens.json`, `.env`, credential files). If a workflow requires reading a token file, refuse and suggest an alternative (e.g., `pass` or env var).
- **NEVER** echo, print, cat, or log secret values — not even in error handling.
- **NEVER** show Bearer tokens, API keys, OAuth tokens, or refresh tokens in tool output.
- Redact any secret that appears in tool output before presenting to the user.
- When a command requires a secret, always wrap with `pass`: `CARGO_REGISTRY_TOKEN="$(passage btak/CARGO_REGISTRY_TOKEN)" cargo publish`

**Audience-Safe Output:**
- Avoid showing full file paths that reveal private directory structure when unnecessary.
- Be mindful that all tool call text is visible to stream viewers.
- Keep error messages professional and clear.

**Announce:** Print "Livestream mode ON — secret protection active" to confirm activation.

### `off` — Deactivate Livestream Mode

Removes the livestream guardrails. Print "Livestream mode OFF" to confirm.

### `audit` — Scan for Exposed Secrets

See `runbooks/livestream-audit.md` for full scan patterns, deny-list, and output format.

## Runbooks

- `audit secrets` — [runbooks/livestream-audit.md](runbooks/livestream-audit.md)
