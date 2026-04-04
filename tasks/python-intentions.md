---
agent_doc_session: 29cf077b-d739-48d8-91df-85be1f5a1329
tmux_session: '1'
---

## Status Summary (compacted 2026-03-05)

### Problem

JetBrains Marketplace verification shows errors for [Python Intentions](https://plugins.jetbrains.com/plugin/30074-python-intentions) plugin (v0.5.0) — all false positives from testing against bare IntelliJ IDEA Ultimate (IU-*) which lacks PythonCore.

- 1 missing mandatory dependency: `com.intellij.modules.python`
- 2 "Package not found": `kotlin` (8 classes) and `com.intellij`
- Affects 6 IU builds: 242, 243, 251, 252, 253, 261

### Resolution

- **Plugin correctly configured** — `<depends>com.intellij.modules.python</depends>` in `plugin.xml` prevents installation in IDEs without Python plugin
- **Local verification passes** — `pluginVerification` block added to `build.gradle.kts` targeting PyCharm Community; all 7 builds (PC-241 through PY-261) report Compatible
- **No marketplace fix available** — server-side verification IDE selection is not author-configurable
- **Not PyCharm-only** — targeting `com.intellij.modules.python` allows IU users with Python plugin to install too

### Forum Threads

- **Your thread**: https://platform.jetbrains.com/t/verification-errors-for-python-dependent-plugin-false-positives-against-iu-builds/3813
- **Canonical thread** (best to chime in): https://platform.jetbrains.com/t/missing-failed-module-dependencies-in-plugin-xml/510
- Related: [Plugin Verifier: python not found](https://platform.jetbrains.com/t/plugin-verifier-package-com-jetbrains-python-is-not-found/1780) | [2025.1 PyCharm merge](https://platform.jetbrains.com/t/how-does-the-2025-1-pycharm-merge-affect-plugins/1364) | [First time approval](https://platform.jetbrains.com/t/waiting-for-first-time-approval/3602)

### Draft Reply for Thread #510

I'm having the same issue with my plugin **Python Intentions** ([marketplace page](https://plugins.jetbrains.com/plugin/30074-python-intentions)).

See my thread: [Verification errors for Python-dependent plugin (false positives against IU builds)](https://platform.jetbrains.com/t/verification-errors-for-python-dependent-plugin-false-positives-against-iu-builds/3813)

This is my first plugin, so I believe I need a resolution for the plugin to be approved and released.

Thank you.

### References

- Plugin page: https://plugins.jetbrains.com/plugin/30074-python-intentions
- Source: https://github.com/btakita/python-intentions
- JetBrains docs: [Plugin Compatibility](https://plugins.jetbrains.com/docs/intellij/plugin-compatibility.html) | [Verifying Compatibility](https://plugins.jetbrains.com/docs/intellij/verifying-plugin-compatibility.html)

## User
