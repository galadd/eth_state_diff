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

pub mod balances;
pub mod eth1_data_votes;
pub mod historical_summaries;
pub mod inactivity_scores;
pub mod participation;
pub mod pending_queue;
pub mod randao_mixes;
pub mod recent_roots;
pub mod slashings;
pub mod sync_committee;
pub mod types;
pub mod validators;

use rkyv::{Archive, Deserialize, Serialize};

use crate::types::{
    BalancesDiff, Eth1DataVotesDiff, HistoricalSummariesDiff, InactivityDiff, ParticipationDiff,
    QueueDiff, RandaoDiff, RootsDiff, SlashingsDiff, SyncCommitteeDiff, ValidatorsDiff,
};

/// Ethereum consensus fork supported by this delta.
///
/// Deltas may only be applied to states from the same fork to ensure layout
/// compatibility.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ForkName {
    Phase0,
    Altair,
    Bellatrix,
    Capella,
    Deneb,
    Electra,
    Fulu,
    Gloas,
    Heze,
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

    // --- Altair+ ---
    /// `None` for Phase0. `Some` for Altair+.
    pub previous_participation: Option<ParticipationDiff>,
    pub current_participation: Option<ParticipationDiff>,
    pub inactivity_scores: Option<InactivityDiff>,
    pub current_sync_committee: Option<SyncCommitteeDiff>,
    pub next_sync_committee: Option<SyncCommitteeDiff>,

    // --- Capella+ ---
    /// `None` for pre-Capella. `Some` for Capella+.
    pub historical_summaries: Option<HistoricalSummariesDiff>,

    // --- Electra+ ---
    /// `None` for pre-Electra. `Some` for Electra+.
    pub pending_deposits: Option<QueueDiff>,
    pub pending_partial_withdrawals: Option<QueueDiff>,
    pub pending_consolidations: Option<QueueDiff>,
}

const PENDING_DEPOSIT_SSZ_SIZE: usize = 192;
const PARTIAL_WITHDRAWAL_SSZ_SIZE: usize = 121;
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
///
/// # Panics
///
/// Panics if the delta was created for a different consensus fork.
///
/// # Complexity
///
/// Linear in the size of the recorded delta.
pub fn apply<M: DiffTarget>(mut state: M, delta: &ArchivedBeaconStateDelta) -> M {
    use rkyv::deserialize;

    let delta_fork: ForkName = deserialize::<ForkName, rkyv::rancor::Error>(&delta.fork)
        .expect("failed to deserialize fork");

    let state_fork = state.get_fork();
    assert_eq!(
        state_fork, delta_fork,
        "Fork mismatch: cannot apply {delta_fork:?} delta to {state_fork:?} state",
    );

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
        historical_summaries::apply_historical_summaries(s, d);
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

    state
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

    // Altair+
    fn previous_participation(&self) -> Option<(&[u8], &[u8])>;
    fn current_participation(&self) -> Option<(&[u8], &[u8])>;
    fn inactivity_scores(&self) -> Option<(&[u64], &[u64])>;
    fn current_sync_committee(&self) -> Option<(&[u8], &[u8])>;
    fn next_sync_committee(&self) -> Option<(&[u8], &[u8])>;

    // Capella+
    fn historical_summaries(&self) -> Option<(&[u8], &[u8])>;

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

    BeaconStateDelta {
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
        historical_summaries: state.historical_summaries().map(|(_, t)| {
            historical_summaries::diff_historical_summaries(
                base_slot,
                target_slot,
                t,
                state.capella_fork_slot(),
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
    }
}
