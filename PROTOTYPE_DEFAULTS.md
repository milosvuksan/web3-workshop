# Prototype defaults for team review

These choices fill the blanks in `SPEC.md` under the implementation request's authorization to choose working defaults. They are **provisional**, not group approval. The supplied `SPEC.md` is preserved byte for byte. Other language implementations must agree on these choices and verify the fixtures before joining this prototype chain. Changes to consensus choices require a new chain identity and regenerated vectors.

The accepted Ed25519, SHA-256, lowercase hex, 64×32 canvas, TCP full mesh, 23-bit difficulty, multiplication-based parcel retry, one reward per block, fork tie-break, and default-on lag are unchanged.

## Canonical transaction bytes (consensus)

Integers are unsigned, fixed width, big-endian. Signing uses ordinary Ed25519, not Ed25519ph. Hashes use SHA-256. All public keys, hashes, RGB colors, and signatures are exact-length **lowercase hexadecimal strings** at text boundaries; there is no `0x` prefix. JSON is a transport representation, never the signing or hash preimage.

| Order | Field | Bytes | Value |
| --- | --- | --- | --- |
| 1 | Domain | 12 | ASCII `PIXEL-TX-v1` followed by one zero byte |
| 2 | Action tag | 1 | Paint = 1, transfer = 2 |
| 3 | Parcel | 2 | ID 0–2047 |
| 4 | Counter | 8 | Next counter for this parcel |
| 5 | Sender | 32 | Ed25519 public key |
| 6 | Payload | 3 or 32 | Paint: red, green, blue bytes; transfer: recipient public key |

The concatenation above is the message signed. Append the 64-byte signature to form the canonical signed transaction (122 bytes for paint, 151 for transfer). Transaction ID = SHA-256(`PIXEL-ID-v1` + zero byte + canonical signed transaction).

Wire transaction shape:

```json
{"parcel":181,"counter":0,"sender":"<64 hex chars>","action":{"type":"paint","color":"ff8000"},"signature":"<128 hex chars>"}
```

A transfer replaces `action` with `{"type":"transfer","recipient":"<64 hex chars>"}`. No extra fields are accepted. JSON integer readers in other languages must preserve all 64 bits exactly; JavaScript `Number` is insufficient for arbitrary counters, timestamps, heights, and nonces.

## Replay and state transitions (consensus)

Every parcel starts unowned, black (`000000`), with counter 0. The origin is `(0,0)` at the upper left. Rendering is row-major, RGB, with no alpha. Allocating a reward assigns the miner as owner; it does not paint the pixel.

Transactions execute in listed block order **before** reward allocation. A newly rewarded parcel cannot be used in that same block. Transfer changes ownership and preserves color and counter. Self-transfer is allowed and increments the counter.

For each transaction, validate in this order:

1. Parcel is in range (wire widths/hex/color lengths were already decoded strictly).
2. Sender key is a decodable, non-weak Ed25519 key and its signature passes strict verification.
3. Transfer recipient is a decodable, non-weak Ed25519 key.
4. Sender owns the parcel in the provisional state at this point in the block.
5. Transaction counter equals that parcel's next counter; increment must not overflow `u64`.

Only then apply the action and increment the counter. The counter follows the parcel across every transfer, including transfer back to an earlier owner. This rejects repeated inclusion, replay after transfer, and two uses of one counter. A transaction at `u64::MAX` is rejected; that parcel is frozen. Reorganization restores the complete winning branch's state, including counters.

Reward selection tries **2048 candidates total**, including the initial candidate. Candidate 0 is a valid parcel. On occupation use `(candidate * (timestamp mod 2048)) mod 2048`, which is mathematically equivalent to the agreed product without overflow. Preserve cycles: exhausting retries or a full canvas gives no reward but leaves the block valid. Do not substitute the additive walk.

## Canonical block bytes and genesis (consensus)

The transaction commitment is SHA-256 of the ordered concatenation of `u16 canonical_transaction_length || canonical_signed_transaction` for every transaction. Empty-list commitment = SHA-256 of the empty string. There is no Merkle tree.

