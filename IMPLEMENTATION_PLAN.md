# Pixel Chain: Rust implementation plan

Target implementation branch: `milos`.

This is an implementation plan, not an implementation or a revision of the group protocol. It uses `original_requirements.md` for project scope and `SPEC.md` for protocol decisions. Per the user's instruction, every decision marked `predlog` is accepted. Remaining blank fields are still unresolved; Rust-specific choices must not silently become protocol rules for the other implementations.

## 1. Starting point and scope

Repository inspection found:

- `main` contains the supplied `SPEC.md`.
- `milos` contains a Rust 2024 package with a `Hello, world!` entry point and no dependencies. It does not currently contain `SPEC.md`.
- The working directory is on `main` and has untracked files, including benchmarks and IDE files. Some paths overlap files tracked by `milos`. Preserve them during branch setup.

Deliver the required decentralized node: keys and signatures, mining, ownership and painting, validation and replay protection, direct connections to every configured peer, fork resolution, catch-up, failure recovery, canvas display, and default-on simulated network lag.

Keep optional workshop goals outside the initial delivery: atomic swaps, delegated painting, variable difficulty, sparse topology, and durable chain storage. Key persistence is required for a usable identity across restarts. A node can satisfy the required restart test by retaining its key and downloading the chain again from surviving peers; offline chain recovery is an optional extension.

## 2. Accepted protocol decisions

| Area | Implementation constraint |
| --- | --- |
| Signatures and hashes | Ed25519; SHA-256 |
| Text representation of bytes | Lowercase hexadecimal |
| Transport and topology | TCP; each node connects directly to every other node |
| Framing | One text message per line, terminated by `\n` |
| Canvas | 64 columns × 32 rows; parcel IDs 0–2047; `x = p % 64`, `y = p / 64` |
| Proof of work | 23 leading zero bits: `hash[0] == 0`, `hash[1] == 0`, `hash[2] < 2` |
| First reward candidate | Last two hash bytes, big-endian, modulo 2048 |
| Reward | At most one new parcel per block |
| Occupied candidate | Retry using `(previous_candidate * block_timestamp) % 2048` |
| Full canvas / exhausted retries | Block remains valid; no parcel reward |
| Fork choice | Most accumulated work; equal work chooses the lexicographically smaller tip hash |
| Lag | Enabled by default, with an explicit option to disable it |

The alternative additive parcel walk in section 6 is an example in an open-question note, not a decision marked `predlog`. Preserve the agreed multiplication rule unless the group explicitly changes it. Its cycles can leave free parcels undiscovered; tests must cover this behavior. The retry bound still needs a precise definition.

## 3. Protocol completion gate

Before implementing protocol-dependent encoding, validation, or networking, complete the following decisions in the shared specification. Independent Rust scaffolding and standard cryptographic tests can proceed meanwhile.

| Area | Decisions needed before implementation |
| --- | --- |
| Transaction encoding | Exact ordered fields, widths, integer byte order, type tags, parcel encoding, color encoding, sender/recipient representation, bytes signed, and transaction-ID preimage |
| Replay protection | Mechanism, exact acceptance rule, scope of counters or identifiers, behavior after transfers, duplicate inclusion in one block, and rollback on a fork change |
| Block encoding | Exact ordered fields and widths, previous hash, height if present, miner identity, timestamp unit, nonce width, transaction commitment, and transaction count/size limits |
| State transition order | Whether transactions execute before or after reward allocation; whether a newly awarded parcel can be used in the same block; ownership and color behavior on transfer |
| Genesis | All exact fields/bytes, expected hash, PoW exemption or requirement, initial state, and reward behavior |
| Parcel allocation | Meaning of the top-left origin, initial color, retry bound, whether the initial candidate counts toward that bound, and safe arithmetic matching the mathematical formula |
| Validation | Exact ordered rejection rules, malformed key/signature handling, timestamp constraints, and whether future-dated blocks are rejected permanently or retried later |
| Fork bookkeeping | Work representation, whether genesis contributes work, side-branch retention, and treatment of transactions detached by reorganization |
| Wire protocol | JSON versus plain text, text encoding, message schemas, handshake/chain identification, missing-block requests/responses, tip discovery, size limits, and unknown-message behavior |
| Peer operation | Address configuration, duplicate connection handling, forwarding rules, reconnect policy, response timeouts, and periodic catch-up behavior |
| Lag simulation | Delay range/distribution, drop probability, application per message/peer, ordering semantics, and the enable/disable interface |
| Shared vectors | Complete the nine project fixtures in section 12 with exact bytes, signatures, hashes, PoW-valid blocks, and parcel outcomes |

Completion means that every `____` is resolved or explicitly declared non-applicable by the group, the section 6 ambiguity is settled, and another language implementation agrees with the shared vectors. Keep local operational choices, such as the key-file location, separate from consensus rules.

Do not substitute JSON serialization for canonical hash/signature bytes unless the completed specification explicitly defines it that way. Do not use Rust memory layouts or platform-native integer encoding as the wire or cryptographic format.

## 4. Rust architecture

