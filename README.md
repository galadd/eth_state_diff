# eth_state_diff

eth_state_diff computes and applies compact binary deltas between Ethereum consensus-layer states. It is designed for archival storage, fast-sync, and historical state reconstruction, replacing full snapshots with highly compressible encodings.

## Design Philosophy

This library targets the **disk layer**. It operates strictly on contiguous, flat SSZ byte buffers (&[u8], &mut Vec<u8>) rather than complex in-memory tree structures. This allows the delta algorithms to saturate memory bandwidth and avoids serialization overhead during reconstruction.

## Features

- **Domain-specific encoders**: Mode-corrected zigzag varints for dense balance updates, sparse delta-varints for participation flags, and field-level SSZ patches for validators.
- **Hardware-optimized**: Linear scans over flat arrays designed to maximize CPU prefetching and memory bus utilization.
- **Zero-copy deserialization**: Native rkyv support for instant access to archived delta data without allocation.
- **Entropy-optimal**: Output payloads are structured to compress to extreme degrees using standard tools like zstd (e.g., ~900KB for 32 epochs of mainnet state).
- **Minimal allocations during apply**: Reconstruction modifies target buffers in-place, requiring heap allocations only for newly appended validators.
- **Generic integration**: Simple traits (DiffSource and DiffTarget) allow integration with any consensus client that can provide or consume raw SSZ byte slices.
