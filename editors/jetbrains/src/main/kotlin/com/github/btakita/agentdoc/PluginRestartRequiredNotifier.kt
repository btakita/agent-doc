package com.github.btakita.agentdoc

import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * A newer Agent Doc package is on disk (or staged) while this IDE still runs the older one.
 *
 * [source] is `installed_jar` when the plugin tree on disk already holds [pendingVersion], or
 * `staged` when agent-doc only queued it for the next IDE start.
 */
internal data class PluginRestartRequired(
    val loadedVersion: String,
    val pendingVersion: String,
    val source: String,
    val reason: String?,
)

/** Mirrors `agent_doc_fs::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER`; it sits beside the plugin tree. */
internal const val PLUGIN_RESTART_REQUIRED_MARKER = ".agent-doc-jetbrains-restart-required"

/** Mirrors `agent_doc_fs::jetbrains_install::STAGED_VERSION_MARKER_PREFIX`. */
private const val STAGED_VERSION_MARKER_PREFIX = "staged_version="

/** Mirrors `agent_doc_fs::jetbrains_install::PREVIOUS_VERSION_MARKER_PREFIX`. */
private const val PREVIOUS_VERSION_MARKER_PREFIX = "previous_version="

/** Versioned plugin jar prefixes of the three package lines (classic, legacy 263, modular). */
private val VERSIONED_PLUGIN_JAR_PREFIXES =
    listOf("agent-doc-jetbrains-263-", "agent-doc-jetbrains-", "agent.doc-")

private const val RESTART_REASON_DISPLAY_LIMIT = 240

private fun numericDotVersion(version: String): List<Int>? {
    if (version.isEmpty()) return null
    val parts = version.split('.')
    if (parts.any { part -> part.isEmpty() || !part.all(Char::isDigit) }) return null
    return parts.map { part -> part.toIntOrNull() ?: return null }
}

private fun compareDotVersions(left: List<Int>, right: List<Int>): Int {
    for (index in 0 until maxOf(left.size, right.size)) {
        val difference = left.getOrElse(index) { 0 }.compareTo(right.getOrElse(index) { 0 })
        if (difference != 0) return difference
    }
    return 0
}

/**
 * The plugin version named by a versioned plugin jar in the installed tree's `lib/` directory, or
 * null when no file there is one. Unversioned module jars (`lib/modules/agent.doc.backend.jar`)
 * never match. Several matches (a half-swapped tree) resolve to the newest.
 */
internal fun installedPluginJarVersion(libFileNames: Collection<String>): String? =
    libFileNames
        .mapNotNull { name ->
            VERSIONED_PLUGIN_JAR_PREFIXES.firstNotNullOfOrNull { prefix ->
                name.removePrefix(prefix)
                    .takeIf { name.startsWith(prefix) && it.endsWith(".jar") }
                    ?.removeSuffix(".jar")
                    ?.takeIf { numericDotVersion(it) != null }
            }
        }
        .maxWithOrNull { left, right ->
            compareDotVersions(numericDotVersion(left)!!, numericDotVersion(right)!!)
        }

/**
 * Whether this IDE must restart to load a different Agent Doc generation than the one it runs.
 *
 * The installed jar is the primary evidence: agent-doc replaced the plugin tree on disk, but the
 * running JVM keeps the classes it already loaded (the exact-262/263 modular packages have no
 * restart-free swap; see `editors/jetbrains/SPEC.md`). Any version other than the loaded one,
 * including a downgrade, needs the restart. A staged install leaves the old tree in place and
 * names its package in the restart marker; that counts only when it is newer than the loaded
 * generation, because a marker whose restart already happened can outlive the swap it described.
 * The marker's first line is the installer's reason and is reported only alongside one of those
 * two version facts, never as a trigger by itself.
 */
internal fun pluginRestartRequiredVerdict(
    loadedVersion: String?,
    installedJarVersion: String?,
    markerBody: String?,
): PluginRestartRequired? {
    val loaded = loadedVersion?.trim()?.takeIf { it.isNotEmpty() && it != "unknown" } ?: return null
    val reason = markerBody
        ?.lineSequence()
        ?.firstOrNull()
        ?.trim()
        ?.takeIf { line ->
            line.isNotEmpty() &&
                !line.startsWith(STAGED_VERSION_MARKER_PREFIX) &&
                !line.startsWith(PREVIOUS_VERSION_MARKER_PREFIX)
        }
    val installed = installedJarVersion?.trim()?.takeIf { it.isNotEmpty() }
    if (installed != null && installed != loaded) {
        return PluginRestartRequired(loaded, installed, "installed_jar", reason)
    }
    val staged = markerBody
        ?.lineSequence()
        ?.map(String::trim)
        ?.firstOrNull { line -> line.startsWith(STAGED_VERSION_MARKER_PREFIX) }
        ?.removePrefix(STAGED_VERSION_MARKER_PREFIX)
        ?.trim()
        ?.takeIf { it.isNotEmpty() }
    val loadedParts = numericDotVersion(loaded)
    val stagedParts = staged?.let(::numericDotVersion)
    if (staged != null && loadedParts != null && stagedParts != null &&
        compareDotVersions(stagedParts, loadedParts) > 0
    ) {
        return PluginRestartRequired(loaded, staged, "staged", reason)
    }
    return null
}

