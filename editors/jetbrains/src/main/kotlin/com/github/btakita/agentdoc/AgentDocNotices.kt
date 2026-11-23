package com.github.btakita.agentdoc

import com.intellij.notification.Notification
import com.intellij.notification.NotificationType
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project

/**
 * GH #434: the one way the plugin shows an operator-facing toast.
 *
 * The operator reported an `agent-doc: input required` toast that `idea.log` had no line for.
 * A balloon raised by a bare `Notification.notify(project)` leaves no trace in the log: when a
 * report later asks "what did the IDE tell me, and when", the log cannot answer, and a toast that
 * outlives the condition behind it cannot be told apart from a fresh one. Every toast therefore
 * goes through [show], which writes one `[notify] shown` line (source, type, title, and the
 * plain text the operator read) before the balloon appears, and [expire], which writes the
 * matching `[notify] expired` line when the plugin retracts it. Editor hints go through
 * [hintShown]. `AgentDocNoticesTest` fails the build when a main source calls `.notify(` on a
 * notification anywhere else.
 */
internal object AgentDocNotices {
    private val LOG = Logger.getInstance(AgentDocNotices::class.java)

    /** Longest content kept in one log line; the rest is elided with its length. */
    internal const val MAX_LOGGED_CONTENT = 2000

    /** Log [notification] as shown, then show it. [source] names the call site for grep. */
    fun show(project: Project?, notification: Notification, source: String) {
        log(
            shownLine(
                source = source,
                type = notification.type,
                title = notification.title,
                content = notification.content,
                project = project?.name,
            ),
            notification.type,
        )
        notification.notify(project)
    }

    /** Log [notification] as retracted, then expire it. */
    fun expire(notification: Notification, source: String) {
        LOG.info(expiredLine(source, notification.title, notification.content))
        notification.expire()
    }

    /** Log an editor information hint (the transient bubble [TerminalUtil.showHint] shows). */
    fun hintShown(message: String, shown: Boolean) {
        LOG.info(
            "[notify] hint ${if (shown) "shown" else "dropped (no selected editor)"} " +
                "content=\"${plainText(message)}\"",
        )
    }

    private fun log(line: String, type: NotificationType) {
        // ERROR stays a warn-level line: `LOG.error` would open the IDE's fatal-error report.
        if (type == NotificationType.INFORMATION) LOG.info(line) else LOG.warn(line)
    }

    internal fun shownLine(
        source: String,
        type: NotificationType,
        title: String,
        content: String,
        project: String? = null,
    ): String =
        "[notify] shown source=$source type=${type.name}" +
            (project?.let { " project=\"${plainText(it)}\"" } ?: "") +
            " title=\"${plainText(title)}\" content=\"${plainText(content)}\""

    internal fun expiredLine(source: String, title: String, content: String): String =
        "[notify] expired source=$source title=\"${plainText(title)}\" content=\"${plainText(content)}\""

    /**
     * The text the operator read, on one log line: the notification HTML ([TerminalUtil.notificationHtml]
     * escapes, `<br>` line breaks, and any other tag) back to plain text, line breaks as `\n`, quotes
     * escaped, and the length capped at [MAX_LOGGED_CONTENT].
     */
    internal fun plainText(html: String): String {
        val text =
            html
                .replace(Regex("(?i)<br\\s*/?>"), "\n")
                .replace(Regex("<[^>]*>"), "")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&nbsp;", " ")
                .replace("&amp;", "&")
                .replace("\r", "")
                .replace("\\", "\\\\")
                .replace("\n", "\\n")
                .replace("\"", "\\\"")
        return if (text.length <= MAX_LOGGED_CONTENT) {
            text
        } else {
            text.take(MAX_LOGGED_CONTENT) + "...(${text.length} chars)"
        }
    }
}
