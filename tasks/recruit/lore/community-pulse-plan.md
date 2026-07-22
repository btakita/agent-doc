# Community Pulse — Prototype Plan (Lore Case Study #2)

Slint desktop app demoing "Community Pulse": a trend engine over real community
data, driven through a **chat + visual topic composer** interface, with a
voice (V2V) lens as an optional stretch layer over the same agent tool bridge.

Framing for the panel: Community Pulse = **signal ingestion → interest model →
scored surfacing under an attention budget**. Chat and voice are just lenses
over that engine (Case #1's Spark is the same engine pointed at a single
user — worth one slide).

## Implementation status (2026-07-22)

The implementation follows this non-blocking order: **core chat demo →
expand-card → polish → mobile frame → MCP → voice**. Milestones after the core
add demo beats; none is required for the 90-second path.

### Done

- [x] Core engine, Slint shell, streaming chat/replay, and the shared four-tool
  bridge remain the standalone 90-second demo.
- [x] Expand-card uses stable evidence ids, one-card toggle/close semantics,
  content-driven animated height, Escape-to-close, and bridge tests.
- [x] Desktop design polish: spec palette and fonts, fader/mute mixer, true
  line sparklines, unique headlines, honest score formula, dynamic prompts and
  chat height, tool errors, atomic chat updates, and ingest timeout behavior.
- [x] `app --mobile` opens a frameless, DPI-aware 418×872 logical portrait phone
frame around a 390×844 app surface with live clock, camera pill, battery, home
indicator, and bezel. The bezel control or Ctrl+R rotates live to 872×418:
bottom tabs become a left rail and an open evidence bottom sheet becomes a
right panel without losing evidence or streaming-agent state.
`app --companion` opens it beside desktop.
- [x] Digest headlines and evidence post rows expose their engine URLs and open
the original community source through the system browser on desktop and mobile.
- [x] Previous-snapshot delta chips, tracked-topic alert, and shared desktop /
mobile / MCP state are complete.
- [x] The attention budget is user-owned and persisted (default 5, ceiling 10).
Desktop and mobile expose it as a distinct 3–10 master fader; replay chat and
MCP update the same setting through `set_interests { attention_budget }`.
Dynamic count/budget status plus persistence and 50→10 clamp tests are complete.
- [x] `app --mcp-port <PORT>` exposes the shared nine-tool surface over an in-process,
stateless Streamable HTTP MCP endpoint. Protocol/unit tests and a real
desktop + mobile + HTTP shared-state smoke test pass.
- [x] **Agentic research interface R1–R4 (desktop-first)**: five research tools
bring MCP to nine total; reports persist and update live; the desktop Research
drawer renders markdown-lite, citations, web-report links, card badges, and the
Claude | Codex comparison; installed CLI delegation has doctor/run state and an
account-free fake-CLI end-to-end test. The desktop buttons were smoke-tested
concurrently through the live MCP endpoint.
- [x] **Research run indicator + elapsed clock**: each active Claude/Codex run
  now has an animated five-segment indicator, live elapsed clock, and latest
  harness activity in both the expanded evidence card and Research drawer.
  Completion/failure chips retain the final duration; failures expose a bounded
  error tail. Unit coverage exercises active, failed, and final-duration states.
- [x] **Research harness diagnostics + Claude headless permissions**: every run
  writes stdout, stderr, exit status, harness/model metadata, and outcome to
  `research/logs/`, with the path echoed into Agent chat. Claude defaults to Opus
  and pre-authorizes only `mcp__pulse__*` under `dontAsk`; this fixes the observed
  headless failure where Pulse MCP calls waited on an invisible permission prompt.
  The exact invocation and fake-CLI launch → MCP → submit → logged-exit lifecycle
  are tested; `make check` is green.
