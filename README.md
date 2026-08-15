# eth_state_diff

[![Crates.io](https://img.shields.io/crates/v/eth-state-diff.svg)](https://crates.io/crates/eth-state-diff)
[![Docs.rs](https://docs.rs/eth-state-diff/badge.svg)](https://docs.rs/eth-state-diff)

Compact binary delta encoding for Ethereum consensus-layer state.

`eth_state_diff` computes a delta between two beacon states and applies that
delta to reconstruct the target state. Instead of storing or transmitting a
complete state snapshot for every state transition, the library encodes only
the changes required to move from one state to the next.

The crate is intended for **archival storage, state synchronization, fast-sync,
and historical state reconstruction** where storing full consensus-state
snapshots is unnecessarily expensive.

---

## Why deltas?

Ethereum consensus state contains many fields with highly predictable update
patterns.

Some fields are append-only:

- historical roots
- historical summaries
- attestations
- Eth1 data votes

Some are sparse:

- validator fields
- inactivity scores
- participation flags
- slashing totals

Some are fixed-capacity circular buffers:

- block roots
- state roots
- RANDAO mixes
- slashings

Others behave like FIFO queues:

- pending deposits
- pending partial withdrawals
- pending consolidations

A generic byte-level diff cannot take advantage of these properties.

`eth_state_diff` therefore uses **field-specific encodings** that match the
semantics of each consensus-state component.

The result is a delta representation that is small before compression and
highly compressible when passed through a general-purpose compressor such as
zstd.

---

## Architecture

The library sits at the state/disk boundary rather than defining a beacon-state
implementation of its own.

```text
                Consensus Client
                       │
              ┌────────┴────────┐
              │                 │
         DiffSource        DiffTarget
              │                 ▲
              ▼                 │
           create()         apply()
              │                 │
              ▼                 │
      BeaconStateDelta ─────────┘
              │
        rkyv / storage
              │
              ▼
       Archived delta
```

The client provides access to its state through two traits:

* `DiffSource` — exposes the base and target state to the diff algorithms.
* `DiffTarget` — exposes mutable state storage to the application algorithms.

This keeps the delta engine independent of the consensus client's internal
state representation.

The client may use its own state types, containers, memory layout, or storage
engine as long as it can provide the interfaces required by these traits.

---

## Delta strategies

Each state component uses a representation appropriate to its update pattern.

| State component             | Delta strategy                                            |
| --------------------------- | --------------------------------------------------------- |
| Balances                    | Packed 2-bit operation tags + compact difference encoding |
| Validators                  | Field-level patches + appended SSZ records                |
| Block roots                 | Circular-buffer writes                                    |
| State roots                 | Circular-buffer writes                                    |
| RANDAO mixes                | Epoch-indexed circular-buffer writes                      |
| Slashings                   | Sparse circular-buffer updates                            |
| Eth1 data votes             | Append/reset encoding                                     |
| Historical roots            | Protocol-defined append intervals                         |
| Historical summaries        | Protocol-defined append intervals                         |
| Phase 0 attestations        | Unchanged / append / replacement                          |
| Participation               | Sparse delta-varint updates / all-zero representation     |
| Inactivity scores           | Sparse updates / all-zero representation                  |
| Sync committees             | Unchanged / full replacement                              |
| Pending deposits            | Validated FIFO delta / full replacement                   |
| Pending partial withdrawals | Validated FIFO delta / full replacement                   |
| Pending consolidations      | Validated FIFO delta / full replacement                   |

The algorithms operate on the serialized representation where doing so avoids
deserializing individual SSZ objects unnecessarily.

---

## State reconstruction

A typical workflow is:

```rust
let delta = eth_state_diff::create(&source);

// Serialize or archive `delta` for storage or transmission.
//
// Later, obtain an archived representation and apply it:
//
// let target = eth_state_diff::apply(state, archived_delta)?;
```

`create` computes the transition using the state exposed by `DiffSource`.

`apply` validates the fork and fork-specific fields before modifying the
destination state through `DiffTarget`.

The destination state must correspond to the base state from which the delta
was created.

---

## Client integration

A consensus client integrates with the library by implementing
`DiffSource` and `DiffTarget`.

The traits intentionally expose primitive buffers, slices, and iterators rather
than client-specific consensus types.

This allows the delta algorithms to operate independently of the client's
internal state representation.

### `DiffSource`

`DiffSource` provides the base and target values used to construct a delta.

Conceptually:

```text
DiffSource
    │
    ├── fork
    ├── base slot / target slot
    ├── scalar state
    ├── balances
    ├── validators
    ├── roots
    ├── RANDAO
    ├── slashings
    ├── Eth1 votes
    └── fork-specific fields
             │
             ▼
           create()
```

Fork-specific accessors should return `None` when the field does not exist for
the current fork.

### `DiffTarget`

`DiffTarget` provides mutable access to the destination state.

```text
ArchivedBeaconStateDelta
            │
            ▼
          apply()
            │
            ├── fork validation
            ├── universal fields
            ├── Phase 0 fields
            ├── Altair+ fields
            ├── Capella+ fields
            └── Electra+ fields
            │
            ▼
       reconstructed state
```

Fork-specific accessors should return `None` when the corresponding field does
not exist in the destination state.

---

## Scalar state

Not every consensus-state field requires a specialized delta algorithm.

Fields without a dedicated encoder are represented by `scalar_header`.

The client must serialize these fields to SSZ and concatenate them in the exact
order required by the consensus specification for the target fork.

The scalar header generally contains fields such as:

* `genesis_time`
* `genesis_validators_root`
* `slot`
* `fork`
* `latest_block_header`
* `eth1_data`
* `eth1_deposit_index`
* `justification_bits`
* `previous_justified_checkpoint`
* `current_justified_checkpoint`
* `finalized_checkpoint`
* `latest_execution_payload_header`
* Electra+ scalar fields such as withdrawal and deposit-request state
* Fulu+ `proposer_lookahead`

Fields with dedicated delta representations **must not** also be included in
the scalar header.

For example, balances, historical summaries, and pending deposits are handled
by their own encoders.

---

## Fork handling

`BeaconStateDelta` records the [`ForkName`] associated with the transition.

Fork-specific fields are represented as `Option<T>`:

```rust
pub struct BeaconStateDelta {
    pub fork: ForkName,

    // ...

    pub previous_participation: Option<ParticipationDiff>,
    pub historical_summaries: Option<HistoricalLogDiff>,
    pub pending_deposits: Option<QueueDiff>,
}
```

This allows one delta type to represent transitions across all supported forks
while keeping fields that do not exist for a particular fork absent from the
delta.

`apply` validates these invariants.

For example:

* Altair fields cannot be present in a Phase 0 delta.
* Capella fields cannot be present before Capella.
* Electra fields cannot be present before Electra.
* Phase 0 attestation fields cannot be present after their removal.

A fork mismatch results in an error rather than silently applying the delta to
the wrong state representation.

---

## Serialization

Delta structures derive the [`rkyv::Archive`], `Serialize`, and `Deserialize`
traits.

This makes deltas suitable for archived storage and zero-copy access to the
serialized delta representation where supported by the application.

The diff algorithms themselves remain independent of the serialization layer.

A typical storage pipeline can therefore be:

```text
Beacon states
     │
     ▼
  create()
     │
     ▼
BeaconStateDelta
     │
     ▼
   rkyv
     │
     ▼
 compressed delta
     │
     ▼
 storage / network
```

The resulting byte representation can additionally be compressed using a
general-purpose compressor such as zstd.

---

## Performance model

The library is designed around **linear iteration over consensus-state components**.

For large vectors such as balances, validators, and participation flags, the diff algorithms process elements sequentially. This keeps the comparison work predictable and allows the underlying client to choose the most appropriate storage representation.

The API intentionally accepts iterators for large state components rather than requiring callers to first materialize them into contiguous buffers. A client with a tree-backed or otherwise non-contiguous state representation can expose an iterator over its existing data and let the delta algorithm perform a single sequential pass.

For clients whose state is already stored densely, these linear scans naturally benefit from CPU caching, hardware prefetching, and compiler optimizations. For tree-backed clients, the iterator interface avoids requiring a separate dense copy solely for delta generation.

This is a deliberate trade-off: the delta algorithms require an O(n) traversal of the logical state components, but they do not require those components to be physically materialized as contiguous `Vec`s beforehand.

---

## Validation and safety

`apply` returns:

```rust
Result<M, Error>
```

rather than assuming that an arbitrary delta is valid for an arbitrary state.

The application path validates:

* fork compatibility;
* fork-specific field presence;
* archived fork decoding;
* malformed delta payloads.

Individual delta encoders also use conservative representations when their
assumptions about the state transition are not satisfied.

For example, the pending-queue encoder uses strict item-boundary validation
before emitting a FIFO delta and falls back to a full replacement when the
transition cannot safely be represented as a FIFO operation.

---

## Supported consensus forks

`ForkName` currently represents:

* Phase 0
* Altair
* Bellatrix
* Capella
* Deneb
* Electra
* Fulu
* Gloas
* Heze

Fork-specific state fields are selected through the `Option<T>` fields in
`BeaconStateDelta`.

---

## Design principles

The crate is built around a few principles:

### Match the encoding to the data structure

A circular buffer should be represented as circular-buffer writes rather than
as a generic byte diff.

A sparse vector should be represented as sparse updates rather than rewriting
the entire vector.

### Prefer protocol invariants over byte comparisons

Where the consensus protocol determines when an item must be appended, the
encoder can derive that information from slots and epochs rather than scanning
the complete buffers.

### Be conservative when assumptions fail

Compact encodings are only useful if they can be reconstructed correctly.

Where a transition cannot be safely represented by a specialized encoding,
the implementation falls back to a replacement representation.

### Keep consensus clients independent

The library does not require a particular beacon-state implementation.

Clients remain responsible for their own state representation while
`eth_state_diff` provides the delta algorithms.

---

## API

The primary public API is:

* [`create`] — compute a delta from a `DiffSource`.
* [`apply`] — apply an archived delta to a `DiffTarget`.
* [`BeaconStateDelta`] — complete state-transition representation.
* [`DiffSource`] — read-only state integration trait.
* [`DiffTarget`] — mutable state integration trait.
* [`ForkName`] — consensus fork identifier.
* [`ListMutTarget`] — abstraction for mutable primitive collections.

See the [API documentation on docs.rs](https://docs.rs/eth-state-diff)
for the complete interface and individual delta algorithms.

---

## License

Licensed under either of:

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
* MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
