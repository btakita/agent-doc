package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class EditorTabSyncListenerTest {

    private fun visibleSignature(
        visibleMdFiles: List<String>,
        editorLayout: EditorLayout? = null,
    ): String = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(
        visibleMdFiles = visibleMdFiles,
        editorLayout = editorLayout,
    )

    @Test
    fun `selection event file wins over stale selected editor file`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/tasks/professional/equityfundingsource.md",
        )

        val activeFile = EditorTabSyncListener.AutomaticCommandPlanner.resolveActiveFilePath(
            preferredActiveFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            selectedEditorFile = "/repo/tasks/professional/equityfundingsource.md",
            visibleMdFiles = visibleMdFiles,
        )

        assertEquals("/repo/tasks/agent-doc/agent-doc-bugs2.md", activeFile)
    }

    @Test
    fun `selection change keeps split layouts on sync when visible markdown set is unchanged`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature(
                listOf(
                    "/repo/src/boost-client/tasks/monsterrodholders.md",
                    "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                )
            ),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `single visible markdown file still uses sync when selection changes`() {
        val visibleMdFiles = listOf("/repo/src/boost-client/tasks/monsterrodholders.md")
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature(visibleMdFiles),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `visible markdown changes trigger non destructive sync`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `unchanged selection state does not rerun commands`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md"

        assertNull(
            EditorTabSyncListener.AutomaticCommandPlanner.plan(
                visibleMdFiles = visibleMdFiles,
                visibleSignature = visibleSignature(visibleMdFiles),
                focusedFile = focusedFile,
                previousVisibleSignature = visibleSignature(visibleMdFiles),
                previousFocusedFile = focusedFile,
            )
        )
    }

    @Test
    fun `opposite pane selection still dispatches sync when visible split is unchanged`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val visibleSignature = visibleSignature(visibleMdFiles)

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature,
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = visibleSignature,
            previousFocusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `visible set changes still dispatch sync`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles),
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = visibleSignature(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
            previousFocusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `selection change to a different file still dispatches sync`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val visibleSignature = visibleSignature(visibleMdFiles)

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature,
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature,
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `newer automatic sync generations replay after the running command finishes`() {
        assertEquals(true, EditorTabSyncListener.AutomaticCommandPlanner.shouldReplayAfterRun(3, 4))
        assertEquals(false, EditorTabSyncListener.AutomaticCommandPlanner.shouldReplayAfterRun(4, 4))
    }

    @Test
    fun `superseded automatic sync results do not schedule deferred retries`() {
        assertEquals(false, EditorTabSyncListener.AutomaticCommandPlanner.shouldScheduleDeferredRetry(3, 4))
        assertEquals(true, EditorTabSyncListener.AutomaticCommandPlanner.shouldScheduleDeferredRetry(4, 4))
    }

    @Test
    fun `safe passive preserve output keeps sync pending for retry`() {
        val result = EditorTabSyncListener.AutomaticCommandPlanner.analyzeCommandResult(
            kind = EditorTabSyncListener.AutomaticCommandKind.Sync,
            exitCode = 0,
            output = """
                [sync] resolved target window after repair: 4:agent-doc → @128
                [sync] safe passive sync preserved the current tmux layout because missing requested pane(s) /repo/tasks/software/tmux-router.md while visible protected pane(s) %241:preflight_started:/repo/tasks/software/tagpath.md cannot be detached safely because those panes still own open closeout cycle(s)
            """.trimIndent(),
        )

        assertEquals(false, result.applied)
        assertEquals(true, result.shouldRetry)
    }

    @Test
    fun `generic preserve output from bugs2 to tsift keeps automatic sync pending for retry`() {
        val result = EditorTabSyncListener.AutomaticCommandPlanner.analyzeCommandResult(
            kind = EditorTabSyncListener.AutomaticCommandKind.Sync,
            exitCode = 0,
            output = """
                [sync] resolved target window after repair: 4:agent-doc -> @128
                [sync] sync preserved the current tmux layout because missing requested pane(s) /repo/tasks/software/tsift.md while visible protected pane(s) %210:preflight_started:/repo/tasks/agent-doc/agent-doc-bugs2.md cannot be detached safely because those panes still own open closeout cycle(s)
            """.trimIndent(),
        )

        assertEquals(false, result.applied)
        assertEquals(true, result.shouldRetry)
    }

    @Test
    fun `safe passive preserve output with reselected focus is treated as applied`() {
        val result = EditorTabSyncListener.AutomaticCommandPlanner.analyzeCommandResult(
            kind = EditorTabSyncListener.AutomaticCommandKind.Sync,
            exitCode = 0,
            output = """
                [sync] safe passive sync preserved the current tmux layout because missing requested pane(s) /repo/tasks/software/tmux-router.md while visible protected pane(s) %241:preflight_started:/repo/tasks/software/tagpath.md cannot be detached safely because those panes still own open closeout cycle(s)
                [sync] safe_passive_layout_preserved_reselected_focus pane=%202 reason=protected_visible
            """.trimIndent(),
        )

        assertEquals(true, result.applied)
        assertEquals(false, result.shouldRetry)
    }

    @Test
    fun `safe passive lock contention keeps automatic sync pending for retry`() {
        val result = EditorTabSyncListener.AutomaticCommandPlanner.analyzeCommandResult(
            kind = EditorTabSyncListener.AutomaticCommandKind.Sync,
            exitCode = 0,
            output = """
                [sync] latency budget exceeded: phase sync_lock_wait took 101ms (budget 100ms, mode=safe-passive)
                [sync] safe_passive_sync_lock_contention_retry phase=sync_lock_wait elapsed_ms=101 budget_ms=100 status=over_budget action=retry
            """.trimIndent(),
        )

        assertEquals(false, result.applied)
        assertEquals(true, result.shouldRetry)
    }

    @Test
    fun `successful sync output without preserve marker applies immediately`() {
        val result = EditorTabSyncListener.AutomaticCommandPlanner.analyzeCommandResult(
            kind = EditorTabSyncListener.AutomaticCommandKind.Sync,
            exitCode = 0,
            output = """
                [sync] resolved target window after repair: 4:agent-doc → @128
                [sync] reconcile path: 2 columns, [["%243"], ["%202"]]
            """.trimIndent(),
        )

        assertEquals(true, result.applied)
        assertEquals(false, result.shouldRetry)
    }

    @Test
    fun `focus command success is treated as applied`() {
        val result = EditorTabSyncListener.AutomaticCommandPlanner.analyzeCommandResult(
            kind = EditorTabSyncListener.AutomaticCommandKind.Focus,
            exitCode = 0,
            output = "",
        )

        assertEquals(true, result.applied)
        assertEquals(false, result.shouldRetry)
    }

    @Test
    fun `column aware signatures preserve splitter identity for replayed requests`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val previousLayout = EditorLayout(
            columns = listOf(
                LayoutColumn(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
                LayoutColumn(listOf("/repo/src/boost-client/tasks/monsterrodholders.md")),
            )
        )
        val latestLayout = EditorLayout(
            columns = listOf(
                LayoutColumn(listOf("/repo/src/boost-client/tasks/monsterrodholders.md")),
                LayoutColumn(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
            )
        )

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles, latestLayout),
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = visibleSignature(visibleMdFiles, previousLayout),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }
}
