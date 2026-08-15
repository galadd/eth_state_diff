//! # `eth-state-diff`
//!
//! High-performance delta encoding and reconstruction for Ethereum consensus
//! state.
//!
//! This crate computes compact deltas between two beacon states and applies
//! those deltas to reconstruct the target state without requiring the entire
//! state to be serialized or rewritten.
//!
//! The delta format is designed around the update semantics of individual
//! Ethereum consensus-state fields. Depending on the field, the crate uses
//! specialized representations such as sparse updates, append-only deltas,
//! circular-buffer writes, FIFO queue deltas, and full replacements.
//!
//! ## Overview
//!
//! A state transition is represented by [`BeaconStateDelta`]. The transition
//! has two stages:
//!
//! 1. [`create`] compares the base and target state through [`DiffSource`] and
//!    constructs a [`BeaconStateDelta`].
//! 2. [`apply`] applies the delta to a state through [`DiffTarget`], producing
//!    the target state.
//!
//! ```text
//!
//!        base state ───────┐
//!                           │
//!                      [`create`]
//!                           │
//!                           ▼
//!                  [`BeaconStateDelta`]
//!                           │
//!                      serialize
//!                           │
//!                           ▼
//!                    transport / storage
//!                           │
//!                       deserialize
//!                           │
//!                           ▼
//!                  [`ArchivedBeaconStateDelta`]
//!                           │
//!                      [`apply`]
//!                           │
//!                           ▼
//!                       target state
//!
//! ```
//!
//! The crate does not impose a particular beacon-state storage layout.
//! [`DiffSource`] and [`DiffTarget`] provide the integration boundary between
//! the delta algorithms and a consensus client's state representation.
//!
//! ## Delta representations
//!
//! Each state component uses an encoding appropriate to its update pattern:
//!
//! - **Balances** use packed 2-bit tags and compact difference encoding.
//! - **Validators** use field-level patches rather than rewriting complete
//!   validator records.
//! - **Recent roots** record only roots written to circular buffers during the
//!   diff window.
//! - **RANDAO mixes** record the mixes written as epochs advance.
//! - **Slashings** use sparse ring-buffer updates.
//! - **Eth1 data votes** use append/reset semantics.
//! - **Historical roots and summaries** use protocol-defined append intervals.
//! - **Attestations** use unchanged, append, or replacement representations.
//! - **Participation flags** use packed sparse updates and an all-zero fast path.
//! - **Inactivity scores** use sparse updates and an all-zero representation.
//! - **Sync committees** use unchanged/full-replacement encoding.
//! - **Pending deposits, withdrawals, and consolidations** use a validated FIFO
//!   representation with full-replacement fallback.
//!
//! This specialization allows the delta to represent the *state transition*
//! rather than treating the serialized beacon state as one opaque byte array.
//!
//! ## Serialization
//!
//! Delta structures derive [`rkyv::Archive`], [`rkyv::Serialize`], and
//! [`rkyv::Deserialize`] and are therefore suitable for zero-copy or archived
//! representations where appropriate.
//!
//! The delta algorithms themselves operate on native Rust values and serialized
//! SSZ byte sequences where field-level SSZ representation is required.
//! Serialization is deliberately kept separate from the diff algorithms.
//!
//! ## Fork handling
//!
//! [`ForkName`] identifies the consensus fork associated with a delta.
//!
//! Fork-specific fields are represented as `Option<T>` inside
//! [`BeaconStateDelta`]. A field is populated only when it exists for the
//! corresponding fork. [`apply`] validates these invariants before modifying
//! the destination state and rejects fork mismatches or fields that are
//! invalid for the delta's fork.
//!
//! ## Integration
//!
//! Consensus clients integrate with this crate by implementing two traits:
//!
//! - [`DiffSource`] exposes the base and target state components required to
//!   create a delta.
//! - [`DiffTarget`] exposes mutable access to the state components required to
//!   apply a delta.
//!
//! Collection-specific integration can additionally use [`ListMutTarget`] for
//! list-like collections and [`ValidatorMutTarget`] for validator registries.
//!
//! The crate intentionally does not require a particular consensus-client
//! implementation, allocation strategy, or state storage backend.

