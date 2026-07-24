//! Delta encoding for Ethereum sync committees.
//!
//! Sync committees are static for the duration of a sync committee period
//! (256 epochs on Mainnet). Because a standard 32-epoch diff window almost never
//! crosses a period boundary, this module simply checks for equality and emits
//! a full replacement only when the committee actually changes.

use crate::types::{ArchivedSyncCommitteeDiff, SyncCommitteeDiff};

/// Computes the delta between two sync committees.
///
/// Expects the raw SSZ bytes of the `SyncCommittee` containers (including the
/// 4-byte SSZ length prefix if serialized as a list, though `SyncCommittee` is
/// typically serialized as a fixed-size struct).
pub fn diff_sync_committee(base_ssz: &[u8], target_ssz: &[u8]) -> SyncCommitteeDiff {
    if base_ssz == target_ssz {
        SyncCommitteeDiff::Unchanged
    } else {
        SyncCommitteeDiff::FullReplacement(target_ssz.to_vec())
    }
}

/// Applies a sync committee delta in place.
///
/// # Complexity
///
/// - `Unchanged`: O(1)
/// - `FullReplacement`: O(target committee size)
pub fn apply_sync_committee(base: &mut Vec<u8>, delta: &ArchivedSyncCommitteeDiff) {
    match delta {
        ArchivedSyncCommitteeDiff::Unchanged => {
            // Do nothing
        }
        ArchivedSyncCommitteeDiff::FullReplacement(replacement) => {
            base.clear();
            base.extend_from_slice(replacement.as_slice());
        }
    }
}
