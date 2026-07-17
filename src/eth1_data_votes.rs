//! Delta encoding for the Eth1 data vote list.
//!
//! The vote list is monotonic within an Eth1 voting period but is reset when
//! the voting period changes. This module encodes both cases compactly:
//!
//! - Appending newly added votes to the existing list.
//! - Replacing the entire list after a reset.

use crate::types::{ArchivedEth1DataVotesDiff, Eth1DataVotesDiff};

/// Computes the delta between two serialized Eth1 data vote lists.
///
/// If the target list extends the base list, only the newly appended votes are
/// recorded.
///
/// If the target list is shorter than the base list, the encoder assumes the
/// vote list has been reset and stores the entire target list.
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

/// Applies an Eth1 data vote delta in place.
///
/// # Behavior
///
/// - [`Eth1DataVotesDiff::Append`] appends the recorded votes to the existing
///   vote list.
/// - [`Eth1DataVotesDiff::ResetAndAppend`] clears the destination list before
///   writing the recorded votes.
///
/// # Complexity
///
/// O(number of appended vote bytes)
pub fn apply_eth1_votes(base: &mut Vec<u8>, delta: &ArchivedEth1DataVotesDiff) {
    match delta {
        ArchivedEth1DataVotesDiff::Append(appended_votes) => {
            base.extend_from_slice(appended_votes.as_slice());
        }
        ArchivedEth1DataVotesDiff::ResetAndAppend(appended_votes) => {
            base.clear();
            base.extend_from_slice(appended_votes.as_slice());
        }
    }
}
