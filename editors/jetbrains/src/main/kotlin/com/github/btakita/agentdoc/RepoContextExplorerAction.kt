package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.google.gson.JsonObject
import com.intellij.openapi.Disposable
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.fileChooser.FileChooser
import com.intellij.openapi.fileChooser.FileChooserDescriptor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.ComboBox
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.ToolWindowAnchor
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiManager
import com.intellij.psi.PsiRecursiveElementWalkingVisitor
import com.intellij.ui.ColoredListCellRenderer
import com.intellij.ui.JBSplitter
import com.intellij.ui.SimpleTextAttributes
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBList
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.components.JBTextArea
import com.intellij.ui.components.JBTextField
import com.intellij.ui.content.ContentFactory
import com.intellij.util.ui.JBFont
import com.intellij.util.ui.JBUI
import java.awt.BorderLayout
import java.awt.FlowLayout
import java.awt.LayoutManager
import java.io.File
import java.nio.file.Files
import java.nio.file.Path
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.Future
import javax.swing.BoxLayout
import javax.swing.DefaultListModel
import javax.swing.JButton
import javax.swing.JComponent
import javax.swing.JList
import javax.swing.JPanel
import javax.swing.ListSelectionModel

/** Shell-forwarded action, available through Find Action and the Agent Doc Tools menu. */
class RepoContextExplorerAction : ImplAction {
    @Suppress("DEPRECATION") // The overload ties the registration to the plugin's project service.
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val manager = ToolWindowManager.getInstance(project)
        val parent = project.getService(ProjectPluginLifecycleService::class.java)
        val window = manager.getToolWindow(TITLE) ?: manager.registerToolWindow(
            TITLE, false, ToolWindowAnchor.RIGHT, parent,
        )
        if (window.contentManager.contentCount == 0) {
            val basePath = project.basePath ?: return
            val panel = RepoContextExplorerPanel(project, Path.of(basePath))
            val content = ContentFactory.getInstance().createContent(panel, "", false)
            content.setDisposer(panel)
            window.contentManager.addContent(content)
        }
        window.show()
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = e.project?.basePath != null
    }

    companion object {
        internal const val TITLE = "Repo Context Explorer"
    }
}

private data class SourceFingerprint(val hash: String, val lines: Int)
private data class SourceSelection(
    val anchor: RepoContextLocation,
    val source: VirtualFile,
    val fingerprint: SourceFingerprint,
    val related: List<RepoContextLocation>,
)
private data class SubmittedQuestion(val id: String, val session: VirtualFile)
private data class CurrentAnswer(val response: RepoContextAnswer, val stamp: Long)

internal open class RepoContextTypography(layout: LayoutManager) : JPanel(layout) {
    private var textComponents: List<JComponent>? = null

    protected fun useCompactFont(vararg components: JComponent) {
        textComponents = components.asList()
        refreshFont()
    }

    override fun updateUI() {
        super.updateUI()
        refreshFont()
    }

    private fun refreshFont() {
        val components = textComponents ?: return
        // Keep the local font when Swing subsequently updates the child delegates.
        val font = JBFont.regular().lessOn(2f)
        components.forEach { it.font = font }
    }
}

internal class RepoContextLocationRenderer : ColoredListCellRenderer<RepoContextLocation>() {
    override fun customizeCellRenderer(
        list: JList<out RepoContextLocation>,
        value: RepoContextLocation?,
        index: Int,
        selected: Boolean,
        hasFocus: Boolean,
    ) {
        font = list.font
        setIpad(JBUI.insets(3, 6))
        append(value?.toString().orEmpty(), SimpleTextAttributes.REGULAR_ATTRIBUTES)
    }
}

