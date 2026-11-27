package com.github.btakita.agentdoc

import com.intellij.ui.components.JBList
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.awt.Font
import java.nio.file.Path
import javax.swing.SwingUtilities
import javax.swing.UIManager
import javax.swing.plaf.FontUIResource

class RepoContextTypographyTest {
    @Test
    fun `result rows and answer follow changes to the IDE UI font`() = SwingUtilities.invokeAndWait {
        val previousFont = UIManager.get("Label.font")
        try {
            UIManager.put("Label.font", FontUIResource(Font.DIALOG, Font.PLAIN, 14))
            val location = RepoContextLocation(Path.of("src/example.kt"), 2, 4, "example", "function")
            val renderer = RepoContextLocationRenderer()
            val list = JBList(listOf(location)).apply { cellRenderer = renderer }
            val answer = RepoContextAnswerArea()
            val originalRowHeight = list.getCellBounds(0, 0).height

            assertEquals(14, answer.font.size)
            assertEquals(14, renderer.font.size)
            assertTrue(originalRowHeight > renderer.getFontMetrics(renderer.font).height)

            UIManager.put("Label.font", FontUIResource(Font.DIALOG, Font.PLAIN, 24))
            SwingUtilities.updateComponentTreeUI(list)
            SwingUtilities.updateComponentTreeUI(answer)
            val enlargedRowHeight = list.getCellBounds(0, 0).height

            assertEquals(24, answer.font.size)
            assertEquals(24, renderer.font.size)
            assertTrue(enlargedRowHeight > originalRowHeight)
            assertTrue(enlargedRowHeight > renderer.getFontMetrics(renderer.font).height)
        } finally {
            UIManager.put("Label.font", previousFont)
        }
    }
}
