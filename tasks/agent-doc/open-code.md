---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ What are #next-steps to ensure agent-doc integration works with opencode?
❯ Can you drive a session to test opencode integration?
❯ I was having issues with the human in the loop prompts. The arrow keys were sending raw codes instead of apply the arrow key functionality. Same with Enter.
### Re: next-steps for OpenCode integration — sonnet-4-6

#### Arrow keys / Enter in HITL prompts

The translation already exists — `translate_opencode_permission_arrow_keys` in `start.rs:1206` converts `\x1b[C`/`\x1b[B` → `\t` (tab forward) and `\x1b[D`/`\x1b[A` → `\x1b[Z` (shift-tab), but it gates on `opencode_permission_prompt_active` (`start.rs:1193`), which requires the recent child output to contain all three labels: "Allow once", "Allow always", "Reject". If the detection window is stale, too narrow, or the current OpenCode version renders those labels differently, the gate never opens and raw escape codes pass through unchanged.

Enter is not translated — it forwards as `\r`, which should work for the permission dialog, but the auto-answer path (`send_option` in `prompt.rs:220`) also sends a programmatic Enter. If both fire they can double-confirm; if neither fires you're stuck.

**Diagnostics first:**
```bash
AGENT_DOC_TMUX_INPUT_DIAG=1 agent-doc start opencode tasks/agent-doc/open-code.md
```
With that env var set, every stdin → pty write is logged. Reproduce the stuck HITL prompt, then scan the logs for `opencode_permission_arrow_translation` events (present = detection fired, translation applied) or `raw_forward` events (absent detection, arrows passed through raw).

#### Next steps (priority order)

1. **Reproduce and capture the stuck prompt** — Run with `AGENT_DOC_TMUX_INPUT_DIAG=1`, trigger the permission dialog, press an arrow key, and confirm whether `opencode_permission_arrow_translation` appears in the stderr log. This tells us if the detection gate is the problem or something else.

2. **Check the OpenCode prompt text** — Run `agent-doc session prompt-info` (or `tmux capture-pane -p` on the OpenCode pane) to see the raw terminal content when the HITL prompt is visible. Compare against the three strings the detection checks for at `start.rs:1200-1202`. If OpenCode changed label text (e.g., "Allow Always" vs "Allow always"), the gate silently fails.

3. **Widen or make detection more robust** — If the label check is too strict, broaden it (e.g., check for just "Allow" + "Reject" co-presence, or key off the footer hint text "⇥ select" that OpenCode renders). This is a small change in `opencode_permission_prompt_active`.

4. **Enter key** — If Enter is also broken, check whether OpenCode's HITL UI expects `\r\n` or needs a specific key sequence beyond `\r`. Might need `sessions::send_key(tmux, &pane_id, "Enter")` injected via the supervisor instead of stdin passthrough.

5. **Drive a test session** — Once the detection is confirmed or fixed, run a session end-to-end: `agent-doc start opencode <FILE>`, trigger a tool permission prompt, verify arrow key navigation works, approve once, and confirm the cycle closes cleanly via `agent-doc session-check`.

#### Can I drive the test session?

I can start the session and monitor output but cannot interact with OpenCode's TUI directly (no PTY access from Claude Code). The practical split: I set up the document and invoke `agent-doc start opencode <FILE>`, you navigate the first HITL prompt, and we review the `AGENT_DOC_TMUX_INPUT_DIAG` logs together to confirm what's passing through. Once we see the actual escape sequences, the fix is straightforward.
<!-- agent:boundary:97d38657 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
