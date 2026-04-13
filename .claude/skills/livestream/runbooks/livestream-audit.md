# Livestream Secret Audit

Scan Claude Code session transcripts or files for exposed secrets.

## Usage

```
/livestream audit                          -- audit current session transcript
/livestream audit ~/.claude/projects/*/ID.jsonl   -- audit specific transcript
/livestream audit /path/to/dir             -- audit all files in directory
```

## Patterns to Scan

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

## Safe Patterns (skip)

- `$(passage ...)` subshell references
- `Bearer redacted`, `Bearer test-token`
- Code comments and documentation references
- Placeholder values like `YOUR_TOKEN_HERE`

## Output

Report each finding with:
- File and approximate location
- Secret type and risk level
- Whether the value is real or a safe reference
- Recommended remediation (rotate, revoke, etc.)

**NEVER output actual secret values in the audit report.** Show only the prefix/pattern.

## File Deny-List

Files the agent should NEVER read during a livestream session:
- `**/tokens.json`, `**/.env`, `**/.env.*`
- `**/credentials.json`, `**/*.pem`, `**/*.key`
- `**/id_rsa*`, `**/.netrc`
