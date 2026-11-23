use super::*;

/// The anonymized live session the digest was designed from: operator
/// decisions and actions, agent follow-ups, blocked rows, a prompt blocker,
/// a Waiting-on-you item, struck rows, and three running workers.
const SESSION: &str = include_str!("../tests/fixtures/loose_ends_session.md");

/// `#looseends`: the three operator cleanup rows asking to tidy agent-doc's
/// own artifacts (`q-8eeadf7937c8`, `q-dade5157fe53`, `q-e107b04c1cd5`) are
/// self-repaired, so they hold no work for the tree's 🗑️/🔒 indicator and
/// are not loose ends either: the digest, the top-level counts and the keep
/// indicator read one classifier.
const SESSION_DIGEST: &str = concat!(
    "- **Decide** (4)\n",
    "  - `[#q-0e29d5fdf1c4]` Decide whether to retain separate 262 and 263 modular ZIPs or authorize the combined `262` through…\n",
    "  - `[#q-cb3ac1276fd3]` decide whether to refresh the VS Code package\n",
    "  - `[#q-5d1c0f2e9a77]` decide whether to revoke the PyPI API token named **`publish 2026-11-13`**\n",
    "  - `[#ghrecvpolicypark]` Decide whether backlog #ghrecvpolicy stays open in the backlog or moves to the icebox\n",
    "- **Do** (1)\n",
    "  - `[#q-a1b2c3d4e5f6]` unlock the `login` keyring once by running `gnome-keyring-unlock-login` in a terminal\n",
    "- **Blocked** (3)\n",
    "  - `[#jbfrontendswap]` blocked by `[#prompt-blocker-b0ddfbae2f94]`: Use chromium-bridge to perform the steps\n",
    "  - `[#ghrecvpolicy]` blocked: GH #40 recipient-side auto_create_components policy; blocked until multi-user workspace access…\n",
    "  - `[#prompt-blocker-b0ddfbae2f94]` blocked: Use chromium-bridge to perform the steps\n",
    "- **Running** (3)\n",
    "  - `[#subagent:recapids#1]` The recap says `- 4 decisions for you`, `- 4 cleanup rows`, `- 3 blocked`\n",
    "  - `[#subagent:ci53#1]` check the agent-doc CI run for `53fa578ad6` on `main`\n",
    "  - `[#subagent:sampledevhook#1]` In a subagent: fix agent-doc issues in sample-dev.md: Stop hook timed out after 5.0s waiting for…\n",
);

fn digest_body(document: &str) -> String {
    let components = element::parse(document).unwrap();
    let component = components
        .iter()
        .find(|component| component.name == LOOSE_ENDS_COMPONENT)
        .expect("agent:loose-ends present");
    component.content(document).to_string()
}

#[test]
fn session_digest_groups_every_open_row_by_id() {
    let synced = reconcile_loose_ends(SESSION).unwrap().expect("inserted");
    assert_eq!(digest_body(&synced), SESSION_DIGEST);
    // Inserted near the top: directly above `agent:active`.
    assert!(
        synced.contains(&format!(
            "<!-- /agent:status -->\n\n## Loose ends <!-- agent:loose-ends -->\n{SESSION_DIGEST}<!-- /agent:loose-ends -->\n\n## Active <!-- agent:active priority -->\n"
        )),
        "{synced}"
    );
}

#[test]
fn agent_work_and_closed_rows_never_reach_decide_or_do() {
    let ends = loose_ends(SESSION).unwrap();
    let operator = ends
        .iter()
        .filter(|end| matches!(end.group, LooseEndGroup::Decide | LooseEndGroup::Do))
        .collect::<Vec<_>>();
    // A `🙋` row whose own owner label is `agent:` is agent work.
    assert!(
        !ends.iter().any(|end| end.id == "q-0f9e8d7c6b5a"),
        "{ends:#?}"
    );
    // The agent action item inside a response tree, the plain operator
    // prompt, and struck rows are not loose ends.
    for end in &operator {
        assert!(!end.text.contains("find and fix the exact step"), "{end:?}");
        assert!(!end.text.contains("duplicated my prompt"), "{end:?}");
        assert!(!end.text.contains("stray runbooks controller"), "{end:?}");
    }
    // Running rows are named by their attempt owner label.
    let running = ends
        .iter()
        .filter(|end| end.group == LooseEndGroup::Running)
        .map(|end| end.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        running,
        [
            "subagent:recapids#1",
            "subagent:ci53#1",
            "subagent:sampledevhook#1"
        ]
    );
}

#[test]
fn a_second_sync_is_a_no_op() {
    let synced = reconcile_loose_ends(SESSION).unwrap().unwrap();
    assert_eq!(reconcile_loose_ends(&synced).unwrap(), None);
}

