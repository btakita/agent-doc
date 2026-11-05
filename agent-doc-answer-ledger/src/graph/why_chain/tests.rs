//! `#firstprincipleswhy`: `decision why` joins the ledger's answer with the
//! first-principles derivation chain.

use super::*;
use crate::{Ledger, PriorIndex, extract};
use agent_doc_principles::PrincipleScope;

const NOW: u64 = 1_800_000_000;

const DOC: &str = concat!(
    "---\nagent_doc_session: why-chain-fixture\n---\n\n",
    "## Main <!-- agent:run id=\"main\" -->\n",
    "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
    "  - 🙋 [#tdpublic] Decide whether haiven-cli is public and open source <!-- agent-doc:waiting-item -->\n",
    "    - Public and open source, per [#Open-By-Default].\n",
    "    - amend: but keep the release App private\n",
    "  - 🙋 [#tdtoken] Decide whether install.sh embeds a download token <!-- agent-doc:waiting-item -->\n",
    "    - No token in install.sh; depends on [#tdpublic]\n",
    "  - 🙋 [#tdport] Choose the demo callback port <!-- agent-doc:waiting-item -->\n",
    "    - Use port 8080.\n",
    "    - depends on [#least-surprise]\n",
    "  - 🙋 [#tdpolish] Decide whether to ship without the polish pass <!-- agent-doc:waiting-item -->\n",
    "    - Ship now, per [#velocity].\n",
    "    - change: Do the polish pass first.\n",
    "<!-- /agent:run -->\n\n",
    "## Todo <!-- agent:todo -->\n",
    "- [ ] [#tdpublic] Decide whether haiven-cli is public and open source\n",
    "- [ ] [#tdtoken] Decide whether install.sh embeds a download token\n",
    "- [ ] [#tdport] Choose the demo callback port\n",
    "- [ ] [#tdpolish] Decide whether to ship without the polish pass\n",
    "<!-- /agent:todo -->\n",
);

fn build(content: &str) -> DecisionGraph {
    let observations = extract::observe(content, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    DecisionGraph::build(&ledger, &observations)
}

fn principle(id: &str, statement: &str) -> Principle {
    Principle {
        id: id.into(),
        statement: statement.into(),
        rationale: None,
        scope: PrincipleScope::Project,
        source: "/repo/.agent-doc/project.md".into(),
        overrides: Vec::new(),
    }
}

fn principles() -> Vec<Principle> {
    vec![
        principle(
            "open-by-default",
            "Tools are public unless they hold secrets.",
        ),
        principle("least-surprise", "Pick the default a new user expects."),
        principle(
            "velocity",
            "When a release is blocked, velocity over polish.",
        ),
    ]
}

fn decision(graph: &DecisionGraph, item: &str) -> String {
    graph
        .resolve_decision(item)
        .unwrap_or_else(|| panic!("no decision for {item}: {graph:#?}"))
}

fn derivation_for<'a>(derivations: &'a [Derivation], decision: &str) -> Option<&'a Derivation> {
    derivations
        .iter()
        .find(|derivation| derivation.conclusion.decision == decision)
}

#[test]
fn operator_answers_citing_principles_and_depends_on_links_are_confirmed_derivations() {
    let graph = build(DOC);
    let derivations = operator_derivations(&graph, &principles());
    let public = decision(&graph, "tdpublic");
    let token = decision(&graph, "tdtoken");
    let port = decision(&graph, "tdport");
    let polish = decision(&graph, "tdpolish");

    // The amendment keeps the cited first answer in force.
    let step = derivation_for(&derivations, &public).expect("tdpublic is derived");
    assert_eq!(
        step.premises,
        vec![NodeRef::new(PremiseKind::Principle, "open-by-default")],
        "the citation matches case-insensitively and keeps the declared spelling"
    );
    assert_eq!(step.status, DerivationStatus::OperatorConfirmed);
    assert!(step.id.starts_with(OPERATOR_DERIVATION_PREFIX), "{step:?}");
    assert_eq!(
        step.inference,
        "Public and open source, per [#Open-By-Default].; but keep the release App private"
    );
    assert_eq!(step.conclusion.answer, "but keep the release App private");

    let step = derivation_for(&derivations, &token).expect("tdtoken is derived");
    assert_eq!(
        step.premises,
        vec![NodeRef::new(PremiseKind::Decision, public.clone())]
    );

    // `depends on [#principle]` names a principle premise, not an item.
    let step = derivation_for(&derivations, &port).expect("tdport is derived");
    assert_eq!(
        step.premises,
        vec![NodeRef::new(PremiseKind::Principle, "least-surprise")]
    );

    // A superseded answer's citation no longer supports the decision.
    assert!(
        derivation_for(&derivations, &polish).is_none(),
        "{derivations:#?}"
    );
}

#[test]
fn why_walks_from_a_decision_through_its_premise_decision_to_the_principle() {
    let graph = build(DOC);
    let principles = principles();
    let (derivations, skipped) = derivation_graph(&graph, &principles, Vec::new());
    assert!(skipped.is_empty(), "{skipped:?}");
    let token = decision(&graph, "tdtoken");
    let public = decision(&graph, "tdpublic");

    let why = reasoned_why(&graph, &derivations, &principles, &token).unwrap();
    assert_eq!(
        why.why.current_answer.as_deref(),
        Some("No token in install.sh; depends on [#tdpublic]")
    );
    assert_eq!(why.derivation_supported, Some(true));
    let premise = &why.derived_from[0].premises[0];
    assert_eq!(premise.node, NodeRef::new(PremiseKind::Decision, public));
    let because = premise.because.as_ref().expect("tdpublic is derived");
    assert_eq!(
        because.steps[0].premises[0].node,
        NodeRef::new(PremiseKind::Principle, "open-by-default")
    );
    assert_eq!(
        why.principles
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        vec!["open-by-default"]
    );
    assert!(why.missing_principles.is_empty());

    let text = render_reasoned_why(&why).join("\n");
    assert!(text.contains("  derived from:"), "{text}");
    assert!(
        text.contains("principle open-by-default “Tools are public unless they hold secrets.”"),
        "{text}"
    );
    assert!(
        text.contains("principle open-by-default (project, /repo/.agent-doc/project.md)"),
        "{text}"
    );
}

