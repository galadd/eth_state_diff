//! High-performance delta encoding for Ethereum consensus state.
//!
//! `eth_state_diff` computes compact deltas between two beacon states and
//! efficiently reconstructs the target state by applying those deltas.
//!
//! The crate is designed for consensus clients, archival storage, state
//! synchronization, and historical state reconstruction.
//!
//! Individual state components use specialized encodings chosen for their
//! respective data structures, including sparse patches, circular buffer
//! updates, packed bit vectors, and FIFO queue deltas.
//!
//! Deltas are designed to serialize efficiently with `rkyv`, although the
//! library itself remains serialization-agnostic.

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

use crate::types::{
    AttestationsDiff, BalancesDiff, Eth1DataVotesDiff, HistoricalLogDiff, InactivityDiff,
    ParticipationDiff, QueueDiff, RandaoDiff, RootsDiff, SlashingsDiff, SyncCommitteeDiff,
    ValidatorsDiff, HISTORICAL_ROOTS_SSZ_SIZE, HISTORICAL_SUMMARIES_SSZ_SIZE,
};

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

/// Ethereum consensus fork supported by this delta.
///
/// Explicit integer discriminants are assigned to allow strict invariant
/// checking during delta creation.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ForkName {
    Phase0 = 0,
    Altair = 1,
    Bellatrix = 2,
    Capella = 3,
    Deneb = 4,
    Electra = 5,
    Fulu = 6,
    Gloas = 7,
    Heze = 8,
}

/// Complete delta describing the transition between two beacon states.
///
/// Fields introduced in later forks are wrapped in `Option<T>`.
/// A delta for Phase0 will have `None` for all Altair/Capella/Electra fields.
/// `rkyv` serializes `None` as a single zero byte, meaning fork-incompatible
/// fields add effectively zero size to the final compressed delta.
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

const PENDING_DEPOSIT_SSZ_SIZE: usize = 192;
const PARTIAL_WITHDRAWAL_SSZ_SIZE: usize = 24;
const PENDING_CONSOLIDATION_SSZ_SIZE: usize = 16;

/// Mutable view of a beacon state.
///
/// Implement this trait for your beacon-state representation to allow
/// [`apply`] to reconstruct a target state from a [`BeaconStateDelta`].
///
/// The trait intentionally operates on primitive buffers and slices rather
/// than client-specific types, allowing integration with any consensus client.
/// Return `None` for fields that do not exist in the state's current fork.
pub trait DiffTarget {
    fn get_fork(&self) -> ForkName;
    fn scalar_header_mut(&mut self) -> &mut Vec<u8>;

    // Universal
    fn balances_mut(&mut self) -> &mut Vec<u64>;
    fn validators_mut(&mut self) -> &mut Vec<u8>;
    fn block_roots_mut(&mut self) -> &mut [[u8; 32]];
    fn state_roots_mut(&mut self) -> &mut [[u8; 32]];
    fn randao_mixes_mut(&mut self) -> &mut [[u8; 32]];
    fn slashings_mut(&mut self) -> &mut [u64];
    fn eth1_data_votes_mut(&mut self) -> &mut Vec<u8>;
    fn historical_roots_mut(&mut self) -> Option<&mut Vec<u8>>;

    // Phase0 specific
    fn previous_epoch_attestations_mut(&mut self) -> Option<&mut Vec<u8>>;
    fn current_epoch_attestations_mut(&mut self) -> Option<&mut Vec<u8>>;

    // Altair+
    fn previous_participation_mut(&mut self) -> Option<&mut Vec<u8>>;
    fn current_participation_mut(&mut self) -> Option<&mut Vec<u8>>;
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

/// Applies a previously created beacon-state delta.
///
/// The supplied [`DiffTarget`] is modified in place by applying each component
/// delta to reconstruct the target state.
///
/// The state's fork must match the fork recorded in the delta.
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
    balances::apply_balances(state.balances_mut(), &delta.balances);
    validators::apply_validators(state.validators_mut(), &delta.validators);
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
        participation::apply_participation(s, d);
    }

    if let (Some(s), Some(d)) = (
        state.current_participation_mut(),
        delta.current_participation.as_ref(),
    ) {
        participation::apply_participation(s, d);
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

/// Read-only view of two beacon states.
///
/// Implement this trait to allow [`create`] to compute a
/// [`BeaconStateDelta`] between two states.
///
/// Each method exposes the state component required by the corresponding delta
/// encoder without imposing any storage layout on the implementation.
///
/// Return `None` for fields that do not exist in the state's current fork.
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
    fn balances(&self) -> (&[u64], &[u64]);
    fn validators(&self) -> (&[u8], &[u8]);
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
    fn previous_participation(&self) -> Option<(&[u8], &[u8])>;
    fn current_participation(&self) -> Option<(&[u8], &[u8])>;
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

/// Creates a delta between two beacon states.
///
/// The supplied [`DiffSource`] provides access to the base and target state
/// components required by each specialized encoder.
///
/// The returned [`BeaconStateDelta`] contains only the information necessary
/// to reconstruct the target state from the base state.
///
/// # Complexity
///
/// Linear in the size of the state components being compared.
pub fn create<R: DiffSource>(state: &R) -> BeaconStateDelta {
    let (base_slot, target_slot) = state.slot();

    let delta = BeaconStateDelta {
        fork: state.fork(),
        base_slot,
        scalar_header: state.scalar_header(),

        // Universal
        balances: balances::diff_balances(state.balances().0, state.balances().1),
        validators: validators::diff_validators(state.validators().0, state.validators().1),
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
            .map(|(b, t)| participation::diff_participation(b, t)),
        current_participation: state
            .current_participation()
            .map(|(b, t)| participation::diff_participation(b, t)),
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
