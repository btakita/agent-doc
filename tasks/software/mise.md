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
<!-- agent:boundary:61003d1a -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
