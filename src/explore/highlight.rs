//! Presentation of one authoritative document snapshot in the terminal.

use super::{clip_to_width, printable};
use agent_doc_document::markdown_highlight::{MarkdownSyntax, markdown_syntax_ranges};
use agent_doc_syntax::{VisualTokenKind, collect_visual_tokens};
use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::{
    Attribute, Color, ContentStyle, Print, ResetColor, SetAttributes, SetForegroundColor,
};
use std::collections::{BTreeMap, btree_map::Entry};
use std::io::{self, Write};
use std::ops::Range;

pub(super) fn enabled() -> bool {
    !std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
        && std::env::var_os("TERM").as_deref() != Some("dumb".as_ref())
}

pub(super) struct SourcePreview {
    content: String,
    lines: Vec<Range<usize>>,
    highlights: Vec<(Range<usize>, ContentStyle)>,
}

impl SourcePreview {
    pub(super) fn new(content: String, enabled: bool) -> Self {
        let mut offset = 0;
        let lines = content
            .split_inclusive('\n')
            .map(|raw| {
                let line = raw
                    .strip_suffix('\n')
                    .map_or(raw, |line| line.strip_suffix('\r').unwrap_or(line));
                let range = offset..offset + line.len();
                offset += raw.len();
                range
            })
            .collect();
        // The projection owns both text and byte ranges, so refresh replaces
        // them together. Scrolling and resizing only read the visible spans.
        let highlights = if enabled {
            highlight_ranges(&content)
        } else {
            Vec::new()
        };
        Self {
            content,
            lines,
            highlights,
        }
    }

    pub(super) fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub(super) fn print_line(
        &self,
        output: &mut impl Write,
        x: u16,
        y: u16,
        width: u16,
        index: usize,
    ) -> io::Result<()> {
        let mut columns = usize::from(width);
        let prefix = format!("{:>4}  ", index + 1);
        let visible_prefix = clip_to_width(&prefix, &mut columns);
        queue!(output, MoveTo(x, y), Print(visible_prefix))?;
        if visible_prefix.len() < prefix.len() {
            return Ok(());
        }

        let line = &self.lines[index];
        let end = line.start + clip_to_width(&self.content[line.clone()], &mut columns).len();
        if line.start == end {
            return Ok(());
        }
        let mut cursor = line.start;
        let start = self
            .highlights
            .partition_point(|(range, _)| range.end <= cursor);
        for (range, style) in self.highlights[start..]
            .iter()
            .take_while(|(range, _)| range.start < end)
        {
            if cursor < range.start {
                self.print_text(output, cursor..range.start)?;
                cursor = range.start;
            }
            if let Some(color) = style.foreground_color {
                queue!(output, SetForegroundColor(color))?;
            }
            queue!(output, SetAttributes(style.attributes))?;
            let next = range.end.min(end);
            self.print_text(output, cursor..next)?;
            queue!(output, ResetColor)?;
            cursor = next;
        }
        if cursor < end {
            self.print_text(output, cursor..end)?;
        }
        Ok(())
    }

    fn print_text(&self, output: &mut impl Write, range: Range<usize>) -> io::Result<()> {
        queue!(
            output,
            Print(printable(&self.content[range]).collect::<String>())
        )
    }
}

// Later variants take foreground precedence. Attributes can compose, so a
// heading with an emphasized link preserves the heading and emphasis styles.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Syntax {
    Italic,
    Bold,
    Strike,
    Comment,
    Heading,
    Link,
    Marker,
    Tag,
    LocalReport,
    PublishedReport,
    PublishedLocalReport,
    Prompt,
    Code,
}

impl Syntax {
    fn markdown(kind: MarkdownSyntax) -> Self {
        match kind {
            MarkdownSyntax::Heading => Self::Heading,
            MarkdownSyntax::Emphasis => Self::Italic,
            MarkdownSyntax::Strong => Self::Bold,
            MarkdownSyntax::Strikethrough => Self::Strike,
            MarkdownSyntax::Link => Self::Link,
            MarkdownSyntax::Code => Self::Code,
        }
    }

