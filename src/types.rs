use rkyv::{Archive, Deserialize, Serialize};

/// Size, in bytes, of an SSZ-serialized validator record.
pub const VALIDATOR_SSZ_SIZE: usize = 121;

/// Protocol-defined withdrawability delay for non-slashed validators.
pub const MIN_VALIDATOR_WITHDRAWABILITY_DELAY: u64 = 256;

/// A field-level modification to a validator record.
///
/// Validator fields are diffed independently to avoid rewriting entire
/// 121-byte SSZ validator records when only a small subset of fields change.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ValidatorField {
    WithdrawalCredentials,
    EffectiveBalance,
    Slashed,
    ActivationEligibilityEpoch,
    ActivationEpoch,
    ExitEpoch,
    WithdrawableEpochSlashed,
}

/// A modification to a single validator field.
///
/// Each patch identifies the validator index, the field being modified,
/// and the replacement SSZ bytes for that field.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ValidatorPatch {
    pub index: u32,
    pub field: ValidatorField,
    pub value: Vec<u8>,
}

/// Compact representation of the difference between two validator registries.
///
/// Only modified validator fields are recorded. Newly appended validators are
/// stored as raw SSZ bytes and appended during reconstruction.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ValidatorDiff {
    pub patches: Vec<ValidatorPatch>,
    pub appended_validators: Vec<u8>,
}

/// Compact representation of the difference between two balance snapshots.
///
/// A `BalanceDiffs` stores only the information required to transform one
/// balance vector into another.
///
/// Small balance differences are encoded as mode-adjusted zig-zag varints,
/// while values that can not be represented efficiently are stored explicitly.
///
/// This structure is intended to be serialized with `rkyv`.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BalanceDiff {
    pub tags: BitTagVec,

    pub mode: i64,

    pub varint_payload: Vec<u8>,
    pub target_values: Vec<u64>,
    pub appended_balances: Vec<u64>,
}

/// Compact delta encoding for participation flags.
///
/// A participation delta stores only the information required to transform one
/// participation vector into another.
///
/// The encoder automatically selects the most compact representation:
///
/// - [`ParticipationDiff::AllZeros`] when the target vector contains only
///   zero-valued flags.
/// - [`ParticipationDiff::Sparse`] when only a subset of indices change.
///
/// This type is intended to be serialized using `rkyv`.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ParticipationDiff {
    /// Represents a participation vector consisting entirely of zeros.
    ///
    /// During reconstruction the destination vector is resized to `len`
    /// and every entry is set to `0`.
    AllZeros(usize),

    /// Sparse delta representation.
    ///
    /// Only indices whose values differ from the base vector are stored.
    ///
    /// Changed indices are encoded as delta-varint gaps to reduce storage
    /// requirements for sparsely changing participation flags.
    Sparse {
        /// Delta-varint encoded gaps between successive modified indices.
        ///
        /// Each decoded gap is added to the previous modified index to recover
        /// the absolute index.
        sparse_indices: Vec<u8>,

        /// Replacement participation flags.
        ///
        /// Each entry corresponds one-to-one with a decoded index from
        /// `sparse_indices`.
        new_values: Vec<u8>,

        /// Participation flags for validators that exist only in the target
        /// vector.
        extension: Vec<u8>,
    },
}

/// Compact representation of the difference between two inactivity-score
/// vectors.
///
/// The encoder automatically selects the most compact representation:
///
/// - [`InactivityDiff::AllZeros`] when the target vector contains only zero
///   scores.
/// - [`InactivityDiff::Sparse`] when only a subset of scores change.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum InactivityDiff {
    /// Fast path for the 99.9% case where no scores change in the overlapping set.
    AllZeros(u32),

    /// Sparse inactivity-score updates.
    ///
    /// Only modified indices are stored.
    Sparse {
        /// Indices of modified inactivity scores.
        indices: Vec<u32>,

        /// Replacement scores corresponding one-to-one with `indices`.
        new_values: Vec<u64>,

        /// Scores for validators appended to the target vector.
        extensions: Vec<u64>,
    },
}

