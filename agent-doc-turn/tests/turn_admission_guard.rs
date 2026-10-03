//! `#admissionsteeringagree` architecture guard (GH #118): two commands, one
//! state, one predicate.
//!
//! Preflight admission and `session-check` both decide "may this turn continue?"
//! for a document whose last cycle is closed. On agent-doc 0.35.442 they told the
//! agent opposite things about the same document in the same minute: preflight
//! refused admission over snapshot/HEAD drift ("do NOT run preflight ... then
//! stop") while `session-check` said a fresh operator prompt was pending and to
//! run `agent-doc <FILE>`. The recovery the refusal named,
//! `reset --from-current`, would have folded that unanswered prompt into the
//! baseline. Same shape as `#percellconverge` / `#strandedremedydeadlock`: the
//! sites agreed on wording, not on the predicate.
//!
//! The rule: the verdict is `agent_doc_turn::turn_admission::TurnAdmission`; its
//! I/O shell is `agent_doc_session_check_io::turn_admission`; the steering
//! observation is `closed_cycle_steering_between`; the steering-preserving
//! recovery text is authored once, in `turn_admission.rs`. This guard checks that
//! every site still reaches them — matched on non-comment, non-test lines, so a
//! doc comment that merely mentions the rule cannot satisfy it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Sites that decide admission or name a recovery, and the call each must make.
const ADMISSION_SITES: [(&str, &str); 4] = [
    // Step 0d / step 1b2 drift gates.
    (
        "agent-doc-preflight-command-io/src/run.rs",
        "turn_admission(",
    ),
    // `detect_uncommitted_closeout_drift` (preflight's refusal) and the
    // committed-cycle `session-check` verdict.
    (
        "agent-doc-session-check-io/src/command.rs",
        "turn_admission(",
    ),
    // `closeout_recovery_hint` — the remedy both surfaces print.
    (
        "agent-doc-closeout-runtime-io/src/lib.rs",
        "turn_admission(",
    ),
    // `reset --from-current`, the recovery GH #118 refused to run.
    ("src/reset.rs", "turn_admission("),
];

/// Recovery executors and the session-check shell must read the one pure
/// steering observation instead of re-assembling "HEAD, then baseline".
const STEERING_OBSERVERS: [(&str, &str); 3] = [
    (
        "agent-doc-session-check-io/src/prompt_bearing.rs",
        "closed_cycle_steering_between(",
    ),
    (
        "agent-doc-commit-io/src/lib.rs",
        "closed_cycle_steering_between(",
    ),
    (
        "agent-doc-flow-io/src/closeout.rs",
        "closed_cycle_steering_between(",
    ),
];

/// A site that calls the raw steering detector to decide admission has
/// re-acquired its own predicate.
const SECOND_PREDICATE: (&str, &str) = (
    "agent-doc-preflight-command-io/src/run.rs",
    "detect_unstarted_prompt_bearing_diff(",
);

/// The steering-preserving recovery token, authored only by its owner.
const RECOVERY_TOKEN: &str = "\"pending_operator_steering\"";
const RECOVERY_OWNER: &str = "agent-doc-turn/src/turn_admission.rs";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate lives in the workspace root")
        .to_path_buf()
}

/// Line numbers (1-based) inside a `#[cfg(test)]` item, tracked by brace depth.
fn cfg_test_lines(source: &str) -> BTreeSet<usize> {
    let mut inside = BTreeSet::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0usize;
    while index < lines.len() {
        if !lines[index].trim_start().starts_with("#[cfg(test)]") {
            index += 1;
            continue;
        }
        let mut depth = 0i32;
        let mut opened = false;
        let mut cursor = index;
        while cursor < lines.len() {
            for ch in lines[cursor].chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            inside.insert(cursor + 1);
            if opened && depth <= 0 {
                break;
            }
            // A `#[cfg(test)]` on a braceless item (`use`, fn decl) ends at `;`.
            if !opened && lines[cursor].trim_end().ends_with(';') {
                break;
            }
            cursor += 1;
        }
        index = cursor + 1;
    }
    inside
}

/// Production (non-comment, non-`#[cfg(test)]`) lines of a workspace file.
fn production_lines(relative: &str) -> Vec<(usize, String)> {
    let path = workspace_root().join(relative);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let test_lines = cfg_test_lines(&source);
    source
        .lines()
        .enumerate()
        .filter(|(index, _)| !test_lines.contains(&(index + 1)))
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .map(|(index, line)| (index + 1, line.to_string()))
        .collect()
}

fn calls(relative: &str, needle: &str) -> bool {
    production_lines(relative)
        .iter()
        .any(|(_, line)| line.contains(needle))
}

#[test]
fn every_admission_site_reaches_the_shared_predicate() {
    for (site, call) in ADMISSION_SITES {
        assert!(
            calls(site, call),
            "{site} no longer calls `{call}`: it decides turn admission (or names a recovery) \
             without the shared predicate, so it can disagree with session-check again (GH #118)"
        );
    }
}

#[test]
fn every_steering_observer_reads_the_one_pure_comparison() {
    for (site, call) in STEERING_OBSERVERS {
        assert!(
            calls(site, call),
            "{site} no longer calls `{call}`: it re-assembles the closed-cycle steering \
             observation on its own"
        );
    }
}

#[test]
fn preflight_does_not_decide_admission_from_the_raw_steering_detector() {
    let (site, call) = SECOND_PREDICATE;
    let hits: Vec<_> = production_lines(site)
        .into_iter()
        .filter(|(_, line)| line.contains(call))
        .collect();
    assert!(
        hits.is_empty(),
        "{site} calls `{call}` directly — a second admission predicate beside \
         `turn_admission`: {hits:?}"
    );
}

#[test]
fn the_steering_preserving_recovery_token_has_one_author() {
    let root = workspace_root();
    let mut stack = vec![root.clone()];
    let mut authors = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if matches!(
                    name.as_str(),
                    "target" | ".git" | "editors" | "node_modules" | "tests" | "benches" | ".tsift"
                ) {
                    continue;
                }
                stack.push(path);
            } else if name.ends_with(".rs") {
                let relative = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if production_lines(&relative)
                    .iter()
                    .any(|(_, line)| line.contains(RECOVERY_TOKEN))
                {
                    authors.push(relative);
                }
            }
        }
    }
    assert_eq!(
        authors,
        vec![RECOVERY_OWNER.to_string()],
        "the steering-preserving recovery must be rendered by `steering_preserving_recovery`, \
         never re-authored"
    );
}
