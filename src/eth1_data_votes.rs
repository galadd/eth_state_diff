use crate::types::{ArchivedEth1DataVotesDiff, Eth1DataVotesDiff};

pub fn diff_eth1_votes(base: &[u8], target: &[u8]) -> Eth1DataVotesDiff {
    let base_len = base.len();
    let target_len = target.len();

    if target_len >= base_len {
        let new_votes_bytes = &target[base_len..];
        Eth1DataVotesDiff::Append(new_votes_bytes.to_vec())
    } else {
        Eth1DataVotesDiff::ResetAndAppend(target.to_vec())
    }
}

pub fn apply_eth1_votes(base: &mut Vec<u8>, delta: &ArchivedEth1DataVotesDiff) {
    match delta {
        ArchivedEth1DataVotesDiff::Append(new_votes) => {
            base.extend_from_slice(new_votes.as_slice());
        }
        ArchivedEth1DataVotesDiff::ResetAndAppend(new_votes) => {
            base.clear();
            base.extend_from_slice(new_votes.as_slice());
        }
    }
}