/// Sequence of roots written while advancing a circular root buffer.
///
/// Roots are stored in chronological slot order beginning at the supplied
/// base slot used during reconstruction.
///
/// The buffer capacity is intentionally omitted from the encoding since it is
/// determined by the destination buffer.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RootsDiff {
    /// Sequential list of 32-byte hashes added to the circular buffer
    /// between the base slot and target slot.
    pub roots: Vec<[u8; 32]>,
}

/// Sparse updates for a slashing ring buffer.
///
/// Each update stores the destination ring index together with its new slashing
/// value.
///
/// Only epochs whose values changed are included.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SlashingsDiff {
    /// A sparse list of (index, new_slashing_amount).
    /// Index fits in u16 (max size is 8192).
    /// In a 32-epoch window, this Vec will have a length of 0 or 1.
    pub updates: Vec<(u16, u64)>,
}

/// Sequence of RANDAO mixes written while advancing through epochs.
///
/// Mixes are stored in chronological epoch order beginning with the epoch
/// containing the supplied base slot used during reconstruction.
///
/// The circular buffer capacity is intentionally omitted from the encoding, as
/// it is determined by the destination buffer during application.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RandaoDiff {
    /// Sequential list of 32-byte randao reveals added during this window.
    pub mixes: Vec<[u8; 32]>,
}

/// Delta representation of the Eth1 data vote list.
///
/// The encoder distinguishes between ordinary vote accumulation and the
/// protocol-defined reset that occurs after an Eth1 voting period.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Eth1DataVotesDiff {
    /// Additional votes appended to the existing list.
    Append(Vec<u8>),

    /// The vote list was reset before appending new votes.
    ResetAndAppend(Vec<u8>),
}

/// Delta representation for FIFO queues.
///
/// Rather than storing the full queue, the delta records how many items were
/// consumed from the front and the newly appended items at the back.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FifoQueueDiff {
    /// The number of items consumed from the front of the base queue.
    pub consumed_count: u32,

    /// The raw SSZ bytes of *only* the newly appended items at the end.
    pub appended_items: Vec<u8>,
}

#[derive(Eq, PartialEq, Debug, Clone, Default, Archive, Deserialize, Serialize)]
pub struct BitTagVec {
    /// Packed storage containing four 2-bit tags per byte.
    pub data: Vec<u8>,
    pub len: usize,
}

/// Balance is unchanged.
pub const SET_NO_CHANGE: u8 = 0b00;

/// Balance is replaced with zero.
pub const SET_TO_ZERO: u8 = 0b10;

/// Balance is reconstructed by applying a stored difference.
pub const SET_TO_DIFF: u8 = 0b11;

/// Balance is replaced with its absolute target value.
pub const SET_TO_TARGET_VALUE: u8 = 0b01;

/// Dense 2-bit tag vector.
///
/// Four entries are packed into each byte.
///
/// Each entry describes how the corresponding validator balance should be
/// reconstructed.
///
/// Encoding:
///
/// - `00` — unchanged
/// - `01` — absolute target value
/// - `10` — set to zero
/// - `11` — apply encoded difference
impl BitTagVec {
    pub fn new(len: usize) -> Self {
        let bytes = len.div_ceil(4);
        Self {
            data: vec![0; bytes],
            len,
        }
    }

    /// Sets the tag at `idx`.
    #[inline]
    pub fn set(&mut self, idx: usize, tag: u8) {
        let byte = idx / 4;
        let shift = (idx % 4) * 2;

        self.data[byte] |= (tag & 0b11) << shift;
    }

    /// Returns the tag stored at `idx`.
    #[inline]
    pub fn get(&self, idx: usize) -> u8 {
        let byte = idx / 4;
        let shift = (idx % 4) * 2;
        (self.data[byte] >> shift) & 0b11
    }
}
