# Plugin Install Runbook

## When to bump the plugin version

Bump `pluginVersion` in `gradle.properties` whenever you make any code change to the plugin — even behavior-only patches. This ensures:
- The new build gets a distinct zip filename, preventing stale cached zips
- The user can verify which version is installed via `agent-doc plugin list`
- Install instructions can reference a specific version number

**Rule:** Any PR or committed change to `editors/jetbrains/` → bump `pluginVersion`.

```bash
# In src/agent-doc/editors/jetbrains/gradle.properties
pluginVersion = 0.2.47  # increment patch version
```

## Before suggesting a plugin install

Always check the currently installed plugin version before telling the user to install:

```bash
agent-doc plugin list
```

This shows the installed version for each editor. Compare against the version you just built.

- If installed version == built version: **do not say "install required"** — it's already current.
- If installed version < built version: tell the user to reinstall with the specific zip path.
- If plugin is not installed: tell the user to install.

## Install steps (JetBrains)

When a new install IS needed:

1. IDEA → Settings → Plugins → find "Agent Doc" → Uninstall → Restart IDE
2. Settings → Plugins → gear icon → "Install Plugin from Disk..."
3. Select the built zip from `src/agent-doc/editors/jetbrains/build/distributions/`
4. Restart IDE

Use full uninstall+restart only when class structure changed (new fields/methods). For behavior-only patches, reinstalling over the existing version is sufficient.

## Signing

For distribution, build both signed and unsigned:

```bash
cd src/agent-doc/editors/jetbrains
./gradlew buildPlugin signPlugin
```
