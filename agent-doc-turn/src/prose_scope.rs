//! Scope `#id` scanning to prose (`#hookhashfalsepositive`, `#hookhashjsprivatefield`).
//!
//! The coined-id guard exists to stop a turn from making an INVENTED `#id`
//! durable. Durable means a commit message or a comment — documentation a reader
//! greps later and finds resolving to nothing. It was never meant to read code.
//!
//! But it scanned whole files, so every language that spells something `#word`
//! tripped it: C preprocessor directives (`#include`, `#pragma`), CSS id
//! selectors, and — the incident that motivated this module — ES2022 private
//! class fields, where `this.#entries` and an indented `#hlc;` declaration both
//! slip past `extract_tags`'s "not glued to a preceding word" rule because the
//! preceding character is `.` or a space.
//!
//! Each incident was patched by adding another extension and another keyword
//! list. That does not converge: the list is "every language anyone ever writes
//! here". So invert it. For a source file whose comment syntax we know, blank
//! everything that is not a comment and scan what remains. Code stops being
//! scanned at all, which is the actual invariant — `#include` is not prose, and
//! neither is `this.#entries`.
//!
//! Two deliberate limits:
//!
//! - An unknown extension (and every prose format: `.md`, `.txt`, plain files)
//!   is returned untouched and scanned whole. Widening this module's language
//!   table can only ever reduce false blocks, never open a hole.
//! - `git commit` messages are pure prose and never reach this module, so the
//!   guard's primary case is unchanged.
//!
//! Blanking preserves byte-for-byte character positions and every newline, so
//! line-start rules in `extract_tags` still see the same line structure.

use std::borrow::Cow;

/// A quote character, and whether its string may span newlines.
type StringDelimiter = (char, bool);

/// Comment syntax for one language family.
struct CommentSyntax {
    line: &'static [&'static str],
    block: &'static [(&'static str, &'static str)],
    /// Strings are skipped so a URL in a literal (`"http://x/#frag"`) cannot
    /// be mistaken for the start of a line comment.
    strings: &'static [StringDelimiter],
}

const C_LIKE: CommentSyntax = CommentSyntax {
    line: &["//"],
    block: &[("/*", "*/")],
    strings: &[('"', false), ('\'', false)],
};

/// JavaScript/TypeScript adds template literals, which legitimately span lines.
const JS_LIKE: CommentSyntax = CommentSyntax {
    line: &["//"],
    block: &[("/*", "*/")],
    strings: &[('"', false), ('\'', false), ('`', true)],
};

const HASH_LINE: CommentSyntax = CommentSyntax {
    line: &["#"],
    block: &[],
    strings: &[('"', false), ('\'', false)],
};

const CSS_LIKE: CommentSyntax = CommentSyntax {
    line: &[],
    block: &[("/*", "*/")],
    strings: &[('"', false), ('\'', false)],
};

/// SCSS and Less accept `//` on top of CSS block comments.
const SCSS_LIKE: CommentSyntax = CommentSyntax {
    line: &["//"],
    block: &[("/*", "*/")],
    strings: &[('"', false), ('\'', false)],
};

const DASH_LINE: CommentSyntax = CommentSyntax {
    line: &["--"],
    block: &[("/*", "*/")],
    strings: &[('"', false), ('\'', false)],
};

const MARKUP: CommentSyntax = CommentSyntax {
    line: &[],
    block: &[("<!--", "-->")],
    strings: &[],
};

/// Comment syntax for a file extension, or `None` to scan the text whole.
///
/// `None` is the conservative answer: prose formats and anything unrecognized
/// keep today's behavior. Only a language listed here has its code excluded.
fn comment_syntax(extension: &str) -> Option<&'static CommentSyntax> {
    match extension {
        "c" | "cc" | "cpp" | "cu" | "cuh" | "cxx" | "h" | "hh" | "hpp" | "hxx" | "inc" | "inl"
        | "ipp" | "m" | "mm" | "tpp" | "rs" | "go" | "java" | "kt" | "kts" | "swift" | "scala"
        | "cs" | "dart" | "php" | "zig" | "proto" | "gradle" | "groovy" | "glsl" | "hlsl"
        | "wgsl" | "sol" | "v" | "sv" => Some(&C_LIKE),
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "mts" | "cts" => Some(&JS_LIKE),
        "py" | "pyi" | "sh" | "bash" | "zsh" | "fish" | "rb" | "pl" | "r" | "jl" | "nix" | "tf"
        | "yml" | "yaml" | "toml" | "ini" | "cfg" | "conf" | "properties" | "dockerfile" => {
            Some(&HASH_LINE)
        }
        "css" => Some(&CSS_LIKE),
        "scss" | "sass" | "less" | "styl" => Some(&SCSS_LIKE),
        "sql" | "hs" | "lua" | "elm" | "ada" => Some(&DASH_LINE),
        "html" | "htm" | "xml" | "svg" | "xhtml" | "vue" | "svelte" => Some(&MARKUP),
        _ => None,
    }
}

