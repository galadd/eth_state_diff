//! Delta encoding for Ethereum sync committees.
//!
//! Sync committees remain unchanged for an entire sync committee period and
//! are replaced when a new period begins. This makes equality-based encoding
//! sufficient for the normal state-diff window: unchanged committees require
//! no payload, while a committee transition stores the complete target
//! committee.
//!
//! The module operates on the serialized SSZ representation of a
//! `SyncCommittee`. It does not inspect individual committee members or SSZ
//! fields; the serialized bytes are treated as an opaque payload.
//!
//! # Encoding strategy
//!
//! Two representations are possible:
//!
//! - [`SyncCommitteeDiff::Unchanged`] when the serialized base and target
//!   committees are identical.
//! - [`SyncCommitteeDiff::FullReplacement`] when the committee has changed.
//!
//! The replacement representation stores the complete serialized target
//! committee. This is intentional: unlike validator balances or participation
//! flags, there is no useful sparse update representation for a committee in
//! this module.
//!
//! # State transition
//!
//! Applying a delta to the same base committee used during diff generation
//! reconstructs the target bytes:
//!
//! ```text
//! base committee + delta -> target committee
//! ```
//!
//! The delta does not depend on the committee period number. The caller is
//! responsible for determining which consensus-state field is being diffed.
//!
//! # SSZ representation
//!
//! The functions accept the raw serialized bytes of the `SyncCommittee`
//! container. The bytes are treated as opaque data and are copied without
//! decoding or re-encoding individual committee members.
//!
//! `SyncCommittee` is a fixed-size consensus container. Callers should
//! therefore normally provide the serialized container bytes directly rather
//! than adding a list-length prefix.
//!
//! # Complexity
//!
//! Let *n* be the serialized size of the committee:
//!
//! - [`diff_sync_committee`] performs an O(n) byte comparison.
//! - [`apply_sync_committee`] is O(1) for [`SyncCommitteeDiff::Unchanged`].
//! - [`apply_sync_committee`] is O(n) for
//!   [`SyncCommitteeDiff::FullReplacement`].
//!
//! Additional memory is O(n) when a changed committee is encoded, because the
//! target bytes are copied into the replacement payload.
//!
//! # Example
//!
//! ```
//! use eth_state_diff::sync_committee::diff_sync_committee;
//! use eth_state_diff::types::SyncCommitteeDiff;
//!
//! let base = b"committee-a";
//! let target = b"committee-a";
//!
//! let delta = diff_sync_committee(base, target);
//!
//! assert_eq!(delta, SyncCommitteeDiff::Unchanged);
//! ```
//!
//! A committee transition produces a full replacement:
//!
//! ```
//! use eth_state_diff::sync_committee::diff_sync_committee;
//! use eth_state_diff::types::SyncCommitteeDiff;
//!
//! let base = b"committee-a";
//! let target = b"committee-b";
//!
//! let delta = diff_sync_committee(base, target);
//!
//! assert_eq!(
//!     delta,
//!     SyncCommitteeDiff::FullReplacement(target.to_vec())
//! );
//! ```

use crate::types::{ArchivedSyncCommitteeDiff, SyncCommitteeDiff};

/// Computes a delta between two serialized Ethereum sync committees.
///
/// The serialized committee bytes are compared as opaque byte sequences.
///
/// If `base_ssz` and `target_ssz` are identical, this function returns
/// [`SyncCommitteeDiff::Unchanged`] and stores no committee bytes.
///
/// If they differ, this function returns
/// [`SyncCommitteeDiff::FullReplacement`] containing a copy of `target_ssz`.
/// The target committee is therefore stored in its entirety rather than
/// attempting to encode individual member changes.
///
/// # Arguments
///
/// * `base_ssz` - Serialized SSZ bytes of the committee in the base state.
/// * `target_ssz` - Serialized SSZ bytes of the committee in the target state.
///
/// Both arguments must represent the same consensus-state field and use the
/// same serialization format.
///
/// # Returns
///
/// A [`SyncCommitteeDiff`] that can be applied to `base_ssz` to reconstruct
/// `target_ssz`.
///
/// # Complexity
///
/// O(n) time, where *n* is the length of the serialized committee.
///
/// If the committees differ, O(n) additional space is required for the copied
/// replacement payload.
///
/// # Example
///
/// ```
/// use eth_state_diff::sync_committee::diff_sync_committee;
/// use eth_state_diff::types::SyncCommitteeDiff;
///
/// let base = b"committee-a";
/// let target = b"committee-b";
///
/// let delta = diff_sync_committee(base, target);
///
/// assert_eq!(
///     delta,
///     SyncCommitteeDiff::FullReplacement(target.to_vec())
/// );
/// ```
pub fn diff_sync_committee(base_ssz: &[u8], target_ssz: &[u8]) -> SyncCommitteeDiff {
    if base_ssz == target_ssz {
        SyncCommitteeDiff::Unchanged
    } else {
        SyncCommitteeDiff::FullReplacement(target_ssz.to_vec())
    }
}

/// Applies a sync committee delta to a serialized SSZ committee in place.
///
/// [`SyncCommitteeDiff::Unchanged`] leaves `base` untouched.
///
/// [`SyncCommitteeDiff::FullReplacement`] clears `base` and replaces it with
/// the serialized committee stored in the delta.
///
/// After successful execution, `base` contains the serialized target committee
/// represented by `delta`.
///
/// # Arguments
///
/// * `base` - Serialized SSZ bytes of the base committee. This buffer is
///   modified in place.
/// * `delta` - Archived sync committee delta to apply.
///
/// # Complexity
///
/// - [`SyncCommitteeDiff::Unchanged`]: O(1) time and O(1) additional space.
/// - [`SyncCommitteeDiff::FullReplacement`]: O(n) time, where *n* is the
///   replacement size.
///
/// The replacement case may allocate when `base` does not have sufficient
/// capacity for the target committee.
///
/// # Example
///
/// ```
/// use eth_state_diff::sync_committee::diff_sync_committee;
/// use eth_state_diff::sync_committee::apply_sync_committee;
/// use eth_state_diff::types::ArchivedSyncCommitteeDiff;
///
/// let mut base = b"committee-a".to_vec();
/// let target = b"committee-b";
///
/// let delta = diff_sync_committee(&base, target);
///
/// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe {
///     rkyv::access_unchecked::<ArchivedSyncCommitteeDiff>(&bytes)
/// };
///
/// apply_sync_committee(&mut base, archived);
///
/// assert_eq!(base, target);
/// ```
pub fn apply_sync_committee(base: &mut Vec<u8>, delta: &ArchivedSyncCommitteeDiff) {
    match delta {
        ArchivedSyncCommitteeDiff::Unchanged => {}
        ArchivedSyncCommitteeDiff::FullReplacement(replacement) => {
            base.clear();
            base.extend_from_slice(replacement.as_slice());
        }
    }
}
