> Extracted from SPEC.md — see [index](../SPEC.md)

# Security

agent-doc is designed for single-user, local operation. There is no authentication, authorization, or multi-user access control.

## Threat Model

- **Trusted user, untrusted content.** The user is trusted; document content may contain prompt injection from external sources (pasted emails, web pages, chat logs).
- **Local filesystem scope.** All data (documents, snapshots, exchange history, links cache) resides on the local filesystem. No network services are exposed.
- **Git as audit trail.** All agent responses are committed to git, providing a complete audit trail. However, git history may contain sensitive content if documents reference private data.

## Known Risks

- **Prompt injection via document content.** Content pasted from external sources could contain injection attempts. The agent processes all document content as user input with no injection scanning. Mitigation: user awareness; planned content scanning in `agent-doc write`.
- **`--dangerously-skip-permissions` exposure.** When running with this flag (common in agent-doc sessions via `claude_args` or `opencode_args` frontmatter), the agent has full filesystem access. Injected prompts could read files or execute commands.
- **Data divulgence through the response channel.** Even with sandboxing, the agent's response IS the output channel. If the model has sensitive data in context, injection can convince it to include that data in the document response. The only real defense is context isolation (see ragie-web-doc security analysis).
- **Links cache may contain sensitive fetched content.** URL content fetched via `links` frontmatter is cached at `.agent-doc/links_cache/`. This cache is not encrypted and persists until manually cleared.
- **Cross-document plan / backlog reads can leak user context.** Moving exchange content, transferring backlog or icebox items, or following a plan-backed backlog item into another `.md` file can expose one user's work queue to another if the document is being used collaboratively.

## Recommendations

- Use a **private git repository** for the project containing session documents.
- Avoid putting secrets (API keys, credentials) in documents or agent context.
- If you are experimenting with shared/collaborative docs, mark the document with `agent_doc_collaboration: shared` and require an auditable `agent_doc_security_review: <review-id>` before cross-document `extract` / `transfer` or plan-backed `do #id` work that reads another `.md` file.
- For broader shared/collaborative use cases, wait for the planned multi-user security model (access control, session isolation, content scanning).
- Review agent responses before sharing or publishing document content.

## Secret Redaction

The `secret_redact` module (`src/secret_redact.rs`) is a backend hygiene layer that scrubs common plaintext secret shapes from anything written to `.agent-doc/` state files or stream/finalize stdout messages. It is **always on** with no operator-facing flag.

### Patterns (most-specific first)

- `sk-proj-[A-Za-z0-9_-]{20,}` → `[REDACTED_OPENAI]` (OpenAI project keys)
- `sk-svcacct-[A-Za-z0-9_-]{20,}` → `[REDACTED_OPENAI]` (OpenAI service-account keys)
- `sk-[A-Za-z0-9]{40,}` → `[REDACTED_OPENAI]` (legacy OpenAI keys)
- `AKIA[0-9A-Z]{16}` → `[REDACTED_AWS]` (AWS access keys)
- `xoxb-[0-9]+-[0-9]+-[A-Za-z0-9]+` → `[REDACTED_SLACK]` (Slack bot tokens)
- `ghp_[A-Za-z0-9]{36}` / `gho_[A-Za-z0-9]{36}` → `[REDACTED_GITHUB]`
- Named env-var family: `OPENAI_API_KEY|ANTHROPIC_API_KEY|AWS_SECRET_ACCESS_KEY|CLOUDFLARE_API_TOKEN|XAI_API_KEY|OPENROUTER_API_KEY|STRIPE_API_KEY` followed by `:` or `=` — the value half is replaced with `[REDACTED]`; the variable name and separator are preserved so debugging still works.

Order matters: longer patterns run first so a project / service-account token is never partially eaten by the generic legacy rule.

### `passage` exemption

Any match whose surrounding ±16-byte window contains the substring `passage ` is left UNCHANGED. `passage open …`, `passage show …`, and `$(passage …)` are the canonical safe patterns and round-trip verbatim. This protects the project's recommended pattern (read secrets from `passage` rather than env dumps) from accidental redaction.

### Call sites

- `src/capture.rs::capture_response` and `checkpoint_partial_response_for_cycle` — redact `response_body` before it is serialized into `.agent-doc/captures/<doc-hash>/<cycle-id>.json` and the corresponding `.partial.json`. The `response_sha256` retains the original in-memory hash so cycle-state correlation stays consistent.
- `src/snapshot.rs::save_unlocked` and `save_pre_response` — redact the snapshot / pre-response body before atomic write to `.agent-doc/snapshots/<hash>.md` and `.agent-doc/pre-response/<hash>.md`.
- `src/stream.rs` — redact flush-error and thinking-flush-error messages before they hit stderr so a failed write that interpolates a streamed chunk cannot leak a token to the terminal.

The streaming-safe variant `redact_streamed(carry, chunk)` holds the last ~128 chars across calls so a token split across a chunk boundary still matches once the second chunk arrives.

### Threat model boundary

This is **best-effort hygiene**, not a security boundary against a determined adversary. It addresses accidental leakage of common token shapes (e.g., a user running `npx convex env list` while a session document is open and the dump landing in capture sidecars or terminal scrollback). Defense-in-depth still relies on (a) keeping secrets in `passage` and (b) never running tools that dump env to stdout.
