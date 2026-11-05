package com.github.btakita.agentdoc.split.frontend;

import com.intellij.lang.Language;
import com.intellij.openapi.editor.Document;
import com.intellij.openapi.editor.Editor;
import com.intellij.openapi.editor.GenericLineWrapPositionStrategy;
import com.intellij.openapi.editor.LanguageLineWrapPositionStrategy;
import com.intellij.openapi.editor.LineWrapPositionStrategy;
import com.intellij.openapi.editor.ex.util.LexerEditorHighlighter;
import com.intellij.openapi.project.Project;
import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.util.function.IntUnaryOperator;
import org.jetbrains.annotations.NotNull;
import org.jetbrains.annotations.Nullable;

/**
 * GH #391: keeps a Markdown list marker on the same visual line as its first words.
 *
 * <p>The bundled Markdown line wrap strategy in 263 and later 262 builds guards links with a token
 * scan. When the wrap position lands on a bracket, a parenthesis or a URL token, it moves the wrap
 * back to the nearest {@code [} earlier on the line if a {@code )} follows anywhere later on that
 * line. It never checks that this {@code [} opens a link. An agent-doc item such as
 * {@code - [#vscodecodersmoke] Run scripts/... (procedure in runbooks/coder-workspace.md): ...}
 * then wraps at its opening bracket, which leaves {@code - } alone on the first visual line. On
 * later visual lines the same bracket lies before the line's start, so the strategy refuses to wrap
 * at all.
 *
 * <p>This strategy is registered first for Markdown in the frontend module, which lays out soft
 * wraps in a JetBrains Client and in a monolithic IDE. It asks the bundled strategy first and
 * replaces that answer with the plain whitespace wrap only when {@link MarkdownBracketWrapPolicy}
 * proves the move came from a bracket that opens no link still open at the wrap. A wrap inside a real
 * link keeps the bundled answer; a link that closed before the wrap no longer pulls a later wrap (in
 * front of a bare URL or a parenthesis) back to its {@code [} ({@code #activesummaryurl}). Builds before 262 use a PSI-based Markdown strategy that is unaffected, so the classic
 * distribution has no copy of this class.
 *
 * <p>Written in Java on purpose: the editor-based {@code calculateWrapPosition},
 * {@code getWrapDocument} and {@code isSoftWrappingAllowed} methods are absent from the 262 GA
 * interface and present in 263. A Java method with the same signature overrides the interface
 * default where it exists and compiles against both, which a Kotlin {@code override} cannot. The
 * bundled strategy's versions are reached through method handles for the same reason.
 */
public final class MarkdownBracketWrapStrategy implements LineWrapPositionStrategy {
    private static final String MARKDOWN_LANGUAGE_ID = "Markdown";

    private static final @Nullable MethodHandle EDITOR_WRAP = findStrategyMethod(
        "calculateWrapPosition",
        MethodType.methodType(int.class, Editor.class, int.class, int.class, int.class, boolean.class, boolean.class)
    );
    private static final @Nullable MethodHandle WRAP_DOCUMENT = findStrategyMethod(
        "getWrapDocument",
        MethodType.methodType(Document.class, Editor.class, boolean.class)
    );
    private static final @Nullable MethodHandle SOFT_WRAPPING_ALLOWED = findStrategyMethod(
        "isSoftWrappingAllowed",
        MethodType.methodType(boolean.class, Editor.class, int.class)
    );

    // GenericLineWrapPositionStrategy keeps per-call scratch state in the instance.
    private static final ThreadLocal<WhitespaceWrapPositions> WHITESPACE =
        ThreadLocal.withInitial(WhitespaceWrapPositions::new);

    @Override
    public int calculateWrapPosition(
        @NotNull Document document,
        @Nullable Project project,
        int startOffset,
        int endOffset,
        int maxPreferredOffset,
        boolean allowToBeyondMaxPreferredOffset,
        boolean isSoftWrap
    ) {
        LineWrapPositionStrategy upstream = upstream();
        int delegated = upstream.calculateWrapPosition(
            document, project, startOffset, endOffset, maxPreferredOffset, allowToBeyondMaxPreferredOffset, isSoftWrap
        );
        return correct(
            document, project, startOffset, endOffset, maxPreferredOffset, allowToBeyondMaxPreferredOffset, isSoftWrap,
            delegated,
            lineStart -> upstream.calculateWrapPosition(
                document, project, lineStart, endOffset, maxPreferredOffset, allowToBeyondMaxPreferredOffset, isSoftWrap
            )
        );
    }

    /** Overrides the interface default where the platform has it (later 262 builds, 263). */
    @SuppressWarnings("unused")
    public int calculateWrapPosition(
        @NotNull Editor editor,
        int startOffset,
        int endOffset,
        int maxPreferredOffset,
        boolean allowToBeyondMaxPreferredOffset,
        boolean isSoftWrap
    ) {
        if (EDITOR_WRAP == null) {
            return calculateWrapPosition(
                editor.getDocument(), editor.getProject(), startOffset, endOffset, maxPreferredOffset,
                allowToBeyondMaxPreferredOffset, isSoftWrap
            );
        }
        LineWrapPositionStrategy upstream = upstream();
        int delegated = editorWrapPosition(
            upstream, editor, startOffset, endOffset, maxPreferredOffset, allowToBeyondMaxPreferredOffset, isSoftWrap
        );
        // The bundled link guard reads the lexer highlighter; without one it applies no guard.
        if (!(editor.getHighlighter() instanceof LexerEditorHighlighter)) return delegated;
        return correct(
            wrapDocument(upstream, editor, isSoftWrap), editor.getProject(), startOffset, endOffset, maxPreferredOffset,
            allowToBeyondMaxPreferredOffset, isSoftWrap,
            delegated,
            lineStart -> editorWrapPosition(
                upstream, editor, lineStart, endOffset, maxPreferredOffset, allowToBeyondMaxPreferredOffset, isSoftWrap
            )
        );
    }

