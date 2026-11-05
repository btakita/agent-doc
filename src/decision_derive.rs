//! `agent-doc decision derive` (`#firstprinciplesstore`): author one
//! first-principles derivation step and store it in the session document's
//! decision graph.

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_preflight_io::derivation_store::{self, DeriveRequest};

pub fn run_derive(file: &Path, request: &DeriveRequest, json: bool) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let composition = agent_doc_preflight_command_io::composed_rules_and_principles(file, &content);
    let outcome = derivation_store::derive(file, &content, &composition.principles, request)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return Ok(());
    }
    let derivation = &outcome.derivation;
    println!(
        "{} {} [{}]",
        if outcome.created {
            "derived"
        } else {
            "already on record:"
        },
        derivation.id,
        derivation_store::status_str(derivation.status)
    );
    println!(
        "  concludes {}: “{}”",
        derivation.decision, derivation.answer
    );
    println!("  because: {}", derivation.inference);
    for premise in &derivation.premises {
        println!(
            "  from {} [#{}] “{}”",
            premise.kind, premise.id, premise.label
        );
    }
    if let Some(note) = &derivation.review_note {
        println!("  review note: {note}");
    }
    for contradiction in &outcome.contradictions {
        let answers: Vec<String> = contradiction
            .derivations
            .iter()
            .map(|(id, answer)| format!("{id} “{answer}”"))
            .collect();
        println!(
            "  contradiction ({}): {}",
            match contradiction.kind {
                agent_doc_principles::derivation::ContradictionKind::SamePremises =>
                    "same premises, different answers: an inference step is wrong",
                agent_doc_principles::derivation::ContradictionKind::DifferentPremises =>
                    "different premises, different answers: the premises conflict",
            },
            answers.join("; ")
        );
    }
    if derivation.status
        == agent_doc_principles::derivation::DerivationStatus::AgentDerivedUnreviewed
    {
        println!(
            "  operator review: `{}` in the queue",
            outcome.review_forms.join("` or `")
        );
    }
    Ok(())
}