pub mod attestations;
pub mod balances;
pub mod eth1_data_votes;
pub mod historical_log;
pub mod inactivity_scores;
pub mod participation;
pub mod pending_queue;
pub mod randao_mixes;
pub mod recent_roots;
pub mod slashings;
pub mod sync_committee;
pub mod types;
pub mod validators;

pub mod error;
use error::Error;

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    types::{
        AttestationsDiff, BalancesDiff, Eth1DataVotesDiff, HistoricalLogDiff, InactivityDiff,
        ParticipationDiff, QueueDiff, RandaoDiff, RootsDiff, SlashingsDiff, SyncCommitteeDiff,
        ValidatorsDiff, HISTORICAL_ROOTS_SSZ_SIZE, HISTORICAL_SUMMARIES_SSZ_SIZE,
    },
    validators::{ValidatorMutTarget, ValidatorSnapshot},
};

/// Identifies the Ethereum consensus fork associated with a beacon state or
/// state delta.
///
/// The discriminants are explicit and stable within the delta representation.
/// They are used when validating that a delta is applied to a state belonging
/// to the same fork.
///
/// Fork ordering follows the protocol progression, so the variants can also be
/// compared to determine whether a fork-specific field has been introduced.
///
/// # Examples
///
/// ```
/// use eth_state_diff::ForkName;
///
/// assert!(ForkName::Electra > ForkName::Capella);
/// assert!(ForkName::Altair >= ForkName::Phase0);
/// ```
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ForkName {
    /// Phase 0 beacon state.
    Phase0 = 0,
    /// Altair beacon state.
    Altair = 1,
    /// Bellatrix beacon state.
    Bellatrix = 2,
    /// Capella beacon state.
    Capella = 3,
    /// Deneb beacon state.
    Deneb = 4,
    /// Electra beacon state.
    Electra = 5,
    /// Fulu beacon state.
    Fulu = 6,
    /// Gloas beacon state.
    Gloas = 7,
    /// Heze beacon state.
    Heze = 8,
}

/// Read-only state interface used by [`create`].
///
/// Implement this trait to expose the base and target beacon states to the
/// specialized delta algorithms without requiring a particular state-storage
/// implementation.
///
/// Each accessor returns either:
///
/// - the state component itself when it exists for the current fork, or
/// - `None` for fork-specific fields that do not exist.
///
/// The first value returned by a pair of iterators or slices represents the
/// base state and the second represents the target state.
///
/// # Base and target state
///
/// The implementation must ensure that all returned components refer to the
/// same pair of states. Mixing components from different state transitions
/// produces a delta that cannot correctly reconstruct the target.
///
/// # Slots
///
/// [`slot`](Self::slot) returns `(base_slot, target_slot)`. These slots are used
/// to reconstruct slot- and epoch-indexed circular buffers.
///
/// # Scalar header
///
/// [`scalar_header`](Self::scalar_header) must contain exactly the state bytes
/// that are not handled by the specialized delta encoders.
///
/// Fields already represented by dedicated delta fields must not also appear
/// in the scalar header.
///
/// # Fork-specific fields
///
/// Accessors for fields introduced by later forks must return `None` when the
/// source state belongs to an earlier fork.
pub trait DiffSource {
    fn fork(&self) -> ForkName;
    fn slot(&self) -> (u64, u64);
    fn capella_fork_slot(&self) -> u64; // Needed for historical_summaries math