#[test]
fn a_depends_on_principle_premise_reads_as_the_principle() {
    let graph = build(DOC);
    let principles = principles();
    let (derivations, _) = derivation_graph(&graph, &principles, Vec::new());
    let port = decision(&graph, "tdport");
    let why = reasoned_why(&graph, &derivations, &principles, &port).unwrap();
    assert_eq!(why.why.depends_on.len(), 1, "{why:#?}");
    assert_eq!(why.why.depends_on[0].kind, NodeKind::Principle);
    assert_eq!(
        why.why.depends_on[0].label,
        "Pick the default a new user expects."
    );
    let text = render_reasoned_why(&why).join("\n");
    assert!(
        text.contains("depends on principle least-surprise “Pick the default a new user expects.”"),
        "{text}"
    );
}

#[test]
fn a_decision_with_no_derivation_answers_from_the_ledger_alone() {
    let graph = build(DOC);
    let principles = principles();
    let (derivations, _) = derivation_graph(&graph, &principles, Vec::new());
    let polish = decision(&graph, "tdpolish");
    let why = reasoned_why(&graph, &derivations, &principles, &polish).unwrap();
    assert!(why.derived_from.is_empty());
    assert_eq!(why.derivation_supported, None);
    let value = serde_json::to_value(&why).unwrap();
    assert_eq!(
        value["decision_id"],
        polish.as_str(),
        "flattened: {value:#}"
    );
    assert!(value.get("derived_from").is_none(), "{value:#}");
    assert!(value.get("derivation_supported").is_none(), "{value:#}");
    assert!(value.get("principles").is_none(), "{value:#}");
}

#[test]
fn stored_derivations_resolve_onto_ledger_decisions_and_report_missing_principles() {
    let graph = build(DOC);
    let principles = principles();
    let public = decision(&graph, "tdpublic");
    let stored = vec![
        // Concludes a decision the ledger never saw, through an item id that
        // resolves to the tdpublic decision.
        Derivation {
            id: "drv-downloads".into(),
            premises: vec![
                NodeRef::new(PremiseKind::Decision, "tdpublic"),
                NodeRef::new(PremiseKind::Principle, "retired-principle"),
                NodeRef::new(PremiseKind::Fact, "releases-are-public"),
            ],
            inference: "public tools ship from public releases".into(),
            conclusion: Conclusion {
                decision: "dec-downloads".into(),
                answer: "downloads come from public GitHub releases".into(),
            },
            status: DerivationStatus::AgentDerivedUnreviewed,
        },
        Derivation {
            id: "drv-empty".into(),
            premises: Vec::new(),
            inference: "nothing".into(),
            conclusion: Conclusion {
                decision: "dec-downloads".into(),
                answer: "x".into(),
            },
            status: DerivationStatus::AgentDerivedUnreviewed,
        },
    ];
    let (derivations, skipped) = derivation_graph(&graph, &principles, stored);
    assert_eq!(
        skipped,
        vec!["derivation drv-empty has no premises; skipped".to_string()]
    );
    let why = reasoned_why(&graph, &derivations, &principles, "dec-downloads")
        .expect("a derivation-only decision still answers");
    assert_eq!(why.why.question, "");
    assert_eq!(
        why.why.current_answer.as_deref(),
        Some("downloads come from public GitHub releases")
    );
    let step = &why.derived_from[0];
    assert_eq!(step.status, DerivationStatus::AgentDerivedUnreviewed);
    assert_eq!(
        step.premises[0].node,
        NodeRef::new(PremiseKind::Decision, public)
    );
    assert!(
        step.premises[0].because.is_some(),
        "the resolved premise recurses into tdpublic's operator derivation"
    );
    assert_eq!(
        why.missing_principles,
        vec!["retired-principle".to_string()]
    );
    assert_eq!(
        why.principles
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        vec!["open-by-default"]
    );
    let text = render_reasoned_why(&why).join("\n");
    assert!(
        text.contains("drv-downloads (agent_derived_unreviewed): “public tools ship from public releases” ⇒ “downloads come from public GitHub releases”"),
        "{text}"
    );
    assert!(text.contains("fact releases-are-public"), "{text}");
    assert!(
        text.contains("principle retired-principle (not in scope)"),
        "{text}"
    );
    assert!(
        text.contains("principles no longer in scope: retired-principle"),
        "{text}"
    );

    assert!(reasoned_why(&graph, &derivations, &principles, "dec-unknown").is_none());
}

#[test]
fn a_stored_derivation_wins_over_the_operator_stated_one_with_its_id() {
    let graph = build(DOC);
    let principles = principles();
    let operator = operator_derivations(&graph, &principles);
    let port = decision(&graph, "tdport");
    let mut stored = derivation_for(&operator, &port).unwrap().clone();
    stored.status = DerivationStatus::Rejected;
    let (derivations, skipped) = derivation_graph(&graph, &principles, vec![stored.clone()]);
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(
        derivations.get(&stored.id).unwrap().status,
        DerivationStatus::Rejected
    );
    let why = reasoned_why(&graph, &derivations, &principles, &port).unwrap();
    assert_eq!(why.derivation_supported, Some(false));
    let text = render_reasoned_why(&why).join("\n");
    assert!(text.contains("every step rejected"), "{text}");
}
