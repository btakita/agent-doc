---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
tmux_session: '1'
---


## Session History

### v0.12.1 — Email attachments ([PR #17](https://github.com/btakita/corky/pull/17))

- `--attach` flag on `corky draft new`, `attachments:` YAML frontmatter field
- Sends as `multipart/mixed` with `mime_guess` for content-type detection
- Files validated at send time, not draft time
- Future phases: inline image embedding (Phase 2), clipboard attachment (Phase 3)

### v0.12.2 — `corky social` → `corky linkedin` ([PR #18](https://github.com/btakita/corky/pull/18))

- BREAKING CHANGE: all social commands scoped under platform-specific subcommand
- Convention: each platform gets its own top-level subcommand (`corky linkedin`, `corky bluesky`)
- Enforced by SPEC.md § 12.7, not code abstractions — complexity grows only when a second platform arrives
- Internal `src/social/` module unchanged, still platform-aware

### Cleanup: `bin/build-mail-filters` deleted

- Superseded by `corky filter build` and `corky filter push`

### Decision: Gmail filters stay manual-only

- `corky filter push` is destructive (deletes all, recreates). No Gmail undo.
- Manual `corky filter push` is the safety feature.

### v0.12.3 — Filter drift detection ([PR](https://github.com/btakita/corky/releases/tag/v0.12.3))

- `corky filter check` — read-only comparison of local `.corky.toml` vs live Gmail filters
- `corky watch` runs hourly drift check, warns on mismatch, never auto-pushes
- 120 lines in `filter/check.rs`, 4 tests

### v0.12.4 — Rust 2024 edition ([PR #19](https://github.com/btakita/corky/pull/19))

- `set_var`/`remove_var` → `unsafe {}`, removed explicit `ref mut` patterns
- Added `collapsible_if = "allow"` clippy lint (48 instances across 21 files — future cleanup)
- instruction-files bumped to v0.1.4
- `imap-proto` macro issue is future-compat warning only (deps use their own edition)

## Assistant

**Document compacted.** 384 lines → ~45 lines. Preserved all version releases, key decisions (manual filters, social platform convention), and open follow-ups (attachment phases 2-3, collapsible_if refactor).

## User

Use the corky logo in all of the docs. Such as the readme, mdbook, etc.

## Assistant

Added the corky logo (SVG, 128px) to:

- **README.md** — centered `<img>` above the `# Corky` heading
- **docs/introduction.md** — same centered logo at the top of the mdbook landing page
- **docs/assets → ../assets** — symlink so mdbook can serve the logo from the existing `assets/` directory

The logo assets already existed (`assets/logo.svg`, `assets/logo-64.png`, `assets/logo-128.png`) — just weren't referenced anywhere.

## User

commit. Since this is a doc only change, can we re-release the same version or do we need a new release?

## Assistant

No new release needed. v0.12.4 was already published to crates.io, PyPI, and GitHub Releases — can't move the tag. But since this is purely cosmetic (README + mdbook logo), the GitHub repo shows the logo on `main` immediately. crates.io/PyPI will pick it up on the next functional release.

Committed and pushed to corky `main`: `docs: add corky logo to README and mdbook introduction` (78a79b5).

## User

tasks/judith-krug.md just implemented pdf and docx generation support. Please verify.

## Assistant

Verified. `corky doc build` is fully functional.

**Build:** compiles clean on current `main`.

**PDF test:** `corky doc build /tmp/test.md` → 5.9 KB PDF via pandoc + weasyprint. CSS template system works (reads `template:` from YAML frontmatter, loads `{data_dir}/templates/{name}.css`). Minor weasyprint warnings on responsive CSS properties — cosmetic only.

**DOCX test:** `corky doc build --format docx /tmp/test.md` → 10.5 KB DOCX via pandoc native. Clean output, no warnings.

**Implementation:** `src/doc/build.rs` handles both pipelines. CLI wired in `src/cli.rs` (DocCommands enum). `corky doc upload` is stashed with a "coming soon" message — correct per the judith-krug session scope.

**Gaps to address before release:**
- SPEC.md needs a `doc build` section (no § exists yet)
- VERSIONS.md needs an entry for this feature
- weasyprint CSS warnings could be cleaned up in the proposal template

**Dependencies:** pandoc 3.5 and weasyprint 68.1 — both already installed. No new Rust crate dependencies; the feature shells out to external tools.

## User

Fill in the gaps
Are there crates for pandoc and weasyprint. If not, are there equivalents crates? Or should we shell out the commands?

## Assistant

**Gaps filled:**
- **SPEC.md § 14** "Document Building" added — subsections 14.1–14.6 with CLI interface, format pipelines, template resolution, tool checking, and edge case table (D1–D9)
- **VERSIONS.md v0.13.0** entry added for `doc build`
- **CSS warnings** — not a corky issue. The template CSS is user-provided (`mail/templates/proposal.css`), so those weasyprint warnings are in your stylesheet, not corky's code

**Rust crate alternatives vs shelling out:**

| Tool | Crate Alternative | Viability |
|------|-------------------|-----------|
| pandoc | `pulldown-cmark`, `comrak` (md→HTML) | Good for HTML. No native DOCX. |
| weasyprint | `printpdf`, `genpdf`, `typst` | Low-level PDF. No CSS support. |
| DOCX | `docx-rs` | Builds DOCX programmatically — no markdown parsing. |

**Recommendation: keep shelling out.** Reasons:
1. pandoc is battle-tested for md→HTML/DOCX with tables, footnotes, frontmatter. Replacing with `pulldown-cmark` + `docx-rs` = 500+ lines for a worse result.
2. weasyprint handles CSS properly — Rust PDF crates don't support CSS (you'd lose the template system).
3. Zero maintenance — system packages update independently.
4. Current code is 152 lines — native crates would be 1000+ lines and more fragile.

Only reason to switch: single-binary distribution without system deps. In that case, `typst` is the best option but uses its own markup, not CSS.

## User

How do we support install external dependencies such as pandoc and weasyprint? Is there at least a friendly error message of the executables are missing from PATH?
