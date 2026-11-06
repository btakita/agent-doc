use super::*;

fn layer(scope: PrincipleScope, source: &str, rules: &[&str]) -> Layer {
    Layer {
        scope,
        source: source.to_string(),
        rules: rules.iter().map(|rule| rule.to_string()).collect(),
    }
}

fn parsed(rule: &str) -> ParsedPrinciple {
    match parse_rule(rule) {
        RuleForm::Principle(parsed) => parsed,
        other => panic!("expected a principle from {rule:?}, got {other:?}"),
    }
}

#[test]
fn a_principle_item_carries_id_statement_rationale_and_scope() {
    let principle = parsed(concat!(
        "principle [#editor-authority]: Editor-visible operator text is authoritative.\n",
        "  rationale: The operator reads and edits the editor buffer;\n",
        "  disk can lag behind it.\n",
        "  scope: project",
    ));
    assert_eq!(
        principle,
        ParsedPrinciple {
            id: "editor-authority".into(),
            statement: "Editor-visible operator text is authoritative.".into(),
            rationale: Some(
                "The operator reads and edits the editor buffer;\ndisk can lag behind it.".into()
            ),
            declared_scope: Some("project".into()),
        }
    );
}

#[test]
fn keyword_and_field_names_are_case_insensitive_and_fields_may_be_sub_items() {
    let principle = parsed(
        "Principle #no-reask: Never re-ask an answered question.\n  - Rationale: It wastes operator attention.\n  - SCOPE: global",
    );
    assert_eq!(principle.id, "no-reask");
    assert_eq!(principle.statement, "Never re-ask an answered question.");
    assert_eq!(
        principle.rationale.as_deref(),
        Some("It wastes operator attention.")
    );
    assert_eq!(principle.declared_scope.as_deref(), Some("global"));
}

#[test]
fn continuation_lines_before_any_field_extend_the_statement() {
    let principle =
        parsed("principle [#velocity]:\n  When a release is blocked,\n  velocity over polish.");
    assert_eq!(
        principle.statement,
        "When a release is blocked,\nvelocity over polish."
    );
    assert_eq!(principle.rationale, None);
}

#[test]
fn ordinary_rules_including_prose_about_principles_stay_plain() {
    for rule in [
        "Run the focused test first.",
        "Principle of least surprise applies to CLI flags.",
        "Principles matter.",
        "principled design",
        "[#principle] is just an id",
        "principled [id]: a different word",
        "In principle [id]: not at the start",
        "",
    ] {
        assert_eq!(parse_rule(rule), RuleForm::Plain, "{rule:?}");
    }
}

#[test]
fn principle_shaped_rules_without_a_valid_id_or_statement_are_malformed() {
    for (rule, needle) in [
        ("principle: no id here", "needs an id"),
        ("principle [#]: empty id", "needs an id"),
        ("principle [velocity]: no hash", "starts with `#`"),
        ("PRINCIPLE [velocity]: no hash", "starts with `#`"),
        ("principle[velocity]: no space", "starts with `#`"),
        ("principle [#open: unclosed", "closing `]`"),
        ("principle [#ok] missing colon", "expected `:`"),
        ("principle #nocolon", "expected `:`"),
        ("principle [#has space]: x", "may use only"),
        ("principle [#-lead]: x", "may use only"),
        ("principle [#empty]:", "has no statement"),
        (
            "principle [#empty]:\n  rationale: only a rationale",
            "has no statement",
        ),
    ] {
        match parse_rule(rule) {
            RuleForm::Malformed { code, reason } => {
                assert_eq!(code, ISSUE_MALFORMED);
                assert!(reason.contains(needle), "{rule:?}: {reason}");
            }
            other => panic!("{rule:?} should be malformed, got {other:?}"),
        }
    }
}

