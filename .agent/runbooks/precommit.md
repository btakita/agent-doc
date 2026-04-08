# Precommit

Steps to run before committing code changes.

## Checklist

1. **Run tests and linting**
   - Rust: `make check` (clippy + test)
   - JS/TS: `bun test` or `npm test`
   - Python: `pytest`

2. **Add tests for new behavior** (MANDATORY — not optional, not deferred)
   - New functions/methods need unit tests covering the happy path
   - Bug fixes need regression tests proving the bug is fixed
   - Edge cases identified during implementation need coverage
   - **Negative tests**: for every classifier/discriminator, test inputs that should NOT match
   - **Boundary tests**: empty inputs, single-element inputs, maximum-length inputs
   - **Integration tests**: if the feature adds a field to JSON output, test serialization (present when set, omitted when None)
   - **Moved code**: if a function was moved between modules, add a direct test in the new location
   - Do NOT defer testing to a later cycle — untested code accumulates risk silently

3. **Audit instruction files** (if changed)
   - `module-harness audit` (validates CLAUDE.md, AGENTS.md, SPEC.md consistency)
   - Verify CLAUDE.md reflects any architectural changes

4. **Update ontology** (if new terms or concepts introduced)
   - Add/update terms in existence-lang (`~/.claude/philosophy/src/`)
   - Update project domain ontology in README.md (e.g., Interaction Model, Domain Ontology sections)
   - Ensure module-harness ontology references are project-local (no `~/` paths in released files)
   - Follow the [ontology-references runbook](../.claude/skills/module-harness/runbooks/ontology-references.md)

5. **Review diff**
   - No secrets, credentials, or API keys in the diff
   - No debug/temporary code left behind
   - No unrelated changes bundled in
