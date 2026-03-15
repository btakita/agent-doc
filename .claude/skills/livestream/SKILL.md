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
- **NEVER** inline cleartext secrets in Bash commands. Always use `pass` subshell: `"$(pass path/to/secret)"`
- **NEVER** read files that contain secrets (`tokens.json`, `.env`, credential files). If a workflow requires reading a token file, refuse and suggest an alternative (e.g., `pass` or env var).
- **NEVER** echo, print, cat, or log secret values — not even in error handling.
- **NEVER** show Bearer tokens, API keys, OAuth tokens, or refresh tokens in tool output.
- Redact any secret that appears in tool output before presenting to the user.
- When a command requires a secret, always wrap with `pass`: `CARGO_REGISTRY_TOKEN="$(pass btak/CARGO_REGISTRY_TOKEN)" cargo publish`

**Audience-Safe Output:**
- Avoid showing full file paths that reveal private directory structure when unnecessary.
- Be mindful that all tool call text is visible to stream viewers.
- Keep error messages professional and clear.

**Announce:** Print "Livestream mode ON — secret protection active" to confirm activation.

### `off` — Deactivate Livestream Mode

Removes the livestream guardrails. Print "Livestream mode OFF" to confirm.

### `audit` — Scan for Exposed Secrets

Scan Claude Code session transcripts or files for exposed secrets.

**Usage:**
```
/livestream audit                          — audit current session transcript
/livestream audit ~/.claude/projects/*/ID.jsonl   — audit specific transcript
/livestream audit /path/to/dir             — audit all files in directory
```

**Scans for:**

| Pattern | Example | Risk |
|---------|---------|------|
| OAuth access tokens | `ya29.a0...` | Medium (short-lived) |
| OAuth refresh tokens | `1//0...` | **Critical** (long-lived) |
| GitHub tokens | `ghp_...`, `gho_...` | Critical |
| PyPI tokens | `pypi-...` | Critical |
| Cargo registry tokens | `CARGO_REGISTRY_TOKEN=<value>` | Critical |
| AWS credentials | `AKIA...`, `aws_secret_access_key` | Critical |
| Cloudflare tokens | `CF_API_TOKEN=<value>` | Critical |
| Generic API keys | Long alphanumeric strings in auth headers | Medium |
| Bearer tokens | `Bearer <actual-token>` (not placeholders) | Critical |
| Private keys | `-----BEGIN.*PRIVATE KEY-----` | Critical |
| `.env` file contents | `KEY=value` patterns from .env reads | Varies |

**Safe patterns (skip):**
- `$(pass ...)` subshell references
- `Bearer redacted`, `Bearer test-token`
- Code comments and documentation references
- Placeholder values like `YOUR_TOKEN_HERE`

**Output:** Report each finding with:
- File and approximate location
- Secret type and risk level
- Whether the value is real or a safe reference
- Recommended remediation (rotate, revoke, etc.)

**NEVER output actual secret values in the audit report.** Show only the prefix/pattern.

## Secret Deny-List

Files the agent should NEVER read during a livestream session:

- `**/tokens.json` — OAuth token storage
- `**/.env` — Environment secrets
- `**/.env.*` — Environment variants
- `**/credentials.json` — Service account keys
- `**/*.pem` — Private keys
- `**/*.key` — Private keys
- `**/id_rsa*` — SSH keys
- `**/.netrc` — Machine credentials

If a workflow requires data from these files, instruct the user to set it up via `pass` or environment variables instead.