#[test]
fn principles_leave_agent_rules_and_plain_rules_compose_in_order() {
    let composition = compose(&[
        layer(
            PrincipleScope::Global,
            "/home/op/.config/agent-doc/principles.md",
            &["principle [#g]: Global value."],
        ),
        layer(
            PrincipleScope::Project,
            "/w/.agent-doc/project.md",
            &[
                "Ancestor rule.",
                "principle [#p]: Project value.\n  rationale: Because.",
            ],
        ),
        layer(
            PrincipleScope::Document,
            "/w/app/doc.md",
            &["Session rule."],
        ),
    ]);
    assert_eq!(composition.rules, vec!["Ancestor rule.", "Session rule."]);
    assert_eq!(
        composition.principles,
        vec![
            Principle {
                id: "g".into(),
                statement: "Global value.".into(),
                rationale: None,
                scope: PrincipleScope::Global,
                source: "/home/op/.config/agent-doc/principles.md".into(),
                overrides: vec![],
            },
            Principle {
                id: "p".into(),
                statement: "Project value.".into(),
                rationale: Some("Because.".into()),
                scope: PrincipleScope::Project,
                source: "/w/.agent-doc/project.md".into(),
                overrides: vec![],
            },
        ]
    );
    assert!(composition.issues.is_empty(), "{:?}", composition.issues);
}

#[test]
fn a_nearer_principle_replaces_a_farther_one_in_place() {
    let composition = compose(&[
        layer(
            PrincipleScope::Global,
            "global.md",
            &["principle [#a]: Global A.", "principle [#b]: Global B."],
        ),
        layer(
            PrincipleScope::Project,
            "ancestor/project.md",
            &["principle [#a]: Ancestor A."],
        ),
        layer(
            PrincipleScope::Project,
            "nearest/project.md",
            &["principle [#c]: Nearest C."],
        ),
        layer(
            PrincipleScope::Document,
            "doc.md",
            &["principle [#a]: Document A."],
        ),
    ]);
    let summary: Vec<(&str, &str, PrincipleScope, Vec<&str>)> = composition
        .principles
        .iter()
        .map(|p| {
            (
                p.id.as_str(),
                p.statement.as_str(),
                p.scope,
                p.overrides.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                "a",
                "Document A.",
                PrincipleScope::Document,
                vec!["global.md", "ancestor/project.md"]
            ),
            ("b", "Global B.", PrincipleScope::Global, vec![]),
            ("c", "Nearest C.", PrincipleScope::Project, vec![]),
        ]
    );
}

#[test]
fn a_duplicate_id_in_one_file_keeps_the_first_and_reports_the_second() {
    let composition = compose(&[layer(
        PrincipleScope::Project,
        "project.md",
        &["principle [#a]: First.", "principle [#a]: Second."],
    )]);
    assert_eq!(composition.principles.len(), 1);
    assert_eq!(composition.principles[0].statement, "First.");
    assert_eq!(composition.issues.len(), 1);
    assert_eq!(composition.issues[0].code, ISSUE_DUPLICATE_ID);
    assert_eq!(composition.issues[0].source, "project.md");
}

#[test]
fn the_source_decides_scope_and_a_disagreeing_declaration_says_where_to_move_it() {
    let composition = compose(&[
        layer(
            PrincipleScope::Project,
            "project.md",
            &["principle [#a]: A.\n  scope: global"],
        ),
        layer(
            PrincipleScope::Document,
            "doc.md",
            &[
                "principle [#b]: B.\n  scope: project",
                "principle [#c]: C.\n  scope: session",
                "principle [#d]: D.\n  scope: everywhere",
            ],
        ),
    ]);
    let scopes: Vec<(&str, PrincipleScope)> = composition
        .principles
        .iter()
        .map(|p| (p.id.as_str(), p.scope))
        .collect();
    assert_eq!(
        scopes,
        vec![
            ("a", PrincipleScope::Project),
            ("b", PrincipleScope::Document),
            ("c", PrincipleScope::Document),
            ("d", PrincipleScope::Document),
        ]
    );
    let issues: Vec<(&str, &str)> = composition
        .issues
        .iter()
        .map(|i| (i.code, i.source.as_str()))
        .collect();
    assert_eq!(
        issues,
        vec![
            (ISSUE_SCOPE_MISMATCH, "project.md"),
            (ISSUE_SCOPE_MISMATCH, "doc.md"),
            (ISSUE_UNKNOWN_SCOPE, "doc.md"),
        ]
    );
    assert!(
        composition.issues[0].message.contains("principles.md"),
        "{}",
        composition.issues[0].message
    );
    assert!(
        composition.issues[1]
            .message
            .contains(".agent-doc/project.md"),
        "{}",
        composition.issues[1].message
    );
}

