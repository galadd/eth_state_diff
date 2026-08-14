//! Compact delta encoding for Ethereum epoch participation flags.
//!
//! This module computes and applies deltas between validator participation
//! vectors.
//!
//! Participation changes are typically sparse: during an epoch transition,
//! most validators retain their existing participation flags while only a
//! subset of validators receive new values. The delta therefore stores only
//! modified indices and their replacement values rather than the complete
//! target vector.
//!
//! ## Encoding
//!
//! The delta has two representations:
//!
//! - [`ParticipationDiff::AllZeros`] represents a target vector containing
//!   only zero-valued participation flags.
//! - [`ParticipationDiff::Sparse`] stores only changed entries.
//!
//! For the sparse representation, modified indices are encoded as
//! delta-varint gaps between successive modified indices. The corresponding
//! replacement values are stored separately in `new_values`. This avoids
//! storing unchanged participation flags and makes sequences of nearby
//! changes particularly compact.
//!
//! Validators present in the target vector but not in the base vector are
//! stored separately in `extension` and appended during application.
//!
//! ## APIs
//!
//! The module provides both slice-based and iterator-based APIs.
//!
//! [`diff_participation`] and [`apply_participation`] operate on contiguous
//! vectors and provide the specialized [`ParticipationDiff::AllZeros`] fast
//! path.
//!
//! [`diff_participation_iter`] and [`apply_participation_iter`] operate through
//! iterators and [`crate::ListMutTarget`], allowing consensus clients with
//! tree-backed or otherwise non-contiguous state representations to compute
//! and apply participation deltas without first materializing the complete
//! vector.
//!
//! The iterator-based diff API always produces the sparse representation.
//! The slice-based API can additionally detect an all-zero target and use the
//! more compact [`ParticipationDiff::AllZeros`] representation.
//!
//! ## Reconstruction
//!
//! Applying a sparse delta updates only the modified indices of the existing
//! collection and then appends any values in `extension`.
//!
//! Applying an [`ParticipationDiff::AllZeros`] delta replaces the destination
//! with a vector of the specified length containing only zero values.
//!
//! ## Complexity
//!
//! Diff generation is `O(n)` time and `O(m)` additional space, where `n` is
//! the number of participation flags examined and `m` is the number of
//! modified entries.
//!
//! Sparse delta application is `O(m + e)` time, where `m` is the number of
//! modified entries and `e` is the number of appended participation flags.
//!
//! The contiguous all-zero fast path performs `O(n)` work to initialize the
//! resulting vector.
//!
//! ## Serialization
//!
//! [`ParticipationDiff`] is designed to be serialized with `rkyv` and can be
//! subsequently compressed with a general-purpose compressor such as `zstd`.

use crate::{
    balances::{read_varint, write_varint},
    types::{ArchivedParticipationDiff, ParticipationDiff},
};

/// Computes a compact delta between two participation flag slices.
///
/// The returned [`ParticipationDiff`] contains the information required to
/// reconstruct `target` from `base`.
///
/// If every flag in `target` is zero, the function uses the specialized
/// [`ParticipationDiff::AllZeros`] representation. Otherwise it produces a
/// sparse delta containing only modified flags.
///
/// This is the contiguous-slice convenience API. Clients whose participation
/// flags are stored in a non-contiguous representation can use
/// [`diff_participation_iter`] instead.
///
/// # Complexity
///
/// `O(n)` time and `O(m)` additional space, where `n` is the number of flags
/// examined and `m` is the number of modified flags.
pub fn diff_participation(base: &[u8], target: &[u8]) -> ParticipationDiff {
    // Fast path for skip slots
    if target.iter().all(|&v| v == 0) {
        return ParticipationDiff::AllZeros(target.len());
    }

    diff_participation_iter(base.iter().copied(), target.iter().copied())
}

/// Applies a participation delta to a contiguous vector in place.
///
/// This is the contiguous-vector convenience API. Sparse deltas are delegated
/// to [`apply_participation_iter`], while [`ParticipationDiff::AllZeros`] is
/// handled directly by replacing the destination with a zero-filled vector of
/// the encoded length.
///
/// After successful application, `base` contains the target participation
/// vector from which `delta` was produced.
///
/// # Complexity
///
/// Sparse deltas require `O(m + e)` work, where `m` is the number of modified
/// entries and `e` is the number of appended flags.
///
/// An [`ParticipationDiff::AllZeros`] delta requires `O(n)` work to construct
/// the resulting zero-filled vector of length `n`.
///
/// # Panics
///
/// Panics if a sparse delta contains an invalid or internally inconsistent
/// payload.
pub fn apply_participation(base: &mut Vec<u8>, delta: &ArchivedParticipationDiff) {
    match delta {
        ArchivedParticipationDiff::AllZeros(len) => {
            base.clear();
            base.resize(len.to_native() as usize, 0);
        }
        // Delegate sparse application to the generic iterator implementation
        sparse => apply_participation_iter(base, sparse),
    }
}

