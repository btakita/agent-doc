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
    fun `stickyMarkdownForWindow prefers the live selection`() {
        assertEquals(
            "/repo/tasks/now.md",
            LayoutDetector.stickyMarkdownForWindow(
                selectedPath = "/repo/tasks/now.md",
                windowMarkdownTabsMruLast = listOf("/repo/tasks/stale.md"),
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
