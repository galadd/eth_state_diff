# Changelog

All notable changes to this project will be documented in this file.

## [0.2.1] - 2026-08-15

### Added

- Added documentation examples and doctests to match the current API.
- Added comprehensive Rustdoc documentation across the public API and state
  delta encoding modules.
- Added detailed documentation for delta application errors and validation
  behavior.
- Added `ListMutTarget` for generic mutable access to list-like collections.
- Added iterator-based diff and apply paths for balances, validators, and
  participation, allowing clients to operate on existing iterators without
  materializing dense collections.
- Added comprehensive crate-level documentation covering architecture,
  integration, serialization, fork handling, and performance.
- Added detailed README documentation covering the library's architecture,
  delta strategies, client integration, and performance model.

## [0.2.0] - 2026-07-26

### Breaking Changes

- apply now returns Result<M, Error> instead of M. Invalid or corrupt deltas must be explicitly handled by the caller.
- DiffSource and DiffTarget trait methods now return Option<T> for fork-specific fields.
- scalar_header footprint significantly reduced by extracting SyncCommittee, historical_summaries, historical_roots, and current_epoch_participation into dedicated diff types.

### Added

- Explicit Error enum for safe delta application (ForkMismatch, InvalidFieldForFork, MalformedDelta).
- SyncCommitteeDiff for efficient unchanged/replacement encoding.
- HistoricalLogDiff using protocol-math to calculate exact append counts for both historical_roots (Phase0) and historical_summaries (Capella+).
- AttestationsDiff to support full Phase0 state reconstruction.
- Strict debug_assert invariant checking in create to catch client implementation bugs during testing.
- Support for Fulu, Gloas, and Heze fork discriminants.

### Fixed

- Ring buffer windowing: state_roots, randao_mixes, and slashings now correctly use target_slot instead of a hardcoded 1-epoch window.
- FIFO queue corruption: QueueDiff rewritten with strict item-boundary alignment checks and prefix validation. Falls back to FullReplacement if mutations are detected.
- Pending queue SSZ sizes: Correctly mapped to actual spec sizes (192, 24, 16 bytes) instead of incorrect constants.
- Delta size bloat: Removed ~2MB+ of highly entropic data (current_epoch_participation, historical logs) from scalar_header, significantly reducing on-disk size.

## [0.1.1] - 2026-07-22

### Changed

- Fixed repository link and crate description metadata.

## [0.1.0] - 2026-07-21

### Added

- Initial release.
- Delta encoding and reconstruction for Ethereum consensus state.
- Support for Phase0 through Electra beacon state formats.
- Specialized encoders for balances, validators, participation, inactivity scores,
  recent roots, RANDAO mixes, slashings, FIFO queues, and Eth1 data votes.
- Zero-copy serialization using `rkyv`.
- Optimized for archival storage and `zstd` compression.
