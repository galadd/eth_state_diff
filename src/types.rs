//! Core data structures used by [`eth_state_diff`].
//!
//! This module defines the serialized delta representations produced by the
//! crate's diff algorithms.
//!
//! The structures in this module are deliberately independent of any specific
//! Ethereum consensus-client implementation. They contain only primitive Rust
//! values, byte buffers, and `rkyv`-archivable types. This allows a delta to be:
//!
//! 1. computed from one consensus state and another;
//! 2. serialized with [`rkyv`];
//! 3. optionally compressed with a general-purpose compressor such as zstd;
//! 4. stored as an archival record; and
//! 5. later deserialized and applied to another compatible state.
//!
//! # Delta model
//!
//! Most types in this module represent the transformation:
//!
//! ```text
//! base state + delta -> target state
//! ```
//!
//! Different consensus-state fields have different update patterns, so the
//! crate uses specialized representations rather than one universal encoding.
//! For example:
//!
//! - validator registries use field-level patches;
//! - balances use packed tags and varint-encoded differences;
//! - participation and inactivity scores use sparse updates;
//! - FIFO-like lists use consumed-item counts and appended bytes;
//! - circular buffers store only values written during the transition;
//! - append-only historical logs use protocol-defined append counts; and
//! - fields that change infrequently use unchanged/full-replacement variants.
//!
//! # Serialization
//!
//! All public delta structures derive [`rkyv::Archive`], [`rkyv::Serialize`],
//! and [`rkyv::Deserialize`]. They are therefore suitable for use as the
//! serialized representation of an archival state delta.
//!
//! The structures themselves do not perform compression. Applications can
//! serialize a delta with `rkyv` and subsequently compress the resulting bytes
//! using a compressor such as zstd.
//!
//! # Compatibility
//!
//! The delta types describe the encoding used by this crate. They should be
//! treated as part of the crate's serialization format: changing field types,
//! enum variants, or encoding invariants may affect the compatibility of
//! previously stored deltas.
//!
//! [`eth_state_diff`]: crate

use rkyv::{Archive, Deserialize, Serialize};

/// Number of slots in an Ethereum consensus epoch.
///
/// Ethereum mainnet uses 32 slots per epoch.
pub const SLOTS_PER_EPOCH: u64 = 32;

/// Size, in bytes, of an SSZ-serialized `Validator` record.
///
/// The size corresponds to the Phase0 validator container represented by this
/// crate. Validator records are treated as fixed-width byte sequences by the
/// byte-oriented validator diff implementation.
pub const VALIDATOR_SSZ_SIZE: usize = 121;

/// Size, in bytes, of an SSZ-serialized historical root.
///
/// A root is a 32-byte hash.
pub const HISTORICAL_ROOTS_SSZ_SIZE: usize = 32;

/// Size, in bytes, of an SSZ-serialized historical summary.
///
/// Historical summaries represented by this crate occupy 64 bytes.
pub const HISTORICAL_SUMMARIES_SSZ_SIZE: usize = 64;

/// Minimum validator withdrawability delay used when reconstructing the
/// `withdrawable_epoch` of a non-slashed validator.
///
/// When a validator is not slashed, its withdrawable epoch can be derived from
/// its exit epoch and this protocol-defined delay rather than being stored as
/// an independent delta field.
pub const MIN_VALIDATOR_WITHDRAWABILITY_DELAY: u64 = 256;

/// Identifies a validator field modified by a [`ValidatorPatch`].
///
/// Validator fields are encoded independently so that changing one field does
/// not require storing the complete 121-byte validator SSZ record.
///
/// The `WithdrawableEpochSlashed` variant is used specifically for slashed
/// validators. For non-slashed validators, `withdrawable_epoch` is derived
/// deterministically from `exit_epoch` and
/// [`MIN_VALIDATOR_WITHDRAWABILITY_DELAY`].
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ValidatorField {
    /// Validator withdrawal credentials.
    WithdrawalCredentials,

    /// Validator effective balance.
    EffectiveBalance,

    /// Whether the validator has been slashed.
    Slashed,

    /// Epoch at which the validator became eligible for activation.
    ActivationEligibilityEpoch,

    /// Epoch at which the validator was activated.
    ActivationEpoch,

    /// Epoch at which the validator exited.
    ExitEpoch,

    /// Explicit withdrawable epoch for a slashed validator.
    ///
    /// Non-slashed validators derive this value from `exit_epoch` instead of
    /// storing a separate patch.
    WithdrawableEpochSlashed,
}

