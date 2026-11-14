# Specification verification

[SPEC.md](SPEC.md) defines the native protocol. The following checks cover its implementation:

| Area | Verification |
| --- | --- |
| Canonical bytes, hashes, signatures and genesis | `tests/conformance.rs`, `scripts/verify-vectors.py` |
| Ownership, replay counters, rewards, validation and forks | `tests/consensus.rs`, `src/ownership.rs` tests |
| Framing, handshake, lag and bounded synchronization | `tests/network.rs`, network and node unit tests |
| Catch-up, failure and restart | `tests/recovery.rs`, `scripts/network-fault-test.py`, `scripts/workshop-demo` |
| Saved settings, peer controls and terminal interface | Settings/TUI unit tests and `scripts/test-*.py` |
| Grupa 1 interoperability | Compatibility unit tests and `scripts/test-compatibility.py`; rules in [COMPATIBILITY.md](COMPATIBILITY.md) |

Build and validation commands are in [README.md](README.md). Local tests do not establish acceptance by the workshop group or interoperability with another team's live server. The group confirmation and LAN/mentor checks in the specification remain applicable.
