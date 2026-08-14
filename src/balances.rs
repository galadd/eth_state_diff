//! Compact delta encoding and reconstruction for Ethereum validator balances.
//!
//! This module computes compact binary deltas between two validator balance
//! snapshots and applies those deltas to reconstruct the target snapshot.
//!
//! The encoding is specialized for Ethereum beacon-chain balances, where most
//! validators either retain the same balance or change by a relatively small
//! amount between consecutive states.
//!
//! ## Encoding
//!
//! For each balance in the portion shared by the base and target snapshots,
//! the delta records one of four states using a packed two-bit tag:
//!
//! - [`SET_NO_CHANGE`] — the balance is unchanged;
//! - [`SET_TO_ZERO`] — the target balance is zero;
//! - [`SET_TO_DIFF`] — the target is reconstructed by applying a signed
//!   difference to the base balance;
//! - [`SET_TO_TARGET_VALUE`] — the target balance is stored explicitly.
//!
//! Changed balances whose difference fits in an `i32` are normally encoded as
//! signed differences. The most frequently occurring difference is selected as
//! the [`BalancesDiff::mode`], and encoded differences store only the
//! difference relative to that mode.
//!
//! Signed corrected differences are encoded as zig-zag integers followed by a
//! variable-length integer encoding. This makes small and frequently occurring
//! changes inexpensive to store.
//!
//! Differences that do not fit in an `i32` are encoded as explicit target
//! values.
//!
//! Balances that exist only in the target snapshot are stored in
//! [`BalancesDiff::appended_balances`].
//!
//! ## Two-pass encoding
//!
//! [`diff_balances_iter`] cannot require its input iterators to implement
//! [`Clone`]. It therefore performs the diff in two logical passes:
//!
//! 1. the common portion of the iterators is consumed into a compact
//!    intermediate list of changes;
//! 2. the statistical mode is selected and the changes are encoded.
//!
//! Any remaining items in the target iterator are treated as newly appended
//! balances.
//!
//! ## Iterator API
//!
//! [`diff_balances`] is the convenience API for contiguous balance slices.
//! [`diff_balances_iter`] is intended for consensus clients whose balances are
//! stored in persistent lists, trees, or other non-contiguous structures.
//!
//! The iterator API avoids requiring the caller to materialize the complete
//! balance registry as a flat buffer.
//!
//! ## Reconstruction
//!
//! [`apply_balances`] and [`apply_balances_iter`] mutate the supplied balance
//! collection in place.
//!
//! The supplied collection must represent the **base snapshot** from which the
//! delta was generated. After successful application, it contains the target
//! balances.
//!
//! Existing balances are updated in place. Target balances that extend beyond
//! the base snapshot are appended from the delta.
//!
//! ## Complexity
//!
//! [`diff_balances`] and [`diff_balances_iter`] run in:
//!
//! ```text
//! O(n)
//! ```
//!
//! where `n` is the number of balances in the common portion of the snapshots.
//!
//! Delta generation additionally requires storage proportional to the number
//! of changed balances:
//!
//! ```text
//! O(k)
//! ```
//!
//! where `k` is the number of changed balances.
//!
//! [`apply_balances`] and [`apply_balances_iter`] run in:
//!
//! ```text
//! O(n + a)
//! ```
//!
//! where `n` is the number of balances represented by the tag vector and `a`
//! is the number of appended balances.
//!
//! Reconstruction operates in place and does not require allocating a second
//! balance buffer.
//!
//! ## Serialization
//!
//! [`BalancesDiff`] is designed to be serialized using `rkyv` and can then be
//! passed to a general-purpose compressor such as zstd.
//!
//! The delta representation itself is independent of the serialization and
//! compression layer.
//!
//! ## Delta validity
//!
//! Applying a delta assumes that the supplied base collection corresponds to
//! the base snapshot used to create the delta. The application functions do
//! not independently verify the original balance values.
//!
//! In particular, a `SET_TO_DIFF` entry applies its decoded difference to the
//! current value in the supplied target collection. Applying the same delta to
//! a different base snapshot therefore does not generally produce the intended
//! target snapshot.
//!
//! [`BalancesDiff`]: crate::types::BalancesDiff
//! [`SET_NO_CHANGE`]: crate::types::SET_NO_CHANGE
//! [`SET_TO_ZERO`]: crate::types::SET_TO_ZERO
//! [`SET_TO_DIFF`]: crate::types::SET_TO_DIFF
//! [`SET_TO_TARGET_VALUE`]: crate::types::SET_TO_TARGET_VALUE