#[test]
fn a_malformed_principle_stays_an_ordinary_rule_with_an_issue() {
    let composition = compose(&[layer(
        PrincipleScope::Document,
        "doc.md",
        &["principle: forgot the id"],
    )]);
    assert_eq!(composition.rules, vec!["principle: forgot the id"]);
    assert!(composition.principles.is_empty());
    assert_eq!(composition.issues[0].code, ISSUE_MALFORMED);
    assert!(
        composition.issues[0]
            .message
            .contains("kept as an ordinary rule")
    );
}

/// `#principlemissinghashwarning`: `principle [id]: x` reads as a principle to
/// the operator, so it is reported rather than left a plain rule. It is kept
/// where rules are kept, and in the global file it is not also told to become
/// a principle, which is what it already tried to be.
#[test]
fn a_principle_id_missing_its_hash_is_reported_not_silently_plain() {
    let composition = compose(&[
        layer(
            PrincipleScope::Global,
            "principles.md",
            &["principle [velocity]: Velocity over polish."],
        ),
        layer(
            PrincipleScope::Document,
            "doc.md",
            &["principle [velocity]: Velocity over polish."],
        ),
    ]);
    assert!(composition.principles.is_empty());
    assert_eq!(
        composition.rules,
        vec!["principle [velocity]: Velocity over polish."]
    );
    let codes: Vec<(&str, &str)> = composition
        .issues
        .iter()
        .map(|issue| (issue.code, issue.source.as_str()))
        .collect();
    assert_eq!(
        codes,
        vec![
            (ISSUE_MALFORMED, "principles.md"),
            (ISSUE_MALFORMED, "doc.md"),
        ]
    );
    let global = &composition.issues[0].message;
    assert!(global.contains("starts with `#`"), "{global}");
    assert!(global.contains("ignored"), "{global}");
    assert!(!global.contains("Write it as `principle"), "{global}");
    let kept = &composition.issues[1].message;
    assert!(kept.contains("starts with `#`"), "{kept}");
    assert!(kept.contains("kept as an ordinary rule"), "{kept}");
}

#[test]
fn the_global_layer_holds_principles_only() {
    let composition = compose(&[layer(
        PrincipleScope::Global,
        "principles.md",
        &[
            "Plain global rule.",
            "principle: no id",
            "principle [#ok]: Fine.",
        ],
    )]);
    assert!(composition.rules.is_empty(), "{:?}", composition.rules);
    assert_eq!(composition.principles.len(), 1);
    let codes: Vec<&str> = composition.issues.iter().map(|i| i.code).collect();
    assert_eq!(codes, vec![ISSUE_GLOBAL_PLAIN_RULE, ISSUE_MALFORMED]);
    assert!(composition.issues[1].message.contains("ignored"));
}

#[test]
fn principles_serialize_with_snake_case_scope_and_omit_empty_fields() {
    let composition = compose(&[layer(
        PrincipleScope::Project,
        "p.md",
        &["principle [#a]: A."],
    )]);
    let value = serde_json::to_value(&composition.principles).unwrap();
    assert_eq!(
        value,
        serde_json::json!([{ "id": "a", "statement": "A.", "scope": "project", "source": "p.md" }])
    );
    assert_eq!(
        principles_guidance(&composition.principles).as_deref(),
        Some(PRINCIPLES_GUIDANCE)
    );
    assert_eq!(principles_guidance(&[]), None);
}

#[test]
fn rules_parsed_from_an_agent_rules_body_feed_composition() {
    let body = concat!(
        "- principle [#editor-authority]: Editor text wins.\n",
        "  rationale: The operator sees it.\n",
        "- Run the focused test first.\n",
    );
    let rules = agent_doc_element::rules::rules_from_body(body);
    let composition = compose(&[Layer {
        scope: PrincipleScope::Document,
        source: "doc.md".into(),
        rules,
    }]);
    assert_eq!(composition.rules, vec!["Run the focused test first."]);
    assert_eq!(composition.principles[0].id, "editor-authority");
    assert_eq!(
        composition.principles[0].rationale.as_deref(),
        Some("The operator sees it.")
    );
}

