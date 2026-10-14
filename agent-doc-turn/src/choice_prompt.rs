//! Safe, harness-neutral projection for a developer choice prompt.
//!
//! Native harness question tools use different request shapes.  This module
//! deliberately accepts only the operator-visible facts common to all of them:
//! a short context, one question, and labelled choices.  It never accepts an
//! opaque rationale or transcript, which keeps hidden deliberation out of the
//! session document.

use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;

const MAX_CONTEXT_CHARS: usize = 600;
const MAX_QUESTION_CHARS: usize = 300;
const MAX_LABEL_CHARS: usize = 80;
const MAX_DESCRIPTION_CHARS: usize = 300;
const MIN_CHOICES: usize = 2;
const MAX_CHOICES: usize = 5;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChoiceCheckpointInput {
    pub context: String,
    pub question: String,
    pub choices: Vec<ChoiceCheckpointOption>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChoiceCheckpointOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoicePromptError(String);

impl fmt::Display for ChoicePromptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ChoicePromptError {}

fn reject(message: impl Into<String>) -> ChoicePromptError {
    ChoicePromptError(message.into())
}

fn validate_field(name: &str, value: &str, max_chars: usize) -> Result<String, ChoicePromptError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(reject(format!("choice checkpoint {name} is empty")));
    }
    if value.contains(['\n', '\r']) {
        return Err(reject(format!("choice checkpoint {name} must be one line")));
    }
    if value.chars().any(char::is_control) {
        return Err(reject(format!(
            "choice checkpoint {name} contains a control character"
        )));
    }
    if value.chars().count() > max_chars {
        return Err(reject(format!(
            "choice checkpoint {name} exceeds {max_chars} characters"
        )));
    }
    if value.contains("<!--") || value.contains("-->") {
        return Err(reject(format!(
            "choice checkpoint {name} may not contain HTML comments"
        )));
    }
    Ok(value.to_string())
}

fn escape_markdown_inline(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(
            character,
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '<' | '>' | '#'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Validate and render the public choice record placed in the cycle's salient
/// response node.  The enclosing node is assistant-authored control state, so
/// this body can never be re-read as a new operator prompt or completion proof.
pub fn render_choice_checkpoint(input: ChoiceCheckpointInput) -> Result<String, ChoicePromptError> {
    let context = validate_field("context", &input.context, MAX_CONTEXT_CHARS)?;
    let question = validate_field("question", &input.question, MAX_QUESTION_CHARS)?;
    if !(MIN_CHOICES..=MAX_CHOICES).contains(&input.choices.len()) {
        return Err(reject(format!(
            "choice checkpoint requires {MIN_CHOICES} to {MAX_CHOICES} choices"
        )));
    }

    let mut labels = HashSet::new();
    let mut choices = Vec::with_capacity(input.choices.len());
    for (index, choice) in input.choices.into_iter().enumerate() {
        let label = validate_field(
            &format!("choice {} label", index + 1),
            &choice.label,
            MAX_LABEL_CHARS,
        )?;
        let description = validate_field(
            &format!("choice {} description", index + 1),
            &choice.description,
            MAX_DESCRIPTION_CHARS,
        )?;
        if !labels.insert(label.to_lowercase()) {
            return Err(reject("choice checkpoint labels must be unique"));
        }
        choices.push((label, description));
    }

    let mut rendered = format!(
        "**Awaiting developer choice**\n\n**Context:** {}\n\n**Question:** {}\n\n**Choices:**\n",
        escape_markdown_inline(&context),
        escape_markdown_inline(&question),
    );
    for (index, (label, description)) in choices.iter().enumerate() {
        rendered.push_str(&format!(
            "\n{}. **{}** — {}",
            index + 1,
            escape_markdown_inline(label),
            escape_markdown_inline(description),
        ));
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> ChoiceCheckpointInput {
        ChoiceCheckpointInput {
            context: "The release can keep compatibility or adopt the new format.".to_string(),
            question: "Which migration policy should this release use?".to_string(),
            choices: vec![
                ChoiceCheckpointOption {
                    label: "Keep compatibility (Recommended)".to_string(),
                    description: "Accept both formats for one release.".to_string(),
                },
                ChoiceCheckpointOption {
                    label: "Switch now".to_string(),
                    description: "Require the new format immediately.".to_string(),
                },
            ],
        }
    }

    #[test]
    fn renders_only_the_public_context_question_and_choices() {
        assert_eq!(
            render_choice_checkpoint(valid_input()).unwrap(),
            "**Awaiting developer choice**\n\n\
**Context:** The release can keep compatibility or adopt the new format.\n\n\
**Question:** Which migration policy should this release use?\n\n\
**Choices:**\n\n\
1. **Keep compatibility (Recommended)** — Accept both formats for one release.\n\
2. **Switch now** — Require the new format immediately."
        );
    }

    #[test]
    fn escapes_markdown_that_could_impersonate_exchange_structure() {
        let mut input = valid_input();
        input.context = "Use <draft> or #stable.".to_string();
        input.choices[0].label = "[draft]".to_string();
        let rendered = render_choice_checkpoint(input).unwrap();
        assert!(rendered.contains("Use \\<draft\\> or \\#stable."));
        assert!(rendered.contains("**\\[draft\\]**"));
    }

    #[test]
    fn rejects_unstructured_or_ambiguous_choice_records() {
        let mut input = valid_input();
        input.context = "public context\nprivate scratch".to_string();
        assert!(render_choice_checkpoint(input).is_err());

        let mut input = valid_input();
        input.choices.truncate(1);
        assert!(render_choice_checkpoint(input).is_err());

        let mut input = valid_input();
        input.choices[1].label = input.choices[0].label.to_uppercase();
        assert!(render_choice_checkpoint(input).is_err());

        let mut input = valid_input();
        input.question = "<!-- agent:queue -->".to_string();
        assert!(render_choice_checkpoint(input).is_err());
    }
}
