//! `#coinedguardledgerasymmetry` — both coined-id guards read ONE predicate.
//!
//! The two guards drifted silently: the `PreToolUse` guard ran a whole-text tag
//! scan over the done archive while `session-check` read only entry ids, so the
//! same tag, document and archive answered "tracked" on one path and "invented"
//! on the other. Nothing failed — the guards simply disagreed, and a
//! verification probe passed for a reason its author did not intend.
//!
//! `#percellconverge` records the same lesson from the retained-write sites:
//! consolidating the *wording* did not stop the *predicate* diverging, so the
//! predicate got one home and a test that every site calls it. This is that
//! test for the coined-id ledger. It cannot tell a correct reading from an
//! incorrect one, so it enforces the thing that is checkable: there is exactly
//! one reading, and both guards use it.

use std::path::Path;

/// The two sites that answer "does this document's ledger vouch for this id?".
const GUARD_SITES: &[(&str, &str)] = &[
    (
        "PreToolUse",
        "../agent-doc-hooks-io/src/coined_id_pretooluse.rs",
    ),
    (
        "session-check",
        "../agent-doc-session-check-io/src/pending_guards.rs",
    ),
];

/// Archive-resolution calls a site must NOT make for itself. Each is a real
/// previous implementation, not a hypothetical.
///
/// Scoped to ARCHIVE resolution on purpose. Both guards still call
/// `coined_ids::extract_tags` to read the live document's own components, which
/// is correct and was never the drift — banning it outright would be a guard
/// that fails on correct code, which is worse than no guard.
const RETIRED_ARCHIVE_CALLS: &[(&str, &str)] = &[
    (
        "external_done_archive_ids(",
        "reads declared archives only, missing the sibling walk the other guard used",
    ),
    (
        "done_archive_candidates(",
        "resolves archives itself, which is how the two readings diverged",
    ),
];

/// Production code only. A test may legitimately name a retired call.
fn production_source(relative: &str) -> String {
    let source = read_site(relative);
    match source.find("#[cfg(test)]") {
        Some(cut) => source[..cut].to_string(),
        None => source,
    }
}

fn read_site(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("guard site {} must be readable: {err}", path.display()))
}

#[test]
fn every_coined_id_guard_calls_the_shared_archive_predicate() {
    for (name, relative) in GUARD_SITES {
        let source = read_site(relative);
        assert!(
            source.contains("archived_tracked_ids"),
            "the {name} coined-id guard must resolve archived ids through \
             done_archive::archived_tracked_ids, not its own reading"
        );
    }
}

#[test]
fn no_coined_id_guard_resolves_the_archive_itself() {
    for (name, relative) in GUARD_SITES {
        let source = production_source(relative);
        for (retired, why) in RETIRED_ARCHIVE_CALLS {
            // A comment may name a retired call to explain the history; only a
            // real call re-creates the drift.
            let calls = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .filter(|line| line.contains(retired))
                .count();
            assert_eq!(
                calls, 0,
                "the {name} coined-id guard calls `{retired}` again: {why}"
            );
        }
    }
}

/// The guard above only bites if these markers still exist. If a rename made
/// them unfindable it would pass vacuously, which is the failure mode a
/// source-scanning test is most prone to.
#[test]
fn the_retired_calls_still_exist_to_be_found() {
    let predicate = read_site("src/done_archive.rs");
    for (retired, _) in RETIRED_ARCHIVE_CALLS {
        assert!(
            predicate.contains(retired.trim_end_matches('(')),
            "`{retired}` no longer exists, so the drift guard would pass vacuously \
             — update RETIRED_ARCHIVE_CALLS to the current names"
        );
    }
}