/// Blank every non-comment region of `text` for a known source language.
///
/// Returns the text untouched when the extension is absent or unrecognized, so
/// prose files and unknown formats keep being scanned whole.
pub fn prose_only<'a>(text: &'a str, extension: Option<&str>) -> Cow<'a, str> {
    let Some(syntax) = extension
        .map(str::to_ascii_lowercase)
        .and_then(|extension| comment_syntax(&extension))
    else {
        return Cow::Borrowed(text);
    };

    let chars: Vec<char> = text.chars().collect();
    // Same length, same newlines: positions and line structure survive so the
    // caller's line-start rules still hold.
    let mut out: Vec<char> = chars
        .iter()
        .map(|ch| if *ch == '\n' { '\n' } else { ' ' })
        .collect();

    let mut index = 0usize;
    while index < chars.len() {
        if let Some((_, multiline)) = syntax
            .strings
            .iter()
            .find(|(quote, _)| *quote == chars[index])
        {
            index = skip_string(&chars, index, *multiline);
            continue;
        }
        if let Some((open, close)) = syntax
            .block
            .iter()
            .find(|(open, _)| matches_at(&chars, index, open))
        {
            let start = index;
            index += open.chars().count();
            while index < chars.len() && !matches_at(&chars, index, close) {
                index += 1;
            }
            index = (index + close.chars().count()).min(chars.len());
            keep(&chars, &mut out, start, index);
            continue;
        }
        if syntax
            .line
            .iter()
            .any(|marker| matches_at(&chars, index, marker))
        {
            let start = index;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            keep(&chars, &mut out, start, index);
            continue;
        }
        index += 1;
    }

    Cow::Owned(out.into_iter().collect())
}

fn keep(chars: &[char], out: &mut [char], start: usize, end: usize) {
    out[start..end].copy_from_slice(&chars[start..end]);
}

fn matches_at(chars: &[char], index: usize, marker: &str) -> bool {
    marker
        .chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(index + offset) == Some(&expected))
}

