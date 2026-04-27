---
name: corky-gmail
description: Use the local corky CLI as a Codex-facing Gmail adapter for auth checks, thread refresh, draft creation, draft push, and send flows.
---

# corky-gmail

Use when the user wants Gmail work to go through the local `corky` CLI instead of a hosted Gmail connector.

## Preconditions

- `corky` is installed, or `src/corky/.bin/corky` exists in this repo, or `CORKY_BIN` is set
- live Gmail operations require outbound DNS + HTTPS access to Google APIs
- local reply flows assume synced thread files under `mail/conversations/`

## Preferred entry points

- `scripts/auth-status.sh [--account ACCOUNT]`
- `scripts/refetch-thread.sh THREAD_ID`
- `scripts/draft-new.sh ...`
- `scripts/push-draft.sh FILE`
- `scripts/send-draft.sh FILE`

## Workflows

### Auth state

1. Run `scripts/auth-status.sh --account <name>` when a connector-like flow needs machine-readable Gmail auth or scope state.
2. Treat scope mismatch or missing token as a real blocker; do not guess from stderr text when the JSON surface exists.

### Thread refresh before reply

1. When a thread looks stale, partially synced, or missing decoded body text, run `scripts/refetch-thread.sh THREAD_ID`.
2. Read the refreshed conversation file and extract both:
   - the RFC 2822 `Message-ID`
   - the Gmail `Thread ID`
3. Keep the original subject for threading.

### Draft creation

For a threaded reply, use:

```bash
scripts/draft-new.sh \
  --to "<recipient@example.com>" \
  --in-reply-to "<message-id>" \
  --thread-id "<thread-id>" \
  "Re: <original subject>"
```

For a new standalone draft, omit the threading flags and pass the same arguments through to `scripts/draft-new.sh`.

### Push vs send

- `scripts/push-draft.sh FILE` creates or updates a Gmail draft and returns JSON.
- `scripts/send-draft.sh FILE` sends immediately and returns JSON.
- Only use the send path after explicit user approval.

## Non-negotiable rules

- Never pass a Gmail thread ID as `--in-reply-to`.
- Prefer the JSON-capable auth, refetch, push, and send surfaces for adapter-like flows.
- Keep Gmail auth, token handling, and send behavior inside `corky`; this plugin is only the adapter layer.
