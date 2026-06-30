//! Pure route-owned supervisor completion policy.
//!
//! This module decides whether a route-owned supervisor pane should reap itself
//! after a committed document cycle. It does not read documents, inspect panes,
//! spawn processes, or write logs; callers provide the liveness facts gathered
//! from their effectful adapters.

use std::{fmt, str::FromStr};

use agent_doc_prompt_lines::text_line_looks_like_prompt_target;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteOwnedReapPolicy {
    Auto,
    ReapAfterCommit,
    KeepAlive,
}

impl RouteOwnedReapPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ReapAfterCommit => "reap_after_commit",
            Self::KeepAlive => "keep_alive",
        }
    }
}

impl fmt::Display for RouteOwnedReapPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::ReapAfterCommit => "reap-after-commit",
            Self::KeepAlive => "keep-alive",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRouteOwnedReapPolicyError {
    value: String,
}

impl fmt::Display for ParseRouteOwnedReapPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid route-owned reap policy {:?}; expected auto, reap-after-commit, or keep-alive",
            self.value
        )
    }
}

impl std::error::Error for ParseRouteOwnedReapPolicyError {}

impl FromStr for RouteOwnedReapPolicy {
    type Err = ParseRouteOwnedReapPolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "reap-after-commit" | "reap_after_commit" => Ok(Self::ReapAfterCommit),
            "keep-alive" | "keep_alive" => Ok(Self::KeepAlive),
            _ => Err(ParseRouteOwnedReapPolicyError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOwnedLivenessReason {
    BacklogNonEmpty,
    QueueNonEmpty,
    PostCommitUserFollowUp,
    ExchangeTailUnresolvedPrompt,
    DocumentDirtyAfterCommit,
    AdapterFailure(String),
}

impl RouteOwnedLivenessReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::BacklogNonEmpty => "backlog_non_empty",
            Self::QueueNonEmpty => "queue_non_empty",
            Self::PostCommitUserFollowUp => "post_commit_user_follow_up",
            Self::ExchangeTailUnresolvedPrompt => "exchange_tail_unresolved_prompt",
            Self::DocumentDirtyAfterCommit => "document_dirty_after_commit",
            Self::AdapterFailure(reason) => reason,
        }
    }
}

impl fmt::Display for RouteOwnedLivenessReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteOwnedReapDecision {
    pub reap: bool,
    pub reason: String,
}

pub fn route_owned_reap_decision(
    policy: RouteOwnedReapPolicy,
    liveness_reason: Option<RouteOwnedLivenessReason>,
) -> RouteOwnedReapDecision {
    match policy {
        RouteOwnedReapPolicy::KeepAlive => RouteOwnedReapDecision {
            reap: false,
            reason: "explicit_keep_alive".to_string(),
        },
        RouteOwnedReapPolicy::ReapAfterCommit => RouteOwnedReapDecision {
            reap: true,
            reason: "explicit_reap_after_commit".to_string(),
        },
        RouteOwnedReapPolicy::Auto => {
            if let Some(reason) = liveness_reason {
                RouteOwnedReapDecision {
                    reap: false,
                    reason: reason.to_string(),
                }
            } else {
                RouteOwnedReapDecision {
                    reap: true,
                    reason: "no_liveness_signals".to_string(),
                }
            }
        }
    }
}

pub fn route_owned_file_dirty_after_commit(
    content: &str,
    committed_file_hash: Option<&str>,
) -> bool {
    committed_file_hash.is_some_and(|hash| agent_doc_hash::content_hash(content) != hash)
}

