package com.github.btakita.agentdoc

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import io.github.lazily.Delta
import io.github.lazily.DeltaOp
import io.github.lazily.IpcValue
import io.github.lazily.NodeSnapshot
import io.github.lazily.Snapshot
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import java.lang.reflect.InvocationTargetException
import java.lang.reflect.Method
import java.util.UUID
import java.awt.event.InputMethodEvent
import java.awt.event.InputMethodListener
import javax.swing.DefaultListModel
import javax.swing.JList
import javax.swing.JTextField

private const val SCALAR_TAG = "lazily.view.scalar.v1"

internal fun repoContextRowId(group: String, row: RepoContextLocation): String =
    "$group:" + JsonArray().apply {
        add(row.path.toString())
        add(row.line)
        add(row.endLine)
        add(row.name)
        add(row.kind)
    }.toString()

/** The Swing renderer owns rows and retains the selected key across a list update. */
internal fun setRepoContextRows(
    group: String,
    list: JList<RepoContextLocation>,
    model: DefaultListModel<RepoContextLocation>,
    rows: List<RepoContextLocation>,
) {
    val selected = list.selectedValue?.let { repoContextRowId(group, it) }
    val desired = rows.distinctBy { repoContextRowId(group, it) }
    val keys = desired.map { repoContextRowId(group, it) }.toSet()
    for (index in model.size() - 1 downTo 0) {
        if (repoContextRowId(group, model.get(index)) !in keys) model.remove(index)
    }
    for ((index, row) in desired.withIndex()) {
        val key = repoContextRowId(group, row)
        val existing = (index until model.size()).firstOrNull { repoContextRowId(group, model.get(it)) == key }
        when {
            existing == null -> model.add(index, row)
            existing != index -> {
                val prior = model.remove(existing)
                model.add(index, if (prior == row) prior else row)
            }
            model.get(index) != row -> model.set(index, row)
        }
    }
    list.selectedIndex = desired.indexOfFirst { repoContextRowId(group, it) == selected }
}

/** The text widget owns local draft, caret and composition state. */
internal class RepoContextQuestionDraft(private val field: JTextField) : InputMethodListener {
    private var composing = false

    init {
        field.addInputMethodListener(this)
    }

    override fun inputMethodTextChanged(event: InputMethodEvent) {
        val text = event.text
        composing = text != null && event.committedCharacterCount < text.endIndex - text.beginIndex
    }

    override fun caretPositionChanged(event: InputMethodEvent) = Unit

    fun submission(): String? = field.text.takeUnless { composing }

    fun clearIfUnchanged(submitted: String) {
        if (!composing && field.text == submitted) field.text = ""
    }
}

/**
 * Shared Explorer definition backed by the Lazily preview receiver, when the
 * runtime contains it. Stable plugin builds continue using the native controls.
 */
