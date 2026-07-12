# eth_state_diff

`eth_state_diff` provides fast delta encoding and reconstruction for Ethereum
consensus-layer state.

The crate is designed for clients that frequently persist, transmit, or
reconstruct Beacon Chain state while minimizing storage and bandwidth costs.

Unlike snapshot-based storage, `eth_state_diff` computes compact binary deltas
between consecutive states and reconstructs the target state with a single
linear pass.

## Features

- Fast O(n) delta generation
- Fast in-place reconstruction
- Compact binary representation
- Zero-copy deserialization via `rkyv`
- Excellent compression with `zstd`
- Minimal allocations during reconstruction
- Designed for embedding into consensus clients

