---
name: [REDACTED_PRODUCT] audits use [REDACTED_PRODUCT] OPENROUTER_API_KEY
description: For [REDACTED_PRODUCT].com scoring/validator runs, always use passage [REDACTED_PRODUCT]/OPENROUTER_API_KEY — not btak/OPENROUTER_API_KEY
type: feedback
originSessionId: a0f5b0a7-e59d-4e63-8f6d-11b40977e801
---
For any [REDACTED_PRODUCT].com audit, scoring, or bootstrap-validator run, export `OPENROUTER_API_KEY="$(passage [REDACTED_PRODUCT]/OPENROUTER_API_KEY)"` — NOT `btak/OPENROUTER_API_KEY`.

**Why:** [REDACTED_PRODUCT] is a [REDACTED_PRODUCT] product (see `src/session-share/CLAUDE.md` — "[REDACTED_PRODUCT] is being built by [REDACTED_PRODUCT]"). OpenRouter spend for [REDACTED_PRODUCT] runs must be billed to the [REDACTED_PRODUCT] account, not the personal btak account. User corrected this after a first bootstrap run that used `btak/OPENROUTER_API_KEY`.

**How to apply:** Whenever running any command under `src/session-share/[REDACTED_PRODUCT].com/` or `src/session-share/[REDACTED_PRODUCT]/` that needs OpenRouter access (scoring-api, bootstrap_validator.py, dry_run.py, any deepeval metric), use `[REDACTED_PRODUCT]/OPENROUTER_API_KEY`. The `[REDACTED_PRODUCT]` passage namespace also has `OPENROUTER_KEY` — use `OPENROUTER_API_KEY` (the `_API_` variant) unless a tool specifically asks for the non-API name.