    /// Returns the serialized SSZ bytes for consensus state fields that are
    /// not covered by specialized diffing algorithms.
    ///
    /// # Required SSZ Layout
    ///
    /// To ensure deterministic reconstruction across clients, the bytes MUST
    /// be concatenated in the exact order defined by the consensus spec for
    /// the target state's fork. The fields generally include:
    ///
    /// - `genesis_time` (8 bytes)
    /// - `genesis_validators_root` (32 bytes)
    /// - `slot` (8 bytes)
    /// - `fork` (Fork struct, variable bytes)
    /// - `latest_block_header` (BeaconBlockHeader struct)
    /// - `eth1_data` (Eth1Data struct)
    /// - `eth1_deposit_index` (8 bytes)
    /// - `justification_bits` (BitVector)
    /// - Checkpoints: `previous_justified`, `current_justified`, `finalized`
    /// - `latest_execution_payload_header` (ExecutionPayloadHeader struct)
    /// - Electra+ scalar fields: `next_withdrawal_index`, `next_withdrawal_validator_index`,
    ///   `deposit_requests_start_index`, `deposit_balance_to_consume`, etc.
    ///
    /// **Note:** Fields that have dedicated diffing algorithms (e.g., `balances`,
    /// `historical_summaries`, `pending_deposits`) MUST NOT be included in this blob.
    fn scalar_header(&self) -> Vec<u8>;

    // Universal
    fn balances(
        &self,
    ) -> (
        impl ExactSizeIterator<Item = u64>,
        impl ExactSizeIterator<Item = u64>,
    );
    fn validators(
        &self,
    ) -> (
        impl ExactSizeIterator<Item = impl ValidatorSnapshot>,
        impl ExactSizeIterator<Item = impl ValidatorSnapshot>,
    );
    fn block_roots(&self) -> &[[u8; 32]];
    fn state_roots(&self) -> &[[u8; 32]];
    fn randao_mixes(&self) -> &[[u8; 32]];
    fn slashings(&self) -> (&[u64], &[u64]);
    fn eth1_data_votes(&self) -> (&[u8], &[u8]);
    fn historical_roots(&self) -> Option<&[u8]>;

    // Phase0
    fn previous_epoch_attestations(&self) -> Option<(&[u8], &[u8])>;
    fn current_epoch_attestations(&self) -> Option<(&[u8], &[u8])>;

    // Altair+
    fn previous_participation(
        &self,
    ) -> Option<(
        impl ExactSizeIterator<Item = u8>,
        impl ExactSizeIterator<Item = u8>,
    )>;
    fn current_participation(
        &self,
    ) -> Option<(
        impl ExactSizeIterator<Item = u8>,
        impl ExactSizeIterator<Item = u8>,
    )>;
    fn inactivity_scores(&self) -> Option<(&[u64], &[u64])>;
    fn current_sync_committee(&self) -> Option<(&[u8], &[u8])>;
    fn next_sync_committee(&self) -> Option<(&[u8], &[u8])>;

    // Capella+
    fn historical_summaries(&self) -> Option<&[u8]>;

    // Electra+
    fn pending_deposits(&self) -> Option<(&[u8], &[u8])>;
    fn pending_partial_withdrawals(&self) -> Option<(&[u8], &[u8])>;
    fn pending_consolidations(&self) -> Option<(&[u8], &[u8])>;
}

/// Mutable state interface used by [`apply`].
///
/// Implement this trait for a beacon-state representation to allow an archived
/// [`BeaconStateDelta`] to be applied directly to the client's native state.
///
/// The trait deliberately exposes only the mutable views required by the delta
/// algorithms. It does not require the underlying state to use the same memory
/// layout as Ethereum's SSZ representation.
///
/// # Fork requirements
///
/// [`get_fork`](Self::get_fork) must return the fork of the state being
/// modified. [`apply`] rejects the operation if it differs from the fork stored
/// in the delta.
///
/// Fork-specific accessors should return `None` when the corresponding field
/// does not exist in the state representation.
///
/// # Mutation
///
/// Implementations must return references to the actual state storage.
/// `apply` mutates these collections in place.
///
/// If an implementation returns a view backed by temporary storage rather than
/// the actual state, the reconstructed values will not be persisted to the
/// beacon state.
pub trait DiffTarget {
    /// Returns the fork of the state being modified.
    ///
    /// This value must match [`BeaconStateDelta::fork`] for [`apply`] to
    /// proceed.
    fn get_fork(&self) -> ForkName;

    /// Returns mutable storage for the scalar state header.
    ///
    /// The returned buffer is replaced with the scalar bytes stored in the
    /// delta.
    fn scalar_header_mut(&mut self) -> &mut Vec<u8>;

