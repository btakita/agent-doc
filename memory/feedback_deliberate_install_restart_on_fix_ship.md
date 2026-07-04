---
name: feedback_deliberate_install_restart_on_fix_ship
description: "When shipping an agent-doc fix that must go live, deliberately install + restart the supervisor — don't defer to self-recycle."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 987745dd-4227-4020-8fe6-87c89193dc33
---

When you ship an agent-doc code fix in a dogfood session (the kind that must take effect on the live binary/cdylib/plugin), **deliberately run the full install + restart yourself** — do NOT defer to the supervisor self-recycle or leave it as "operator action needed."

Operator directive 2026-06-14: "You should always deliberately run install + restart in these types of cases."

**How to apply:** After the response/commit closeout for the fix lands, run the install+restart sequence to make the fix live, then verify:
1. `cargo build --release`
2. `cargo install --path .` (binary for `~/.cargo/bin` → tmux/supervisor)
3. `agent-doc lib-install` (cdylib for the live JB plugin hot-reload)
4. Restart the supervisor so it promotes the new binary: `agent-doc session restart-supervisor <FILE> --force` (or `agent-doc admin recycle --all-projects`).
5. For a `.kt` plugin change, the new signed zip still needs an operator IntelliJ reinstall + restart (can't be agent-done) — call that out, but do everything else.

**Why:** The `#no-mid-session-install` hazard (installing mid-turn re-stales the running supervisor → live_prompt_drift/exit-75) is real, but the right answer is NOT to defer — it's to install and **immediately restart/recycle the supervisor** so the staleness window is closed at once and the fix is actually live. Deferring to self-recycle leaves the fix not-live and the operator waiting. This **refines** [[feedback_no_mid_session_install_against_live_supervisor]]: do the install, but pair it with the deliberate restart in the same closeout, after the response commits.
