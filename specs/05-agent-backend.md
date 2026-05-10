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

## Streaming Checkpoints

- Streaming agent paths save the first non-empty partial response immediately, then save changed partial output at most once every 30 seconds.
- Partial checkpoints live beside final response captures at `.agent-doc/captures/<doc-hash>/<cycle-id>.partial.json`.
- Partial checkpoints are recovery evidence only: they must not advance the document cycle to `response_captured`, and automatic replay still requires a final validated response capture or an already-visible response in the document.

## Direct Run Heartbeats

- Non-streaming `agent-doc run` child waits are parent-visible: while the backend blocks, `run` emits `[run] heartbeat phase=child_agent_wait ...` stderr every 30 seconds by default.
- `AGENT_DOC_RUN_HEARTBEAT_SECS` overrides the interval for tests and local diagnostics; values below 1 second clamp to 1.
- Each heartbeat also updates the open cycle state's `updated_at` and `last_event` without advancing the phase, so `session-check` and operators can distinguish a progressing long wait from a stale `preflight_started` cycle.

## Custom Backends

Config overrides `command` and `args` for any agent name.

## Codex Capability Checks

- Documents may declare `required_ssh_targets` or `required_ssh_profile` in frontmatter.
- Known ops documents may also resolve required SSH targets from project-local `.agent-doc/config.toml` entries under `[ssh.docs."<relative/path.md>"]` and `[ssh.profiles.<name>]`.
- If a document is configured as SSH-dependent but no targets resolve, preflight/startup must fail closed before the agent launches.
- For Codex, agent-doc probes those SSH targets before launch.
- Managed interactive Codex sessions also record `codex_capability_proof status=pending` before the child pane launches and then run live probes asynchronously when the document requests `codex_network_access: enabled`, `required_ssh_targets`, explicit extra writable roots via `--add-dir`, or auto-injected submodule/superproject git metadata roots. For explicit network access, the proof must include host DNS and a bounded `codex exec --json` child command that performs DNS plus HTTPS from inside the Codex sandbox under the same launch args. For writable roots, the proof must test both launcher write access and a bounded `codex exec --json` child command under the same launch args; git metadata roots must prove that `index.lock` can be created and removed, so parent `.git/modules/...` denial is caught before commit work. Successful proof events must include `timings_ms` phase data for host DNS, child network, required SSH, launcher writable-root checks, child writable-root checks, and total elapsed proof time. Prompt dispatch remains gated while proof is pending and is disabled if proof fails. Managed non-dispatch route treats a ready actor without a current proof after the latest `session_start` as unproven, restarts the managed Codex session fresh once with the original launch contract, and reroutes only after the fresh session becomes ready and proven.
- `agent-doc session status` reports the managed Codex capability proof as `not_required`, `pending`, `proven`, `failed`, `missing`, or `unknown` so operators can distinguish launch args from live proof.
- When a resumed Codex session later surfaces a target-specific SSH failure, agent-doc treats that as capability drift: retry once with fresh `codex exec`, then fail closed if the required SSH capability still cannot be proven.
- Resumed Codex streaming turns with required SSH must not leak stale assistant prelude text from the discarded session: buffer early agent chunks until the stream proves required SSH success or completes successfully, then flush them; if required SSH drift forces a fresh retry first, drop that buffered prelude.
- That required-SSH drift detector must also catch bare `socket: Operation not permitted` output when the same Codex `command_execution` event proves SSH context for one of the required targets, while still excluding unrelated localhost/CDP `Operation not permitted` failures.
