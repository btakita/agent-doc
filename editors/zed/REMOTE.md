# Zed remote development qualification

Use this runbook to qualify agent-doc's Zed integration on a Zed remote
development host (`editors/SPEC.md` section 13, "Zed remote" row). Status: the
procedure and its scripted check exist, but no Zed remote host has run them
yet. Until a run is recorded here, Zed remote stays "not qualified".

## What runs where

- **Locally:** the Zed UI and the installed (or dev) agent-doc extension.
- **Remote host:** Zed's headless server, which must exactly match the local
  Zed version. Zed starts it over SSH and syncs the extension to it by id and
  version; a dev extension is synced by content fingerprint.
- **On the remote, from the worktree's login-shell `PATH`:**
  - `agent-doc zed-lsp`, which the extension resolves with `worktree.which("agent-doc")`;
  - the project controller it talks to;
  - tmux and every agent-doc session.

  Nothing agent-doc runs on the local machine. A local agent-doc binary or
  controller is irrelevant to the remote worktree.

## Requirements on the remote host

1. **`agent-doc` on the remote worktree's `PATH`.** When the lookup fails,
   the extension reports "agent-doc was not found in the Zed worktree PATH".
   The script checks with a login shell (`sh -lc`), which is only an
   approximation of the environment Zed's remote server gives the worktree.
   If Zed still cannot find agent-doc while the script passes, set
   `lsp.agent-doc.binary.path` in the remote project's settings.
2. **A binary that serves `zed-lsp` and the in-place handoff protocol
   (`agent_doc_zed_lsp_handoff/v1`).** With that protocol, a later install on
   the remote is picked up without restarting Zed. Without it, every remote
   upgrade needs `editor: restart language server`.
3. **The project's `.agent-doc/` on the remote checkout.** `zed-lsp` resolves
   the project root from the opened file and its controller socket from that
   root, so the controller is always the remote host's.
4. **Matching extension and binary generations.** On each registration the
   controller logs `controller_crdt_replica_editor_generation`. It compares the
   extension version (`editor_version`) with the version this binary pairs
   with (`editors/zed/extension.toml`), and the `zed-lsp` binary version with
   the controller's.

## Scripted check

Run it from an agent-doc checkout on the local machine:

```sh
# This machine (the local half, or a host you are logged into)
python3 scripts/zed_remote_qualify.py --project /path/to/project

# A Zed remote host, through the same SSH destination Zed uses
python3 scripts/zed_remote_qualify.py --remote user@host --project /remote/path/to/project
```

It checks, on the target, in a login shell:

| Check | Passes when |
|---|---|
| `path` | `agent-doc` resolves on the login-shell `PATH` |
| `version` | the target reports a version (remote: and it matches local) |
| `lsp` | `agent-doc zed-lsp` answers initialize, shutdown and exit, and its `serverInfo.version` matches |
| `handoff` | the binary validates an `agent_doc_zed_lsp_handoff/v1` file in check mode |
| `controller` | `agent-doc controller status --project-root` resolves exactly that project on the target, with its socket inside it (read-only: no controller is started) |
| `extension` | informational: the extension version this checkout ships |

The script exits non-zero when any check fails. `--json` prints the results.

## Manual steps the script cannot do

1. Open the remote project in Zed (**File → Open Remote**) with the same local
   Zed version the remote server runs.
2. Install the agent-doc extension locally. For a dev extension, use
   **Extensions → Install Dev Extension** on `editors/zed`. Confirm that Zed
   syncs it: the remote server's log shows the extension loading.
3. Open a session document. In Zed's language server log for `agent-doc`,
   confirm that the server started from the remote `agent-doc` path. In the
   remote project's `.agent-doc/ops.log`, confirm that
   `controller_crdt_replica_editor_generation ... editor_kind=zed` reports
   `status=current` and `layout_authority=document_only`.
4. Upgrade agent-doc on the remote host with an install that replaces the
   binary file. The `agent-doc` language server log should show
   `zed_lsp_binary_handoff status=exec`, then `status=resumed` with the same
   `pid`, and editing should continue without a restart.

## Record

| Date | Zed version | Remote host | Checks | Result |
|---|---|---|---|---|
| 2026-10-26 | 1.25.3 (local only) | none: no Zed remote host available | script, locally only: `path`, `version`, `lsp`, `handoff` and `controller` pass against a build of this branch; the remote transport ran only to its unreachable-host failure | local half only; remote **not qualified** |