- [x] **In-app + live ingest**: the desktop header action, per-source statuses,
`app --live`, and recompute path share one `IngestController`; fixture mode is
protected, all triggers honor the 120-second floor, failures back off, delta /
tracked-alert state refreshes, and `pulse snapshot` creates a tested standalone
SQLite fallback.
- [x] README, demo script, and `make demo-mobile` document the additive beats.

### Needs review or is intentionally deferred

- [ ] Rehearse the real provider and an external MCP client on the presentation
  machine; automated tests and direct HTTP smoke coverage are complete.
- [ ] **Pre-stage the mid-call fallback workspace** (i3): dedicated
  workspace with demo video paused in mpv, per-beat screenshots in an
  arrow-key viewer ordered by demo beat, mockup HTML tabs open. Zoom
  shares the whole monitor so recovery = one workspace switch. In-call
  ladder: restart live app on fixture (~15s) → video → stills. Rehearse
  the switch once while actually screen-sharing; pre-decide the one calm
  transition sentence.
- [ ] Confirm expanded-card scroll positioning at the final Zoom window size;
  layout and close/toggle behavior are complete.
- [ ] Decide whether delta chips should persist a cross-session "last visit"
  snapshot. The demo currently compares consecutive engine snapshots.
- [ ] Regenerate backup video/screenshots after final presentation review.
  Fallback ladder: live → video (primary backup — state changes only read
  as real in motion) → per-beat screenshots → HTML mockups. Codex task: a
  `make demo-shots` target on the existing Xvfb harness (fixture data,
  one labeled PNG per demo beat into `demo/shots/`) so captures are
  regenerable after every UI change instead of going stale. Re-record
  video + shots once R1-lite lands (research report visible in both
  windows is the strongest still; include the Claude Code terminal in
  frame).
- [ ] Repair the GitHub Actions runner image: the latest `main` CI run fails
before compiling application code because the image lacks the system
`fontconfig` development package. Local `make check` is green.
- [ ] Recheck the frameless, DPI-scaled phone and source-link browser handoff on
the presentation desktop. Xvfb checks cover logical/HiDPI portrait-landscape
geometry, the no-decoration window hint, and rotation while evidence is open or
the Agent is streaming; launching an actual browser remains a human check.
- [ ] Run the attention-budget rehearsal checklist on the presentation desktop:
master 3→8→10 synchronization, replay-chat budget 8, restart persistence, MCP
50→10 without error, and evidence remaining open. Automated coverage and the
headless interaction rehearsal are complete.
- [ ] **Agent-terminal lifecycle bug (operator observed)**: spawned
  terminals accumulate across app instances — add pidfile reuse (no
  second spawn while one lives) + reap the terminal process group on app
  exit; `--agent-terminal` stays opt-in. In-app terminal emulation is
  explicitly rejected; the in-app path is the specced `--chat-agent`
  mode. Spec: research-interface-plan § Agent terminal lifecycle.
- [ ] **UI affordance punch (operator feedback)**: replace the "why?" /
  "close" text links with icon buttons — chevron-down expand button that
  rotates 180° + takes accent border when open, plus a ✕ in the evidence
  panel's top-right (spec: repo `docs/design/expand-card-plan.md`
  § Affordance update). And the research report view must render
  markdown-lite instead of plain text (spec: research-interface-plan
  § Punch).
- [ ] **Mobile research parity** remains intentionally deferred, including the
evidence-sheet report view, badges/live report acceptance, companion-window
parity, and bundled i3 floating-rule work.
- [ ] **R5 remote mobile client** remains optional and deferred; the in-process
companion demo is the supported boundary for this pass.
- [ ] Share-card PNG export, native Android/Waydroid packaging, and voice remain
optional follow-ons. They do not block any implemented demo beat.

## Interface Decision: chat-first, voice as stretch

- Case #2 never asks for voice; it asks for insight surfacing without
  flooding users, a scaling story, and "the simplest version you can show."
  Chat + visual composer answers that directly.
- Chat via the standard streaming API with tool calling is a fraction of the
  integration risk of the realtime audio loop (no cpal, no resampling, no
  VAD/barge-in, no Zoom audio routing).
