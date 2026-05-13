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
        assertFalse(result.displayText.contains("agent-doc-bugs.md"))
        assertFalse(result.displayText.contains("prompts pending"))
        val tooltip = requireNotNull(result.tooltipText)
        assertTrue(tooltip.contains("[agent-doc-bugs.md]"))
        assertTrue(tooltip.contains("(3 prompts pending)"))
    }

    @Test
    fun `questionPresentation keeps secondary context out of visible prompt row`() {
        val result = questionPresentation(
            question = "Allow this action?",
            fileName = "agent-doc-bugs.md",
            totalActive = 2,
        )

        assertEquals("Allow this action?", result.displayText)
        val tooltip = requireNotNull(result.tooltipText)
        assertTrue(tooltip.contains("[agent-doc-bugs.md]"))
        assertTrue(tooltip.contains("(2 prompts pending)"))
    }

    @Test
    fun `buildPromptControlsRow uses non wrapping horizontal layout and compact detail control`() {
        val answers = mutableListOf<Int>()
        val controls = buildPromptControlsRow(
            options = listOf(
                PromptOption(1, "Yes"),
                PromptOption(2, "No"),
            ),
            buttonFont = Font(Font.SANS_SERIF, Font.PLAIN, 12),
            totalActive = 2,
            onAnswer = { answers.add(it) },
        )

        assertTrue(controls.layout is BoxLayout)
        val children = controls.components.toList()
        assertTrue(children.any { it is JButton && it.toolTipText == "[1] Yes" })
        assertTrue(children.any { it is JLabel && it.toolTipText?.contains("2 prompts pending") == true })

        val yesButton = children.filterIsInstance<JButton>().first { it.toolTipText == "[1] Yes" }
        yesButton.doClick()
        assertEquals(listOf(1), answers)
    }

    @Test
    fun `buildPromptControlsRow answers by one based option position not display index`() {
        val answers = mutableListOf<Int>()
        val controls = buildPromptControlsRow(
            options = listOf(
                PromptOption(4, "Allow once"),
                PromptOption(7, "Reject"),
            ),
            buttonFont = Font(Font.SANS_SERIF, Font.PLAIN, 12),
            totalActive = 1,
            onAnswer = { answers.add(it) },
        )

        val rejectButton = controls.components
            .filterIsInstance<JButton>()
            .first { it.toolTipText == "[7] Reject" }
        rejectButton.doClick()

        assertEquals(listOf(2), answers)
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
