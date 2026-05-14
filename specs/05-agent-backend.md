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

## OpenCode Backend

Default: `opencode run`. Session handling: `--session {id}` or `--continue --fork`. Model handling uses `--model <provider/model>`, so a document can set `agent: opencode` with `opencode_model: zai/glm-5` (or another OpenCode model ID). Removes `OPENCODE_CLIENT` env var. The non-streaming backend returns trimmed stdout and does not persist a session id because default `opencode run` output does not expose one.

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

Prompt context must keep remote host evidence project-scoped. Globally approved SSH commands, ambient SSH config, and unrelated project history are not evidence that a named remote host belongs to the current document's project. A prompt may identify a named remote host only when the current user prompt, document/frontmatter, project-local `.agent-doc/config.toml`, or project-local runbook explicitly names it; otherwise the agent should ask or record a follow-up to confirm the intended host.

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
- For Codex and OpenCode, agent-doc probes those SSH targets before launch. OpenCode also runs a child SSH probe through `opencode run --format json` so the managed child capability, not just host SSH, is proven before dispatch.
- Managed interactive Codex sessions record `codex_capability_proof status=pending` before the child pane launches and then run live probes asynchronously when the document requests `codex_network_access: enabled`, `required_ssh_targets`, explicit extra writable roots via `--add-dir`, or auto-injected submodule/superproject git metadata roots. Managed interactive OpenCode sessions record `opencode_capability_proof status=pending` for documents with `required_ssh_targets` and for explicit network checks. For explicit network access, Codex uses host DNS and a bounded `codex exec --json` child command; OpenCode uses a bounded `opencode run --format json` child command. For writable roots, Codex proof must test both launcher write access and a bounded `codex exec --json` child command under the same launch args; git metadata roots must prove that `index.lock` can be created and removed, so parent `.git/modules/...` denial is caught before commit work. Successful proof events must include `timings_ms` phase data for host DNS, child network, required SSH, launcher writable-root checks, child writable-root checks, total elapsed proof time, and a normalized `writable_root_contract` fingerprint when Codex writable roots are present. Prompt dispatch remains gated while proof is pending and is disabled if proof fails. User-visible successful/failed proof summaries are tmux status messages targeted at the owned pane and must not be written into the child pane transcript; the full event remains in the session log. OpenCode readiness detection may treat an otherwise chrome-only `context ... % used` footer or idle splash (`Ask anything...`, build-plan line, command/footer chrome, cwd/version status) as an idle composer, because the OpenCode TUI can render no standalone `>` prompt line after proof completion. Managed non-dispatch route treats a ready actor without a current proof after the latest `session_start`, or with proof for a different Codex writable-root contract, as unproven, restarts the managed session fresh once with the original launch contract, and reroutes only after the fresh session becomes ready and proven.
- `agent-doc session status` reports the managed Codex/OpenCode capability proof as `not_required`, `pending`, `proven`, `failed`, `missing`, or `unknown` so operators can distinguish launch args from live proof.
- Direct Codex backend resume cannot add new `--add-dir` roots. If the current launch args require writable roots, a saved resume id must be ignored and the turn must start a fresh `codex exec` process with the full root set.
- Direct non-streaming Codex backend response capture selects the last `item.completed` `agent_message` before `turn.completed` as the patchback body. Earlier assistant messages from the same JSONL stream are treated as progress/status chatter and must not be concatenated into durable response text. If the stream contains multiple assistant messages but no `turn.completed` boundary, capture is ambiguous and the backend must fail closed.
- When a resumed Codex session later surfaces a target-specific SSH failure, agent-doc treats that as capability drift: retry once with fresh `codex exec`, then fail closed if the required SSH capability still cannot be proven.
- Resumed Codex streaming turns with required SSH must not leak stale assistant prelude text from the discarded session: buffer early agent chunks until the stream proves required SSH success or completes successfully, then flush them; if required SSH drift forces a fresh retry first, drop that buffered prelude.
- That required-SSH drift detector must also catch bare `socket: Operation not permitted` output when the same Codex `command_execution` event proves SSH command context for one of the required targets, while still excluding unrelated localhost/CDP `Operation not permitted` failures. Generic command output that merely mentions a required target and an old SSH failure, such as `rg` output from `.agent-doc/captures`, must not be classified as live required-SSH capability loss unless the command is the required SSH command or the diagnostic line itself is a direct SSH failure from that command.
