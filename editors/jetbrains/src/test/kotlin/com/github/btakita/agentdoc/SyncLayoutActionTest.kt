package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class SyncLayoutActionTest {

    @Test
    fun `automatic sync commands opt out of autostart`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/tasks/one.md", "/repo/tasks/two.md"),
            editorLayout = null,
            focusedFile = "/repo/tasks/one.md",
            noAutostart = true,
            exactVisible = true,
        )

        assertTrue(cmd.contains("--no-autostart"))
        assertTrue(cmd.contains("--exact-visible"))
    }

    @Test
    fun `manual sync commands keep autostart available`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/tasks/one.md"),
            editorLayout = null,
            focusedFile = "/repo/tasks/one.md",
            noAutostart = false,
        )

        assertFalse(cmd.contains("--no-autostart"))
    }

    @Test
    fun `resync publishes a distinct reactive generation kind`() {
        assertEquals(
            "resync",
            SyncLayoutAction.syncCallerKind(
                noAutostart = true,
                requestedCallerKind = "resync",
            ),
        )
        assertEquals(
            "automatic",
            SyncLayoutAction.syncCallerKind(
                noAutostart = true,
                requestedCallerKind = null,
            ),
        )
        assertEquals(
            "manual",
            SyncLayoutAction.syncCallerKind(
                noAutostart = false,
                requestedCallerKind = null,
            ),
        )
    }

    @Test
    fun `sync failure message surfaces controller diagnostic`() {
        assertEquals(
            "Sync failed: pane creation failed in tmux session agent-doc",
            SyncLayoutAction.syncFailureMessage(
                "pane creation failed in tmux session agent-doc",
            ),
        )
    }

    @Test
    fun `sync subprocess timeout returns and marks timed out`() {
        val result = SyncLayoutAction.runCommandWithTimeout(
            listOf("sh", "-c", "sleep 2"),
            File(".").absolutePath,
            timeoutMs = 50,
        )

        assertEquals(124, result.exitCode)
        assertTrue(result.timedOut)
    }

    @Test
    fun `supersede re-runs only when the guard holder is superseded mid-flight`() {
        // Held the guard AND a newer sync bumped the generation while we ran → re-run with
        // the latest layout (supersede, not drop).
        assertTrue(
            SyncLayoutAction.shouldRerunAfterSupersede(
                heldGuard = true,
                generationStillCurrent = false,
            ),
        )
        // Held the guard and no newer sync arrived → done, do not re-run (would loop).
        assertFalse(
            SyncLayoutAction.shouldRerunAfterSupersede(
                heldGuard = true,
                generationStillCurrent = true,
            ),
        )
        // Deferred request (never held the guard) → never re-runs; the in-flight holder
        // owns the supersede re-run.
        assertFalse(
            SyncLayoutAction.shouldRerunAfterSupersede(
                heldGuard = false,
                generationStillCurrent = false,
            ),
        )
        assertFalse(
            SyncLayoutAction.shouldRerunAfterSupersede(
                heldGuard = false,
                generationStillCurrent = true,
            ),
        )
    }

    @Test
    fun `deferred sync retries are bounded while guard is held`() {
        assertEquals(
            SyncLayoutAction.SYNC_DEFERRED_RETRY_MS,
            SyncLayoutAction.deferredSyncRetryDelayMs(0),
        )
        assertEquals(
            SyncLayoutAction.SYNC_DEFERRED_RETRY_MS,
            SyncLayoutAction.deferredSyncRetryDelayMs(
                SyncLayoutAction.SYNC_DEFERRED_MAX_RETRIES - 1,
            ),
        )
        assertNull(
            SyncLayoutAction.deferredSyncRetryDelayMs(
                SyncLayoutAction.SYNC_DEFERRED_MAX_RETRIES,
            )
        )
    }

    @Test
    fun `sync command never injects a window flag`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/tasks/one.md"),
            editorLayout = null,
            focusedFile = "/repo/tasks/one.md",
            noAutostart = false,
        )

        assertFalse(cmd.contains("--window"))
    }

    @Test
    fun `manual sync preserves layout warning is extracted for notification`() {
        val warning = SyncLayoutAction.preservedLayoutWarning(
            """
                [sync] resolved target window after repair: 4:agent-doc -> @128
                [sync] sync preserved the current tmux layout because missing requested pane(s) /repo/tasks/software/tsift.md while visible protected pane(s) %210:preflight_started:/repo/tasks/agent-doc/agent-doc-bugs2.md cannot be detached safely because those panes still own open closeout cycle(s)
            """.trimIndent(),
        )

        assertEquals(
            "Sync deferred: another visible agent-doc pane is mid-closeout, so the current tmux layout was preserved. Try again after that closeout finishes. Blocked pane(s): %210 preflight_started /repo/tasks/agent-doc/agent-doc-bugs2.md",
            warning,
        )
    }

    @Test
    fun `manual sync preserves layout warning lists multiple protected panes`() {
        val warning = SyncLayoutAction.preservedLayoutWarning(
            """
                [sync] sync preserved the current tmux layout because missing requested pane(s) /repo/tasks/new.md while visible protected pane(s) %208:preflight_started:/repo/tasks/software/tsift.md, %210:write_applied:/repo/tasks/agent-doc/agent-doc-bugs2.md cannot be detached safely because those panes still own open closeout cycle(s)
            """.trimIndent(),
        )

        assertEquals(
            "Sync deferred: another visible agent-doc pane is mid-closeout, so the current tmux layout was preserved. Try again after that closeout finishes. Blocked pane(s): %208 preflight_started /repo/tasks/software/tsift.md; %210 write_applied /repo/tasks/agent-doc/agent-doc-bugs2.md",
            warning,
        )
    }

    @Test
    fun `manual sync warning extraction ignores ordinary sync output`() {
        assertNull(
            SyncLayoutAction.preservedLayoutWarning(
                """
                    [sync] resolve_file: /repo/tasks/one.md -> Registered
                    [sync] reconcile path: 1 columns
                """.trimIndent(),
            )
        )
    }

    @Test
    fun `sync command preserves empty columns so sync can restore remembered panes`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/src/sample-app/tasks/sampleorders.md"),
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("/repo/src/sample-app/tasks/sampleorders.md")),
                )
            ),
            focusedFile = "/repo/src/sample-app/tasks/sampleorders.md",
            noAutostart = false,
        )

        assertEquals(
            listOf(
                "agent-doc",
                "sync",
                "--col",
                "",
                "--col",
                "/repo/src/sample-app/tasks/sampleorders.md",
                "--focus",
                "/repo/src/sample-app/tasks/sampleorders.md",
            ),
            cmd,
        )
    }

    @Test
    fun `automatic exact visible single file sync does not ask cli to expand remembered siblings`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/tasks/software/tsift.md"),
            editorLayout = null,
            focusedFile = "/repo/tasks/software/tsift.md",
            noAutostart = true,
            exactVisible = true,
        )

        assertEquals(
            listOf(
                "agent-doc",
                "sync",
                "--col",
                "/repo/tasks/software/tsift.md",
                "--focus",
                "/repo/tasks/software/tsift.md",
                "--exact-visible",
                "--no-autostart",
            ),
            cmd,
        )
    }

    @Test
    fun `normalize editor layout rewrites workspace relative files into submodule relative files`() {
        val normalized = SyncLayoutAction.normalizeEditorLayout(
            basePath = "/repo",
            projectRoot = "/repo/src/sample-app",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(listOf("src/sample-app/tasks/one.md")),
                    LayoutColumn(listOf("src/sample-app/tasks/two.md", "tasks/ignored.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("tasks/one.md")),
                    LayoutColumn(listOf("tasks/two.md", "/repo/tasks/ignored.md")),
                )
            ),
            normalized,
        )
    }

    @Test
    fun `normalize editor layout preserves empty columns for non markdown siblings`() {
        val normalized = SyncLayoutAction.normalizeEditorLayout(
            basePath = "/repo",
            projectRoot = "/repo/src/sample-app",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("src/sample-app/tasks/sampleorders.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("tasks/sampleorders.md")),
                )
            ),
            normalized,
        )
    }

    @Test
    fun `normalize editor layout preserves cross root markdown files as absolute paths`() {
        val normalized = SyncLayoutAction.normalizeEditorLayout(
            basePath = "/repo",
            projectRoot = "/repo/src/sample-app",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(listOf("tasks/agent-doc/agent-doc-bugs2.md")),
                    LayoutColumn(listOf("src/sample-app/tasks/sampleorders.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
                    LayoutColumn(listOf("tasks/sampleorders.md")),
                )
            ),
            normalized,
        )
    }

    @Test
    fun `absolutize editor layout rewrites project relative files into absolute paths`() {
        val absolute = SyncLayoutAction.absolutizeEditorLayout(
            projectRoot = "/repo/src/sample-app",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(listOf("tasks/one.md")),
                    LayoutColumn(listOf("/already/absolute.md", "tasks/two.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/src/sample-app/tasks/one.md")),
                    LayoutColumn(listOf("/already/absolute.md", "/repo/src/sample-app/tasks/two.md")),
                )
            ),
            absolute,
        )
    }

    @Test
    fun `absolutize editor layout preserves empty columns`() {
        val absolute = SyncLayoutAction.absolutizeEditorLayout(
            projectRoot = "/repo/src/sample-app",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("tasks/sampleorders.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("/repo/src/sample-app/tasks/sampleorders.md")),
                )
            ),
            absolute,
        )
    }

    @Test
    fun `collect visible markdown files keeps cross root paths`() {
        val visible = arrayOf(
            FakeVirtualFile("/repo/tasks/agent-doc/agent-doc-bugs2.md"),
            FakeVirtualFile("/repo/src/sample-app/tasks/sampleorders.md"),
            FakeVirtualFile("/repo/notes/todo.txt"),
        )

        assertEquals(
            listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/src/sample-app/tasks/sampleorders.md",
            ),
            SyncLayoutAction.collectVisibleMarkdownFiles(visible),
        )
    }

    @Test
    fun `collect visible markdown files excludes non-session markdown plans`() {
        val session = FakeVirtualFile("/repo/tasks/backend.md")
        val plan = FakeVirtualFile("/repo/docs/backend-fpe-contracts-sdk-pr-plan.md")

        assertEquals(
            listOf(session.path),
            SyncLayoutAction.collectVisibleMarkdownFiles(
                arrayOf(session, plan),
                isSessionDocument = { it.path == session.path },
            ),
        )
    }

    @Test
    fun `choose sync project root keeps single root sync scoped to that root`() {
        assertEquals(
            "/repo/src/sample-app",
            SyncLayoutAction.chooseSyncProjectRoot(
                basePath = "/repo",
                fallbackRoot = "/repo/src/sample-app",
                visibleMarkdownFiles = listOf(
                    "/repo/src/sample-app/tasks/sampleorders.md",
                    "/repo/src/sample-app/tasks/buildparty.md",
                ),
            ),
        )
    }

    @Test
    fun `choose sync project root uses workspace root for cross root layouts`() {
        assertEquals(
            "/repo",
            SyncLayoutAction.chooseSyncProjectRoot(
                basePath = "/repo",
                fallbackRoot = "/repo/src/agent-doc",
                visibleMarkdownFiles = listOf(
                    "/repo/src/agent-doc/specs/08-session-routing.md",
                    "/repo/src/sample-app/tasks/sampleorders.md",
                ),
            ),
        )
    }

    @Test
    fun `build columns groups windows by x into separate columns`() {
        // Two editor windows at distinct x positions → a 2-column (2-editor-pane)
        // layout, which sync mirrors as 2 editor panes + 1 agent-doc pane.
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "left.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 900, y = 0, file = "right.md"),
            )
        )

        assertEquals(
            listOf(
                LayoutColumn(listOf("left.md")),
                LayoutColumn(listOf("right.md")),
            ),
            columns,
        )
    }

    @Test
    fun `build columns merges windows within x tolerance into one column`() {
        // Stacked windows (same x, different y) stay a single column → one editor
        // pane, so navigation does not spuriously grow the tmux layout.
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 4, y = 300, file = "bottom.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "top.md"),
            )
        )

        assertEquals(
            listOf(LayoutColumn(listOf("top.md", "bottom.md"))),
            columns,
        )
    }

    private class FakeVirtualFile(private val rawPath: String) :
        com.intellij.testFramework.LightVirtualFile(
            java.io.File(rawPath).name,
            ""
        ) {
        override fun getPath(): String = rawPath
    }
}
