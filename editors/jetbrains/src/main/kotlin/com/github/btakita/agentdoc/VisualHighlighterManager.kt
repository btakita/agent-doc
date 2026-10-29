package com.github.btakita.agentdoc

import com.intellij.codeInsight.daemon.DaemonCodeAnalyzer
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.application.ex.ApplicationEx
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.CaretEvent
import com.intellij.openapi.editor.event.CaretListener
import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.editor.event.EditorFactoryEvent
import com.intellij.openapi.editor.event.EditorFactoryListener
import com.intellij.openapi.editor.markup.EffectType
import com.intellij.openapi.editor.markup.GutterIconRenderer
import com.intellij.openapi.editor.markup.HighlighterLayer
import com.intellij.openapi.editor.markup.HighlighterTargetArea
import com.intellij.openapi.editor.markup.RangeHighlighter
import com.intellij.openapi.editor.markup.TextAttributes
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiDocumentManager
import com.github.btakita.agentdoc.shell.ShellQueueReportGutterIconRenderer
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileContentChangeEvent
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import java.awt.Color
import java.awt.Font
import java.awt.Graphics
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import javax.swing.Icon
import javax.swing.SwingUtilities
import javax.swing.UIManager

/**
 * What one agent-doc range highlighter covers and how it paints. [attributes] is
 * compared by value, so a changed color or font style is re-applied.
 */
internal data class VisualHighlightKey(
    val start: Int,
    val end: Int,
    val layer: Int,
    val kind: String,
    val attributes: Any?,
) {
    /** GH #339 R1: the identity a highlighter carries, as JDK values only (never this class). */
    fun toUserData(): List<Any?> = listOf(start, end, layer, kind, attributes)

    companion object {
        fun fromUserData(data: List<Any?>): VisualHighlightKey? {
            if (data.size != 5) return null
            val start = data[0] as? Int ?: return null
            val end = data[1] as? Int ?: return null
            val layer = data[2] as? Int ?: return null
            val kind = data[3] as? String ?: return null
            return VisualHighlightKey(start, end, layer, kind, data[4])
        }
    }
}

internal data class VisualHighlightPlan<H>(
    val remove: List<H>,
    val add: List<VisualHighlightKey>,
)

/**
 * Match live highlighters to the desired tokens as a multiset: every existing
 * highlighter whose current identity equals a desired token is kept untouched,
 * every unmatched or invalid one (`null` identity) is removed, and only the
 * remaining desired tokens are added. Unchanged text therefore costs no markup
 * mutation at all (`#keepidesnappy`).
 */
internal fun <H> planVisualHighlightReconcile(
    existing: List<Pair<H, VisualHighlightKey?>>,
    desired: List<VisualHighlightKey>,
): VisualHighlightPlan<H> {
    val unmatched = HashMap<VisualHighlightKey, ArrayDeque<H>>()
    val remove = ArrayList<H>()
    for ((highlighter, key) in existing) {
        if (key == null) {
            remove.add(highlighter)
        } else {
            unmatched.getOrPut(key) { ArrayDeque() }.addLast(highlighter)
        }
    }
    val add = ArrayList<VisualHighlightKey>()
    for (key in desired) {
        val candidates = unmatched[key]
        if (candidates != null && candidates.isNotEmpty()) {
            candidates.removeFirst()
        } else {
            add.add(key)
        }
    }
    unmatched.values.forEach { remove.addAll(it) }
    return VisualHighlightPlan(remove, add)
}

internal fun recentPresentationKinds(kind: String, dismissed: Boolean): List<String> {
    val rank = kind.removePrefix("recent_").toIntOrNull()?.takeIf { it in 1..3 }
        ?: return listOf(kind)
    return buildList {
        add("recent_rank_$rank")
        if (!dismissed) add("recent_highlight_$rank")
    }
}

