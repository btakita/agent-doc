//! Lazily-owned effect epochs shared across agent-doc crates
//! (`#implementlazilyowned`).
//!
//! `lazily::EffectCasCore` owns the effect identity, each attempt's expected
//! authority, the single-flight claim, retry eligibility, and supersession.
//! [`drive_effect_epochs`] is the one driver every bounded "observe, derive,
//! compare-and-apply, re-observe on refusal" producer runs through, so no
//! application crate keeps its own retry or supersession loop.
//!
//! The producer contributes two narrow callbacks:
//!
//! - `attempt` performs exactly one claimed attempt and maps the backend
//!   outcome to an [`EpochStep`]. `Superseded` means the backend authority
//!   moved past the cut the attempt compared against; Lazily records it and the
//!   still-wanted effect is re-desired over the observed authority as the next
//!   epoch. `Retryable` means the authority is unchanged and the backend asked
//!   to be asked again; Lazily records it and re-claims the same epoch from
//!   its retained intent. An `Err` is a Lazily rejection and is returned as-is.
//! - `between` runs only when Lazily will claim another attempt. It holds the
//!   diagnostics and the physical wait (a backoff, a delivery-convergence
//!   witness); it never decides whether to retry. It may stop the drive with
//!   [`Pace::Stop`] (a fail-closed witness: the drive returns
//!   [`EpochDrive::Stopped`] with the owned reason) or abort it with an error.
//!
//! After `max_attempts` superseded or retryable attempts the drive returns
//! [`EpochDrive::Exhausted`] with the last reason, so the caller renders its own
//! exhaustion message unchanged.

use anyhow::{Result, bail};
use lazily::{
    EffectApply, EffectCasApplied, EffectCasClaim, EffectCasCore, EffectCasFailure,
    EffectCasOutcome, EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan,
};

/// The single actor generation of an in-process drive. Cross-process
/// ownership, where it exists, is a durable claim owned by the caller.
const GENERATION: u64 = 0;

/// Key = drive identity, authority = opaque token, payload = the epoch,
/// receipt = the applied authority, error = rendered reason.
type EpochCore = EffectCasCore<String, String, u64, String, String>;

/// One claimed attempt as Lazily hands it to the producer.
#[derive(Debug, Clone, Copy)]
pub struct EpochAttempt<'a> {
    /// 1-based attempt number across the whole drive.
    pub attempt: u32,
    /// The drive's attempt budget.
    pub max_attempts: u32,
    /// The Lazily epoch of the desired effect this attempt serves.
    pub epoch: u64,
    /// The Lazily-owned expected authority of this attempt.
    pub expected: &'a str,
}

impl EpochAttempt<'_> {
    /// Whether this is the last attempt Lazily will claim.
    pub fn is_final(&self) -> bool {
        self.attempt >= self.max_attempts
    }
}

/// The only outcomes an attempt may report.
#[derive(Debug)]
pub enum EpochStep<R, E> {
    /// The effect landed over `authority`.
    Applied { receipt: R, authority: String },
    /// The backend authority moved: Lazily records `Superseded { observed }`
    /// and re-desires the effect over `observed` as the next epoch.
    Superseded { observed: String, reason: E },
    /// Transient, authority unchanged: Lazily re-claims the same epoch.
    Retryable(E),
}

/// What `between` reports after observing the wait before another attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// Let Lazily claim the next attempt.
    Continue,
    /// Fail closed: the wait proved another attempt cannot succeed.
    Stop,
}

/// Terminal result of a drive that was not rejected.
#[derive(Debug)]
pub enum EpochDrive<R, E> {
    Applied(R),
    /// Every attempt was superseded or retryable; `reason` is the last one.
    Exhausted {
        attempts: u32,
        reason: E,
    },
    /// `between` stopped the drive after attempt `attempt`.
    Stopped {
        attempt: u32,
        reason: E,
    },
}

fn desire(core: &mut EpochCore, identity: &str, epoch: u64, expected: String) -> Result<()> {
    let intent = EffectIntent {
        identity: EffectIdentity::new(format!("{identity}#{epoch}"))
            .map_err(|error| anyhow::anyhow!("invalid effect identity {identity}: {error:?}"))?,
        epoch,
        key: identity.to_string(),
        expected,
        plan: EffectPlan::single(epoch),
    };
    match core.upsert_desired(intent).1 {
        EffectCasUpsert::Accepted => Ok(()),
        other => bail!("{identity}: effect epoch {epoch} was not accepted: {other:?}"),
    }
}

