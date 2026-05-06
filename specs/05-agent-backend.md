> Extracted from SPEC.md — see [index](../SPEC.md)

# Agent Backend

## Trait

`fn send(prompt, session_id, fork, model) -> (text, session_id)`

## Resolution Order

1. CLI `--agent` flag
2. Frontmatter `agent` field
3. Config `default_agent`
4. Fallback: `"claude"`

## Claude Backend

Default: `claude -p --output-format json --permission-mode acceptEdits`. Session handling: `--resume {id}` or `--continue --fork-session`. Appends `--append-system-prompt` with document-mode instructions. Removes `CLAUDECODE` env var. Parses JSON: `result`, `session_id`, `is_error`.

## Prompt Contract

For resumed document turns, the prompt must include:

1. The unified diff since the last run
2. The full current document
3. An ordered `user-authored prompt-bearing changes` section extracted from the diff

That section is oldest-first and uses explicit subtypes:
- `prompt_target` — user-authored prompts that require a response
- `content_edit` — user corrections the agent must incorporate as the new source of truth
- `recovery_artifact` — likely missed/uncommitted response material that should route through repair/session-check logic
- `boundary_artifact` — transient boundary / `(HEAD)` churn that should be normalized, not answered

Prompt-bearing change extraction is document-body only. YAML frontmatter metadata drift such as `agent: codex`, session ids, model tier, or other config edits must not be surfaced as `content_edit` or `prompt_target`.

A turn is not complete until each `prompt_target` item is answered or explicitly grouped into one response. The prompt must also tell the agent to incorporate `content_edit` items and normalize artifact items instead of treating them as ordinary conversation.

That section must preserve the diff encounter order across mixed kinds. If a `content_edit` or artifact appears before a later prompt in the changed tail, the prompt payload must keep that ordering instead of moving all `prompt_target` items to the front.

## Custom Backends

Config overrides `command` and `args` for any agent name.

## Codex Capability Checks

- Documents may declare `required_ssh_targets` or `required_ssh_profile` in frontmatter.
- Known ops documents may also resolve required SSH targets from project-local `.agent-doc/config.toml` entries under `[ssh.docs."<relative/path.md>"]` and `[ssh.profiles.<name>]`.
- If a document is configured as SSH-dependent but no targets resolve, preflight/startup must fail closed before the agent launches.
- For Codex, agent-doc probes those SSH targets before launch.
- When a resumed Codex session later surfaces a target-specific SSH failure, agent-doc treats that as capability drift: retry once with fresh `codex exec`, then fail closed if the required SSH capability still cannot be proven.
- That required-SSH drift detector must also catch bare `socket: Operation not permitted` output when the same Codex `command_execution` event proves SSH context for one of the required targets, while still excluding unrelated localhost/CDP `Operation not permitted` failures.
