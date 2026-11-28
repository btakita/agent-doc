//! Markdown source ranges for syntax highlighting.

use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownSyntax {
    Heading,
    Emphasis,
    Strong,
    Strikethrough,
    Link,
    Code,
}

/// Return byte ranges in the original source, including their Markdown delimiters.
pub fn markdown_syntax_ranges(
    content: &str,
) -> impl Iterator<Item = (Range<usize>, MarkdownSyntax)> + '_ {
    Parser::new_ext(
        content,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS,
    )
    .into_offset_iter()
    .filter_map(|(event, range)| {
        let kind = match event {
            Event::Start(Tag::Heading { .. }) => MarkdownSyntax::Heading,
            Event::Start(Tag::Emphasis) => MarkdownSyntax::Emphasis,
            Event::Start(Tag::Strong) => MarkdownSyntax::Strong,
            Event::Start(Tag::Strikethrough) => MarkdownSyntax::Strikethrough,
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => MarkdownSyntax::Link,
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => MarkdownSyntax::Code,
            _ => return None,
        };
        Some((range, kind))
    })
}
