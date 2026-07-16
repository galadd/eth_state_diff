pub mod balances;
pub mod eth1_data_votes;
pub mod fifo_queue;
pub mod inactivity_scores;
pub mod participation;
pub mod randao_mixes;
pub mod recent_roots;
pub mod slashings;
pub mod types;
pub mod validators;

use rkyv::{Archive, Deserialize, Serialize};

use crate::types::{
    BalanceDiff, FifoQueueDiff, InactivityDiff, ParticipationDiff, RandaoDiff, RootsDiff,
    SlashingsDiff, ValidatorDiff,
};

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

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BeaconStateDelta {
    pub fork: ForkName,
    pub base_slot: u64,
    pub scalar_header: Vec<u8>,

    pub balances: BalanceDiff,
    pub previous_participation: ParticipationDiff,
    pub validators: ValidatorDiff,
    pub block_roots: RootsDiff,
    pub state_roots: RootsDiff,
    pub randao_mixes: RandaoDiff,
    pub slashings: SlashingsDiff,
    pub inactivity_scores: InactivityDiff,

    pub eth1_data_votes: Vec<u8>,

    pub pending_deposits: FifoQueueDiff,
    pub pending_partial_withdrawals: FifoQueueDiff,
    pub pending_consolidations: FifoQueueDiff,
}

pub trait DiffTarget {
    fn get_fork(&self) -> ForkName;
    fn scalar_header_mut(&mut self) -> &mut Vec<u8>;

    fn balances_mut(&mut self) -> &mut Vec<u64>;
    fn previous_participation_mut(&mut self) -> &mut Vec<u8>;
    fn validators_mut(&mut self) -> &mut Vec<u8>;
    fn block_roots_mut(&mut self) -> &mut [[u8; 32]];
    fn state_roots_mut(&mut self) -> &mut [[u8; 32]];
    fn randao_mixes_mut(&mut self) -> &mut [[u8; 32]];
    fn slashings_mut(&mut self) -> &mut [u64];
    fn inactivity_scores_mut(&mut self) -> &mut Vec<u64>;

    fn eth1_data_votes_mut(&mut self) -> &mut Vec<u8>;

    fn pending_deposits_mut(&mut self) -> &mut Vec<u8>;
    fn pending_partial_withdrawals_mut(&mut self) -> &mut Vec<u8>;
    fn pending_consolidations_mut(&mut self) -> &mut Vec<u8>;
}

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
    let slots_per_epoch: u64 = 32;

    *state.scalar_header_mut() = delta.scalar_header.as_slice().to_vec();

    balances::apply_balances(state.balances_mut(), &delta.balances);
    participation::apply_participation(
        state.previous_participation_mut(),
        &delta.previous_participation,
    );
    validators::apply_validators(state.validators_mut(), &delta.validators);

    recent_roots::apply_roots(base_slot, state.block_roots_mut(), &delta.block_roots);
    recent_roots::apply_roots(base_slot, state.state_roots_mut(), &delta.state_roots);
    randao_mixes::apply_randao(
        base_slot,
        state.randao_mixes_mut(),
        &delta.randao_mixes,
        slots_per_epoch,
    );
    slashings::apply_slashings(state.slashings_mut(), &delta.slashings);
    inactivity_scores::apply_inactivity(state.inactivity_scores_mut(), &delta.inactivity_scores);

    *state.eth1_data_votes_mut() = delta.eth1_data_votes.as_slice().to_vec();

    fifo_queue::apply_fifo_queue(state.pending_deposits_mut(), &delta.pending_deposits, 88);
    fifo_queue::apply_fifo_queue(
        state.pending_partial_withdrawals_mut(),
        &delta.pending_partial_withdrawals,
        121,
    );
    fifo_queue::apply_fifo_queue(
        state.pending_consolidations_mut(),
        &delta.pending_consolidations,
        169,
    );

    state
}

pub trait DiffSource {
    fn fork(&self) -> ForkName;
    fn slot(&self) -> (u64, u64);
    fn scalar_header(&self) -> Vec<u8>;

    fn balances(&self) -> (&[u64], &[u64]);
    fn previous_participation(&self) -> (&[u8], &[u8]);
    fn validators(&self) -> (&[u8], &[u8]);

    fn block_roots(&self) -> &[[u8; 32]];
    fn state_roots(&self) -> &[[u8; 32]];
    fn randao_mixes(&self) -> &[[u8; 32]];
    fn slashings(&self) -> (&[u64], &[u64]);
    fn inactivity_scores(&self) -> (&[u64], &[u64]);

    fn eth1_data_votes(&self) -> Vec<u8>;

    fn pending_deposits(&self) -> FifoQueueDiff;
    fn pending_partial_withdrawals(&self) -> FifoQueueDiff;
    fn pending_consolidations(&self) -> FifoQueueDiff;
}

pub fn create<R: DiffSource>(state: &R) -> BeaconStateDelta {
    let (base_balances, target_balances) = state.balances();
    let (base_prev_participation, target_prev_participation) = state.previous_participation();
    let (base_validators, target_validators) = state.validators();
    let (base_inactivity, target_inactivity) = state.inactivity_scores();
    let (base_slashings, target_slashings) = state.slashings();

    let (base_slot, target_slot) = state.slot();
    let slots_per_epoch: u64 = 32;

    BeaconStateDelta {
        fork: state.fork(),
        base_slot,
        scalar_header: state.scalar_header(),

        balances: balances::diff_balances(base_balances, target_balances),
        previous_participation: participation::diff_participation(
            base_prev_participation,
            target_prev_participation,
        ),
        validators: validators::diff_validators(base_validators, target_validators),

        block_roots: recent_roots::diff_roots(base_slot, target_slot, state.block_roots()),
        state_roots: recent_roots::diff_roots(
            base_slot,
            base_slot + slots_per_epoch,
            state.state_roots(),
        ),
        randao_mixes: randao_mixes::diff_randao(
            base_slot,
            base_slot + slots_per_epoch,
            state.randao_mixes(),
            slots_per_epoch,
        ),
        slashings: slashings::diff_slashings(
            base_slot,
            base_slot + slots_per_epoch,
            base_slashings,
            target_slashings,
            slots_per_epoch,
        ),
        inactivity_scores: inactivity_scores::diff_inactivity(base_inactivity, target_inactivity),

        eth1_data_votes: state.eth1_data_votes(),

        pending_deposits: state.pending_deposits(),
        pending_partial_withdrawals: state.pending_partial_withdrawals(),
        pending_consolidations: state.pending_consolidations(),
    }
}
