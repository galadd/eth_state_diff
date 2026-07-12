//! Compact balance-state delta encoding.
//!
//! This module computes compact binary deltas between two validator balance
//! snapshots.
//!
//! The encoding is optimized for Ethereum beacon chain balance updates where
//! the overwhelming majority of balances either:
//!
//! - remain unchanged,
//! - change by a small amount,
//! - become zero, or
//! - are newly initialized.
//!
//! The resulting delta is designed for:
//!
//! - efficient serialization with `rkyv`,
//! - high compression with `zstd`,
//! - zero-copy decoding, and
//! - fast in-place reconstruction.

use rustc_hash::FxHashMap;

use crate::{
    BalanceDiffs,
    types::{
        ArchivedBalanceDiffs, BitTagVec, SET_NO_CHANGE, SET_TO_DIFF, SET_TO_TARGET_VALUE,
        SET_TO_ZERO,
    },
};

#[inline]
fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

fn write_varint(mut val: u64, buf: &mut Vec<u8>) {
    loop {
        if val < 0x80 {
            buf.push(val as u8);
            break;
        }
        buf.push((val as u8) | 0x80);
        val >>= 7;
    }
}

/// Computes a compact delta between two balance snapshots.
///
/// The returned delta contains sufficient information to reconstruct
/// `target` from `base`.
///
/// # Complexity
///
/// O(n)
///
/// # Panics
///
/// Never.
///
/// # Notes
///
/// The most common balance difference is stored separately as a mode and all
/// encoded differences are stored relative to this mode, improving varint
/// compression.
pub fn diff_balances(base: &[u64], target: &[u64]) -> BalanceDiffs {
    let common_len = base.len().min(target.len());

    let mut freq_map = FxHashMap::default();
    freq_map.reserve(1024);

    for i in 0..common_len {
        let v1 = base[i];
        let v2 = target[i];
        if v1 != v2 {
            let diff = v2 as i64 - v1 as i64;
            *freq_map.entry(diff).or_insert(0usize) += 1;
        }
    }

    let mode = freq_map
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(val, _)| val)
        .unwrap_or(0);

    let mut tags = BitTagVec::new(common_len);

    let mut varint_payload = Vec::with_capacity(common_len / 2);
    let mut target_values = Vec::new();

    for i in 0..common_len {
        let v1 = base[i];
        let v2 = target[i];

        if v1 == v2 {
            continue; // SET_NO_CHANGE is implicitly 0
        }

        if v2 == 0 {
            tags.set(i, SET_TO_ZERO);
        } else if v1 == 0 {
            tags.set(i, SET_TO_TARGET_VALUE);
            target_values.push(v2);
        } else {
            let diff = v2 as i64 - v1 as i64;

            if let Ok(_diff_i32) = i32::try_from(diff) {
                tags.set(i, SET_TO_DIFF);
                let corrected = diff - mode;
                let encoded = zigzag_encode(corrected);
                write_varint(encoded, &mut varint_payload);
            } else {
                tags.set(i, SET_TO_TARGET_VALUE);
                target_values.push(v2);
            }
        }
    }

    let appended_balances = if target.len() > base.len() {
        target[base.len()..].to_vec()
    } else {
        Vec::new()
    };

    BalanceDiffs {
        tags,
        mode,
        varint_payload,
        target_values,
        appended_balances,
    }
}

#[inline]
fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ -((n & 1) as i64)
}

#[inline]
fn read_varint(buf: &[u8], cursor: &mut usize) -> u64 {
    let mut val = 0u64;
    let mut shift = 0u32;

    loop {
        // SAFETY: The diff logic guarantees the payload is perfectly valid
        // and we will not read past the end of the buffer.
        let byte = buf[*cursor];
        *cursor += 1;

        val |= ((byte & 0x7F) as u64) << shift;

        // If the high bit is 0, this is the last byte of the varint
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
    }

    val
}

/// Applies a balance delta in-place.
///
/// After successful execution, `base` is transformed into the original target
/// snapshot used to produce `delta`.
///
/// This function performs no heap allocations except when new validators are
/// appended.
///
/// # Complexity
///
/// O(n)
pub fn apply_balances(base: &mut Vec<u64>, delta: &ArchivedBalanceDiffs) {
    let mode = delta.mode.to_native();

    // Setup iterators for the sparse payloads
    let mut target_iter = delta.target_values.iter();
    let payload = delta.varint_payload.as_slice(); // Zero-copy byte slice
    let mut payload_cursor = 0usize;

    let mut base_idx = 0usize;

    // Iterate over the dense tag array, processing 4 validators per byte
    for &tag_byte in delta.tags.data.iter() {
        // Fast path: if the whole byte is 0b00000000, skip 4 validators instantly
        if tag_byte == 0 {
            base_idx += 4;
            // Edge case: if base.len() isn't perfectly divisible by 4,
            // ensure we don't run past the end on the very last iteration.
            if base_idx > base.len() {
                break;
            }
            continue;
        }

        // Slow path: at least one validator in this chunk of 4 changed
        for bit in 0..4 {
            if base_idx >= base.len() {
                break;
            }

            // Extract the 2-bit tag for this specific validator
            let tag = (tag_byte >> (bit * 2)) & 0b11;

            match tag {
                SET_NO_CHANGE => {}
                SET_TO_ZERO => {
                    base[base_idx] = 0;
                }
                SET_TO_TARGET_VALUE => {
                    // Unwrap is safe: diff logic guarantees length congruence
                    let val = target_iter.next().unwrap();
                    base[base_idx] = val.to_native();
                }
                SET_TO_DIFF => {
                    // 1. Read the zigzag-encoded varint from the payload stream
                    let encoded = read_varint(payload, &mut payload_cursor);

                    // 2. Decode back to a signed i64 correction
                    let corrected = zigzag_decode(encoded);

                    // 3. Reconstruct the absolute diff by adding the mode back
                    let diff = corrected + mode;

                    // 4. Apply to base
                    base[base_idx] = (base[base_idx] as i64 + diff) as u64;
                }
                _ => unreachable!("Invalid 2-bit tag state encountered during apply"),
            }

            base_idx += 1;
        }
    }

    if !delta.appended_balances.is_empty() {
        base.extend(delta.appended_balances.iter().map(|v| v.to_native()));
    }
}
