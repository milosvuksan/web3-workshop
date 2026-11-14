# Code audit and mining measurements — 2026-11-14

Three `gpt-6.1-sol` subagents at **medium reasoning** audited the Rust prototype in parallel: consensus/cryptography/mempool, networking/coordinator/recovery, and CLI/benchmark behavior. The primary agent reviewed the patches, investigated an additional signal-shutdown issue, consolidated the checks, and ran the performance measurements serially. All changes are on `milos` in the implementation worktree.

## Findings fixed

| Issue | Impact and correction |
| --- | --- |
| Periodic maintenance could invalidate an already queued mined result | A finished worker could trigger a new generation before its successful event was processed, discarding valid PoW. Keep successful queued results pending until handled; retry only failed delivery. Previous-generation and mining-off results still cannot extend the chain. |
| Rejected/full peer sessions did not promptly close their transports | Dropping a sender could drain queued lagged output or wait for a read timeout. Explicit cancellation now closes read/write transport promptly, including while coordinator events are backpressured. Transport drops before awaiting a disconnect notification. |
| Network tasks outlived the coordinator | An owned `Network` guard now aborts listener/outgoing tasks; incoming tasks are owned by a `JoinSet`, and writers run within their session future. |
| A full wanted-hash set could starve missing ancestors | Ancestors required by retained orphans now displace speculative tip requests when storage fills. |
| Duplicate orphan insertion could evict another pending block | Deduplicate before applying capacity eviction. |
| Detached transaction lookup scanned complete histories quadratically | Walk only divergent branch suffixes and preserve transaction order. An ordinary one-block extension now takes a constant number of parent steps. |
| Seed parsing allowed arbitrary whitespace | Accept exactly 64 lowercase hex digits and at most one trailing LF, matching the documented format. |
| Ctrl-C could hang with stdin still open | Reproduced on an idle node with an open stdin pipe. Finish node cleanup and bound Tokio runtime teardown to 100 ms, avoiding an indefinite wait for its uncancellable blocking stdin read. Synchronous commands do not create a Tokio runtime. |
| Zero-work benchmark inputs produced invalid timing estimates | CLI parsers reject zero hash attempts and zero block samples. |

No consensus rules, canonical encodings, difficulty, genesis, fixtures, or wire schemas changed. The supplied `SPEC.md` remains byte-identical. The defaults document clarifies ancestry request priority, prompt session cancellation, queued mining results, and strict seed-file formatting.

## Verification

The final code passes `cargo fmt --check`, Clippy for all targets with warnings denied, and **29 Rust tests**: 6 library, 5 binary/benchmark, 3 conformance, 8 consensus, 5 framing/synchronization, and 2 identity-recovery tests. The added coverage includes queue cancellation/backpressure, stale and queued mining results, handshake rejection, orphan/request capacity, seed formatting, detached transaction ordering, dependent mempool restoration, and benchmark statistics/input validation.

The independent Python/OpenSSL fixture check passes. The enhanced real TCP fault test verifies forced loss, mid-sync disconnect/reconnect, out-of-order ancestry, fork rollback, and actual socket closure for malformed/oversized/incompatible/self-identity connections. The SIGINT subprocess regression verifies successful shutdown while its stdin writer remains open. The three-node release demo passes default-on lag, concurrent mining, painting/transfers, late join, SIGKILL, and identity-preserving recovery from surviving peers.

## Benchmark scope

The machine is an **AMD Ryzen 7 PRO 8840HS**, 8 physical cores / 16 logical CPUs, running Linux x86-64. Build: Rust/Cargo 1.100.0, optimized release profile, default compiler target/flags, CPU governor `powersave` with boost enabled. Each timed run uses **one mining worker**, the unchanged **23-bit** target, and empty transaction lists. The measurements run serially after this audit's test miners and builds stop. This is a desktop machine, not an isolated or CPU-pinned benchmark host.

Two workloads are measured:

- `benchmark-mining`: mine and fully validate 100 sequential blocks with a fixed public test identity, fixed timestamp sequence, and nonce starting at zero. Record actual successful hash attempts and every mining/complete-block duration. Complete-block time includes candidate construction, hashing, and chain insertion/validation; it excludes network/coordinator/thread lifecycle.
- `benchmark-node.py`: start a normal isolated node with a new temporary identity, discard five warm-up blocks, then measure 100 consecutive tip-notification intervals. This includes actual worker replacement, candidate construction, PoW, validation, coordinator scheduling, and normal diagnostic exports. Lag stays enabled by default; there are no peers, so peer propagation is outside this timing.

The two workloads use different block headers and therefore different PoW search lengths. Differences in observed averages must not be interpreted as a direct measurement of node overhead. PoW block times vary substantially; percentile estimates from 100 samples are descriptive rather than guarantees.

## Results

| Workload | Blocks | Mean (s) | Median (s) | p95 (s) | Min (s) | Max (s) |
| --- | --- | --- | --- | --- | --- | --- |
| Miner + chain validation | 100 | 0.580058 | 0.450612 | 1.563468 | 0.002963 | 2.546280 |
| Live node (after warm-up) | 100 | 0.459331 | 0.320188 | 1.347109 | 0.003222 | 2.286185 |

The deterministic miner workload performed **933,794,118 hash attempts** in **57.989 seconds** of mining: **16.103 million hashes/second**. At 23 bits this predicts **0.521 seconds** expected solo time from `2^23 / rate`. Its complete-block elapsed time was 58.006 seconds. The live-node 100 measured intervals totaled 45.933 seconds, after five excluded warm-up blocks.

The observed averages differ because the deterministic workload searched 9,337,941 attempts per block, whereas a 23-bit target has an expected 8,388,608 attempts; the live node used different random identity/timestamp headers. Typical block time on this machine is around half a second with one worker under this workload; individual searches are much faster or slower.

Raw evidence: [deterministic samples](benchmark-results/mining-100.json), [live-node samples](benchmark-results/node-100.json), and [machine/build metadata](benchmark-results/machine.json). An independent Python check reconstructed all 100 deterministic headers and verified their hashes, PoW and attempt counts; checked the live samples' heights/PoW and summary statistics; and matched the benchmark binary's SHA-256 fingerprint. No private keys are included.

Reproduce after a release build:

```bash
target/release/baneworkshop benchmark-mining --samples 100 --json > /tmp/mining-100.json
python3 scripts/benchmark-node.py --blocks 100 --warmup 5 --output /tmp/node-100.json
```

Functional regression commands:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 scripts/verify-vectors.py
python3 scripts/network-fault-test.py
python3 scripts/test-shutdown.py --binary target/release/baneworkshop
./scripts/workshop-demo
```

The final audited release demo converged at height 22, tip `000001b9e4e7f695afbaeab853b24890a469522e08977e3e6ab5137fa8239df2`, state hash `75e2ab57e5ff4a85d73218fadf5ff91fa82650c265b90a72f16a5ba016898002`. Its process logs and diagnostic exports are retained at `/tmp/pixel-audit-final-demo-20261114`.


## Remaining scope

This audit found no additional consensus mismatch against the documented prototype defaults. History/state snapshots still grow without pruning, and mempool projection/reconciliation can repeat signature verification; these are existing workshop-scale limitations. Cross-language approval and interoperability checks remain team work, and restart recovery still requires a surviving peer because chain storage is in memory.
