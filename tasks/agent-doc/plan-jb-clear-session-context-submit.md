# JB Clear Session Context Codex Submit Plan

## Problem

`JB Clear Session Context` inserted `/clear` into a Codex input for
`tasks/monsterrodholders.md`, but the command was not submitted.

## Hypothesis

This is the same route-submit class as `Run Agent Doc` prompt visibility:
the direct-pane send path can place text into a Codex composer without an
accepted Enter/submit proof. The clear-session command needs the same
submit detection and one-shot Enter retry contract as routed agent-doc
triggers.

## Implementation

1. Locate the JetBrains clear-session route that sends `/clear` to Codex.
2. Reuse or generalize the direct-pane submit proof helper so `/clear` emits
   route-submit diagnostics and retries Enter exactly once when the command
   remains visible.
3. Scope the retry to prompt-visible Codex/Claude composers; avoid sending a
   stray Enter into an idle pane.
4. Add SimWorld or unit coverage for `/clear` visible-but-not-submitted,
   submitted, and stale-scrollback cases.

## Acceptance

- Running JB `Clear Session Context` against a Codex pane submits `/clear`
  without operator Enter.
- Logs include a proof line for initial submit issue and retry outcome.
- Stale scrollback containing `/clear` is not mistaken for active composer
  text.
- Focused tests plus `make check` pass before install/push.
