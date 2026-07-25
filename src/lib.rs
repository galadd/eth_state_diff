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
}

/// Complete delta describing the transition between two beacon states.
///
/// Each field uses a specialized encoding optimized for the corresponding
/// consensus data structure.
///
/// A `BeaconStateDelta` can be serialized, persisted, transmitted, and later
/// applied to a compatible base state to reconstruct the target state.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BeaconStateDelta {
    pub fork: ForkName,
    pub base_slot: u64,
    pub scalar_header: Vec<u8>,

    pub balances: BalancesDiff,
    pub previous_participation: ParticipationDiff,
    pub current_participation: ParticipationDiff,
    pub validators: ValidatorsDiff,
    pub block_roots: RootsDiff,
    pub state_roots: RootsDiff,
    pub randao_mixes: RandaoDiff,
    pub slashings: SlashingsDiff,
    pub inactivity_scores: InactivityDiff,

    pub eth1_data_votes: Eth1DataVotesDiff,

    pub current_sync_committee: SyncCommitteeDiff,
    pub next_sync_committee: SyncCommitteeDiff,

    pub historical_summaries: HistoricalSummariesDiff,

    pub pending_deposits: QueueDiff,
    pub pending_partial_withdrawals: QueueDiff,
    pub pending_consolidations: QueueDiff,
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
pub trait DiffTarget {
    fn get_fork(&self) -> ForkName;
    fn scalar_header_mut(&mut self) -> &mut Vec<u8>;

    fn balances_mut(&mut self) -> &mut Vec<u64>;
    fn previous_participation_mut(&mut self) -> &mut Vec<u8>;
    fn current_participation_mut(&mut self) -> &mut Vec<u8>;
    fn validators_mut(&mut self) -> &mut Vec<u8>;
    fn block_roots_mut(&mut self) -> &mut [[u8; 32]];
    fn state_roots_mut(&mut self) -> &mut [[u8; 32]];
    fn randao_mixes_mut(&mut self) -> &mut [[u8; 32]];
    fn slashings_mut(&mut self) -> &mut [u64];
    fn inactivity_scores_mut(&mut self) -> &mut Vec<u64>;

    fn eth1_data_votes_mut(&mut self) -> &mut Vec<u8>;

    fn current_sync_committee_mut(&mut self) -> &mut Vec<u8>;
    fn next_sync_committee_mut(&mut self) -> &mut Vec<u8>;

    fn historical_summaries_mut(&mut self) -> &mut Vec<u8>;

    fn pending_deposits_mut(&mut self) -> &mut Vec<u8>;
    fn pending_partial_withdrawals_mut(&mut self) -> &mut Vec<u8>;
    fn pending_consolidations_mut(&mut self) -> &mut Vec<u8>;
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

    balances::apply_balances(state.balances_mut(), &delta.balances);
    participation::apply_participation(
        state.previous_participation_mut(),
        &delta.previous_participation,
    );
    participation::apply_participation(
        state.current_participation_mut(),
        &delta.current_participation,
    );
    validators::apply_validators(state.validators_mut(), &delta.validators);

    recent_roots::apply_roots(base_slot, state.block_roots_mut(), &delta.block_roots);
    recent_roots::apply_roots(base_slot, state.state_roots_mut(), &delta.state_roots);
    randao_mixes::apply_randao(base_slot, state.randao_mixes_mut(), &delta.randao_mixes);
    slashings::apply_slashings(state.slashings_mut(), &delta.slashings);
    inactivity_scores::apply_inactivity(state.inactivity_scores_mut(), &delta.inactivity_scores);

    eth1_data_votes::apply_eth1_votes(state.eth1_data_votes_mut(), &delta.eth1_data_votes);

    sync_committee::apply_sync_committee(
        state.current_sync_committee_mut(),
        &delta.current_sync_committee,
    );
    sync_committee::apply_sync_committee(
        state.next_sync_committee_mut(),
        &delta.next_sync_committee,
    );

    historical_summaries::apply_historical_summaries(
        state.historical_summaries_mut(),
        &delta.historical_summaries,
    );

    pending_queue::apply_queue(
        state.pending_deposits_mut(),
        &delta.pending_deposits,
        PENDING_DEPOSIT_SSZ_SIZE,
    );
    pending_queue::apply_queue(
        state.pending_partial_withdrawals_mut(),
        &delta.pending_partial_withdrawals,
        PARTIAL_WITHDRAWAL_SSZ_SIZE,
    );
    pending_queue::apply_queue(
        state.pending_consolidations_mut(),
        &delta.pending_consolidations,
        PENDING_CONSOLIDATION_SSZ_SIZE,
    );

    state
}

