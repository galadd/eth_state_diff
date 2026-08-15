//! Delta encoding for the Eth1 data vote list.
//!
//! Eth1 data votes accumulate during an Eth1 voting period and are reset when
//! the voting period changes. This module represents those transitions using
//! either an append-only update or a complete replacement.
//!
//! The delta operates directly on the serialized SSZ representation and does
//! not deserialize individual Eth1 data values.

use crate::types::{ArchivedEth1DataVotesDiff, Eth1DataVotesDiff};

/// Computes a delta between two serialized Eth1 data vote lists.
///
/// The target length determines which representation is used:
///
/// - If `target` is at least as long as `base`, the bytes beyond the base
///   length are stored as [`Eth1DataVotesDiff::Append`].
/// - If `target` is shorter than `base`, the vote list is treated as having
///   been reset and the complete target list is stored in
///   [`Eth1DataVotesDiff::ResetAndAppend`].
///
/// This function operates directly on serialized SSZ bytes and does not
/// deserialize individual votes.
///
/// # Arguments
///
/// * `base` - Serialized SSZ representation of the current vote list.
/// * `target` - Serialized SSZ representation of the target vote list.
///
/// # Returns
///
/// A delta that represents the transition from `base` to `target` according
/// to the length-based append/reset semantics described above.
///
/// # Complexity
///
/// O(n), where *n* is the size of the target buffer, due to copying the
/// resulting delta payload.
///
/// # Example
///
/// ```
/// use eth_state_diff::eth1_data_votes::diff_eth1_votes;
/// use eth_state_diff::types::Eth1DataVotesDiff;
///
/// let base = b"AAAA";
/// let target = b"AAAABBBB";
///
/// let delta = diff_eth1_votes(base, target);
///
/// assert_eq!(delta, Eth1DataVotesDiff::Append(b"BBBB".to_vec()));
/// ```
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

/// Applies an Eth1 data vote delta to a serialized vote list in place.
///
/// [`Eth1DataVotesDiff::Append`] preserves the existing bytes and appends the
/// delta payload.
///
/// [`Eth1DataVotesDiff::ResetAndAppend`] clears the existing vote list before
/// writing the replacement payload.
///
/// The delta is assumed to have been produced for the current base state.
/// This function does not validate that an append delta's base bytes match
/// the state being modified.
///
/// # Arguments
///
/// * `base` - Serialized SSZ vote list to modify.
/// * `delta` - Archived delta to apply.
///
/// # Complexity
///
/// - [`Eth1DataVotesDiff::Append`]: O(k), where *k* is the number of appended
///   bytes.
/// - [`Eth1DataVotesDiff::ResetAndAppend`]: O(k), where *k* is the size of the
///   replacement payload.
///
/// # Example
///
/// ```
/// use eth_state_diff::eth1_data_votes::{apply_eth1_votes, diff_eth1_votes};
/// use eth_state_diff::types::ArchivedEth1DataVotesDiff;
///
/// let mut base = b"AAAA".to_vec();
/// let delta = diff_eth1_votes(&base, b"AAAABBBB");
///
/// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe {
///     rkyv::access_unchecked::<ArchivedEth1DataVotesDiff>(&bytes)
/// };
///
/// apply_eth1_votes(&mut base, archived);
///
/// assert_eq!(base, b"AAAABBBB");
/// ```
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
