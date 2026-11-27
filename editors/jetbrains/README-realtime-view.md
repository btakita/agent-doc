# Repo Context Explorer View pilot

JetBrains and VS Code use the [same Explorer View definition](../vscode/media/repoContextView.json).
Each IDE provides its existing native input controls and session routing. The
Lazily receiver validates structural revisions, folds graph Snapshots and
Deltas, and admits typed events before the host applies an authorized action.
The released binding pins still support the existing Explorer. When a
candidate binding with Realtime View is available, the Explorer uses its
revision and event path.

For a JetBrains pilot build use the existing source override
`AGENT_DOC_LAZILY_KT_DIR` with a checkout that exports `RealtimeView`.
Run `./gradlew test --tests '*RepoContextRealtimeViewTest'` from
`editors/jetbrains`. For VS Code use a package build of lazily-js that exports
`@lazily-hub/lazily-js/realtime-view` in the extension's dependencies, run
`npm run bundle`, then run `npm test` from `editors/vscode`.
Run `python3 editors/vscode/scripts/check-repo-context-browser.py` from the
repository root to test keyed DOM rows, caret, focus, composition, draft
preservation and same-ID reconnect replay in isolated Chromium. The browser
test requires Chromium and ChromeDriver.
