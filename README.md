# eth_state_diff

`eth_state_diff` is a high-performance library for computing and applying
compact deltas between Ethereum consensus-layer states.

The crate is intended for consensus clients, archival storage, state
synchronization, and historical state reconstruction. Instead of storing full
snapshots, it represents state transitions as specialized binary deltas that
can be efficiently serialized, compressed, and applied to reconstruct the
target state.

## Features

- Fast linear-time delta generation
- Fast in-place state reconstruction
- Specialized encoders for consensus state components
- Compact binary delta representation
- Zero-copy serialization with `rkyv`
- Excellent compression with `zstd`
- Generic traits for integration with any consensus client
- Minimal allocations during reconstruction
