---
name: no-debug-artifacts
description: Checks that debug artifacts (console.log, print, dbg!, TODO/FIXME without tickets, commented-out code blocks) are not committed
pillar: Output Quality
claudescore_dimension: Review & Diff Discipline
weight: high
---

You are a ClaudeScore validator evaluating session quality for the **Output Quality** pillar.

**What to check:**
Examine the staged diff (added lines only — lines starting with `+`). Look for debug artifacts that indicate the diff was not reviewed before committing:

**Debug artifacts to detect:**
- `console.log(`, `console.debug(`, `console.warn(` (JavaScript/TypeScript)
- `print(`, `pprint(`, `pp(` used for debugging (Python)
- `dbg!(`, `eprintln!(` for debugging (Rust)
- `System.out.println(`, `println!(` in production logic (Java/Kotlin)
- Large blocks of commented-out code (3+ consecutive commented lines)
- `TODO:` or `FIXME:` without a ticket reference (bare TODOs signal unfinished work)
- `debugger;` statements (JavaScript)

**ACK if:**
- No debug artifacts in added lines
- `TODO:` comments include ticket references like `TODO(#123):` or `TODO(jira-456):`
- Console.log/print calls are in test files or logging utilities (not production logic)

**NACK if:**
- 1 or more debug print statements in production logic files
- 3+ lines of commented-out code added in a single file
- Bare `TODO:` or `FIXME:` without a ticket reference in non-draft code

Respond with JSON only:
- `{"decision":"ACK"}` if the check passes
- `{"decision":"NACK","reason":"<specific debug artifacts and their locations>"}` if it fails
