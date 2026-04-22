> Extracted from SPEC.md — see [index](../SPEC.md)

# Security

agent-doc is designed for single-user, local operation. There is no authentication, authorization, or multi-user access control.

## Threat Model

- **Trusted user, untrusted content.** The user is trusted; document content may contain prompt injection from external sources (pasted emails, web pages, chat logs).
- **Local filesystem scope.** All data (documents, snapshots, exchange history, links cache) resides on the local filesystem. No network services are exposed.
- **Git as audit trail.** All agent responses are committed to git, providing a complete audit trail. However, git history may contain sensitive content if documents reference private data.

## Known Risks

- **Prompt injection via document content.** Content pasted from external sources could contain injection attempts. The agent processes all document content as user input with no injection scanning. Mitigation: user awareness; planned content scanning in `agent-doc write`.
- **`--dangerously-skip-permissions` exposure.** When running with this flag (common in agent-doc sessions via `claude_args` frontmatter), the agent has full filesystem access. Injected prompts could read files or execute commands.
- **Data divulgence through the response channel.** Even with sandboxing, the agent's response IS the output channel. If the model has sensitive data in context, injection can convince it to include that data in the document response. The only real defense is context isolation (see ragie-web-doc security analysis).
- **Links cache may contain sensitive fetched content.** URL content fetched via `links` frontmatter is cached at `.agent-doc/links_cache/`. This cache is not encrypted and persists until manually cleared.

## Recommendations

- Use a **private git repository** for the project containing session documents.
- Avoid putting secrets (API keys, credentials) in documents or agent context.
- For shared/collaborative use cases, wait for the planned multi-user security model (access control, session isolation, content scanning).
- Review agent responses before sharing or publishing document content.