    // Universal
    fn balances_mut(&mut self) -> &mut impl ListMutTarget<u64>;
    fn validators_mut(&mut self) -> &mut impl ValidatorMutTarget;
    fn block_roots_mut(&mut self) -> &mut [[u8; 32]];
    fn state_roots_mut(&mut self) -> &mut [[u8; 32]];
    fn randao_mixes_mut(&mut self) -> &mut [[u8; 32]];
    fn slashings_mut(&mut self) -> &mut [u64];
    fn eth1_data_votes_mut(&mut self) -> &mut Vec<u8>;
    fn historical_roots_mut(&mut self) -> Option<&mut Vec<u8>>;

    // Phase0 specific

    /// Returns mutable access to `previous_epoch_attestations`.
    ///
    /// Returns `None` for forks where this field does not exist.
    fn previous_epoch_attestations_mut(&mut self) -> Option<&mut Vec<u8>>;

    /// Returns mutable access to `previous_epoch_attestations`.
    ///
    /// Returns `None` for forks where this field does not exist.
    fn current_epoch_attestations_mut(&mut self) -> Option<&mut Vec<u8>>;

    // Altair+
    fn previous_participation_mut(&mut self) -> Option<&mut impl ListMutTarget<u8>>;
    fn current_participation_mut(&mut self) -> Option<&mut impl ListMutTarget<u8>>;
    fn inactivity_scores_mut(&mut self) -> Option<&mut Vec<u64>>;
    fn current_sync_committee_mut(&mut self) -> Option<&mut Vec<u8>>;
    fn next_sync_committee_mut(&mut self) -> Option<&mut Vec<u8>>;

    // Capella+
    fn historical_summaries_mut(&mut self) -> Option<&mut Vec<u8>>;

    // Electra+
    fn pending_deposits_mut(&mut self) -> Option<&mut Vec<u8>>;
    fn pending_partial_withdrawals_mut(&mut self) -> Option<&mut Vec<u8>>;
    fn pending_consolidations_mut(&mut self) -> Option<&mut Vec<u8>>;
}

/// Complete compact representation of the transition between two beacon
/// states.
///
/// A [`BeaconStateDelta`] contains one specialized delta for each state
/// component handled by this crate. Applying the delta to the corresponding
/// base state reconstructs the target state.
///
/// The delta records the fork and base slot required to interpret fork-specific
/// fields and circular-buffer updates.
///
/// # Fork-specific fields
///
/// Fields introduced by later consensus forks are represented as `Option<T>`:
///
/// - Phase 0 fields are present only on Phase 0.
/// - Altair fields are present on Altair and later forks.
/// - Capella fields are present on Capella and later forks.
/// - Electra fields are present on Electra and later forks.
///
/// [`apply`] validates these invariants before modifying the destination state.
///
/// # Serialization
///
/// This type derives [`rkyv::Archive`], [`rkyv::Serialize`], and
/// [`rkyv::Deserialize`] and can therefore be archived for storage or
/// transmission.
///
/// The delta algorithms themselves do not require the delta to be serialized.
///
/// # Lifecycle
///
/// A typical workflow is:
///
/// ```text
/// DiffSource
///     │
///     ▼
///  create()
///     │
///     ▼
/// BeaconStateDelta
///     │
///     ├── serialize / store / transmit
///     │
///     ▼
/// ArchivedBeaconStateDelta
///     │
///     ▼
///   apply()
///     │
///     ▼
/// DiffTarget
/// ```
///
/// # Correctness
///
/// The delta is intended to reconstruct the target state represented by the
/// [`DiffSource`] used during creation. The caller is responsible for ensuring
/// that the destination state corresponds to the base state from which the
/// delta was created.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BeaconStateDelta {
    pub fork: ForkName,
    pub base_slot: u64,
    pub scalar_header: Vec<u8>,

    // --- Universal (Phase0+) ---
    pub balances: BalancesDiff,
    pub validators: ValidatorsDiff,
    pub block_roots: RootsDiff,
    pub state_roots: RootsDiff,
    pub randao_mixes: RandaoDiff,
    pub slashings: SlashingsDiff,
    pub eth1_data_votes: Eth1DataVotesDiff,
    pub historical_roots: Option<HistoricalLogDiff>,

    // --- Phase0 Specific ---
    /// `Some` for Phase0. `None` for Altair+.
    pub previous_epoch_attestations: Option<AttestationsDiff>,
    pub current_epoch_attestations: Option<AttestationsDiff>,

    // --- Altair+ ---
    /// `None` for Phase0. `Some` for Altair+.
    pub previous_participation: Option<ParticipationDiff>,
    pub current_participation: Option<ParticipationDiff>,
    pub inactivity_scores: Option<InactivityDiff>,
    pub current_sync_committee: Option<SyncCommitteeDiff>,
    pub next_sync_committee: Option<SyncCommitteeDiff>,

    // --- Capella+ ---
    /// `None` for pre-Capella. `Some` for Capella+.
    pub historical_summaries: Option<HistoricalLogDiff>,

    // --- Electra+ ---
    /// `None` for pre-Electra. `Some` for Electra+.
    pub pending_deposits: Option<QueueDiff>,
    pub pending_partial_withdrawals: Option<QueueDiff>,
    pub pending_consolidations: Option<QueueDiff>,
}