/// A modification to a single validator field.
///
/// Each patch identifies the validator by its registry index and contains the
/// replacement value for exactly one [`ValidatorField`].
///
/// The `value` field contains the encoded representation expected by the
/// corresponding field. The interpretation depends on [`ValidatorField`].
///
/// For example:
///
/// - `WithdrawalCredentials` contains 32 bytes;
/// - integer epoch and balance fields contain their little-endian `u64`
///   representation; and
/// - `Slashed` contains a single byte.
///
/// This structure is intended to be produced by the validator diff algorithm
/// rather than constructed manually.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ValidatorPatch {
    /// Zero-based validator registry index.
    pub index: u32,

    /// Validator field modified by this patch.
    pub field: ValidatorField,

    /// Replacement bytes for the selected field.
    pub value: Vec<u8>,
}

/// Compact representation of the difference between two validator registries.
///
/// Existing validators are represented using field-level [`ValidatorPatch`]es.
/// Validators present only in the target registry are stored as consecutive
/// raw SSZ validator records in [`Self::appended_validators`].
///
/// This avoids rewriting complete validator records when only a small number
/// of fields changed.
///
/// # Reconstruction
///
/// Applying all patches to the common validator range and then appending the
/// records in `appended_validators` reconstructs the target registry.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ValidatorsDiff {
    /// Field-level modifications to existing validators.
    pub patches: Vec<ValidatorPatch>,

    /// Raw SSZ bytes of validators appended to the target registry.
    ///
    /// The bytes are stored consecutively, with each validator occupying
    /// [`VALIDATOR_SSZ_SIZE`] bytes.
    pub appended_validators: Vec<u8>,
}

/// Compact representation of the difference between two validator balance
/// snapshots.
///
/// The representation combines four mechanisms:
///
/// - [`BitTagVec`] stores a two-bit operation for each balance;
/// - `mode` stores the most common representable balance difference;
/// - `varint_payload` stores mode-adjusted signed differences using
///   zig-zag encoding; and
/// - `target_values` stores balances that are more efficiently represented as
///   absolute values.
///
/// Validators that exist only in the target snapshot are stored in
/// `appended_balances`.
///
/// This representation is designed to serialize efficiently with `rkyv` and
/// compress well with general-purpose compressors such as zstd.
///
/// # Reconstruction
///
/// Each tag determines how the corresponding target balance is reconstructed:
///
/// - unchanged balances require no payload;
/// - zero balances are set directly to zero;
/// - difference-encoded balances are reconstructed from `mode` and the
///   corresponding varint; and
/// - absolute values are read from `target_values`.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BalancesDiff {
    /// Packed two-bit operation tags for the common balance range.
    pub tags: BitTagVec,

    /// Most frequently occurring representable balance difference.
    ///
    /// Difference-encoded balances store their difference relative to this
    /// value before zig-zag and varint encoding.
    pub mode: i64,

    /// Zig-zag encoded, varint-serialized balance differences.
    pub varint_payload: Vec<u8>,

    /// Absolute target balances for changes represented without a difference.
    pub target_values: Vec<u64>,

    /// Balances belonging to validators appended to the target vector.
    pub appended_balances: Vec<u64>,
}

/// Compact delta representation for Ethereum participation flags.
///
/// The encoder selects between a dense all-zero representation and a sparse
/// representation depending on the target vector.
///
/// This type is intended to be serialized using `rkyv`.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ParticipationDiff {
    /// Represents a target participation vector containing only zero flags.
    ///
    /// During application, the destination vector is cleared and resized to
    /// `len`, with every entry initialized to zero.
    AllZeros(usize),

    /// Sparse representation containing only changed participation flags.
    ///
    /// The changed indices are stored as delta-varint encoded gaps in
    /// `sparse_indices`. Each decoded index corresponds positionally to one
    /// value in `new_values`.
    Sparse {
        /// Delta-varint encoded gaps between successive changed indices.
        ///
        /// Starting from index zero, each decoded gap advances the current
        /// index to the next changed entry.
        sparse_indices: Vec<u8>,

        /// Replacement participation flag for each changed index.
        ///
        /// This vector has the same number of logical entries as the decoded
        /// index sequence.
        new_values: Vec<u8>,

        /// Participation flags belonging to validators appended to the target
        /// vector.
        extension: Vec<u8>,
    },
}

