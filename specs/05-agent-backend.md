> Extracted from SPEC.md — see [index](../SPEC.md)

# Agent Backend

## Trait

`fn send(prompt, session_id, fork, model) -> (text, session_id)`

Agent backends are response producers, not session-document writers. Claude
Code, Codex, OpenCode, and custom backends may differ in process invocation,
resume flags, JSON schemas, capability probes, and streaming events, but they
all hand final response text back to the same closeout layer. They must not
apply editor IPC payloads, write `agent:exchange`, save snapshots, or decide
that console-visible output is a durable response. The response exists only
after `agent-doc finalize <FILE>` or strict `agent-doc write --commit <FILE>`
applies it through the shared write/commit/session-check path.

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

## Prompt Cache Boundary

Direct-run prompts are assembled as:

1. Stable prefix
2. `PROMPT_CACHE_BOUNDARY`
3. Volatile suffix

Only durable instructions may live above the boundary: response format contracts,
harness-neutral behavior instructions, turn-payload reading rules, cache-control
metadata, and the provider replay-key material derived from those durable
instructions. Turn-local facts must stay below the boundary: file paths, queue
heads, diffs, current document excerpts, status text, prompt-bearing change
sections, compaction/accretion diagnostics, bounded context packs, session ids,
and recovery markers.

Provider-specific cache requirements are represented explicitly instead of
being inferred from raw prompt text. The boundary carries the ephemeral
cache-control breakpoint, and the provider cache key is
`agent-doc-prompt-cache-v1:<routing-affinity-sha256>:<stable-prefix-sha256>`.
Routing affinity includes the agent, model, and prompt mode. The volatile suffix
never contributes to the stable-prefix fingerprint or provider cache key.

The token/performance gate persists prompt-cache effectiveness history as JSONL
samples keyed by provider, harness, and real transcript id. Current Codex/OpenAI
and Claude/Anthropic transcript samples are compared against the latest matching
workload history. A missing history match reports `baseline_required`; a
regression fails with cached-input delta, creation-token spike, thresholds, and
the same ranked miss causes used by session-cost diagnostics.

## Streaming Checkpoints

- Streaming agent paths save the first non-empty partial response immediately, then save changed partial output at most once every 30 seconds.
- Partial checkpoints and final response captures are cycle-scoped facts in the shared `state.db` ledger; neither creates a per-cycle file.
- Partial checkpoints are recovery evidence only: they must not advance the document cycle to `response_captured`, and automatic replay still requires a final validated response capture or an already-visible response in the document.
- Partial checkpoint writers are bound to the cycle ID observed at writer creation. If the document cycle is later committed, abandoned, or replaced by another cycle, the writer must stop checkpointing and log `partial_response_checkpoint_stopped` instead of mutating stale partial ledgers from an older response. This includes stale `preflight_started` repair that abandons an empty prompt-bearing cycle while the unresolved prompt remains visible for the next fresh preflight.
- Streaming chunks must not invoke document write-back. The runtime buffers them until `is_final`, then submits the complete final response exactly once. Console progress and `state.db` recovery facts are the only permitted partial-output surfaces.
- A healthy streamed turn must reach committed closeout without `repair`. Repair remains an exceptional crash/restart recovery path and must never be required merely because the same turn first wrote a response prefix and later produced the full body.

## Direct Run Heartbeats

- Non-streaming `agent-doc run` child waits are parent-visible in ordinary terminal runs: while the backend blocks, `run` emits `[run] heartbeat phase=child_agent_wait ...` stderr every 30 seconds by default.
- When `run` executes with terminal stderr inside a tmux pane owned by a Codex/OpenCode parent harness, routine `[run]` / `[diff]` / `[commit]` stderr is redirected to `.agent-doc/logs/run-stderr.log` so progress output cannot paint over the foreground TUI. `AGENT_DOC_TMUX_INPUT_DIAG` / `AGENT_DOC_DEBUG_STDIN` opt back into foreground stderr for diagnostics.
- `AGENT_DOC_RUN_HEARTBEAT_SECS` overrides the interval for tests and local diagnostics; values below 1 second clamp to 1.
- Each heartbeat also updates the open cycle state's `updated_at` and `last_event` without advancing the phase, so `session-check` and operators can distinguish a progressing long wait from a stale `preflight_started` cycle.

## Custom Backends

Config overrides `command` and `args` for any agent name.

## Codex Capability Checks