use rustc_hash::FxHashMap;

use crate::types::{
    ArchivedBalancesDiff, BalancesDiff, BitTagVec, SET_NO_CHANGE, SET_TO_DIFF, SET_TO_TARGET_VALUE,
    SET_TO_ZERO,
};

/// Computes a compact balance delta between two contiguous balance slices.
///
/// The returned [`BalancesDiff`] contains the information required to
/// reconstruct `target` from `base`.
///
/// The common portion of the two slices is encoded using packed two-bit tags,
/// statistical mode correction, zig-zag encoded signed differences, and
/// explicit target values where necessary. If `target` contains more balances
/// than `base`, the additional balances are stored in
/// [`BalancesDiff::appended_balances`].
///
/// This is a convenience wrapper around [`diff_balances_iter`].
///
/// # Complexity
///
/// `O(n)` time and `O(k)` additional space, where `n` is the number of balances
/// in the common portion and `k` is the number of changed balances.
///
/// # Examples
///
/// ```ignore
/// let delta = diff_balances(&base, &target);
/// ```
///
/// [`BalancesDiff`]: crate::types::BalancesDiff
/// [`diff_balances_iter`]: crate::balances::diff_balances_iter
pub fn diff_balances(base: &[u64], target: &[u64]) -> BalancesDiff {
    diff_balances_iter(base.iter().copied(), target.iter().copied())
}

/// Computes a compact balance delta between two balance iterators.
///
/// This is the generic counterpart to [`diff_balances`]. It is intended for
/// consensus clients whose balance storage is not represented as a contiguous
/// `&[u64]`.
///
/// The iterators must implement [`ExactSizeIterator`], allowing the function
/// to determine the size of the common portion and distinguish existing
/// balances from balances appended to the target.
///
/// The iterators are consumed during encoding and do not need to implement
/// [`Clone`].
///
/// The common portion is first collected into a compact list of changed
/// balances so that the most frequently occurring balance difference can be
/// selected as the encoding mode. Remaining items in the target iterator are
/// stored as appended balances.
///
/// # Complexity
///
/// `O(n)` time and `O(k + a)` additional space, where:
///
/// - `n` is the number of balances in the common portion;
/// - `k` is the number of changed balances; and
/// - `a` is the number of balances appended to the target.
///
/// # Panics
///
/// Panics if an iterator violates the [`ExactSizeIterator`] contract and
/// yields a different number of items than reported by `len()`.
///
/// [`diff_balances`]: crate::balances::diff_balances
pub fn diff_balances_iter<I1, I2>(mut base: I1, mut target: I2) -> BalancesDiff
where
    I1: ExactSizeIterator<Item = u64>,
    I2: ExactSizeIterator<Item = u64>,
{
    let common_len = base.len().min(target.len());
    let mut changes = Vec::with_capacity(1024);

    // Pass 1: Identify changes and store them in a compact intermediate buffer.
    // This avoids requiring the caller's iterator to be `Clone` while still
    // allowing us to find the statistical mode.
    for i in 0..common_len {
        let v1 = base.next().unwrap();
        let v2 = target.next().unwrap();

        if v1 != v2 {
            changes.push(Change {
                idx: i,
                diff: v2 as i64 - v1 as i64,
                target: v2,
            });
        }
    }

    let mode = find_mode(&changes);
    let (tags, varint_payload, target_values) = encode(common_len, &changes, mode);

    // Any remaining items in the target iterator are newly appended balances.
    BalancesDiff {
        tags,
        mode,
        varint_payload,
        target_values,
        appended_balances: target.collect(),
    }
}

