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
- Submit/route uses inline hints (`HintManager`) for progress and success instead of an in-flight information balloon.
- Error feedback uses persistent balloon notifications.
- `plugin.xml` action IDs are stable — only change `text` attributes for renames.
