# Plan — harden component parsing against UTF-8 byte-boundary panics

**Pending:** `#utf8p`
**Status:** Draft

## Problem

The second failure pasted into `tasks/agent-doc/agent-doc-bugs2.md` is a concrete parser bug, not a tmux/recovery ambiguity:

```text
thread 'main' panicked at src/component.rs:443:24:
byte index 12303 is not a char boundary; it is inside '❯'
```

At `src/agent-doc/src/component.rs:443`, the parser is iterating byte offsets and then slicing `&str` with:

- `peek_start = pos + 4`
- `&doc[peek_start..doc.len().min(peek_start + 20)]`

That upper bound can land inside a multibyte UTF-8 code point such as `❯`, which turns malformed user-visible content into a hard panic in the route path.

## Desired Behavior

1. Component parsing must never panic on valid UTF-8 document content.
2. Prompt glyphs such as `❯`, emojis, and other multibyte characters near agent comment markers must be treated as ordinary text unless they are part of a real marker.
3. Parsing failures should become regular `Result` errors only for actual malformed agent component structure, not for unrelated Unicode proximity.

## Implementation Steps

### Fix the immediate panic site

- [ ] Replace the current `peek` slicing logic in `component.rs` with a byte-safe or char-boundary-safe helper.
- [ ] Preserve the fast-path behavior that ignores non-agent HTML comments without consuming real component close markers.

### Audit nearby byte/str boundary assumptions

- [ ] Review the rest of `component.rs` for `usize` byte offsets that later become direct `&str` slices.
- [ ] Audit closely related parser helpers used in route/write/template flows for the same pattern.
- [ ] Prefer one shared helper for "bounded preview from byte offset" rather than repeating ad hoc slicing math.

### Add regressions

- [ ] Regression: a document with `❯` immediately before or near an `<!-- agent:... -->` comment parses without panic.
- [ ] Regression: non-agent comments in prose near multibyte characters still skip cleanly.
- [ ] Regression: real malformed component nesting still returns a structured error rather than being masked.

### Verification

- [ ] Run the relevant `component.rs` / parser test slices plus full `cargo test` in `src/agent-doc`.
- [ ] Re-run the original routed repro that previously panicked at `component.rs:443` and confirm it now fails, succeeds, or routes normally without a Rust panic.

## Files Likely To Change

- `src/agent-doc/src/component.rs`
- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/specs/07-commands.md` if parser guarantees are documented there

## Exit Criteria

- Valid UTF-8 content can no longer crash `agent-doc` component parsing through byte-boundary slicing.
- The specific `❯` panic repro from `agent-doc-bugs2.md` is covered by a regression test.