/// Applies a balance delta to a mutable balance collection in place.
///
/// `target` must initially contain the **base balance snapshot** used to
/// generate `delta`.
///
/// Existing balances are reconstructed according to the packed tags in the
/// delta. Changed balances encoded as differences are updated relative to
/// their current base value, while explicit and zero-valued entries replace
/// the corresponding balance directly.
///
/// Any balances stored in [`BalancesDiff::appended_balances`] are appended to
/// the collection after the common portion has been reconstructed.
///
/// This API accepts [`crate::ListMutTarget`] so that consensus clients can
/// apply the delta directly to tree-backed or persistent balance collections
/// without first materializing them as a `Vec<u64>`.
///
/// # Correctness
///
/// The supplied collection must correspond to the base snapshot for which
/// `delta` was generated. The delta does not contain enough information to
/// independently validate the original base balances.
///
/// # Complexity
///
/// The delta is processed in linear order over the encoded balance entries.
/// The total cost therefore depends on the complexity of
/// [`crate::ListMutTarget::get_mut`] for the supplied collection.
///
/// # Panics
///
/// In debug builds, panics if the length of the supplied collection does not
/// match the length represented by the delta's tag vector.
///
/// The function also assumes that the serialized delta is structurally valid.
/// Invalid varint payloads or inconsistent tag/payload data may panic during
/// reconstruction.
///
/// [`BalancesDiff::appended_balances`]: crate::types::BalancesDiff::appended_balances
pub fn apply_balances_iter<T: crate::ListMutTarget<u64>>(
    target: &mut T,
    delta: &ArchivedBalancesDiff,
) {
    let mode = delta.mode.to_native();
    let tag_len = delta.tags.len.to_native() as usize;

    debug_assert_eq!(
        target.len(),
        tag_len,
        "Target balance length does not match delta tag length"
    );

    let mut target_iter = delta.target_values.iter();
    let payload = delta.varint_payload.as_slice();
    let mut payload_cursor = 0usize;
    let mut base_idx = 0usize;

    for &tag_byte in delta.tags.data.iter() {
        if base_idx >= tag_len {
            break;
        }

        // Fast path: four consecutive SET_NO_CHANGE entries.
        if tag_byte == 0 {
            base_idx = (base_idx + 4).min(tag_len);
            continue;
        }

        for bit in 0..4 {
            if base_idx >= tag_len {
                break;
            }

            let tag = (tag_byte >> (bit * 2)) & 0b11;

            match tag {
                SET_NO_CHANGE => {}
                SET_TO_ZERO => {
                    *target.get_mut(base_idx).unwrap() = 0;
                }
                SET_TO_TARGET_VALUE => {
                    *target.get_mut(base_idx).unwrap() = target_iter.next().unwrap().to_native();
                }
                SET_TO_DIFF => {
                    let encoded = read_varint(payload, &mut payload_cursor);
                    let corrected = zigzag_decode(encoded);
                    let diff = corrected + mode;

                    // Bind to a variable to avoid multiple tree traversals
                    // in tree-backed implementations like Grandine's.
                    let val = target.get_mut(base_idx).unwrap();
                    *val = (*val as i64 + diff) as u64;
                }
                _ => unreachable!("Invalid 2-bit tag state encountered during apply"),
            }

            base_idx += 1;
        }
    }

    if !delta.appended_balances.is_empty() {
        for val in delta.appended_balances.iter() {
            target.push(val.to_native());
        }
    }
}

