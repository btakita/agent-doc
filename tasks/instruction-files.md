---
agent_doc_session: cc8cf497-7431-42cc-90b4-17268064e428
tmux_session: claude
---

Convert instruction-files to Rust 2024 edition

## Assistant

**Ready for direct upgrade.** 1,364 lines across 4 source files — zero `unsafe`, no `gen` keyword, no `impl Trait` lifetime issues, no problematic pattern matching. All 47 tests pass. No dependency blockers.

Migration: change `edition = "2021"` → `"2024"` in Cargo.toml, run `cargo check` + `cargo test`. Proceeding now.

**Done.** `edition = "2024"` in Cargo.toml. `cargo check` clean, all 47 tests pass. Zero code changes needed.

## User

version bump and release