/// Drive one bounded effect through Lazily epochs. See the module docs.
pub fn drive_effect_epochs<R, E>(
    identity: &str,
    initial_authority: impl Into<String>,
    max_attempts: u32,
    attempt: &mut dyn FnMut(&EpochAttempt<'_>) -> Result<EpochStep<R, E>>,
    between: &mut dyn FnMut(u32, &E) -> Result<Pace>,
) -> Result<EpochDrive<R, E>> {
    let key = identity.to_string();
    let max_attempts = max_attempts.max(1);
    let mut core = EpochCore::new(GENERATION);
    let mut epoch = 1;
    desire(&mut core, identity, epoch, initial_authority.into())?;
    let mut now = 0;
    for number in 1..=max_attempts {
        let claimed = match core.claim(&key, GENERATION, now).1 {
            EffectCasClaim::Claimed(claimed) => claimed,
            other => bail!("{identity}: attempt {number} was not claimed: {other:?}"),
        };
        let step = attempt(&EpochAttempt {
            attempt: number,
            max_attempts,
            epoch: claimed.identity.epoch,
            expected: &claimed.expected,
        });
        let step = match step {
            Ok(step) => step,
            Err(error) => {
                let rejected = EffectApply::Rejected(format!("{error:#}"));
                core.record_outcome(&key, &claimed.identity, rejected, now + 1);
                return Err(error);
            }
        };
        let (apply, step) = match step {
            EpochStep::Applied { receipt, authority } => (
                EffectApply::Applied {
                    receipt: authority.clone(),
                    authority,
                },
                Ok(receipt),
            ),
            EpochStep::Superseded { observed, reason } => (
                EffectApply::Superseded {
                    observed: observed.clone(),
                },
                Err((Some(observed), reason)),
            ),
            EpochStep::Retryable(reason) => (
                EffectApply::Retryable(format!("retryable attempt {number}")),
                Err((None, reason)),
            ),
        };
        let recorded = core
            .record_outcome(&key, &claimed.identity, apply, now + 1)
            .1;
        match (recorded, step) {
            (EffectCasOutcome::Applied(EffectCasApplied::Completed { .. }), Ok(receipt)) => {
                return Ok(EpochDrive::Applied(receipt));
            }
            (
                EffectCasOutcome::Failure(EffectCasFailure::Superseded),
                Err((Some(observed), reason)),
            ) => {
                if number == max_attempts {
                    return Ok(EpochDrive::Exhausted {
                        attempts: max_attempts,
                        reason,
                    });
                }
                if between(number, &reason)? == Pace::Stop {
                    return Ok(EpochDrive::Stopped {
                        attempt: number,
                        reason,
                    });
                }
                epoch += 1;
                desire(&mut core, identity, epoch, observed)?;
            }
            (
                EffectCasOutcome::Failure(EffectCasFailure::Retrying { retry_at }),
                Err((None, reason)),
            ) => {
                if number == max_attempts {
                    return Ok(EpochDrive::Exhausted {
                        attempts: max_attempts,
                        reason,
                    });
                }
                if between(number, &reason)? == Pace::Stop {
                    return Ok(EpochDrive::Stopped {
                        attempt: number,
                        reason,
                    });
                }
                now = retry_at;
            }
            (recorded, _) => {
                bail!("{identity}: attempt {number} outcome diverged from Lazily: {recorded:?}")
            }
        }
    }
    unreachable!("the final attempt always returns")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Seen {
        attempt: u32,
        epoch: u64,
        expected: String,
        final_attempt: bool,
    }

    fn seen(attempt: &EpochAttempt<'_>) -> Seen {
        Seen {
            attempt: attempt.attempt,
            epoch: attempt.epoch,
            expected: attempt.expected.to_string(),
            final_attempt: attempt.is_final(),
        }
    }

    #[test]
    fn superseded_attempt_redesires_the_next_epoch_over_the_observed_authority() -> Result<()> {
        let mut log = Vec::new();
        let mut paced = Vec::new();
        let drive = drive_effect_epochs(
            "test:superseded",
            "cut-a",
            3,
            &mut |attempt| {
                log.push(seen(attempt));
                Ok(if attempt.attempt == 1 {
                    EpochStep::Superseded {
                        observed: "cut-b".to_string(),
                        reason: "moved",
                    }
                } else {
                    EpochStep::Applied {
                        receipt: attempt.expected.to_string(),
                        authority: "cut-c".to_string(),
                    }
                })
            },
            &mut |number, reason| {
                paced.push((number, *reason));
                Ok(Pace::Continue)
            },
        )?;
        assert!(matches!(drive, EpochDrive::Applied(ref receipt) if receipt == "cut-b"));
        assert_eq!(
            log,
            vec![
                Seen {
                    attempt: 1,
                    epoch: 1,
                    expected: "cut-a".to_string(),
                    final_attempt: false,
                },
                Seen {
                    attempt: 2,
                    epoch: 2,
                    expected: "cut-b".to_string(),
                    final_attempt: false,
                },
            ]
        );
        assert_eq!(paced, vec![(1, "moved")]);
        Ok(())
    }

    #[test]
    fn retryable_attempt_reclaims_the_same_epoch_and_authority() -> Result<()> {
        let mut log = Vec::new();
        let drive = drive_effect_epochs(
            "test:retryable",
            "cut-a",
            3,
            &mut |attempt| {
                log.push(seen(attempt));
                Ok(if attempt.attempt < 3 {
                    EpochStep::Retryable("transient")
                } else {
                    EpochStep::Applied {
                        receipt: attempt.attempt,
                        authority: attempt.expected.to_string(),
                    }
                })
            },
            &mut |_, _| Ok(Pace::Continue),
        )?;
        assert!(matches!(drive, EpochDrive::Applied(3)));
        assert!(
            log.iter()
                .all(|seen| seen.epoch == 1 && seen.expected == "cut-a")
        );
        assert_eq!(log.len(), 3);
        assert!(log[2].final_attempt);
        Ok(())
    }

    #[test]
    fn exhaustion_returns_the_last_reason_without_pacing_after_the_final_attempt() -> Result<()> {
        let mut paced = Vec::new();
        let drive = drive_effect_epochs::<(), u32>(
            "test:exhausted",
            "cut-0",
            3,
            &mut |attempt| {
                Ok(EpochStep::Superseded {
                    observed: format!("cut-{}", attempt.attempt),
                    reason: attempt.attempt,
                })
            },
            &mut |number, _| {
                paced.push(number);
                Ok(Pace::Continue)
            },
        )?;
        assert!(matches!(
            drive,
            EpochDrive::Exhausted {
                attempts: 3,
                reason: 3
            }
        ));
        assert_eq!(paced, vec![1, 2]);
        Ok(())
    }

    #[test]
    fn rejected_attempt_returns_its_error_at_once_without_pacing() {
        let mut attempts = 0;
        let mut paced = 0;
        let error = drive_effect_epochs::<(), ()>(
            "test:rejected",
            "cut-a",
            5,
            &mut |_| {
                attempts += 1;
                anyhow::bail!("fatal backend error")
            },
            &mut |_, _| {
                paced += 1;
                Ok(Pace::Continue)
            },
        )
        .unwrap_err();
        assert_eq!(format!("{error:#}"), "fatal backend error");
        assert_eq!((attempts, paced), (1, 0));
    }

    #[test]
    fn a_pacing_error_aborts_the_drive_before_another_claim() {
        let mut attempts = 0;
        let error = drive_effect_epochs::<(), &str>(
            "test:abort",
            "cut-a",
            5,
            &mut |_| {
                attempts += 1;
                Ok(EpochStep::Retryable("transient"))
            },
            &mut |_, reason| anyhow::bail!("fail closed after {reason}"),
        )
        .unwrap_err();
        assert_eq!(format!("{error:#}"), "fail closed after transient");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn a_stopped_pace_returns_the_owned_reason_before_another_claim() -> Result<()> {
        let mut attempts = 0;
        let drive = drive_effect_epochs::<(), String>(
            "test:stop",
            "cut-a",
            5,
            &mut |_| {
                attempts += 1;
                Ok(EpochStep::Retryable("refused".to_string()))
            },
            &mut |_, _| Ok(Pace::Stop),
        )?;
        assert!(matches!(
            drive,
            EpochDrive::Stopped { attempt: 1, ref reason } if reason == "refused"
        ));
        assert_eq!(attempts, 1);
        Ok(())
    }

    #[test]
    fn a_single_attempt_budget_never_paces() -> Result<()> {
        let mut paced = 0;
        let drive = drive_effect_epochs::<(), &str>(
            "test:single",
            "cut-a",
            0,
            &mut |attempt| {
                assert!(attempt.is_final());
                Ok(EpochStep::Retryable("transient"))
            },
            &mut |_, _| {
                paced += 1;
                Ok(Pace::Continue)
            },
        )?;
        assert!(matches!(
            drive,
            EpochDrive::Exhausted {
                attempts: 1,
                reason: "transient"
            }
        ));
        assert_eq!(paced, 0);
        Ok(())
    }
}