internal class RepoContextRealtimeView private constructor(
    private val runtime: Any,
    private val eventCompanion: Any,
    private val parseEvent: Method,
    private val routeEvent: Method,
    private val attach: Method,
    private val replace: Method,
    private val delta: Method,
    private val signals: Method,
    private val acceptedType: Class<*>,
    private val template: JsonObject,
    private val instance: String,
) {
    private var definition: JsonObject? = null
    private var revision = 1L
    private var epoch = 0L
    private var count = 0

    fun publish(
        results: List<RepoContextLocation>,
        related: List<RepoContextLocation>,
        hasSelection: Boolean,
        hasSession: Boolean,
        hasAnswer: Boolean,
        reconnect: Boolean = false,
    ) {
        val candidate = template.deepCopy()
        val nodes = candidate.getAsJsonObject("definition").getAsJsonObject("nodes")
        val actions = candidate.getAsJsonObject("actions")
        nodes.getAsJsonObject("submit-question").addProperty("disabled", !hasSelection)
        nodes.getAsJsonObject("open-source").addProperty("disabled", !hasSelection)
        nodes.getAsJsonObject("open-session").addProperty("disabled", !hasSession)
        nodes.getAsJsonObject("open-answer").addProperty("disabled", !hasAnswer)
        for ((group, values, action) in listOf(Triple("results", results, "select"), Triple("related", related, "openLink"))) {
            val children = JsonArray()
            for (row in values.distinctBy { repoContextRowId(group, it) }) {
                val key = repoContextRowId(group, row)
                children.add(key)
                nodes.add(key, JsonObject().apply {
                    addProperty("role", "button")
                    addProperty("text", row.name)
                })
                actions.add(key, JsonObject().apply { addProperty("activate", action) })
            }
            nodes.getAsJsonObject(group).add("children", children)
        }
        val bodyChanged = candidate != definition
        val binding = candidate.getAsJsonObject("bindings").get("relatedCount").asLong
        val updatedCount = related.distinctBy { repoContextRowId("related", it) }.size
        val payload = updatedCount.toString().encodeToByteArray()
        if (definition != null && count != updatedCount) {
            val nextEpoch = epoch + 1
            delta.call(runtime, Delta(epoch, nextEpoch, listOf(DeltaOp.CellSet(binding, IpcValue.Inline(payload)))))
            epoch = nextEpoch
        }
        count = updatedCount
        if (bodyChanged && definition != null) revision++
        if (definition == null || bodyChanged || reconnect) {
            val frame = candidate.deepCopy().apply {
                addProperty("version", 1)
                addProperty("view_instance", instance)
                addProperty("revision", revision)
            }.toString().let(Json::parseToJsonElement)
            if (definition == null || reconnect) {
                attach.call(runtime, frame, Snapshot(epoch, listOf(NodeSnapshot.payload(binding, SCALAR_TAG, payload)), roots = listOf(binding)))
            } else {
                replace.call(runtime, frame)
            }
        }
        definition = candidate
    }

    fun relatedCount(): Int =
        ((signals.call(runtime) as Map<*, *>)["relatedCount"] as Number).toInt()

    fun accepts(
        node: String,
        event: String,
        action: String,
        payload: String? = null,
        eventId: String = UUID.randomUUID().toString(),
        displayedRevision: Long = revision,
    ): Boolean {
        val frame = JsonObject().apply {
            addProperty("version", 1)
            addProperty("view_instance", instance)
            addProperty("displayed_revision", displayedRevision)
            addProperty("node_id", node)
            addProperty("event_id", eventId)
            addProperty("event", event)
            addProperty("payload", payload)
        }.toString().let(Json::parseToJsonElement)
        val request = checkNotNull(parseEvent.call(eventCompanion, frame))
        val authorize: (String, Any) -> Boolean = { name, _ -> name == action }
        return acceptedType.isInstance(routeEvent.call(runtime, request, authorize))
    }

    companion object {
        fun open(): RepoContextRealtimeView? {
            val receiver = try {
                Class.forName("io.github.lazily.RealtimeView")
            } catch (_: ClassNotFoundException) {
                return null // The released dependency does not yet ship the preview.
            }
            val event = Class.forName("io.github.lazily.ViewEvent")
            val companion = event.getField("Companion").get(null)
            val instance = UUID.randomUUID().toString()
            val template = checkNotNull(javaClass.getResourceAsStream("/repoContextView.json")) {
                "The shared Explorer View is missing from the plugin resources."
            }.bufferedReader().use { JsonParser.parseReader(it).asJsonObject }
            return RepoContextRealtimeView(
                receiver.getConstructor(String::class.java).newInstance(instance),
                companion,
                companion.javaClass.getMethod("fromJson", JsonElement::class.java),
                receiver.getMethod("routeEvent", event, Function2::class.java),
                receiver.getMethod("attach", JsonElement::class.java, Snapshot::class.java),
                receiver.getMethod("replace", JsonElement::class.java),
                receiver.getMethod("applyDelta", Delta::class.java),
                receiver.getMethod("getSignals"),
                Class.forName("io.github.lazily.ViewEventResult\$Accepted"),
                template,
                instance,
            )
        }
    }
}

private fun Method.call(receiver: Any, vararg args: Any): Any? =
    try {
        invoke(receiver, *args)
    } catch (error: InvocationTargetException) {
        throw error.targetException
    }
