
# Plan — cancelled Codex Stop hook replays the full exchange

- [ ] Reproduce the cancelled-run failure in `src/agent-doc` with an integration test that feeds `hook codex-stop` a `last_assistant_message` payload containing the full exchange or a transcript-shaped blob instead of a single assistant closeout.
- [ ] Inspect the current auto-close path in `src/agent-doc/src/codex_hook.rs` and `src/agent-doc/src/repair.rs` to document the exact unsafe flow: `apply_stop()` trusts `last_assistant_message`, `save_pending()` persists it verbatim, and `repair::run()` replays it through the template write path.
- [ ] Define a binary-owned validation contract for Stop-hook recovery input so auto-close only accepts a real assistant closeout shape:
  - one explicit `<!-- patch:exchange --> ... <!-- /patch:exchange -->` response, or
  - one assistant-response body that can be losslessly wrapped as a single exchange append.
- [ ] Reject or block auto-close when the Stop-hook payload looks like a transcript dump rather than a single response. Minimum red flags to cover: `<!-- agent:exchange -->`, multiple prompt targets, `❯ ` transcript lines, `## User`, or multiple assistant-response headings.
- [ ] Decide whether the binary should ever extract the last valid assistant response from a larger payload. Default plan: do not auto-extract on ambiguous transcript-shaped input; capture it for diagnostics and fail closed instead of guessing.
- [ ] Implement the minimal fix in `src/agent-doc` so cancelled runs cannot patch back the whole exchange via the Stop hook. Keep ordinary interrupted-closeout recovery working for legitimate single-response payloads.
- [ ] Add regression coverage for both sides of the boundary:
  - valid single-response `last_assistant_message` still auto-closes successfully
  - transcript/full-exchange payload does not auto-replay and instead blocks with a concrete recovery message
- [ ] Tighten the Codex-facing runbook/skill text only if needed so the documented Stop-hook contract matches the binary behavior, but keep the enforcement in code rather than instructions.
- [ ] Run the relevant verification in `src/agent-doc` after the fix: targeted Stop-hook tests, broader `cargo test`, and any required build/install step if the instruction surface changes.
