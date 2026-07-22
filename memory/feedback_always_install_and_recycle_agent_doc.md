---
name: feedback_always_install_and_recycle_agent_doc
description: "After any agent-doc code fix, always run install + admin recycle without asking — never hold the binary back"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 1505425f-ec1e-4ff0-8444-09df14277d3d
---

After landing ANY agent-doc code change, always `make install-full` (or equivalent
release build + `cargo install`) and then `agent-doc admin recycle`. Do not ask
first, and do not defer it because a concurrent session has uncommitted work in the
tree or because a release is gated.

**Why:** a committed fix that is not installed changes nothing — the running
supervisors keep serving the old binary and stay wedged. Brian confirmed this as a
standing preference on 2026-07-18 after a session committed a queue-drain wedge fix
and stopped short of installing it, leaving two live sessions still stalled.

**How to apply:** install + recycle is NOT the same as cutting a release. It stays
allowed even while [[feedback_stop_cutting_agent_doc_releases]] blocks version
bumps, tags, and publishes. Do the local build + install + recycle; skip the
bump/tag/publish unless separately confirmed. Note in the report if the built tree
included another session's uncommitted changes, but do not let that block the
install. Related: [[feedback_cargo_install_on_release]],
[[feedback_supervisor_autoinstall_drives_install_not_manual]],
[[reference_bin_agent_doc_symlink_is_reporoot_relative]].
