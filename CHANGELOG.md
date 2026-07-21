# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-07-21

### Added

- Initial release.
- Delta encoding and reconstruction for Ethereum consensus state.
- Support for Phase0 through Electra beacon state formats.
- Specialized encoders for balances, validators, participation, inactivity scores,
  recent roots, RANDAO mixes, slashings, FIFO queues, and Eth1 data votes.
- Zero-copy serialization using `rkyv`.
- Optimized for archival storage and `zstd` compression.