#[test]
fn a_stale_digest_is_re_rendered_and_nothing_open_when_all_resolved() {
    let synced = reconcile_loose_ends(SESSION).unwrap().unwrap();
    // Strike the one keyring action: it leaves Do on the next sync.
    let struck = synced.replace(
        "- ### 🙋 follow up on an action item from queue item “[from:chat key=keyring] fix the gh keyring hang”: operator: unlock",
        "- ### ~~follow up on an action item from queue item “[from:chat key=keyring] fix the gh keyring hang”: operator: unlock",
    );
    let struck = struck.replace(
        "in a terminal. [operator-verify] [#q-a1b2c3d4e5f6]\n",
        "in a terminal. [operator-verify] [#q-a1b2c3d4e5f6]~~\n",
    );
    let resynced = reconcile_loose_ends(&struck).unwrap().unwrap();
    let body = digest_body(&resynced);
    assert!(!body.contains("- **Do**"), "{body}");
    assert!(!body.contains("q-a1b2c3d4e5f6"), "{body}");

    let empty = concat!(
        "## Loose ends <!-- agent:loose-ends -->\n",
        "- **Do** (1)\n",
        "  - `[#q-a1b2c3d4e5f6]` unlock the keyring\n",
        "<!-- /agent:loose-ends -->\n\n",
        "## Active <!-- agent:active -->\n<!-- /agent:active -->\n\n",
        "## Run <!-- agent:run id=\"main\" -->\n",
        "- ~~unlock the keyring [operator-verify] [#q-a1b2c3d4e5f6]~~\n",
        "<!-- /agent:run -->\n",
    );
    let resolved = reconcile_loose_ends(empty).unwrap().unwrap();
    assert_eq!(digest_body(&resolved), "Nothing open\n");
    assert_eq!(reconcile_loose_ends(&resolved).unwrap(), None);
}

#[test]
fn missing_digest_is_inserted_only_in_a_managed_session_with_open_rows() {
    let no_active = concat!(
        "## Run <!-- agent:run id=\"main\" -->\n",
        "- ⛔ ⟦blocked by gate⟧ do [#work]\n",
        "<!-- /agent:run -->\n",
    );
    assert_eq!(reconcile_loose_ends(no_active).unwrap(), None);
    let nothing_open = concat!(
        "## Active <!-- agent:active -->\n<!-- /agent:active -->\n\n",
        "## Run <!-- agent:run id=\"main\" -->\n",
        "- ~~done~~\n",
        "- plain queued work\n",
        "<!-- /agent:run -->\n",
    );
    assert_eq!(reconcile_loose_ends(nothing_open).unwrap(), None);
}

#[test]
fn operator_lines_typed_under_the_digest_are_kept() {
    let synced = reconcile_loose_ends(SESSION).unwrap().unwrap();
    let noted = synced.replace(
        "  - `[#q-cb3ac1276fd3]` decide whether to refresh the VS Code package\n",
        "  - `[#q-cb3ac1276fd3]` decide whether to refresh the VS Code package\n    - later, after the release\n",
    );
    let noted = noted.replace(
        "<!-- /agent:loose-ends -->",
        "my own note\n<!-- /agent:loose-ends -->",
    );
    assert_eq!(
        reconcile_loose_ends(&noted).unwrap(),
        None,
        "already current"
    );
    // Re-rendering after a queue change keeps both lines.
    let changed = noted.replace(
        "[operator-verify] [#q-a1b2c3d4e5f6]",
        "[operator-verify] [#q-a1b2c3d4e5f6] now",
    );
    let resynced = reconcile_loose_ends(&changed).unwrap().unwrap_or(changed);
    let body = digest_body(&resynced);
    assert!(
        body.contains("refresh the VS Code package\n    - later, after the release\n"),
        "{body}"
    );
    assert!(body.ends_with("my own note\n"), "{body}");
}

/// `#followupshorttitle`: a short-form follow-up row lists by its own
/// `[#q-…]` with its action as the imperative. The `[#origin]` it follows up
/// never names it, and neither does the origin's text.
#[test]
fn a_short_form_followup_lists_by_its_own_id_and_action() {
    let document = concat!(
        "## Loose ends <!-- agent:loose-ends -->\n",
        "<!-- /agent:loose-ends -->\n\n",
        "## Active <!-- agent:active -->\n<!-- /agent:active -->\n\n",
        "## Run <!-- agent:run id=\"main\" -->\n",
        "- ### ~~do [#checkadragainst]~~\n",
        "- ### 🙋 [#checkadragainst] follow-up: operator: decide whether to keep the ADR check [operator-verify] [#q-0123456789ab]\n",
        "- ### 🙋 [#checkadragainst] follow-up: operator: approve PR 9 [operator-verify]\n",
        "<!-- /agent:run -->\n",
    );
    let synced = reconcile_loose_ends(document).unwrap().unwrap();
    let body = digest_body(&synced);
    assert!(
        body.contains("  - `[#q-0123456789ab]` decide whether to keep the ADR check\n"),
        "{body}"
    );
    assert!(!body.contains("`[#checkadragainst]`"), "{body}");
    assert!(!body.contains("follow-up:"), "{body}");

    let tree = crate::queue_tree::queue_tree(&synced, &Default::default()).unwrap();
    for node in tree
        .nodes
        .iter()
        .filter(|node| node.full_text.contains("follow-up:"))
    {
        assert_ne!(node.id.as_deref(), Some("checkadragainst"), "{node:?}");
        let label = crate::queue_tree::RowRef::of(node).label();
        assert!(!label.contains("checkadragainst"), "{label}");
        assert!(label.starts_with("`operator: "), "{label}");
    }
}
