# Finish Closeout After Answered Marker Drift

## Problem

The prior `agent-doc` turn wrote and committed the assistant response, but `session-check` still interrupted because live IDE normalization changed the session document after commit. The remaining diff was not a new prompt: it consisted of answered prompt markers (`❯`), a `(HEAD)` response-heading marker, a fresh boundary id, and backlog metadata wording.

## Root Cause

`detect_active_session_post_commit_drift` treated any active-session drift outside the exact exchange-only transient path as unfinished closeout work. Once backlog metadata also changed, the guard no longer recognized the drift as promptless closeout metadata, even though `detect_unstarted_prompt_bearing_diff` found no unresolved `prompt_target`.

## Fix Plan

1. Treat active-session post-commit drift as closed when there is no unresolved prompt marker and the differences are confined to exchange closeout metadata plus backlog metadata.
2. Preserve the existing fail-closed behavior for unrelated body/status edits after commit.
3. Add regression coverage for answered prompt-marker drift with backlog metadata.
4. Reinstall the CLI/shared library and rerun `session-check` on `tasks/agent-doc/agent-doc-bugs2.md`.

## Verification

- Focused regression tests for answered marker handling and exchange/backlog metadata drift.
- Full `cargo test`.
- `agent-doc session-check tasks/agent-doc/agent-doc-bugs2.md` after install.
