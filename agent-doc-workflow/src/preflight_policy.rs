//! Pure preflight warning, note-formatting, and linked-content policy.

use indexmap::IndexMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightPolicyWarning {
    pub code: String,
    pub message: String,
}

pub fn post_exchange_comment_prompt_preset_warning(
    file_display: &str,
    content: &str,
    prompt_presets: &IndexMap<String, String>,
) -> Option<PreflightPolicyWarning> {
    let mut referenced = Vec::new();
    for comment in agent_doc_diff::post_exchange_ordinary_html_comments(content) {
        if !prompt_presets.is_empty() {
            agent_doc_prompt_contract::push_unique_strings(
                &mut referenced,
                agent_doc_prompt_contract::requested_prompt_presets(
                    std::slice::from_ref(&comment),
                    &[],
                    prompt_presets,
                ),
            );
        }
        agent_doc_prompt_contract::push_unique_strings(
            &mut referenced,
            agent_doc_diff::post_exchange_comment_directive_signals(&comment),
        );
    }
    if referenced.is_empty() {
        return None;
    }

    Some(PreflightPolicyWarning {
        code: "post_exchange_comment_prompt_preset".to_string(),
        message: format!(
            "Post-exchange HTML comment in {file_display} references prompt preset/directive text ({}) that is preserved as a non-executable user note. Move it into `agent:exchange` or `agent:queue` if it should run.",
            referenced.join(", ")
        ),
    })
}

pub fn preset_item_id_collision_warning(content: &str) -> Option<PreflightPolicyWarning> {
    let collisions = agent_doc_element_backlog::backlog::detect_identity_collisions(content);
    if collisions.is_empty() {
        return None;
    }
    Some(PreflightPolicyWarning {
        code: "preset_item_id_collision".to_string(),
        message: format!(
            "Ambiguous identities — the same #id resolves under multiple active sources: {}. Each #id must have one active meaning per document, so `do #id`, queue generation, and \"top backlog item\" are unambiguous. Rename the colliding prompt preset or tracked item before dispatch. (#preset-item-id-collision)",
            collisions.join("; ")
        ),
    })
}

pub fn format_ipc_dogfood_note(diagnostic: &str) -> String {
    let diagnostic = diagnostic.replace("```", "'''");
    // The note opens with a `### Re:` response heading and folds the body into
    // a fenced block so prompt-bearing diff classifiers see one binary-authored
    // recovery artifact block instead of a fresh user prompt.
    format!(
        "### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc\n\n\
```text\n\
**IPC proof issue dogfood log**\n\
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.\n\
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.\n\
Issue class: `ipc_proof_insufficient`\n\
Affected component: editor IPC / writeback\n\n\
{}\n\
```",
        diagnostic
    )
}

pub fn is_url(link: &str) -> bool {
    link.starts_with("http://") || link.starts_with("https://")
}

pub fn url_cache_path(cache_dir: &Path, url: &str) -> PathBuf {
    let hash = agent_doc_hash::content_hash(url);
    cache_dir.join(format!("{hash}.txt"))
}

pub fn html_to_markdown(html: &str) -> String {
    use htmd::HtmlToMarkdown;
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "nav", "footer", "noscript", "svg"])
        .build();
    converter.convert(html).unwrap_or_else(|_| html.to_string())
}