| Order | Field | Bytes |
| --- | --- | --- |
| 1 | ASCII `PIXEL-BLOCK-v1` plus one zero byte | 15 |
| 2 | Previous block hash | 32 |
| 3 | Height | 8 |
| 4 | Miner public key | 32 |
| 5 | Unix timestamp, seconds | 8 |
| 6 | Transaction count | 2 |
| 7 | Transaction commitment | 32 |
| 8 | Nonce | 8 |

The 137-byte concatenation is the SHA-256 preimage. The transaction list travels with the block on the wire and the receiver recomputes the commitment. Maximum 256 transactions per block. There is no block signature or separate hash field in the wire block.

Genesis has zero previous hash, height 0, all-zero miner bytes, timestamp 0, nonce 0, and an empty transaction list. Only this exact block is genesis. It is exempt from PoW and miner-key validation, gives no reward, and contributes zero work. Its hash is:

```text
56ecd78b0620f10bcb47263b61ca4e5fbbfeaea4a6860ed24cc2b19d36f2e1ce
```

The full genesis bytes and expected hash are in `tests/fixtures/prototype-v1.json`.

For non-genesis blocks validate: transaction limit → known parent / matching parent hash → parent height + 1 without overflow → valid non-weak miner key → timestamp >= parent timestamp → timestamp <= receiver's Unix time + 120 seconds → 23-bit PoW → every transaction sequentially → reward. Equal timestamps are allowed. Future blocks are deferred and retried when eligible, not permanently blacklisted. Unknown-parent blocks are pending, not classified as invalid. State is cloned before executing transactions; any failure rejects the entire block without mutation.

Every non-genesis block contributes exactly `2^23` units of work, independent of extra leading zero bits. Chain work = height × `2^23`, represented as `u128`. Greater work wins; ties choose the smaller raw 32-byte tip hash. Keep all valid side branches in memory. Reorganization reconsiders detached transactions alongside the existing mempool in dependency order, retains only actions valid on the new state, and removes transactions already confirmed/conflicting there.

## Wire protocol (prototype network agreement)

TCP, UTF-8 JSON objects, one object per newline. JSON member order and insignificant whitespace do not affect consensus. Limit 262,144 bytes before the newline. Invalid UTF-8, malformed JSON, bad fixed-width fields, extra fields, unknown message types, and oversized lines close that TCP session. Consensus-invalid transactions/blocks are ignored; invalid blocks are logged. Incomplete lines are buffered within the limit. Idle sessions time out after 30 seconds without bytes.

| `type` | Other fields | Purpose / response |
| --- | --- | --- |
| `hello` | `version:1`, `genesis:<hash>`, `identity:<public key>` | Chain/version identification; required before processing other messages |
| `get_tip` | none | Reply `tip` |
| `tip` | `hash:<hash>`, `height:<u64>` | Discover an unknown tip; height is advisory, never trusted as work |
| `get_block` | `hash:<hash>` | Reply `block` when known; no reply when unknown |
| `block` | `block:<object>` | Fields: previous, height, miner, timestamp, nonce, transactions |
| `transaction` | `transaction:<object>` | Signed paint or transfer |

`hello` with a different version/genesis, a malformed/weak identity, or our own identity rejects the session. Identity here is discovery metadata, not an authenticated transport login; block/transaction validation is authoritative. Hello messages repeat every 2 seconds, even after local acceptance, so a dropped handshake cannot strand the other side. A first accepted hello also elicits our hello and tip. Every ready session exchanges tip requests/announcements every 2 seconds.

Addresses are explicit `--peers` socket addresses, repeated or comma-separated. Every configured node should list every other node. Duplicate configured addresses are collapsed. Simultaneous inbound and outbound sessions are both retained (normally two TCP sessions per peer pair); block/transaction deduplication prevents echo loops. `status.connections` counts sessions, not unique identities. No peer auto-discovery or sparse-topology guarantees.

New valid blocks, including side branches, and newly admitted transactions forward once to all ready sessions, including the origin. Known blocks/transactions do not forward again. Tip anti-entropy recovers missed block announcements. Missing ancestry is fetched backward, then validated forward. Outstanding missing hashes are retried every 2 seconds, sent to every ready peer so a disappearing source does not stall recovery. Up to four outstanding hashes are retried per tick with a rotating cursor; receiving an orphan also immediately requests its parent from that source. Up to four mempool transactions are reannounced per tick with a rotating cursor, recovering lost transaction announcements and out-of-order dependent transactions.

