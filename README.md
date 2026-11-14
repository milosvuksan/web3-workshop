# Pixel Chain — Rust prototype

Implementation of `IMPLEMENTATION_PLAN.md` on branch `milos`. The current [SPEC.md](SPEC.md) defines the protocol, including the original prototype decisions. [PROTOTYPE_DEFAULTS.md](PROTOTYPE_DEFAULTS.md) retains the rationale and local operational details. The [spec alignment review](SPEC_ALIGNMENT.md) confirms that the published rules match the implementation.

Progress against all original milestones and stretch goals is tracked in [MILESTONE_STATUS.md](MILESTONE_STATUS.md). The default interface is a live Rust terminal dashboard with a color canvas, controls, test runner, debug submenus, and logs. The scriptable line CLI and PPM export remain available. There is no browser UI or HTTP endpoint.

The node implements Ed25519 identities, fixed 23-bit PoW, ownership/painting/transfers, parcel replay counters, atomic validation, fork resolution, a mempool, full-mesh TCP gossip, lag/loss, missing-ancestor retries, late join, and recovery from surviving peers after a crash. Canvas output is a 64×32 PPM image. There is no durable chain storage.

## Start everything with one command

```bash
cargo run --release --locked
```

This opens the Ratatui/Crossterm dashboard and starts the node with its persistent identity. Mining starts stopped; networking starts online with the configured peers (none by default). Once built, run `target/release/baneworkshop` directly. An interactive terminal is required; 100×25 or larger shows the outlined parcel grid beside its inspector. Each parcel has its own border and unobstructed color fill; the viewport follows arrow-key selection across the 64×32 canvas. Larger terminals show more parcels. RGB rendering uses terminal true color; `NO_COLOR` is respected.

Configure a node, or start completely offline:

```bash
cargo run --release --locked -- tui --listen 127.0.0.1:7001 --peers 127.0.0.1:7002,127.0.0.1:7003 --data-dir data/a
cargo run --release --locked -- tui --offline --data-dir data/solo
# Optional: add --mine to start mining immediately, including while offline.
```

| Page | Available actions |
| --- | --- |
| **1 Canvas** | Live RGB grid, ownership overlay (`v`), arrow-key parcel selection, owner/color/counter inspection, your-parcel outlines, a color picker (`p`), and transfer (`t`) prompts |
| **2 Controls** | Toggle network or mining independently, export canvas/state, quit; select with arrows and Enter |
| **3 Tests** | Run all validation checks or individual test suites, format/Clippy, independent vectors, release build, TCP faults, shutdown/control/PTY regressions, workshop demo, and three benchmarks; Enter runs, `x` cancels |
| **4 Debug** | Left/right submenus for status, peer session identities, the latest 100 canonical blocks and total fork entries, mempool transactions, missing/deferred blocks, startup configuration and file paths |
| **5 Logs** | Recent node events, command results and rejected commands; full history is appended to `node.log` |
| **6 Help** | Keyboard reference and line commands |
| **7 Owned** | All parcels owned by this identity, coordinates, color swatches, acquisition time and method; Enter edits color, `g` locates on the map |
| **8 Settings** | Choose an exact mining worker count or all available CPUs; apply live and save per node |

Use **Tab / Shift-Tab** or **1–8** to change pages. Global keys: **`o`** online/offline, **`m`** start/stop mining, **`e`** export, **`:`** command prompt, **`?`** help, **`q` / Ctrl-C** quit. Enter submits commands; Esc cancels input. PgUp/PgDn scroll text, Home shows the beginning, End follows the latest output. Shortcuts are treated as text while entering a command.


Press **`8`** or **`s`** for **Settings**. Choose **Use all available** or **Use exact count**, then **Enter** to apply and save. Use **↑/↓** to select, **←/→** or **+/-** to adjust the exact count, or **`n`** to type a count. The page shows configured workers, active workers, and available logical CPUs. “All” uses the CPUs available to this process, including its CPU affinity limits. A fresh node defaults to one mining worker. Exact counts range from 1 to 1024.

Thread changes immediately replace the running mining workers. Mining that is stopped stays stopped; networking and offline mode keep their current settings. Preferences are saved to `<data-dir>/settings.json` and loaded on restart. The startup flag overrides the saved value for that run:

```bash
cargo run --release --locked -- tui --threads all
cargo run --release --locked -- tui --threads 4
```

