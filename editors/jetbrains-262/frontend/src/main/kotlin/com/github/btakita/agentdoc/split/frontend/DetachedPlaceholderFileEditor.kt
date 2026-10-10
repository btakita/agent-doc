package com.github.btakita.agentdoc.split.frontend

import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.fileEditor.FileEditorPolicy
import com.intellij.openapi.fileEditor.FileEditorProvider
import com.intellij.openapi.fileEditor.FileEditorState
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.UserDataHolderBase
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.testFramework.LightVirtualFile
import com.intellij.ui.components.JBLabel
import com.intellij.util.ui.JBUI
import java.awt.BorderLayout
import java.beans.PropertyChangeListener
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.SwingConstants

internal class DetachedPlaceholderVirtualFile(
    val logicalDocument: String,
    val message: String,
) : LightVirtualFile("Agent Doc — read-only") {
    init {
        isWritable = false
    }
}

class DetachedPlaceholderFileEditorProvider : FileEditorProvider, DumbAware {
    override fun accept(project: Project, file: VirtualFile): Boolean =
        file is DetachedPlaceholderVirtualFile

    override fun createEditor(project: Project, file: VirtualFile): FileEditor =
        DetachedPlaceholderFileEditor(file as DetachedPlaceholderVirtualFile)

    override fun getEditorTypeId(): String = "agent-doc-detached-placeholder"

    override fun getPolicy(): FileEditorPolicy = FileEditorPolicy.HIDE_DEFAULT_EDITOR
}

private class DetachedPlaceholderFileEditor(
    private val file: DetachedPlaceholderVirtualFile,
) : UserDataHolderBase(), FileEditor {
    private val panel = JPanel(BorderLayout()).apply {
        border = JBUI.Borders.empty(24)
        add(
            JBLabel(file.message, SwingConstants.CENTER).apply {
                isFocusable = false
            },
            BorderLayout.CENTER,
        )
    }

    override fun getComponent(): JComponent = panel
    override fun getPreferredFocusedComponent(): JComponent? = null
    override fun getName(): String = "Agent Doc placeholder"
    override fun getFile(): VirtualFile = file
    override fun setState(state: FileEditorState) = Unit
    override fun isModified(): Boolean = false
    override fun isValid(): Boolean = file.isValid
    override fun addPropertyChangeListener(listener: PropertyChangeListener) = Unit
    override fun removePropertyChangeListener(listener: PropertyChangeListener) = Unit
    override fun dispose() = Unit
}
