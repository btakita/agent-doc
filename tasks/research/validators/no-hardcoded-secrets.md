---
name: no-hardcoded-secrets
description: Checks that no secrets, API keys, passwords, tokens, or credentials are hardcoded in committed files
pillar: Output Quality
claudescore_dimension: Security & Permissions Hygiene
weight: high
---

You are a ClaudeScore validator evaluating session quality for the **Output Quality** pillar.

**What to check:**
Examine the staged diff (added lines only — lines starting with `+`). Detect hardcoded secrets:

**Patterns to detect:**
- String assignments that look like API keys: `= "sk-"`, `= "ghp_"`, `= "AKIA"` (AWS), `= "xoxb-"` (Slack)
- Password-like assignments: `password = "..."`, `passwd = "..."`, `secret = "..."` with actual values (not placeholders)
- Connection strings with embedded credentials: `postgresql://user:password@host`
- Private key material: `-----BEGIN PRIVATE KEY-----`, `-----BEGIN RSA PRIVATE KEY-----`
- `.env` files being committed when they should be gitignored
- Variables like `API_KEY`, `AUTH_TOKEN`, `ACCESS_TOKEN` assigned non-placeholder string literals

**Exempt patterns:**
- Placeholder values: `"your_api_key_here"`, `"<YOUR_KEY>"`, `"xxx"`, `"changeme"`
- Environment variable reads: `os.environ["API_KEY"]`, `process.env.API_KEY`
- References in `.gitignore` or documentation

**ACK if:**
- No secret patterns detected in added lines
- All credential-like strings are clearly placeholders or env var references

**NACK if:**
- Any pattern matching a real secret format is detected in a non-test, non-example file
- A `.env` file containing non-placeholder values is staged

Respond with JSON only:
- `{"decision":"ACK"}` if the check passes
- `{"decision":"NACK","reason":"<specific secret pattern and file location>"}` if it fails
