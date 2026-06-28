# #ckythreadmerge — Cross-Provider Thread Identity Reconciliation

Split from `#ckythread`. [P2 correctness] corky threading.

## Problem

The same email conversation synced from two providers produces **duplicate thread
files** because thread identity is provider-specific:

- **IMAP** (`imap_sync.rs:585`) keys threads by `thread_key_from_subject(subject)`
  (e.g. `"hello world"`).
- **Gmail API** (`gmail_api_sync.rs:447`) keys threads by Gmail's opaque
  `threadId` (e.g. `"18c4f2a8b3d"`).
- `find_thread_file` matches only on the `**Thread ID**` metadata line, so a key
  from one provider never resolves a file written under the other provider's key.

Result: the same messages (identical RFC 5322 `Message-ID`) land in two separate
`.md` files — one subject-keyed, one threadId-keyed.

## Design — aliases + Message-ID cross-linking + consolidation

All reconciliation lives in the single shared `merge_message_to_file`
(`imap_sync.rs`), which IMAP, Gmail-API, SMS, Slack, Telegram, and refetch
already call. No provider-specific changes are needed at call sites.

### 1. Thread aliases

`Thread` gains `aliases: Vec<String>` — provider keys (beyond the primary `id`)
that map to this thread. Serialized as `**Thread Aliases**: k1, k2` (emitted only
when non-empty). Old files have no such line → `aliases` is empty → the thread is
identified by `id` alone (backward compatible). A file **claims** a key `K` when
`id == K` or `aliases.contains(K)`.

### 2. Reconciliation in `merge_message_to_file`

When merging a message with key `K` and (optional) `Message-ID` `M`:

1. Gather candidate files: all files claiming `K`, plus (if `M` is present) all
   files containing a `**Message-ID**: M` line.
2. If no candidates → create a new thread as today.
3. If candidates exist → pick the first as **canonical**, consolidate every other
   candidate into it (`merge_union`: union messages with existing `dedup_key`,
   union labels/accounts/tracking/aliases, claim the other file's `id` as an
   alias, re-sort by date), then delete the consolidated files.
4. Merge the incoming message into the canonical thread (existing dedup).
5. Ensure `K` is claimed by the canonical thread (`id` or `aliases`).

This handles both "same message via two providers" (Message-ID linkage) and "same
conversation, different messages, different providers" (once one message links the
files, the alias lets the rest of the thread find it).

### 3. Identity lookup everywhere

`find_thread_file` (imap_sync) and `find_thread_file_by_id` (mod.rs, used by
refetch) both become alias-aware via a shared `markdown::parse_thread_identity`
text helper, so a refetch by Gmail `threadId` still finds a thread merged under a
subject key.

### Correctness / no-regression notes

- Subject-key collision risk for IMAP is unchanged (it pre-exists); Gmail threads
  keep accurate intra-provider identity via their `threadId` alias.
- Canonical `id`/filename is "first-seen" — merging never renames files, only
  adds aliases, so existing references stay valid.
- Consolidation unions before deleting; `dedup_key` (Message-ID-first) prevents
  message loss across providers.

## Scope

Files: `crates/corky-mail/src/sync/{types.rs,markdown.rs,imap_sync.rs,mod.rs}`,
`SPEC.md`.
