package com.github.btakita.agentdoc

import com.intellij.openapi.editor.colors.EditorColorsManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.WindowManager
import com.intellij.ui.JBColor
import java.awt.*
import java.awt.event.ComponentAdapter
import java.awt.event.ComponentEvent
import java.awt.event.FocusAdapter
import java.awt.event.FocusEvent
import java.awt.event.KeyAdapter
import java.awt.event.KeyEvent
import javax.swing.*
import javax.swing.Timer

// Colors for unfocused (muted) and focused (highlighted) states
private val BG_UNFOCUSED = JBColor(Color(245, 243, 228), Color(50, 48, 35))
private val BG_FOCUSED = JBColor(Color(255, 252, 235), Color(60, 58, 40))
private val BORDER_UNFOCUSED = JBColor.border()
private val BORDER_FOCUSED = JBColor(Color(70, 130, 220), Color(100, 160, 255))
private const val MAX_QUESTION_LABEL_LEN = 96
private const val MAX_OPTION_LABEL_LEN = 25
private const val PANEL_V_PADDING = 6
private const val PANEL_H_PADDING = 12

internal data class PromptTextPresentation(
    val displayText: String,
    val tooltipText: String?,
)

private fun htmlTooltip(vararg lines: String?): String? {
    val parts = lines.filterNotNull().filter { it.isNotBlank() }
    return when (parts.size) {
        0 -> null
        1 -> parts.first()
        else -> parts.joinToString(prefix = "<html>", separator = "<br>", postfix = "</html>")
    }
}

internal fun truncateSingleLineText(text: String, maxLen: Int): PromptTextPresentation {
    val normalized = text.replace(Regex("\\s+"), " ").trim()
    if (normalized.length <= maxLen) {
        return PromptTextPresentation(normalized, null)
    }
    if (maxLen <= 1) {
        return PromptTextPresentation("\u2026", normalized)
    }
    return PromptTextPresentation(normalized.take(maxLen - 1) + "\u2026", normalized)
}

internal fun questionPresentation(
    question: String?,
    fileName: String?,
    totalActive: Int,
): PromptTextPresentation {
    val baseQuestion = question ?: "Permission required"
    val questionText = truncateSingleLineText(baseQuestion, MAX_QUESTION_LABEL_LEN)
    val pending = if (totalActive > 1) "($totalActive prompts pending)" else null
    return PromptTextPresentation(
        displayText = questionText.displayText,
        tooltipText = htmlTooltip(questionText.tooltipText, fileName?.let { "[$it]" }, pending),
    )
}

internal fun detailTooltipText(totalActive: Int): String {
    val pending = if (totalActive > 1) "<br>$totalActive prompts pending" else ""
    return buildString {
        append("<html>")
        append("Alt+Esc toggle focus")
        append("<br>Alt+1-9 or 1-9 select")
        append("<br>Esc dismiss")
        append(pending)
        append("</html>")
    }
}

internal fun buildPromptControlsRow(
    options: List<PromptOption>,
    buttonFont: Font,
    totalActive: Int,
    onAnswer: (Int) -> Unit,
): JPanel {
    val controls = JPanel().apply {
        layout = BoxLayout(this, BoxLayout.X_AXIS)
        isOpaque = false
    }

    for ((index, opt) in options.withIndex()) {
        val fullText = "[${opt.index}] ${opt.label}"
        val display = truncateSingleLineText(fullText, MAX_OPTION_LABEL_LEN)
        val answerIndex = index + 1
        val btn = JButton(display.displayText).apply {
            isFocusable = false
            font = font.deriveFont(buttonFont.size2D)
            toolTipText = display.tooltipText ?: fullText
            margin = Insets(2, 8, 2, 8)
            addActionListener { onAnswer(answerIndex) }
        }
        controls.add(btn)
        if (index != options.lastIndex) {
            controls.add(Box.createHorizontalStrut(6))
        }
    }

    controls.add(Box.createHorizontalStrut(8))
    controls.add(JLabel("?").apply {
        foreground = JBColor.GRAY
        font = font.deriveFont(Font.BOLD, buttonFont.size2D - 1f)
        toolTipText = detailTooltipText(totalActive)
    })

    return controls
}

internal fun lockSingleRowHeight(component: JComponent) {
    val preferred = component.preferredSize
    val lockedHeight = preferred.height
    component.minimumSize = Dimension(0, lockedHeight)
    component.maximumSize = Dimension(Int.MAX_VALUE, lockedHeight)
    component.preferredSize = Dimension(preferred.width, lockedHeight)
}

/**
 * Lightweight prompt panel at the bottom of the IDE window.
 *
 * - Panel appears without stealing focus (editor keeps cursor)
 * - **Alt+Esc** toggles focus: editor → panel (highlight) or panel → editor
 * - **1-9** to select an option (when panel is focused)
 * - **Alt+1..9** to select an option directly (works from any focus)
 * - **Esc** to dismiss (works from any focus)
 * - Click a button to select (always works)
 */
object PromptPanel {

