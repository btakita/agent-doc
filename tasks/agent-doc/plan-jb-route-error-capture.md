# Plan: JetBrains Route Error Capture

**Pending:** `#jberr` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

`agent-doc route` already fails closed with the important startup-miss / pending-drift reason text, but the JetBrains `Run Agent Doc` surface can still make that message ephemeral. In the latest `claudescore-3.md` repro, the plugin surfaced an error briefly enough that the user could not copy it before it disappeared.

That leaves the underlying route bug diagnosable from logs, but not reliably from the IDE surface that triggered it.

## Goal

Keep routed `agent-doc` failures copyable and inspectable from JetBrains long enough that users can retain the exact binary-owned error without racing a transient notification.

## Desired Behavior

- a routed failure still returns the original binary-owned error text unchanged
- JetBrains keeps that text in a persistent surface long enough to inspect or copy
- the plugin exposes a deterministic follow-up path such as:
  - copy error details
  - open recent route diagnostics
  - point to the relevant session/log file when appropriate

## Implementation Steps

- [ ] Audit the current JetBrains `Run Agent Doc` failure notification path and determine which route stderr/stdout surfaces are transient only
- [ ] Decide the persistence mechanism for the error text:
  - sticky notification with copy action
  - dedicated tool window/log panel entry
  - persisted diagnostics file with direct open/copy actions
- [ ] Preserve the original route error string exactly so plugin wrappers do not paraphrase away startup-miss timestamps or pending-drift markers
- [ ] Add plugin-side coverage for a routed failure whose message must remain available after the first toast/hint expires
- [ ] Update the relevant spec/release notes if the JetBrains error-surface contract changes

## Files Likely To Change

- `src/agent-doc/editors/jetbrains/src/main/kotlin/...`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- a failed `Run Agent Doc` route in JetBrains leaves the exact failure text available to copy after the initial notification moment
- users no longer need to race the toast lifetime to capture the binary-owned error
