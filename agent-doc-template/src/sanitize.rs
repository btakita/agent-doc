//! Sanitization for template patch payloads.
//!
//! Agent-authored examples of `<!-- agent:NAME -->` component delimiters must
//! remain display text when they are written through template patches. Escaping
//! those comments before patch application prevents later component parses from
//! treating examples as real document structure.

use crate::template::PatchBlock;

/// Sanitize component tags in patch block content to prevent parser corruption.
///
/// Only `<!-- agent:NAME -->` and `<!-- /agent:NAME -->` patterns where `NAME`
/// is a valid component name are escaped. Patch markers and other comments pass
/// through unchanged.
pub fn sanitize_component_tags(content: &str) -> String {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut pos = 0;

    while pos + 4 <= len {
        if &bytes[pos..pos + 4] != b"<!--" {
            let ch_len = utf8_char_len(bytes[pos]);
            result.push_str(&content[pos..pos + ch_len]);
            pos += ch_len;
            continue;
        }

        let close = match find_comment_close(bytes, pos + 4) {
            Some(c) => c,
            None => {
                result.push_str("<!--");
                pos += 4;
                continue;
            }
        };

        let inner = &content[pos + 4..close - 3];
        let trimmed = inner.trim();

        if agent_doc_element::element::is_agent_marker(trimmed) {
            let original = &content[pos..close];
            result.push_str(&original.replace('<', "&lt;").replace('>', "&gt;"));
        } else {
            result.push_str(&content[pos..close]);
        }
        pos = close;
    }

    if pos < len {
        result.push_str(&content[pos..]);
    }

    result
}

/// Sanitize the content of each patch block in-place.
pub fn sanitize_patches(patches: &mut [PatchBlock]) {
    for patch in patches.iter_mut() {
        patch.content = sanitize_component_tags(&patch.content);
    }
}

/// Sanitize unmatched response text before appending it to a component.
pub fn sanitize_unmatched(unmatched: &mut String) {
    *unmatched = sanitize_component_tags(unmatched);
}

fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xFF => 4,
        _ => 1,
    }
}

fn find_comment_close(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut i = start;
    while i + 3 <= len {
        if &bytes[i..i + 3] == b"-->" {
            return Some(i + 3);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_escapes_open_agent_tag() {
        let input = "Here is an example: <!-- agent:exchange --> marker.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "open agent tag should be escaped, got: {}",
            result
        );
        assert!(
            !result.contains("<!-- agent:exchange -->"),
            "raw open agent tag should not remain"
        );
    }

    #[test]
    fn sanitize_escapes_close_agent_tag() {
        let input = "End marker: <!-- /agent:pending --> done.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("&lt;!-- /agent:pending --&gt;"),
            "close agent tag should be escaped, got: {}",
            result
        );
        assert!(
            !result.contains("<!-- /agent:pending -->"),
            "raw close agent tag should not remain"
        );
    }

    #[test]
    fn sanitize_does_not_escape_patch_markers() {
        let input = "<!-- patch:exchange -->\nsome content\n<!-- /patch:exchange -->\n";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "patch markers must not be escaped");
    }

    #[test]
    fn sanitize_passes_normal_content_through() {
        let input = "Just some normal markdown content.\n\nWith paragraphs and **bold**.";
        let result = sanitize_component_tags(input);
        assert_eq!(
            result, input,
            "normal content should pass through unchanged"
        );
    }

    #[test]
    fn sanitize_preserves_utf8_em_dash() {
        let input = "This is a test \u{2014} with em dashes \u{2014} in content.";
        let result = sanitize_component_tags(input);
        assert_eq!(
            result, input,
            "em dashes must survive sanitization unchanged"
        );
        assert_eq!(
            result.as_bytes(),
            input.as_bytes(),
            "byte-level content must be identical"
        );
    }

    #[test]
    fn sanitize_preserves_mixed_utf8_and_agent_tags() {
        let input = "Response with \u{2014} em dash and <!-- agent:exchange --> tag reference.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains('\u{2014}'),
            "em dash must be preserved, got: {:?}",
            result
        );
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "agent tag must be escaped"
        );
    }

    #[test]
    fn sanitize_preserves_various_unicode() {
        let input = "Caf\u{00E9} \u{2019}quotes\u{2019} \u{2014} \u{2026} \u{1F600}";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "all unicode must survive sanitization");
    }

    #[test]
    fn sanitize_unmatched_escapes_exchange_markers_in_response() {
        let mut unmatched =
            "### Re: deploy\n\nDone.\n\n<!-- agent:exchange -->\nExtra\n<!-- /agent:exchange -->\n"
                .to_string();
        sanitize_unmatched(&mut unmatched);
        assert!(
            !unmatched.contains("<!-- agent:exchange -->"),
            "agent exchange markers must be escaped in unmatched text, got: {unmatched}"
        );
        assert!(
            unmatched.contains("&lt;!-- agent:exchange --&gt;"),
            "escaped markers expected, got: {unmatched}"
        );
    }

    #[test]
    fn sanitize_patches_escapes_each_patch_content() {
        let mut patches = vec![PatchBlock::new(
            "exchange",
            "Example: <!-- agent:exchange -->\n<!-- /agent:exchange -->",
        )];

        sanitize_patches(&mut patches);

        assert!(patches[0].content.contains("&lt;!-- agent:exchange --&gt;"));
        assert!(
            patches[0]
                .content
                .contains("&lt;!-- /agent:exchange --&gt;")
        );
    }
}
