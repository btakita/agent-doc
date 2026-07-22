# Live AI-Agent ↔ Trends Demo — Call-Day Runbook

Goal on the call: a real agent session (Claude Code, optionally Codex)
drives the visible pulse app against live-ingested trend data.

## What works when

- **Today (before R1 lands):** the live-agent demo already works with the
  four existing tools — "check the pulse, boost rust, track wasm
  runtimes, explain the top trend" all update the visible window. This is
  demo beat 6 and needs nothing new.
- **After R1-lite/R2:** the full research flow — agent investigates a
  trend and its report appears in the app.

> Once the startup integration lands (spec:
> `docs/design/research-interface-plan.md` § Agent integration at
> startup), this whole section collapses to: `pulse setup` once, then
> `pulse app --companion` — MCP is on by default with a status-bar
> indicator, and `--agent-terminal` can spawn the visible claude session.
> Until then, follow the manual steps below.

## Install / registration (one-time, before rehearsal)

Nothing to install for the server — MCP is in-process in the app. Per
client:

- **Claude Code** (registration is a config entry, user-scoped so it
  works from any directory):
  ```
  claude mcp add -s user --transport http pulse http://127.0.0.1:7432/mcp
  claude mcp list        # verify it shows up
  ```
  In a session, `/mcp` shows connection status (app must be running).
- **Codex**: `[mcp_servers.pulse]` in `~/.codex/config.toml`. If the
  installed Codex lacks native streamable-HTTP MCP, use the shim:
  `command = "npx", args = ["-y", "mcp-remote", "http://127.0.0.1:7432/mcp"]`
  — this is the one case where something gets installed (npx fetches
  mcp-remote on first run; do that BEFORE the call, not on it).
- `pulse research doctor` (R3) automates these checks once it exists.

## Skill (optional, recommended for the call)

Purpose: one-line invocation + pinned behavior instead of live prompt
typing. Install as `~/.claude/skills/pulse-research/SKILL.md`:

- **Trigger**: `/pulse-research <topic-or-question>`
- **Body (behavior contract)**: use the pulse MCP tools — `get_pulse` /
  `list_topics` to orient, `topic_posts` for sources; read the top
  threads with web tools; assess whether the spike is organic (source
  diversity, velocity shape); keep narration to 2–3 sentences per step
  (the panel is watching); ALWAYS finish with `submit_research`
  including citations, ≤400 words; report the submitted title.
- Codex equivalent: no skill mechanism needed — pass the same contract
  via the R3 prompt template (`docs/research-prompt.md`) to
  `codex exec`, or put it in an `AGENTS.md` in the demo working dir.
- Until R1 lands, a trimmed skill using only the four existing tools
  works for beat 6 ("orient, adjust the mix as asked, track topics").

## In-app chat: live API setup (rehearse once)

The chat panel's live mode is API-billed BYOK (separate from all the
CLI/MCP paths): put `PULSE_API_KEY` (and optionally `PULSE_API_BASE`,
`PULSE_MODEL`) in `.env`, drop `--replay` from the launch command, and
run one full conversation with tool calls before the call — including
verifying the model id is still current. If the key is absent or the
provider misbehaves, `--replay` is the deterministic fallback and the
demo script works identically. Tier narrative while demoing: chat =
metered API key, research buttons = subscription CLIs, terminal agent =
subscription via MCP — three billing modes, one tool surface, all live.

## Call-day flow

1. Pre-call: `pulse ingest` (warm data), then
   `pulse app --live --mcp-port 7432 --companion` (or `--fixture
   --replay` if wifi is sketchy — the agent beat still works, against
   fixture trends; the MCP endpoint doesn't care).
2. Pre-call: open the Claude Code terminal on the shared monitor, run
   one throwaway `/pulse-research` to warm login/quota and confirm the
   `/mcp` connection. Kill the output; the panel never sees the warm-up.
3. On the beat: `/pulse-research <whatever is genuinely spiking>` —
   narrate that a second vendor process is driving the same state the
   UI reads. Watch the window update; after R2, open the report.
4. Optional encore: same question through `codex exec` → comparative
   view (two vendors, one tool surface).
5. Narration tip: while a delegated run's clock ticks, that's your
   moment to talk architecture (shared state, billing tiers, the
   via-mcp chips appearing) — the run indicator turns the wait into
   the talk track, not dead air. Rehearse timing one real run so you
   know your ~mm:ss window.

## Failure modes specific to the live agent beat

- Subscription/login state is per-machine and expires — the warm-up run
  in step 2 is the check. No warm-up, no live agent beat; fall back to
  the in-app replay chat, which demos the same shared-state story
  without any external process.
- First-token latency on a cold session can feel long on a call — the
  warm-up also fixes this.
- Agent goes off-script: the skill contract keeps it short, but if a
  run wanders, interrupt it, say "the tools constrain what it can touch
  — watch the budget clamp," and re-invoke. The MCP surface's clamps
  (budget ceiling, capped submissions) mean a wandering agent still
  can't break the demo state.
