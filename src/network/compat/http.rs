//! Bounded HTTP/1.x transport; one request per connection, no identity handshake.
use super::{
    Compatibility, PeerStatus,
    grupa1::{self, Block, Transaction},
};
use crate::{Error, Result, protocol::Hash};
use serde_json::{Value, json};
use std::{collections::BTreeSet, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

const MAX_BODY: usize = 1024 * 1024;
const MAX_HEADERS: usize = 16 * 1024;
const BATCH: usize = 64;
struct Head {
    first: String,
    length: Option<usize>,
    json: bool,
}
async fn read_head(stream: &mut TcpStream) -> Result<Head> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= MAX_HEADERS {
            return Err(Error::Invalid("HTTP headers too large"));
        }
        bytes.push(stream.read_u8().await?);
    }
    let head = std::str::from_utf8(&bytes).map_err(|_| Error::Invalid("invalid HTTP headers"))?;
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or_default().to_owned();
    let mut length = None;
    let mut is_json = false;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or(Error::Invalid("invalid HTTP header"))?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::Invalid("invalid HTTP Content-Length"));
            }
            let size: usize = value
                .parse()
                .map_err(|_| Error::Invalid("invalid HTTP Content-Length"))?;
            if size > MAX_BODY {
                return Err(Error::Invalid("HTTP body too large"));
            }
            length = Some(size);
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(Error::Invalid(
                "HTTP Transfer-Encoding is unsupported; use Content-Length",
            ));
        }
        if name.eq_ignore_ascii_case("content-type") {
            is_json = value
                .split(';')
                .next()
                .is_some_and(|s| s.trim().eq_ignore_ascii_case("application/json"));
        }
    }
    Ok(Head {
        first,
        length,
        json: is_json,
    })
}
async fn read_body(stream: &mut TcpStream, length: Option<usize>) -> Result<Vec<u8>> {
    if let Some(length) = length {
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await?;
        Ok(bytes)
    } else {
        let mut bytes = vec![];
        stream
            .take((MAX_BODY + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > MAX_BODY {
            return Err(Error::Invalid("HTTP body too large"));
        }
        Ok(bytes)
    }
}
async fn write_response(stream: &mut TcpStream, status: u16, body: Value) -> Result<()> {
    let body = serde_json::to_vec(&body)?;
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        404 => "Not Found",
        _ => "Bad Request",
    };
    stream.write_all(format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await?;
    Ok(())
}
async fn route(stream: &mut TcpStream, state: &Compatibility) -> Result<(u16, Value)> {
    let head = read_head(stream).await?;
    let parts: Vec<_> = head.first.split_whitespace().collect();
    let [method, path, version] = parts.as_slice() else {
        return Err(Error::Invalid("invalid HTTP request line"));
    };
    if !matches!(*version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(Error::Invalid("unsupported HTTP version"));
    }
    match (*method, *path) {
        ("GET", "/tip") => {
            let ledger = state.ledger.lock().unwrap();
            Ok((
                200,
                json!({"height":ledger.head().block.height, "hash":ledger.tip}),
            ))
        }
        ("GET", path) if path.starts_with("/blocks/") => {
            let hash: Hash = path[8..].parse()?;
            let ledger = state.ledger.lock().unwrap();
            Ok(match ledger.entries.get(&hash) {
                Some(entry) => (200, serde_json::to_value(&entry.block)?),
                None => (404, json!({"error":"block not found"})),
            })
        }
        ("POST", "/blocks" | "/transactions") => {
            if !head.json || head.length.is_none() {
                return Err(Error::Invalid(
                    "POST requires application/json and Content-Length",
                ));
            }
            let body = read_body(stream, head.length).await?;
            // Deserialize the original bytes so duplicate and unknown fields fail.
            if *path == "/blocks" {
                let block: Block = serde_json::from_slice(&body)?;
                let status = state.ledger.lock().unwrap().accept(block)?;
                Ok((status, json!({"accepted":true})))
            } else {
                let tx: Transaction = serde_json::from_slice(&body)?;
                state.ledger.lock().unwrap().admit(tx)?;
                Ok((202, json!({"accepted":true})))
            }
        }
        _ => Ok((404, json!({"error":"route not found"}))),
    }
}
pub async fn serve(mut stream: TcpStream, state: Arc<Compatibility>) {
    let _ = timeout(Duration::from_secs(5), async {
        let (status, body) = match route(&mut stream, &state).await {
            Ok(result) => result,
            Err(error) => (400, json!({"error":error.to_string()})),
        };
        write_response(&mut stream, status, body).await
    })
    .await;
}
async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>)> {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await?;
        let body = body.unwrap_or_default();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
        stream.write_all(body).await?;
        let head = read_head(&mut stream).await?;
        let mut words = head.first.split_whitespace();
        if !matches!(words.next(), Some("HTTP/1.0" | "HTTP/1.1")) { return Err(Error::Invalid("invalid HTTP response")); }
        let status: u16 = words.next().and_then(|s| s.parse().ok()).ok_or(Error::Invalid("invalid HTTP status"))?;
        Ok((status, read_body(&mut stream, head.length).await?))
    }).await.map_err(|_| Error::Invalid("Grupa 1 HTTP request timed out"))?
}
#[derive(serde::Deserialize)]
struct Tip {
    height: u64,
    hash: Hash,
}
#[derive(Default)]
struct Synchronization {
    sent: BTreeSet<Hash>,
    download: Option<Download>,
    remote_height: u64,
    transaction_cursor: usize,
}
struct Download {
    target: Hash,
    height: u64,
    blocks: Vec<Block>,
    bytes: usize,
}
async fn synchronize(
    address: SocketAddr,
    state: &Compatibility,
    sync: &mut Synchronization,
) -> Result<()> {
    let (status, body) = request(address, "GET", "/tip", None).await?;
    if status != 200 {
        return Err(Error::Invalid("Grupa 1 /tip failed (peer may be offline)"));
    }
    let tip: Tip = serde_json::from_slice(&body)?;
    if tip.height > grupa1::MAX_INTEGER || (tip.height == 0) != (tip.hash == grupa1::GENESIS) {
        return Err(Error::Invalid("invalid Grupa 1 tip"));
    }
    if tip.height < sync.remote_height || tip.hash == grupa1::GENESIS {
        sync.sent.clear();
    }
    sync.remote_height = tip.height;
    {
        let ledger = state.ledger.lock().unwrap();
        if let Some(entry) = ledger.entries.get(&tip.hash) {
            if entry.block.height != tip.height {
                return Err(Error::Invalid("Grupa 1 tip height does not match its hash"));
            }
        } else if sync.download.is_none() {
            sync.download = Some(Download {
                target: tip.hash,
                height: tip.height,
                blocks: vec![],
                bytes: 0,
            });
        }
    }
    // Keep the backward walk across ticks. Only apply once a known parent is
    // reached, oldest first. Bound both each tick's work and unvalidated memory.
    for _ in 0..BATCH {
        let Some(download) = &mut sync.download else {
            break;
        };
        let known = state
            .ledger
            .lock()
            .unwrap()
            .entries
            .contains_key(&download.target);
        if known {
            let download = sync.download.take().unwrap();
            let mut ledger = state.ledger.lock().unwrap();
            if ledger.entries[&download.target].block.height != download.height {
                return Err(Error::Invalid("Grupa 1 ancestor height mismatch"));
            }
            for block in download.blocks.into_iter().rev() {
                ledger.accept(block)?;
            }
            break;
        }
        if download.height == 0 {
            return Err(Error::Invalid(
                "Grupa 1 branch does not reach the zero genesis",
            ));
        }
        let (status, body) = request(
            address,
            "GET",
            &format!("/blocks/{}", download.target),
            None,
        )
        .await?;
        if status != 200 {
            return Err(Error::Invalid("Grupa 1 ancestor fetch failed"));
        }
        let block: Block = serde_json::from_slice(&body)?;
        if block.hash() != download.target || block.height != download.height {
            return Err(Error::Invalid(
                "Grupa 1 block does not match requested hash/height",
            ));
        }
        block.validate()?;
        download.bytes += body.len();
        if download.bytes > 32 * MAX_BODY {
            return Err(Error::Invalid(
                "Grupa 1 catch-up exceeds 32 MiB; retry from a nearer peer",
            ));
        }
        download.target = block.prev_hash;
        download.height -= 1;
        download.blocks.push(block);
    }
    // Publish canonical ancestors in order, then signed pending transactions.
    let (blocks, transactions, pending_len) = {
        let ledger = state.ledger.lock().unwrap();
        let len = ledger.pending.len();
        (
            ledger
                .path()
                .into_iter()
                .filter(|h| !sync.sent.contains(h))
                .take(BATCH)
                .map(|h| (h, ledger.entries[&h].block.clone()))
                .collect::<Vec<_>>(),
            (0..len.min(BATCH))
                .map(|offset| ledger.pending[(sync.transaction_cursor + offset) % len].clone())
                .collect::<Vec<_>>(),
            len,
        )
    };
    for (hash, block) in blocks {
        let body = serde_json::to_vec(&block)?;
        let (status, _) = request(address, "POST", "/blocks", Some(&body)).await?;
        if !matches!(status, 200 | 202) {
            return Err(Error::Invalid("Grupa 1 peer rejected block"));
        }
        sync.sent.insert(hash);
    }
    for tx in transactions {
        let body = serde_json::to_vec(&tx)?;
        let (status, _) = request(address, "POST", "/transactions", Some(&body)).await?;
        // A transaction may have been confirmed remotely since the last /tip.
        if !matches!(status, 202 | 400) {
            return Err(Error::Invalid("Grupa 1 transaction request failed"));
        }
    }
    sync.transaction_cursor = if pending_len == 0 {
        0
    } else {
        (sync.transaction_cursor + BATCH) % pending_len
    };
    Ok(())
}
pub async fn poll(address: SocketAddr, state: Arc<Compatibility>) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sync = Synchronization::default();
    loop {
        interval.tick().await;
        let result = synchronize(address, &state, &mut sync).await;
        // Re-advertise after failure/restart, including a peer that lost its chain.
        if result.is_err() {
            sync.sent.clear();
            sync.download = None;
        }
        state.peers.lock().unwrap().insert(
            address,
            PeerStatus {
                connected: result.is_ok(),
                error: result.err().map(|e| e.to_string()),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        network::peer,
        settings::{CompatibilityProfile, SavedPeer},
    };
    use tokio::{net::TcpListener, sync::mpsc};

    fn fixture(name: &str) -> Block {
        let vectors: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/grupa1-luka.json")).unwrap();
        serde_json::from_value(vectors[name]["block"].clone()).unwrap()
    }
    async fn start(
        state: Arc<Compatibility>,
        peers: Vec<SavedPeer>,
    ) -> (peer::Network, SocketAddr, mpsc::Receiver<peer::Event>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let (events, receiver) = mpsc::channel(32);
        let network = peer::start_compatible(
            address,
            vec![],
            events,
            false,
            1,
            None,
            Some(super::super::Config { state, peers }),
        )
        .await
        .unwrap();
        (network, address, receiver)
    }
    fn saved(address: SocketAddr) -> SavedPeer {
        let mut peer = SavedPeer::new(address, "Luka").unwrap();
        peer.compatibility = CompatibilityProfile::Grupa1Luka;
        peer
    }
    async fn wait_for(mut predicate: impl FnMut() -> bool) {
        timeout(Duration::from_secs(10), async {
            while !predicate() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn routes_validate_direct_payloads_and_coexist_with_native_sessions() {
        let state = Arc::new(Compatibility::default());
        let (network, address, mut events) = start(state.clone(), vec![]).await;
        let (status, body) = request(address, "GET", "/tip", None).await.unwrap();
        assert_eq!(status, 200);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["hash"],
            grupa1::GENESIS.to_string()
        );
        let block = fixture("one");
        let mut invalid = serde_json::to_value(&block).unwrap();
        invalid["timestamp"] = 0.into();
        assert_eq!(
            request(
                address,
                "POST",
                "/blocks",
                Some(&serde_json::to_vec(&invalid).unwrap())
            )
            .await
            .unwrap()
            .0,
            400
        );
        assert_eq!(
            request(
                address,
                "POST",
                "/blocks",
                Some(&serde_json::to_vec(&block).unwrap())
            )
            .await
            .unwrap()
            .0,
            200
        );
        assert_eq!(
            request(address, "GET", &format!("/blocks/{}", block.hash()), None)
                .await
                .unwrap()
                .0,
            200
        );
        assert_eq!(
            request(
                address,
                "GET",
                &format!("/blocks/{}", crate::protocol::Bytes([0x42; 32])),
                None
            )
            .await
            .unwrap()
            .0,
            404
        );
        let tx = fixture("two").transactions[0].clone();
        assert_eq!(
            request(
                address,
                "POST",
                "/transactions",
                Some(&serde_json::to_vec(&tx).unwrap())
            )
            .await
            .unwrap()
            .0,
            202
        );
        let mut invalid = tx.clone();
        invalid.value = "ffffff".into();
        assert_eq!(
            request(
                address,
                "POST",
                "/transactions",
                Some(&serde_json::to_vec(&invalid).unwrap())
            )
            .await
            .unwrap()
            .0,
            400
        );
        // A later child may wait; arrival of the parent applies both in order.
        assert_eq!(
            request(
                address,
                "POST",
                "/blocks",
                Some(&serde_json::to_vec(&fixture("three")).unwrap())
            )
            .await
            .unwrap()
            .0,
            202
        );
        assert_eq!(
            request(
                address,
                "POST",
                "/blocks",
                Some(&serde_json::to_vec(&fixture("two")).unwrap())
            )
            .await
            .unwrap()
            .0,
            200
        );
        assert_eq!(state.ledger.lock().unwrap().head().block.height, 3);
        assert!(
            events.try_recv().is_err(),
            "HTTP must not create a native handshake session"
        );
        let mut native = TcpStream::connect(address).await.unwrap();
        native.write_all(b"{\"type\":\"get_tip\"}\n").await.unwrap();
        let peer::Event::Connected {
            sender,
            close: _close,
            ..
        } = timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap()
        else {
            panic!("native session");
        };
        sender
            .try_send(crate::network::wire::Message::GetTip {})
            .unwrap();
        let mut line = [0; 19];
        native.read_exact(&mut line).await.unwrap();
        assert_eq!(&line, b"{\"type\":\"get_tip\"}\n");
        network.shutdown().await;
        assert_eq!(native.read(&mut [0]).await.unwrap(), 0);
        assert!(TcpStream::connect(address).await.is_err());
    }
    #[tokio::test]
    async fn multiple_http_peers_exchange_blocks_and_signed_transactions_both_ways() {
        let a = Arc::new(Compatibility::default());
        let b = Arc::new(Compatibility::default());
        let c = Arc::new(Compatibility::default());
        let (remote_b, address_b, mut b_events) = start(b.clone(), vec![]).await;
        let (remote_c, address_c, mut c_events) = start(c.clone(), vec![]).await;
        a.ledger.lock().unwrap().accept(fixture("one")).unwrap();
        a.ledger
            .lock()
            .unwrap()
            .admit(fixture("two").transactions[0].clone())
            .unwrap();
        let (local, _, mut a_events) =
            start(a.clone(), vec![saved(address_b), saved(address_c)]).await;
        wait_for(|| {
            b.ledger.lock().unwrap().pending.len() == 1
                && c.ledger.lock().unwrap().pending.len() == 1
        })
        .await;
        b.ledger.lock().unwrap().accept(fixture("two")).unwrap();
        wait_for(|| {
            a.ledger.lock().unwrap().head().block.height == 2
                && c.ledger.lock().unwrap().head().block.height == 2
        })
        .await;
        assert!(
            a_events.try_recv().is_err()
                && b_events.try_recv().is_err()
                && c_events.try_recv().is_err()
        );
        local.set_compatibility(super::super::Config {
            state: a.clone(),
            peers: vec![saved(address_b)],
        });
        wait_for(|| !a.peers.lock().unwrap().contains_key(&address_c)).await;
        local.shutdown().await;
        remote_b.shutdown().await;
        remote_c.shutdown().await;
        assert_eq!(a.ledger.lock().unwrap().head().block.height, 2);
    }
    #[tokio::test]
    async fn malformed_http_lengths_and_json_do_not_change_state() {
        let state = Arc::new(Compatibility::default());
        let (network, address, _) = start(state.clone(), vec![]).await;
        for headers in [
            "Content-Length: 2\r\nContent-Length: 2\r\nContent-Type: application/json\r\n",
            "Content-Length: 1048577\r\nContent-Type: application/json\r\n",
            "Content-Length: 2\r\nContent-Type: text/plain\r\n",
            "Content-Type: application/json\r\n",
        ] {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream
                .write_all(format!("POST /blocks HTTP/1.1\r\n{headers}\r\n{{}}").as_bytes())
                .await
                .unwrap();
            let head = read_head(&mut stream).await.unwrap();
            assert!(head.first.starts_with("HTTP/1.1 400"));
        }
        assert_eq!(state.ledger.lock().unwrap().tip, grupa1::GENESIS);
        network.shutdown().await;
    }
}