/** The balloon text: what is pending, why the IDE could not load it live, and the remedy. */
internal fun pluginRestartRequiredMessage(verdict: PluginRestartRequired): String {
    val where =
        if (verdict.source == "staged") "is staged for the next IDE start" else "is installed"
    val reason = verdict.reason?.let { text ->
        val shortened =
            if (text.length > RESTART_REASON_DISPLAY_LIMIT) {
                text.take(RESTART_REASON_DISPLAY_LIMIT).trimEnd() + "..."
            } else {
                text
            }
        "\nagent-doc could not swap it into the running IDE: $shortened"
    }.orEmpty()
    return "Agent Doc v${verdict.pendingVersion} $where, but this IDE is still running " +
        "v${verdict.loadedVersion}.$reason\nRestart the IDE to load it."
}

/**
 * Tells the operator, inside the IDE, when an agent-doc install left a plugin generation this
 * running IDE cannot load without a restart, and offers the IDE's own restart action.
 *
 * Before this, the only signal was a CLI warning at install time and a
 * `plugin_bytes_superseded_advisory` appended to later route output, so the IDE kept running
 * the superseded plugin until the operator noticed. One poller runs per plugin generation; it is
 * a [PluginGeneration] resource, so a dynamic unload (the classic line's restart-free upgrade)
 * cancels it before the replacement generation starts its own. A mismatch is reported only once
 * it has been observed on two consecutive polls, so the brief window inside a classic restart-free
 * swap, where new files precede the unload of the old generation, never raises a prompt.
 */
internal object PluginRestartRequiredNotifier {
    private const val PLUGIN_ID = "com.github.btakita.agent-doc"
    private const val NOTIFICATION_GROUP = "Agent Doc Plugin Update"
    private const val POLL_SECONDS = 30L

    private val log = Logger.getInstance(PluginRestartRequiredNotifier::class.java)
    private val started = AtomicBoolean(false)

    @Volatile
    private var lastObserved: PluginRestartRequired? = null

    @Volatile
    private var notifiedPendingVersion: String? = null

    @Volatile
    private var notification: com.intellij.notification.Notification? = null

    fun ensureStarted() {
        val application = ApplicationManager.getApplication() ?: return
        if (application.isUnitTestMode || PluginGeneration.retired) return
        if (!started.compareAndSet(false, true)) return
        val future = GenerationWork.scheduleWithFixedDelay(
            { poll() },
            POLL_SECONDS,
            POLL_SECONDS,
            TimeUnit.SECONDS,
        )
        PluginGeneration.registerResource("plugin-restart-required-notifier") {
            future.cancel(false)
            notification?.expire()
            notification = null
        }
    }

    private fun poll() {
        if (PluginGeneration.retired) return
        val verdict = try {
            observe()
        } catch (error: Throwable) {
            log.debug("[plugin-update] restart check failed", error)
            null
        }
        val previous = lastObserved
        lastObserved = verdict
        if (verdict == null || previous?.pendingVersion != verdict.pendingVersion) return
        if (notifiedPendingVersion == verdict.pendingVersion) return
        notifiedPendingVersion = verdict.pendingVersion
        log.warn(
            "[plugin-update] restart_required loaded=${verdict.loadedVersion} " +
                "pending=${verdict.pendingVersion} source=${verdict.source} reason=${verdict.reason.orEmpty()}",
        )
        GenerationWork.invokeLater { show(verdict) }
    }

    private fun observe(): PluginRestartRequired? {
        val pluginDir = MarketplacePlatformBridge.pluginPath(PLUGIN_ID) ?: return null
        val libNames = runCatching {
            Files.list(pluginDir.resolve("lib")).use { stream ->
                stream.map { it.fileName.toString() }.toList()
            }
        }.getOrDefault(emptyList())
        val marker = pluginDir.parent?.resolve(PLUGIN_RESTART_REQUIRED_MARKER)
        val markerBody = marker?.let { path -> runCatching { readMarker(path) }.getOrNull() }
        return pluginRestartRequiredVerdict(
            loadedVersion = MarketplacePlatformBridge.pluginVersion(PLUGIN_ID),
            installedJarVersion = installedPluginJarVersion(libNames),
            markerBody = markerBody,
        )
    }

    private fun readMarker(path: Path): String? =
        if (Files.isRegularFile(path)) Files.readString(path) else null

    private fun show(verdict: PluginRestartRequired) {
        if (PluginGeneration.retired) return
        try {
            notification?.expire()
            val created = NotificationGroupManager.getInstance()
                .getNotificationGroup(NOTIFICATION_GROUP)
                .createNotification(
                    "Agent Doc restart required",
                    TerminalUtil.notificationHtml(pluginRestartRequiredMessage(verdict)),
                    NotificationType.WARNING,
                )
            created.isImportant = true
            created.addAction(NotificationAction.createSimpleExpiring("Restart IDE") {
                ApplicationManager.getApplication().restart()
            })
            created.addAction(NotificationAction.createSimpleExpiring("Later") {})
            notification = created
            created.notify(null)
        } catch (error: Throwable) {
            log.warn("[plugin-update] restart-required notification failed", error)
        }
    }
}
