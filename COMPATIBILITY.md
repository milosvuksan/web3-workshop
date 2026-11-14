# Cross compatibility: Grupa 1 - Luka

Open **9 Network**, press **a**, and enter a server IP and optional port and alias. Use **Tab** to focus **Cross compatibility**, **Space** to open its dropdown, and **↑/↓** or **←/→** to select **Grupa 1 - Luka**. **Enter** closes the open dropdown; another **Enter** saves the peer. Leaving it at **None (native)** uses the original protocol. You can also cycle the collapsed selector with the arrows. Edit existing peers the same way. Save any number of peers up to the existing 256-peer limit, mixing native and compatibility profiles.

A bare Grupa 1 IP defaults to **8080**. An explicit port is preserved. `http://IP:PORT` is also accepted. Use their **server** IP, not an address copied from a client log. Existing saved peers without a profile remain native.

The equivalent line commands (also available through the TUI's `:` prompt) are:

```text
peer add 192.168.1.20 --compat grupa1-luka Luka laptop
peer add 192.168.1.21:8080 --compat grupa1-luka Second Grupa 1 node
peer edit 192.168.1.20:8080 192.168.1.20:8080 --compat native Native again
```

The option goes before the alias. Editing a peer without the option preserves its current profile. Profiles and aliases persist in `settings.json`. Native startup targets at the same address are overridden by the saved compatibility profile. Updates reconnect only affected outgoing transports; offline updates open no sockets.

## Two chains with different consensus

Grupa 1 uses a zero genesis ID, a 256-parcel canvas, decimal text hashes, Ed25519 text signatures, sender-wide nonces starting at zero, 20-bit proof of work, last-byte rewards without probing, and the lexicographically **larger** hash on equal-height forks. These differ from the native chain's rules. Renaming fields cannot preserve signatures or proof of work.

The profile therefore maintains an independent, validated Grupa 1 ledger and transaction pool. Native blocks and signed native transactions are never converted into foreign blocks or transactions. The existing canvas, Owned page, `paint`, `transfer`, and `m` mining shortcut continue to operate on the native chain. The Network page and `status.compatibility` show the Grupa 1 height and pending transactions. `grupa1-state.json` exports its full ownership, color, and sender-nonce state.

Use these commands for actions on Grupa 1's chain, with this node's existing Ed25519 identity:

```text
grupa1 status
grupa1 mining on
grupa1 mining off
grupa1 paint 223 orange
grupa1 transfer 223 <recipient-public-key>
```

Only parcels actually owned by this identity in the Grupa 1 ledger can be painted or transferred. `grupa1 status` lists their IDs. The example parcel ID is illustrative. Grupa 1 mining uses one separate worker at their required 20-bit difficulty; it starts stopped. Native mining settings and the native mining switch do not control this worker. Quit stops both miners. Both chains retain state while offline, and both are in memory only; restart requires surviving peers for recovery.

## HTTP transport

For this profile, outgoing connections send HTTP requests without `hello`, `version`, identity exchange, or authentication. The node polls `/tip` about every two seconds, walks unknown ancestors backward, verifies the hashes and heights, and applies blocks oldest first after finding a known ancestor. It publishes canonical blocks in order and rotates signed pending transactions across all configured Grupa 1 peers. Retry after failure and remote rollback recovers synchronization. Status shows HTTP sync success as **Connected**, failures as **Retrying**, and the most recent error. Grupa 1 sessions have no exchanged identity.

For two-way synchronization, the same listener serves these routes alongside native TCP traffic:

| Route | Behavior |
| --- | --- |
| `GET /tip` | Grupa 1 height and hash |
| `GET /blocks/{hash}` | Direct Grupa 1 block, or 404 |
| `POST /blocks` | Direct block; 200 known/connected, 202 waiting for parent, 400 invalid |
| `POST /transactions` | Direct signed transaction; 202 accepted, 400 invalid |

Tell the other team to add `--peer http://YOUR_SERVER_IP:YOUR_CONFIGURED_PORT`. This is **your node's configured listening port** (7001 by default), which need not equal their port 8080. For example:

```bash
curl -i http://YOUR_SERVER_IP:7001/tip
```

POST requests use UTF-8 JSON, `Content-Type: application/json`, and `Content-Length`. Requests use no token. Going offline closes both transports and all polling, preserving local state and mining choices. This node closes its listener rather than serving 503 while offline.

HTTP bodies are limited to 1 MiB, headers to 16 KiB, requests to five seconds, missing-parent storage to 128 blocks, and pending transactions to 1024. Catch-up fetches at most 64 ancestors per pass and keeps its cursor across passes; a backward walk is limited to 32 MiB before state validation. State-invalid orphan blocks are discarded when their parent arrives. JSON rejects unknown/duplicate fields and noninteger numbers; heights/nonces are bounded at 2^53 − 1.

## Verification

The supplied block with nonce `1426967` is tested against hash `000008a6403b5a52eca437a0912dc7a1d788b2d0e093b3ee72667e1636a024df` and reward parcel 223. Additional fixtures independently generated with Python SHA-256 and OpenSSL Ed25519 cover painting, transferring, sender nonces, orphan replay, and forks. Regenerate with:

```bash
python3 scripts/generate-grupa1-vectors.py
cargo test --locked
```

Run `python3 scripts/test-compatibility.py --binary target/release/baneworkshop` for a real node against independent Python HTTP servers, including OpenSSL verification of locally signed paint and transfer transactions.

Loopback tests exercise all four HTTP routes, malformed inputs, native/HTTP multiplexing, multiple compatibility peers, bidirectional block and signed transaction exchange, and shutdown. The tests do not contact the other team's real server.