Use one library crate for deterministic protocol/state logic and a small executable for configuration, input, networking, and rendering. Keep the existing Rust 2024 package on `milos`.

Suggested layout:

```text
src/
  lib.rs
  main.rs
  config.rs              # local settings and peer addresses
  crypto.rs              # keys, signatures, hashing, seed-file handling
  protocol/
    types.rs             # fixed-size keys/hashes, parcel IDs, transactions, blocks
    codec.rs             # exact agreed bytes and wire conversion
    validation.rs        # ordered, explicit validation errors
    params.rs            # agreed chain constants and genesis
  state.rs               # parcel ownership, colors, and replay state
  chain.rs               # block tree, work, best tip, reorganization
  mempool.rs             # pending transactions and state-aware selection
  mining.rs              # candidate construction and cancellable PoW workers
  node.rs                # owns live state and processes events
  network/
    wire.rs              # framing and agreed message schemas
    peer.rs              # connections, bounded queues, reconnects
    sync.rs              # tips, missing ancestors, retries
    lag.rs               # seeded delay/drop scheduler
  cli.rs                 # key, node, paint, transfer, status, mining control
  render.rs              # deterministic canvas output
tests/
  fixtures/              # shared protocol vectors, with provenance
  conformance.rs
  consensus.rs
  network.rs
  recovery.rs
scripts/
  workshop-demo          # reproducible multi-process final test
```

Use fixed-size byte types at protocol boundaries and checked conversions for IDs, lengths, and integers. Select maintained dependencies for cryptography, the networking runtime, CLI parsing, and any agreed wire/image formats during setup; pin resolved versions in `Cargo.lock`.

One coordinator owns the block tree, active canvas, replay state, and mempool. Network tasks send it bounded events. Dedicated CPU workers perform mining and submit candidates back for full validation. A new best tip invalidates stale mining work. Avoid shared mutable consensus state across peer and mining tasks.

A block is validated against its own parent's state, including when it extends a side branch. Failed validation must leave the committed state unchanged. For this workshop-sized canvas, begin with state snapshots or replay from a common ancestor; optimize only if measurements justify it.

## 5. Implementation sequence and acceptance criteria

### Phase 0 — Branch preparation and tools (workshop milestone 0)

1. Preserve the current untracked files. Prefer a separate worktree for the existing `milos` branch so this checkout is not disturbed; do not force-switch, clean, or reset it.
2. Bring the supplied `SPEC.md` into the implementation branch deliberately, reviewing the branch difference first. Keep its protocol content unchanged during planning.
3. Add the library/module skeleton, local configuration, structured errors, and CLI foundation.
4. Implement key generation, explicit key loading, signing/verifying raw bytes, and SHA-256. Persist the seed in a local file with owner-only access where supported; avoid logging it or committing generated keys.
5. Add standard SHA-256 `abc` and Ed25519 RFC 8032 tests from the supplied spec, including verification of the standard signature and a changed-message rejection.

Done when the Rust package builds on `milos`, key generation/reloading works, and standard cryptographic vectors pass. Protocol completion can happen in parallel with this phase, but later dependent phases wait for it.

### Phase 1 — Exact protocol and state transitions (milestone 1 gate)

1. Implement the agreed canonical transaction/block encoders and separate wire decoders.
2. Add the exact genesis fixture and all shared cross-language vectors before building networking.
3. Implement painting, transfer, ownership checks, replay protection, and parcel allocation as deterministic state transitions.
4. Validate transactions in block order against provisional state. Apply rewards in the agreed order. Commit only after the entire block passes.
5. Cover malformed lengths, invalid IDs/colors, wrong signatures/owners, duplicate transactions, counter boundaries if applicable, occupied-parcel cycles, exhausted retries, and a full canvas.

Done when canonical bytes, signatures, block hashes, and resulting parcel state match the shared fixtures exactly. Include sequential transactions where the second depends on the first, and verify that one invalid transaction rejects the whole block without partial mutation.

### Phase 2 — Standalone node and canvas (milestone 2)

1. Implement candidate construction, nonce search, PoW validation, and reward allocation at the fixed production difficulty of 23 bits.
2. Mine outside the network/coordinator thread. Cancel or rebuild candidates when the best tip changes, and revalidate worker results before acceptance.
3. Add mempool admission/deduplication and transaction selection against the candidate parent state. Track pending local replay counters correctly if the agreed mechanism uses them.
4. Expose commands to inspect identity/status, paint an owned parcel, transfer it, start/stop mining, and export the canvas.
5. Render a deterministic image, such as a 64×32 PPM, using the agreed coordinate/color semantics. Show ownership separately in status output if useful.
6. Measure release-build hashing using the actual block encoding. Report expected group block time as approximately `2^23 / combined_hashes_per_second`; do not change the agreed difficulty locally if it is faster or slower than the workshop's suggested range.

Done when a standalone node mines valid blocks, receives parcels, paints and transfers them using signed transactions, rejects invalid actions, and exports the expected canvas. Use fixtures for most tests; keep any reduced-difficulty test profile isolated from the workshop chain.

### Phase 3 — Forks and reorganizations (required for milestones 3–4)