pub fn route_owned_liveness_reason_for_content(
    content: &str,
    committed_file_hash: Option<&str>,
) -> Option<RouteOwnedLivenessReason> {
    let dirty_after_commit = route_owned_file_dirty_after_commit(content, committed_file_hash);
    if dirty_after_commit && route_owned_exchange_tail_has_unresolved_prompt(content) {
        return Some(RouteOwnedLivenessReason::PostCommitUserFollowUp);
    }

    let components = match agent_doc_element::element::parse(content) {
        Ok(components) => components,
        Err(err) => {
            return Some(if dirty_after_commit {
                RouteOwnedLivenessReason::DocumentDirtyAfterCommit
            } else {
                RouteOwnedLivenessReason::AdapterFailure(format!("component_parse_failed:{err}"))
            });
        }
    };

    for component in &components {
        let body = component.content(content);
        if agent_doc_element::element::is_backlog_component(&component.name)
            && route_owned_backlog_has_live_items(body)
        {
            return Some(RouteOwnedLivenessReason::BacklogNonEmpty);
        }
        if component.name == "queue" && route_owned_queue_has_prompts(body) {
            return Some(RouteOwnedLivenessReason::QueueNonEmpty);
        }
        if component.name == "exchange" && route_owned_exchange_tail_has_unresolved_prompt(body) {
            return Some(if dirty_after_commit {
                RouteOwnedLivenessReason::PostCommitUserFollowUp
            } else {
                RouteOwnedLivenessReason::ExchangeTailUnresolvedPrompt
            });
        }
    }

    if dirty_after_commit {
        return Some(RouteOwnedLivenessReason::DocumentDirtyAfterCommit);
    }

    None
}

pub fn route_owned_backlog_has_live_items(body: &str) -> bool {
    let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(body);
    items
        .iter()
        .any(|item| item.state != agent_doc_element_backlog::backlog::PendingState::Done)
}

pub fn route_owned_queue_has_prompts(body: &str) -> bool {
    match agent_doc_queue::document_queue::parse(body) {
        Ok(entries) => !agent_doc_queue::document_queue::prompts(&entries).is_empty(),
        Err(_) => !body.trim().is_empty(),
    }
}

pub fn route_owned_exchange_tail_has_unresolved_prompt(body: &str) -> bool {
    let mut tail_start = 0usize;
    let mut line_start = 0usize;
    for line in body.split_inclusive('\n') {
        if route_owned_line_is_response_heading(line.trim()) {
            tail_start = line_start + line.len();
        }
        line_start += line.len();
    }
    if line_start < body.len() && route_owned_line_is_response_heading(body[line_start..].trim()) {
        tail_start = body.len();
    }

    body[tail_start..]
        .lines()
        .any(text_line_looks_like_prompt_target)
}

