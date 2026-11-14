# Prototype validation — 2026-11-14

Validated on Rust 1.100.0 / Cargo 1.100.0, with the Rust 2024 package on `milos`. The implementation is in a separate worktree so the original `main` checkout and its untracked files remain intact. `SPEC.md` is byte-identical to the supplied file. `PROTOTYPE_DEFAULTS.md` records the provisional decisions that complete its blanks for this implementation.

## Checks performed

| Check | Result |
| --- | --- |
| `cargo build --release --locked` | Passed |
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed, no warnings |
| `cargo test --locked` | Passed: 14 integration tests, no failures |
| `python3 scripts/verify-vectors.py` | Passed: independent Python encoding/SHA-256 and OpenSSL Ed25519 checks |
| `python3 scripts/network-fault-test.py` | Passed against final release binary with default-on lag |
| `./scripts/workshop-demo` | Passed against final release binary at the actual 23-bit target, with default-on lag |
| `git diff --check` | Passed |
| Byte comparison of supplied and implementation `SPEC.md` | Identical |

The dependency downloads were cached in the ignored `.cargo-home` directory. Checks in this environment used `CARGO_HOME=/home/losmi/baneworkshop/implementation-milos/.cargo-home` and Cargo's `--offline` flag after fetching the locked packages. The TCP tests used local loopback sockets.

The 14 Rust tests cover standard cryptographic vectors; all nine project fixture categories plus PoW-valid fork/invalid-block fixtures; strict hex/field decoding; sequential paint/transfer behavior; atomic rejection and duplicate inclusion; ownership, signatures, counter exhaustion; multiplication cycles, exhausted allocation and full canvas; equal-work arrival independence; longer fork reorganization and replay restoration; detached mempool dependencies; unknown-parent and future-block handling; mining cancellation; framing/size limits; reproducible lag/loss; bounded pending ancestry; and persistent identity with Unix key permissions.

The fault-injection test deliberately omits a block announcement, drops the first request/response for each required block, disconnects the source while fetching ancestry, reconnects, sends descendants before parents, supplies a longer side branch, supplies an invalid side branch and a delayed old block, and opens malformed/oversized TCP sessions. It verifies the final winning state and detached mempool transactions.

## Final three-process demonstration

All three Rust nodes used explicit direct peer lists, separate data directories, and lag seeds 41, 42, and 43. Lag was enabled throughout (20–250 ms delay and 10% simulated outbound loss). No reduced-difficulty configuration was used.

The script verified:

1. Two peers connect and a miner earns a parcel at 23-bit difficulty.
2. Paint and transfer transactions gossip to another miner and confirm in order.
3. A third, late node downloads the chain and matches ownership, RGB values, and counters.
4. All three nodes mine concurrently, advance the chain, stop mining, and settle to the same branch.
5. One node is forcibly killed with `SIGKILL`; surviving peers mine and paint successfully.
6. The killed node restarts with its seed unchanged and its diagnostic exports removed, reconstructs the chain from peers, and retains its identity.
7. Mining and transaction creation stop while networking stays active through multiple retry periods. Every node has the same genesis, tip, complete parcel state, and binary PPM canvas.

The final run ended at height **13**, with:

```text
tip:        00000193d65062606ed97b1bedf450cfb5327c35eabe93d4b1a9e98d098eb4a6
state hash: 5c9a05959a2b9ccd570ab6a8fb7c96d0ff0e761776752c15ea2a71f48b139a87
```

Parcel 1078 was painted orange, transferred to the second node, then painted blue by its new owner after the first node crashed. The final state and canvas matched on all three nodes. Logs and diagnostic exports for this run were retained at `/tmp/pixel-demo-validation-final`; generated identities and outputs are excluded from Git. Run-to-run tips and parcel rewards vary because operational timestamps and freshly generated identities vary.

An earlier three-process run also passed at height 11 before the concurrent-miner portion was added.

## Mining measurement

`baneworkshop benchmark --hashes 4000000` measured approximately **8.7–15.5 million hashes/second** over two runs with different machine load. This uses the actual 137-byte block encoding and cached SHA-256 prefix, as does the mining worker. At 23 bits, this implies roughly **0.54–0.96 seconds** expected solo block time on this machine. Group expectation is `8388608 / sum(node_hash_rates)` seconds. This is a throughput estimate, not a promised interval; individual PoW searches vary widely. Difficulty was kept at 23 bits.

## Handoff status and limits

The standalone Rust prototype, deterministic state/fork tests, local Rust-to-Rust lagged mesh, and local crash/restart scenario pass. The supplied optional goals remain outside scope: durable chain storage, swaps, delegated painting, dynamic difficulty, and sparse topology.

The team still needs to approve or replace the provisional protocol choices, verify fixtures in their own languages, connect independently implemented nodes on the workshop LAN, and perform the mentor-observed scenario. Neither the independent Python/OpenSSL fixture checker nor the local three-process Rust test establishes those cross-language/team milestones. Restart recovery requires at least one surviving peer because chain history is held in memory.