## Local operational defaults (not consensus)

| Setting | Default / rationale |
| --- | --- |
| Listen address | `127.0.0.1:7001`; explicit LAN bind needed for workshop machines |
| Peer input | IP socket addresses; explicit full mesh, no DNS discovery |
| Data directory | `data/node`; use a unique directory per running process |
| Identity | `identity.seed` within data directory, automatically created from OS randomness; `--key PATH` explicitly loads an existing seed |
| Seed file | 64 lowercase hex digits, optional trailing newline; create-new and sync; Unix mode 0600; reject broader Unix permissions; never print private bytes |
| Chain storage | Memory only; restarts download from surviving peers; JSON/PPM exports are diagnostics and are never trusted as recovery input |
| Diagnostics | Atomically replaced `status.json`, deterministic `state.json`, and 64×32 binary P6 `canvas.ppm`, refreshed every 2 seconds and after local commands |
| Diagnostic state hash | SHA-256 of row-major parcels: owner-present `u8`, owner bytes (zero when absent), RGB 3 bytes, next counter `u64` big-endian; not a consensus field |
| Mining | Off until `--mine` or `mining on`; one dedicated CPU worker; fixed 23-bit target, no reduced-difficulty CLI mode |
| Cancellation | Check at most every 16,384 hashes; new tip or changed mempool replaces the candidate; stale results fail generation/parent checks and accepted results undergo full validation |
| Mempool | 4096 entries; arrival order, state-aware admission/selection, 256 selected per block; local commands reserve pending counters |
| Unknown/future blocks | At most 1024 pending blocks and 1024 wanted hashes, in memory; on overflow prefer lower-height ancestors, and rediscover evicted descendants from missing-parent/tip exchange |
| Queues | 1024 coordinator events; 128 outbound messages per session; a full/closed outbound queue drops that session from the coordinator; reconnect/catch-up repairs it |
| Connection limits | At most 128 incoming sessions; configured outgoing connections separately bounded by configured peer count |
| Timeouts/reconnect | 5-second connect/write timeouts; reconnect after 2 seconds; 2-second tip/request retry interval; 30-second read-idle timeout |
| Lag | On by default; disable explicitly using `--no-lag` |
| Simulation | Independently per outbound message and TCP session, including hello, requests, responses, broadcasts; 10% drop, uniform integer delay 20–250 ms, surviving messages preserve FIFO order within a session |
| Simulation randomness | Seeded xorshift64, `--lag-seed 1` by default, mixed with connection sequence; reproducible for the same session/event order; never used for keys |
| Console | stdin commands; EOF leaves the node serving peers; `quit` or Ctrl-C shuts down; stdout responses, stderr lifecycle/rejection logs |

The node is a workshop prototype: valid chain history and its state snapshots remain in memory without pruning. Pending work and per-peer queues are bounded, but indefinitely long chains require a storage/pruning design. Losing all peers and restarting all nodes loses the chain; keep at least one survivor for the required recovery scenario. Run one process per data directory. Optional swaps, delegation, dynamic difficulty, sparse topology, and durable chain storage are not implemented.

## Fixtures and review status

`examples/generate_vectors.rs` deterministically generates the nine requested fixture categories plus additional fork/invalid-block fixtures. Public test seeds are intentional and must never be used for real identities. Block 1 mines parcel **181**. The occupied-candidate example uses that occupied parcel and timestamp 0, so the next candidate is parcel 0. The wrong-signer vector has a cryptographically valid B signature but fails ownership against A's parcel.

`tests/conformance.rs` verifies fixture bytes/hashes/signatures and RFC 8032 / SHA-256 standard vectors. `scripts/verify-vectors.py` independently implements the binary encodings in Python, checks hashes/PoW/rewards, and verifies keys/signatures through OpenSSL. This is useful independent verification, **not** validation against another team's network implementation.

Team decisions still needed: accept or replace each provisional consensus/wire choice above; verify the published vectors in the other languages; run the LAN/full-team and mentor scenarios. Cross-language workshop milestones are not claimed complete by local Rust tests.
