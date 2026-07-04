---
name: no-cleartext-secrets
description: Never inline cleartext secrets in commands — user livestreams and tool output is visible
type: feedback
---

Never inline cleartext secrets (tokens, passwords, API keys) in Bash commands. Always use `pass` via subshell substitution.

**Why:** User livestreams their coding sessions. Tool call output (including command text) is visible to viewers. A `CARGO_REGISTRY_TOKEN=<actual_token>` was shown on stream.

**How to apply:** For any command needing a secret, use `"$(pass <path>)"` instead of the actual value. Examples:
- `CARGO_REGISTRY_TOKEN="$(pass crates.io/token)" cargo publish`
- `MATURIN_PYPI_TOKEN="$(pass pypi/publish_token)" maturin publish`
- Never `echo`, `cat`, or log secret values
