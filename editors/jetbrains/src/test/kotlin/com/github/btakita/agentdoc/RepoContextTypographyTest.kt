package com.github.btakita.agentdoc

import com.intellij.openapi.ui.ComboBox
import com.intellij.ui.scale.JBUIScale
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBList
import com.intellij.ui.components.JBTextArea
import com.intellij.ui.components.JBTextField
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.awt.FlowLayout
import java.awt.Font
import java.nio.file.Path
import javax.swing.JButton
import javax.swing.JComponent
import javax.swing.SwingUtilities
import javax.swing.UIManager
import javax.swing.plaf.FontUIResource

class RepoContextTypographyTest {
    @Test
    fun `explorer text remains compact when the IDE UI font changes`() = SwingUtilities.invokeAndWait {
        val previousFont = UIManager.get("Label.font")
        try {
            UIManager.put("Label.font", FontUIResource(Font.DIALOG, Font.PLAIN, 14))
            val location = RepoContextLocation(Path.of("src/example.kt"), 2, 4, "example", "function")
            val renderer = RepoContextLocationRenderer()
            val list = JBList(listOf(location)).apply { cellRenderer = renderer }
            val textComponents = arrayOf<JComponent>(
                ComboBox(RepoContextMode.entries.toTypedArray()), JBTextField(), JButton("Search"), JBLabel("Status"),
                list, JBTextArea(7, 20),
            )
            val explorer = object : RepoContextTypography(FlowLayout()) {
                init {
                    textComponents.forEach { add(it) }
                    useCompactFont(*textComponents)
                }
            }
            val otherLabel = JBLabel("Outside the Explorer")
            val originalRowHeight = list.getCellBounds(0, 0).height

            val compactOffset = JBUIScale.scale(2f)
            textComponents.forEach { assertEquals(14f - compactOffset, it.font.size2D, 0f) }
            assertEquals(list.font, renderer.font)
            assertEquals(14, otherLabel.font.size)
            assertTrue(originalRowHeight > renderer.getFontMetrics(renderer.font).height)

            UIManager.put("Label.font", FontUIResource(Font.SERIF, Font.BOLD, 24))
            repeat(2) { SwingUtilities.updateComponentTreeUI(explorer) }
            SwingUtilities.updateComponentTreeUI(otherLabel)
            val enlargedRowHeight = list.getCellBounds(0, 0).height

            textComponents.forEach {
                assertEquals(24f - compactOffset, it.font.size2D, 0f)
                assertEquals(Font.SERIF, it.font.family)
                assertEquals(Font.BOLD, it.font.style)
            }
            assertEquals(list.font, renderer.font)
            assertEquals(24, otherLabel.font.size)
            assertTrue(enlargedRowHeight > originalRowHeight)
            assertTrue(enlargedRowHeight > renderer.getFontMetrics(renderer.font).height)
        } finally {
            UIManager.put("Label.font", previousFont)
        }
    }
}
