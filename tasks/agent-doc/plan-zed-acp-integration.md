# Plan: agent-doc → Zed integration via ACP

**Status:** proposed
**Date:** 2026-07-03
**Owner:** Brian Takita
**Parent direction:** [`plan-acp-comm-layer.md`](plan-acp-comm-layer.md) — this
plan is its de-risking Milestone 0 (Zed = first ACP client). Realtime semantics,
tier definitions, and the seqcrdt-over-ACP mechanism live there.

## Summary

Integrate agent-doc into Zed through the **Agent Client Protocol (ACP)**, not a
Zed editor plugin. Zed's ACP surface natively provides the live-buffer-aware
read/write, edit review, and process lifecycle that the JetBrains/VSCode plugins
hand-roll. Integrating via ACP lets agent-doc **retire — for the Zed target —**
the editor-plugin + cdylib + socket-IPC + supervisor/controller/tmux + CRDT
stack, shrinking agent-doc to its actual value: document-session semantics +
reasoning orchestration + a thin ACP frontend.

## Why not a Zed plugin (dead end)

The JB/VSCode plugins load the cdylib **in-process** (JNA/koffi `dlopen`),
register a C callback for the socket accept loop, do atomic buffer edits, watch
`.agent-doc/patches/`, and introspect multi-pane layout. Zed's extension model
supports none of this:

- Extensions are **WASM-sandboxed** (`wasm32-wasi`): no `dlopen` of
  `libagent_doc.so`, no C callback into a native accept loop, no in-process FFI.
- Zed has **no general buffer-manipulation / file-watch / multi-pane API** for
  extensions (unlike VSCode). Extension types are LSP, themes, snippets, debug
  adapters, MCP servers, slash commands, (deprecated) agent servers.
- The **agent-server extension type is deprecated** (~Zed v1.5.0) in favor of
  the ACP Registry.

Even the VSCode *fallback* transport (file-signals + shell-out, no socket)
cannot be rebuilt as a Zed extension, because the buffer read/write and pane
geometry it relies on aren't exposed. Do not attempt plugin parity.

## Why ACP fits

| agent-doc mechanism (today) | ACP equivalent (Zed owns it) |
|---|---|
| `publish-live-buffer.signal` + digest provenance (`no_unsaved_operator_edits`) to read unsaved edits without clobber | `fs/read_text_file` **returns unsaved buffer content** |
| Patch-file IPC + socket + ACK + apply-proof (`modificationStamp`/`version`) | `fs/write_text_file` — Zed owns buffer/disk reconciliation |
| Pending-ops / `--pending-add` review flow | `session/request_permission` + native diff review UI |
| Supervisor + tmux pane + controller (CPC) + `controller-state.json` + recycle/handoff | Zed spawns/manages the agent process, threads, worktrees |
| cdylib mtime hot-reload + shadow-copy | n/a — plain binary speaking JSON-RPC over stdio |
| yrs CRDT to merge concurrent editor+agent edits | read-live-buffer-then-write + Zed's serialized diff/permission gate |

Load-bearing fact: **`fs/read_text_file` reads the live, unsaved buffer.** That
is the entire reason the cdylib/CRDT/IPC clobber-avoidance stack exists, and the
source of most recovery war-stories (IPC snapshot drift, stale-cdylib deadlock,
`no_ack`, buffer-differs). Under ACP those failure modes have no place to occur.

## Interaction model: unified A + B

Two interaction modes that **collapse into one implementation** — they share the
ACP frontend, the document-transform core, and the reasoning engine. The only
difference is where a turn's input comes from.

- **A — document is the interface, panel is a thin trigger.** Each turn the
  agent `fs/read_text_file(doc)` (live buffer), parses the exchange component as
  the authoritative input, responds into components. Panel text minimal/ignored.
- **B — conversation in panel, doc as artifact.** `session/prompt` carries the
  conversation; the agent writes results into the `.md` via the transform core.
  Standard ACP agent shape (how Claude Code already runs in Zed).

**Unification:** the prompt handler reads **both** the live doc buffer and the
panel prompt every turn.

- Empty / trigger-token prompt → behaves as A (doc is authoritative input).
- Substantive prompt → behaves as B (panel steers, doc is the edited artifact).

No mode switch to maintain. The doc is *always* live-read and *always* the write
target; the panel is optional conversational steering. Marginal cost of
supporting both over one: input-routing logic in the prompt handler (~90%
shared).

### Constraint on A (open verification item)

**ACP turns are client-initiated.** The agent cannot start a reviewed turn from
its own file-watch; edits and `session/request_permission` only exist inside a
`session/prompt` turn that Zed opens. A pure "save the doc → agent responds"
trigger (the current tmux `route --dispatch-only` model) is **not natively
expressible**. Something must send the prompt — a trivial "respond" message, a
Zed keybinding, or a slash command. Verify against Zed behavior
(`dev: open acp logs`): whether Zed exposes a bindable action to fire a prompt,
and whether it surfaces reviewed edits for a near-empty prompt.