/// Applies a balance delta to a contiguous balance vector in place.
///
/// `base` must initially contain the balance snapshot from which `delta` was
/// generated. After application, `base` contains the reconstructed target
/// snapshot.
///
/// This is a convenience wrapper around [`apply_balances_iter`] for clients
/// that store balances contiguously.
///
/// # Complexity
///
/// `O(n + a)` time and `O(1)` additional working space, excluding storage
/// required for appended balances.
///
/// # Panics
///
/// In debug builds, panics if the length of `base` does not match the length
/// represented by the delta's tag vector.
///
/// [`apply_balances_iter`]: crate::balances::apply_balances_iter
pub fn apply_balances(base: &mut Vec<u64>, delta: &ArchivedBalancesDiff) {
    apply_balances_iter(base, delta)
}

/// Intermediate representation of a changed balance.
///
/// This is retained between the comparison pass and encoding pass so that
/// `diff_balances_iter` can determine the statistical mode without requiring
/// the source iterators to be cloned.
struct Change {
    idx: usize,
    diff: i64,
    target: u64,
}

/// Finds the most frequently occurring representable balance difference.
///
/// Only differences that fit within `i32` participate in mode selection.
/// Larger differences are always encoded as explicit target values and
/// therefore do not benefit from mode correction.
///
/// Returns `0` when no representable balance difference exists.
fn find_mode(changes: &[Change]) -> i64 {
    let mut freq_map = FxHashMap::default();
    freq_map.reserve(256);

    for change in changes {
        if i32::try_from(change.diff).is_ok() {
            *freq_map.entry(change.diff).or_insert(0usize) += 1;
        }
    }

    freq_map
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(val, _)| val)
        .unwrap_or(0)
}

/// Encodes changed balances using the selected statistical mode.
///
/// Each changed balance is assigned a two-bit tag. Differences that fit in
/// `i32` are encoded as zig-zag varints after subtracting `mode`; values that
/// cannot use the difference representation are stored as absolute target
/// values.
fn encode(common_len: usize, changes: &[Change], mode: i64) -> (BitTagVec, Vec<u8>, Vec<u64>) {
    let mut tags = BitTagVec::new(common_len);
    let mut varint_payload = Vec::with_capacity(changes.len());
    let mut target_values = Vec::new();

    for change in changes {
        let Change { idx, diff, target } = *change;

        if target == 0 {
            tags.set(idx, SET_TO_ZERO);
        } else if diff == target as i64 {
            // Implies base was 0
            tags.set(idx, SET_TO_TARGET_VALUE);
            target_values.push(target);
        } else if i32::try_from(diff).is_ok() {
            tags.set(idx, SET_TO_DIFF);
            let corrected = diff - mode;
            write_varint(zigzag_encode(corrected), &mut varint_payload);
        } else {
            tags.set(idx, SET_TO_TARGET_VALUE);
            target_values.push(target);
        }
    }

    (tags, varint_payload, target_values)
}

/// Encodes a signed integer using zig-zag encoding.
///
/// Negative and positive values of similar magnitude are mapped to nearby
/// unsigned values, making small signed differences efficient to represent
/// with the subsequent variable-length encoding.
#[inline]
fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

/// Decodes a value previously encoded with [`zigzag_encode`].
#[inline]
fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ -((n & 1) as i64)
}

/// Encodes an unsigned integer using the variable-length format used by the
/// balance delta representation.
///
/// Each output byte stores seven payload bits. The high bit indicates whether
/// another byte follows.
#[inline]
pub(super) fn write_varint(mut val: u64, buf: &mut Vec<u8>) {
    loop {
        if val < 0x80 {
            buf.push(val as u8);
            break;
        }
        buf.push((val as u8) | 0x80);
        val >>= 7;
    }
}

/// Decodes one unsigned variable-length integer from `buf`.
///
/// `cursor` is advanced past the decoded integer.
///
/// The caller must provide a structurally valid varint payload. This function
/// does not perform bounds or overflow validation and may panic when the
/// payload is malformed or truncated.
#[inline]
pub(super) fn read_varint(buf: &[u8], cursor: &mut usize) -> u64 {
    let mut val = 0u64;
    let mut shift = 0u32;

    loop {
        let byte = buf[*cursor];
        *cursor += 1;

        val |= ((byte & 0x7F) as u64) << shift;

        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
    }

    val
}
