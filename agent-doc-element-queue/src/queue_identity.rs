//! The one rule that gives every runnable component (`agent:run`,
//! `agent:loop`, legacy `agent:queue`) its canonical queue id
//! (`#idlessqueueidentity`).
//!
//! Queue ids address durable state: schedule rows, loop pass counters, the
//! dispatch brief's origin queue, `queue follow-up --id`, and the document-cell
//! merge's component pairing. Every reader resolves them through
//! [`resolve_queue_ids`], so the queue graph, preflight's id backfill and the
//! merge can never disagree about which component a queue id names.
//!
//! In document order, a runnable component's id is:
//!
//! 1. its explicit `id=` when it carries a value (an explicit id always wins);
//! 2. [`LEGACY_SINGLE_QUEUE_ID`] (`default`) when it is the document's only
//!    runnable component and has no usable id;
//! 3. its **kind** (`run` or `loop`, see [`QueueKind`]) when its `id=` is
//!    absent, it is the only runnable component of that kind in the document,
//!    and no explicit `id=` already spells that kind;
//! 4. otherwise unresolved ([`ResolvedQueueId::Ambiguous`]): two components
//!    of one kind without ids, an id-less component whose kind id is taken by
//!    an explicit `id=`, or a malformed id (a bare `id`, `id=`, or `id=true`).
//!    Readers fail closed on it.
//!
//! The kind is what the component *is*: `agent:loop` (and the legacy
//! `agent:run loop` / `agent:queue loop` spellings) repeats, every other
//! runnable component runs once. Heading text never contributes: a heading is
//! freely edited prose, and renaming `## Queue: Main` to `## Run` must not move
//! the queue's durable state. A derived id is computed, never written back, so
//! an operator who deletes the ids keeps an id-free marker.

/// The identity of a document's only runnable component when it has no id.
pub const LEGACY_SINGLE_QUEUE_ID: &str = "default";

/// What a runnable component is, and the id it derives when its `id=` is
/// absent and the kind is unique in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueKind {
    /// Runs its rows once (`agent:run`, legacy `agent:queue`).
    Run,
    /// Repeats its rows every pass (`agent:loop`, `agent:run loop`).
    Loop,
}

impl QueueKind {
    /// The kind's derived queue id and marker spelling (`run` / `loop`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Loop => "loop",
        }
    }

    /// The kind of a component whose parsed attributes do (`loop` / `loop=N`)
    /// or do not carry the repeating flag. `agent:loop` implies the flag in
    /// both the element parser and the markdown overlay.
    pub fn from_loop_flag(is_loop: bool) -> Self {
        if is_loop { Self::Loop } else { Self::Run }
    }
}

/// One runnable component, as [`resolve_queue_ids`] sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueIdentityInput<'a> {
    /// The raw `id` attribute value: `None` when the attribute is absent,
    /// `Some("")` for a bare `id` / empty `id=`.
    pub explicit_id: Option<&'a str>,
    pub kind: QueueKind,
}

/// Why a runnable component has no resolvable id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmbiguousQueueId {
    /// The `id` attribute is present but bare, empty, or `true`.
    MalformedId,
    /// The document holds several components of this kind (zero-based
    /// positions among the runnable components, in document order), so the
    /// kind cannot name this id-less one.
    SharedKind { siblings: Vec<usize> },
    /// The kind's id is already spelled by an explicit `id=` at `owner`.
    KindIdTaken { owner: usize },
}

/// A runnable component's resolved identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedQueueId {
    Explicit(String),
    /// [`LEGACY_SINGLE_QUEUE_ID`].
    LegacySingle,
    Derived(QueueKind),
    Ambiguous(AmbiguousQueueId),
}

impl ResolvedQueueId {
    /// The canonical id string, unless the identity is ambiguous.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Explicit(id) => Some(id),
            Self::LegacySingle => Some(LEGACY_SINGLE_QUEUE_ID),
            Self::Derived(kind) => Some(kind.as_str()),
            Self::Ambiguous(_) => None,
        }
    }
}

/// A usable explicit id: trimmed, nonempty and not the bare-flag `true`.
fn usable_explicit(raw: Option<&str>) -> Option<&str> {
    let id = raw?.trim();
    (!id.is_empty() && id != "true").then_some(id)
}

