# eth_state_diff


[![Crates.io](https://img.shields.io/crates/v/eth-state-diff.svg)](https://crates.io/crates/eth-state-diff)
[![Docs.rs](https://docs.rs/eth-state-diff/badge.svg)](https://docs.rs/eth-state-diff)

eth_state_diff computes and applies compact binary deltas between Ethereum consensus-layer states. It is designed for archival storage, fast-sync, and historical state reconstruction, replacing full snapshots with highly compressible encodings.

This library targets the **disk layer** — the output is always a compact, serializable delta. 

Consensus clients using persistent or tree-backed data structures will need to materialize these flat views before calling create. While this materialization introduces overhead to the integration step, it allows the diffing engine itself to operate at maximum memory bandwidth, strictly avoiding the cache-thrashing penalties of per-element tree traversals on dense data.

**Note on Integration Overhead**: The primary bottleneck in adopting this library for tree-backed clients is the flattening step. Future optimizations may explore streaming SSZ serializers or tree-specific traversal helpers, and contributions in this area are highly welcome.

## Features

- **Domain-specific encoders**: Mode-corrected zigzag varints for dense balance updates, sparse delta-varints for participation flags, and field-level SSZ patches for validators.
- **Hardware-optimized**: Linear scans over flat arrays designed to maximize CPU prefetching and memory bus utilization.
- **Zero-copy deserialization**: Native rkyv support for instant access to archived delta data without allocation.
- **Entropy-optimal**: Output payloads are structured to compress to extreme degrees using standard tools like zstd (e.g., ~900KB for 32 epochs of mainnet state).
- **Minimal allocations during apply**: Reconstruction modifies target buffers in-place, requiring heap allocations only for newly appended validators.
- **Generic integration**: Simple traits (DiffSource and DiffTarget) allow integration with any consensus client that can provide or consume raw SSZ byte slices.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