- Both interfaces call the **identical tool bridge**, so voice is additive:
  if milestones land early, bolt the realtime session on; if not, the demo is
  complete without it. No fork, no rework.

Companion docs: **[`brief.md`](brief.md) — the interviewer-facing solution
brief (share THAT, not this file; this file is the internal build
tracker)**; visual/presentation version of the brief:
https://claude.ai/code/artifact/31315cca-6e8e-4318-8320-4bb5ff2c1967
(shareable — the private lens-map section is excluded); [`engagement-hooks.md`](engagement-hooks.md) (return/viral/
contribution loops + metrics; 3 demoable), [`mobile-demo-plan.md`](mobile-demo-plan.md)
(phone-frame first, Android stretch, scrcpy delivery),
[`live-agent-demo.md`](live-agent-demo.md) (call-day runbook: MCP
registration, optional /pulse-research skill, warm-up, failure modes).

## Demo Story (~2–3 minutes, over Zoom screen share; beats 1–4 are the 90-second core)

1. Type (or click a suggested prompt): "What's the pulse today?" → agent
   calls `get_pulse()` → 5 capped digest cards animate into the Slint UI,
   agent streams a 2-sentence summary in the chat panel.
2. Compose interests visually: drag/toggle topic chips (Rust, local-first,
   AI infra; mute crypto) → `set_interests(...)` → digest re-ranks visibly.
   Same operation also works by chat ("more Rust, less crypto") to show the
   agent and UI share state.