private class RecencyRankIcon(
    private val glyph: String,
    private val color: Color,
) : Icon {
    override fun getIconWidth(): Int = 16
    override fun getIconHeight(): Int = 16

    override fun paintIcon(component: java.awt.Component?, graphics: Graphics, x: Int, y: Int) {
        val font = (UIManager.getFont("Label.font") ?: Font(Font.SANS_SERIF, Font.PLAIN, 12))
            .deriveFont(Font.BOLD, 13f)
        graphics.font = font
        graphics.color = color
        val metrics = graphics.fontMetrics
        graphics.drawString(
            glyph,
            x + (iconWidth - metrics.stringWidth(glyph)) / 2,
            y + (iconHeight - metrics.height) / 2 + metrics.ascent,
        )
    }
}

private class RecencyRankGutterRenderer(
    private val rank: Int,
    private val color: Color,
) : GutterIconRenderer() {
    override fun getIcon(): Icon = RecencyRankIcon(listOf("①", "②", "③")[rank - 1], color)
    override fun getTooltipText(): String =
        "${listOf("Newest", "Second-newest", "Third-newest")[rank - 1]} recently addressed agent-doc row"
    override fun getAlignment(): Alignment = Alignment.LEFT
    override fun equals(other: Any?): Boolean =
        other is RecencyRankGutterRenderer && other.rank == rank && other.color == color
    override fun hashCode(): Int = 31 * rank + color.hashCode()
}

class VisualHighlighterManager private constructor(private val project: Project) : Disposable {
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(VisualHighlighterManager::class.java)

    private data class VisualDocumentText(
        val text: String,
        val modificationStamp: Long,
    )

    private data class VisualTokenSnapshot(
        val modificationStamp: Long,
        val tokens: List<NativePatching.VisualToken>,
        /**
         * GH #19: native component boundaries for the same text. Empty for a non-session
         * document (removes its outline); null when the native parse was unavailable or the
         * document is mid-edit unparseable, which keeps the last tracked outline.
         */
        val components: List<AgentDocComponentOutline.ComponentSpan>?,
    )

    /** Component boundaries consumed by folding, gutter markers, and the structure view. */
    internal val componentOutlines = ComponentOutlineStore()

    private val refreshExecutor = GenerationWork.newSingleThreadScheduledExecutor("agent-doc-visual-highlighter-events")
    private val pendingRefreshes = ConcurrentHashMap<Document, ScheduledFuture<*>>()
    private val disposed = AtomicBoolean(false)
    private val refreshLifecycleLock = Any()