    fn agent_doc(kind: VisualTokenKind) -> Option<Self> {
        match kind {
            VisualTokenKind::ComponentOpen
            | VisualTokenKind::ComponentClose
            | VisualTokenKind::PatchOpen
            | VisualTokenKind::PatchClose
            | VisualTokenKind::Boundary => Some(Self::Marker),
            VisualTokenKind::ScratchComment | VisualTokenKind::ScratchCommentBody => {
                Some(Self::Comment)
            }
            VisualTokenKind::Prompt => Some(Self::Prompt),
            VisualTokenKind::ResponseHeading => Some(Self::Heading),
            VisualTokenKind::TrackedId | VisualTokenKind::LabelTag => Some(Self::Tag),
            VisualTokenKind::QueueReportLocal => Some(Self::LocalReport),
            VisualTokenKind::QueueReportPublished => Some(Self::PublishedReport),
            VisualTokenKind::QueueReportPublishedLocal => Some(Self::PublishedLocalReport),
            VisualTokenKind::ComponentBody
            | VisualTokenKind::Recent1
            | VisualTokenKind::Recent2
            | VisualTokenKind::Recent3
            | VisualTokenKind::Bold
            | VisualTokenKind::Italic => None,
        }
    }

    fn style(self) -> (Option<Color>, Option<Attribute>) {
        match self {
            Self::Italic => (None, Some(Attribute::Italic)),
            Self::Bold => (None, Some(Attribute::Bold)),
            Self::Strike => (None, Some(Attribute::CrossedOut)),
            Self::Comment => (Some(Color::DarkGrey), None),
            Self::Heading => (Some(Color::Cyan), Some(Attribute::Bold)),
            Self::Link => (Some(Color::Blue), Some(Attribute::Underlined)),
            Self::Marker => (Some(Color::Magenta), None),
            Self::Tag => (Some(Color::Yellow), None),
            Self::LocalReport => (Some(Color::DarkYellow), None),
            Self::PublishedReport => (Some(Color::Green), None),
            Self::PublishedLocalReport => (Some(Color::Cyan), None),
            Self::Prompt => (Some(Color::Green), Some(Attribute::Bold)),
            Self::Code => (Some(Color::DarkCyan), None),
        }
    }
}

fn highlight_ranges(content: &str) -> Vec<(Range<usize>, ContentStyle)> {
    let markdown =
        markdown_syntax_ranges(content).map(|(range, kind)| (range, Syntax::markdown(kind)));
    let agent_doc = collect_visual_tokens(content)
        .into_iter()
        .filter_map(|token| Syntax::agent_doc(token.kind).map(|s| (token.start..token.end, s)));
    let mut edges: Vec<_> = markdown
        .chain(agent_doc)
        .filter(|(range, _)| !range.is_empty())
        .flat_map(|(range, syntax)| [(range.start, true, syntax), (range.end, false, syntax)])
        .collect();
    edges.sort_unstable();

    let mut highlights: Vec<(Range<usize>, ContentStyle)> = Vec::new();
    let mut active = BTreeMap::<Syntax, usize>::new();
    let mut start = 0;
    for (offset, entering, syntax) in edges {
        if start < offset && !active.is_empty() {
            let style = active_style(&active);
            if let Some((previous, previous_style)) = highlights.last_mut()
                && previous.end == start
                && *previous_style == style
            {
                previous.end = offset;
            } else {
                highlights.push((start..offset, style));
            }
        }
        if entering {
            *active.entry(syntax).or_default() += 1;
        } else if let Entry::Occupied(mut entry) = active.entry(syntax) {
            *entry.get_mut() -= 1;
            if *entry.get() == 0 {
                entry.remove();
            }
        }
        start = offset;
    }
    highlights
}

