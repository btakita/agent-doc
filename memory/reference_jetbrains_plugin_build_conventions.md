---
name: reference_jetbrains_plugin_build_conventions
description: "JetBrains plugin build/packaging conventions — ActionPromoter breaks .md intentions, verifier false positives on bare IU, agent-doc plugin toolchain pins, and the never-bundle-platform-transitives rule"
metadata: 
  node_type: memory
  type: reference
  originSessionId: c54d12a1-95e4-49a0-b66d-0178f0a3a3d8
  modified: 2026-07-25T18:08:09.588Z
---

**ActionPromoter breaks intentions in `.md` files.** The fix is a self-disabling
`update()`. The python-intentions verifier reports false positives because a bare IU
distribution lacks PythonCore. Build both zips; a full uninstall is only required for
structural class changes, not ordinary edits.

**agent-doc JB plugin toolchain:** K2 / JVM 21 / IntelliJ 2024.2 (`sinceBuild 242`).
lazily-kt is wired through a conditional Gradle composite build.

**NEVER bundle platform transitives.** Exclude `org.jetbrains.kotlinx` — bundling it
breaks the IDE dispatcher — along with kotlin, annotations, and jna. Verify what
actually shipped with `unzip -l` on the built zip before publishing.

Related: [[feedback_jb_plugin_version_bump_per_edit]], [[feedback_build_on_version_bump.md]],
[[feedback_jb_plugin_cdylib_hot_reload]].