    private var currentPanel: JPanel? = null
    private var currentFrame: JFrame? = null
    private var resizeListener: ComponentAdapter? = null
    private var previousFocusOwner: Component? = null
    private var autoFocusTimer: Timer? = null
    private var activityListener: java.awt.event.AWTEventListener? = null
    private val AUTO_FOCUS_DELAY_MS = 1000
    private val ESC_ACTION_KEY = "agentDocDismissPrompt"
    private val FOCUS_PANEL_KEY = "agentDocFocusPrompt"
    private val ALT_OPTION_KEY_PREFIX = "agentDocAltOption"

    fun show(project: Project, info: PromptInfo, fileName: String? = null,
             totalActive: Int = 1, onAnswer: (Int) -> Unit) {
        dismiss(project)

        val frame = WindowManager.getInstance().getFrame(project) ?: return
        val layeredPane = frame.layeredPane ?: return

        val panel = JPanel(BorderLayout(8, 0)).apply {
            border = BorderFactory.createCompoundBorder(
                BorderFactory.createMatteBorder(2, 0, 0, 0, BORDER_UNFOCUSED),
                BorderFactory.createEmptyBorder(PANEL_V_PADDING, PANEL_H_PADDING, PANEL_V_PADDING, PANEL_H_PADDING)
            )
            background = BG_UNFOCUSED
            isOpaque = true
        }

        // Derive font sizes from the IDE's editor font
        val editorFontSize = EditorColorsManager.getInstance()
            .globalScheme.editorFontSize.toFloat()
        val questionFontSize = editorFontSize + 1f
        val questionText = questionPresentation(info.question, fileName, totalActive)
        val questionLabel = JLabel(questionText.displayText).apply {
            font = font.deriveFont(Font.BOLD, questionFontSize)
            toolTipText = questionText.tooltipText
        }

        val options = info.options ?: return
        val rowPanel = JPanel(BorderLayout(8, 0)).apply {
            isOpaque = false
        }
        rowPanel.add(questionLabel, BorderLayout.CENTER)
        rowPanel.add(buildPromptControlsRow(options, questionLabel.font, totalActive) { optionIndex ->
            onAnswer(optionIndex)
            dismiss(project)
        }, BorderLayout.EAST)
        lockSingleRowHeight(rowPanel)
        panel.add(rowPanel, BorderLayout.CENTER)
        lockSingleRowHeight(panel)

        // --- Focus highlight ---
        // When panel gains focus: highlight border + brighter background
        // When panel loses focus: muted border + muted background
        panel.isFocusable = true
        panel.addFocusListener(object : FocusAdapter() {
            override fun focusGained(e: FocusEvent?) {
                // Remember the component that had focus before us (for Alt+Esc toggle back)
                val opposite = e?.oppositeComponent
                if (opposite != null && opposite !== panel) {
                    previousFocusOwner = opposite
                }
                panel.border = BorderFactory.createCompoundBorder(
                    BorderFactory.createMatteBorder(2, 0, 0, 0, BORDER_FOCUSED),
                    BorderFactory.createEmptyBorder(PANEL_V_PADDING, PANEL_H_PADDING, PANEL_V_PADDING, PANEL_H_PADDING)
                )
                panel.background = BG_FOCUSED
                panel.repaint()
            }
            override fun focusLost(e: FocusEvent?) {
                panel.border = BorderFactory.createCompoundBorder(
                    BorderFactory.createMatteBorder(2, 0, 0, 0, BORDER_UNFOCUSED),
                    BorderFactory.createEmptyBorder(PANEL_V_PADDING, PANEL_H_PADDING, PANEL_V_PADDING, PANEL_H_PADDING)
                )
                panel.background = BG_UNFOCUSED
                panel.repaint()
            }
        })

        // Panel-level key listener: 1-9 to select (only when panel focused)
        panel.addKeyListener(object : KeyAdapter() {
            override fun keyPressed(e: KeyEvent) {
                if (e.keyChar in '1'..'9') {
                    val idx = e.keyChar - '0'
                    val opt = options.withIndex().find { it.value.index == idx }
                    if (opt != null) {
                        onAnswer(opt.index + 1)
                        dismiss(project)
                    }
                }
            }
        })

        // --- Frame-level key bindings ---
        val rootPane = frame.rootPane
        val inputMap = rootPane.getInputMap(JComponent.WHEN_IN_FOCUSED_WINDOW)
        val actionMap = rootPane.actionMap

        // Esc to dismiss (always works)
        inputMap.put(KeyStroke.getKeyStroke(KeyEvent.VK_ESCAPE, 0), ESC_ACTION_KEY)
        actionMap.put(ESC_ACTION_KEY, object : AbstractAction() {
            override fun actionPerformed(e: java.awt.event.ActionEvent?) {
                dismiss(project)
            }
        })

        // Alt+Esc to toggle focus: editor → panel or panel → editor
        inputMap.put(
            KeyStroke.getKeyStroke(KeyEvent.VK_ESCAPE, KeyEvent.ALT_DOWN_MASK),
            FOCUS_PANEL_KEY
        )
        actionMap.put(FOCUS_PANEL_KEY, object : AbstractAction() {
            override fun actionPerformed(e: java.awt.event.ActionEvent?) {
                if (panel.isFocusOwner) {
                    // Panel has focus → return to previous component (editor)
                    val target = previousFocusOwner
                    if (target != null && target.isDisplayable) {
                        target.requestFocusInWindow()
                    }
                } else {
                    // Editor has focus → focus the panel
                    panel.requestFocusInWindow()
                }
            }
        })

        // Alt+1..9 to select options directly (works from any focus state)
        for (idx in 1..9) {
            val actionKey = "$ALT_OPTION_KEY_PREFIX$idx"
            inputMap.put(
                KeyStroke.getKeyStroke(KeyEvent.VK_0 + idx, KeyEvent.ALT_DOWN_MASK),
                actionKey
            )
            actionMap.put(actionKey, object : AbstractAction() {
                override fun actionPerformed(e: java.awt.event.ActionEvent?) {
                    val opt = options.withIndex().find { it.value.index == idx }
                    if (opt != null) {
                        onAnswer(opt.index + 1)
                        dismiss(project)
                    }
                }
            })
        }

        // Size and position at bottom of the layered pane.
        // Height stays fixed to a single row; width tracks the IDE window.
        fun layoutPanel(lp: JLayeredPane) {
            val h = panel.preferredSize.height
            panel.setSize(lp.width, h)
            panel.setLocation(0, lp.height - h)
        }
        layoutPanel(layeredPane)

        // Track frame resizes
        val listener = object : ComponentAdapter() {
            override fun componentResized(e: ComponentEvent?) {
                val lp = frame.layeredPane ?: return
                layoutPanel(lp)
            }
        }
        frame.addComponentListener(listener)

        layeredPane.add(panel, JLayeredPane.POPUP_LAYER)
        layeredPane.revalidate()
        layeredPane.repaint()
        // Don't steal focus — editor keeps cursor

        // Auto-focus after 2 seconds of inactivity.
        // Any key press or mouse click resets the timer.
        val timer = Timer(AUTO_FOCUS_DELAY_MS) {
            if (currentPanel === panel && !panel.isFocusOwner) {
                panel.requestFocusInWindow()
            }
        }
        timer.isRepeats = false

        val awtListener = object : java.awt.event.AWTEventListener {
            override fun eventDispatched(event: java.awt.AWTEvent?) {
                // Reset timer on any key or mouse activity
                if (currentPanel === panel && !panel.isFocusOwner) {
                    timer.restart()
                }
            }
        }
        Toolkit.getDefaultToolkit().addAWTEventListener(
            awtListener,
            java.awt.AWTEvent.KEY_EVENT_MASK or java.awt.AWTEvent.MOUSE_EVENT_MASK
        )
        activityListener = awtListener

        // Stop timer when panel gains focus; restart if focus leaves
        panel.addFocusListener(object : FocusAdapter() {
            override fun focusGained(e: FocusEvent?) { timer.stop() }
            override fun focusLost(e: FocusEvent?) {
                if (currentPanel === panel) timer.restart()
            }
        })

        timer.start()
        autoFocusTimer = timer

        currentPanel = panel
        currentFrame = frame
        resizeListener = listener
    }

