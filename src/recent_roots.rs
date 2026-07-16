//! Delta encoding for fixed-size Ethereum consensus root buffers.
//!
//! Ethereum consensus stores historical roots (such as block roots and state
//! roots) in fixed-capacity circular buffers. Advancing the chain overwrites
//! the oldest entries once the buffer wraps.
//!
//! Rather than diffing the entire buffer, this module records only the sequence
//! of newly written roots between two slots. Applying the delta replays those
//! writes into another buffer using the same circular indexing logic.
//!
//! This representation is independent of the buffer capacity and works with
//! any fixed-size root ring.

use crate::types::{ArchivedRootsDiff, RootsDiff};

/// Computes the sequence of newly written roots between two slots.
///
/// The returned delta contains every root written during the half-open slot
/// range `[base_slot, target_slot)`.
///
/// The input `buffer` is treated as a circular buffer, with its length defining
/// the modulo capacity.
///
/// # Arguments
///
/// * `base_slot` - First slot represented by the delta.
/// * `target_slot` - Slot immediately following the final recorded root.
/// * `buffer` - Circular root buffer.
///
/// # Complexity
///
/// O(target_slot - base_slot)
pub fn diff_roots(base_slot: u64, target_slot: u64, buffer: &[[u8; 32]]) -> RootsDiff {
    debug_assert!(
        target_slot >= base_slot,
        "target_slot must not precede base_slot"
    );

    let span = target_slot - base_slot;
    let capacity = buffer.len() as u64;

    let mut roots = Vec::with_capacity(span as usize);

    for i in 0..span {
        let slot = base_slot + i;
        let idx = (slot % capacity) as usize;
        roots.push(buffer[idx]);
    }

    RootsDiff { roots }
}

/// Applies a root delta to a circular root buffer.
///
/// Each stored root is written back into the destination buffer using the same
/// slot-to-index mapping employed during diff generation.
///
/// After application, the destination buffer contains the same root values for
/// every slot represented by the delta.
///
/// # Complexity
///
/// O(number of recorded roots)
pub fn apply_roots(base_slot: u64, base_buffer: &mut [[u8; 32]], delta: &ArchivedRootsDiff) {
    let capacity = base_buffer.len() as u64;

    for (i, root) in delta.roots.iter().enumerate() {
        let slot = base_slot + i as u64;
        let idx = (slot % capacity) as usize;
        base_buffer[idx] = *root;
    }
}
