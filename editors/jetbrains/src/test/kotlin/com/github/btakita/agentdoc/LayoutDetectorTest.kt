package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

class LayoutDetectorTest {

    @Test
    fun `buildColumnsFromSnapshots keeps screen order when focused window is listed first`() {
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 600, y = 0, file = "right.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "left.md"),
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
    fun `buildColumnsFromSnapshots stacks windows in the same column by y position`() {
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 400, file = "bottom.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "top.md"),
            )
        )

        assertEquals(
            listOf(LayoutColumn(listOf("top.md", "bottom.md"))),
            columns,
        )
    }

    @Test
    fun `buildColumnsFromSnapshots preserves empty columns for non markdown editor panes`() {
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = null),
                LayoutDetector.LayoutWindowSnapshot(x = 600, y = 0, file = "right.md"),
            )
        )

        assertEquals(
            listOf(
                LayoutColumn(emptyList()),
                LayoutColumn(listOf("right.md")),
            ),
            columns,
        )
    }

    @Test
    fun `buildColumnsFromSnapshots keeps unlaid-out splits apart instead of stacking them`() {
        // GH #77: a JetBrains Remote Dev backend never lays the editor splitters out,
        // so both splits report origin (0,0). Grouping by x folded them into one
        // column, the controller kept only the first document, and tmux converged to
        // a single pane swapped on every switch.
        val columns = LayoutDetector.buildColumnsFromSnapshots(
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "tasks/agent-doc/agent-doc.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "tasks/pmt2/mr/1109.md"),
            )
        )

        assertEquals(
            listOf(
                LayoutColumn(listOf("tasks/agent-doc/agent-doc.md")),
                LayoutColumn(listOf("tasks/pmt2/mr/1109.md")),
            ),
            columns,
        )
    }

    @Test
    fun `observed layout line names windows origins documents and columns`() {
        // GH #81 discriminator: tells "the IDE exposed one window" from a lossy join.
        val line = LayoutDetector.observedLayoutLine(
            2,
            listOf(
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = "a.md"),
                LayoutDetector.LayoutWindowSnapshot(x = 0, y = 0, file = null),
            ),
            listOf(LayoutColumn(listOf("a.md")), LayoutColumn(emptyList())),
        )
        assertEquals(
            "[layout-detect] observed windows=2 snapshots=[(0,0) a.md, (0,0) <none>] " +
                "columns=2 [a.md | <empty>]",
            line,
        )
    }

    @Test
    fun `stickyMarkdownForWindow prefers the live selection`() {
        assertEquals(
            "/repo/tasks/now.md",
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/tasks/now.md",
                windowMarkdownTabsMruLast =
                    listOf("/repo/tasks/stale.md", "/repo/tasks/now.md"),
            ),
        )
    }

    @Test
    fun `stickyMarkdownForWindow ignores a selected non-session markdown plan`() {
        assertEquals(
            "/repo/tasks/backend.md",
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/docs/backend-fpe-contracts-sdk-pr-plan.md",
                windowMarkdownTabsMruLast = listOf("/repo/tasks/backend.md"),
            ),
        )
    }

    @Test
    fun `stickyMarkdownForWindow falls back to the last document when source is selected`() {
        // #stickymdpane: the operator opened source in this column. The column
        // still stands for the document it was showing, so the tmux mirror
        // keeps that pane instead of collapsing.
        assertEquals(
            "/repo/tasks/recent.md",
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/src/Thing.kt",
                windowMarkdownTabsMruLast =
                    listOf("/repo/tasks/older.md", "/repo/tasks/recent.md"),
            ),
        )
    }

    @Test
    fun `stickyMarkdownForWindow invents nothing for a source-only window`() {
        assertEquals(
            null,
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/src/Thing.kt",
                windowMarkdownTabsMruLast = emptyList(),
            ),
        )
        assertEquals(
            null,
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = null,
                windowMarkdownTabsMruLast = emptyList(),
            ),
        )
    }
}