/// Computes a compact sparse delta between two participation flag iterators.
///
/// This API is intended for consensus clients whose participation flags are
/// stored in tree-backed or otherwise non-contiguous structures. The caller
/// can expose the values through [`ExactSizeIterator`]s without first
/// materializing the complete vectors as contiguous buffers.
///
/// The iterators are consumed during diff generation.
///
/// Unlike [`diff_participation`], this function always returns
/// [`ParticipationDiff::Sparse`]. It does not perform the all-zero
/// specialization because the iterator is consumed while determining the
/// changed entries.
///
/// Values remaining in `target` after the common portion are treated as
/// newly appended participation flags and are stored in the delta's
/// `extension` field.
///
/// # Complexity
///
/// `O(n)` time and `O(m + e)` additional space, where `n` is the size of the
/// common portion, `m` is the number of modified entries, and `e` is the
/// number of appended target entries.
pub fn diff_participation_iter<I1, I2>(mut base: I1, mut target: I2) -> ParticipationDiff
where
    I1: ExactSizeIterator<Item = u8>,
    I2: ExactSizeIterator<Item = u8>,
{
    let common_len = base.len().min(target.len());

    // Reserve space for a typical sparse update while allowing the vectors
    // to grow normally for larger participation changes.
    let mut sparse_indices = Vec::with_capacity(50_000);
    let mut new_values = Vec::with_capacity(50_000);

    let mut last_idx = 0u64;

    for i in 0..common_len {
        let v1 = base.next().unwrap();
        let v2 = target.next().unwrap();

        if v1 != v2 {
            // Calculate and write the gap from the last changed index
            let gap = (i - last_idx as usize) as u64;
            write_varint(gap, &mut sparse_indices);

            new_values.push(v2);
            last_idx = i as u64;
        }
    }

    // Any remaining items in the target iterator are newly appended flags
    let extension = target.collect();

    ParticipationDiff::Sparse {
        sparse_indices,
        new_values,
        extension,
    }
}

/// Applies a participation delta to a mutable collection in place.
///
/// This API is intended for consensus clients whose participation flags are
/// stored in tree-backed or otherwise non-contiguous structures.
///
/// The destination collection is updated according to the sparse entries in
/// `delta`. Each encoded index gap identifies the next modified entry, whose
/// value is replaced with the corresponding entry from `new_values`. Values
/// in `extension` are then appended to the destination.
///
/// [`ParticipationDiff::AllZeros`] is intentionally not handled by this
/// function. Callers using the generic API must handle that representation
/// separately if they need to support it.
///
/// # Panics
///
/// Panics if the sparse delta contains an invalid or inconsistent payload,
/// including malformed varint data or fewer encoded indices than replacement
/// values.
///
/// The implementation may also extend the destination if an encoded index
/// falls outside its current length.
///
/// # Complexity
///
/// `O(m + e)` time and `O(1)` additional working space, excluding allocations
/// performed by the destination collection when it grows.
///
/// Here `m` is the number of modified entries and `e` is the number of
/// appended entries.
pub fn apply_participation_iter<T: crate::ListMutTarget<u8>>(
    target: &mut T,
    delta: &ArchivedParticipationDiff,
) {
    if let ArchivedParticipationDiff::Sparse {
        sparse_indices,
        new_values,
        extension,
    } = delta
    {
        let indices_raw = sparse_indices.as_slice();
        let values_iter = new_values.iter();

        let mut cursor = 0usize;
        let mut current_idx = 0usize;

        for val in values_iter {
            // Decode the gap to the next changed index
            let gap = read_varint(indices_raw, &mut cursor) as usize;
            current_idx += gap;

            // Defensively grow the target if the delta implies an index
            // beyond the current length (should rarely happen in practice).
            while target.len() <= current_idx {
                target.push(0);
            }

            // Apply the change
            *target.get_mut(current_idx).unwrap() = *val;
        }

        // Append any new flags from the extension
        for byte in extension.iter() {
            target.push(*byte);
        }
    }
}