/// Compact delta representation for validator inactivity scores.
///
/// The encoder uses a dedicated all-zero representation when the target vector
/// contains only zero scores. Otherwise, only changed scores are stored.
///
/// Scores belonging to validators appended to the target vector are stored
/// separately in `extensions`.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum InactivityDiff {
    /// Represents a target vector containing only zero inactivity scores.
    ///
    /// The destination vector is resized to `len` and initialized with zeroes
    /// during application.
    AllZeros(u32),

    /// Sparse inactivity-score updates.
    Sparse {
        /// Zero-based indices of modified inactivity scores.
        indices: Vec<u32>,

        /// Replacement score corresponding positionally to each entry in
        /// `indices`.
        new_values: Vec<u64>,

        /// Scores belonging to validators appended to the target vector.
        extensions: Vec<u64>,
    },
}

/// Sequence of roots written while advancing through a circular root buffer.
///
/// Roots are stored in chronological slot order. The slot used to reconstruct
/// the first entry is supplied separately to the apply function.
///
/// The buffer capacity is intentionally not serialized into the delta because
/// it is already known by the destination state.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RootsDiff {
    /// Roots written for the represented slot range, in chronological order.
    ///
    /// Each entry is a 32-byte consensus root.
    pub roots: Vec<[u8; 32]>,
}

/// Sparse updates for an Ethereum slashing ring buffer.
///
/// Each update identifies a ring-buffer index and the replacement slashing
/// total for that index.
///
/// Entries that do not appear in this vector are left unchanged during
/// application.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SlashingsDiff {
    /// Pairs of `(ring_index, new_slashing_amount)`.
    ///
    /// The index is stored as `u16`, which is sufficient for the consensus
    /// slashing-vector capacity represented by this crate.
    pub updates: Vec<(u16, u64)>,
}

/// Sequence of RANDAO mixes written while advancing through epochs.
///
/// Mixes are stored in chronological epoch order. The destination ring-buffer
/// capacity is supplied separately during application.
///
/// The capacity is intentionally omitted from the serialized representation
/// because it is a property of the destination consensus state.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RandaoDiff {
    /// RANDAO mixes written during the represented epoch range.
    ///
    /// Each mix is a 32-byte value.
    pub mixes: Vec<[u8; 32]>,
}

/// Delta representation for an Ethereum Eth1 data vote list.
///
/// The list normally grows by appending votes within an Eth1 voting period.
/// When the voting period resets, the representation switches to
/// [`Eth1DataVotesDiff::ResetAndAppend`].
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Eth1DataVotesDiff {
    /// Additional serialized vote bytes appended to the existing list.
    Append(Vec<u8>),

    /// The vote list was reset and replaced with the supplied serialized bytes.
    ///
    /// Application clears the existing list before appending these bytes.
    ResetAndAppend(Vec<u8>),
}

/// Universal delta representation for SSZ-serialized queue-like lists.
///
/// The representation supports both:
///
/// - FIFO transitions, where items are consumed from the front and appended at
///   the back; and
/// - safe fallback to complete replacement when the FIFO assumptions cannot be
///   established.
///
/// The FIFO representation stores raw SSZ bytes rather than deserializing
/// individual queue items.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum QueueDiff {
    /// Validated FIFO transition.
    ///
    /// The base queue loses `consumed_count` items from its front and receives
    /// `appended_items` at its back.
    Fifo {
        /// Number of complete queue items consumed from the front.
        consumed_count: u32,

        /// Raw SSZ bytes of items appended to the target queue.
        appended_items: Vec<u8>,
    },

    /// Complete replacement of the serialized target queue.
    ///
    /// This is used when the queue cannot safely be represented as a FIFO
    /// transition.
    FullReplacement(Vec<u8>),
}

/// Delta representation for an Ethereum sync committee.
///
/// Sync committees are stable for a sync committee period. Consequently, most
/// state-diff windows can represent the committee using
/// [`SyncCommitteeDiff::Unchanged`].
///
/// When the committee changes, the complete serialized target committee is
/// stored as a replacement rather than attempting to encode individual member
/// changes.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum SyncCommitteeDiff {
    /// The serialized sync committee did not change.
    Unchanged,

    /// Complete serialized SSZ representation of the target sync committee.
    FullReplacement(Vec<u8>),
}

/// Delta representation for an append-only historical consensus log.
///
/// Historical roots and historical summaries are appended according to
/// protocol-defined slot intervals. The diff algorithm can therefore determine
/// how many entries should have been appended from the slot transition rather
/// than comparing the complete base and target buffers.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum HistoricalLogDiff {
    /// No historical-log boundary was crossed.
    Unchanged,

    /// Raw SSZ bytes of the historical items appended during the transition.
    ///
    /// The item width depends on the historical log being represented.
    Append(Vec<u8>),
}