    @Suppress("UNUSED_PARAMETER")
    fun dismiss(project: Project) {
        val panel = currentPanel ?: return
        val frame = currentFrame

        // Remove frame-level key bindings
        frame?.rootPane?.let { rootPane ->
            val inputMap = rootPane.getInputMap(JComponent.WHEN_IN_FOCUSED_WINDOW)
            inputMap.remove(KeyStroke.getKeyStroke(KeyEvent.VK_ESCAPE, 0))
            inputMap.remove(KeyStroke.getKeyStroke(KeyEvent.VK_ESCAPE, KeyEvent.ALT_DOWN_MASK))
            rootPane.actionMap.remove(ESC_ACTION_KEY)
            rootPane.actionMap.remove(FOCUS_PANEL_KEY)
            // Remove Alt+1..9 bindings
            for (idx in 1..9) {
                val actionKey = "$ALT_OPTION_KEY_PREFIX$idx"
                inputMap.remove(KeyStroke.getKeyStroke(KeyEvent.VK_0 + idx, KeyEvent.ALT_DOWN_MASK))
                rootPane.actionMap.remove(actionKey)
            }
        }

        // Stop auto-focus timer and remove activity listener
        autoFocusTimer?.stop()
        autoFocusTimer = null
        activityListener?.let { Toolkit.getDefaultToolkit().removeAWTEventListener(it) }
        activityListener = null

        // Remove resize listener
        resizeListener?.let { frame?.removeComponentListener(it) }
        resizeListener = null

        // Remove panel from layered pane
        panel.parent?.let { parent ->
            parent.remove(panel)
            parent.revalidate()
            parent.repaint()
        }

        currentPanel = null
        currentFrame = null
        previousFocusOwner = null
    }
}
