# Updated specification alignment

Reviewed on **2026-11-14** against [SPEC.md](SPEC.md) from commit **`16b4686`** (`Update spec from main`), in the current `/home/losmi/baneworkshop` checkout on `milos`.

**Result: no runtime protocol change is required.** The updated specification publishes the consensus, wire, and operational choices already implemented by the prototype. Genesis, canonical bytes, signatures/hashes, reward allocation, replay state, and peer messages remain compatible with existing prototype nodes. No chain reset, fixture regeneration, key migration, or benchmark rerun is needed for this specification update.

## Comparison

| Specification section | Implementation checked | Result |
| --- | --- | --- |
| 1 — Cryptography/text representation | `src/crypto.rs`, `src/protocol/types.rs` | Ordinary Ed25519 with strict verification/non-weak keys; SHA-256; exact-width lowercase hex; fixed-width big-endian numbers; JSON excluded from cryptographic preimages. Matches. |
| 2 — Transactions | `src/protocol/codec.rs`, `src/protocol/types.rs` | `PIXEL-TX-v1\0`; tags 1/2; `u16` parcel, `u64` counter, 32-byte sender, RGB/recipient payload, 64-byte signature. Signing messages are 58/87 bytes; signed transactions 122/151 bytes. ID domain and wire fields match. |
| 3 — State/replay | `src/state.rs`, `src/mempool.rs` | Unowned black parcels/counter 0; transactions validate in the specified order; counter follows transfers; self-transfer increments; color preserved; overflow rejected; winning branch restores complete state. Matches. |
| 4 — Blocks | `src/protocol/codec.rs`, `src/protocol/validation.rs` | Exact 137-byte header; ordered length-prefixed transaction commitment; at most 256 transactions; transactions before reward; no block signature or transmitted hash field. Matches. |
| 5 — Genesis | `src/protocol/params.rs`, `src/chain.rs`, fixtures | Exact zero fields/empty list; zero work, no reward, PoW/key exemption; hash `56ecd78b0620f10bcb47263b61ca4e5fbbfeaea4a6860ed24cc2b19d36f2e1ce`. Matches. |
| 6 — PoW/allocation | `src/protocol/validation.rs`, `src/state.rs`, `src/render.rs` | 23 bits; 64×32 RGB canvas; last-two-byte initial candidate; safe timestamp multiplication modulo 2048; 2048 attempts including the first; cycles/no reward remain valid. Matches. |
| 7 — Block validation | `src/protocol/validation.rs`, `src/node.rs` | Parent/height/miner/time/PoW/ordered transactions/reward; equal timestamps allowed; 120-second future window; provisional state cloned; future/orphan retry storage. Matches. |
| 8 — Forks | `src/chain.rs`, `src/mempool.rs`, `src/node.rs` | At fixed difficulty, height ordering is exactly accumulated-work ordering; reported work is computed as `u128(height) << 23`. Smaller raw tip hash wins a tie. Side states retained; detached transactions reconsidered in dependency order. Matches the specified outcomes. |
| 9 — TCP/wire/sync | `src/network/wire.rs`, `src/network/peer.rs`, `src/network/sync.rs`, `src/node.rs` | Exact message shapes; bounded UTF-8 JSON lines; strict fields/session rejection; hello gating and retries; explicit peer addresses/retained duplicate sessions; gossip deduplication; rotating retries/reannouncements and ancestry priority. Matches. |
| 10 — Failure/restart | `src/network/peer.rs`, `src/node.rs` | Memory-only history, persistent identity, recovery from survivors; 2-second reconnect and retry, 5-second connect/write timeouts. Matches. |
| 11 — Lag | `src/network/lag.rs`, `src/network/peer.rs`, `src/config.rs` | Default on; `--no-lag` opt-out; per-outbound-message/session 10% loss and 20–250 ms delay; FIFO delivery of surviving messages; xorshift64 seed mixed with connection sequence. Matches. |
| 12 — Local settings | `src/config.rs`, `src/crypto.rs`, `src/node.rs`, `src/render.rs` | Listen/data/key defaults, strict seed format, diagnostic exports/hash, one worker, mining off by default, and mempool/pending/session limits match. |
| 13 — Fixtures | `tests/fixtures/prototype-v1.json`, `tests/conformance.rs`, `scripts/verify-vectors.py` | Existing exact fixture bytes/signatures/hashes remain applicable, including block 1 parcel 181 and occupied-candidate fallback to 0. Independent Python/OpenSSL verification passes. |

## Changes made during this review

Runtime implementation, `SPEC.md`, fixture data, and consensus constants were left unchanged.

Two regression tests were added for requirements made explicit in the published specification:

- `json_preserves_full_width_unsigned_integers`: a wire round-trip preserves counters, heights, timestamps, and nonces above the IEEE-754 exact-integer range, including `u64::MAX`; canonical bytes remain unchanged. This checks wire representation, not state acceptance of an exhausted counter.
- `self_transfer_and_transfer_back_preserve_replay_history`: self-transfer increments the counter; transferring to B and back to A preserves color and counter; an earlier A-signed transaction still cannot replay after A regains ownership.

`README.md` and `PROTOTYPE_DEFAULTS.md` now refer to the current `SPEC.md` as the protocol contract instead of claiming its fields are still blank. `MILESTONE_STATUS.md` now records completion of the written specification and fixtures. The original audit/validation reports remain historical records of their earlier checks.

## Verification

- `cargo fmt --check` — passed.
- `cargo clippy --offline --locked --all-targets -- -D warnings` — passed.
- `cargo test --offline --locked` — **31 tests passed**.
- `python3 scripts/verify-vectors.py` — independent Python/OpenSSL check passed.

The current spec still labels sections 2–11 provisional and retains checkboxes for group confirmation, cross-language fixture checks, node speeds, and the LAN/mentor scenario. Those labels were preserved. The remaining `____` entries are participant names/speed measurements in section 6, not missing consensus or wire definitions. This review establishes alignment of the Rust implementation with the published rules; it does not establish interoperability with other implementations or mentor acceptance.