/// Advance past a string literal, returning the index just after its close.
///
/// An unterminated single-line string stops at the newline rather than eating
/// the rest of the file — a stray apostrophe in a comment is common, and letting
/// it swallow everything after would silently disable the guard.
fn skip_string(chars: &[char], open: usize, multiline: bool) -> usize {
    let quote = chars[open];
    let mut index = open + 1;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            '\n' if !multiline => return index,
            ch if ch == quote => return index + 1,
            _ => index += 1,
        }
    }
    chars.len()
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The incident that motivated this module: ES2022 private class fields.
    /// `this.#entries` slips past the "glued to a preceding word" rule because
    /// the preceding character is `.`, and an indented `#hlc;` declaration
    /// slips past because it is a space.
    #[test]
    fn javascript_private_class_fields_are_code_not_prose() {
        let source = "class SeqCrdt {\n  #hlc;\n  #peer;\n  size() {\n    return this.#entries.size\n  }\n}\n";
        let scanned = prose_only(source, Some("js"));
        assert!(!scanned.contains("#hlc"), "{scanned}");
        assert!(!scanned.contains("#peer"), "{scanned}");
        assert!(!scanned.contains("#entries"), "{scanned}");
    }

    /// Every extension the incident named must be covered, not just `.js`.
    #[test]
    fn the_whole_javascript_family_is_covered() {
        for extension in ["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts"] {
            let scanned = prose_only("  #hlc;\n", Some(extension));
            assert!(
                !scanned.contains("#hlc"),
                "{extension} still scans code: {scanned}"
            );
        }
    }

    /// The previous incident: C preprocessor directives. This replaces the
    /// hand-maintained directive keyword list, so a directive that was never on
    /// that list is handled too.
    #[test]
    fn c_preprocessor_directives_are_code_not_prose() {
        let source =
            "#pragma once\n#include <string>\n#embed \"data.bin\"\n#some_future_directive\n";
        let scanned = prose_only(source, Some("h"));
        assert_eq!(scanned.trim(), "", "no directive line is prose: {scanned}");
    }

    /// CSS id selectors are code. Named by the original report.
    #[test]
    fn css_id_selectors_are_code_not_prose() {
        let scanned = prose_only("#main .row {{ color: red }}\n", Some("css"));
        assert!(!scanned.contains("#main"), "{scanned}");
    }

    /// A comment is exactly what the guard exists for: an id written there is
    /// documentation a reader greps later.
    #[test]
    fn comments_are_still_scanned() {
        let source = "// See #coinedid for why.\nconst x = 1\n";
        let scanned = prose_only(source, Some("ts"));
        assert!(scanned.contains("#coinedid"), "{scanned}");

        let block = "/* tracked by #coinedid */\n";
        assert!(prose_only(block, Some("cpp")).contains("#coinedid"));

        let hash = "  # tracked by #coinedid\nvalue = 1\n";
        assert!(prose_only(hash, Some("py")).contains("#coinedid"));

        let markup = "<!-- #coinedid -->\n<div id=\"main\"></div>\n";
        assert!(prose_only(markup, Some("html")).contains("#coinedid"));
    }

    /// A `//` inside a string literal does not open a comment, so a URL fragment
    /// cannot be read as prose.
    #[test]
    fn a_url_in_a_string_does_not_open_a_comment() {
        let source = "const url = \"http://example.test/#anchortag\"\n";
        let scanned = prose_only(source, Some("js"));
        assert!(!scanned.contains("#anchortag"), "{scanned}");
    }

    /// A multi-line template literal must not be treated as an unterminated
    /// string that swallows the comment after it.
    #[test]
    fn template_literals_span_lines_without_swallowing_later_comments() {
        let source = "const t = `line one\nline two`\n// #coinedid\n";
        let scanned = prose_only(source, Some("ts"));
        assert!(scanned.contains("#coinedid"), "{scanned}");
    }

    /// An unterminated single-line string stops at the newline. An apostrophe in
    /// a comment is common, and letting it eat the rest of the file would
    /// silently disable the guard for everything below.
    #[test]
    fn an_apostrophe_does_not_swallow_the_rest_of_the_file() {
        let source = "// it's fine\n// #coinedid\n";
        let scanned = prose_only(source, Some("rs"));
        assert!(scanned.contains("#coinedid"), "{scanned}");
    }

    /// Prose formats and unknown extensions keep being scanned whole. Narrowing
    /// must never open a hole in the guard.
    #[test]
    fn prose_and_unknown_formats_are_untouched() {
        let text = "Landed #coinedid today.\n";
        for extension in [None, Some("md"), Some("txt"), Some("json"), Some("weird")] {
            let scanned = prose_only(text, extension);
            assert_eq!(scanned, text, "{extension:?} must be scanned whole");
            assert!(matches!(scanned, Cow::Borrowed(_)));
        }
    }

    /// Positions and line structure must survive, or the caller's line-start
    /// rules read the wrong lines.
    #[test]
    fn blanking_preserves_length_and_newlines() {
        let source = "const a = 1\n// note\nconst b = 2\n";
        let scanned = prose_only(source, Some("js"));
        assert_eq!(scanned.chars().count(), source.chars().count());
        assert_eq!(
            scanned.lines().count(),
            source.lines().count(),
            "{scanned:?}"
        );
        assert_eq!(scanned.lines().nth(1), Some("// note"));
    }

    /// Extension matching is case-insensitive: `.H` and `.TS` are the same
    /// languages as `.h` and `.ts`.
    #[test]
    fn extension_matching_is_case_insensitive() {
        assert!(!prose_only("#pragma once\n", Some("H")).contains("#pragma"));
        assert!(!prose_only("  #hlc;\n", Some("TS")).contains("#hlc"));
    }

    /// An unterminated block comment runs to end of file rather than panicking
    /// on the missing close marker.
    #[test]
    fn an_unterminated_block_comment_does_not_panic() {
        let scanned = prose_only("/* #coinedid", Some("c"));
        assert!(scanned.contains("#coinedid"), "{scanned}");
    }
}
