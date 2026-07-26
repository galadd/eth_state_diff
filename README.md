# eth_state_diff


[![Crates.io](https://img.shields.io/crates/v/eth-state-diff.svg)](https://crates.io/crates/eth-state-diff)
[![Docs.rs](https://docs.rs/eth-state-diff/badge.svg)](https://docs.rs/eth-state-diff)

eth_state_diff computes and applies compact binary deltas between Ethereum consensus-layer states. It is designed for archival storage, fast-sync, and historical state reconstruction, replacing full snapshots with highly compressible encodings.

## Architecture

This library targets the disk layer. It expects dense state components (e.g., balances, validators) as flat primitive vectors to maximize memory bandwidth via linear SIMD scans. Tree-backed clients must materialize these flat views before diffing, which is cheaper than per-element tree traversal over dense data.

To accommodate different client architectures and consensus forks, the API is built around two traits: DiffSource for reading, and DiffTarget for applying. Fork-specific fields are wrapped in Option<T>, serializing to zero bytes when unused.

## Client Integration

Implementing DiffSource and DiffTarget requires mapping your internal state types to flat primitive buffers. 

### The scalar_header Contract

Any consensus field not covered by a specialized encoder must be serialized to SSZ and concatenated into the scalar_header blob in the exact order defined by the consensus spec for the target fork:

- genesis_time, genesis_validators_root, slot, fork
- latest_block_header, eth1_data, eth1_deposit_index
- justification_bits, previous_justified_checkpoint, current_justified_checkpoint, finalized_checkpoint
- latest_execution_payload_header
- Electra+: next_withdrawal_index, next_withdrawal_validator_index, deposit_requests_start_index, etc.
- Fulu+: proposer_lookahead

Do not include fields with dedicated diffing algorithms (e.g., balances, historical_summaries, pending_deposits).

### Fork Mapping

Implement the trait once on your master BeaconState enum. Use match to return Some for active fields and None for inactive ones. The library enforces these invariants at runtime (apply) and in debug builds (create).

## Features

- **Fork-aware layout**: `Option<T>` for post-Phase0 fields. Unused fields compress to zero bytes.
- **Domain-specific encoders**: Mode-corrected zigzag varints for dense balance updates, sparse delta-varints for participation flags, and field-level SSZ patches for validators.
- **Hardware-optimized**: Linear scans over flat arrays designed to maximize CPU prefetching and memory bus utilization.
- **Zero-copy deserialization**: Native rkyv support for instant access to archived delta data without allocation.
- **Entropy-optimal**: Output payloads are structured to compress to extreme degrees using standard tools like zstd (e.g., ~900KB for 32 epochs of mainnet state).
- **Strict validation**: apply returns Result<M, Error>, safely rejecting corrupt payloads without panicking.
- **Generic integration**: Simple traits (DiffSource and DiffTarget) allow integration with any consensus client that can provide or consume raw SSZ byte slices.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
