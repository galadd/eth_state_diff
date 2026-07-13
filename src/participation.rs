//! Delta encoding for validator participation flags.
//!
//! This module provides a compact sparse encoding for participation flag
//! vectors used in Ethereum consensus state.
//!
//! Rather than storing the entire target vector, only modified indices and
//! their replacement values are encoded. Modified indices are represented as
//! delta-varint encoded gaps, allowing sparse updates to compress efficiently.
//!
//! Deltas produced by [`diff_participation`] can be applied in-place using
//! [`apply_participation`].

use crate::{
    balances::{read_varint, write_varint},
    types::{ArchivedParticipationDiff, ParticipationDiff},
};

/// Computes a compact participation delta.
///
/// The returned delta contains sufficient information to reconstruct `target`
/// from `base`.
pub fn diff_participation(base: &[u8], target: &[u8]) -> ParticipationDiff {
    let target_len = target.len();

    if target.iter().all(|&v| v == 0) {
        return ParticipationDiff::AllZeros(target_len);
    }

    let common_len = base.len().min(target_len);
    let mut sparse_indices = Vec::with_capacity(50_000);
    let mut new_values = Vec::with_capacity(50_000);

    let mut last_idx = 0u64;

    for i in 0..common_len {
        if base[i] != target[i] {
            // Calculate and write the gap from the last changed index
            let gap = (i - last_idx as usize) as u64;
            write_varint(gap, &mut sparse_indices);

            new_values.push(target[i]);
            last_idx = i as u64;
        }
    }

    let extension = if target_len > base.len() {
        target[base.len()..].to_vec()
    } else {
        Vec::new()
    };

    ParticipationDiff::Sparse {
        sparse_indices,
        new_values,
        extension,
    }
}

/// Applies a participation delta in-place.
///
/// After successful execution, `base` is identical to the target participation
/// vector used to produce `delta`.
///
/// This function performs a single linear pass over the encoded delta.
/// Additional storage is allocated only when the reconstructed vector grows
/// beyond its current length.
///
/// # Complexity
///
/// O(m)
///
/// where `m` is the number of encoded modifications plus any appended values.
pub fn apply_participation(base: &mut Vec<u8>, delta: &ArchivedParticipationDiff) {
    match delta {
        ArchivedParticipationDiff::AllZeros(len) => {
            base.clear();
            base.resize(len.to_native() as usize, 0);
        }
        ArchivedParticipationDiff::Sparse {
            sparse_indices,
            new_values,
            extension,
        } => {
            let indices_raw = sparse_indices.as_slice();
            let values_iter = new_values.iter();

            let mut cursor = 0usize;
            let mut current_idx = 0usize;

            for val in values_iter {
                // Decode the gap
                let gap = read_varint(indices_raw, &mut cursor) as usize;
                current_idx += gap;

                if current_idx >= base.len() {
                    base.resize(current_idx + 1, 0);
                }

                base[current_idx] = *val;
            }

            base.extend(extension.iter().copied());
        }
    }
}
