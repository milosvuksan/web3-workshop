# Pixel Chain — Rust prototype

Implementation of `IMPLEMENTATION_PLAN.md` on branch `milos`. The current [SPEC.md](SPEC.md) defines the protocol, including the original prototype decisions. [PROTOTYPE_DEFAULTS.md](PROTOTYPE_DEFAULTS.md) retains the rationale and local operational details. The [spec alignment review](SPEC_ALIGNMENT.md) confirms that the published rules match the implementation.

Progress against all original milestones and stretch goals is tracked in [MILESTONE_STATUS.md](MILESTONE_STATUS.md). The current canvas interface is CLI plus PPM image export; there is no browser UI or HTTP endpoint.

The node implements Ed25519 identities, fixed 23-bit PoW, ownership/painting/transfers, parcel replay counters, atomic validation, fork resolution, a mempool, full-mesh TCP gossip, lag/loss, missing-ancestor retries, late join, and recovery from surviving peers after a crash. Canvas output is a 64×32 PPM image. There is no durable chain storage.

## Build and check

Rust 2024 edition; resolved dependencies are pinned in `Cargo.lock`.

```bash
cargo build --release --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 scripts/verify-vectors.py          # Python 3 + OpenSSL; independent encoding/crypto check
python3 scripts/network-fault-test.py      # after release build; real TCP fault injection
python3 scripts/test-shutdown.py --binary target/release/baneworkshop
./scripts/workshop-demo                    # real 23-bit mining, lag, SIGKILL, restart
```

`workshop-demo` starts three independent processes in a temporary directory, prints its location, and cleans up its processes on success or failure. It deliberately drops no difficulty, uses lag by default, and compares final tips, full ownership/color/counter state, and canvas files. Use `--timeout 300` on slower hardware and `--data-dir /path/to/empty-directory` to retain logs at a chosen location. The fault test deliberately withholds responses, disconnects mid-sync, supplies missing ancestors out of order, and changes to a longer fork.

Regenerate full-difficulty fixtures with `cargo run --release --example generate_vectors`. Review fixture changes as protocol changes. Local Rust-to-Rust and Python/OpenSSL checks do not establish interoperability with another team's node.

## Run a local full mesh

In three terminals:

```bash
target/release/baneworkshop node --listen 127.0.0.1:7001 --peers 127.0.0.1:7002,127.0.0.1:7003 --data-dir data/a --lag-seed 41
target/release/baneworkshop node --listen 127.0.0.1:7002 --peers 127.0.0.1:7001,127.0.0.1:7003 --data-dir data/b --lag-seed 42
target/release/baneworkshop node --listen 127.0.0.1:7003 --peers 127.0.0.1:7001,127.0.0.1:7002 --data-dir data/c --lag-seed 43
```

Each node creates/reloads its own private `identity.seed` with owner-only Unix access and prints its public key. Use a unique data directory for each process. For LAN use, bind `--listen 0.0.0.0:7001` and configure the actual LAN addresses of all other nodes. Lag remains enabled unless `--no-lag` is explicitly supplied.

Console commands:

```text
help
identity
status
mining on
mining off
paint 181 ff8000
transfer 181 <recipient-public-key>
export
export /tmp/canvas.ppm
quit
```

Use a parcel ID in `status.owned`, and replace the recipient with the other node's 64-character lowercase public key. Paint/transfer commands sign and submit a transaction; a subsequent mined block confirms it. Sequential pending commands use the next pending counter automatically. Commands fail if the identity does not own the parcel. `mining off` leaves networking and synchronization active. Closing stdin also leaves the node serving.

Every data directory contains atomically refreshed `status.json`, `state.json`, and `canvas.ppm`. Open the PPM with an image viewer, or convert it with a PPM-capable tool. `state.json` contains deterministic genesis/tip/ownership/RGB/replay state and can be compared byte for byte between stopped-mining peers. `status.connections` counts TCP sessions; retaining both directions normally yields two sessions per peer pair. `state_hash` is diagnostic, not part of consensus.

For crash recovery: leave other nodes running, kill one process with `kill -9 PID`, and restart its exact command with its existing data directory. Its identity persists; it reconstructs the chain from peers. The JSON exports are never loaded as chain storage. Restarting every node after all peers have stopped loses the chain.

Generate/reload a standalone key with `target/release/baneworkshop key --file data/identity.seed`. Load an explicit existing identity using `node --key PATH`. Keep the seed private; it is excluded from version control.

## Hashing measurement

```bash
target/release/baneworkshop benchmark --hashes 4000000
# Mine and fully validate 100 sequential new blocks with a reproducible workload:
target/release/baneworkshop benchmark-mining --samples 100 --json
# Measure 100 actual node block intervals, including worker/coordinator overhead:
python3 scripts/benchmark-node.py --blocks 100 --warmup 5 --output /tmp/node-mining.json
```

This hashes the actual canonical header using the same cached SHA-256 prefix as the miner. Expected group block time is approximately `8388608 / combined_hashes_per_second` seconds. Difficulty stays at the agreed 23 bits regardless of measured speed. The new-block benchmark uses the production miner and validator at 23 bits with one worker; its JSON retains every sample and reports mean, median, p95, and hash rate. The live-node benchmark starts an isolated node with a fresh temporary identity, normal diagnostics, and default-on lag; no peers are connected. See [AUDIT_REPORT.md](AUDIT_REPORT.md) for the audit fixes and current measured mining times, or [VALIDATION.md](VALIDATION.md) for the original implementation run.
