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

### Re: gpg-agent thrashing

`pass` calls gpg-agent on every invocation. If mise sources `.env` on each shell prompt (via shell hooks), that's continuous decryption → gpg-agent pins the CPU.

**Immediate fix — extend cache TTL:**

```
# ~/.gnupg/gpg-agent.conf
default-cache-ttl 86400
max-cache-ttl 86400
```

❯ already have a long cache
```
default-cache-ttl 604800                                                                   
max-cache-ttl 604800    
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

**22**Is there a faster alternative to gpg?
❯ already have a long cache
```
default-cache-ttl 604800                                                                   
max-cache-ttl 604800    
```

### Re: cache already long — likely a stuck agent (HEAD)

With a 7-day cache, that 23-minute 99% CPU run isn't decryption thrashing — gpg-agent is stuck in a loop. Kill and restart it:

```bash
gpgconf --kill gpg-agent
```

It restarts on next `pass` call. No data loss.

❯ Is there a faster alternative to gpg?

### Re: faster alternatives to GPG (HEAD)

**`age` + `passage`** — best drop-in replacement:

- [`age`](https://github.com/FiloSottile/age) is modern, minimal, no daemon, ~instant decryption
- [`passage`](https://github.com/FiloSottile/passage) is pass-compatible CLI backed by age
- Same store layout, same workflow, no gpg-agent process at all

```bash
# install
yay -S age passage  # or pacman/brew

# migrate existing store
cd ~/.password-store
for f in **/*.gpg; do
  pass show "${f%.gpg}" | age -r "$(age-keygen -y ~/.age/key.txt)" -o "${f%.gpg}.age"
  rm "$f"
done
```

**`secret-tool`** (GNOME Keyring / libsecret) — fastest for local secrets:

```bash
# store
secret-tool store --label "API Key" service myapp key api-key
# retrieve (instant, no crypto overhead per call)
export API_KEY="$(secret-tool lookup service myapp key api-key)"
```

No external process, queries the running keyring daemon directly. Works well in `.env` sourced by mise because the keyring stays unlocked for your session.

**`sops` + age** — for structured secrets files:

```toml
# .mise.toml
[tasks.gen-env]
run = "sops -d secrets.env.sops > .env.local"
```

**Recommendation:** If you want to keep the `pass` UX, switch to `passage` (age backend). If you want zero-overhead per-call reads, `secret-tool` with GNOME Keyring is the fastest option on Linux.
<!-- agent:boundary:bef25b04 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
