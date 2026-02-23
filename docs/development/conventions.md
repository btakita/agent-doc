# Conventions

## Code style

- Use `clap` derive for CLI argument parsing
- Use `serde` derive for all data types
- Use `serde_yaml` for frontmatter parsing
- Use `similar` crate for diffing (pure Rust, no shell `diff` dependency)
- Use `serde_json` for agent response parsing
- Use `std::process::Command` for git operations (not `git2`)
- Use `toml + serde` for config file parsing
- No async — sequential per-submit
- Use `anyhow` for application errors

## Instruction files

- `CLAUDE.md` is the primary instruction file
- Personal overrides: `CLAUDE.local.md` (gitignored)
- **Actionable over informational.** Instruction files contain the minimum needed to generate correct code. Reference material belongs in `README.md`.
- **Update with the code.** When a change affects patterns, conventions, or module boundaries, update instruction files as part of the same change.

## Version management

- Never bump versions automatically — the user will bump versions explicitly.
- Commits that include a version change should include the version number in the commit message.
- Use `BREAKING CHANGE:` prefix in VERSIONS.md entries for incompatible changes.
- Update `SPECS.md` when agent-doc functionality changes (commands, formats, algorithms).

## Workflow

Follow a research, plan, implement cycle:

1. **Research** — Read the relevant code deeply.
2. **Plan** — Write a detailed implementation plan.
3. **Implement** — Execute the plan. Run `make check` continuously.
4. **Precommit** — Run `make precommit` before committing.