3. **Expand a card**: click "why?" → the card expands inline (animated) to
   the evidence panel — sparkline vs dashed 7-day baseline, z-score, source
   posts, rank formula. Then type "Why?" in chat → `explain_trend` expands
   the card with no pointer input (derived selection from the shared
   evidence cell — say this out loud). "close" collapses; opening another
   card auto-collapses the first. Click a source post → the original
   HN/lobste.rs thread opens in the browser ("we route you outward — no
   walled garden"). Spec: repo `docs/design/expand-card-plan.md`.
4. "Track WASM runtimes for me" → `subscribe_topic(...)` → tracked-topics
   list (the bridge slide to Spark/#1).
5. **Mobile view**: `pulse app --mobile` opens the phone-frame window
   (Pulse / Mix / Agent tabs, evidence bottom sheet, delta chips) beside
   the desktop window — change the mix in desktop chat, the phone's faders
   move. Same engine, same cells, second lens. Spec:
   [`mobile-demo-plan.md`](mobile-demo-plan.md) Phase 1.
6. **Agent mesh (if MCP built)**: from a Claude Code session, "check the
   community pulse and track wasm runtimes" → the visible window updates.
7. **Stretch:** same conversation, spoken — mic button starts a realtime V2V
   session against the same tools (see Appendix A/B).

## Architecture (demo)

```
Slint UI (digest cards, topic composer, chat panel, evidence view)
        │  lazily reactive cells (single source of state for UI + agent)
        ▼
   tool bridge ── get_pulse / set_interests / explain_trend / subscribe_topic
        ▲                              │
        │ tool calls                   ▼
  chat session (streaming HTTPS,   Pulse engine (in-process)
  provider-agnostic; OpenAI or         │
  Grok endpoint via config)        SQLite ◀── ingesters (HN Algolia,
        ▲                                     lobste.rs, Product Hunt RSS)
        │ (stretch) realtime WS session — same tools, spoken lens
```

- **No LiveKit for the demo** (even with voice: single client, direct WS is
  the entire media path). LiveKit appears only in the production diagram.
- **Provider-agnostic agent layer**: chat = OpenAI-compatible API (works
  against x.ai too); voice stretch = OpenAI Realtime protocol (Grok Voice
  Agent API is spec-compatible). Endpoints are config values.
- **State**: lazily reactive cells feed Slint models; agent tool results and
  direct UI manipulation write the same cells, so chat, voice, and clicks can
  never disagree (dogfooding — worth stating in the brief).

## Components

### 1. Pulse engine (pure Rust lib, no agent dependency)
- Ingesters: HN Algolia API (`hn.algolia.com/api/v1/search_by_date`),
  lobste.rs JSON, Product Hunt/launch RSS. Poll → normalize → SQLite.
- Topic extraction: tag + title keyword extraction (embeddings optional; keep
  demo deterministic).
- Trend scoring: per-topic mention velocity over sliding windows (1h/6h/24h),
  z-score vs 7-day baseline. Store score snapshots for sparklines.
- Interest model: weighted topic vector per user; final rank =
  `trend_score × interest_affinity`, digest capped at 5 (the "don't flood
  users" answer made concrete).
- Offline mode: `--fixture` flag loads a canned SQLite snapshot — demo works
  with zero network except the LLM API (and a `--replay` mode can canned-play
  even that if interview wifi dies).

### 2. Chat session
- Streaming completions over HTTPS (reqwest + SSE), tool calling enabled.
- Loop: user text → stream deltas into chat panel → on tool call, invoke
  bridge, append tool result, continue stream.
- Small, boring, reliable — that is the point.

### 3. Tool bridge (shared by chat, UI actions, and voice stretch)
- `get_pulse(limit?)` → ranked digest
- `set_interests(add[], remove[])`
- `explain_trend(id)` → evidence: velocity, baseline, top posts
- `subscribe_topic(topic)` → tracked-topic list (Spark bridge)
- Each call: engine query → write lazily cells (UI updates) → return compact
  JSON for the model to narrate. Direct UI actions (chip toggles, card
  clicks) call the same functions minus the narration.

### 4. Slint UI
- Design locked in the HTML mockup (`scratchpad/community-pulse-ui.html`,
  published: https://claude.ai/code/artifact/dabc2255-ac79-4165-821f-d2acaa06fe7d) —
  mixing-desk topic composer, capped digest with evidence panel, agent chat
  with inline tool-call chips. Tokens, CVD-validated topic palette, type
  (Instrument Sans + JetBrains Mono), metrics, and `.slint` port notes are in
  the mockup's spec section. The mockup doubles as the webapp fallback if
  Slint runs out of runway.
- Mobile design: https://claude.ai/code/artifact/fc4808cf-3774-48fc-9ea1-f4ffcd74c620
  (`scratchpad/community-pulse-mobile.html`) — Pulse / Mix / Agent bottom
  tabs, evidence bottom sheet, hold-to-talk mic on the Agent tab; inherits
  desktop tokens; spec deltas in the artifact.
- **Design parity punch list** (current Slint build vs mockup):
  1. Palette: swap teal/green accent + purple pills for the spec tokens —
     amber accent #EFA43F (dark) / #B97C1E (light), blue-charcoal grounds
     (#101419 / #1B212B surfaces). Amber ONLY where the pulse speaks
     (waveform, tool calls, budget meter).
  2. Fonts: Instrument Sans (UI) + JetBrains Mono (every number/label/
     readout/z-score, tabular figures, uppercase letter-spaced eyebrows).
  3. Interest mixer → channel strips: fader (track/fill/thumb) + weight
     readout + latching M mute, left border in the topic's categorical color
     (rust #C8822A, local #17A183, ai #8672E8, wasm #C75F81, steel #4C82C4 —
     dark variants; CVD-validated). Replace Boost+/mute× buttons.
  4. Digest cards: rank numeral, topic chip w/ color square, LINE sparkline
     (2px, 12% area fill, endpoint dot) right-aligned with z badge — not a
     bar chart. Fix duplicate story appearing under two topics (dedup by
     story id before ranking).
  5. Evidence: line sparkline + dashed 7-day baseline, mono stat readouts,
     rank formula line (`trend × interest = rank`).
  6. Chat: tool calls as amber left-border chips inline in the transcript
     (`⚙ get_pulse(limit: 5) · 5 items · 38ms`).
  7. Header: brand + animated pulse waveform + mono readouts
     (items/surfaced/ingest) + LIVE badge; demote Refresh (the pulse
     should feel live, not fetched).
  8. "Track"/"Why?" gray boxes → quiet underlined text actions.
- Digest column (5 cards: title, sources, trend arrow, score, "why?").
- **Topic composer**: interest chips — add/mute/weight by click or drag;
  suggested topics surfaced from the trend table.
- Chat panel with streaming responses + suggested prompts.
- Evidence panel (sparkline, posts) on explain.
- Reuse patchboard-app Slint techniques (hit-testing in Rust, zoom/drag if
  the evidence view wants it).

## Milestones (order = de-risking order)

1. **Engine + CLI digest — done.** Ingest HN → SQLite → trend scores →
`pulse top` prints capped digest. Proves the data story with zero UI/agent risk.
2. **Slint shell — done.** Digest/trending/composer rendered from engine, direct
manipulation working (no agent yet).
3. **Chat + tool bridge — done.** Streaming chat wired to the 4 tools; UI updates
on tool calls. Demo is *complete* at this milestone.
4. **Expand-card interaction — done.** Id-based derived selection, close/toggle
path in Rust, content-driven animated height. Spec + defect list: repo
`docs/design/expand-card-plan.md`. Small, but it is demo beat 3 —
treat as core, not polish.
5. **Polish + fallbacks — code done; presentation artifacts need review.**
Fixture/replay modes and suggested prompts are complete; refresh the backup
demo video and brief artifacts after final review.
6. **Mobile phone-frame view — Phase 1 done; native packaging deferred.**
`--mobile` tab shell reusing the
existing components + evidence bottom sheet + delta chips
([`mobile-demo-plan.md`](mobile-demo-plan.md) Phase 1). Android/Waydroid
build stays a stretch behind it (Phase 2).
7. **MCP endpoint — done; external-client rehearsal needs review.** In-process
HTTP, spec: repo `docs/design/mcp-integration-plan.md`. Unlocks demo beat 6.
8. **Stretch: voice lens — intentionally deferred.** Realtime WS session
(Appendix A) over the same tool bridge + Zoom routing (Appendix B). Only start
once 1–5 are demo-ready.

## Production Section (for the brief, not built)

- **Engine at 100k+ users**: trends are community-shared → precompute once per
  window (stream or cron); personalization = cheap per-user rerank over the
  top-N candidate set at read time (no per-user trend computation).
  Interest vectors in a KV; digest fan-out on read; SQLite → Postgres +
  columnar/stream processor for velocity windows.
- **Attention budget as a product invariant**: cap stays at ~5 regardless of
  scale; scores decay so stale trends fall out (the "doesn't go stale" and
  "doesn't overwhelm" answers).
- **Voice at scale: LiveKit as the media plane.** WebRTC edge for
  browser/mobile last-mile (WebRTC AEC/AGC is also what makes open-mic
  clients viable), SIP if needed, reconnection semantics; agent workers
  (LiveKit Agents; xAI ships an official plugin, OpenAI integrates) scale
  horizontally behind it. The demo's direct-WS voice client swaps under the
  same session/tool logic.
- **Mobile path**: Slint ships supported Android + iOS ports for Rust — the
  desktop codebase ports rather than rewrites.
- Cost note: voice minutes are the dominant per-user cost → voice is a lens,
  not the only surface; digest ships as UI/chat/notification first.

## Why Desktop (the panel-ready argument)

Web wins distribution for the community surface, and the viral loops
(share cards, cloneable mixes) want URLs — production is web-first. The
desktop app is the **power node**, justified by three things a browser tab
categorically cannot do:

1. **A web app can't be a server.** The ToolBridge's four tools exposed
   over an in-process MCP endpoint make every local coding agent (Claude
   Code, Codex, OpenCode, local-LLM runtimes) a client of the same shared
   state — an external agent calls `get_pulse`/`subscribe_topic` and the
   Slint UI updates live. "One engine, N lenses" where lenses include
   other vendors' agents. See `docs/design/mcp-integration-plan.md` in the
   repo (3–5h build). For TrendForge's developer audience this is
   on-theme: the users ARE people running these agents.
2. **Local context stays local.** Infer the interest mix from the user's
   actual repos (Cargo.toml/package.json/imports) without any of it
   leaving the machine — privacy-preserving personalization that is also
   better personalization. Slide, not demo.
3. **Ambient OS presence.** Tray pulse dot, real notifications for
   tracked-topic alerts, global hotkey for voice check-in — the habit
   surface behind engagement-hooks.md #1/#3/#4. A closed tab can't tap
   you on the shoulder.

Weak arguments to avoid: voice capture (browsers are fine), performance,
offline (service workers get most of it).

## Risks

- Interview-room network → fixture + replay modes, phone hotspot, recorded
  backup video.
- Slint chat-panel text layout (streaming text, wrapping) is the least-proven
  UI piece — build it in milestone 2, not last.
- Voice stretch: barge-in/truncation feel and Zoom routing are the two known
  hard parts (appendices); they are deliberately fenced off from the core
  demo so they can slip without damage.

---

## Appendix A — Voice lens (stretch)

- Direct WebSocket (tokio-tungstenite) to the OpenAI Realtime API; Grok Voice
  Agent API is spec-compatible (config swap). Auth via env API key.
- Events: `session.update` (voice, instructions, same 4 tools, server VAD),
  `input_audio_buffer.append` mic frames, audio deltas → playback ring
  buffer, `response.function_call_arguments.done` → tool bridge →
  `conversation.item.create` + `response.create`.
- Audio: cpal capture from the HyperX SoloCast (48kHz) → downmix mono →
  resample 24kHz PCM16 (rubato or linear). Playback via dedicated sink.
- Barge-in: on `input_audio_buffer.speech_started`, flush playback ring
  buffer + send truncate. Note: no WebRTC audio processing on this path (no
  AEC) — use headphones; PipeWire echo-cancel module if open speakers.
- Fallback: half-duplex push-to-talk (corky/whisper STT → chat API → TTS).

## Appendix B — Zoom audio routing (only needed for voice stretch)

Zoom transmits the microphone source; agent playback never reaches the panel
unless routed. Linux Zoom historically lacks "share computer sound", so build
a PipeWire virtual mix (idempotent `demo-audio-setup.sh`):

```bash
# sink the agent app plays into
pactl load-module module-null-sink sink_name=agent_out sink_properties=device.description=AgentOut
# mix bus Zoom uses as its "microphone"
pactl load-module module-null-sink sink_name=zoom_mix sink_properties=device.description=ZoomMix
# operator voice -> mix
pactl load-module module-loopback source=alsa_input.usb-HP__Inc_HyperX_SoloCast-00.HiFi__Mic__source sink=zoom_mix latency_msec=20
# agent voice -> mix
pactl load-module module-loopback source=agent_out.monitor sink=zoom_mix latency_msec=20
# agent voice -> headphones so the operator hears it
pactl load-module module-loopback source=agent_out.monitor sink=<headphone-sink> latency_msec=20
```

- Slint app playback device → `AgentOut`; Zoom mic → `ZoomMix` monitor;
  enable Zoom "Original sound" so noise suppression doesn't chew the TTS.
- App still captures the SoloCast directly (PipeWire multi-reader), so
  VAD/barge-in are unaffected by the routing.
- Default sink is currently Bluetooth: keep it A2DP playback-only (capture on
  SoloCast) and prefer wired headphones — BT adds 100–300ms that pads
  responses and skews barge-in truncation math.
- Rehearse the full chain with a second Zoom account before interview day.
