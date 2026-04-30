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
}
