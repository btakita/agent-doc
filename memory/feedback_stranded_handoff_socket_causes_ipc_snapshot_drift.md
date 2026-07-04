---
name: feedback_stranded_handoff_socket_causes_ipc_snapshot_drift
description: "finalize \"timed out waiting for project controller at controller.sock\" + later IPC snapshot-adoption drift = a stranded controller-generation handoff; fix is self-promotion (#stuckhandoffselfheal)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 9912a50a-d4b8-449c-a7e2-7a6ad8ad2fea
---

An uncommitted agent-doc response whose finalize logged **"timed out waiting for
project controller at .../controller.sock"**, and whose later `agent-doc commit`
refused closeout with **"IPC snapshot adoption was blocked" / "captured response body
is not present in the staged snapshot even though the snapshot already matches HEAD"**
(snapshot bytes == HEAD but < the on-disk file), is a **stranded controller-generation
handoff**, not ordinary staleness.

**How to recognize it live:** `.agent-doc/controller.sock` is a 0-byte file with **no
listener** (`ss -xlp | grep controller.sock` empty), while a `controller serve` pid is
listening only on `.agent-doc/controller-handoff-<pid>-<gen>.sock`; yet
`controller-state.json` says `socket_path=controller.sock, handoff_state=stable`. The
gen-N handoff client got `promote_handoff` (in-memory→Stable, bootstrap rewritten to
claim the public socket) but **died before `rename(temp_sock → controller.sock)`**.
Promotion in `handoff_stale_controller` (`project_controller/rpc.rs`) is non-atomic and
trusts the client to survive that final rename.

**Drift chain:** dead `controller.sock` → finalize times out on the canonical
`socket_path` → writes the response to disk out-of-band → controller IPC snapshot never
adopts it → `commit` sees snapshot==HEAD but body absent → refuses. The discovering
`connect()`/`admin inspect` path still reaches the controller via the handoff socket,
so the concurrent session looks fine — only canonical-path clients (finalize) time out.

**Recovery now:** the on-disk doc is usually consistent (complete response, single
boundary, answered head struck by `#ftstrike` from the refused commit); commit it
directly with git (`agent-doc(<doc>): recover...`). The stranded controller self-reaps
at the 45s stranded/suicide watchdog, clearing the live drift.

**Root-cause fix (`#stuckhandoffselfheal`, this session):** the replacement now
COMPLETES its own promotion in the serve loop instead of only suiciding — pure policy
`handoff_replacement_should_self_promote` in `agent-doc-controller/src/status.rs`
(Stable + still on temp socket + past a 3s grace), wired in `serve_with_options`
(`rpc.rs`) to `rename(temp → public_sock)` and keep serving. Restores `controller.sock`
in ≤3s so finalize never falls to the disk-write drift path. See
[[feedback_no_ack_write_socket_finalize_fails_writecommit_recovers]] and
[[feedback_ipc_crdt_response_loss_forcedisk_recovery]] for adjacent finalize-drift modes.