/// Creates a [`BeaconStateDelta`] describing the transition between two
/// beacon states.
///
/// [`DiffSource`] supplies both the base and target components. Each component
/// is passed to the specialized encoder best suited to its update semantics.
///
/// The resulting delta contains enough information to reconstruct the target
/// state when applied to the corresponding base state with [`apply`].
///
/// # Fork handling
///
/// The fork returned by [`DiffSource::fork`] is stored in the delta. Fork-specific
/// components are populated according to that fork.
///
/// The function performs debug-only invariant checks to ensure that
/// fork-specific fields are present exactly when expected.
///
/// # Complexity
///
/// O(n) in the amount of state data examined by the individual diff algorithms.
/// No single representation is imposed on all state components; the actual
/// amount of work depends on the component sizes and their specialized encoding
/// strategies.
///
/// # Examples
///
/// A consensus client typically implements [`DiffSource`] for a wrapper around
/// its state representation and then calls:
///
/// ```text
/// let delta = eth_state_diff::create(&source);
/// ```
///
/// The resulting [`BeaconStateDelta`] can then be serialized or archived for
/// later application.
pub fn create<R: DiffSource>(state: &R) -> BeaconStateDelta {
    let (base_slot, target_slot) = state.slot();

    let delta = BeaconStateDelta {
        fork: state.fork(),
        base_slot,
        scalar_header: state.scalar_header(),

        // Universal
        balances: balances::diff_balances_iter(state.balances().0, state.balances().1),
        validators: validators::diff_validators_iter(state.validators().0, state.validators().1),
        block_roots: recent_roots::diff_roots(base_slot, target_slot, state.block_roots()),
        state_roots: recent_roots::diff_roots(base_slot, target_slot, state.state_roots()),
        randao_mixes: randao_mixes::diff_randao(base_slot, target_slot, state.randao_mixes()),
        slashings: slashings::diff_slashings(
            base_slot,
            target_slot,
            state.slashings().0,
            state.slashings().1,
        ),
        eth1_data_votes: eth1_data_votes::diff_eth1_votes(
            state.eth1_data_votes().0,
            state.eth1_data_votes().1,
        ),
        historical_roots: state.historical_roots().map(|t| {
            historical_log::diff_historical_log(
                base_slot,
                target_slot,
                t,
                HISTORICAL_ROOTS_SSZ_SIZE,
                None,
            )
        }),

        // Phase0
        previous_epoch_attestations: state
            .previous_epoch_attestations()
            .map(|(b, t)| attestations::diff_attestations(b, t)),
        current_epoch_attestations: state
            .current_epoch_attestations()
            .map(|(b, t)| attestations::diff_attestations(b, t)),

        // Altair+
        previous_participation: state
            .previous_participation()
            .map(|(b, t)| participation::diff_participation_iter(b, t)),
        current_participation: state
            .current_participation()
            .map(|(b, t)| participation::diff_participation_iter(b, t)),
        inactivity_scores: state
            .inactivity_scores()
            .map(|(b, t)| inactivity_scores::diff_inactivity(b, t)),
        current_sync_committee: state
            .current_sync_committee()
            .map(|(b, t)| sync_committee::diff_sync_committee(b, t)),
        next_sync_committee: state
            .next_sync_committee()
            .map(|(b, t)| sync_committee::diff_sync_committee(b, t)),

        // Capella+
        historical_summaries: state.historical_summaries().map(|t| {
            historical_log::diff_historical_log(
                base_slot,
                target_slot,
                t,
                HISTORICAL_SUMMARIES_SSZ_SIZE,
                Some(state.capella_fork_slot()),
            )
        }),

        // Electra+
        pending_deposits: state
            .pending_deposits()
            .map(|(b, t)| pending_queue::diff_queue(b, t, PENDING_DEPOSIT_SSZ_SIZE)),
        pending_partial_withdrawals: state
            .pending_partial_withdrawals()
            .map(|(b, t)| pending_queue::diff_queue(b, t, PARTIAL_WITHDRAWAL_SSZ_SIZE)),
        pending_consolidations: state
            .pending_consolidations()
            .map(|(b, t)| pending_queue::diff_queue(b, t, PENDING_CONSOLIDATION_SSZ_SIZE)),
    };

    debug_assert_eq!(
        delta.previous_participation.is_some(),
        delta.fork >= ForkName::Altair,
        "DiffSource bug: previous_participation must exist iff fork >= Altair (got {:?})",
        delta.fork
    );

    debug_assert_eq!(
        delta.current_participation.is_some(),
        delta.fork >= ForkName::Altair,
        "DiffSource bug: current_participation must exist iff fork >= Altair (got {:?})",
        delta.fork
    );

    debug_assert_eq!(
        delta.inactivity_scores.is_some(),
        delta.fork >= ForkName::Altair,
        "DiffSource bug: inactivity_scores must exist iff fork >= Altair (got {:?})",
        delta.fork
    );

    debug_assert_eq!(
        delta.current_sync_committee.is_some(),
        delta.fork >= ForkName::Altair,
        "DiffSource bug: current_sync_committee must exist iff fork >= Altair (got {:?})",
        delta.fork
    );

    debug_assert_eq!(
        delta.next_sync_committee.is_some(),
        delta.fork >= ForkName::Altair,
        "DiffSource bug: next_sync_committee must exist iff fork >= Altair (got {:?})",
        delta.fork
    );

    debug_assert_eq!(
        delta.historical_summaries.is_some(),
        delta.fork >= ForkName::Capella,
        "DiffSource bug: historical_summaries must exist iff fork >= Capella (got {:?})",
        delta.fork
    );

    debug_assert_eq!(
        delta.pending_deposits.is_some(),
        delta.fork >= ForkName::Electra,
        "DiffSource bug: pending_deposits must exist iff fork >= Electra (got {:?})",
        delta.fork
    );

    debug_assert_eq!(
        delta.pending_partial_withdrawals.is_some(),
        delta.fork >= ForkName::Electra,
        "DiffSource bug: pending_partial_withdrawals must exist iff fork >= Electra (got {:?})",
        delta.fork
    );

    debug_assert_eq!(
        delta.pending_consolidations.is_some(),
        delta.fork >= ForkName::Electra,
        "DiffSource bug: pending_consolidations must exist iff fork >= Electra (got {:?})",
        delta.fork
    );

    delta
}

