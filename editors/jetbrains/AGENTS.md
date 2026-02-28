# agent-doc JetBrains Plugin

## Build

Always build **both** unsigned and signed plugin zips:

```bash
cd agent-doc/editors/jetbrains
./gradlew buildPlugin signPlugin
```

Output:
- `build/distributions/agent-doc-jetbrains-0.1.0.zip` (unsigned)
- `build/distributions/agent-doc-jetbrains-0.1.0-signed.zip` (signed)

## Install

IDEA → Settings → Plugins → gear icon → "Install Plugin from Disk..." → select the zip.

If classes changed structurally (new imports, methods, fields): **uninstall first → restart → install → restart**. Reinstalling over an existing plugin may not replace cached bytecode.

## Logging

Uses `com.intellij.openapi.diagnostic.Logger`. No temp files.

Enable debug output: IDEA → `Help > Diagnostic Tools > Debug Log Settings` → add `#com.github.btakita.agentdoc`. Output appears in `idea.log`.

## Conventions

- Plugin is a thin wrapper — business logic lives in the `agent-doc` CLI.
- All CLI calls run from the project root directory.
- Success feedback uses inline hints (`HintManager`), not balloon notifications.
- Error feedback uses persistent balloon notifications.
- `plugin.xml` action IDs are stable — only change `text` attributes for renames.