/// Read-only view of two beacon states.
///
/// Implement this trait to allow [`create`] to compute a
/// [`BeaconStateDelta`] between two states.
///
/// Each method exposes the state component required by the corresponding delta
/// encoder without imposing any storage layout on the implementation.
pub trait DiffSource {
    fn fork(&self) -> ForkName;
    fn slot(&self) -> (u64, u64);
    fn scalar_header(&self) -> Vec<u8>;

    fn balances(&self) -> (&[u64], &[u64]);
    fn previous_participation(&self) -> (&[u8], &[u8]);
    fn current_participation(&self) -> (&[u8], &[u8]);
    fn validators(&self) -> (&[u8], &[u8]);

    fn block_roots(&self) -> &[[u8; 32]];
    fn state_roots(&self) -> &[[u8; 32]];
    fn randao_mixes(&self) -> &[[u8; 32]];
    fn slashings(&self) -> (&[u64], &[u64]);
    fn inactivity_scores(&self) -> (&[u64], &[u64]);

    fn eth1_data_votes(&self) -> (&[u8], &[u8]);

    fn current_sync_committee(&self) -> (&[u8], &[u8]);
    fn next_sync_committee(&self) -> (&[u8], &[u8]);

    fn capella_fork_slot(&self) -> u64;
    fn historical_summaries(&self) -> (&[u8], &[u8]);

    fn pending_deposits(&self) -> (&[u8], &[u8]);
    fn pending_partial_withdrawals(&self) -> (&[u8], &[u8]);
    fn pending_consolidations(&self) -> (&[u8], &[u8]);
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
    let (base_balances, target_balances) = state.balances();
    let (base_prev_participation, target_prev_participation) = state.previous_participation();
    let (base_curr_participation, target_curr_participation) = state.current_participation();
    let (base_validators, target_validators) = state.validators();
    let (base_inactivity, target_inactivity) = state.inactivity_scores();
    let (base_slashings, target_slashings) = state.slashings();
    let (base_eth1_data_votes, target_eth1_data_votes) = state.eth1_data_votes();
    let (base_curr_sync, target_curr_sync) = state.current_sync_committee();
    let (base_next_sync, target_next_sync) = state.next_sync_committee();
    let (_, target_historical_summaries) = state.historical_summaries();
    let (base_pending_deposits, target_pending_deposits) = state.pending_deposits();
    let (base_pending_pw, target_pending_pw) = state.pending_partial_withdrawals();
    let (base_pending_consolidations, target_pending_consolidations) =
        state.pending_consolidations();

    let (base_slot, target_slot) = state.slot();

    BeaconStateDelta {
        fork: state.fork(),
        base_slot,
        scalar_header: state.scalar_header(),

        balances: balances::diff_balances(base_balances, target_balances),
        previous_participation: participation::diff_participation(
            base_prev_participation,
            target_prev_participation,
        ),
        current_participation: participation::diff_participation(
            base_curr_participation,
            target_curr_participation,
        ),
        validators: validators::diff_validators(base_validators, target_validators),

        block_roots: recent_roots::diff_roots(base_slot, target_slot, state.block_roots()),
        state_roots: recent_roots::diff_roots(base_slot, target_slot, state.state_roots()),
        randao_mixes: randao_mixes::diff_randao(base_slot, target_slot, state.randao_mixes()),
        slashings: slashings::diff_slashings(
            base_slot,
            target_slot,
            base_slashings,
            target_slashings,
        ),
        inactivity_scores: inactivity_scores::diff_inactivity(base_inactivity, target_inactivity),

        eth1_data_votes: eth1_data_votes::diff_eth1_votes(
            base_eth1_data_votes,
            target_eth1_data_votes,
        ),

        current_sync_committee: sync_committee::diff_sync_committee(
            base_curr_sync,
            target_curr_sync,
        ),
        next_sync_committee: sync_committee::diff_sync_committee(base_next_sync, target_next_sync),

        historical_summaries: historical_summaries::diff_historical_summaries(
            base_slot,
            target_slot,
            target_historical_summaries,
            state.capella_fork_slot(),
        ),

        pending_deposits: pending_queue::diff_queue(
            base_pending_deposits,
            target_pending_deposits,
            PENDING_DEPOSIT_SSZ_SIZE,
        ),
        pending_partial_withdrawals: pending_queue::diff_queue(
            base_pending_pw,
            target_pending_pw,
            PARTIAL_WITHDRAWAL_SSZ_SIZE,
        ),
        pending_consolidations: pending_queue::diff_queue(
            base_pending_consolidations,
            target_pending_consolidations,
            PENDING_CONSOLIDATION_SSZ_SIZE,
        ),
    }
}