fn route_owned_line_is_response_heading(line: &str) -> bool {
    line == "## Assistant"
        || line.starts_with("### Re:")
        || line.starts_with("#### Re:")
        || line.starts_with("##### Re:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_reaps_without_liveness_signals() {
        assert_eq!(
            route_owned_reap_decision(RouteOwnedReapPolicy::Auto, None),
            RouteOwnedReapDecision {
                reap: true,
                reason: "no_liveness_signals".to_string()
            }
        );
    }

    #[test]
    fn auto_keeps_alive_for_each_liveness_reason() {
        for reason in [
            RouteOwnedLivenessReason::BacklogNonEmpty,
            RouteOwnedLivenessReason::QueueNonEmpty,
            RouteOwnedLivenessReason::PostCommitUserFollowUp,
            RouteOwnedLivenessReason::ExchangeTailUnresolvedPrompt,
            RouteOwnedLivenessReason::DocumentDirtyAfterCommit,
        ] {
            assert_eq!(
                route_owned_reap_decision(RouteOwnedReapPolicy::Auto, Some(reason.clone())),
                RouteOwnedReapDecision {
                    reap: false,
                    reason: reason.as_str().to_string()
                }
            );
        }
    }

    #[test]
    fn auto_keeps_alive_with_adapter_failure_reason() {
        assert_eq!(
            route_owned_reap_decision(
                RouteOwnedReapPolicy::Auto,
                Some(RouteOwnedLivenessReason::AdapterFailure(
                    "read_failed:permission denied".to_string()
                ))
            ),
            RouteOwnedReapDecision {
                reap: false,
                reason: "read_failed:permission denied".to_string()
            }
        );
    }

    #[test]
    fn explicit_reap_overrides_liveness() {
        assert_eq!(
            route_owned_reap_decision(
                RouteOwnedReapPolicy::ReapAfterCommit,
                Some(RouteOwnedLivenessReason::BacklogNonEmpty)
            ),
            RouteOwnedReapDecision {
                reap: true,
                reason: "explicit_reap_after_commit".to_string()
            }
        );
    }

    #[test]
    fn explicit_keep_alive_overrides_missing_liveness() {
        assert_eq!(
            route_owned_reap_decision(RouteOwnedReapPolicy::KeepAlive, None),
            RouteOwnedReapDecision {
                reap: false,
                reason: "explicit_keep_alive".to_string()
            }
        );
    }

    #[test]
    fn parse_reap_policy_accepts_cli_and_log_spellings() {
        assert_eq!("auto".parse(), Ok(RouteOwnedReapPolicy::Auto));
        assert_eq!(
            "reap-after-commit".parse(),
            Ok(RouteOwnedReapPolicy::ReapAfterCommit)
        );
        assert_eq!(
            "reap_after_commit".parse(),
            Ok(RouteOwnedReapPolicy::ReapAfterCommit)
        );
        assert_eq!("keep-alive".parse(), Ok(RouteOwnedReapPolicy::KeepAlive));
        assert_eq!("keep_alive".parse(), Ok(RouteOwnedReapPolicy::KeepAlive));
        assert!("never".parse::<RouteOwnedReapPolicy>().is_err());
    }

    #[test]
    fn route_owned_body_liveness_detects_backlog_queue_and_exchange_tail() {
        assert!(route_owned_backlog_has_live_items(
            "- [ ] [#next] Continue\n- [x] [#done] Finished\n"
        ));
        assert!(!route_owned_backlog_has_live_items(
            "- [x] [#done] Finished\n"
        ));

        assert!(route_owned_queue_has_prompts("- do #next\n"));
        assert!(!route_owned_queue_has_prompts("<!-- empty -->\n"));

        let body = "\
### Re: done — gpt-5
Done.

do #next
";
        assert!(route_owned_exchange_tail_has_unresolved_prompt(body));
    }

    #[test]
    fn route_owned_exchange_tail_ignores_prompt_text_before_latest_response() {
        let body = "\
### Re: earlier — gpt-5
Do #old after this.

### Re: latest — gpt-5
Done.
";

        assert!(!route_owned_exchange_tail_has_unresolved_prompt(body));
    }

    fn committed_hash(content: &str) -> String {
        agent_doc_hash::content_hash(content)
    }

    #[test]
    fn route_owned_content_liveness_detects_live_backlog() {
        let content = "\
<!-- agent:exchange -->
### Re: prior — gpt-5
Done.
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] [#next] Continue the session
<!-- /agent:backlog -->
";

        assert_eq!(
            route_owned_liveness_reason_for_content(content, Some(&committed_hash(content))),
            Some(RouteOwnedLivenessReason::BacklogNonEmpty)
        );
    }

    #[test]
    fn route_owned_content_liveness_detects_queue_prompt() {
        let content = "\
<!-- agent:queue -->
- do #next
<!-- /agent:queue -->
";

        assert_eq!(
            route_owned_liveness_reason_for_content(content, Some(&committed_hash(content))),
            Some(RouteOwnedLivenessReason::QueueNonEmpty)
        );
    }

    #[test]
    fn route_owned_content_liveness_names_post_commit_user_follow_up() {
        let committed =
            "<!-- agent:exchange -->\n### Re: done — gpt-5\nDone.\n<!-- /agent:exchange -->\n";
        let edited = format!("{committed}\nnew prompt?\n");

        assert_eq!(
            route_owned_liveness_reason_for_content(&edited, Some(&committed_hash(committed))),
            Some(RouteOwnedLivenessReason::PostCommitUserFollowUp)
        );
    }

    #[test]
    fn route_owned_content_liveness_keeps_non_prompt_dirty_doc_alive() {
        let committed =
            "<!-- agent:exchange -->\n### Re: done — gpt-5\nDone.\n<!-- /agent:exchange -->\n";
        let edited = format!("{committed}\n<!-- local note -->\n");

        assert_eq!(
            route_owned_liveness_reason_for_content(&edited, Some(&committed_hash(committed))),
            Some(RouteOwnedLivenessReason::DocumentDirtyAfterCommit)
        );
    }

    #[test]
    fn route_owned_content_liveness_is_empty_when_no_signals() {
        let content = "\
<!-- agent:exchange -->
### Re: done — gpt-5
Done.
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";

        assert_eq!(
            route_owned_liveness_reason_for_content(content, Some(&committed_hash(content))),
            None
        );
    }
}
