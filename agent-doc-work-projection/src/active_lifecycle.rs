//! `#managedlistnodrift`: `agent:active` rendered from durable lifecycle
//! records, never from the queue markers it would otherwise read back.
//!
//! The durable state is the claim ledger, the reported-release receipts, and
//! the lineage ledger. An active entry exists exactly when a live, executable
//! queue row (operator input: the row and its text) joins a durable attempt by
//! work identity. The visible `☑️`/`🚧` prefix on that row is never consulted,
//! so a stale marker cannot keep an entry alive, and a response-packet row
//! under a Waiting-on-you item (decision evidence, never queue work) is never
//! integration work even when a worker was once dispatched onto it.
//!
//! Entries split into **Running** (claimed or running), **Lost** (lost before
//! a report; needs recovery), and **Needs integration** (the worker reported;
//! the coordinator still has to integrate, verify, and close the row). Each needs-integration entry shows
//! its age, from the receipt's release time, else the attempt's latest lineage
//! event, else its worker start, else its claim.
//!
//! The loader (state.db IO) is installed by the binary
//! ([`install_loader`]); this module stays pure.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use agent_doc_document::queue_projection::QueueLifecycleMarker;
use agent_doc_element_queue::QueueItemIdentity;
use anyhow::Result;

/// The phase an active entry is grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActiveGroup {
    Running,
    /// Lost before a report: needs recovery, not integration.
    Lost,
    NeedsIntegration,
}

impl ActiveGroup {
    pub fn title(self) -> &'static str {
        match self {
            ActiveGroup::Running => "Running",
            ActiveGroup::Lost => "Lost",
            ActiveGroup::NeedsIntegration => "Needs integration",
        }
    }

    /// Stable group key used by the renderer.
    pub fn key(self) -> &'static str {
        match self {
            ActiveGroup::Running => "running",
            ActiveGroup::Lost => "lost",
            ActiveGroup::NeedsIntegration => "needs-integration",
        }
    }

    pub fn of_marker(marker: QueueLifecycleMarker) -> Self {
        match marker {
            QueueLifecycleMarker::Reported | QueueLifecycleMarker::LostReported => {
                ActiveGroup::NeedsIntegration
            }
            QueueLifecycleMarker::Lost
            | QueueLifecycleMarker::LostClaimed
            | QueueLifecycleMarker::LostRunning => ActiveGroup::Lost,
            _ => ActiveGroup::Running,
        }
    }
}

/// One durable attempt that may back an `agent:active` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveAttempt {
    pub owner: String,
    pub generation: u64,
    /// The lifecycle the attempt's records prove (claim state, or `Reported`
    /// / `LostReported` from a release receipt).
    pub marker: QueueLifecycleMarker,
    /// When the entry entered its current group, in unix seconds.
    pub since_secs: u64,
    /// Which record `since_secs` came from (`released`, `pushed`, `started`,
    /// `claimed`, ...).
    pub since_source: String,
    /// Lineage recorded an integration for this exact attempt; only the row's
    /// closeout (strike) is left.
    pub integrated: bool,
}

/// What lineage records about one exact attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineageFacts {
    /// The latest event, `(kind, at_secs)`.
    pub latest: Option<(String, u64)>,
    pub integrated: bool,
    /// Cancelled or removed: the attempt is over.
    pub ended: bool,
}

/// The durable lifecycle state `agent:active` is rendered from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveLifecycle {
    pub attempts: HashMap<QueueItemIdentity, ActiveAttempt>,
    pub now_secs: u64,
}

impl ActiveLifecycle {
    /// Join the ledgers the same way the queue lifecycle projection does:
    /// cancelled attempts are gone; an expired, unreported claim is lost; a
    /// receipt stands for its identity only when no claim does; lineage that
    /// ended an attempt removes it.
    pub fn from_ledgers(
        claims: &agent_doc_queue::queue_claim::QueueClaimLedger,
        receipts: &agent_doc_queue::claim_release_receipt::ClaimReleaseReceiptLedger,
        lineage: &HashMap<(String, u64), LineageFacts>,
        now_secs: u64,
    ) -> Self {
        let cancelled = |identity: &QueueItemIdentity, generation: u64| {
            claims
                .cancellations
                .iter()
                .any(|cancel| cancel.cancels_attempt(identity, generation))
        };
        let facts = |owner: &str, generation: u64| lineage.get(&(owner.to_string(), generation));
        let mut attempts = HashMap::new();
        for claim in &claims.claims {
            if cancelled(&claim.identity, claim.generation)
                || facts(&claim.owner, claim.generation).is_some_and(|facts| facts.ended)
            {
                continue;
            }
            let mut claim = claim.clone();
            if claim.state != agent_doc_queue::queue_claim::QueueClaimState::Reported
                && claim.is_expired(now_secs)
            {
                claim.mark_lost();
            }
            let fact = facts(&claim.owner, claim.generation)
                .cloned()
                .unwrap_or_default();
            let (since_secs, since_source) = match (&fact.latest, &claim.worker) {
                (Some((kind, at)), _) => (*at, kind.clone()),
                (None, Some(worker)) => (worker.started_at_secs, "started".to_string()),
                (None, None) => (claim.claimed_at_secs, "claimed".to_string()),
            };
            attempts.insert(
                claim.identity.clone(),
                ActiveAttempt {
                    owner: claim.owner.clone(),
                    generation: claim.generation,
                    marker: claim.projection_marker(),
                    since_secs,
                    since_source,
                    integrated: fact.integrated,
                },
            );
        }
        for receipt in &receipts.receipts {
            if attempts.contains_key(&receipt.identity)
                || cancelled(&receipt.identity, receipt.generation)
                || facts(&receipt.owner, receipt.generation).is_some_and(|facts| facts.ended)
            {
                continue;
            }
            let fact = facts(&receipt.owner, receipt.generation)
                .cloned()
                .unwrap_or_default();
            // Older receipts carry no release time; never render an epoch age.
            let (since_secs, since_source) = if receipt.released_at_secs > 0 {
                (receipt.released_at_secs, "released".to_string())
            } else if let Some((kind, at)) = &fact.latest {
                (*at, kind.clone())
            } else {
                (receipt.claimed_at_secs, "claimed".to_string())
            };
            attempts.insert(
                receipt.identity.clone(),
                ActiveAttempt {
                    owner: receipt.owner.clone(),
                    generation: receipt.generation,
                    marker: if receipt.completion_recovered_after_loss {
                        QueueLifecycleMarker::LostReported
                    } else {
                        QueueLifecycleMarker::Reported
                    },
                    since_secs,
                    since_source,
                    integrated: fact.integrated,
                },
            );
        }
        ActiveLifecycle { attempts, now_secs }
    }
}