/// Resolve every runnable component's id, index-aligned with `inputs` (the
/// document's runnable components in document order). See the module docs.
///
/// Explicit ids are returned verbatim (trimmed); validating their syntax and
/// uniqueness stays with the queue graph.
pub fn resolve_queue_ids(inputs: &[QueueIdentityInput<'_>]) -> Vec<ResolvedQueueId> {
    let lone = inputs.len() == 1;
    inputs
        .iter()
        .enumerate()
        .map(|(position, input)| {
            if let Some(id) = usable_explicit(input.explicit_id) {
                return ResolvedQueueId::Explicit(id.to_string());
            }
            if lone {
                return ResolvedQueueId::LegacySingle;
            }
            if input.explicit_id.is_some() {
                return ResolvedQueueId::Ambiguous(AmbiguousQueueId::MalformedId);
            }
            let siblings = inputs
                .iter()
                .enumerate()
                .filter(|(_, other)| other.kind == input.kind)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            // An explicitly identified sibling of the same kind still makes the
            // kind name ambiguous for the id-less one: `run` must not name one
            // of two runs.
            if siblings.len() > 1 {
                return ResolvedQueueId::Ambiguous(AmbiguousQueueId::SharedKind { siblings });
            }
            if let Some(owner) = inputs
                .iter()
                .position(|other| usable_explicit(other.explicit_id) == Some(input.kind.as_str()))
            {
                return ResolvedQueueId::Ambiguous(AmbiguousQueueId::KindIdTaken { owner });
            }
            debug_assert_eq!(siblings, vec![position]);
            ResolvedQueueId::Derived(input.kind)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: Option<&str>) -> QueueIdentityInput<'_> {
        QueueIdentityInput {
            explicit_id: id,
            kind: QueueKind::Run,
        }
    }

    fn looped(id: Option<&str>) -> QueueIdentityInput<'_> {
        QueueIdentityInput {
            explicit_id: id,
            kind: QueueKind::Loop,
        }
    }

    fn ids(inputs: &[QueueIdentityInput<'_>]) -> Vec<Option<String>> {
        resolve_queue_ids(inputs)
            .iter()
            .map(|resolved| resolved.id().map(str::to_string))
            .collect()
    }

    #[test]
    fn idless_run_and_loop_derive_their_kind() {
        assert_eq!(
            ids(&[run(None), looped(None)]),
            vec![Some("run".into()), Some("loop".into())]
        );
        // Order is irrelevant: the identity is the kind, not the position.
        assert_eq!(
            ids(&[looped(None), run(None)]),
            vec![Some("loop".into()), Some("run".into())]
        );
    }

    #[test]
    fn explicit_ids_win_and_a_lone_queue_stays_default() {
        assert_eq!(
            ids(&[run(Some("main")), looped(Some("issues"))]),
            vec![Some("main".into()), Some("issues".into())]
        );
        assert_eq!(
            ids(&[run(Some("main")), looped(None)]),
            vec![Some("main".into()), Some("loop".into())]
        );
        assert_eq!(ids(&[looped(None)]), vec![Some("default".into())]);
        assert_eq!(ids(&[run(Some(""))]), vec![Some("default".into())]);
    }

    #[test]
    fn same_kind_or_taken_kind_id_is_ambiguous() {
        assert_eq!(
            resolve_queue_ids(&[run(None), run(None), looped(None)]),
            vec![
                ResolvedQueueId::Ambiguous(AmbiguousQueueId::SharedKind {
                    siblings: vec![0, 1]
                }),
                ResolvedQueueId::Ambiguous(AmbiguousQueueId::SharedKind {
                    siblings: vec![0, 1]
                }),
                ResolvedQueueId::Derived(QueueKind::Loop),
            ]
        );
        assert_eq!(
            resolve_queue_ids(&[run(Some("a")), run(None)])[1],
            ResolvedQueueId::Ambiguous(AmbiguousQueueId::SharedKind {
                siblings: vec![0, 1]
            })
        );
        assert_eq!(
            resolve_queue_ids(&[looped(Some("run")), run(None)])[1],
            ResolvedQueueId::Ambiguous(AmbiguousQueueId::KindIdTaken { owner: 0 })
        );
        assert_eq!(
            resolve_queue_ids(&[run(Some("a")), looped(Some(" "))])[1],
            ResolvedQueueId::Ambiguous(AmbiguousQueueId::MalformedId)
        );
    }
}