    init {
        GenerationListeners.addDocumentListener(object : DocumentListener {
            override fun documentChanged(event: DocumentEvent) {
                if (!isMarkdown(event.document)) return
                scheduleRefresh(event.document)
            }
        }, this)
        GenerationListeners.addEditorFactoryListener(object : EditorFactoryListener {
            override fun editorCreated(event: EditorFactoryEvent) {
                if (!isMarkdown(event.editor.document)) return
                installCaretDismissal(event.editor)
                scheduleRefresh(event.editor.document)
            }
        }, this)
        GenerationListeners.subscribe(
            project,
            FileEditorManagerListener.FILE_EDITOR_MANAGER,
            object : FileEditorManagerListener {
                override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
                    refreshMarkdownFileAfterEditorEvent(file)
                }

                override fun selectionChanged(event: FileEditorManagerEvent) {
                    event.newFile?.let { refreshMarkdownFileAfterEditorEvent(it) }
                }
            },
            this,
        )
        GenerationListeners.subscribe(
            project,
            VirtualFileManager.VFS_CHANGES,
            object : BulkFileListener {
                override fun after(events: List<VFileEvent>) {
                    for (event in events) {
                        if (event !is VFileContentChangeEvent) continue
                        val file = event.file
                        if (!file.name.endsWith(".md")) continue
                        reparseAndRefresh(file)
                    }
                }
            },
            this,
        )
        refreshAll()
    }

    private fun reparseAndRefresh(file: VirtualFile) {
        val document = FileDocumentManager.getInstance().getDocument(file) ?: return
        scheduleRefresh(document)
    }

    private fun refreshMarkdownFileAfterEditorEvent(file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        GenerationWork.invokeLater {
            if (disposed.get() || project.isDisposed) return@invokeLater
            refreshFile(file)
        }
    }

    private fun scheduleRefresh(document: Document) {
        if (!isMarkdown(document)) return
        synchronized(refreshLifecycleLock) {
            if (disposed.get() || project.isDisposed || refreshExecutor.isShutdown) return
            pendingRefreshes.remove(document)?.cancel(false)
            lateinit var future: ScheduledFuture<*>
            try {
                future = refreshExecutor.schedule({
                    try {
                        if (disposed.get() || project.isDisposed) return@schedule
                        val snapshot = collectVisualTokens(document)
                        if (snapshot != null) {
                            GenerationWork.invokeLater {
                                if (disposed.get() || project.isDisposed) return@invokeLater
                                applyVisualTokens(document, snapshot)
                            }
                        }
                    } finally {
                        pendingRefreshes.remove(document, future)
                    }
                }, 120, TimeUnit.MILLISECONDS)
            } catch (_: RejectedExecutionException) {
                // Defense in depth for an executor implementation that rejects
                // despite the lifecycle lock. The replacement generation owns refreshes.
                return
            }
            pendingRefreshes[document] = future
        }
    }

    private fun refreshAll() {
        EditorFactory.getInstance().allEditors
            .filter { it.project == project }
            .forEach {
                installCaretDismissal(it)
                scheduleRefresh(it.document)
            }
    }

    fun refreshFile(file: VirtualFile) {
        val document = FileDocumentManager.getInstance().getDocument(file) ?: return
        scheduleRefresh(document)
    }

    private fun collectVisualTokens(document: Document): VisualTokenSnapshot? {
        return try {
            val application = ApplicationManager.getApplication()
            val snapshot =
                if (application.isReadAccessAllowed) {
                    if (!isMarkdown(document)) return null
                    VisualDocumentText(
                        text = document.text,
                        modificationStamp = document.modificationStamp,
                    )
                } else if (SwingUtilities.isEventDispatchThread()) {
                    // `#edt-no-implicit-read`: the EDT does not carry an implicit
                    // read lock on modern IntelliJ platforms, so reading
                    // `document.text` here asserts instead of succeeding. Take the
                    // read action explicitly. Unlike the background branch below
                    // this may block, which is correct on the EDT: write actions
                    // also run on the EDT, so no writer can be holding the lock.
                    ReadAction.compute<VisualDocumentText?, RuntimeException> {
                        if (isMarkdown(document)) {
                            VisualDocumentText(
                                text = document.text,
                                modificationStamp = document.modificationStamp,
                            )
                        } else {
                            null
                        }
                    } ?: return null
                } else {
                    val applicationEx = application as? ApplicationEx ?: return null
                    val result = AtomicReference<VisualDocumentText?>()
                    if (
                        !applicationEx.tryRunReadAction {
                            if (isMarkdown(document)) {
                                result.set(
                                    VisualDocumentText(
                                        text = document.text,
                                        modificationStamp = document.modificationStamp,
                                    ),
                                )
                            }
                        }
                    ) {
                        return null
                    }
                    result.get() ?: return null
                }
            VisualTokenSnapshot(
                modificationStamp = snapshot.modificationStamp,
                // FFI unavailability is not an empty token projection. Keeping
                // the last valid ranges avoids erasing agent-doc highlighting
                // while a restart-required native generation is recovered.
                tokens = NativePatching.visualTokensOrNull(snapshot.text) ?: return null,
                components =
                    if (isAgentDocDocumentTextUtil(snapshot.text)) {
                        NativePatching.componentSpansOrNull(snapshot.text)
                    } else {
                        emptyList()
                    },
            )
        } catch (e: Throwable) {
            LOG.debug("[visual] token refresh skipped: ${e.message}")
            null
        }
    }

    private fun applyVisualTokens(document: Document, snapshot: VisualTokenSnapshot) {
        if (!isMarkdown(document)) return
        if (document.modificationStamp != snapshot.modificationStamp) return
        EditorFactory.getInstance().getEditors(document, project).forEach { refreshEditor(it, snapshot.tokens) }
        snapshot.components?.let { installComponentOutline(document, it) }
    }

    /**
     * GH #19: publish the outline and, only when its structure changed, restart the daemon for
     * this file so folding and gutter markers are rebuilt. Offset drift alone needs no restart:
     * the store's range markers already track it.
     */
    private fun installComponentOutline(
        document: Document,
        spans: List<AgentDocComponentOutline.ComponentSpan>,
    ) {
        val changed =
            try {
                componentOutlines.install(document, spans)
            } catch (e: Throwable) {
                LOG.debug("[visual] component outline install skipped: ${e.message}")
                false
            }
        if (!changed) return
        val psiFile = PsiDocumentManager.getInstance(project).getPsiFile(document) ?: return
        DaemonCodeAnalyzer.getInstance(project).restart(psiFile)
    }

    /**
     * `#keepidesnappy`: reconcile instead of rebuild. This runs on the EDT after
     * every 120 ms typing pause; tearing down and re-adding every agent-doc
     * highlighter (hundreds on a long session document) fired a markup event and
     * a repaint per highlighter while the operator typed. Range markers already
     * track offset drift, so after a keystroke almost every highlighter still
     * matches its token and only the changed ones are touched.
     */
    private fun refreshEditor(editor: Editor, tokens: List<NativePatching.VisualToken>) {
        if (editor.isDisposed) return
        if (!isMarkdown(editor.document)) {
            clearEditor(editor)
            return
        }

        val markup = editor.markupModel
        val textLength = editor.document.textLength
        val documentText = editor.document.text
        val attrsByKind = HashMap<String, TextAttributes>()
        val recentIds = tokens
            .filter { it.kind.startsWith("recent_") }
            .sortedBy { it.kind }
            .mapNotNull { recentTokenId(documentText, it.start, it.end) }
        val recencyState = recencyState(editor, recentIds)
        @Suppress("UNCHECKED_CAST")
        val dismissed = recencyState["dismissed"] as MutableSet<String>
        val desired =
            tokens
                .filter { it.end > it.start && it.end <= textLength }
                .flatMap { token ->
                    val id = recentTokenId(documentText, token.start, token.end)
                    recentPresentationKinds(token.kind, id != null && id in dismissed).map { kind ->
                        VisualHighlightKey(
                            start = token.start,
                            end = token.end,
                            layer = layerFor(kind),
                            kind = kind,
                            attributes = attrsByKind.getOrPut(kind) { attrsFor(editor, kind) },
                        )
                    }
                }
        // First refresh of this editor: nothing is tracked yet, so sweep any
        // agent-doc highlighter it carries (one full pass) before reconciling.
        val tracked =
            editor.getUserData(TRACKED_HIGHLIGHTERS_KEY) ?: run {
                clearEditor(editor)
                emptyList()
            }
        val plan =
            planVisualHighlightReconcile(
                tracked.map { highlighter -> highlighter to highlighter.visualHighlightKey() },
                desired,
            )
        for (highlighter in plan.remove) {
            if (highlighter.isValid) markup.removeHighlighter(highlighter)
        }
        val removed = plan.remove.toHashSet()
        val next = ArrayList<RangeHighlighter>(desired.size)
        tracked.filterTo(next) { it !in removed }
        for (key in plan.add) {
            val highlighter = markup.addRangeHighlighter(
                key.start,
                key.end,
                key.layer,
                key.attributes as TextAttributes,
                HighlighterTargetArea.EXACT_RANGE,
            )
            highlighter.putUserData(HIGHLIGHTER_KEY, true)
            highlighter.putUserData(HIGHLIGHT_IDENTITY_KEY, key.toUserData())
            if (key.kind.startsWith("recent_rank_")) {
                val rank = key.kind.removePrefix("recent_rank_").toInt()
                highlighter.gutterIconRenderer = RecencyRankGutterRenderer(
                    rank,
                    editor.colorsScheme.defaultForeground,
                )
            } else {
                queueReportPresentation(key.kind)?.let { (glyph, tooltip) ->
                    highlighter.gutterIconRenderer = ShellQueueReportGutterIconRenderer(glyph, tooltip)
                    highlighter.errorStripeTooltip = tooltip
                }
            }
            next.add(highlighter)
        }
        editor.putUserData(TRACKED_HIGHLIGHTERS_KEY, next)
    }

    private fun installCaretDismissal(editor: Editor) {
        if (editor.getUserData(RECENCY_CARET_LISTENER_KEY) == true) return
        editor.putUserData(RECENCY_CARET_LISTENER_KEY, true)
        editor.caretModel.addCaretListener(object : CaretListener {
            override fun caretPositionChanged(event: CaretEvent) {
                dismissRecentHighlightAt(
                    event.editor,
                    event.caret?.offset ?: event.editor.caretModel.offset,
                )
            }
        }, this)
    }

    /**
     * Visiting a recent row acknowledges only its transient background wash.
     * The gutter rank and durable document annotation remain untouched.
     */
    private fun dismissRecentHighlightAt(editor: Editor, offset: Int) {
        val tracked = editor.getUserData(TRACKED_HIGHLIGHTERS_KEY) ?: return
        val matching = tracked.filter { highlighter ->
            val key = highlighter.visualHighlightKey() ?: return@filter false
            key.kind.startsWith("recent_highlight_") && offset >= key.start && offset <= key.end
        }
        if (matching.isEmpty()) return

        val state = editor.getUserData(RECENCY_STATE_KEY) ?: return
        @Suppress("UNCHECKED_CAST")
        val dismissed = state["dismissed"] as? MutableSet<String> ?: return
        val documentText = editor.document.text
        matching.forEach { highlighter ->
            recentTokenId(documentText, highlighter.startOffset, highlighter.endOffset)?.let(dismissed::add)
            if (highlighter.isValid) editor.markupModel.removeHighlighter(highlighter)
        }
        val removed = matching.toHashSet()
        editor.putUserData(TRACKED_HIGHLIGHTERS_KEY, tracked.filterNot(removed::contains))
    }

    /** Reset transient acknowledgements only when the editor-derived top three changes. */
    private fun recencyState(editor: Editor, projection: List<String>): MutableMap<String, Any> {
        val existing = editor.getUserData(RECENCY_STATE_KEY)
        @Suppress("UNCHECKED_CAST")
        val existingProjection = existing?.get("projection") as? List<String>
        if (existing != null && existingProjection == projection) return existing

        val replacement: MutableMap<String, Any> = hashMapOf(
            "projection" to ArrayList(projection),
            "dismissed" to HashSet<String>(),
        )
        editor.putUserData(RECENCY_STATE_KEY, replacement)
        return replacement
    }

    private fun recentTokenId(text: String, start: Int, end: Int): String? {
        if (start !in 0..text.length || end !in start..text.length) return null
        val annotationStart = text.indexOf(ADDRESSED_AT_PREFIX, start)
        if (annotationStart < start || annotationStart >= end) return null
        val annotationEnd = text.indexOf('⟧', annotationStart + ADDRESSED_AT_PREFIX.length)
        if (annotationEnd < 0 || annotationEnd >= end) return null
        return text.substring(annotationStart, annotationEnd + 1)
    }

    /** The identity this highlighter was created with, or null once it no longer matches it. */
    private fun RangeHighlighter.visualHighlightKey(): VisualHighlightKey? {
        if (!isValid) return null
        val created = getUserData(HIGHLIGHT_IDENTITY_KEY)?.let(VisualHighlightKey::fromUserData) ?: return null
        return created.copy(start = startOffset, end = endOffset)
    }

    private fun clearEditor(editor: Editor) {
        val markup = editor.markupModel
        editor.putUserData(TRACKED_HIGHLIGHTERS_KEY, null)
        markup.allHighlighters
            .filter { it.getUserData(HIGHLIGHTER_KEY) == true }
            .forEach { markup.removeHighlighter(it) }
    }

    private fun isMarkdown(document: Document): Boolean {
        val file = FileDocumentManager.getInstance().getFile(document) ?: return false
        return file.name.endsWith(".md")
    }

    private fun attrsFor(editor: Editor, kind: String): TextAttributes {
        fun baseAttrs(base: TextAttributes?): TextAttributes {
            val fgBase = MarkdownStyleSettings.foregroundFor(
                kind,
                com.intellij.ui.JBColor(
                    base?.foregroundColor ?: editor.colorsScheme.defaultForeground,
                    base?.foregroundColor ?: editor.colorsScheme.defaultForeground,
                ),
            )
            return TextAttributes(
                fgBase,
                base?.backgroundColor,
                base?.effectColor,
                base?.effectType,
                MarkdownStyleSettings.fontStyleFor(kind, base?.fontType ?: Font.PLAIN),
            )
        }

        return when (kind) {
            "component_body" -> baseAttrs(null).apply {
                // The element body must read as a distinct block. Inheriting
                // the raw editor background (0.2.344) painted an invisible
                // highlighter; tint toward the theme foreground instead so the
                // wash stays neutral (no theme-green accent) yet visible.
                backgroundColor = MarkdownStyleSettings.backgroundFor(
                    kind,
                    componentBodyBackground(
                        editor.colorsScheme.defaultBackground,
                        editor.colorsScheme.defaultForeground,
                    ),
                )
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.PLAIN)
            }
            "scratch_comment_body" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.BLOCK_COMMENT)
            ).apply {
                backgroundColor = MarkdownStyleSettings.backgroundFor(kind, mutedBackground(editor, foregroundColor))
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.ITALIC)
            }
            "component_open", "component_close" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.METADATA)
            ).apply {
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.BOLD)
            }
            "patch_open", "patch_close" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.MARKUP_ATTRIBUTE)
            ).apply {
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.BOLD)
            }
            "boundary" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.KEYWORD)
            ).apply {
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.ITALIC)
            }
            "scratch_comment" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.BLOCK_COMMENT)
            ).apply {
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.ITALIC)
            }
            // #editor-bold-markdown-rendering: render markdown emphasis inline.
            "bold" -> baseAttrs(null)
                .apply { fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.BOLD) }
            "italic" -> baseAttrs(null)
                .apply { fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.ITALIC) }
            "prompt" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.STRING)
            ).apply {
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.BOLD)
            }
            "response_heading" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.FUNCTION_DECLARATION)
            ).apply {
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.BOLD)
            }
            "recent_highlight_1", "recent_highlight_2", "recent_highlight_3" -> baseAttrs(null).apply {
                val rank = kind.removePrefix("recent_highlight_").toInt()
                val ratio = listOf(0.24f, 0.16f, 0.10f)[rank - 1]
                val accent = editor.colorsScheme
                    .getAttributes(DefaultLanguageHighlighterColors.FUNCTION_DECLARATION)
                    ?.foregroundColor ?: editor.colorsScheme.defaultForeground
                backgroundColor = blend(editor.colorsScheme.defaultBackground, accent, ratio)
            }
            "recent_rank_1", "recent_rank_2", "recent_rank_3" -> TextAttributes()
            "tracked_id" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.CONSTANT)
            ).apply {
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.BOLD)
                effectType = EffectType.ROUNDED_BOX
                effectColor = foregroundColor
                backgroundColor = MarkdownStyleSettings.backgroundFor(kind, mutedBackground(editor, foregroundColor))
            }
            "label_tag" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.MARKUP_ATTRIBUTE)
            ).apply {
                fontType = MarkdownStyleSettings.fontStyleFor(kind, Font.BOLD)
                effectType = EffectType.ROUNDED_BOX
                effectColor = foregroundColor
                backgroundColor = MarkdownStyleSettings.backgroundFor(kind, mutedBackground(editor, foregroundColor))
            }
            else -> TextAttributes()
        }
    }

    private fun layerFor(kind: String): Int =
        when (kind) {
            "component_body", "scratch_comment_body" -> HighlighterLayer.ADDITIONAL_SYNTAX - 1
            "recent_highlight_1", "recent_highlight_2", "recent_highlight_3" ->
                HighlighterLayer.ADDITIONAL_SYNTAX + 1
            else -> HighlighterLayer.ADDITIONAL_SYNTAX
        }

    private fun queueReportPresentation(kind: String): Pair<String, String>? =
        when (kind) {
            "queue_report_local" -> "✎" to "Worker-reported local changes"
            "queue_report_published" -> "⇧" to "Worker-reported published state"
            "queue_report_published_local" -> "⇧✎" to "Published, then changed locally again"
            else -> null
        }

    private fun mutedBackground(editor: Editor, accent: Color?): Color {
        val base = editor.colorsScheme.defaultBackground
        return blend(base, accent ?: base, 0.10f)
    }

    override fun dispose() {
        if (!disposed.compareAndSet(false, true)) return
        synchronized(refreshLifecycleLock) {
            pendingRefreshes.values.forEach { it.cancel(false) }
            pendingRefreshes.clear()
            refreshExecutor.shutdownNow()
        }
        componentOutlines.clear()
        EditorFactory.getInstance().allEditors
            .filter { it.project == project }
            .forEach { clearEditor(it) }
    }

    companion object {
        // GH #339 R1: shell-created keys holding JDK and platform values only.
        private val HIGHLIGHTER_KEY = GenerationKeys.create<Boolean>("agent-doc.visual.highlighter")
        private val HIGHLIGHT_IDENTITY_KEY = GenerationKeys.create<List<Any?>>("agent-doc.visual.highlight-identity")
        private val TRACKED_HIGHLIGHTERS_KEY = GenerationKeys.create<List<RangeHighlighter>>("agent-doc.visual.tracked-highlighters")
        private val RECENCY_STATE_KEY = GenerationKeys.create<MutableMap<String, Any>>("agent-doc.visual.recency-state")
        private val RECENCY_CARET_LISTENER_KEY = GenerationKeys.create<Boolean>("agent-doc.visual.recency-caret-listener")
        private val INSTANCES = ConcurrentHashMap<Project, VisualHighlighterManager>()
        private const val ADDRESSED_AT_PREFIX = "⟦addressed-at:"

        /** Share of the theme foreground mixed into a component body's background. */
        internal const val COMPONENT_BODY_TINT = 0.08f

        /**
         * Default (non-overridden) background for `component_body` highlighters:
         * the editor background tinted toward the editor foreground, so element
         * content is visibly distinct in both light and dark schemes.
         */
        internal fun componentBodyBackground(background: Color, foreground: Color): Color =
            blend(background, foreground, COMPONENT_BODY_TINT)

        internal fun blend(base: Color, accent: Color, accentRatio: Float): Color {
            val clamped = accentRatio.coerceIn(0f, 1f)
            val baseRatio = 1f - clamped
            return Color(
                (base.red * baseRatio + accent.red * clamped).toInt().coerceIn(0, 255),
                (base.green * baseRatio + accent.green * clamped).toInt().coerceIn(0, 255),
                (base.blue * baseRatio + accent.blue * clamped).toInt().coerceIn(0, 255),
            )
        }

        /** The live instance for [project] without creating one (extension callbacks). */
        fun peek(project: Project): VisualHighlighterManager? = INSTANCES[project]?.takeUnless { it.disposed.get() }

        fun getInstance(project: Project): VisualHighlighterManager {
            return INSTANCES.computeIfAbsent(project) { VisualHighlighterManager(it) }
        }

        fun disposeProject(project: Project) {
            INSTANCES.remove(project)?.dispose()
        }
    }
}