/// Reads one document's [`ActiveLifecycle`] from durable storage.
pub type ActiveLifecycleLoader = fn(&Path) -> Result<ActiveLifecycle>;

static LOADER: OnceLock<ActiveLifecycleLoader> = OnceLock::new();

/// Install the durable loader. The binary installs it at startup, so every
/// production caller of [`crate::reconcile_managed_active_items_for`] renders
/// from durable state. A second install is ignored.
pub fn install_loader(loader: ActiveLifecycleLoader) {
    let _ = LOADER.set(loader);
}

/// The installed loader, if any.
pub fn loader() -> Option<ActiveLifecycleLoader> {
    LOADER.get().copied()
}

/// One read of a document's durable lifecycle, captured once so a reactive
/// observation can carry it as a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveLifecycleRead {
    /// No loader is installed in this process (unit tests of crates that
    /// never start the binary): the legacy marker derivation applies.
    NoLoader,
    Loaded(ActiveLifecycle),
    /// The durable state could not be read; `agent:active` stays unchanged.
    Unreadable(String),
}

/// Read `file`'s lifecycle through the installed loader.
pub fn read(file: &Path) -> ActiveLifecycleRead {
    match loader() {
        None => ActiveLifecycleRead::NoLoader,
        Some(loader) => match loader(file) {
            Ok(lifecycle) => ActiveLifecycleRead::Loaded(lifecycle),
            Err(err) => ActiveLifecycleRead::Unreadable(format!("{err:#}")),
        },
    }
}

/// Coarse, human age: `<1h`, `5h`, `3d`.
pub fn format_age(seconds: u64) -> String {
    const HOUR: u64 = 3600;
    const DAY: u64 = 24 * HOUR;
    if seconds < HOUR {
        "<1h".to_string()
    } else if seconds < 2 * DAY {
        format!("{}h", seconds / HOUR)
    } else {
        format!("{}d", seconds / DAY)
    }
}

/// UTC `YYYY-MM-DDTHH:MMZ` for a unix timestamp.
pub fn format_utc_minute(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, minute) = (rem / 3600, (rem % 3600) / 60);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}Z")
}

/// The `- **Age:**` child of a needs-integration entry.
pub fn age_line(attempt: &ActiveAttempt, now_secs: u64) -> String {
    format!(
        "**Age:** {} (since {} {})",
        format_age(now_secs.saturating_sub(attempt.since_secs)),
        attempt.since_source,
        format_utc_minute(attempt.since_secs)
    )
}

/// Strip the clock-dependent age value from an `agent:active` body so an
/// invariant check compares only what durable state determines.
pub fn without_age_values(body: &str) -> String {
    body.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            match trimmed.strip_prefix("- **Age:** ") {
                Some(rest) => {
                    let indent = &line[..line.len() - trimmed.len()];
                    let since = rest
                        .split_once(" (")
                        .map(|(_, since)| since)
                        .unwrap_or(rest);
                    format!("{indent}- **Age:** _ ({since}")
                }
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_and_timestamps_are_coarse_and_stable() {
        assert_eq!(format_age(59), "<1h");
        assert_eq!(format_age(5 * 3600 + 7), "5h");
        assert_eq!(format_age(47 * 3600), "47h");
        assert_eq!(format_age(3 * 86_400), "3d");
        assert_eq!(format_utc_minute(0), "1970-01-01T00:00Z");
        assert_eq!(format_utc_minute(1_793_765_160), "2026-11-04T04:06Z");
        assert_eq!(
            without_age_values("    - **Age:** 5h (since released 2026-11-04T02:06Z)\n- x"),
            "    - **Age:** _ (since released 2026-11-04T02:06Z)\n- x"
        );
    }
}
