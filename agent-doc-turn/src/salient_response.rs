//! Pure admission policy for operator-visible, non-final response progress.
//!
//! Semantic salience is an explicit harness/agent declaration. This module owns
//! the structural boundary: a declared checkpoint must be standalone Markdown
//! and must not smuggle agent-doc protocol markers into the live document.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SalientCheckpointRejection {
    Empty,
    ProtocolMarker,
    UnbalancedFence,
}

impl SalientCheckpointRejection {
    pub const fn message(&self) -> &'static str {
        match self {
            Self::Empty => "salient response checkpoint is empty",
            Self::ProtocolMarker => {
                "salient response checkpoint may not contain agent-doc protocol markers"
            }
            Self::UnbalancedFence => {
                "salient response checkpoint has an unbalanced Markdown code fence"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SalientCheckpointDecision {
    Apply { body: String },
    Reject(SalientCheckpointRejection),
}

fn fence_delimiter(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let character = trimmed.chars().next()?;
    if !matches!(character, '`' | '~') {
        return None;
    }
    let count = trimmed
        .chars()
        .take_while(|candidate| *candidate == character)
        .count();
    (count >= 3).then_some((character, count))
}

fn fences_balanced(body: &str) -> bool {
    let mut open: Option<(char, usize)> = None;
    for line in body.lines() {
        let Some(candidate) = fence_delimiter(line) else {
            continue;
        };
        match open {
            None => open = Some(candidate),
            Some((character, count))
                if candidate.0 == character
                    && candidate.1 >= count
                    && line.trim_start()[candidate.1..].trim().is_empty() =>
            {
                open = None;
            }
            Some(_) => {}
        }
    }
    open.is_none()
}

/// Decide whether explicitly declared salient text is structurally safe to
/// project into the live document.
pub fn decide_salient_checkpoint(text: &str) -> SalientCheckpointDecision {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let body = normalized.trim_matches('\n');
    if body.trim().is_empty() {
        return SalientCheckpointDecision::Reject(SalientCheckpointRejection::Empty);
    }
    if body.contains("<!-- agent:")
        || body.contains("<!-- /agent:")
        || body.contains("<!-- patch:")
        || body.contains("<!-- /patch:")
    {
        return SalientCheckpointDecision::Reject(SalientCheckpointRejection::ProtocolMarker);
    }
    if !fences_balanced(body) {
        return SalientCheckpointDecision::Reject(SalientCheckpointRejection::UnbalancedFence);
    }
    SalientCheckpointDecision::Apply {
        body: body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_standalone_salient_markdown_and_normalizes_newlines() {
        assert_eq!(
            decide_salient_checkpoint("\r\nDiagnosis confirmed.\r\n\r\n- proof\r\n"),
            SalientCheckpointDecision::Apply {
                body: "Diagnosis confirmed.\n\n- proof".to_string(),
            }
        );
    }

    #[test]
    fn rejects_empty_protocol_and_unbalanced_candidates() {
        assert_eq!(
            decide_salient_checkpoint(" \n"),
            SalientCheckpointDecision::Reject(SalientCheckpointRejection::Empty)
        );
        assert_eq!(
            decide_salient_checkpoint("<!-- agent:queue -->"),
            SalientCheckpointDecision::Reject(SalientCheckpointRejection::ProtocolMarker)
        );
        assert_eq!(
            decide_salient_checkpoint("```rust\nfn main() {}"),
            SalientCheckpointDecision::Reject(SalientCheckpointRejection::UnbalancedFence)
        );
    }

    #[test]
    fn admits_balanced_fences_without_treating_inner_fences_as_closers() {
        let body = "````md\n```rust\nfn main() {}\n```\n````";
        assert!(matches!(
            decide_salient_checkpoint(body),
            SalientCheckpointDecision::Apply { .. }
        ));
    }
}
