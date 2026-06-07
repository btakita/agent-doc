# Plan: Guard codex_network_access for Non-Codex Agents

**Bug:** `#codex-network-access-opencode-guard`
**Files:** `src/agent-doc/agent-doc-orchestration/src/preflight.rs`, `src/agent-doc/specs/05-agent-backend.md`

## Problem

`codex_network_access: enabled` in frontmatter is Codex-specific but agent-doc
does not warn when the session runs under a non-Codex harness (opencode,
claude-code). The field has no effect outside Codex — opencode has no
equivalent network sandbox flag — so operators may falsely assume network
access is controlled when it is not.

Runtime paths (`start.rs`, `orchestrate.rs`) already guard
`resolve_codex_network_access` behind `if harness.binary == "codex"`, so the
env is never applied. Missing: a preflight warning so the operator knows.

## Fix

1. In `preflight::run_with_options`, after the harness_mismatch warning check,
   add a new warning (`codex_network_access_non_codex_harness`) when
   `codex_network_access` is set in frontmatter and the active harness is not
   codex.
2. The warning message tells the operator to remove the field or switch the
   agent to codex.
3. Test: verify the guard condition fires for opencode + codex_network_access.

## Status

IMPLEMENTED.