fn active_style(active: &BTreeMap<Syntax, usize>) -> ContentStyle {
    if active.contains_key(&Syntax::Code) {
        // Markup inside code stays literal, even for overlapping outer styles.
        return ContentStyle {
            foreground_color: Syntax::Code.style().0,
            ..ContentStyle::default()
        };
    }
    let mut style = ContentStyle::default();
    for syntax in active.keys() {
        let (foreground, attribute) = syntax.style();
        style.foreground_color = foreground.or(style.foreground_color);
        if let Some(attribute) = attribute {
            style.attributes.set(attribute);
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_markdown_and_agent_doc_without_interpreting_code_contents() {
        let source = "# Hé界ding **_nested_** [link](https://example.test)

<!-- agent:exchange -->

❯ Prompt

### Re: Reply

<!-- /agent:exchange -->

- Work [#task] [recommended] ⟦local⟧

<!-- comment -->

~~struck~~ `[#literal] **inline**`

```rust
<!-- agent:queue -->
# Code [#fenced]
```

    # [#indented]
";
        let highlights = highlight_ranges(source);
        let style_at = |needle: &str| {
            let byte = source.find(needle).unwrap();
            highlights
                .iter()
                .find_map(|(range, style)| range.contains(&byte).then_some(*style))
                .unwrap_or_else(|| panic!("missing highlight for {needle}"))
        };
        assert_eq!(style_at("Hé界ding").foreground_color, Some(Color::Cyan));
        let nested = style_at("nested");
        assert!(nested.attributes.has(Attribute::Bold));
        assert!(nested.attributes.has(Attribute::Italic));
        assert_eq!(nested.foreground_color, Some(Color::Cyan));
        assert!(style_at("link").attributes.has(Attribute::Underlined));
        assert!(style_at("struck").attributes.has(Attribute::CrossedOut));
        assert_eq!(
            style_at("agent:exchange").foreground_color,
            Some(Color::Magenta)
        );
        assert_eq!(style_at("❯ Prompt").foreground_color, Some(Color::Green));
        assert_eq!(style_at("Re: Reply").foreground_color, Some(Color::Cyan));
        assert_eq!(style_at("[#task]").foreground_color, Some(Color::Yellow));
        assert_eq!(
            style_at("[recommended]").foreground_color,
            Some(Color::Yellow)
        );
        assert_eq!(
            style_at("⟦local⟧").foreground_color,
            Some(Color::DarkYellow)
        );
        assert_eq!(style_at("comment").foreground_color, Some(Color::DarkGrey));
        for needle in [
            "[#literal]",
            "```rust",
            "agent:queue",
            "[#fenced]",
            "[#indented]",
        ] {
            let code = style_at(needle);
            assert_eq!(code.foreground_color, Some(Color::DarkCyan), "{needle}");
            assert!(code.attributes.is_empty(), "{needle}");
        }
        assert!(
            highlights
                .windows(2)
                .all(|pair| pair[0].0.end <= pair[1].0.start)
        );
    }

    #[test]
    fn renders_styled_source_without_leaking_controls_or_counting_escape_sequences_as_cells() {
        let content = "first\r\n**é界**\x1bnext\r\n".to_owned();
        let preview = SourcePreview::new(content.clone(), true);
        assert_eq!(preview.line_count(), 2);
        let mut styled = Vec::new();
        preview.print_line(&mut styled, 0, 0, 15, 1).unwrap();
        let styled = String::from_utf8(styled).unwrap();
        assert!(styled.ends_with("   2  \x1b[1m**é界**\x1b[0m n"));

        let mut plain = Vec::new();
        SourcePreview::new(content, false)
            .print_line(&mut plain, 0, 0, 15, 1)
            .unwrap();
        let plain = String::from_utf8(plain).unwrap();
        assert!(plain.ends_with("   2  **é界** n"));
        assert!(!plain.contains("\x1b[1m"));

        let preview = SourcePreview::new("# 界".to_owned(), true);
        let mut clipped = Vec::new();
        preview.print_line(&mut clipped, 0, 0, 9, 0).unwrap();
        let clipped = String::from_utf8(clipped).unwrap();
        assert!(clipped.ends_with("# \x1b[0m"));
        assert!(!clipped.contains('界'));
    }
}