pub fn is_html_content(content_type: &str) -> bool {
    content_type.contains("text/html") || content_type.contains("application/xhtml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn warns_on_prompt_preset_text_inside_post_exchange_html_comment() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "Scratch note while testing.\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        let warning =
            post_exchange_comment_prompt_preset_warning("session.md", content, &fm.prompt_presets)
                .expect("known prompt preset in ordinary post-exchange comment should warn");

        assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
        assert!(
            warning
                .message
                .contains("#spec-test-build-install-commit-push")
        );
        assert!(warning.message.contains("non-executable user note"));
    }

    #[test]
    fn comment_prompt_preset_warning_ignores_agent_components() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:done -->\n",
            "<!-- archived #spec-test-build-install-commit-push -->\n",
            "<!-- /agent:done -->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();

        assert!(
            post_exchange_comment_prompt_preset_warning("session.md", content, &fm.prompt_presets)
                .is_none(),
            "agent-owned queue directives remain executable state, not ordinary scratch comments"
        );
    }

    #[test]
    fn warns_on_dispatch_text_inside_post_exchange_html_comment_without_presets() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "dispatch #manual-review\n",
            "/clear\n",
            "-->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        let warning =
            post_exchange_comment_prompt_preset_warning("session.md", content, &fm.prompt_presets)
                .expect("dispatch-looking text in ordinary post-exchange comment should warn");

        assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
        assert!(warning.message.contains("dispatch #manual-review"));
        assert!(warning.message.contains("/clear"));
    }

    #[test]
    fn preset_item_id_collision_warning_formats_collision() {
        let content = concat!(
            "---\n",
            "prompt_presets:\n",
            "  '#same': do one thing\n",
            "---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#same] do another thing\n",
            "<!-- /agent:backlog -->\n"
        );

        let warning = preset_item_id_collision_warning(content)
            .expect("same id in preset and backlog should warn");

        assert_eq!(warning.code, "preset_item_id_collision");
        assert!(warning.message.contains("#same"));
        assert!(warning.message.contains("Ambiguous identities"));
    }

    #[test]
    fn ipc_dogfood_note_sanitizes_fences_and_marks_recovery_artifact() {
        let note = format_ipc_dogfood_note("before\n```bad\n```");

        assert!(note.starts_with("### Re: IPC proof diagnostic"));
        assert!(note.contains("Issue class: `ipc_proof_insufficient`"));
        assert!(!note.contains("```bad"));
        assert!(note.contains("'''bad"));
    }

    #[test]
    fn is_url_detects_http() {
        assert!(is_url("http://example.com"));
        assert!(is_url("https://example.com/path"));
        assert!(!is_url("../relative/path.md"));
        assert!(!is_url("tasks/software/agent-doc.md"));
        assert!(!is_url(""));
    }

    #[test]
    fn is_html_content_detects_html() {
        assert!(is_html_content("text/html; charset=utf-8"));
        assert!(is_html_content("text/html"));
        assert!(is_html_content("application/xhtml+xml"));
        assert!(!is_html_content("application/json"));
        assert!(!is_html_content("text/plain"));
    }

    #[test]
    fn html_to_markdown_converts_basic_html() {
        let html = "<h1>Title</h1><p>Hello <strong>world</strong>.</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Title"), "should contain heading text");
        assert!(md.contains("**world**"), "should convert bold");
    }

    #[test]
    fn html_to_markdown_strips_script_and_style() {
        let html =
            "<p>Visible</p><script>alert('xss')</script><style>.foo{}</style><p>Also visible</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Visible"));
        assert!(md.contains("Also visible"));
        assert!(!md.contains("alert"), "script content should be stripped");
        assert!(!md.contains(".foo"), "style content should be stripped");
    }

    #[test]
    fn html_to_markdown_strips_nav_and_footer() {
        let html =
            "<nav><a href='/'>Home</a></nav><main><p>Content</p></main><footer>Copyright</footer>";
        let md = html_to_markdown(html);
        assert!(md.contains("Content"));
        assert!(!md.contains("Home"), "nav content should be stripped");
        assert!(
            !md.contains("Copyright"),
            "footer content should be stripped"
        );
    }

    #[test]
    fn url_cache_path_is_deterministic() {
        let dir = TempDir::new().unwrap();
        let p1 = url_cache_path(dir.path(), "https://example.com");
        let p2 = url_cache_path(dir.path(), "https://example.com");
        assert_eq!(p1, p2, "same URL should produce same cache path");

        let p3 = url_cache_path(dir.path(), "https://other.com");
        assert_ne!(
            p1, p3,
            "different URLs should produce different cache paths"
        );
        assert_eq!(p1.extension().and_then(|s| s.to_str()), Some("txt"));
    }
}
