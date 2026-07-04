---
name: JB plugin version bump per source edit
description: Bump JB plugin patch version on every source edit — reinstalling same-version zip doesn't replace cached bytecode in IntelliJ
type: feedback
originSessionId: c607e39a-2d26-486e-9929-c79486ce3727
---
**Rule:** Bump `pluginVersion` in `src/agent-doc/editors/jetbrains/gradle.properties` on every `.kt` source edit before rebuilding, even if no user-visible behavior changes.

**Why:** IntelliJ's "Install Plugin from Disk..." deduplicates by version string. Installing `agent-doc-jetbrains-0.2.60-signed.zip` over an already-installed 0.2.60 is a no-op — the cached bytecode stays. The user observed: "I reinstalled the JB plugin 0.2.60...Note the version did not change. Why does this keep on failing?" Root cause: same-version zip reinstall silently did nothing. New rule forces cache invalidation.

**How to apply:** Same cycle as source edit:
1. Edit `.kt`
2. Bump `pluginVersion = X.Y.Z` → `X.Y.(Z+1)`
3. `./gradlew buildPlugin signPlugin` inside `editors/jetbrains/`
4. Tell user to uninstall → restart → install new zip → restart (structural bytecode refresh)

Corollary: if I say "plugin is current at version X" but user's IDE still shows old behavior, the first hypothesis is bytecode cache, not stale source. Fix by bumping version and asking for full uninstall/reinstall cycle, not re-checking code.
