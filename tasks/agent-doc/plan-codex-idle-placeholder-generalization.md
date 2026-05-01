# Plan: Codex Idle Placeholder Generalization

## Problem

`route` currently recognizes Codex idle-composer suggestion lines through an exact string allowlist in `src/agent-doc/src/harness.rs`. When Codex rotates that placeholder text, an otherwise idle pane can be misclassified as busy and routed `agent-doc <FILE>` triggers fail closed.

## Goal

Replace the brittle exact-match placeholder detection with a more durable readiness signal that still rejects real drafted user input.

## Steps

1. Audit recent Codex pane captures and existing route regressions to catalog placeholder-line shapes that represent an idle composer.
2. Design a classifier that uses stronger structure than raw string equality, such as placeholder-position rules plus surrounding footer/chrome signals, without accepting arbitrary drafted text.
3. Add harness and route regressions that prove the generalized classifier accepts future placeholder variants while still rejecting real drafted prompts and queued/busy panes.