## Delivery / config

Registered as a custom ACP agent (later: ACP Registry):

```json
{ "agent_servers": { "agent-doc": { "type": "custom", "command": "agent-doc", "args": ["--acp"], "env": {} } } }
```

## Implementation phases

### Phase 0 — validate the free path (hours)

Claude Code is already an ACP agent in Zed's registry, and agent-doc is
fundamentally a skill Claude Code runs (works headless in Codex/OpenCode via CLI
subcommands, not FFI). Open the `.md` in Zed, select Claude Code as the external
agent, invoke `/agent-doc <file>`. Its Edit/Write flow through ACP `fs/*` → live
Zed buffer + diff + permission. No plugin, no cdylib, no IPC.

- [ ] Confirm every **pure** document-transform op the skill needs is reachable
      via `agent-doc` CLI subcommands (not FFI-only): parse components, boundary
      reposition (`agent_doc_reposition_boundary_*`), structure-normalize guard
      (`agent_doc_normalize_template_structure`), exchange compaction, diff.
- [ ] Run the skill end-to-end in Zed; confirm live-buffer read + diff review.

### Phase 1 — `agent-doc --acp` first-class agent (medium)

New crate `agent-doc-acp` implementing JSON-RPC over stdio:

- [ ] `initialize` → declare capabilities.
- [ ] `session/new`.
- [ ] `session/prompt` handler:
  - `fs/read_text_file(doc)` → parse (`agent-doc-element`) → compute diff.
  - Route input: empty/trigger → A; substantive → B (both live-read the doc).
  - Run reasoning turn (Claude API / headless Claude Code / Codex — private to
    agent-doc; Zed neither knows nor cares).
  - Assemble patched full document → `fs/write_text_file` under
    `session/request_permission` (Zed renders the diff).
  - Stream `plan` (intended edits), `agent_message_chunk` (status),
    `tool_call`/`tool_call_update` (sub-steps); end with `StopReason`.
- [ ] Boundary/compaction/exchange logic runs **in-process** against the pure
      Rust crates — no FFI/WASM boundary (normal binary).
- [ ] Retire, for the Zed path, the plugin/cdylib/socket-IPC/supervisor/CRDT
      surface (keep for JB/VSCode).

### Phase 2 — optional polish (additive, independent)

- [ ] Component-marker **syntax highlighting** via a tree-sitter/language
      extension (a supported Zed extension type), replacing
      `agent_doc_visual_tokens_json`. The one plugin piece that legitimately
      maps to a Zed extension.

## Open questions to verify

1. **`fs/write_text_file` — live buffer or disk only?** ACP docs are explicit
   that *read* returns unsaved buffer, ambiguous on write. If write only touches
   disk, a smaller reconciliation concern returns. Confirm via `dev: open acp
   logs`. (Almost certainly buffer + diff, since that's Zed's review UX.)
2. **Whole-file write vs. component-granular patches.** ACP has no "patch
   component X" primitive — it's whole-file. agent-doc already computes whole
   documents, but confirm the read-live-then-write-whole flow preserves surgical
   merging when the operator edits mid-turn (re-read immediately before write,
   mirroring the existing apply-proof discipline).
3. **CRDT is NOT retired (correction).** Earlier framing said ACP makes the CRDT
   redundant — true only for weak clobber-avoidance. Under a realtime goal the
   CRDT is load-bearing: Zed caps at **Tier 2 turn-granular convergence**
   (lazily seqcrdt on the agent side + ACP fs), not keystroke realtime. See
   [`plan-acp-comm-layer.md`](plan-acp-comm-layer.md) § Zed design.
4. **Turn triggering for mode A** (see constraint above).

## References

- Zed: [AI Overview](https://zed.dev/docs/ai/overview) ·
  [External Agents](https://zed.dev/docs/ai/external-agents) ·
  [Agent Server Extensions (deprecated)](https://zed.dev/docs/extensions/agent-servers)
- ACP: [Introduction](https://agentclientprotocol.com/overview/introduction) ·
  [File System](https://agentclientprotocol.com/protocol/file-system) ·
  [Prompt Turn](https://agentclientprotocol.com/protocol/prompt-turn)
- [JetBrains × Zed on ACP interop](https://blog.jetbrains.com/ai/2025/10/jetbrains-zed-open-interoperability-for-ai-coding-agents-in-your-ide/)
- Internal: `editors/PLUGIN-SPEC.md`, `editors/SPEC.md`, `editors/vscode/src/`
  (closest template — but not portable to Zed; see "Why not a Zed plugin").