/// Delta representation for Phase0 pending attestation lists.
///
/// The representation supports both append-only transitions and complete
/// replacement. This matches the two update patterns used by
/// `current_epoch_attestations` and `previous_epoch_attestations`.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum AttestationsDiff {
    /// The serialized attestation list is unchanged.
    Unchanged,

    /// Serialized attestations appended to the existing list.
    Append(Vec<u8>),

    /// Complete serialized replacement of the target attestation list.
    FullReplacement(Vec<u8>),
}

/// Packed two-bit operation tags used by [`BalancesDiff`].
///
/// Four tags are stored in each byte, with the tag for index `i` occupying
/// bits `((i % 4) * 2)..((i % 4) * 2 + 2)`.
///
/// The four tag values are:
///
/// | Tag | Meaning |
/// | --- | --- |
/// | `00` | Balance is unchanged |
/// | `01` | Replace with an absolute target value |
/// | `10` | Set balance to zero |
/// | `11` | Apply an encoded balance difference |
///
/// A newly created [`BitTagVec`] is initialized entirely with
/// [`SET_NO_CHANGE`] tags.
///
/// # Storage
///
/// A vector containing *n* logical tags requires:
///
/// ```text
/// ceil(n / 4)
/// ```
///
/// bytes of backing storage.
///
/// # Example
///
/// ```
/// use eth_state_diff::types::{BitTagVec, SET_TO_DIFF, SET_TO_ZERO};
///
/// let mut tags = BitTagVec::new(6);
///
/// tags.set(0, SET_TO_DIFF);
/// tags.set(4, SET_TO_ZERO);
///
/// assert_eq!(tags.get(0), SET_TO_DIFF);
/// assert_eq!(tags.get(1), 0);
/// assert_eq!(tags.get(4), SET_TO_ZERO);
/// ```
#[derive(Eq, PartialEq, Debug, Clone, Default, Archive, Deserialize, Serialize)]
pub struct BitTagVec {
    /// Packed storage containing four two-bit tags per byte.
    pub data: Vec<u8>,

    /// Number of logical tags represented by `data`.
    ///
    /// This may be smaller than `data.len() * 4` because the final byte can
    /// contain unused tag positions.
    pub len: usize,
}

/// Tag indicating that the corresponding balance is unchanged.
pub const SET_NO_CHANGE: u8 = 0b00;

/// Tag indicating that the corresponding balance is replaced with zero.
pub const SET_TO_ZERO: u8 = 0b10;

/// Tag indicating that the corresponding balance is reconstructed by applying
/// an encoded signed difference.
pub const SET_TO_DIFF: u8 = 0b11;

/// Tag indicating that the corresponding balance is replaced with an absolute
/// target value.
pub const SET_TO_TARGET_VALUE: u8 = 0b01;

impl BitTagVec {
    /// Creates a zero-initialized tag vector containing `len` logical entries.
    ///
    /// Every entry initially has the [`SET_NO_CHANGE`] tag.
    ///
    /// # Complexity
    ///
    /// O(len / 4) time and memory.
    pub fn new(len: usize) -> Self {
        let bytes = len.div_ceil(4);

        Self {
            data: vec![0; bytes],
            len,
        }
    }

    /// Sets the two-bit tag at `idx`.
    ///
    /// Only the lowest two bits of `tag` are used.
    ///
    /// # Panics
    ///
    /// Panics if `idx >= self.len`.
    ///
    /// # Complexity
    ///
    /// O(1).
    #[inline]
    pub fn set(&mut self, idx: usize, tag: u8) {
        assert!(
            idx < self.len,
            "tag index {idx} out of bounds for length {}",
            self.len
        );

        let byte = idx / 4;
        let shift = (idx % 4) * 2;

        self.data[byte] |= (tag & 0b11) << shift;
    }

    /// Returns the two-bit tag stored at `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx >= self.len`.
    ///
    /// # Complexity
    ///
    /// O(1).
    #[inline]
    pub fn get(&self, idx: usize) -> u8 {
        assert!(
            idx < self.len,
            "tag index {idx} out of bounds for length {}",
            self.len
        );

        let byte = idx / 4;
        let shift = (idx % 4) * 2;

        (self.data[byte] >> shift) & 0b11
    }
}
