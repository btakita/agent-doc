# Community Pulse — Mobile Demo Plan

Goal: show the same Rust engine + Slint component library running as a
mobile app, matching `mockup-mobile.html` (Pulse / Mix / Agent tabs,
evidence bottom sheet), plus 1–2 engagement hooks from
`engagement-hooks.md` (delta chips, tracked-topic alert).

Strategy: **phone-frame first, Android second.** Every phase is
independently demoable, so the mobile story never blocks on the NDK.

## Implementation status (2026-07-22)

- [x] Phase 1: mobile-only and companion launch modes, shared bridge/models,
Pulse / Mix / Agent tabs, evidence sheet, delta chips, tracked alert, and the
synchronized 3–10 attention-budget master fader.
- [x] Phase 1b: true portrait phone chrome plus live portrait ⇄ landscape
rotation. Bottom tabs become a left rail, evidence becomes a right panel, and
the selected evidence or active Agent response survives rotation. Bezel and
Ctrl+R controls were exercised at the intended 418×872 / 872×418 logical frame
sizes. The outer window is frameless and follows the desktop's DPI scaling.
- [ ] Research-interface parity is intentionally deferred with the companion/i3
floating-rule pass; the current implementation target remains desktop ↔ agent.
- [ ] Phase 2 needs review and remains optional: native Android packaging has
not been started.
- [ ] Phase 3 needs review: rehearse the final phone-frame/scrcpy delivery and
refresh fallback capture assets on the presentation machine.

## Phase 1 — Mobile shell in a phone-frame window (desktop, ~1 day)

No Android toolchain. Add to community-pulse:

- `ui/mobile.slint`: `MobileWindow` 390×740, reusing the existing
  components (DigestCard, TopicChannel, ChatMessage, EvidencePanel) inside
  a tab switcher: bottom bar Pulse / Mix / Agent per the mockup (active
  tab in accent, mono uppercase labels).
- Evidence becomes a bottom sheet on mobile: overlay Rectangle + scrim at
  the Pulse tab root, driven by the same evidence cell (selection logic
  from expand-card-plan.md carries over unchanged — derived state, no
  local flags).
- `pulse app --mobile` flag chooses the window. Same ToolBridge, same
  callbacks — this is the "one engine, N lenses" slide made literal.
- Touch sizing per mockup spec: strips ≥56px, fader thumb 21px with ≥44pt
  hit slop, mutes 30px.
- Engagement hooks: delta chip row at the top of Pulse ("2 new · rust
  cooled") from a stored last-seen digest; tracked-topic alert banner
  (fires when a subscribed topic's z crosses threshold — trigger via
  fixture).

Demo value: side-by-side desktop + phone-frame windows on the same state.
Change the mix in the desktop chat → the phone's Mix tab faders move.
That's a stronger interview moment than an emulator.

## Phase 1b — Phone chrome + orientation (half day, polish on Phase 1)

- **Device framing**: rounded bezel around the app surface, status bar
  (live clock via chrono, battery glyph, camera pill), home-indicator bar —
  per `mockup-mobile.html`'s frame styling. The window stops looking like
  "a small desktop window" and reads as a phone on screen share.
- **True aspect**: portrait 390×844 logical (19.5:9, iPhone-14 class;
  replaces the initial 390×740).
- **Rotate**: bezel button + keyboard shortcut toggles portrait ⇄ landscape
  (844×390). Landscape relayout: tab bar → left rail, evidence bottom
  sheet → right-side panel. Demo beat: rotate mid-demo — responsive
  relayout from the same components, no second codepath.
- Spec for the implementing agent: repo `docs/design/IMPLEMENTATION.md`
  § Phone chrome + orientation.

## Phase 2 — Android build (1–2 days, stretch)

- Toolchain: Android NDK + `cargo-apk` (or `xbuild`); Slint feature
  `backend-android-activity`. Targets: `x86_64-linux-android` for
  emulator/Waydroid, `aarch64-linux-android` for a real device.
- App plumbing: SQLite path → app data dir (`android-activity` provides
  it); bundle fixture; reqwest already on rustls (no openssl cross pain);
  fonts already bundled via `.slint` import so typography survives.
- Chat: reuse `--replay` deterministic agent on-device (no key management
  on the phone for the demo).
- Run on CachyOS: **Waydroid** (Android container, native perf) or the
  Android Studio AVD. Waydroid is lighter and demos on your own screen.

## Phase 3 — Demo-day delivery

- Best: real phone + `scrcpy` mirror → the phone appears in a desktop
  window → Zoom screen share carries it with zero extra setup. Hold the
  phone up to the camera for one beat, then drive it mirrored.
- Fallbacks, in order: Waydroid window → Phase-1 phone-frame window →
  `mockup-mobile.html` walkthrough. Every rung uses the same visual
  design, so the story survives any rung.

## Risks

- `android-activity` + winit event-loop quirks in Slint's Android backend:
  timebox to half a day; if it fights back, Phase 1 IS the mobile demo
  (say "the Android port is a supported Slint target; here's the same
  binary shape in a phone frame").
- Waydroid needs a kernel module (binder); CachyOS ships it, but verify
  before demo day, not on it.
- Keep Phase 2 off the critical path: desktop demo + brief are done first.

## Brief tie-in

One slide: "Mobile is a lens, not a fork" — engine crate unchanged, same
.slint components, tab shell + bottom sheet swapped in; Slint ships
supported Android/iOS ports for Rust; daily-pulse notification +
voice check-in are the mobile-native engagement hooks (engagement-hooks.md
#1, #4).
