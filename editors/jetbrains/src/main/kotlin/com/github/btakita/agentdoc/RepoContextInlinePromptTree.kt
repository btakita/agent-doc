package com.github.btakita.agentdoc

import com.intellij.ui.components.JBTextArea
import com.intellij.ui.treeStructure.Tree
import com.intellij.util.ui.JBUI
import java.awt.Dimension
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.tree.DefaultMutableTreeNode
import javax.swing.tree.DefaultTreeCellRenderer
import javax.swing.tree.DefaultTreeModel
import javax.swing.tree.TreeCellRenderer
import javax.swing.tree.TreePath

/** Give the live composer its own tree row without passing it to Swing's paint-only cell renderer. */
internal class RepoContextInlinePromptTree(private val composer: JComponent) : Tree() {
    private val promptRow = DefaultMutableTreeNode(null, false)
    private val spacer = JPanel().apply { isOpaque = false }
    private val answerRow = JBTextArea().apply {
        isEditable = false
        isFocusable = false
        isOpaque = false
        lineWrap = true
        wrapStyleWord = true
        border = JBUI.Borders.empty(2, 0)
    }

    init {
        setRowHeight(0)
        val targetRenderer = getCellRenderer() ?: DefaultTreeCellRenderer()
        // Use the Swing setter so the bounds cache and paint delegate see the same renderer.
        setCellRenderer(TreeCellRenderer { tree, value, selected, expanded, leaf, row, hasFocus ->
            if (value === promptRow) {
                spacer.preferredSize = Dimension(composer.minimumSize.width, composer.preferredSize.height + JBUI.scale(4))
                spacer
            } else {
                val standard = targetRenderer.getTreeCellRendererComponent(tree, value, selected, expanded, leaf, row, hasFocus)
                val target = (value as? DefaultMutableTreeNode)?.userObject as? DocumentPromptTarget
                val text = target?.answerText
                if (text == null) standard else {
                    answerRow.font = tree.font
                    answerRow.foreground = standard.foreground
                    answerRow.text = text
                    val wrapWidth = answerRow.getFontMetrics(answerRow.font).charWidth('m') * 72
                    answerRow.setSize(wrapWidth, Short.MAX_VALUE.toInt())
                    answerRow.preferredSize = answerRow.ui.getPreferredSize(answerRow)
                    answerRow
                }
            }
        })
        composer.isVisible = false
        add(composer)
    }

    fun putComposerUnder(target: DefaultMutableTreeNode?) {
        val model = model as DefaultTreeModel
        val previous = promptRow.parent as? DefaultMutableTreeNode
        if (previous !== target) {
            if (previous != null) {
                if (previous.root === model.root) model.removeNodeFromParent(promptRow)
                else promptRow.removeFromParent()
            }
            if (target != null) {
                model.insertNodeInto(promptRow, target, 0)
                expandPath(TreePath(target.path))
            }
        }
        if (target == null) composer.isVisible = false
        revalidate()
        repaint()
    }

    // Keep keyboard navigation on document targets. The composer has its own focusable controls.
    override fun setSelectionPath(path: TreePath?) {
        if (path?.lastPathComponent !== promptRow) {
            super.setSelectionPath(path)
            return
        }
        val row = getRowForPath(path)
        val next = if (leadSelectionRow < row) getPathForRow(row + 1) else path.parentPath
        if (next != null) super.setSelectionPath(next)
    }

    override fun doLayout() {
        super.doLayout()
        val bounds = if (promptRow.parent == null) null else getPathBounds(TreePath(promptRow.path))
        composer.isVisible = bounds != null
        if (bounds == null) return
        composer.setBounds(
            bounds.x,
            bounds.y,
            maxOf(bounds.width, width - bounds.x - JBUI.scale(4)),
            composer.preferredSize.height,
        )
    }
}
