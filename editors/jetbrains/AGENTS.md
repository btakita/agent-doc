# agent-doc JetBrains Plugin

## Build

Current version lives in `gradle.properties` as `pluginVersion = <x.y.z>`. **Never hardcode the version in docs or scripts** — read it from `gradle.properties` or glob the zip filename.

To bump the patch version and build both zips in one shot:

```bash
# from src/agent-doc
make bump-plugin
```

Or manually:

```bash
cd agent-doc/editors/jetbrains
./gradlew buildPlugin signPlugin
```

Output (where `<version>` comes from `gradle.properties`):
- `build/distributions/agent-doc-jetbrains-<version>.zip` (unsigned)
- `build/distributions/agent-doc-jetbrains-<version>-signed.zip` (signed)

Reference zips via glob: `build/distributions/agent-doc-jetbrains-*-signed.zip`.

## Install

IDEA → Settings → Plugins → gear icon → "Install Plugin from Disk..." → select the zip.

If classes changed structurally (new imports, methods, fields): **uninstall first → restart → install → restart**. Reinstalling over an existing plugin may not replace cached bytecode.

## Logging

Uses `com.intellij.openapi.diagnostic.Logger`. No temp files.

Enable debug output: IDEA → `Help > Diagnostic Tools > Debug Log Settings` → add `#com.github.btakita.agentdoc`. Output appears in `idea.log`.

## Conventions

- Plugin is a thin wrapper — business logic lives in the `agent-doc` CLI.
- All CLI calls run from the project root directory.
- Automatic startup/layout sync paths stay thin: they use report-only `agent-doc resync`, and `agent-doc sync` receives only layout/focus file paths while the CLI owns autostart, ambiguity handling, and tmux targeting.
- Submit/route waits for the markdown typing debounce before saving and dispatching, and stays silent on progress and success.
- Repeating `Run Agent Doc` should supersede any stale plugin-spawned route process and dispatch again immediately.
- Error feedback is routed to the IDE Event Log / notification tool window instead of bottom-right balloon popups.
- `plugin.xml` action IDs are stable — only change `text` attributes for renames.
- **No prompt poller:** do not reintroduce the defensive `PromptPoller` / `PromptPanel` path. JetBrains must not poll `agent-doc prompt --all`, auto-save tracked documents, refresh tracked files, or merge/reload editor buffers from a prompt UI timer.