/// Applies an archived [`BeaconStateDelta`] to a mutable beacon state.
///
/// The destination state is modified in place and returned after all delta
/// components have been applied.
///
/// Before mutation begins, the function validates that:
///
/// - the destination state's fork matches the delta's fork;
/// - fork-specific fields are valid for that fork; and
/// - the fork value can be successfully decoded from the archived delta.
///
/// # Errors
///
/// Returns [`Error::ForkMismatch`] when the delta and destination state belong
/// to different forks.
///
/// Returns [`Error::InvalidFieldForFork`] when a fork-specific field is present
/// in a delta where that field is not valid.
///
/// Returns [`Error::MalformedDelta`] when the archived fork cannot be decoded.
///
/// # Mutation
///
/// Fork and field validation occurs before state components are modified.
/// Component application itself operates in place.
///
/// The destination state must correspond to the base state from which the
/// delta was created. Applying a valid delta to an unrelated state is not
/// expected to reconstruct the original target state.
///
/// # Complexity
///
/// Linear in the amount of data represented by the delta, with the exact cost
/// determined by the individual component encodings.
pub fn apply<M: DiffTarget>(mut state: M, delta: &ArchivedBeaconStateDelta) -> Result<M, Error> {
    use rkyv::deserialize;

    let delta_fork: ForkName = deserialize::<ForkName, rkyv::rancor::Error>(&delta.fork)
        .map_err(|e| Error::MalformedDelta(format!("failed to deserialize fork: {e}")))?;

    let state_fork = state.get_fork();
    if state_fork != delta_fork {
        return Err(Error::ForkMismatch {
            state_fork,
            delta_fork,
        });
    }

    macro_rules! validate_removed_field {
        ($field:ident, $removed_in:expr) => {
            if delta.$field.is_some() && delta_fork >= $removed_in {
                return Err(Error::InvalidFieldForFork {
                    field: stringify!($field),
                    fork: delta_fork,
                });
            }
        };
    }

    // Validate fork-specific fields.
    macro_rules! validate_field {
        ($field:ident, $fork:expr) => {
            if delta.$field.is_some() && delta_fork < $fork {
                return Err(Error::InvalidFieldForFork {
                    field: stringify!($field),
                    fork: delta_fork,
                });
            }
        };
    }

    // Introduced in Altair+
    validate_field!(previous_participation, ForkName::Altair);
    validate_field!(current_participation, ForkName::Altair);
    validate_field!(inactivity_scores, ForkName::Altair);
    validate_field!(current_sync_committee, ForkName::Altair);
    validate_field!(next_sync_committee, ForkName::Altair);

    // Introduced in Capella+
    validate_field!(historical_summaries, ForkName::Capella);

    // Introduced in Electra+
    validate_field!(pending_deposits, ForkName::Electra);
    validate_field!(pending_partial_withdrawals, ForkName::Electra);
    validate_field!(pending_consolidations, ForkName::Electra);

    // Removed in Altair
    validate_removed_field!(previous_epoch_attestations, ForkName::Altair);
    validate_removed_field!(current_epoch_attestations, ForkName::Altair);

    // Removed in Capella
    validate_removed_field!(historical_roots, ForkName::Capella);

    let base_slot = delta.base_slot.to_native();

    *state.scalar_header_mut() = delta.scalar_header.as_slice().to_vec();

    // Universal
    balances::apply_balances_iter(state.balances_mut(), &delta.balances);
    validators::apply_validators_iter(state.validators_mut(), &delta.validators);
    recent_roots::apply_roots(base_slot, state.block_roots_mut(), &delta.block_roots);
    recent_roots::apply_roots(base_slot, state.state_roots_mut(), &delta.state_roots);
    randao_mixes::apply_randao(base_slot, state.randao_mixes_mut(), &delta.randao_mixes);
    slashings::apply_slashings(state.slashings_mut(), &delta.slashings);
    eth1_data_votes::apply_eth1_votes(state.eth1_data_votes_mut(), &delta.eth1_data_votes);

    if let (Some(s), Some(d)) = (
        state.historical_roots_mut(),
        delta.historical_roots.as_ref(),
    ) {
        historical_log::apply_historical_log(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.previous_epoch_attestations_mut(),
        delta.previous_epoch_attestations.as_ref(),
    ) {
        attestations::apply_attestations(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.current_epoch_attestations_mut(),
        delta.current_epoch_attestations.as_ref(),
    ) {
        attestations::apply_attestations(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.previous_participation_mut(),
        delta.previous_participation.as_ref(),
    ) {
        participation::apply_participation_iter(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.current_participation_mut(),
        delta.current_participation.as_ref(),
    ) {
        participation::apply_participation_iter(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.inactivity_scores_mut(),
        delta.inactivity_scores.as_ref(),
    ) {
        inactivity_scores::apply_inactivity(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.current_sync_committee_mut(),
        delta.current_sync_committee.as_ref(),
    ) {
        sync_committee::apply_sync_committee(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.next_sync_committee_mut(),
        delta.next_sync_committee.as_ref(),
    ) {
        sync_committee::apply_sync_committee(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.historical_summaries_mut(),
        delta.historical_summaries.as_ref(),
    ) {
        historical_log::apply_historical_log(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.pending_deposits_mut(),
        delta.pending_deposits.as_ref(),
    ) {
        pending_queue::apply_queue(s, d, PENDING_DEPOSIT_SSZ_SIZE);
    }

    if let (Some(s), Some(d)) = (
        state.pending_partial_withdrawals_mut(),
        delta.pending_partial_withdrawals.as_ref(),
    ) {
        pending_queue::apply_queue(s, d, PARTIAL_WITHDRAWAL_SSZ_SIZE);
    }

    if let (Some(s), Some(d)) = (
        state.pending_consolidations_mut(),
        delta.pending_consolidations.as_ref(),
    ) {
        pending_queue::apply_queue(s, d, PENDING_CONSOLIDATION_SSZ_SIZE);
    }

    Ok(state)
}

/// A mutable target for list-like collections of copyable values.
///
/// [`ListMutTarget`] provides the minimal interface required by the generic
/// delta-application routines in this crate. It allows those routines to
/// update consensus-state collections without requiring the collection to be
/// backed by a contiguous `Vec`.
///
/// Implementations may use any underlying storage strategy, including
/// contiguous buffers, persistent trees, or other client-specific data
/// structures.
///
/// # Type parameter
///
/// `T` is the element type stored by the collection. It must implement
/// [`Copy`] because delta application reads values from the encoded delta and
/// writes them directly into the target collection.
///
/// # Required operations
///
/// An implementation must provide:
///
/// - [`len`](Self::len) to report the current number of elements.
/// - [`get_mut`](Self::get_mut) to obtain mutable access to an existing
///   element by index.
/// - [`push`](Self::push) to append a newly decoded element.
///
/// # Example
///
/// The crate provides an implementation for `Vec<u64>` and `Vec<u8>`.
///
/// ```
/// use eth_state_diff::ListMutTarget;
///
/// let mut values = vec![100u64, 200, 300];
/// let target: &mut dyn ListMutTarget<u64> = &mut values;
///
/// *target.get_mut(1).unwrap() = 250;
/// target.push(400);
///
/// assert_eq!(values, [100, 250, 300, 400]);
/// ```
///
/// # Implementing for client-specific collections
///
/// Consensus clients with non-contiguous or tree-backed state can implement
/// this trait to allow the generic delta algorithms to operate directly on
/// their native collections, without first materializing the collection as a
/// flat buffer.
///
/// Implementations should return `None` from [`get_mut`](Self::get_mut) when
/// the requested index is outside the current collection bounds.
pub trait ListMutTarget<T: Copy> {
    /// Returns the current number of elements in the collection.
    fn len(&self) -> usize;

    /// Returns `true` if the collection contains no elements.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns mutable access to the element at `index`.
    ///
    /// Returns `None` if `index` is outside the current collection bounds.
    fn get_mut(&mut self, index: usize) -> Option<&mut T>;

    /// Appends `value` to the end of the collection.
    fn push(&mut self, value: T);
}

impl ListMutTarget<u64> for Vec<u64> {
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn get_mut(&mut self, index: usize) -> Option<&mut u64> {
        self.as_mut_slice().get_mut(index)
    }

    #[inline]
    fn push(&mut self, value: u64) {
        self.push(value);
    }
}

impl ListMutTarget<u8> for Vec<u8> {
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn get_mut(&mut self, index: usize) -> Option<&mut u8> {
        self.as_mut_slice().get_mut(index)
    }

    #[inline]
    fn push(&mut self, value: u8) {
        self.push(value);
    }
}

const PENDING_DEPOSIT_SSZ_SIZE: usize = 192;
const PARTIAL_WITHDRAWAL_SSZ_SIZE: usize = 24;
const PENDING_CONSOLIDATION_SSZ_SIZE: usize = 16;
