//! Build concurrency for subagent queues (`#buildconcurrencygate`).
//!
//! On 2026-11-05 a `subagents` queue dispatched 20 workers whose concurrent
//! `make check` runs drove the load average to ~100 on 32 cores: wall-clock
//! test flakes, a 12.3 s IDE start past the plugin's 10 s kill, and Lazily
//! actor timeouts that failed a closeout. The coordinator had to improvise a
//! flock script. agent-doc owns the gate instead:
//!
//! * **Heavy verification** runs through `agent-doc build-slot -- <cmd...>`,
//!   which holds one of [`effective_build_slots`] machine-wide slots. The
//!   worker brief names that command ([`SUBAGENT_BUILD_SLOT_RULE`]).
//!
//! Dispatch pacing (how many workers run at once) belongs to the dispatch
//! pool, which reads the host slot count through
//! `agent_doc_queue_io::build_slot::host_base_build_slots`.
//!
//! Everything here is pure: cores and load are injected, so the sizing is
//! tested without reading the host.

use std::num::NonZeroUsize;

/// One build slot per this many cores. A Cargo build or nextest run already
/// uses every core it is given, so a slot is a whole build, not a thread.
pub const CORES_PER_BUILD_SLOT: usize = 8;

/// Environment override for the slot count (`AGENT_DOC_BUILD_SLOTS=N`).
pub const BUILD_SLOTS_ENV: &str = "AGENT_DOC_BUILD_SLOTS";

/// Environment override for the shared slot directory.
pub const BUILD_SLOT_DIR_ENV: &str = "AGENT_DOC_BUILD_SLOT_DIR";

/// Set for a command running under a slot, so a nested `build-slot` runs it
/// directly instead of waiting on the slot its own parent holds.
pub const BUILD_SLOT_HELD_ENV: &str = "AGENT_DOC_BUILD_SLOT_HELD";

/// The coordinator rule every worker brief restates.
#[macro_export]
macro_rules! subagent_build_slot_rule {
    () => {
        "Build concurrency: sibling workers share this machine's cores, so run every heavy \
         build or test command (`cargo build`/`test`/`nextest`/`clippy`, `make check`) as \
         `agent-doc build-slot -- <command...>`. It waits for a free machine-wide build slot \
         and returns the command's exit status."
    };
}

/// The build-slot rule (`subagent_build_slot_rule!`).
pub const SUBAGENT_BUILD_SLOT_RULE: &str = subagent_build_slot_rule!();

/// Slots the machine's cores support, or the configured count.
pub fn base_build_slots(cores: usize, configured: Option<NonZeroUsize>) -> NonZeroUsize {
    configured.unwrap_or_else(|| {
        NonZeroUsize::new(cores / CORES_PER_BUILD_SLOT).unwrap_or(NonZeroUsize::MIN)
    })
}

/// Slots open right now: the base count, scaled down by `cores / load1` once
/// the one-minute load average exceeds the core count. Never below one, so a
/// load the gate does not control (another user's build) slows queued
/// commands down but never stops them.
pub fn effective_build_slots(base: NonZeroUsize, cores: usize, load1: Option<f64>) -> NonZeroUsize {
    let cores = cores.max(1) as f64;
    let Some(load) = load1.filter(|load| load.is_finite() && *load > cores) else {
        return base;
    };
    let scaled = (base.get() as f64 * cores / load).floor() as usize;
    NonZeroUsize::new(scaled.min(base.get())).unwrap_or(NonZeroUsize::MIN)
}

/// Parse a positive slot count (`--slots` / [`BUILD_SLOTS_ENV`]).
pub fn parse_build_slots(value: &str) -> Result<NonZeroUsize, String> {
    value.trim().parse::<NonZeroUsize>().map_err(|_| {
        format!(
            "expected a positive build slot count, got `{}`",
            value.trim()
        )
    })
}

/// The first field of `/proc/loadavg`.
pub fn parse_loadavg(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    #[test]
    fn base_slots_are_one_per_eight_cores_with_a_floor_of_one() {
        assert_eq!(base_build_slots(32, None), n(4));
        assert_eq!(base_build_slots(64, None), n(8));
        assert_eq!(base_build_slots(12, None), n(1));
        assert_eq!(base_build_slots(4, None), n(1));
        assert_eq!(base_build_slots(0, None), n(1));
        assert_eq!(base_build_slots(32, Some(n(2))), n(2));
    }

    #[test]
    fn load_above_cores_scales_slots_down_but_never_to_zero() {
        // The 2026-11-05 incident: load ~100 on 32 cores leaves one slot.
        assert_eq!(effective_build_slots(n(4), 32, Some(100.0)), n(1));
        assert_eq!(effective_build_slots(n(4), 32, Some(40.0)), n(3));
        assert_eq!(effective_build_slots(n(4), 32, Some(64.0)), n(2));
        assert_eq!(effective_build_slots(n(4), 32, Some(1000.0)), n(1));
        // At or below the core count the base stands.
        assert_eq!(effective_build_slots(n(4), 32, Some(32.0)), n(4));
        assert_eq!(effective_build_slots(n(4), 32, Some(3.5)), n(4));
        assert_eq!(effective_build_slots(n(4), 32, None), n(4));
        assert_eq!(effective_build_slots(n(4), 32, Some(f64::NAN)), n(4));
    }

    #[test]
    fn slot_counts_and_loadavg_parse() {
        assert_eq!(parse_build_slots(" 3 "), Ok(n(3)));
        assert!(parse_build_slots("0").unwrap_err().contains("positive"));
        assert!(parse_build_slots("x").is_err());
        assert_eq!(
            parse_loadavg("97.31 80.02 60.00 3/2000 12345\n"),
            Some(97.31)
        );
        assert_eq!(parse_loadavg(""), None);
    }

    #[test]
    fn the_brief_rule_names_the_exact_command() {
        assert!(SUBAGENT_BUILD_SLOT_RULE.contains("`agent-doc build-slot -- <command...>`"));
        assert!(SUBAGENT_BUILD_SLOT_RULE.contains("`make check`"));
    }
}