The line CLI and `:` prompt also accept `threads all` or `threads 4`; these commands save the preference. `status` exposes `mining_thread_setting`, `mining_threads`, `active_mining_threads`, and `available_threads`. This setting controls node mining workers; the terminal/network runtime runs independently, and the existing timing benchmarks retain their reproducible single-worker workloads.

Your parcels have **thick yellow rectangular outlines**. Other parcels have thin gray outlines. The **`›` cursor stays inside the selected parcel**, with its color fill visible. Press **`h`** to toggle ownership outlines. Arrow keys move the selection and pan the viewport; **`7`**, choose a parcel, then **`g`** locates it on the map.

Press **`7`** or **`b`** for **Owned**. Each entry includes its parcel ID, zero-based **(x, y)** coordinates, current confirmed color, and the latest acquisition's **UTC confirmation time**, through **mining** or **transfer**. Transfer details include the sender and transaction ID; both methods include the confirming block and height. The history follows the canonical chain, rebuilds during peer recovery/reorganizations, and preserves acquisition dates through painting and self-transfers. A transfer away and back records a new acquisition. Pending transactions do not change confirmed ownership or color. The grid and Owned list show the latest local paint as a clearly labeled **pending preview** until it is confirmed.

Select an owned parcel with **↑/↓**, then press **Enter** or **`p`** to edit its color. Type a named color (for example `red`, `skyblue`, `orange`, or `rebeccapurple`) or an RGB hex value such as `#12ABEF`. The picker shows matching names and a live color preview: **↑/↓** chooses a suggestion, **Tab** autocompletes it, **Enter** queues the signed paint transaction, and **Esc** cancels. Enter on an empty or ambiguous prefix requires a color choice; a complete name or hex value is accepted directly. Names ignore case, spaces, hyphens, and underscores. An admitted edit immediately appears as a pending preview and returns the map to RGB view. The footer reports pending or confirmed status. When mining is stopped, it explains that **`m`** starts mining or an online peer can confirm the edit. Errors keep the picker open with the reason so you can correct or cancel the edit. A new block confirms the transaction; confirmation replaces the preview with the confirmed color. If ownership changes while the picker is open, submission is blocked.

The line command `owned` returns all currently owned parcels with coordinates and acquisition details as JSON. Named colors also work in line commands, for example `paint 181 red` or `paint 181 lightblue`; hex input continues to work. The wire protocol retains its strict lowercase RGB hex encoding.

**Offline** closes the listener, every peer transport, pending sends, and reconnect loops. It retains the current in-memory chain, pending transactions, identity, and mining setting. No peer messages are sent or received by this node while offline. Local mining and signed paint/transfer submissions still work; reconnecting resumes synchronization and pending transaction gossip. A reconnect bind error leaves the node offline and displays the error. **Stop mining** cancels and joins all its workers and rejects stale results, while leaving the network setting unchanged.

Diagnostic jobs run separately, with live output and exit status; the dashboard keeps processing node events. Full stdout/stderr is saved in `<data-dir>/diagnostics/*.log`; the UI retains the latest 2,000 output lines. Jobs stop at the first failing check. Cancel and quit terminate the job's process group on Unix, including test nodes and miners. On other platforms cancellation stops the direct child; Python PTY tests and Unix-specific workshop scenarios require Unix. Tests may create isolated **loopback nodes even when the dashboard node is offline**. The all-checks option includes real full-difficulty mining; timing benchmarks are separate menu choices.

The diagnostic menu requires the repository, Cargo with rustfmt/Clippy, Python 3, and OpenSSL on PATH. If you move the executable, set `PIXEL_WORKSPACE=/path/to/baneworkshop` to point at the source checkout. Identity persists across restarts; the chain remains in memory and recovers from surviving online peers.

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
python3 scripts/test-controls.py --binary target/release/baneworkshop
python3 scripts/test-tui.py --binary target/release/baneworkshop
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
owned
mining on
mining off
threads all
threads 4
network off
network on
paint 181 ff8000
paint 181 orange
transfer 181 <recipient-public-key>
export
export /tmp/canvas.ppm
quit
```

Use a parcel ID in `status.owned`, and replace the recipient with the other node's 64-character lowercase public key. Paint/transfer commands sign and submit a transaction; a subsequent mined block confirms it. Sequential pending commands use the next pending counter automatically. Commands fail if the identity does not own the parcel. `mining off` leaves the network setting unchanged. `network off` closes all network activity independently; `network on` reconnects. The line CLI also accepts `--offline` at startup. Closing stdin also leaves the node serving.

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