- Documents may declare `required_ssh_targets` or `required_ssh_profile` in frontmatter.
- Known ops documents may also resolve required SSH targets from project-local `.agent-doc/config.toml` entries under `[ssh.docs."<relative/path.md>"]` and `[ssh.profiles.<name>]`.
- If a document is configured as SSH-dependent but no targets resolve, preflight/startup must fail closed before the agent launches.
- For Codex and OpenCode, agent-doc probes those SSH targets before launch. OpenCode also runs a child SSH probe through `opencode run --format json` so the managed child capability, not just host SSH, is proven before dispatch.
- Managed interactive Codex capability proof is frontmatter-opt-in: only
  `managed_proof: true` may turn network, SSH, explicit `--add-dir`, or
  auto-injected submodule/superproject writable roots into a pending dispatch
  gate. Omitted/false records `codex_capability_proof status=not_required`.
  Managed OpenCode proof selection is unchanged. An enabled Codex proof uses
  ephemeral, rule-free, low-reasoning `codex exec --json` children; when
  sandboxed network and writable-root checks are both required they share one
  child invocation and validate both exact markers. `danger-full-access` still
  runs the DNS/HTTPS shell check directly. Writable-root proof tests launcher
  access plus sandboxed child access, including `index.lock` creation/removal
  for git metadata. Successful events retain per-phase `timings_ms`, proof and
  writable-root contract fingerprints, and the existing pending/proven/failed
  dispatch semantics.
- A supervisor binary hot-reexec that adopts the same still-live harness child may carry a successful managed proof forward only when an exact fingerprint of the harness command, launch args, resolved environment, network requirement, SSH targets, and writable-root contract matches. The replacement must consume supervisor-only child-pid, PTY-fd, and preserved-contract handoff variables before resolving the managed child environment, and launch-spec assembly must defensively exclude them, so transport-state changes cannot perturb the fingerprint. The replacement must record a new post-`session_start` `status=proven source=reexec_preserved_child` event, while a missing/mismatched contract, non-proven gate, dead child, or fresh child must run the probe again. Raw environment values must never appear in the handoff or proof event.
- OpenCode child capability probes must build arguments from the `opencode run` surface, preserving supported run flags such as `--model`, `--agent`, `--command`, `--file`, and `--dangerously-skip-permissions` while dropping TUI-only launch flags. If the child prints OpenCode usage/help or unknown-option output instead of the probe marker, classify it as CLI construction/usage failure, not as sandbox network or SSH failure.
- `agent-doc session status` reports the managed Codex/OpenCode capability proof as `not_required`, `pending`, `proven`, `failed`, `missing`, or `unknown` so operators can distinguish launch args from live proof.
- Direct Codex backend resume cannot add new `--add-dir` roots. If the current launch args require writable roots, a saved resume id must be ignored and the turn must start a fresh `codex exec` process with the full root set.
- Direct non-streaming Codex backend response capture selects the last `item.completed` `agent_message` before `turn.completed` as the patchback body. Earlier assistant messages from the same JSONL stream are treated as progress/status chatter and must not be concatenated into durable response text. If the stream contains multiple assistant messages but no `turn.completed` boundary, capture is ambiguous and the backend must fail closed.
- Codex backend stderr handling must preserve real subprocess errors while suppressing known unrelated marketplace validation noise from Codex's plugin and skill loaders, including external `interface.defaultPrompt` warnings under `.codex/.tmp/plugins/plugins/` and the loader's synthetic `interface.icon_small` / `interface.icon_large` path warnings. Local project plugin manifest warnings must remain visible.
- When a resumed Codex session later surfaces a target-specific SSH failure, agent-doc treats that as capability drift: retry once with fresh `codex exec`, then fail closed if the required SSH capability still cannot be proven.
- Resumed Codex streaming turns with required SSH must not leak stale assistant prelude text from the discarded session: buffer early agent chunks until the stream proves required SSH success or completes successfully, then flush them; if required SSH drift forces a fresh retry first, drop that buffered prelude.
- That required-SSH drift detector must also catch bare `socket: Operation not permitted` output when the same Codex `command_execution` event proves SSH command context for one of the required targets, while still excluding unrelated localhost/CDP `Operation not permitted` failures. Generic command output that merely mentions a required target and an old SSH failure, such as `rg` output from `.agent-doc/captures`, must not be classified as live required-SSH capability loss unless the command is the required SSH command or the diagnostic line itself is a direct SSH failure from that command.