/// `#firstprinciplesfactlist`: `compose` lists the facts of the same layers
/// (nearest wins, overrides recorded) after the principle issues, reports a
/// duplicate fact id in one file and a fact written in the global file, and
/// keeps fact items in `rules` as written.
#[test]
fn compose_lists_facts_with_overrides_and_reports_their_problems() {
    let composition = compose(&[
        layer(
            PrincipleScope::Global,
            "principles.md",
            &["fact [#g]: Facts are not read from here."],
        ),
        layer(
            PrincipleScope::Project,
            "project.md",
            &["fact [#jws]: Credentials are JWS.", "principle [#p]: P."],
        ),
        layer(
            PrincipleScope::Document,
            "doc.md",
            &[
                "fact [#jws]: Credentials are JWS with a kid header.",
                "fact [#jws]: declared twice",
                "principle [#p]: P again.",
                "principle [#p]: P a third time.",
            ],
        ),
    ]);
    assert_eq!(composition.facts.len(), 1);
    let fact = &composition.facts[0];
    assert_eq!(
        (
            fact.id.as_str(),
            fact.statement.as_str(),
            fact.scope,
            fact.source.as_str()
        ),
        (
            "jws",
            "Credentials are JWS with a kid header.",
            PrincipleScope::Document,
            "doc.md"
        )
    );
    assert_eq!(fact.overrides, vec!["project.md".to_string()]);
    assert_eq!(
        composition.rules,
        vec![
            "fact [#jws]: Credentials are JWS.",
            "fact [#jws]: Credentials are JWS with a kid header.",
            "fact [#jws]: declared twice",
        ]
    );
    let codes: Vec<(&str, &str)> = composition
        .issues
        .iter()
        .map(|issue| (issue.code, issue.source.as_str()))
        .collect();
    assert_eq!(
        codes,
        vec![
            (ISSUE_FACT_IN_GLOBAL_FILE, "principles.md"),
            (ISSUE_DUPLICATE_ID, "doc.md"),
            (ISSUE_FACT_DUPLICATE_ID, "doc.md"),
        ]
    );
    assert!(composition.issues[0].message.contains("ignored fact [#g]"));
    assert_eq!(
        serde_json::to_value(fact).unwrap(),
        serde_json::json!({
            "id": "jws",
            "statement": "Credentials are JWS with a kid header.",
            "scope": "document",
            "source": "doc.md",
            "overrides": ["project.md"],
        })
    );
}

/// `#factmalformedwarning`: a fact-shaped item with a bad id or no statement
/// is a `fact_malformed` issue, not a premise. In a project or document it
/// stays an ordinary rule. In the global file it is ignored, and it is not
/// also told to become a principle.
#[test]
fn compose_reports_a_malformed_fact_and_keeps_it_out_of_the_premises() {
    let composition = compose(&[
        layer(
            PrincipleScope::Global,
            "principles.md",
            &["fact: no id", "Facts are not principles."],
        ),
        layer(
            PrincipleScope::Project,
            "project.md",
            &["fact [#bad id]: x"],
        ),
        layer(
            PrincipleScope::Document,
            "doc.md",
            &["fact [#empty]:", "fact [#ok]: fine", "Fact checking first."],
        ),
    ]);
    assert_eq!(
        composition
            .facts
            .iter()
            .map(|fact| fact.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ok"]
    );
    assert_eq!(
        composition.rules,
        vec![
            "fact [#bad id]: x",
            "fact [#empty]:",
            "fact [#ok]: fine",
            "Fact checking first.",
        ]
    );
    let codes: Vec<(&str, &str)> = composition
        .issues
        .iter()
        .map(|issue| (issue.code, issue.source.as_str()))
        .collect();
    assert_eq!(
        codes,
        vec![
            (ISSUE_FACT_MALFORMED, "principles.md"),
            (ISSUE_GLOBAL_PLAIN_RULE, "principles.md"),
            (ISSUE_FACT_MALFORMED, "project.md"),
            (ISSUE_FACT_MALFORMED, "doc.md"),
        ]
    );
    let global = &composition.issues[0].message;
    assert!(global.contains("needs an id"), "{global}");
    assert!(global.contains("ignored \"fact: no id\""), "{global}");
    assert!(!global.contains("Write it as `principle"), "{global}");
    let kept = &composition.issues[2].message;
    assert!(kept.contains("may use only"), "{kept}");
    assert!(kept.contains("kept as an ordinary rule"), "{kept}");
}