1. Store valid blocks by hash with parent links, height where applicable, accumulated work, and enough state to validate side branches.
2. Select tips by work and then the specified raw-byte hash ordering. At fixed difficulty, use the agreed constant per-block work; a rarer-looking valid hash does not earn extra work.
3. Switch active state to the winning branch, including ownership, colors, reward allocation, and replay protection.
4. Remove confirmed mempool transactions; reconsider detached transactions under the agreed policy and the new state. Do not blindly replay all detached transactions.
5. Keep required side branches and retry blocks whose ancestors arrive later. Bound pending unknown-parent storage without treating an unknown parent as proof of an invalid block.

Done when tests cover equal-work arrival-order independence, a longer winning branch, invalid side branches, reorganization across transfers/paints, restored replay state, and stale mining cancellation.

### Phase 4 — Two-peer networking with lag (milestone 3)

1. Implement the agreed newline-framed protocol with bounded input buffers, partial-read handling, multiple messages per read, and explicit malformed-message behavior.
2. Connect directly to configured peers and support simultaneous inbound/outbound connections according to the agreed duplicate policy.
3. Route every outbound protocol message through the default-on lag scheduler, including requests, replies, and tip announcements. Do not delay the coordinator with blocking sleeps.
4. Exchange transactions and blocks, deduplicate them, and implement missing-parent retrieval.
5. Add request retries and periodic tip comparison: one dropped broadcast or response must not prevent eventual catch-up. Apply the agreed forwarding policy without endless echo loops.
6. Test Rust-to-Rust locally, then connect to another team member's implementation using the same genesis and fixtures.

Done when two independently implemented nodes agree on the tip, ownership, replay state, and colors with lag enabled. A Rust-only test is useful but does not complete the workshop's cross-language milestone.

### Phase 5 — Full mesh, late join, and restart (milestones 4–5)

1. On startup, load or generate the configured identity, establish peer connections, discover tips, fetch missing ancestry, and validate the recovered history from genesis.
2. Synchronize in dependency order with bounded in-flight requests. Retrieve missing blocks again after drops/timeouts, and try another peer when one disappears.
3. Keep serving healthy peers while another is slow, disconnected, or reconnecting. Use bounded queues and backpressure so lag does not cause unlimited memory growth.
4. Reconnect according to the agreed policy and resume synchronization after interruption. Do not assume a connected peer has the best valid chain.
5. Restart with the same key and recover the chain from peers. Add on-disk chain storage only after the required recovery path is reliable.

Done when every team node participates in a direct full mesh, a late node catches up, killing any single process leaves the others operating, and the restarted process recovers its identity and converges again.

### Phase 6 — Workshop demonstration and handoff (milestone 6)

1. Supply example local configurations, independent node data directories/ports, command instructions, and a repeatable multi-process demo script.
2. Start all nodes with lag enabled. Mine, paint, and transfer parcels.
3. Kill one process forcibly while its peers continue working. Restart it with the same identity and wait for catch-up.
4. Stop transaction creation and mining on every node while leaving networking/synchronization active. Drain pending messages and allow retries to finish.
5. Compare genesis identity, final tip hash, canvas ownership/colors, and replay state across all nodes. Use a deterministic diagnostic state export; a locally invented state checksum is not a consensus field.

Done when the mentor's complete final scenario passes, the commands are documented, and no required behavior depends on manually disabling lag.

## 6. Verification and review checkpoints

- Run formatting, linting, and the relevant Rust tests for each code change; run the full suite at milestone boundaries: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
- Use deterministic fixture and fork tests for consensus rules. Inject time and seeded lag randomness in integration tests so failures can be reproduced.
- Exercise loss explicitly: dropped transaction/block announcements, dropped requests/responses, reconnect during catch-up, unknown-parent arrival, and delayed messages after a reorganization.
- Run a release-build mining smoke test at the actual 23-bit difficulty, in addition to fast fixture-based tests.
- Run multi-process crash/restart tests and cross-language checks before declaring interoperability complete.
- Keep phase-sized commits on `milos`. Do not commit private keys, generated canvases, local runtime data, or benchmark build artifacts.

The critical path is protocol completion → exact vectors and transitions → standalone chain → fork handling → lagged peer exchange → full-mesh recovery → final convergence. Network framing/scaffolding can proceed independently after the wire schema is settled, but interoperability is not established until consensus vectors and cross-language tests pass.

## 7. Requirement coverage

| Original requirement | Delivery phase |
| --- | --- |
| 1. Generate keys, sign and verify | 0–1 |
| 2. PoW and hash-derived rewards | 1–2 |
| 3. Block/transaction validation and replay protection | 1–3 |
| 4. Transaction/block exchange; direct full mesh | 4–5 |
| 5. Fork resolution and late catch-up | 3–5 |
| 6. Sudden peer failure/departure | 5–6 |
| 7. Painting and ownership transfer | 1–2 |
| 8. Display the canvas | 2 |
| 9. Default-on random message delay/loss | 4–6 |

The plan is ready for implementation preparation. The remaining shared protocol decisions in section 3 are the prerequisite for an interoperable implementation; accepting the existing `predlog` entries alone does not resolve those blanks.
