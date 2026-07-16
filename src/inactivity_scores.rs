//! Delta encoding for validator inactivity scores.
//!
//! Inactivity scores are encoded as sparse updates. Only indices whose values
//! differ from the base vector are stored together with their replacement
//! values.
//!
//! When the target vector consists entirely of zero-valued scores, a dedicated
//! all-zero representation is emitted, avoiding any per-validator storage.
//!
//! Deltas produced by [`diff_inactivity`] can be applied in-place using
//! [`apply_inactivity`].

use crate::types::{ArchivedInactivityDiff, InactivityDiff};

/// Computes a compact inactivity-score delta.
///
/// The returned delta contains sufficient information to reconstruct `target`
/// from `base`.
///
/// If every inactivity score in the target vector is zero, the encoder emits an
/// [`InactivityDiff::AllZeros`] representation. Otherwise, only modified
/// indices and their replacement values are stored.
///
/// Newly appended validator scores are included as an extension.
///
/// # Arguments
///
/// * `base` - Base inactivity score vector.
/// * `target` - Target inactivity score vector.
///
/// # Complexity
///
/// O(n)
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

/// Applies an inactivity-score delta in-place.
///
/// After successful execution, `base` is identical to the target inactivity
/// score vector used to produce `delta`.
///
/// Newly appended inactivity scores are added to the end of the destination
/// vector.
///
/// # Complexity
///
/// O(number of recorded updates + appended scores)
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
