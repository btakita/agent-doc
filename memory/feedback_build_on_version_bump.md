---
name: Build artifacts on every version bump
description: When bumping any plugin/binary version, build the artifact in the same cycle — never leave "build pending"
type: feedback
originSessionId: 24ab40e9-0483-4e0e-9ee4-842c91a5d8ae
---
When bumping a version (JB plugin `gradle.properties::pluginVersion`, VS Code `package.json::version`, or `agent-doc` `Cargo.toml`), build the artifact in the same cycle. Do not report "version bumped, build pending" as a stopping point.

**Why:** A bumped version without a built zip/VSIX/binary makes local testing impossible — the user can't install what doesn't exist, so the manual testing gate can't be entered. User called this out on 2026-04-18 after a cycle bumped JB 0.2.58 + VSIX 0.2.6 but left both "pending" right before asking the user to test.

**How to apply:**
- Bump `editors/jetbrains/gradle.properties::pluginVersion` → run `./gradlew buildPlugin signPlugin` in the same turn; verify `build/distributions/agent-doc-jetbrains-<version>.zip` exists.
- Bump `editors/vscode/package.json::version` → run `npm run compile && npm run package` in the same turn; verify `agent-doc-<version>.vsix` exists.
- Bump core `agent-doc` `Cargo.toml` → run `cargo build --release && cargo install --path .` in the same turn.
- If a build fails, fix it or revert the bump — never ship a bumped-but-unbuilt state.
- Codified in `.claude/skills/agent-doc/runbooks/plugin-install.md` § Build on every bump.
