---
agent_doc_session: bfac1651-ad84-4f35-87cc-3daf9ff3029d
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ How do I configure .mise.toml to read .env with shell expansion. See .env with $(pass ...) 
❯ How do I configure .mise.toml to read .env with shell expansion. See .env with $(pass ...) 

### Re: `.env` with shell expansion (HEAD)

Use `_.source` — it executes the file as a shell script, so `$(pass ...)` expands normally. `_.file` parses dotenv format and treats `$(...)` as a literal string.

**`.mise.toml`:**
```toml
[env]
_.source = ".env"
```

**`.env`:**
```bash
export DATABASE_URL="$(pass myapp/database-url)"
export API_KEY="$(pass myapp/api-key)"
```

The `export` prefix is required when using `_.source` (shell sourcing, not dotenv parsing).

❯ gpg agent is thrashing
```
   2251 brian     30  10  582268   6600   5880 S  98.9   0.0  23:43.18 gpg-agent
```
How do I configure .mise.toml to read .env with shell expansion. See .env with $(pass ...) 

### Re: `.env` with shell expansion

Use `_.source` — it executes the file as a shell script, so `$(pass ...)` expands normally. `_.file` parses dotenv format and treats `$(...)` as a literal string.

**`.mise.toml`:**
```toml
[env]
_.source = ".env"
```

**`.env`:**
```bash
export DATABASE_URL="$(pass myapp/database-url)"
export API_KEY="$(pass myapp/api-key)"
```

The `export` prefix is required when using `_.source` (shell sourcing, not dotenv parsing).

❯ gpg agent is thrashing
```
   2251 brian     30  10  582268   6600   5880 S  98.9   0.0  23:43.18 gpg-agent
```

### Re: gpg-agent thrashing (HEAD)

`pass` calls gpg-agent on every invocation. If mise sources `.env` on each shell prompt (via shell hooks), that's continuous decryption → gpg-agent pins the CPU.

**Immediate fix — extend cache TTL:**

```
# ~/.gnupg/gpg-agent.conf
default-cache-ttl 86400
max-cache-ttl 86400
```

Then restart the agent:
```bash
gpgconf --kill gpg-agent
```

This caches the PIN/passphrase for 24h so gpg-agent doesn't re-decrypt on every call.

**Structural fix — pre-generate a cached `.env.local`:**

Keep `$(pass ...)` out of the sourced env entirely. Use a mise task to generate a plaintext `.env.local` on demand:

```toml
# .mise.toml
[env]
_.file = ".env.local"   # plaintext, no pass calls at runtime

[tasks.gen-env]
description = "Populate .env.local from pass"
run = """
printf 'DATABASE_URL=%s\n' "$(pass myapp/database-url)" > .env.local
printf 'API_KEY=%s\n' "$(pass myapp/api-key)" >> .env.local
"""
```

Add `.env.local` to `.gitignore`. Run `mise run gen-env` once per session (or after credential rotation). No more continuous gpg-agent calls.
<!-- agent:boundary:35433c24 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
