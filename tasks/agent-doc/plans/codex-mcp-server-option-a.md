# Plan — Option A: drive Codex agent-doc via an MCP server (root-cause fix for recursive self-shell-out)

**Backlog:** `#codex-mcp-server` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft — deferred behind Option B (`#codex-self-reinvoke-prevent`), which ships first as the interim.
**Decision:** User chose Option B now + this Option A plan for later (2026-06-05).

## Problem

agent-doc is a CLI. Claude Code runs the workflow **in-process** (the `/agent-doc`
skill executes inside the Claude turn). Codex has no in-process skill execution,
so it drives agent-doc by **running the CLI** (`agent-doc <FILE>`). "Continue the
turn / drain the next queue item" therefore means **re-running the CLI**, and the
Stop-hook auto-loop does so inside the owner pane → the new process re-enters the
pane it runs in → recursion (`recursive_direct_invocation_blocked`).

Option B (the interim, shipping first) stops the Stop hook from *instructing* the
re-run and redirects Codex to answer + `finalize` in-pane. That guards the
recursion but keeps the CLI-drives-Codex model. Option A removes the model.

## Goal

Expose the agent-doc hot path as an **MCP server** so Codex drives agent-doc via
**tool calls against a persistent session** instead of spawning a fresh CLI in the
owner pane. With tool calls there is no nested process and no pane re-entry, so
the self-reinvoke recursion is structurally impossible.

## Design sketch

- **MCP server** (`agent-doc mcp serve` or a dedicated binary) holding per-document
  session state in-process (cycle_state, capture ledger, snapshot handles).
- **Tools** mirroring the hot path, each operating on a `file` arg:
  - `preflight` → returns the cycle contract JSON (baseline, diff, queue, tiers).
  - `plan` → execution contract.
  - `respond`/`finalize` → accept the assistant response body + pending mutations,
    run the write+commit+session-check boundary, return the closeout result.
  - `session_check`, `repair`, `read`.
- **Codex integration**: register the MCP server in Codex config; replace the
  Stop-hook "run `agent-doc <FILE>`" auto-loop with a tool-call continuation
  (the server returns the next queue prompt + contract; Codex calls `finalize`).
- **Reuse the orchestration crate**: the MCP tool handlers call the same
  `agent_doc_orchestration` functions the CLI subcommands call — the CLI and MCP
  surfaces share one implementation. No logic fork.

## Risks / open questions

- Session lifetime + concurrency: one server per project vs per document; how the
  server maps to tmux panes / live IDE IPC (the visible-buffer guard, CRDT merge).
- Streaming: the CLI `--stream` path writes incrementally; the MCP equivalent
  needs a streaming tool result or checkpoint protocol.
- Migration: keep the CLI fully working (Claude/OpenCode/direct-exec) while Codex
  moves to MCP; do not regress the existing harnesses.
- Auth/headless: interactively-authenticated MCP servers may be absent in
  headless/cron Codex runs (cf. the claude.ai MCP caveat).

## Exit criteria

- Codex drives a full agent-doc cycle (preflight → respond → finalize →
  session-check) and a multi-item auto-queue drain entirely via MCP tool calls,
  with zero `agent-doc <FILE>` shell-outs from the owner pane.
- The recursive-direct-invocation guard never fires under the MCP path.
- CLI harnesses (Claude Code, OpenCode, direct-exec) are unchanged and green.

## Relationship to Option B

Option B (`#codex-self-reinvoke-prevent`, shipped) is the tactical fix: the Stop
hook redirects Codex to answer + `finalize` in-pane instead of instructing a CLI
re-run. Option A supersedes it for Codex once the MCP surface exists; until then
Option B keeps the deadlock from biting.