    /** Overrides the interface default where the platform has it (later 262 builds, 263). */
    @SuppressWarnings("unused")
    public @NotNull Document getWrapDocument(@NotNull Editor editor, boolean isSoftWrap) {
        return wrapDocument(upstream(), editor, isSoftWrap);
    }

    /** Overrides the interface default where the platform has it (263). */
    @SuppressWarnings("unused")
    public boolean isSoftWrappingAllowed(@NotNull Editor editor, int offset) {
        if (SOFT_WRAPPING_ALLOWED == null) return true;
        try {
            return (boolean) SOFT_WRAPPING_ALLOWED.invoke(upstream(), editor, offset);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    @Override
    public boolean canWrapLineAtOffset(@NotNull CharSequence text, int offset) {
        return upstream().canWrapLineAtOffset(text, offset);
    }

    /**
     * Asks {@code strategy} for an editor wrap position: through the editor-based method where the
     * platform has one, otherwise through the document-based one. The platform test compares the
     * bundled strategy with this one through it on every build.
     */
    public static int editorWrapPosition(
        @NotNull LineWrapPositionStrategy strategy,
        @NotNull Editor editor,
        int startOffset,
        int endOffset,
        int maxPreferredOffset,
        boolean allowToBeyondMaxPreferredOffset,
        boolean isSoftWrap
    ) {
        if (EDITOR_WRAP == null) {
            return strategy.calculateWrapPosition(
                editor.getDocument(), editor.getProject(), startOffset, endOffset, maxPreferredOffset,
                allowToBeyondMaxPreferredOffset, isSoftWrap
            );
        }
        try {
            return (int) EDITOR_WRAP.invoke(
                strategy, editor, startOffset, endOffset, maxPreferredOffset, allowToBeyondMaxPreferredOffset, isSoftWrap
            );
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    private static int correct(
        @NotNull Document document,
        @Nullable Project project,
        int startOffset,
        int endOffset,
        int maxPreferredOffset,
        boolean allowToBeyondMaxPreferredOffset,
        boolean isSoftWrap,
        int delegated,
        @NotNull IntUnaryOperator askFromLineStart
    ) {
        CharSequence text = document.getCharsSequence();
        if (!MarkdownBracketWrapPolicy.mayHaveBracket(text, startOffset, endOffset)) return delegated;
        int whitespaceWrap = WHITESPACE.get().calculateWrapPosition(
            document, project, startOffset, endOffset, maxPreferredOffset, allowToBeyondMaxPreferredOffset, isSoftWrap
        );
        return MarkdownBracketWrapPolicy.correct(text, whitespaceWrap, delegated, () -> {
            // A NEVER_WRAP from the link guard depends on the visual line's start; one from a
            // table, heading or setext rule does not. On a continuation visual line, asking again
            // from the logical line's start tells the two apart.
            int lineStart = document.getLineStartOffset(document.getLineNumber(whitespaceWrap));
            if (lineStart < startOffset) return askFromLineStart.applyAsInt(lineStart) >= 0;
            return !MarkdownBracketWrapPolicy.mayNeverWrapByLayout(text, lineStart);
        });
    }

    /** The next Markdown strategy: the bundled one, or the platform default without it. */
    private @NotNull LineWrapPositionStrategy upstream() {
        Language language = Language.findLanguageByID(MARKDOWN_LANGUAGE_ID);
        if (language != null) {
            for (LineWrapPositionStrategy strategy : LanguageLineWrapPositionStrategy.INSTANCE.allForLanguage(language)) {
                // Compare by name: another classloader's copy of this class must not become the upstream.
                if (!strategy.getClass().getName().equals(getClass().getName())) return strategy;
            }
        }
        return LanguageLineWrapPositionStrategy.INSTANCE.getDefaultImplementation();
    }

    private static @NotNull Document wrapDocument(
        @NotNull LineWrapPositionStrategy strategy,
        @NotNull Editor editor,
        boolean isSoftWrap
    ) {
        if (WRAP_DOCUMENT == null) return editor.getDocument();
        try {
            return (Document) WRAP_DOCUMENT.invoke(strategy, editor, isSoftWrap);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    private static @Nullable MethodHandle findStrategyMethod(String name, MethodType type) {
        try {
            return MethodHandles.publicLookup().findVirtual(LineWrapPositionStrategy.class, name, type);
        } catch (NoSuchMethodException | IllegalAccessException absent) {
            return null;
        }
    }

    private static RuntimeException rethrow(Throwable t) {
        if (t instanceof RuntimeException runtime) throw runtime;
        if (t instanceof Error error) throw error;
        throw new IllegalStateException(t);
    }

    /** The bundled strategy's first step: wrap after a space or a tab, and only there. */
    private static final class WhitespaceWrapPositions extends GenericLineWrapPositionStrategy {
        WhitespaceWrapPositions() {
            addRule(new Rule(' ', WrapCondition.AFTER));
            addRule(new Rule('\t', WrapCondition.AFTER));
        }

        @Override
        protected boolean canUseOffset(@NotNull Document document, int offset, boolean virtual) {
            char c = document.getCharsSequence().charAt(offset);
            return c == ' ' || c == '\t';
        }
    }
}
