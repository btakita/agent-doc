package com.github.btakita.agentdoc

import com.intellij.testFramework.TestApplicationManager
import com.intellij.testFramework.UsefulTestCase
import java.awt.Container
import javax.swing.JPanel
import javax.swing.SwingUtilities

class RepoContextTabLayoutTest : UsefulTestCase() {
    override fun runInDispatchThread() = true

    fun testCaretCheckboxStaysAtRightEdgeBesideBothTabs() {
        TestApplicationManager.getInstance()
        val document = RepoContextDocumentTree({}, {})
        val tabs = createRepoContextTabs(null, testRootDisposable, document, JPanel())
        val strip = tabs.component
        val caret = document.followEditorCaret
        for (width in listOf(340, 520)) {
            strip.setSize(width, 420)
            layoutTree(strip)

            val caretBounds = SwingUtilities.convertRectangle(caret.parent, caret.bounds, strip)
            assertEquals("checkbox uses its preferred width", caret.preferredSize.width, caretBounds.width)
            assertEquals(
                "checkbox reaches the right edge at width $width",
                strip.width - strip.insets.right,
                caretBounds.x + caretBounds.width,
            )
            for (tab in tabs.tabs) {
                val label = requireNotNull(tabs.getTabLabel(tab))
                val tabBounds = SwingUtilities.convertRectangle(label.parent, label.bounds, strip)
                assertTrue(
                    "${tab.text} shares the caret checkbox row: $tabBounds and $caretBounds",
                    tabBounds.maxY > caretBounds.y && caretBounds.maxY > tabBounds.y,
                )
                assertTrue(
                    "${tab.text} sits before the caret checkbox: $tabBounds and $caretBounds",
                    tabBounds.maxX <= caretBounds.x,
                )
            }
        }
    }

    private fun layoutTree(container: Container) {
        container.doLayout()
        container.components.filterIsInstance<Container>().forEach(::layoutTree)
    }
}
