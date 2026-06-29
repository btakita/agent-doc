//! Pure route-owned supervisor completion policy.
//!
//! This module decides whether a route-owned supervisor pane should reap itself
//! after a committed document cycle. It does not read documents, inspect panes,
//! spawn processes, or write logs; callers provide the liveness facts gathered
//! from their effectful adapters.

use std::{fmt, str::FromStr};

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
}
