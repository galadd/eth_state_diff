//! Delta encoding for Phase 0 pending attestation lists.
//!
//! Phase 0 maintains two attestation lists with different update semantics:
//!
//! - `current_epoch_attestations` grows by appending new attestations during an
//!   epoch.
//! - `previous_epoch_attestations` is replaced when the epoch transitions.
//!
//! This module provides a single diff function that selects the most compact
//! supported representation for both cases. If the target list starts with the
//! exact byte sequence of the base list, only the appended bytes are stored.
//! Otherwise, the complete target list is stored as a full replacement.
//!
//! Both representations use [`AttestationsDiff`] and can be applied with
//! [`apply_attestations`] without deserializing individual attestations.

use crate::types::{ArchivedAttestationsDiff, AttestationsDiff};

/// Computes a compact delta between two serialized SSZ attestation lists.
///
/// The function automatically selects between the supported delta
/// representations:
///
/// - If the lists are identical, [`AttestationsDiff::Unchanged`] is returned.
/// - If the target list starts with the exact byte sequence of the base list,
///   only the trailing bytes are stored in [`AttestationsDiff::Append`].
/// - Otherwise, the complete target list is stored in
///   [`AttestationsDiff::FullReplacement`].
///
/// This covers both append-only updates, such as growth of
/// `current_epoch_attestations`, and epoch-boundary replacement of
/// `previous_epoch_attestations`.
///
/// An empty base list is naturally represented as an append of the complete
/// target list.
///
/// # Arguments
///
/// * `base_ssz` - Serialized SSZ representation of the base attestation list.
/// * `target_ssz` - Serialized SSZ representation of the target attestation list.
///
/// # Returns
///
/// A compact [`AttestationsDiff`] representing the transition from `base_ssz`
/// to `target_ssz`.
///
/// # Complexity
///
/// O(n) time, where *n* is the length of `base_ssz`, for the prefix comparison.
/// Additional space is proportional to the selected delta payload.
///
/// # Example
///
/// ```
/// # use eth_state_diff::attestations::diff_attestations;
/// # use eth_state_diff::types::AttestationsDiff;
///
/// let base = b"AAAA";
///
/// // Append case.
/// let target = b"AAAABBBB";
/// assert_eq!(
///     diff_attestations(base, target),
///     AttestationsDiff::Append(b"BBBB".to_vec())
/// );
///
/// // Replacement case.
/// let target = b"CCCC";
/// assert_eq!(
///     diff_attestations(base, target),
///     AttestationsDiff::FullReplacement(b"CCCC".to_vec())
/// );
/// ```
pub fn diff_attestations(base_ssz: &[u8], target_ssz: &[u8]) -> AttestationsDiff {
    if base_ssz == target_ssz {
        return AttestationsDiff::Unchanged;
    }

    if target_ssz.starts_with(base_ssz) {
        AttestationsDiff::Append(target_ssz[base_ssz.len()..].to_vec())
    } else {
        AttestationsDiff::FullReplacement(target_ssz.to_vec())
    }
}

/// Applies an attestation delta to a serialized SSZ list in place.
///
/// [`AttestationsDiff::Unchanged`] leaves the base buffer untouched.
///
/// [`AttestationsDiff::Append`] appends the serialized bytes stored in the
/// delta to the existing buffer.
///
/// [`AttestationsDiff::FullReplacement`] clears the existing buffer and
/// replaces it with the serialized target bytes.
///
/// # Complexity
///
/// - [`AttestationsDiff::Unchanged`]: O(1).
/// - [`AttestationsDiff::Append`]: O(k), where *k* is the number of appended bytes.
/// - [`AttestationsDiff::FullReplacement`]: O(n), where *n* is the size of the replacement list.
///
/// # Example
///
/// ```
/// # use eth_state_diff::attestations::{apply_attestations, diff_attestations};
/// # use eth_state_diff::types::{ArchivedAttestationsDiff, AttestationsDiff};
///
/// let mut base = b"AAAA".to_vec();
/// let delta = diff_attestations(&base, b"AAAABBBB");
///
/// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe { rkyv::access_unchecked::<ArchivedAttestationsDiff>(&bytes) };
///
/// apply_attestations(&mut base, archived);
///
/// assert_eq!(base, b"AAAABBBB");
/// ```
pub fn apply_attestations(base: &mut Vec<u8>, delta: &ArchivedAttestationsDiff) {
    match delta {
        ArchivedAttestationsDiff::Unchanged => {}
        ArchivedAttestationsDiff::Append(bytes) => {
            base.extend_from_slice(bytes.as_slice());
        }
        ArchivedAttestationsDiff::FullReplacement(bytes) => {
            base.clear();
            base.extend_from_slice(bytes.as_slice());
        }
    }
}
