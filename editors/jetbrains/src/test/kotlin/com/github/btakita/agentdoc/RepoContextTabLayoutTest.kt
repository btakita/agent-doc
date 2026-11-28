package com.github.btakita.agentdoc

import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.LogicalPosition
import com.intellij.openapi.project.ProjectManager
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

    fun testOpeningFollowsTheInvokingEditorWhenItsOutlineArrives() {
        TestApplicationManager.getInstance()
        val project = ProjectManager.getInstance().defaultProject
        val factory = EditorFactory.getInstance()
        val document = factory.createDocument("Run\nFirst follow-up\nVerify the Explorer")
        val invokingEditor = factory.createEditor(document, project)
        try {
            val run = DocumentPromptTarget("run", "run", "Run", 1, 0, DocumentPromptAction.QUEUE, endLine = 3)
            val first = DocumentPromptTarget("first", "first", "First follow-up", 2, 1, DocumentPromptAction.EXCHANGE)
            val followUp = first.copy(id = "verify", selectionKey = "verify", label = "Verify the Explorer", line = 3)
            val outline = RepoContextDocumentOutline("current", listOf(run, first, followUp))
            lateinit var caretSync: RepoContextCaretSync
            val tree = RepoContextDocumentTree({}, {}, onTreeSelection = { caretSync.cancelOpeningFollow() })
            caretSync = RepoContextCaretSync(
                project, testRootDisposable, canFollow = { false },
                canFollowOnOpen = { tree.followEditorCaret.isSelected }, selectTarget = tree::selectTarget,
            )
            invokingEditor.caretModel.moveToLogicalPosition(LogicalPosition(2, 0))
            caretSync.openAtCaret(invokingEditor)

            tree.show(outline)
            assertEquals(run, tree.selection)
            caretSync.show(document, document.modificationStamp - 1, outline)
            assertEquals(run, tree.selection)
            caretSync.show(document, document.modificationStamp, outline)
            assertEquals(followUp, tree.selection)

            tree.followEditorCaret.isSelected = false
            tree.selectTarget(first.id)
            caretSync.openAtCaret(invokingEditor)
            assertEquals(first, tree.selection)

            tree.followEditorCaret.isSelected = true
            caretSync.clear()
            caretSync.openAtCaret(invokingEditor)
            tree.selectTarget(run.id)
            caretSync.show(document, document.modificationStamp, outline)
            assertEquals(run, tree.selection)
        } finally {
            factory.releaseEditor(invokingEditor)
        }
    }

    private fun layoutTree(container: Container) {
        container.doLayout()
        container.components.filterIsInstance<Container>().forEach(::layoutTree)
    }
}
