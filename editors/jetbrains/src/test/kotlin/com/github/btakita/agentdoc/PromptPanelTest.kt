package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test
import java.awt.Font
import javax.swing.BoxLayout
import javax.swing.JButton
import javax.swing.JLabel
import javax.swing.JPanel

class PromptPanelTest {

    @Test
    fun `truncateSingleLineText trims long text and preserves tooltip`() {
        val result = truncateSingleLineText("1234567890", 6)
        assertEquals("12345…", result.displayText)
        assertEquals("1234567890", result.tooltipText)
    }

    @Test
    fun `questionPresentation includes file and pending count in tooltip when truncated`() {
        val result = questionPresentation(
            question = "A".repeat(140),
            fileName = "agent-doc-bugs.md",
            totalActive = 3,
        )

        assertTrue(result.displayText.endsWith("…"))
        val tooltip = requireNotNull(result.tooltipText)
        assertTrue(tooltip.contains("[agent-doc-bugs.md]"))
        assertTrue(tooltip.contains("(3 prompts pending)"))
    }

    @Test
    fun `buildPromptControlsRow uses non wrapping horizontal layout and compact detail control`() {
        val controls = buildPromptControlsRow(
            options = listOf(
                PromptOption(1, "Yes"),
                PromptOption(2, "No"),
            ),
            buttonFont = Font(Font.SANS_SERIF, Font.PLAIN, 12),
            totalActive = 2,
            onAnswer = {},
        )

        assertTrue(controls.layout is BoxLayout)
        val children = controls.components.toList()
        assertTrue(children.any { it is JButton && it.toolTipText == "Yes" })
        assertTrue(children.any { it is JLabel && it.toolTipText?.contains("2 prompts pending") == true })
    }

    @Test
    fun `lockSingleRowHeight fixes preferred minimum and maximum height`() {
        val panel = JPanel().apply {
            add(JLabel("Prompt"))
            add(JButton("Approve"))
        }

        lockSingleRowHeight(panel)

        assertEquals(panel.preferredSize.height, panel.minimumSize.height)
        assertEquals(panel.preferredSize.height, panel.maximumSize.height)
    }
}
