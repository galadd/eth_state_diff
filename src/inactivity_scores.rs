//! Delta encoding for validator inactivity scores.
//!
//! Inactivity scores are represented as a vector indexed by validator index.
//! This module encodes changes between two score vectors without storing the
//! complete target vector.
//!
//! Two representations are supported:
//!
//! - A sparse representation containing only changed indices and their target
//!   values.
//! - An all-zero representation for transitions to a completely zero-valued
//!   target vector.
//!
//! Newly added validator scores are stored separately as an extension.
//!
//! Deltas produced by [`diff_inactivity`] can be applied in place using
//! [`apply_inactivity`].

use crate::types::{ArchivedInactivityDiff, InactivityDiff};

/// Computes a compact delta between two validator inactivity-score vectors.
///
/// The returned delta contains sufficient information to reconstruct `target`
/// from `base`.
///
/// If the target contains only zero-valued scores and the base contains at
/// least one non-zero score, [`InactivityDiff::AllZeros`] is emitted. This
/// avoids storing individual updates when the entire target vector has been
/// cleared.
///
/// Otherwise, [`InactivityDiff::Sparse`] stores only the indices whose values
/// changed and their corresponding target values. Scores belonging to newly
/// appended validators are stored in the `extensions` field.
///
/// If `base` and `target` have different lengths, only their common prefix is
/// compared. Any remaining target scores are treated as newly appended
/// entries.
///
/// # Arguments
///
/// * `base` - Inactivity scores from the source state.
/// * `target` - Inactivity scores from the target state.
///
/// # Returns
///
/// A compact [`InactivityDiff`] representing the transition from `base` to
/// `target`.
///
/// # Complexity
///
/// O(n) time, where *n* is the length of the larger input vector.
///
/// Additional space is proportional to the number of changed scores plus the
/// number of newly appended scores.
///
/// # Example
///
/// ```
/// use eth_state_diff::inactivity::diff_inactivity;
/// use eth_state_diff::types::InactivityDiff;
///
/// let base = vec![10, 20, 30, 40];
/// let target = vec![10, 25, 30, 50];
///
/// let delta = diff_inactivity(&base, &target);
///
/// assert_eq!(
///     delta,
///     InactivityDiff::Sparse {
///         indices: vec![1, 3],
///         new_values: vec![25, 50],
///         extensions: vec![],
///     }
/// );
/// ```
///
/// A completely cleared vector can be represented without storing individual
/// zero values:
///
/// ```
/// use eth_state_diff::inactivity::diff_inactivity;
/// use eth_state_diff::types::InactivityDiff;
///
/// let base = vec![10, 20, 30];
/// let target = vec![0, 0, 0];
///
/// assert_eq!(
///     diff_inactivity(&base, &target),
///     InactivityDiff::AllZeros(3)
/// );
/// ```
pub fn diff_inactivity(base: &[u64], target: &[u64]) -> InactivityDiff {
    let target_is_zero = target.iter().all(|&v| v == 0);

    if target_is_zero {
        let base_has_non_zero = base.iter().any(|&v| v != 0);

        if base_has_non_zero {
            return InactivityDiff::AllZeros(target.len() as u32);
        }
    }

    let common_len = base.len().min(target.len());
    let mut indices = Vec::with_capacity(100);
    let mut new_values = Vec::with_capacity(100);

    for (i, (&v1, &v2)) in base.iter().zip(target.iter()).take(common_len).enumerate() {
        if v1 != v2 {
            indices.push(i as u32);
            new_values.push(v2);
        }
    }

    let extensions = target[common_len..].to_vec();

    InactivityDiff::Sparse {
        indices,
        new_values,
        extensions,
    }
}

/// Applies an inactivity-score delta to a vector in place.
///
/// After successful execution, `base` contains the inactivity-score vector
/// represented by `delta`.
///
/// [`ArchivedInactivityDiff::AllZeros`] clears the existing vector and
/// recreates it with the specified number of zero-valued scores.
///
/// [`ArchivedInactivityDiff::Sparse`] updates the recorded indices and then
/// appends any newly added validator scores.
///
/// # Panics
///
/// This function panics if a sparse delta contains an index that is outside
/// the current `base` vector.
///
/// A sparse delta is therefore expected to have been produced for a compatible
/// base state.
///
/// # Complexity
///
/// - [`ArchivedInactivityDiff::AllZeros`]: O(n), where *n* is the target
///   vector length.
/// - [`ArchivedInactivityDiff::Sparse`]: O(m + k), where *m* is the number of
///   recorded updates and *k* is the number of appended scores.
///
/// # Example
///
/// ```
/// use eth_state_diff::inactivity::{apply_inactivity, diff_inactivity};
/// use eth_state_diff::types::ArchivedInactivityDiff;
///
/// let mut base = vec![10, 20, 30];
/// let target = vec![10, 25, 30];
///
/// let delta = diff_inactivity(&base, &target);
///
/// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe {
///     rkyv::access_unchecked::<ArchivedInactivityDiff>(&bytes)
/// };
///
/// apply_inactivity(&mut base, archived);
///
/// assert_eq!(base, target);
/// ```
pub fn apply_inactivity(base: &mut Vec<u64>, delta: &ArchivedInactivityDiff) {
    match delta {
        ArchivedInactivityDiff::AllZeros(len) => {
            base.clear();
            base.resize(len.to_native() as usize, 0);
        }

        ArchivedInactivityDiff::Sparse {
            indices,
            new_values,
            extensions,
        } => {
            for (idx, val) in indices.iter().zip(new_values.iter()) {
                let i = (*idx).to_native() as usize;
                base[i] = val.to_native();
            }

            base.reserve(extensions.len());
            base.extend(extensions.iter().map(|v| v.to_native()));
        }
    }
}
