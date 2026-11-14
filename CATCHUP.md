# Pixel Chain catch-up

Catch-up runs automatically when a node joins late, restarts, reconnects, or misses
a block announcement. It uses the existing `tip`, `get_block`, and `block` wire
messages; there is no whole-chain download or durable chain storage.

## Fetch backward, validate forward

1. A ready session exchanges `get_tip` and `tip` every two seconds. An unknown
   tip hash enters the wanted set and is requested immediately from that session.
   The advertised height is advisory and never establishes chain work.
2. A received block is known if its hash is in the validated chain or pending
   storage. Known blocks are neither requested nor relayed again.
3. A block with an unknown parent must have the network's 23-bit proof of work
   before entering pending storage. Its unknown parent enters the wanted set and
   is requested immediately from the source. A parent already pending is known
   and is not requested again.
4. Once a parent arrives, pending descendants are fully validated in forward
   order, using the same consensus checks as newly received blocks. Each valid
   block is retained and broadcast once to all ready sessions, including the
   source. Invalid descendants are removed without changing confirmed state.

For example, a node with only genesis learns about height 5, requests blocks
5, 4, 3, 2, 1, then validates and adds blocks 1 through 5. Tip exchanges continue
while downloading, so a moving tip or a missed announcement needs no special
recovery mode.

## Retries, future blocks, and limits

- Every two seconds, up to four wanted hashes are retried in a rotating order to
  **every ready session**. Removing completed requests does not skip the next
  hashes. A lost request, response, or original source therefore does not strand
  synchronization.
- Every two seconds, up to four mempool transactions are also reannounced in a
  rotating order to recover lost transaction announcements.
- A block more than 120 seconds ahead of the local clock stays pending without
  being relayed. Pending blocks are retried every two seconds and after receiving
  or mining a block. The boundary `timestamp <= now + 120` is inclusive.
- Pending storage holds at most **1024 blocks**. On overflow, the highest-height
  block is discarded, including the incoming block if it is higher than all
  retained blocks. This preserves ancestors; evicted descendants can be
  rediscovered through subsequent tips, including for histories over 1024 blocks.
- Wanted storage holds at most **1024 hashes**. A new request at capacity evicts
  the oldest request. Duplicate requests do not refresh its age. A hash leaves
  wanted storage as soon as its block enters the chain or pending storage.
- The mempool holds **4096 transactions**. A session's send queue holds **128
  messages**; overflow closes the session. Outgoing connections retry after two
  seconds. Thirty seconds without receiving a byte closes a session.

## Forks and restarts

All valid branches stay in memory and can answer `get_block`, even after they
cease to be canonical. Greater height wins; at equal height the smaller hash wins.
A change of canonical branch selects its validated state, rebuilds ownership
history, reconciles detached transactions with the mempool, and restarts active
mining on the selected tip.

Only the identity and local settings persist. The `status.json`, `state.json`,
and `canvas.ppm` exports are diagnostics and are never loaded as chain storage.
A restarted node begins at genesis and recovers from surviving peers. The same
identity recognizes its parcels again after catch-up. If every node stops, the
in-memory chain is lost.

## Verify in this project

This Rust project uses a terminal dashboard and line CLI. Debug → Chain and
Debug → Sync show the validated chain and missing/deferred blocks. `status.json`
reports the tip, height, waiting count, and state hash; `state.json` and
`canvas.ppm` allow complete state and image comparisons.

```bash
cargo test --locked
cargo build --release --locked
python3 scripts/network-fault-test.py
./scripts/workshop-demo
```

The Rust tests cover backward requests, forward replay, duplicate suppression,
orphan PoW filtering, request rotation/eviction, future-block boundaries, and
fork state/mempool recovery. The TCP fault test drops first responses, disconnects
mid-sync, and switches to a longer fork. The workshop demo uses real 23-bit mining
and lag to verify late join, SIGKILL/restart with the same identity, and final
tip/state/canvas convergence after mining stops.
