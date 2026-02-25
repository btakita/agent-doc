package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.popup.JBPopupFactory
import com.intellij.openapi.wm.WindowManager
import com.intellij.ui.JBColor
import java.awt.*
import java.awt.event.KeyAdapter
import java.awt.event.KeyEvent
import javax.swing.*

/**
 * Lightweight prompt panel that appears at the bottom of the editor area.
 *
 * Shows Claude Code permission prompts with numbered hotkeys.
 * Press 1-9 to select an option, Esc to dismiss.
 */
object PromptPanel {

    private var currentPopup: JDialog? = null

    fun show(project: Project, info: PromptInfo, onAnswer: (Int) -> Unit) {
        dismiss(project)

        val frame = WindowManager.getInstance().getFrame(project) ?: return

        val panel = JPanel(BorderLayout(8, 4)).apply {
            border = BorderFactory.createCompoundBorder(
                BorderFactory.createMatteBorder(1, 0, 0, 0, JBColor.border()),
                BorderFactory.createEmptyBorder(6, 12, 6, 12)
            )
            background = JBColor(Color(255, 252, 235), Color(60, 58, 40))
        }

        // Question label
        val questionText = info.question ?: "Permission required"
        val questionLabel = JLabel(questionText).apply {
            font = font.deriveFont(Font.BOLD, 12f)
        }
        panel.add(questionLabel, BorderLayout.NORTH)

        // Options as buttons with hotkey labels
        val optionsPanel = JPanel(FlowLayout(FlowLayout.LEFT, 6, 2))
        optionsPanel.isOpaque = false

        val options = info.options ?: return
        for (opt in options) {
            val btn = JButton("[${opt.index}] ${opt.label}").apply {
                isFocusable = false
                font = font.deriveFont(11f)
                toolTipText = "Press ${opt.index} or click"
                addActionListener {
                    onAnswer(opt.index)
                    dismiss(project)
                }
            }
            optionsPanel.add(btn)
        }

        // Esc hint
        val escLabel = JLabel("  Esc to dismiss").apply {
            font = font.deriveFont(Font.ITALIC, 10f)
            foreground = JBColor.GRAY
        }
        optionsPanel.add(escLabel)

        panel.add(optionsPanel, BorderLayout.CENTER)

        // Create undecorated dialog anchored to bottom of IDE window
        val dialog = JDialog(frame, false).apply {
            isUndecorated = true
            contentPane = panel
            pack()

            // Position at bottom of the frame
            val frameLocation = frame.locationOnScreen
            val frameSize = frame.size
            setLocation(
                frameLocation.x,
                frameLocation.y + frameSize.height - preferredSize.height
            )
            setSize(frameSize.width, preferredSize.height)
        }

        // Key listener for hotkeys (1-9 for options, Esc to dismiss)
        dialog.addKeyListener(object : KeyAdapter() {
            override fun keyPressed(e: KeyEvent) {
                when {
                    e.keyCode == KeyEvent.VK_ESCAPE -> dismiss(project)
                    e.keyChar in '1'..'9' -> {
                        val idx = e.keyChar - '0'
                        val opt = options.find { it.index == idx }
                        if (opt != null) {
                            onAnswer(opt.index)
                            dismiss(project)
                        }
                    }
                }
            }
        })

        // Also add key listener to the panel so it works when panel has focus
        panel.isFocusable = true
        panel.addKeyListener(object : KeyAdapter() {
            override fun keyPressed(e: KeyEvent) {
                when {
                    e.keyCode == KeyEvent.VK_ESCAPE -> dismiss(project)
                    e.keyChar in '1'..'9' -> {
                        val idx = e.keyChar - '0'
                        val opt = options.find { it.index == idx }
                        if (opt != null) {
                            onAnswer(opt.index)
                            dismiss(project)
                        }
                    }
                }
            }
        })

        dialog.isVisible = true
        panel.requestFocusInWindow()

        currentPopup = dialog
    }

    @Suppress("UNUSED_PARAMETER")
    fun dismiss(project: Project) {
        currentPopup?.let {
            it.isVisible = false
            it.dispose()
        }
        currentPopup = null
    }
}
