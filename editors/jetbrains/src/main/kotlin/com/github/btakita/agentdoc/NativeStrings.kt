package com.github.btakita.agentdoc

import com.sun.jna.Library
import com.sun.jna.Pointer
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

/**
 * GH #274: every string that crosses the agent-doc JNA boundary is byte-exact UTF-8.
 *
 * JNA marshals a Kotlin `String` argument to `char*` (and decodes a `String` return or
 * [Pointer.getString]) with `Native.getDefaultStringEncoding()`, which follows the JVM's
 * `native.encoding` — the platform locale, NOT `file.encoding`. An IDE backend started under a
 * non-UTF-8 locale (`locale=en_US`, `sun.jnu.encoding=ANSI_X3.4-1968`) therefore encoded document
 * text as US-ASCII with replacement: every em dash, `·`, `→` and emoji became a literal `?`
 * (0x3F, valid UTF-8, so the Rust side could not tell), and every non-ASCII byte Rust returned
 * decoded to U+FFFD. A committed response came back as a second, lossy copy and was committed.
 *
 * The rule: the library is bound with [NATIVE_LIBRARY_OPTIONS] (UTF-8 for every argument and
 * return), every returned pointer is read with [readNativeUtf8] (strict), and every `String`
 * argument is proven well-formed UTF-16 by [requireLosslessNativeStringArgs] before the call —
 * a lone surrogate is the one input UTF-8 also replaces with `?`. A lossy crossing fails closed;
 * it never substitutes.
 */
internal const val NATIVE_STRING_ENCODING = "UTF-8"

/** Options every agent-doc [Library.Handler] is created with. Never bind with `emptyMap()`. */
internal val NATIVE_LIBRARY_OPTIONS: Map<String, Any> =
    mapOf(Library.OPTION_STRING_ENCODING to NATIVE_STRING_ENCODING)

/** A native string crossing that would lose characters. */
internal class LossyNativeStringException(message: String) : IllegalArgumentException(message)

/** True when [text] has no unpaired surrogate, i.e. it encodes to UTF-8 without replacement. */
internal fun isWellFormedUtf16(text: String): Boolean {
    var index = 0
    while (index < text.length) {
        val ch = text[index]
        if (Character.isHighSurrogate(ch)) {
            if (index + 1 >= text.length || !Character.isLowSurrogate(text[index + 1])) return false
            index += 2
            continue
        }
        if (Character.isLowSurrogate(ch)) return false
        index += 1
    }
    return true
}

/**
 * Refuse a native call whose `String` arguments cannot cross as exact UTF-8. Called by the
 * generation proxy for every `AgentDocLib` method, so no call site can opt out.
 */
internal fun requireLosslessNativeStringArgs(methodName: String, args: Array<out Any?>?) {
    args?.forEachIndexed { position, arg ->
        if (arg is String && !isWellFormedUtf16(arg)) {
            throw LossyNativeStringException(
                "refusing native call $methodName: argument $position contains an unpaired " +
                    "UTF-16 surrogate, which cannot cross the JNA boundary as exact UTF-8",
            )
        }
    }
}

/**
 * Strictly decode the NUL-terminated UTF-8 string at this pointer. Malformed bytes throw
 * [CharacterCodingException] instead of decoding to U+FFFD, and the JVM's locale never
 * participates (unlike [Pointer.getString] with no encoding argument).
 */
internal fun Pointer.readNativeUtf8(): String {
    val length = indexOf(0L, 0.toByte())
    require(length >= 0 && length <= Int.MAX_VALUE) { "native string is not NUL-terminated" }
    val bytes = getByteArray(0L, length.toInt())
    return StandardCharsets.UTF_8
        .newDecoder()
        .onMalformedInput(CodingErrorAction.REPORT)
        .onUnmappableCharacter(CodingErrorAction.REPORT)
        .decode(ByteBuffer.wrap(bytes))
        .toString()
}
