# corky-gmail

Thin Codex adapter for Gmail work through the local `corky` CLI.

## What it wraps

- `scripts/auth-status.sh` -> `corky doctor gmail --json`
- `scripts/refetch-thread.sh THREAD_ID` -> `corky sync refetch THREAD_ID --json`
- `scripts/draft-new.sh ...` -> guarded `corky draft new ...`
- `scripts/push-draft.sh FILE` -> `corky draft push FILE --json`
- `scripts/send-draft.sh FILE` -> `corky draft send FILE --json`

## Why it exists

`corky` stays the mail engine and owns Gmail OAuth, token storage, threading semantics, and send behavior. This plugin keeps the Codex-facing adapter outside `corky` while still using the machine-readable CLI surfaces shipped in `src/corky`.

## Runtime requirements

- `corky` must be available on `PATH`, or at `src/corky/.bin/corky`, or via `CORKY_BIN=/absolute/path/to/corky`
- outbound DNS + HTTPS access to Google APIs for live auth, refetch, draft, and send operations
- a configured mail workspace with synced conversation files and draft files where applicable

## Threading safety

For Gmail replies:

- `--in-reply-to` must be the RFC 2822 `Message-ID`
- `--thread-id` must be the Gmail thread ID

The `draft-new.sh` wrapper rejects the common mistake of passing the Gmail thread ID as `--in-reply-to`.