private class RepoContextExplorerPanel(private val project: Project, private val projectPath: Path) :
    RepoContextTypography(BorderLayout(JBUI.scale(8), JBUI.scale(8))), Disposable {

    private val mode = ComboBox(RepoContextMode.entries.toTypedArray())
    private val query = JBTextField()
    private val search = JButton("Search")
    private val cancel = JButton("Cancel")
    private val status = JBLabel("Search code symbols or Markdown headings.")
    private val resultsModel = DefaultListModel<RepoContextLocation>()
    private val results = JBList(resultsModel)
    private val relatedModel = DefaultListModel<RepoContextLocation>()
    private val related = JBList(relatedModel)
    private val sourceLabel = JBLabel("Select a search result.")
    private val openSource = JButton("Open source")
    private val openRelated = JButton("Open related source")
    private val chooseSession = JButton("Choose session")
    private val sessionLabel = JBLabel("No session selected.")
    private val question = JBTextField()
    private val ask = JButton("Ask about selection")
    private val answer = JBTextArea(7, 20).apply {
        isEditable = false
        lineWrap = true
        wrapStyleWord = true
        margin = JBUI.insets(4, 6)
    }
    private val openAnswer = JButton("Open answer")

    private var root: Path? = null
    private var selection: SourceSelection? = null
    private var session: VirtualFile? = FileEditorManager.getInstance(project).selectedFiles
        .firstOrNull(AgentDocSessionFiles::isSessionDocument)
    private var request: SubmittedQuestion? = null
    private var matchingAnswer: CurrentAnswer? = null
    private var background: Future<*>? = null
    private var taskRevision = 0L
    private var answerWork: Future<*>? = null
    @Volatile private var disposed = false

    init {
        border = JBUI.Borders.empty(8)
        query.emptyText.text = "Symbol, function, or type"
        question.emptyText.text = "Question about this source"
        mode.addActionListener {
            query.emptyText.text = if (mode.selectedItem == RepoContextMode.DOCS) "Heading or Markdown path"
                else "Symbol, function, or type"
            resultsModel.clear()
            status.text = "Enter a search."
        }
        results.selectionMode = ListSelectionModel.SINGLE_SELECTION
        results.cellRenderer = RepoContextLocationRenderer()
        related.selectionMode = ListSelectionModel.SINGLE_SELECTION
        related.cellRenderer = RepoContextLocationRenderer()
        add(JPanel(BorderLayout(JBUI.scale(6), 0)).apply {
            add(mode, BorderLayout.WEST)
            add(query, BorderLayout.CENTER)
            add(buttonRow(search, cancel), BorderLayout.EAST)
        }, BorderLayout.NORTH)
        add(JBSplitter(true, 0.35f).apply {
            firstComponent = JBScrollPane(results)
            secondComponent = JPanel(BorderLayout(0, JBUI.scale(5))).apply {
                add(JPanel().apply {
                    layout = BoxLayout(this, BoxLayout.Y_AXIS)
                    add(sourceLabel)
                    add(buttonRow(openSource, openRelated))
                }, BorderLayout.NORTH)
                add(JBScrollPane(related), BorderLayout.CENTER)
                add(JPanel().apply {
                    layout = BoxLayout(this, BoxLayout.Y_AXIS)
                    add(buttonRow(chooseSession, sessionLabel))
                    add(JPanel(BorderLayout(JBUI.scale(5), 0)).apply {
                        add(question, BorderLayout.CENTER)
                        add(ask, BorderLayout.EAST)
                    })
                    add(JBScrollPane(answer))
                    add(buttonRow(openAnswer))
                }, BorderLayout.SOUTH)
            }
        }, BorderLayout.CENTER)
        add(status, BorderLayout.SOUTH)
        useCompactFont(
            mode, query, search, cancel, status, results, related, sourceLabel, openSource, openRelated,
            chooseSession, sessionLabel, question, ask, answer, openAnswer,
        )
        search.addActionListener { search() }
        cancel.addActionListener {
            taskRevision++
            background?.cancel(true)
            background = null
            status.text = "Cancelled."
            updateControls()
        }
        query.addActionListener { search() }
        results.addListSelectionListener { event ->
            if (!event.valueIsAdjusting) {
                selection = null
                relatedModel.clear()
                sourceLabel.text = "Select a search result."
                updateControls()
                results.selectedValue?.let(::select)
            }
        }
        related.addListSelectionListener { if (!it.valueIsAdjusting) updateControls() }
        openSource.addActionListener { selection?.anchor?.let(::open) }
        openRelated.addActionListener { related.selectedValue?.let(::open) }
        chooseSession.addActionListener { chooseSession() }
        question.addActionListener { submitQuestion() }
        ask.addActionListener { submitQuestion() }
        openAnswer.addActionListener {
            val submitted = request ?: return@addActionListener
            val response = matchingAnswer ?: return@addActionListener
            val document = FileDocumentManager.getInstance().getCachedDocument(submitted.session)
            if (document?.modificationStamp != response.stamp) return@addActionListener
            OpenFileDescriptor(project, submitted.session, response.response.offset).navigate(true)
        }
        GenerationListeners.addDocumentListener(object : DocumentListener {
            override fun documentChanged(event: DocumentEvent) {
                val submitted = request ?: return
                if (FileDocumentManager.getInstance().getFile(event.document) == submitted.session) {
                    refreshAnswer(event.document, submitted)
                }
            }
        }, this)
        updateControls()
    }

    private fun buttonRow(vararg components: java.awt.Component) = JPanel(FlowLayout(FlowLayout.LEFT, JBUI.scale(4), 0))
        .apply { components.forEach(::add) }

    private fun updateControls() {
        val idle = background == null
        search.isEnabled = idle
        cancel.isVisible = !idle
        query.isEnabled = idle
        mode.isEnabled = idle
        results.isEnabled = idle
        related.isEnabled = idle
        openSource.isEnabled = idle && selection != null
        openRelated.isEnabled = idle && related.selectedValue != null
        chooseSession.isEnabled = idle
        ask.isEnabled = idle && selection != null
        question.isEnabled = ask.isEnabled
        openAnswer.isEnabled = matchingAnswer != null
        sessionLabel.text = session?.name ?: "No session selected."
    }

    private fun <T> runTask(loading: String, task: () -> T, success: (T) -> Unit) {
        if (background != null || disposed) return
        status.text = loading
        val revision = ++taskRevision
        background = GenerationWork.executeOnPooledThread {
            val outcome = runCatching(task)
            GenerationWork.invokeLater {
                if (disposed || project.isDisposed || taskRevision != revision) return@invokeLater
                background = null
                outcome.fold(
                    { value -> runCatching { success(value) }.onFailure { status.text = it.message ?: "Explorer failed." } },
                    { error -> status.text = error.message ?: "Explorer failed." },
                )
                updateControls()
            }
        }
        updateControls()
    }

    private fun search() {
        if (background != null) return
        val text = query.text.trim()
        if (text.isEmpty() || text.length > 256) {
            status.text = "Enter a search between 1 and 256 characters."
            return
        }
        val selectedMode = mode.selectedItem as RepoContextMode
        resultsModel.clear()
        runTask("Searching…", {
            val repo = gitRoot(projectPath)
            val found = if (selectedMode == RepoContextMode.DOCS && RepoContextModel.isMarkdown(text)) {
                val file = checkedFile(repo, text)
                outline(repo, file, "24")
            } else {
                val filters = if (selectedMode == RepoContextMode.DOCS) listOf("--lang", "markdown", "--kind", "heading")
                    else emptyList()
                val json = tsift(repo, listOf("search", "--json", "--limit", "24", "--no-tagpath", "--path",
                    repo.toString()) + filters + listOf("--", text))
                val symbols = RepoContextModel.anchors(json, repo, selectedMode)
                if (symbols.isEmpty() && selectedMode == RepoContextMode.CODE) RepoContextModel.codeMatches(json, repo, text)
                else symbols
            }
            repo to found
        }) { (repo, found) ->
            root = repo
            found.forEach(resultsModel::addElement)
            status.text = if (found.isEmpty()) "No results." else "${found.size} results."
        }
    }

    private fun select(anchor: RepoContextLocation) {
        val repo = root ?: return
        runTask("Reading source…", {
            val file = checkedFile(repo, anchor.path.toString())
            val virtual = virtualFile(file)
            val before = fingerprint(file, virtual)
            val (current, links) = if (anchor.kind == "heading") {
                val heading = outline(repo, file, "100").firstOrNull { it.name == anchor.name && it.line == anchor.line }
                    ?: error("The heading changed. Search again.")
                heading to markdownLinks(repo, file, virtual, heading)
            } else if (anchor.kind == "source match") {
                anchor to emptyList()
            } else {
                val info = tsift(repo, listOf("explain", "--json", "--no-tagpath", "--limit", "6", "--path",
                    repo.toString(), "--", anchor.name))
                val symbol = RepoContextModel.definitions(info, repo).firstOrNull {
                    it.path == anchor.path && it.name == anchor.name && it.kind == anchor.kind && it.line == anchor.line
                } ?: error("The symbol changed. Search again.")
                symbol to RepoContextModel.callSites(info, repo, symbol)
            }
            check(current.endLine <= before.lines && fingerprint(file, virtual) == before) {
                "The source changed while loading. Search again."
            }
            SourceSelection(current, virtual, before, links)
        }) { selected ->
            selection = selected
            sourceLabel.text = "${selected.anchor.path}:${selected.anchor.line}–${selected.anchor.endLine}"
            selected.related.forEach(relatedModel::addElement)
            status.text = if (selected.related.isEmpty()) "Source selected." else "${selected.related.size} related locations."
        }
    }

    private fun open(location: RepoContextLocation) {
        val repo = root ?: return
        runTask("Opening source…", { virtualFile(checkedFile(repo, location.path.toString())) }) { file ->
            val doc = FileDocumentManager.getInstance().getDocument(file) ?: error("The file cannot be opened.")
            check(location.line <= doc.lineCount) { "The source location changed. Search again." }
            OpenFileDescriptor(project, file, location.line - 1, 0).navigate(true)
            status.text = "Opened ${location.path}:${location.line}."
        }
    }

    private fun chooseSession(): VirtualFile? {
        val descriptor = FileChooserDescriptor(true, false, false, false, false, false).apply {
            title = "Choose an Agent Doc Session"
            withFileFilter { it.isDirectory || AgentDocSessionFiles.isSessionDocument(it) }
        }
        val file = FileChooser.chooseFile(descriptor, project, session) ?: return null
        session = file
        updateControls()
        return file
    }

    private fun submitQuestion() {
        val selected = selection ?: return
        val repo = root ?: return
        val text = question.text.trim()
        if (text.isEmpty() || text.length > 1_500) {
            status.text = "Enter a question between 1 and 1500 characters."
            return
        }
        val selectedSession = session ?: chooseSession() ?: return
        runTask("Checking the selected revision…", {
            check(checkedFile(repo, selectedSession.path).toString() == selectedSession.path) {
                "The session is outside this repository."
            }
            val file = checkedFile(repo, selected.anchor.path.toString())
            check(fingerprint(file, selected.source) == selected.fingerprint) {
                "The selected source changed. Search again before asking."
            }
        }) {
            val documents = FileDocumentManager.getInstance()
            check(!isUnsaved(selected.source)) { "Save the source and search again before asking." }
            val doc = documents.getDocument(selectedSession) ?: error("The session cannot be opened.")
            check(AgentDocSessionFiles.isSessionDocument(selectedSession) && !documents.isDocumentUnsaved(doc)) {
                "Choose a saved Agent Doc session in this repository."
            }
            val snapshot = doc.text
            val exchange = exchange(snapshot) ?: error("The session does not have one native exchange component.")
            val submitted = SubmittedQuestion(UUID.randomUUID().toString().replace("-", ""), selectedSession)
            val prompt = RepoContextModel.question(selected.anchor, selected.fingerprint.hash, text, submitted.id)
            WriteCommandAction.runWriteCommandAction(project, Runnable {
                check(doc.text == snapshot) { "The session changed. Try again." }
                doc.insertString(exchange.closeStart, RepoContextModel.insertion(snapshot, exchange, prompt))
            })
            documents.saveDocument(doc)
            check(!documents.isDocumentUnsaved(doc)) { "The question could not be saved. Run it from the editor after saving." }
            request = submitted
            matchingAnswer = null
            answer.text = "Question saved. Waiting for the matching answer."
            question.text = ""
            status.text = "Question saved. Running Agent Doc for ${selectedSession.name}."
            SubmitAction().submit(project, selectedSession)
        }
    }

    private fun refreshAnswer(doc: Document, submitted: SubmittedQuestion) {
        val snapshot = doc.text
        val revision = doc.modificationStamp
        answerWork?.cancel(false)
        answerWork = GenerationWork.executeOnPooledThread {
            val response = exchange(snapshot)?.let { RepoContextModel.answer(snapshot, it, submitted.id) }
            GenerationWork.invokeLater {
                if (disposed || project.isDisposed || request != submitted || doc.modificationStamp != revision) {
                    return@invokeLater
                }
                matchingAnswer = response?.let { CurrentAnswer(it, revision) }
                answer.text = response?.text ?: "Waiting for the matching answer in ${submitted.session.name}."
                answer.caretPosition = 0
                updateControls()
            }
        }
    }

    private fun exchange(snapshot: String): AgentDocComponentOutline.ComponentSpan? =
        NativePatching.componentSpansOrNull(snapshot)?.singleOrNull { it.name == "exchange" }

    private fun outline(repo: Path, file: Path, limit: String): List<RepoContextLocation> =
        RepoContextModel.anchors(tsift(repo, listOf("markdown-ast", "--json", "--max-items", limit, "--path",
            repo.toString(), "--", file.toString())), repo, RepoContextMode.DOCS, file)

    private fun markdownLinks(repo: Path, file: Path, virtual: VirtualFile, heading: RepoContextLocation): List<RepoContextLocation> {
        val result = resolvedLinks(virtual).mapNotNull { (line, target) ->
            val path = RepoContextModel.pathInRepo(repo, target.path)
            if (path == null || line !in heading.line..heading.endLine) null
            else RepoContextLocation(repo.relativize(path), 1, 1, "Links to ${repo.relativize(path)}", "Link")
        }.toMutableList()
        val candidates = tsift(repo, listOf("search", "--json", "--exact", "--lang", "markdown", "--no-tagpath",
            "--limit", "8", "--path", repo.toString(), "--", file.fileName.toString()))
        for (candidate in RepoContextModel.markdownFiles(candidates, repo)) {
            if (candidate == file) continue
            val referrer = LocalFileSystem.getInstance().findFileByNioFile(candidate) ?: continue
            val line = resolvedLinks(referrer).firstOrNull { (_, target) -> target == virtual }?.first ?: continue
            val path = repo.relativize(candidate)
            result += RepoContextLocation(path, line, line, "Links from $path", "Backlink")
        }
        return result.distinct().take(20)
    }

    private fun resolvedLinks(file: VirtualFile): List<Pair<Int, VirtualFile>> = ReadAction.compute<List<Pair<Int, VirtualFile>>, RuntimeException> {
        val psi = PsiManager.getInstance(project).findFile(file) ?: return@compute emptyList()
        val document = FileDocumentManager.getInstance().getDocument(file) ?: return@compute emptyList()
        val links = mutableSetOf<Pair<Int, VirtualFile>>()
        if (psi.language.id.equals("Markdown", ignoreCase = true)) {
            psi.accept(object : PsiRecursiveElementWalkingVisitor() {
                override fun visitElement(element: PsiElement) {
                    for (reference in element.references) {
                        val target = reference.resolve()?.containingFile?.virtualFile ?: continue
                        if (target != file) {
                            val offset = element.textRange.startOffset + reference.rangeInElement.startOffset
                            links += (document.getLineNumber(offset) + 1) to target
                        }
                    }
                    super.visitElement(element)
                }
            })
        }
        links.toList()
    }

    override fun dispose() {
        disposed = true
        background?.cancel(true)
        answerWork?.cancel(false)
    }

    companion object {
        private const val MAX_SOURCE = 4 * 1024 * 1024
        private const val TSIFT_TIMEOUT_MS = 45_000L

        private fun virtualFile(path: Path): VirtualFile =
            LocalFileSystem.getInstance().refreshAndFindFileByNioFile(path) ?: error("The source file is unavailable.")

        private fun isUnsaved(file: VirtualFile): Boolean = ReadAction.compute<Boolean, RuntimeException> {
            val manager = FileDocumentManager.getInstance()
            manager.getCachedDocument(file)?.let(manager::isDocumentUnsaved) == true
        }

        private fun fingerprint(path: Path, file: VirtualFile): SourceFingerprint {
            check(!isUnsaved(file)) { "Save the source file before selecting its context." }
            val bytes = Files.newInputStream(path).use { it.readNBytes(MAX_SOURCE + 1) }
            check(bytes.size <= MAX_SOURCE) { "The selected source exceeds 4 MiB." }
            val hash = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
            return SourceFingerprint(hash, bytes.count { it == '\n'.code.toByte() } + 1)
        }

        private fun gitRoot(dir: Path): Path =
            Path.of(run(listOf("git", "rev-parse", "--show-toplevel"), dir)).toRealPath()

        private fun checkedFile(root: Path, path: String): Path {
            val candidate = RepoContextModel.pathInRepo(root, path)?.toRealPath()
                ?: error("The source is outside this repository.")
            check(candidate.startsWith(root) && Files.isRegularFile(candidate) && gitRoot(candidate.parent) == root) {
                "The source is outside this repository."
            }
            return candidate
        }

        private fun tsift(root: Path, args: List<String>): JsonObject {
            val home = Path.of(System.getProperty("user.home"))
            val candidates = System.getenv("PATH").orEmpty().split(File.pathSeparator)
                .filter { it.isNotBlank() }.map { File(it, "tsift") } +
                listOf(".cargo/bin", ".local/bin", "bin").map { home.resolve(it).resolve("tsift").toFile() }
            val binary = candidates.firstOrNull { it.isFile && it.canExecute() }
                ?: error("tsift is not available on the IDE host.")
            return RepoContextModel.decode(run(listOf(binary.path) + args, root, TSIFT_TIMEOUT_MS))
        }

        // These read-only, operator-triggered queries are bounded one-shot CLI observations.
        // The editor, change listener, and normal SubmitAction own the live document lifecycle.
        private fun run(command: List<String>, root: Path, timeoutMs: Long = 10_000): String {
            val result = SyncLayoutAction.runCommandWithTimeout(command, root.toString(),
                timeoutMs = timeoutMs, captureStderrSeparately = true)
            check(result.exitCode == 0) {
                if (result.timedOut) "${command.first()} timed out." else
                    result.errorOutput.take(240).ifBlank { "${command.first()} failed (${result.exitCode})." }
            }
            return result.output
        }
    }
}
