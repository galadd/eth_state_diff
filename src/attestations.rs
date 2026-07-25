//! Delta encoding for Phase0 pending attestations.
//!
//! `current_epoch_attestations` is an append-only log during an epoch.
//! `previous_epoch_attestations` is completely replaced at epoch boundaries.

use crate::types::{ArchivedAttestationsDiff, AttestationsDiff};

/// Diffs an append-only attestation list (e.g., `current_epoch_attestations`).
pub fn diff_attestations_append(base_ssz: &[u8], target_ssz: &[u8]) -> AttestationsDiff {
    if base_ssz == target_ssz {
        return AttestationsDiff::Unchanged;
    }

    if target_ssz.len() > base_ssz.len() {
        AttestationsDiff::Append(target_ssz[base_ssz.len()..].to_vec())
    } else {
        // Fallback if it somehow shrank or resets
        AttestationsDiff::FullReplacement(target_ssz.to_vec())
    }
}

/// Diffs a replaced attestation list (e.g., `previous_epoch_attestations`).
pub fn diff_attestations_replacement(base_ssz: &[u8], target_ssz: &[u8]) -> AttestationsDiff {
    if base_ssz == target_ssz {
        AttestationsDiff::Unchanged
    } else {
        AttestationsDiff::FullReplacement(target_ssz.to_vec())
    }
}

/// Applies an attestation delta in place.
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
